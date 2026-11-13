//! 独立的多邮箱收信服务。没有启用账户时不创建后台线程。
//!
//! 每个账户只有一个可取消的工作线程；网络等待不阻塞界面或其他账户。
//! 首版每60秒增量检查，未使用IMAP IDLE，也不读取其他应用的通知数据库。

#[path = "mail/gmail.rs"]
mod gmail;
#[path = "mail/imap.rs"]
mod imap;
#[path = "mail/mime.rs"]
mod mime;

use crate::events::{AppEvent, EventSink};
use crate::mail::{MailAction, MailConnection, MailCredentialInput, MailEvent, MailMessage};
use poni_core::mail::{MailAccount, MailProvider, MailSettings};
#[cfg(target_os = "macos")]
use security_framework::base::Error as KeychainError;
#[cfg(target_os = "macos")]
use security_framework::os::macos::keychain::SecKeychain;
#[cfg(target_os = "macos")]
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::net::{Shutdown, TcpStream};
#[cfg(target_os = "macos")]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
#[path = "../../services/mail_credentials.rs"]
mod credentials;
#[cfg(windows)]
use credentials::{delete_generic_password, get_generic_password, set_generic_password, CredentialError};

const KEYCHAIN_SERVICE: &str = "com.oyenza.poni.mail";
// 旧版登录钥匙串的交互开关作用于整个进程，所有邮箱的调用必须串行使用。
#[cfg(target_os = "macos")]
static KEYCHAIN_ACCESS: Mutex<()> = Mutex::new(());
const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: usize = 1024 * 1024;
const MAX_TEXT: usize = 100_000;
#[cfg(target_os = "macos")]
const CREDENTIALS_PAUSED: &str = "请前往高级设置 → 邮件，点击此邮箱的“授权”后继续收信";

#[cfg(windows)]
const CREDENTIALS_PAUSED: &str = "邮箱凭据访问已暂停，请点击刷新重试或编辑账户重新授权";

/// 所有凭据都在系统凭据库中；不实现Debug，避免调试日志打印令牌或密码。
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Secret {
    identity: String,
    address: String,
    provider: String,
    password: String,
    client_id: String,
    client_secret: String,
    refresh_token: String,
    access_token: String,
    expires_at: i64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct MailboxCache {
    identity: String,
    validity: String,
    max_uid: u32,
    history_id: String,
    initialized: bool,
    messages: Vec<MailMessage>,
}

/// 取消标志同时控制事件、磁盘与钥匙串写入。停止时关闭socket唤醒阻塞读取。
struct Gate {
    active: AtomicBool,
    finished: AtomicBool,
    socket: Mutex<Option<TcpStream>>,
    writes: Arc<Mutex<()>>,
    credentials_blocked: Arc<AtomicBool>,
    /// 仅高级设置中的主动授权与保存允许显示系统授权框。
    allow_keychain_ui: AtomicBool,
    /// 保存完成后交给新配置的首次连接，不再立即第二次访问钥匙串。
    saved_secret: Mutex<Option<Secret>>,
}

impl Gate {
    fn new(writes: Arc<Mutex<()>>, credentials_blocked: Arc<AtomicBool>, allow_keychain_ui: bool) -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(true),
            finished: AtomicBool::new(false),
            socket: Mutex::new(None),
            writes,
            credentials_blocked,
            allow_keychain_ui: AtomicBool::new(allow_keychain_ui),
            saved_secret: Mutex::new(None),
        })
    }

    fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.active.store(false, Ordering::Release);
        if let Ok(socket) = self.socket.lock() {
            if let Some(socket) = socket.as_ref() {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
    }

    fn emit(&self, sink: &EventSink, event: MailEvent) {
        if self.active() {
            sink.send(AppEvent::Mail(event));
        }
    }
}

enum Request {
    Refresh,
    Preview(String),
    Stop,
}

struct Worker {
    account: MailAccount,
    sender: mpsc::Sender<Request>,
    gate: Arc<Gate>,
}

struct Operation {
    gate: Arc<Gate>,
    /// 只有保存操作需要等此修订生效后交接凭据，移除操作不使用。
    saved_revision: Option<u64>,
}

pub struct MailService {
    sink: EventSink,
    settings: MailSettings,
    suspended: bool,
    workers: HashMap<String, Worker>,
    operations: HashMap<String, Operation>,
    /// 同一账户历次工作线程和管理操作共享，阻止取消后的后台任务重新写回。
    writes: HashMap<String, Arc<Mutex<()>>>,
    /// 缺少授权后跨锁屏、账户开关和worker重建保留；只有主动授权或保存才解除。
    credentials_blocked: HashMap<String, Arc<AtomicBool>>,
}

impl MailService {
    pub fn new(sink: EventSink) -> Self {
        Self {
            sink,
            settings: MailSettings::default(),
            suspended: false,
            workers: HashMap::new(),
            operations: HashMap::new(),
            writes: HashMap::new(),
            credentials_blocked: HashMap::new(),
        }
    }

    pub fn configure(&mut self, settings: &MailSettings, suspended: bool) {
        let suspended = suspended || !settings.enabled;
        self.settings = settings.clone();
        self.suspended = suspended;
        if suspended {
            for operation in self.operations.values() {
                operation.gate.cancel();
            }
        }
        self.operations.retain(|id, operation| {
            if !operation.gate.finished.load(Ordering::Acquire) {
                return true;
            }
            // 成功保存需等对应配置抵达再消费；取消、失败、禁用的操作直接释放。
            operation.gate.active()
                && operation.saved_revision.is_some()
                && !settings.accounts.iter().any(|account| account.id == *id && !account.enabled && Some(account.revision) == operation.saved_revision)
                && operation.gate.saved_secret.lock().is_ok_and(|secret| secret.is_some())
        });
        let stale: Vec<_> = self
            .workers
            .iter()
            .filter(|(id, worker)| suspended || worker.gate.finished.load(Ordering::Acquire) || !settings.accounts.iter().any(|a| a.id == **id && a.enabled && a == &worker.account))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.stop(&id);
        }
        for account in &settings.accounts {
            if suspended || !account.enabled {
                self.sink.send(AppEvent::Mail(MailEvent::Status {
                    account_id: account.id.clone(),
                    revision: account.revision,
                    state: if suspended && account.enabled { MailConnection::Paused } else { MailConnection::Off },
                }));
                continue;
            }
            let saved = self.operations.get(&account.id).is_some_and(|operation| {
                operation.saved_revision == Some(account.revision) && operation.gate.finished.load(Ordering::Acquire)
            });
            let authorized_secret = if saved {
                let operation = self.operations.remove(&account.id).unwrap();
                let secret = match operation.gate.saved_secret.lock() {
                    Ok(mut secret) => secret.take(),
                    Err(_) => {
                        self.sink.send(AppEvent::Mail(MailEvent::Status {
                            account_id: account.id.clone(),
                            revision: account.revision,
                            state: MailConnection::Error("无法交接邮箱授权，请重新授权".into()),
                        }));
                        continue;
                    }
                };
                secret
            } else {
                None
            };
            if self.workers.contains_key(&account.id) || self.operations.contains_key(&account.id) {
                continue;
            }
            if let Err(error) = account.validate() {
                self.sink.send(AppEvent::Mail(MailEvent::Status {
                    account_id: account.id.clone(),
                    revision: account.revision,
                    state: MailConnection::Error(error),
                }));
                continue;
            }
            let writes = self.writes.entry(account.id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
            let blocked = self.credentials_blocked.entry(account.id.clone()).or_insert_with(|| Arc::new(AtomicBool::new(false))).clone();
            let gate = Gate::new(writes, blocked, false);
            let (sender, receiver) = mpsc::channel();
            let worker_account = account.clone();
            let sink = self.sink.clone();
            let worker_gate = gate.clone();
            std::thread::spawn(move || run_account(worker_account, receiver, worker_gate, sink, authorized_secret));
            self.workers.insert(
                account.id.clone(),
                Worker {
                    account: account.clone(),
                    sender,
                    gate,
                },
            );
        }
    }

    pub fn submit(&mut self, action: MailAction) {
        match action {
            MailAction::Save { mut account, credentials } => {
                account.name = account.name.trim().to_string();
                account.address = account.address.trim().to_string();
                account.host = account.host.trim().to_string();
                if let Err(error) = account.validate() {
                    self.sink.send(AppEvent::Mail(MailEvent::AccountError {
                        account_id: account.id,
                        revision: Some(account.revision),
                        error,
                    }));
                    return;
                }
                self.stop(&account.id);
                self.cancel_operation(&account.id);
                let writes = self.writes.entry(account.id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
                let blocked = self.credentials_blocked.entry(account.id.clone()).or_insert_with(|| Arc::new(AtomicBool::new(false))).clone();
                let gate = Gate::new(writes, blocked, true);
                self.operations.insert(account.id.clone(), Operation { gate: gate.clone(), saved_revision: Some(account.revision) });
                let sink = self.sink.clone();
                std::thread::spawn(move || {
                    #[cfg(windows)]
                    let _mta = crate::services::http::MtaGuard::new();
                    let result = save_account(&account, credentials, &gate, &sink).and_then(|secret| {
                        let mut saved = gate.saved_secret.lock().map_err(|_| "无法交接邮箱授权，请重试".to_string())?;
                        if !gate.active() {
                            return Err("操作已取消".into());
                        }
                        *saved = Some(secret);
                        Ok(())
                    });
                    gate.finished.store(true, Ordering::Release);
                    match result {
                        Ok(()) => sink.send(AppEvent::Mail(MailEvent::AccountSaved(account))),
                        Err(error) => sink.send(AppEvent::Mail(MailEvent::AccountError {
                            account_id: account.id,
                            revision: Some(account.revision),
                            error,
                        })),
                    }
                });
            }
            MailAction::Remove(id) => {
                self.stop(&id);
                self.cancel_operation(&id);
                let writes = self.writes.entry(id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
                let blocked = self.credentials_blocked.entry(id.clone()).or_insert_with(|| Arc::new(AtomicBool::new(false))).clone();
                let gate = Gate::new(writes, blocked, false);
                self.operations.insert(id.clone(), Operation { gate: gate.clone(), saved_revision: None });
                let sink = self.sink.clone();
                std::thread::spawn(move || {
                    #[cfg(windows)]
                    let _mta = crate::services::http::MtaGuard::new();
                    // 只在后台等待；旧worker和旧授权的写入与删除使用同一账户锁。
                    let result = (|| {
                        let _write = gate.writes.lock().map_err(|_| "邮箱数据正被修改，请重试".to_string())?;
                        if !gate.active() {
                            return Err("操作已取消".into());
                        }
                        // 用户明确删除时允许重试；失败后不能因重新配置又自动读凭据。
                        gate.credentials_blocked.store(false, Ordering::Release);
                        remove_account(&id, &gate)
                    })();
                    gate.finished.store(true, Ordering::Release);
                    match result {
                        Ok(()) => sink.send(AppEvent::Mail(MailEvent::AccountRemoved(id))),
                        Err(error) => sink.send(AppEvent::Mail(MailEvent::AccountError {
                            account_id: id,
                            revision: None,
                            error,
                        })),
                    }
                });
            }
            #[cfg(target_os = "macos")]
            MailAction::Authorize(id) => {
                let Some(account) = self.settings.accounts.iter().find(|account| account.id == id).cloned() else { return };
                if self.suspended || !account.enabled {
                    self.sink.send(AppEvent::Mail(MailEvent::Authorized {
                        account_id: id,
                        revision: account.revision,
                        result: Err("请先启用邮箱，并返回桌面或关闭低电量模式后再授权".into()),
                    }));
                    return;
                }
                self.stop(&id);
                self.cancel_operation(&id);
                let writes = self.writes.entry(id.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
                let blocked = self.credentials_blocked.entry(id.clone()).or_insert_with(|| Arc::new(AtomicBool::new(false))).clone();
                let gate = Gate::new(writes, blocked, true);
                let (sender, receiver) = mpsc::channel();
                self.workers.insert(id.clone(), Worker { account: account.clone(), sender, gate: gate.clone() });
                let sink = self.sink.clone();
                std::thread::spawn(move || {
                    let result = load_secret(&account, &gate);
                    // 本次主动读取完成后，后续收信和令牌更新立即恢复静默访问。
                    gate.allow_keychain_ui.store(false, Ordering::Release);
                    match result {
                        Ok(secret) => {
                            sink.send(AppEvent::Mail(MailEvent::Authorized {
                                account_id: id,
                                revision: account.revision,
                                result: Ok(()),
                            }));
                            // 复用本次获准读取的凭据，选择“允许一次”也无需立即第二次访问。
                            run_account(account, receiver, gate, sink, Some(secret));
                        }
                        Err(error) => {
                            gate.finished.store(true, Ordering::Release);
                            sink.send(AppEvent::Mail(MailEvent::Authorized {
                                account_id: id,
                                revision: account.revision,
                                result: Err(error),
                            }));
                        }
                    }
                });
            }
            MailAction::Refresh(id) => {
                #[cfg(windows)]
                for (key, blocked) in &self.credentials_blocked {
                    if id.as_ref().is_none_or(|id| id == key) {
                        blocked.store(false, Ordering::Release);
                    }
                }
                let settings = self.settings.clone();
                self.configure(&settings, self.suspended);
                for (key, worker) in &self.workers {
                    if id.as_ref().is_none_or(|id| id == key) {
                        let _ = worker.sender.send(Request::Refresh);
                    }
                }
            }
            MailAction::Preview { account_id, message_id } => {
                if let Some(worker) = self.workers.get(&account_id) {
                    let _ = worker.sender.send(Request::Preview(message_id));
                } else if let Some(account) = self.settings.accounts.iter().find(|a| a.id == account_id) {
                    self.sink.send(AppEvent::Mail(MailEvent::Preview {
                        account_id,
                        revision: account.revision,
                        message_id,
                        result: Err("请先启用账户并恢复连接，再查看正文".into()),
                    }));
                }
            }
            MailAction::EditAccount(_) | MailAction::CloseEditor | MailAction::ConfirmRemove(_) | MailAction::OpenMailbox(_)
            | MailAction::ClosePreview => {},
            #[cfg(target_os = "macos")]
            MailAction::Translate | MailAction::CancelTranslation => {}
        }
    }

    fn stop(&mut self, id: &str) {
        if let Some(worker) = self.workers.remove(id) {
            worker.gate.cancel();
            let _ = worker.sender.send(Request::Stop);
        }
    }

    fn cancel_operation(&mut self, id: &str) {
        if let Some(operation) = self.operations.remove(id) {
            operation.gate.cancel();
        }
    }
}

impl Drop for MailService {
    fn drop(&mut self) {
        for worker in self.workers.values() {
            worker.gate.cancel();
            let _ = worker.sender.send(Request::Stop);
        }
        for operation in self.operations.values() {
            operation.gate.cancel();
        }
    }
}

fn run_account(account: MailAccount, receiver: mpsc::Receiver<Request>, gate: Arc<Gate>, sink: EventSink, mut authorized_secret: Option<Secret>) {
    #[cfg(windows)]
    let _mta = crate::services::http::MtaGuard::new();
    let mut cache = loop {
        if !gate.active() {
            gate.finished.store(true, Ordering::Release);
            return;
        }
        match load_cache(&account) {
            Ok(cache) => break cache,
            Err(error) => {
                gate.emit(
                    &sink,
                    MailEvent::Status {
                        account_id: account.id.clone(),
                        revision: account.revision,
                        state: MailConnection::Error(error),
                    },
                );
                match wait_request(&receiver, &gate) {
                    Some(Request::Stop) | None => {
                        gate.finished.store(true, Ordering::Release);
                        return;
                    }
                    _ => {}
                }
            }
        }
    };
    let identity = account_identity(&account);
    if cache.identity != identity {
        cache = MailboxCache { identity, ..MailboxCache::default() };
    }
    if !cache.messages.is_empty() {
        gate.emit(
            &sink,
            MailEvent::Cached {
                account_id: account.id.clone(),
                revision: account.revision,
                messages: cache.messages.clone(),
            },
        );
    }
    let mut backend = None;
    let mut synced = false;
    let mut next = Request::Refresh;
    loop {
        if !gate.active() {
            break;
        }
        let result = match next {
            Request::Stop => break,
            Request::Refresh => {
                if !synced {
                    gate.emit(
                        &sink,
                        MailEvent::Status {
                            account_id: account.id.clone(),
                            revision: account.revision,
                            state: MailConnection::Connecting,
                        },
                    );
                }
                if backend.is_none() {
                    backend = match Backend::connect(&account, &gate, authorized_secret.take()) {
                        Ok(backend) => Some(backend),
                        Err(error) => {
                            gate.emit(
                                &sink,
                                MailEvent::Status {
                                    account_id: account.id.clone(),
                                    revision: account.revision,
                                    #[cfg(target_os = "macos")]
                                    state: if gate.credentials_blocked.load(Ordering::Acquire) {
                                        MailConnection::AuthorizationRequired(error)
                                    } else {
                                        MailConnection::Error(error)
                                    },
                                    #[cfg(windows)]
                                    state: MailConnection::Error(error),
                                },
                            );
                            match wait_request(&receiver, &gate) {
                                Some(request) => {
                                    next = request;
                                    continue;
                                }
                                None => break,
                            }
                        }
                    };
                }
                // 只有真实收信成功过的连接才能保持绿色；本地缓存不证明在线。
                if synced {
                    gate.emit(
                        &sink,
                        MailEvent::Status {
                            account_id: account.id.clone(),
                            revision: account.revision,
                            state: MailConnection::Syncing,
                        },
                    );
                }
                // 游标与快照一起提交；保存失败不会先发提醒再在下次重复提醒。
                let mut updated = cache.clone();
                backend.as_mut().unwrap().sync(&mut updated, &account, &gate).and_then(|(new_ids, unread)| {
                    save_cache(&account, &updated, &gate)?;
                    cache = updated;
                    synced = true;
                    gate.emit(
                        &sink,
                        MailEvent::Synced {
                            account_id: account.id.clone(),
                            revision: account.revision,
                            messages: cache.messages.clone(),
                            new_ids,
                            unread,
                        },
                    );
                    gate.emit(
                        &sink,
                        MailEvent::Status {
                            account_id: account.id.clone(),
                            revision: account.revision,
                            state: MailConnection::Ready { unread },
                        },
                    );
                    Ok(())
                })
            }
            Request::Preview(ref id) => {
                let result = match backend.as_mut() {
                    Some(backend) => backend.preview(id, &cache, &account, &gate),
                    None => Err("邮箱暂未连接，请重试收信后再预览".into()),
                };
                let credential_error = if gate.credentials_blocked.load(Ordering::Acquire) {
                    result.as_ref().err().cloned()
                } else {
                    None
                };
                gate.emit(
                    &sink,
                    MailEvent::Preview {
                        account_id: account.id.clone(),
                        revision: account.revision,
                        message_id: id.clone(),
                        result,
                    },
                );
                match credential_error {
                    Some(error) => Err(error),
                    None => Ok(()),
                }
            }
        };
        if let Err(error) = result {
            synced = false;
            // IMAP错误后重连；Gmail保留限流等待及令牌过期状态，避免下一轮丢失。
            if matches!(backend, Some(Backend::Imap(_))) || gate.credentials_blocked.load(Ordering::Acquire) {
                backend = None;
            }
            gate.emit(
                &sink,
                MailEvent::Status {
                    account_id: account.id.clone(),
                    revision: account.revision,
                    #[cfg(target_os = "macos")]
                    state: if gate.credentials_blocked.load(Ordering::Acquire) {
                        MailConnection::AuthorizationRequired(error)
                    } else {
                        MailConnection::Error(error)
                    },
                    #[cfg(windows)]
                    state: MailConnection::Error(error),
                },
            );
        }
        next = match wait_request(&receiver, &gate) {
            Some(request) => request,
            None => break,
        };
    }
    gate.finished.store(true, Ordering::Release);
}

fn wait_request(receiver: &mpsc::Receiver<Request>, gate: &Gate) -> Option<Request> {
    if gate.credentials_blocked.load(Ordering::Acquire) {
        // 授权失败后只等用户操作或停止通知，不再定时唤醒、读取钥匙串。
        return receiver.recv().ok();
    }
    // 仅启用且Mac唤醒的账户每60秒收信；界面操作通过channel立即唤醒。
    match receiver.recv_timeout(CHECK_INTERVAL) {
        Ok(request) => Some(request),
        Err(mpsc::RecvTimeoutError::Timeout) => Some(Request::Refresh),
        Err(mpsc::RecvTimeoutError::Disconnected) => None,
    }
}

enum Backend {
    Imap(imap::Connection),
    Gmail(gmail::Connection),
}

impl Backend {
    fn connect(account: &MailAccount, gate: &Arc<Gate>, authorized_secret: Option<Secret>) -> Result<Self, String> {
        let secret = match authorized_secret {
            Some(secret) => secret,
            None => load_secret(account, gate)?,
        };
        if account.provider == MailProvider::Gmail {
            Ok(Self::Gmail(gmail::Connection::new(secret)?))
        } else {
            Ok(Self::Imap(imap::Connection::new(account, gate, secret)?))
        }
    }

    fn sync(&mut self, cache: &mut MailboxCache, account: &MailAccount, gate: &Gate) -> Result<(Vec<String>, u32), String> {
        match self {
            Self::Imap(c) => c.sync(cache),
            Self::Gmail(c) => c.sync(cache, account, gate),
        }
    }

    fn preview(&mut self, id: &str, cache: &MailboxCache, account: &MailAccount, gate: &Gate) -> Result<crate::mail::MailPreview, String> {
        match self {
            Self::Imap(c) => c.preview(id, cache),
            Self::Gmail(c) => c.preview(id, account, gate),
        }
    }
}

fn save_account(account: &MailAccount, credentials: MailCredentialInput, gate: &Gate, sink: &EventSink) -> Result<Secret, String> {
    let secret = match credentials {
        MailCredentialInput::Keep => {
            // 保存与授权按钮一样是主动操作；读取自行持锁。
            return load_secret(account, gate);
        }
        MailCredentialInput::Password(password) => {
            if account.provider == MailProvider::Gmail {
                return Err("Gmail 使用 Google 授权，不接受邮箱密码".into());
            }
            if password.is_empty() || password.len() > 4096 {
                return Err("请输入邮箱授权码或应用专用密码".into());
            }
            Secret {
                identity: account_identity(account),
                address: account.address.clone(),
                provider: format!("{:?}", account.provider),
                password,
                ..Secret::default()
            }
        }
        MailCredentialInput::Google { client_id, client_secret } => {
            if account.provider != MailProvider::Gmail {
                return Err("此邮箱不使用 Google 授权".into());
            }
            gate.emit(
                sink,
                MailEvent::Status {
                    account_id: account.id.clone(),
                    revision: account.revision,
                    state: MailConnection::Authorizing,
                },
            );
            gmail::authorize(account, client_id.trim(), client_secret.trim(), gate, sink)?
        }
    };
    store_secret(&account.id, &secret, gate)?;
    Ok(secret)
}

/// 沿用主应用直接访问钥匙串；后台调用临时禁止系统交互，只有明确授权可弹框。
#[cfg(target_os = "macos")]
fn keychain_access<T>(gate: &Gate, operation: impl FnOnce() -> Result<T, KeychainError>) -> Result<T, KeychainError> {
    let _access = KEYCHAIN_ACCESS.lock().map_err(|_| KeychainError::from_code(-25291))?;
    if !gate.active() {
        return Err(KeychainError::from_code(-128));
    }
    let _interaction = if gate.allow_keychain_ui.load(Ordering::Acquire) {
        None
    } else {
        Some(SecKeychain::disable_user_interaction()?)
    };
    operation()
}

/// Windows凭据管理器不弹出钥匙串对话框，保留现有系统API访问。
#[cfg(windows)]
fn keychain_access<T>(_gate: &Gate, operation: impl FnOnce() -> Result<T, CredentialError>) -> Result<T, CredentialError> {
    operation()
}

fn load_secret(account: &MailAccount, gate: &Gate) -> Result<Secret, String> {
    let _write = gate.writes.lock().map_err(|_| "邮箱凭据正在被修改，请重试".to_string())?;
    if !gate.active() {
        return Err("操作已取消".into());
    }
    if gate.credentials_blocked.load(Ordering::Acquire) && !gate.allow_keychain_ui.load(Ordering::Acquire) {
        return Err(CREDENTIALS_PAUSED.into());
    }
    let bytes = keychain_access(gate, || get_generic_password(KEYCHAIN_SERVICE, &account.id)).map_err(|error| {
        if !gate.active() {
            return "操作已取消".to_string();
        }
        gate.credentials_blocked.store(true, Ordering::Release);
        #[cfg(target_os = "macos")]
        let message = match error.code() {
            -128 | -25293 => "钥匙串授权未获允许，请在高级设置 → 邮件中点击“授权”重试",
            -25308 => "尚未获得钥匙串访问授权，请在高级设置 → 邮件中点击“授权”",
            -25300 => "尚未保存邮箱凭据，已暂停自动请求；请编辑账户并重新授权",
            _ => "无法读取钥匙串，请在高级设置 → 邮件中点击“授权”重试",
        };
        #[cfg(windows)]
        let message = match error.code() {
            1168 => "尚未保存邮箱凭据，已暂停自动请求；请编辑账户并重新授权",
            _ => "无法读取系统凭据库，已暂停自动请求；请点击刷新重试或重新授权",
        };
        message.to_string()
    })?;
    if !gate.active() {
        return Err("操作已取消".into());
    }
    let secret: Secret = serde_json::from_slice(&bytes).map_err(|_| {
        gate.credentials_blocked.store(true, Ordering::Release);
        "邮箱凭据格式无效，请重新授权".to_string()
    })?;
    if secret.identity != account_identity(account) || !secret.address.eq_ignore_ascii_case(&account.address) || secret.provider != format!("{:?}", account.provider) {
        gate.credentials_blocked.store(true, Ordering::Release);
        return Err("账户地址、服务器或类型已变化，请重新输入凭据".into());
    }
    if account.provider == MailProvider::Gmail && secret.refresh_token.is_empty() {
        gate.credentials_blocked.store(true, Ordering::Release);
        return Err("Gmail尚未授权，请编辑账户并完成Google授权".into());
    }
    if account.provider != MailProvider::Gmail && secret.password.is_empty() {
        gate.credentials_blocked.store(true, Ordering::Release);
        return Err("请编辑账户并填写邮箱授权码或应用专用密码".into());
    }
    gate.credentials_blocked.store(false, Ordering::Release);
    Ok(secret)
}

fn store_secret(id: &str, secret: &Secret, gate: &Gate) -> Result<(), String> {
    let _write = gate.writes.lock().map_err(|_| "邮箱凭据正在被修改，请重试".to_string())?;
    if !gate.active() {
        return Err("操作已取消".into());
    }
    if gate.credentials_blocked.load(Ordering::Acquire) && !gate.allow_keychain_ui.load(Ordering::Acquire) {
        return Err(CREDENTIALS_PAUSED.into());
    }
    let bytes = serde_json::to_vec(secret).map_err(|_| "无法保存邮箱凭据".to_string())?;
    keychain_access(gate, || set_generic_password(KEYCHAIN_SERVICE, id, &bytes)).map_err(|_| {
        if !gate.active() {
            return "操作已取消".to_string();
        }
        gate.credentials_blocked.store(true, Ordering::Release);
        #[cfg(target_os = "macos")]
        let message = "无法保存到系统钥匙串，请在高级设置 → 邮件中重新授权";
        #[cfg(windows)]
        let message = "无法保存到系统凭据库，已暂停自动请求；请点击刷新重试或重新授权";
        message.to_string()
    })?;
    if !gate.active() {
        return Err("操作已取消".into());
    }
    gate.credentials_blocked.store(false, Ordering::Release);
    Ok(())
}

fn cache_path(id: &str) -> PathBuf {
    crate::paths::data_dir().join("mail").join(format!("{id}.json"))
}

fn account_identity(account: &MailAccount) -> String {
    format!("{:?}|{}|{}|{}|INBOX", account.provider, account.address.to_lowercase(), account.host.to_lowercase(), account.port)
}

fn load_cache(account: &MailAccount) -> Result<MailboxCache, String> {
    let path = cache_path(&account.id);
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(cache) => Ok(cache),
            Err(_) => {
                eprintln!("[mail] local mailbox index invalid; rebuilding baseline");
                Ok(MailboxCache::default())
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(MailboxCache::default()),
        Err(_) => Err("无法读取本地邮件索引".into()),
    }
}

fn save_cache(account: &MailAccount, cache: &MailboxCache, gate: &Gate) -> Result<(), String> {
    let _write = gate.writes.lock().map_err(|_| "本地邮件索引正被修改".to_string())?;
    if !gate.active() {
        return Err("操作已取消".into());
    }
    let path = cache_path(&account.id);
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).map_err(|_| "无法创建邮件索引目录".to_string())?;
    let temp = path.with_extension("tmp");
    let bytes = serde_json::to_vec(cache).map_err(|_| "无法生成邮件索引".to_string())?;
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(target_os = "macos")]
    options.mode(0o600);
    let mut file = options.open(&temp).map_err(|_| "无法写入邮件索引".to_string())?;
    file.write_all(&bytes).map_err(|_| "无法写入邮件索引".to_string())?;
    file.sync_all().map_err(|_| "无法保存邮件索引".to_string())?;
    drop(file);
    #[cfg(target_os = "macos")]
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o600)).map_err(|_| "无法设置邮件索引权限".to_string())?;
    fs::rename(temp, path).map_err(|_| "无法更新邮件索引".into())
}

fn remove_account(id: &str, gate: &Gate) -> Result<(), String> {
    match keychain_access(gate, || delete_generic_password(KEYCHAIN_SERVICE, id)) {
        Ok(()) => {}
        Err(error) if matches!(error.code(), -25300 | 1168) => {}
        Err(_) => {
            if !gate.active() {
                return Err("操作已取消".into());
            }
            gate.credentials_blocked.store(true, Ordering::Release);
            #[cfg(target_os = "macos")]
            let message = "无法移除钥匙串中的邮箱凭据，请先在高级设置 → 邮件中授权，再重试移除";
            #[cfg(windows)]
            let message = "无法移除系统凭据库中的邮箱凭据，已暂停自动请求；请重试移除";
            return Err(message.into());
        }
    }
    // 删除已完成，后续索引清理若失败也不能自动读取已删除的凭据。
    gate.credentials_blocked.store(true, Ordering::Release);
    for path in [cache_path(id), cache_path(id).with_extension("tmp")] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("凭据已移除，但本地邮件索引未能清理，请重试".into()),
        }
    }
    Ok(())
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}
