//! Codex 的本机消息桥：关闭时无监听，开启后阻塞等待本机传输事件。
//! MCP 辅助进程只转交原始工具请求的回答，不创建或接管桌面会话。

use crate::codex::{Kind, Message};
#[cfg(windows)]
use crate::codex_pipe::{Listener as UnixListener, Stream as UnixStream};
use crate::events::{AppEvent, EventSink};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
#[cfg(unix)]
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::Shutdown;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_PACKET: u64 = 96 * 1024;
const MAX_CLIENTS: usize = 8;
const MAX_MCP_CALLS: usize = 16;
const ASK_WAIT: Duration = Duration::from_secs(10 * 60);
const IO_WAIT: Duration = Duration::from_secs(3);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(unix)]
extern "C" {
    fn geteuid() -> u32;
}

#[derive(Clone, Debug)]
pub enum Reply {
    Answer(String),
    Cancel,
}

pub enum BridgeEvent {
    Client(crate::codex_client::ClientEvent),
    Incoming {
        message: Message,
        reply: Option<mpsc::Sender<Reply>>,
    },
    Closed {
        id: String,
        answered: bool,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    op: String,
    message: Message,
}

#[derive(Serialize, Deserialize)]
struct Answer {
    status: String,
    text: String,
}

struct Connections {
    stopping: AtomicBool,
    clients: Mutex<HashMap<u64, UnixStream>>,
}

struct PendingCall {
    cancelled: AtomicBool,
    stream: Mutex<Option<UnixStream>>,
}

impl PendingCall {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(stream) = self.stream.lock().unwrap().as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

pub struct Bridge {
    connections: Arc<Connections>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl Bridge {
    pub fn start(sink: EventSink) -> io::Result<Self> {
        let path = socket_path()?;
        #[cfg(unix)]
        let listener = {
            let directory = path.parent().expect("socket has a parent");
            fs::create_dir_all(directory)?;
            let metadata = fs::symlink_metadata(directory)?;
            if !metadata.is_dir() || metadata.uid() != unsafe { geteuid() } {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Codex桥接目录必须由当前用户拥有，且不能是符号链接",
                ));
            }
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
            if let Ok(metadata) = fs::symlink_metadata(&path) {
                if !metadata.file_type().is_socket() || metadata.uid() != unsafe { geteuid() } {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Codex桥接路径已被其他文件占用",
                    ));
                }
                if UnixStream::connect(&path).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "Codex桥接已在运行",
                    ));
                }
                fs::remove_file(&path)?;
            }

            let listener = UnixListener::bind(&path)?;
            if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            listener
        };
        #[cfg(windows)]
        let listener = UnixListener::bind(&path)?;
        let connections = Arc::new(Connections {
            stopping: AtomicBool::new(false),
            clients: Mutex::new(HashMap::new()),
        });
        let state = connections.clone();
        let handle = thread::Builder::new()
            .name("codex-bridge".into())
            .spawn(move || {
                let mut workers: Vec<JoinHandle<()>> = Vec::new();
                for incoming in listener.incoming() {
                    if state.stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let Ok(stream) = incoming else { break };
                    let token = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                    let Ok(copy) = stream.try_clone() else {
                        continue;
                    };
                    {
                        let mut clients = state.clients.lock().unwrap();
                        if state.stopping.load(Ordering::Acquire) {
                            break;
                        }
                        if clients.len() >= MAX_CLIENTS {
                            continue;
                        }
                        clients.insert(token, copy);
                    }
                    let state_for_worker = state.clone();
                    let sink = sink.clone();
                    match thread::Builder::new()
                        .name("codex-message".into())
                        .spawn(move || {
                            serve(stream, sink);
                            state_for_worker.clients.lock().unwrap().remove(&token);
                        }) {
                        Ok(worker) => workers.push(worker),
                        Err(_) => {
                            state.clients.lock().unwrap().remove(&token);
                        }
                    }
                    workers.retain(|worker| !worker.is_finished());
                }
                for stream in state.clients.lock().unwrap().values() {
                    let _ = stream.shutdown(Shutdown::Both);
                }
                for worker in workers {
                    let _ = worker.join();
                }
            });
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => {
                #[cfg(unix)]
                let _ = fs::remove_file(&path);
                return Err(error);
            }
        };
        Ok(Self {
            connections,
            thread: Some(handle),
            path,
        })
    }

    pub fn stop(&mut self) {
        if self.connections.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        // shutdown同时唤醒读请求和等待中的问题；不用周期性检查开关。
        for stream in self.connections.clients.lock().unwrap().values() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = UnixStream::connect(&self.path);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        #[cfg(unix)]
        let _ = fs::remove_file(&self.path);
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop();
    }
}

fn socket_path() -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        crate::codex_pipe::path()
    }
    #[cfg(unix)]
    {
        Ok(crate::paths::cache_dir().join("codex").join("bridge.sock"))
    }
}

fn unique_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{}-{nanos}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn read_line_limited(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let read = reader.take(MAX_PACKET + 1).read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if read as u64 > MAX_PACKET {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Codex消息超过大小限制",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Codex消息不是UTF-8"))
}

fn write_json(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn validate_packet(packet: &Packet) -> io::Result<()> {
    let message = &packet.message;
    let valid_op = packet.op == "notify" || packet.op == "ask";
    if !valid_op || (packet.op == "ask" && message.kind != Kind::Question) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "不支持的Codex消息类型",
        ));
    }
    if message.id.trim().is_empty()
        || message.id.len() > 512
        || message.thread_id.len() > 512
        || message.turn_id.len() > 512
        || message.cwd.len() > 4096
        || message.title.trim().is_empty()
        || message.title.chars().count() > 120
        || message.text.trim().is_empty()
        || message.text.chars().count() > 16_000
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Codex消息字段缺失或超过长度限制",
        ));
    }
    Ok(())
}

fn serve(mut stream: UnixStream, sink: EventSink) {
    let packet = (|| -> io::Result<Packet> {
        stream.set_read_timeout(Some(IO_WAIT))?;
        stream.set_write_timeout(Some(IO_WAIT))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let line = read_line_limited(&mut reader)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "请求为空"))?;
        let packet: Packet = serde_json::from_str(&line)?;
        validate_packet(&packet)?;
        Ok(packet)
    })();
    let packet = match packet {
        Ok(packet) => packet,
        Err(_) => {
            let _ = write_json(
                &mut stream,
                &Answer {
                    status: "error".into(),
                    text: "无法读取Codex消息，请检查桥接配置与字段长度".into(),
                },
            );
            return;
        }
    };
    if packet.op == "notify" {
        sink.send(AppEvent::Codex(BridgeEvent::Incoming {
            message: packet.message,
            reply: None,
        }));
        let _ = write_json(
            &mut stream,
            &Answer {
                status: "received".into(),
                text: String::new(),
            },
        );
        return;
    }
    let id = packet.message.id.clone();
    let (reply, receiver) = mpsc::channel();
    let disconnected = reply.clone();
    let Ok(mut monitor) = stream.try_clone() else {
        return;
    };
    // 请求已完整读入，后续只等待客户端关闭；shutdown会立刻唤醒此阻塞读。
    if monitor.set_read_timeout(None).is_err() {
        return;
    }
    let watcher = thread::Builder::new()
        .name("codex-disconnect".into())
        .spawn(move || {
            let mut byte = [0];
            let _ = monitor.read(&mut byte);
            let _ = disconnected.send(Reply::Cancel);
        });
    let Ok(watcher) = watcher else { return };
    sink.send(AppEvent::Codex(BridgeEvent::Incoming {
        message: packet.message,
        reply: Some(reply),
    }));
    let response = match receiver.recv_timeout(ASK_WAIT) {
        Ok(Reply::Answer(text)) if !text.trim().is_empty() && text.chars().count() <= 2_000 => {
            Answer {
                status: "answered".into(),
                text,
            }
        }
        Ok(Reply::Answer(_)) => Answer {
            status: "error".into(),
            text: "回答为空或超过长度限制".into(),
        },
        Ok(Reply::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => Answer {
            status: "cancelled".into(),
            text: "问题已取消或连接已断开，未发送回答".into(),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => Answer {
            status: "expired".into(),
            text: "等待回答已超过10分钟，请重新发起问题".into(),
        },
    };
    let answered = write_json(&mut stream, &response).is_ok() && response.status == "answered";
    let _ = stream.shutdown(Shutdown::Both);
    let _ = watcher.join();
    sink.send(AppEvent::Codex(BridgeEvent::Closed { id, answered }));
}

fn call_bridge(packet: Packet, pending: Option<&PendingCall>) -> io::Result<Answer> {
    if pending.is_some_and(|call| call.cancelled.load(Ordering::Acquire)) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "请求已取消"));
    }
    let mut stream = UnixStream::connect(&socket_path()?).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "Island未运行或未开启Codex连接，请打开应用并启用“Codex消息”",
        )
    })?;
    stream.set_write_timeout(Some(IO_WAIT))?;
    stream.set_read_timeout(Some(if packet.op == "ask" {
        ASK_WAIT + IO_WAIT
    } else {
        IO_WAIT
    }))?;
    if let Some(call) = pending {
        let mut active = call.stream.lock().unwrap();
        // 注册与取消共用同一把锁，防止取消发生在连接建立前，留下十分钟等待。
        if call.cancelled.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "请求已取消"));
        }
        *active = Some(stream.try_clone()?);
    }
    write_json(&mut stream, &packet)?;
    let mut reader = BufReader::new(stream);
    let line = read_line_limited(&mut reader)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "Island连接已关闭，未确认回答")
    })?;
    serde_json::from_str(&line).map_err(io::Error::other)
}

/// 在创建窗口、读取日志和单实例锁之前调用，辅助进程不会打开第二个岛。
pub fn cli_entry(args: &[String]) -> Option<i32> {
    if let Some(index) = args.iter().position(|arg| arg == "--codex-hook") {
        // 提醒不可阻断Codex；配置错误和未启用时安静退出，审批流程由Codex处理。
        if let Ok(value) = read_hook(args.get(index + 1)) {
            if let Some(message) = hook_message(&value) {
                let _ = call_bridge(
                    Packet {
                        op: "notify".into(),
                        message,
                    },
                    None,
                );
            }
        }
        let _ = write_json(&mut io::stdout().lock(), &json!({}));
        return Some(0);
    }
    if args.iter().any(|arg| arg == "--codex-mcp") {
        return Some(match mcp_main() {
            Ok(()) => 0,
            Err(_) => 1,
        });
    }
    None
}

fn read_hook(argument: Option<&String>) -> io::Result<Value> {
    if let Some(argument) = argument {
        if argument.len() as u64 > MAX_PACKET {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Hook消息过长"));
        }
        return serde_json::from_str(argument).map_err(io::Error::other);
    }
    let stdin = io::stdin();
    let mut input = String::new();
    stdin
        .lock()
        .take(MAX_PACKET + 1)
        .read_to_string(&mut input)?;
    if input.len() as u64 > MAX_PACKET {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Hook消息过长"));
    }
    serde_json::from_str(&input).map_err(io::Error::other)
}

fn field(value: &Value, names: &[&str]) -> String {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

fn shorten(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(max)
        .collect()
}

fn hook_message(value: &Value) -> Option<Message> {
    let event = field(value, &["hook_event_name", "type"]);
    let (kind, title, text) = match event.as_str() {
        "Stop" | "agent-turn-complete" => (
            Kind::Completed,
            "Codex 已回复",
            field(value, &["last_assistant_message", "last-assistant-message"]),
        ),
        "PermissionRequest" => {
            let tool = field(value, &["tool_name"]);
            let reason = value
                .pointer("/tool_input/description")
                .and_then(Value::as_str)
                .unwrap_or("请回到Codex查看操作详情并决定是否允许");
            (Kind::Approval, "Codex等待审批", format!("{tool}\n{reason}"))
        }
        _ => return None,
    };
    let thread_id = field(value, &["session_id", "thread_id", "thread-id"]);
    let turn_id = field(value, &["turn_id", "turn-id"]);
    let id = if thread_id.is_empty() || turn_id.is_empty() {
        format!("hook-{}", unique_id())
    } else {
        format!("hook-{thread_id}-{turn_id}-{event}")
    };
    let text = if text.trim().is_empty() {
        "打开Codex查看本轮消息".to_string()
    } else {
        shorten(&text, 16_000)
    };
    Some(Message {
        id,
        thread_id,
        turn_id,
        cwd: field(value, &["cwd"]),
        title: title.into(),
        text,
        kind,
    })
}

fn mcp_tools() -> Value {
    let properties = json!({
        "title": {"type":"string", "description":"会话或问题的简短标题，最多120字", "maxLength":120},
        "text": {"type":"string", "description":"需要显示的消息或完整问题，最多16000字", "maxLength":16000},
        "thread_id": {"type":"string", "description":"已知的当前Codex会话ID；不要猜测"},
        "turn_id": {"type":"string", "description":"已知的当前轮次ID；不要猜测"},
        "cwd": {"type":"string", "description":"当前项目目录，帮助用户区分会话"}
    });
    let schema = json!({"type":"object", "properties":properties,"required":["title","text"],"additionalProperties":false});
    json!({"tools":[
        {"name":"island_notify", "description":"在本机Island显示Codex消息摘要。只发送用户需要关注的结果或状态，不替用户审批，也不创建会话。功能未启用时返回错误。", "inputSchema":schema,
         "annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}},
        {"name":"island_ask", "description":"当需要用户回答普通澄清问题时，在本机Island显示问题并等待最多10分钟。用户明确发送的文本作为本工具结果返回当前会话。取消、超时或断线返回错误，不代表同意；不能代替权限审批、密码输入或其他专用确认流程。", "inputSchema":schema,
         "annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}}
    ]})
}

fn tool_packet(name: &str, arguments: &Value) -> Result<Packet, &'static str> {
    let kind = match name {
        "island_notify" => Kind::Completed,
        "island_ask" => Kind::Question,
        _ => return Err("未知的Island工具"),
    };
    let Some(object) = arguments.as_object() else {
        return Err("工具参数必须是对象");
    };
    if object
        .keys()
        .any(|key| !["title", "text", "thread_id", "turn_id", "cwd"].contains(&key.as_str()))
    {
        return Err("工具参数包含未知字段");
    }
    if object.values().any(|value| !value.is_string()) {
        return Err("工具参数必须是文本");
    }
    let mut thread_id = field(arguments, &["thread_id"]);
    if thread_id.is_empty() {
        thread_id = std::env::var("CODEX_THREAD_ID").unwrap_or_default();
    }
    let mut cwd = field(arguments, &["cwd"]);
    if cwd.is_empty() {
        cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    let message = Message {
        id: format!("mcp-{}", unique_id()),
        thread_id,
        turn_id: field(arguments, &["turn_id"]),
        cwd,
        title: field(arguments, &["title"]),
        text: field(arguments, &["text"]),
        kind,
    };
    let packet = Packet {
        op: if kind == Kind::Question {
            "ask".into()
        } else {
            "notify".into()
        },
        message,
    };
    validate_packet(&packet).map_err(|_| "标题或正文为空，或参数超过长度限制")?;
    Ok(packet)
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id,"error":{"code":code,"message":message}})
}

fn mcp_main() -> io::Result<()> {
    let output = Arc::new(Mutex::new(io::stdout()));
    let pending = Arc::new(Mutex::new(HashMap::<String, Arc<PendingCall>>::new()));
    let mut initialized = false;
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    // 所有退出路径都经过下方清理；stdin异常不能遗留仍等待回答的工作线程。
    let result = (|| -> io::Result<()> {
        while let Some(line) = read_line_limited(&mut input)? {
            let value: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => {
                    write_json(
                        &mut *output.lock().unwrap(),
                        &rpc_error(Value::Null, -32700, "JSON解析失败"),
                    )?;
                    continue;
                }
            };
            let id = value.get("id").cloned();
            let method = value
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
                || method.is_empty()
                || id
                    .as_ref()
                    .is_some_and(|id| !id.is_string() && !id.is_number())
            {
                write_json(
                    &mut *output.lock().unwrap(),
                    &rpc_error(Value::Null, -32600, "无效的JSON-RPC请求"),
                )?;
                continue;
            }
            if method == "notifications/cancelled" && id.is_none() {
                if let Some(cancel_id) = value.pointer("/params/requestId") {
                    if let Some(call) = pending.lock().unwrap().get(&cancel_id.to_string()) {
                        call.cancel();
                    }
                }
                continue;
            }
            let Some(id) = id else { continue };
            let response = match method {
                "initialize" if !initialized => {
                    initialized = true;
                    let requested = value
                        .pointer("/params/protocolVersion")
                        .and_then(Value::as_str)
                        .unwrap_or("2024-11-05");
                    let supported = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
                    let version = if supported.contains(&requested) {
                        requested
                    } else {
                        "2024-11-05"
                    };
                    json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"island-codex","version":env!("CARGO_PKG_VERSION")}}})
                }
                "initialize" => rpc_error(id, -32600, "MCP连接已经初始化"),
                "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
                _ if !initialized => rpc_error(id, -32000, "请先初始化MCP连接"),
                "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":mcp_tools()}),
                "tools/call" => {
                    let name = value
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = value
                        .pointer("/params/arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let packet = match tool_packet(name, &arguments) {
                        Ok(packet) => packet,
                        Err(message) => {
                            write_json(
                                &mut *output.lock().unwrap(),
                                &rpc_error(id, -32602, message),
                            )?;
                            continue;
                        }
                    };
                    let key = id.to_string();
                    let call = Arc::new(PendingCall {
                        cancelled: AtomicBool::new(false),
                        stream: Mutex::new(None),
                    });
                    let accepted = {
                        let mut pending = pending.lock().unwrap();
                        if pending.len() >= MAX_MCP_CALLS || pending.contains_key(&key) {
                            false
                        } else {
                            pending.insert(key.clone(), call.clone());
                            true
                        }
                    };
                    if !accepted {
                        write_json(
                            &mut *output.lock().unwrap(),
                            &rpc_error(id, -32000, "已有过多等待请求或请求ID重复"),
                        )?;
                        continue;
                    }
                    let pending_for_worker = pending.clone();
                    let worker_output = output.clone();
                    let worker_key = key.clone();
                    let failed_id = id.clone();
                    let spawn = thread::Builder::new().name("codex-mcp-call".into()).spawn(move || {
                        let result = call_bridge(packet, Some(&call));
                        call.stream.lock().unwrap().take();
                        let result = match result {
                            Ok(answer) => json!({"content":[{"type":"text","text":if answer.text.is_empty() { "消息已交给Island".to_string() } else { answer.text }}],"isError":answer.status != "answered" && answer.status != "received"}),
                            Err(error) => json!({"content":[{"type":"text","text":error.to_string()}],"isError":true}),
                        };
                        // 发出响应前释放ID；客户端收到响应后可立即复用此请求ID。
                        pending_for_worker.lock().unwrap().remove(&worker_key);
                        let _ = write_json(&mut *worker_output.lock().unwrap(), &json!({"jsonrpc":"2.0","id":id,"result":result}));
                    });
                    match spawn {
                        Ok(worker) => workers.push(worker),
                        Err(_) => {
                            pending.lock().unwrap().remove(&key);
                            write_json(
                                &mut *output.lock().unwrap(),
                                &rpc_error(failed_id, -32603, "无法启动本机消息请求"),
                            )?;
                        }
                    }
                    workers.retain(|worker| !worker.is_finished());
                    continue;
                }
                _ => rpc_error(id, -32601, "不支持的MCP方法"),
            };
            write_json(&mut *output.lock().unwrap(), &response)?;
        }
        Ok(())
    })();
    for call in pending.lock().unwrap().values() {
        call.cancel();
    }
    for worker in workers {
        let _ = worker.join();
    }
    result
}
