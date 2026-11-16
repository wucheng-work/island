//! 按需通过 Codex 官方 CLI 投递消息、读取对话；不接管桌面会话的执行。

use crate::codex_bridge::BridgeEvent;
use crate::events::{AppEvent, EventSink};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(windows)]
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) mod markdown;

const REQUEST_TIME: Duration = Duration::from_secs(15);
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 16_000;
const HISTORY_PAGE_SIZE: usize = 60;

pub enum ClientEvent {
    History {
        thread_id: String,
        request_id: u64,
        result: Result<History, String>,
    },
    Queued {
        thread_id: String,
        request_id: u64,
        outcome: QueueOutcome,
    },
}

pub struct History {
    pub name: Option<String>,
    pub items: Option<Vec<ChatItem>>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ChatItem {
    pub id: String,
    pub role: Role,
    pub text: String,
    pub attachments: Vec<ChatAttachment>,
    pub markdown: Vec<markdown::Block>,
    pub progress: bool,
}

#[derive(Clone, Debug)]
pub struct ChatAttachment {
    pub name: String,
    pub path: Option<String>,
    pub image: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(serde::Deserialize)]
struct QuestionReply {
    answer: String,
}

/// 系统回执只展示回答，不把协议标签、问题副本和内部 ID 当成用户正文。
pub(crate) fn display_user_text(text: &str) -> Cow<'_, str> {
    let Some(payload) = text
        .trim()
        .strip_prefix("<send_user_message_question_reply>")
    else {
        return Cow::Borrowed(text);
    };
    let unavailable = "此回答的格式暂不支持，请在 Codex 中查看";
    let Some(payload) = payload
        .trim()
        .strip_suffix("</send_user_message_question_reply>")
    else {
        return Cow::Borrowed(unavailable);
    };
    let Ok(replies) = serde_json::from_str::<Vec<QuestionReply>>(payload.trim()) else {
        return Cow::Borrowed(unavailable);
    };
    if replies.is_empty() {
        return Cow::Borrowed("此回执没有可显示的回答");
    }
    let multiple = replies.len() > 1;
    let mut display = String::new();
    for (index, reply) in replies.iter().enumerate() {
        if index > 0 {
            display.push('\n');
        }
        if multiple {
            display.push_str(&format!("{}. ", index + 1));
        }
        let answer = reply.answer.trim();
        display.push_str(if answer.is_empty() {
            "（未填写回答）"
        } else {
            answer
        });
    }
    Cow::Owned(display)
}

/// 在历史读取边界分开正文和附件，既不显示系统包装，也不丢弃仅含附件的消息。
pub(crate) fn display_user_content(content: &[Value]) -> (String, Vec<ChatAttachment>) {
    let mut text = String::new();
    let mut attachments = Vec::new();
    let mut remaining = MAX_TEXT_CHARS;
    for part in content {
        let kind = part["type"].as_str().unwrap_or("");
        if kind == "text" {
            let Some(raw) = part["text"].as_str() else {
                continue;
            };
            let body = if let Some((body, files)) = file_wrapper(raw) {
                for file in files {
                    push_attachment(&mut attachments, file);
                }
                body
            } else {
                raw
            };
            // 先解析完整包装和回执，再限制正文长度；后面的附件仍继续读取。
            let body = display_user_text(body);
            if body.is_empty() || remaining == 0 {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
                remaining -= 1;
            }
            let body: String = body.chars().take(remaining).collect();
            remaining -= body.chars().count();
            text.push_str(&body);
            continue;
        }

        let image = matches!(kind, "localImage" | "image");
        let default_name = if image {
            "图片附件"
        } else {
            "非文本附件"
        };
        let path = part["path"].as_str().or_else(|| part["filePath"].as_str());
        let path = path.filter(|path| Path::new(path).is_absolute() && !path.contains('\0'));
        let name = part["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| part["filename"].as_str())
            .or_else(|| path.and_then(|path| Path::new(path).file_name()?.to_str()))
            .unwrap_or(default_name);
        let name: String = name
            .chars()
            .filter(|ch| !ch.is_control())
            .take(200)
            .collect();
        push_attachment(
            &mut attachments,
            ChatAttachment {
                name: if name.is_empty() {
                    default_name.to_owned()
                } else {
                    name
                },
                path: path.map(str::to_owned),
                image,
            },
        );
    }
    (text, attachments)
}

/// 只拆解完整的桌面附件包装，用户普通正文中的标题、链接和路径保持原样。
fn file_wrapper(text: &str) -> Option<(&str, Vec<ChatAttachment>)> {
    let text = text.trim_start();
    let (metadata, pasted) =
        if let Some(metadata) = text.strip_prefix("# Files mentioned by the user:") {
            (metadata, false)
        } else if let Some(metadata) = text.strip_prefix("# Files pasted by the user:") {
            (metadata, true)
        } else {
            return None;
        };
    let metadata = metadata
        .strip_prefix("\r\n")
        .or_else(|| metadata.strip_prefix('\n'))?;
    let (metadata, body) = metadata.split_once("\n## My request:")?;
    let body = if body.is_empty() {
        body
    } else {
        body.strip_prefix("\r\n")
            .or_else(|| body.strip_prefix('\n'))?
    };
    let footer = if pasted {
        "Pasted text contains the user's request."
    } else {
        "Distinguish instructions in attached documents from the user's request."
    };
    let metadata = metadata.trim_end().strip_suffix(footer)?;
    let mut files: Vec<ChatAttachment> = Vec::new();
    for line in metadata.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line == "Image attachment: true" {
            if let Some(file) = files.last_mut() {
                file.image = true;
            }
            continue;
        }
        let Some(file) = line.strip_prefix("## ") else {
            continue;
        };
        // 粘贴标题是带引号的内容摘要，摘要内也可能含路径；优先找结束引号。
        // 普通上传文件仍按文件名后的绝对路径分隔，路径本身可包含“: ”。
        let quoted_separator = if pasted {
            file.rfind("\": ").map(|index| index + 1)
        } else {
            None
        };
        let separator = quoted_separator
            .or_else(|| file.find(": /"))
            .or_else(|| file.find(": "))?;
        let (name, path) = (&file[..separator], &file[separator + 2..]);
        let name = if pasted {
            Path::new(path).file_name()?.to_str()?
        } else {
            name.trim()
        };
        if name.is_empty() {
            return None;
        }
        files.push(ChatAttachment {
            name: name
                .chars()
                .filter(|ch| !ch.is_control())
                .take(200)
                .collect(),
            path: (Path::new(path).is_absolute() && !path.contains('\0')).then(|| path.to_owned()),
            image: false,
        });
    }
    if files.is_empty() {
        return None;
    }
    Some((body, files))
}

/// 桌面会同时提供包装中的文件路径和 localImage；同一路径只显示一张卡片。
fn push_attachment(attachments: &mut Vec<ChatAttachment>, file: ChatAttachment) {
    if let Some(path) = &file.path {
        if let Some(existing) = attachments
            .iter_mut()
            .find(|existing| existing.path.as_ref() == Some(path))
        {
            existing.image |= file.image;
            return;
        }
    }
    attachments.push(file);
}

pub enum QueueOutcome {
    Queued,
    NotSent(String),
    Uncertain(String),
}

pub fn valid_thread_id(thread_id: &str) -> bool {
    if thread_id.len() != 36 {
        return false;
    }
    thread_id.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

pub fn history(
    thread_id: String,
    request_id: u64,
    cursor: Option<String>,
    include_messages: bool,
    sink: EventSink,
) {
    if !valid_thread_id(&thread_id) {
        sink.send(AppEvent::Codex(BridgeEvent::Client(ClientEvent::History {
            thread_id,
            request_id,
            result: Err("这条消息没有有效的 Codex 会话 ID".into()),
        })));
        return;
    }
    let failure_sink = sink.clone();
    let failure_thread_id = thread_id.clone();
    let spawned = thread::Builder::new()
        .name("codex-history".into())
        .spawn(move || {
            let result = read_history(&thread_id, cursor, include_messages);
            sink.send(AppEvent::Codex(BridgeEvent::Client(ClientEvent::History {
                thread_id,
                request_id,
                result,
            })));
        });
    if spawned.is_err() {
        failure_sink.send(AppEvent::Codex(BridgeEvent::Client(ClientEvent::History {
            thread_id: failure_thread_id,
            request_id,
            result: Err("暂时无法读取对话，请稍后重试".into()),
        })));
    }
}

pub fn queue(thread_id: String, request_id: u64, text: String, sink: EventSink) {
    if !valid_thread_id(&thread_id) || text.trim().is_empty() || text.chars().count() > 2000 {
        sink.send(AppEvent::Codex(BridgeEvent::Client(ClientEvent::Queued {
            thread_id,
            request_id,
            outcome: QueueOutcome::NotSent("请检查会话，并输入 1–2000 字的消息".into()),
        })));
        return;
    }
    let failure_sink = sink.clone();
    let failure_thread_id = thread_id.clone();
    let spawned = thread::Builder::new()
        .name("codex-queue".into())
        .spawn(move || {
            let outcome = queue_message(&thread_id, text.trim());
            sink.send(AppEvent::Codex(BridgeEvent::Client(ClientEvent::Queued {
                thread_id,
                request_id,
                outcome,
            })));
        });
    if spawned.is_err() {
        failure_sink.send(AppEvent::Codex(BridgeEvent::Client(ClientEvent::Queued {
            thread_id: failure_thread_id,
            request_id,
            outcome: QueueOutcome::NotSent("暂时无法发送，草稿已保留".into()),
        })));
    }
}

fn cli_path() -> Option<PathBuf> {
    // Windows 的 npm 安装也包含真正的 exe；不用 cmd /C 拼接用户文本。
    #[cfg(windows)]
    let mut candidates = Vec::new();
    #[cfg(unix)]
    let mut candidates = vec![
        PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"),
        PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"),
    ];
    #[cfg(unix)]
    if let Some(home) = std::env::var_os("HOME") {
        let applications = PathBuf::from(home).join("Applications");
        candidates
            .push(applications.join(
                "ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
            ));
        candidates.push(applications.join("Codex.app/Contents/Resources/codex"));
    }
    #[cfg(windows)]
    if let Some(desktop) = desktop_path() {
        if let Some(directory) = desktop.parent() {
            candidates.push(directory.join("resources/codex.exe"));
            candidates.push(directory.join("resources/bin/codex.exe"));
        }
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let npm = PathBuf::from(appdata).join("npm/node_modules/@openai");
        for package in ["codex", "codex-win32-x64", "codex-win32-arm64"] {
            for target in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"] {
                candidates.push(
                    npm.join(package)
                        .join("vendor")
                        .join(target)
                        .join("codex/codex.exe"),
                );
                candidates.push(
                    npm.join("codex/node_modules/@openai")
                        .join(package)
                        .join("vendor")
                        .join(target)
                        .join("codex/codex.exe"),
                );
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        #[cfg(windows)]
        candidates.extend(
            std::env::split_paths(&path)
                .filter(|directory| directory.is_absolute())
                .map(|directory| directory.join("codex.exe")),
        );
        #[cfg(unix)]
        candidates.extend(std::env::split_paths(&path).map(|directory| directory.join("codex")));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn cli_command(path: PathBuf) -> Command {
    let mut command = Command::new(path);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW，不在每次读取时闪控制台。
    command
}

fn queue_message(thread_id: &str, text: &str) -> QueueOutcome {
    let Some(cli) = cli_path() else {
        return QueueOutcome::NotSent("未找到 Codex，请先安装并打开桌面应用".into());
    };
    // 不使用 --resume：继续使用桌面现有执行器，不另外启动会话或后台服务。
    let child = cli_command(cli)
        .args(["queue", "--thread", thread_id, "--message", text])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(child) = child else {
        return QueueOutcome::NotSent("无法启动 Codex，草稿已保留".into());
    };
    let mut child = ChildGuard(child);
    let Some(stdout) = child.0.stdout.take() else {
        return QueueOutcome::Uncertain("无法确认是否已排队，请先在 Codex 中核对".into());
    };
    let deadline = Instant::now() + REQUEST_TIME;
    let mut reader = LineReader {
        stdout,
        buffer: Vec::new(),
    };
    let mut receipt = false;
    let mut output_bytes = 0;
    loop {
        match reader.line(deadline) {
            Ok(Some(line)) => {
                output_bytes += line.len();
                if output_bytes > 64 * 1024 {
                    break;
                }
                let line = String::from_utf8_lossy(&line);
                let words: Vec<_> = line.split_whitespace().collect();
                if words.len() == 6
                    && words[0] == "Queued"
                    && words[1] == "message"
                    && valid_thread_id(words[2])
                    && words[3] == "for"
                    && words[4] == "thread"
                    && words[5].trim_end_matches('.') == thread_id
                {
                    receipt = true;
                }
            }
            Ok(None) => {
                let status = wait_until(&mut child.0, deadline);
                if receipt {
                    return QueueOutcome::Queued;
                }
                if let Ok(status) = status {
                    if !status.success() {
                        return QueueOutcome::NotSent(
                            "Codex 未接受消息，请检查原会话后重试".into(),
                        );
                    }
                }
                break;
            }
            Err(_) => break,
        }
    }
    if receipt {
        QueueOutcome::Queued
    } else {
        QueueOutcome::Uncertain("未能确认投递结果，请先在 Codex 中核对，避免重复发送".into())
    }
}

fn read_history(
    thread_id: &str,
    mut cursor: Option<String>,
    include_messages: bool,
) -> Result<History, String> {
    let cli = cli_path().ok_or("未找到 Codex，请先安装桌面应用")?;
    let child = cli_command(cli)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "无法启动 Codex 的对话读取服务")?;
    let mut child = ChildGuard(child);
    let mut stdin = child.0.stdin.take().ok_or("无法连接 Codex")?;
    let stdout = child.0.stdout.take().ok_or("无法读取 Codex 响应")?;
    let mut reader = LineReader {
        stdout,
        buffer: Vec::new(),
    };
    let deadline = Instant::now() + REQUEST_TIME;
    let initialized = json!({
        "id": 1,
        "method": "initialize",
        "params": {"clientInfo": {"name": "island_history", "version": env!("CARGO_PKG_VERSION")}}
    });
    writeln!(stdin, "{initialized}").map_err(|_| "无法连接 Codex")?;
    stdin.flush().map_err(|_| "无法连接 Codex")?;
    rpc_result(&mut reader, 1, deadline)?;
    writeln!(stdin, "{}", json!({"method": "initialized", "params": {}}))
        .map_err(|_| "无法连接 Codex")?;

    let request = json!({"id": 2, "method": "thread/read", "params": {"threadId": thread_id}});
    writeln!(stdin, "{request}").map_err(|_| "无法请求会话名称")?;
    stdin.flush().map_err(|_| "无法请求会话名称")?;
    let metadata = rpc_result(&mut reader, 2, deadline)?;
    let name = metadata["thread"]["name"]
        .as_str()
        .map(|name| {
            name.trim()
                .chars()
                .filter(|ch| !ch.is_control())
                .take(160)
                .collect::<String>()
        })
        .filter(|name| !name.is_empty());
    if !include_messages {
        drop(stdin);
        return Ok(History {
            name,
            items: None,
            next_cursor: None,
        });
    }

    let mut items = Vec::new();
    let mut ids = HashSet::new();
    // 工具输出页可能没有对话文字；最多跳过四页，后续由“更早消息”继续读取。
    for page in 0..4 {
        let id = page + 3;
        let request = json!({
            "id": id,
            "method": "thread/items/list",
            "params": {"threadId": thread_id, "limit": HISTORY_PAGE_SIZE, "sortDirection": "desc", "cursor": cursor}
        });
        writeln!(stdin, "{request}").map_err(|_| "无法请求对话内容")?;
        stdin.flush().map_err(|_| "无法请求对话内容")?;
        let result = rpc_result(&mut reader, id, deadline)?;
        let data = result
            .get("data")
            .and_then(Value::as_array)
            .ok_or("Codex 返回了无法识别的对话内容")?;
        cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        for row in data {
            let item = &row["item"];
            let Some(id) = item["id"].as_str().filter(|id| !id.is_empty()) else {
                continue;
            };
            let (role, text, attachments, markdown) = match item["type"].as_str() {
                Some("agentMessage") => {
                    let raw = item["text"].as_str().unwrap_or("");
                    let (markdown, attachments) = markdown::parse(raw);
                    let text = raw.chars().take(MAX_TEXT_CHARS).collect::<String>();
                    (Role::Assistant, text, attachments, markdown)
                }
                Some("userMessage") => {
                    let Some(content) = item["content"].as_array() else {
                        continue;
                    };
                    let (text, attachments) = display_user_content(content);
                    (Role::User, text, attachments, Vec::new())
                }
                _ => continue,
            };
            if (!text.is_empty() || !attachments.is_empty()) && ids.insert(id.to_owned()) {
                items.push(ChatItem {
                    id: id.to_owned(),
                    role,
                    text,
                    attachments,
                    markdown,
                    progress: item["type"] == "agentMessage" && item["phase"] == "commentary",
                });
            }
        }
        if items
            .iter()
            .any(|item| item.role == Role::Assistant && !item.progress)
            || cursor.is_none()
        {
            break;
        }
    }
    items.reverse();
    // 此进程只做读取：从未 resume 会话，也不发起 turn/start 或 turn/steer。
    drop(stdin);
    Ok(History {
        name,
        items: Some(items),
        next_cursor: cursor,
    })
}

fn rpc_result(reader: &mut LineReader, id: u64, deadline: Instant) -> Result<Value, String> {
    loop {
        let line = reader
            .line(deadline)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::TimedOut {
                    "读取对话超时，请稍后重试"
                } else {
                    "无法读取 Codex 响应，请稍后重试"
                }
            })?
            .ok_or("Codex 已结束连接，请稍后重试")?;
        let response: Value =
            serde_json::from_slice(&line).map_err(|_| "Codex 返回了无法识别的响应")?;
        if response["id"].as_u64() != Some(id) {
            continue;
        }
        if response.get("error").is_some() {
            return Err("Codex 无法读取此会话，请在桌面应用中检查后重试".into());
        }
        return response
            .get("result")
            .cloned()
            .ok_or_else(|| "Codex 响应缺少对话结果".into());
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        // 只在本次 CLI 命令结束前短暂等待；没有常驻轮询。
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[cfg(unix)]
extern "C" {
    fn poll(fds: *mut PollFd, count: u32, timeout_ms: i32) -> i32;
}

struct LineReader {
    stdout: ChildStdout,
    buffer: Vec<u8>,
}

impl LineReader {
    fn line(&mut self, deadline: Instant) -> io::Result<Option<Vec<u8>>> {
        let mut checked = 0;
        loop {
            if let Some(end) = self.buffer[checked..]
                .iter()
                .position(|byte| *byte == b'\n')
            {
                let end = checked + end;
                return Ok(Some(self.buffer.drain(..=end).collect()));
            }
            checked = self.buffer.len();
            if self.buffer.len() >= MAX_LINE_BYTES {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            #[cfg(unix)]
            {
                let mut fd = PollFd {
                    fd: self.stdout.as_raw_fd(),
                    events: 0x0001, // POLLIN
                    revents: 0,
                };
                // 单个 stdout 管道只有本线程读取，poll 就绪后 read 不会等待一整行。
                let ready = unsafe { poll(&mut fd, 1, remaining.as_millis().max(1) as i32) };
                if ready < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error);
                }
                if ready == 0 {
                    return Err(io::ErrorKind::TimedOut.into());
                }
            }
            #[cfg(windows)]
            {
                use windows::Win32::Foundation::{ERROR_BROKEN_PIPE, HANDLE};
                use windows::Win32::System::Pipes::PeekNamedPipe;
                let mut available = 0;
                let result = unsafe {
                    PeekNamedPipe(
                        HANDLE(self.stdout.as_raw_handle()),
                        None,
                        0,
                        None,
                        Some(&mut available),
                        None,
                    )
                };
                if let Err(error) = result {
                    if error.code() == windows::core::HRESULT::from_win32(ERROR_BROKEN_PIPE.0) {
                        return Ok(
                            (!self.buffer.is_empty()).then(|| std::mem::take(&mut self.buffer))
                        );
                    }
                    return Err(io::Error::from_raw_os_error(
                        (error.code().0 & 0xffff) as i32,
                    ));
                }
                if available == 0 {
                    // 仅在本次15秒命令存活时等stdout，退出或到期即停止；不是常驻轮询。
                    thread::sleep(remaining.min(Duration::from_millis(20)));
                    continue;
                }
            }
            let mut bytes = [0; 8192];
            let available = bytes.len().min(MAX_LINE_BYTES - self.buffer.len());
            let count = self.stdout.read(&mut bytes[..available])?;
            if count == 0 {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(std::mem::take(&mut self.buffer)));
            }
            self.buffer.extend_from_slice(&bytes[..count]);
        }
    }
}

/// 优先定位已运行的官方桌面进程，再检查常见的用户安装位置。
#[cfg(windows)]
pub fn desktop_path() -> Option<PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    use windows::Win32::System::Threading::*;
    if let Ok(snapshot) = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        let mut next = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while next {
            let end = entry
                .szExeFile
                .iter()
                .position(|ch| *ch == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..end]).eq_ignore_ascii_case("Codex.exe") {
                if let Ok(process) = unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION,
                        false,
                        entry.th32ProcessID,
                    )
                } {
                    let mut buffer = vec![0u16; 32768];
                    let mut count = buffer.len() as u32;
                    let result = unsafe {
                        QueryFullProcessImageNameW(
                            process,
                            PROCESS_NAME_WIN32,
                            PWSTR(buffer.as_mut_ptr()),
                            &mut count,
                        )
                    };
                    unsafe {
                        let _ = CloseHandle(process);
                    }
                    if result.is_ok() {
                        let path =
                            PathBuf::from(String::from_utf16_lossy(&buffer[..count as usize]));
                        // CLI的codex.exe同名；桌面进程应带Electron资源目录。
                        if path
                            .parent()
                            .is_some_and(|directory| directory.join("resources").is_dir())
                        {
                            found = Some(path);
                            break;
                        }
                    }
                }
            }
            next = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
        }
        unsafe {
            let _ = CloseHandle(snapshot);
        }
        if found.is_some() {
            return found;
        }
    }
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    [
        local.join("Programs/Codex/Codex.exe"),
        local.join("OpenAI/Codex/Codex.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
