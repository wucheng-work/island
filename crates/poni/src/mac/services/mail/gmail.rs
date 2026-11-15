//! Gmail官方只读API。OAuth使用用户自己的桌面应用、loopback回调和PKCE。

use super::{
    account_identity, mime, store_secret, unix_now, Gate, MailboxCache, Secret, MAX_BODY, MAX_TEXT,
    NETWORK_TIMEOUT,
};
use crate::events::EventSink;
use crate::mail::{MailAttachment, MailEvent, MailMessage, MailPreview};
use crate::services::http::{Client, Response};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use poni_core::mail::MailAccount;
use poni_core::urlenc::encode;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

const API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USER_AGENT: &str = "IslandForMac-Mail/1";
const SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";

pub(super) struct Connection {
    client: Client,
    secret: Secret,
    retry_at: i64,
}

impl Connection {
    pub(super) fn new(secret: Secret) -> Result<Self, String> {
        Ok(Self {
            client: private_client()?,
            secret,
            retry_at: 0,
        })
    }

    fn token(&mut self, account: &MailAccount, gate: &Gate) -> Result<(), String> {
        if !gate.active() {
            return Err("操作已取消".into());
        }
        if unix_now() < self.retry_at {
            return Err("Gmail请求过于频繁，稍后将自动重试".into());
        }
        if !self.secret.access_token.is_empty() && self.secret.expires_at > unix_now() + 60 {
            return Ok(());
        }
        let form = form(&[
            ("client_id", &self.secret.client_id),
            ("client_secret", &self.secret.client_secret),
            ("refresh_token", &self.secret.refresh_token),
            ("grant_type", "refresh_token"),
        ]);
        let response = self
            .client
            .post_form(TOKEN_URL, &form, NETWORK_TIMEOUT)
            .map_err(|_| "无法刷新Gmail授权，请检查网络".to_string())?;
        apply_token(&mut self.secret, response, false)?;
        store_secret(&account.id, &self.secret, gate)
    }

    fn get(&mut self, path: &str, account: &MailAccount, gate: &Gate) -> Result<Response, String> {
        self.token(account, gate)?;
        let bearer = format!("Bearer {}", self.secret.access_token);
        let response = self
            .client
            .get_with_headers(
                &format!("{API}/{path}"),
                &[("Authorization", &bearer)],
                NETWORK_TIMEOUT,
            )
            .map_err(|_| "Gmail连接失败或超时，请检查网络".to_string())?;
        if !gate.active() {
            return Err("操作已取消".into());
        }
        if response.status == 401 {
            self.secret.expires_at = 0;
        }
        if response.status == 429 || response.status == 503 {
            self.retry_at = unix_now() + response.retry_after_s.unwrap_or(60).clamp(1, 3600) as i64;
        }
        Ok(response)
    }

    pub(super) fn sync(
        &mut self,
        cache: &mut MailboxCache,
        account: &MailAccount,
        gate: &Gate,
    ) -> Result<(Vec<String>, u32), String> {
        let mut baseline = !cache.initialized || cache.history_id.is_empty();
        let mut added = HashSet::new();
        let mut changed = HashSet::new();
        let mut cursor = cache.history_id.clone();
        if !baseline {
            let mut page = String::new();
            // 每轮最多十页；用已处理记录的id接续，不能直接跳到服务器最新游标。
            for page_number in 0..10 {
                let mut path = format!(
                    "history?startHistoryId={}&maxResults=100",
                    encode(&cache.history_id)
                );
                if !page.is_empty() {
                    path.push_str(&format!("&pageToken={}", encode(&page)));
                }
                let response = self.get(&path, account, gate)?;
                if response.status == 404 {
                    baseline = true;
                    break;
                }
                let history = json(response)?;
                if let Some(records) = history["history"].as_array() {
                    for record in records {
                        if let Some(id) = record["id"].as_str() {
                            cursor = id.to_string();
                        }
                        for field in [
                            "messagesAdded",
                            "messagesDeleted",
                            "labelsAdded",
                            "labelsRemoved",
                        ] {
                            if let Some(items) = record[field].as_array() {
                                for item in items {
                                    if let Some(id) = item["message"]["id"].as_str() {
                                        changed.insert(id.to_string());
                                        if field == "messagesAdded" {
                                            added.insert(id.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                page = history["nextPageToken"].as_str().unwrap_or("").to_string();
                if page.is_empty() {
                    cursor = history["historyId"]
                        .as_str()
                        .ok_or("Gmail未返回增量游标")?
                        .to_string();
                    break;
                }
                if page_number == 9 && cursor == cache.history_id {
                    return Err("Gmail增量记录无效，请重试收信".into());
                }
            }
        }
        // 最新50封的快照用于移出/删除及恢复后的校对；未改变的头信息复用索引。
        let listing = json(self.get("messages?labelIds=INBOX&maxResults=50", account, gate)?)?;
        let mut messages = Vec::new();
        if let Some(items) = listing["messages"].as_array() {
            for item in items.iter().take(50) {
                let id = item["id"].as_str().ok_or("Gmail邮件标识无效")?;
                if !baseline && !changed.contains(id) {
                    if let Some(old) = cache.messages.iter().find(|message| message.id == id) {
                        messages.push(old.clone());
                        continue;
                    }
                }
                if let Some(message) = self.summary(id, account, gate)? {
                    messages.push(message);
                }
            }
        }
        let unread_data = json(self.get("labels/INBOX?fields=messagesUnread", account, gate)?)?;
        let unread = unread_data["messagesUnread"]
            .as_u64()
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        if baseline {
            // 第一次完整快照建立后才定基线，避免导入历史邮件时弹出提醒。
            let profile = json(self.get("profile", account, gate)?)?;
            verify_address(&profile, account)?;
            cursor = profile["historyId"]
                .as_str()
                .ok_or("Gmail未返回收信基线")?
                .to_string();
            added.clear();
        }
        let new_ids = messages
            .iter()
            .filter(|message| added.contains(&message.id))
            .map(|message| message.id.clone())
            .collect();
        messages.sort_by(|left, right| {
            right
                .received_unix
                .cmp(&left.received_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        cache.messages = messages;
        cache.history_id = cursor;
        cache.initialized = true;
        Ok((new_ids, unread))
    }

    fn summary(
        &mut self,
        id: &str,
        account: &MailAccount,
        gate: &Gate,
    ) -> Result<Option<MailMessage>, String> {
        let path = format!("messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=To&metadataHeaders=Subject&metadataHeaders=Date&metadataHeaders=Content-Type&fields=id,snippet,internalDate,labelIds,payload(headers,mimeType,filename,parts(mimeType,filename))", encode(id));
        let response = self.get(&path, account, gate)?;
        if response.status == 404 {
            return Ok(None);
        }
        let data = json(response)?;
        if !has_label(&data, "INBOX") {
            return Ok(None);
        }
        Ok(Some(summary(&data)?))
    }

    pub(super) fn preview(
        &mut self,
        id: &str,
        account: &MailAccount,
        gate: &Gate,
    ) -> Result<MailPreview, String> {
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("邮件标识无效，请重新收信".into());
        }
        let response = self.get(
            &format!("messages/{}?format=full", encode(id)),
            account,
            gate,
        )?;
        if response.status == 404 {
            return Err("邮件已被删除，请重新收信".into());
        }
        let data = json(response)?;
        let message = summary(&data)?;
        let mut bodies = Bodies::default();
        collect_parts(&data["payload"], &mut bodies, 0)?;
        let text = bodies.plain.or(bodies.html).unwrap_or_default();
        let too_long = text.chars().count() > MAX_TEXT;
        let to = mime::preview(&header_bytes(&data["payload"]), message.clone(), false)?.to;
        Ok(MailPreview {
            message,
            to,
            body: mime::clean(&text, MAX_TEXT),
            attachments: bodies.attachments,
            truncated: bodies.truncated || too_long,
        })
    }
}

#[derive(Default)]
struct Bodies {
    plain: Option<String>,
    html: Option<String>,
    attachments: Vec<MailAttachment>,
    bytes: usize,
    truncated: bool,
}

fn collect_parts(part: &Value, result: &mut Bodies, depth: usize) -> Result<(), String> {
    if depth > 30 {
        result.truncated = true;
        return Ok(());
    }
    let name = part["filename"].as_str().unwrap_or("");
    let disposition = header(part, "Content-Disposition");
    if !name.is_empty() || disposition.to_ascii_lowercase().starts_with("attachment") {
        if result.attachments.len() < 100 {
            result.attachments.push(MailAttachment {
                name: mime::clean(
                    if name.is_empty() {
                        "未命名附件"
                    } else {
                        name
                    },
                    1000,
                ),
                size: part["body"]["size"].as_u64(),
            });
        }
        return Ok(());
    }
    let kind = part["mimeType"].as_str().unwrap_or("");
    if kind.eq_ignore_ascii_case("text/plain") || kind.eq_ignore_ascii_case("text/html") {
        if let Some(encoded) = part["body"]["data"].as_str() {
            let remaining = MAX_BODY.saturating_sub(result.bytes);
            if encoded.len() > remaining.saturating_mul(4) / 3 + 8 {
                result.truncated = true;
            } else {
                let bytes = URL_SAFE_NO_PAD
                    .decode(encoded)
                    .or_else(|_| URL_SAFE.decode(encoded))
                    .map_err(|_| "Gmail正文编码无效，请打开邮箱查看".to_string())?;
                if bytes.len() > remaining {
                    result.truncated = true;
                    return Ok(());
                }
                result.bytes += bytes.len();
                let content_type = header(part, "Content-Type");
                let content_type = if content_type.is_empty() {
                    kind.to_string()
                } else {
                    content_type
                };
                let mut raw = format!("Content-Type: {content_type}\r\n\r\n").into_bytes();
                raw.extend_from_slice(&bytes);
                let preview = mime::preview(&raw, MailMessage::default(), false)?;
                result.truncated |= preview.truncated;
                let text = preview.body;
                if kind.eq_ignore_ascii_case("text/plain") && result.plain.is_none() {
                    result.plain = Some(text);
                } else if kind.eq_ignore_ascii_case("text/html") && result.html.is_none() {
                    result.html = Some(text);
                }
            }
        } else if part["body"]["attachmentId"].is_string() {
            // 大正文由Gmail转成独立部分时不自动请求附件接口。
            result.truncated = true;
        }
    }
    if let Some(parts) = part["parts"].as_array() {
        for child in parts {
            collect_parts(child, result, depth + 1)?;
        }
    }
    Ok(())
}

fn summary(data: &Value) -> Result<MailMessage, String> {
    let id = data["id"].as_str().ok_or("Gmail邮件标识无效")?.to_string();
    let received = data["internalDate"]
        .as_str()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        / 1000;
    let mut message = mime::metadata(
        &header_bytes(&data["payload"]),
        id,
        has_label(data, "UNREAD"),
        received,
    )?;
    message.snippet = mime::clean(
        &mail_parser::decoders::html::html_to_text(data["snippet"].as_str().unwrap_or("")),
        400,
    );
    message.attachments |= has_attachment(&data["payload"], 0);
    Ok(message)
}

fn has_attachment(part: &Value, depth: usize) -> bool {
    if depth > 30 {
        return false;
    }
    if part["filename"]
        .as_str()
        .is_some_and(|name| !name.is_empty())
    {
        return true;
    }
    part["parts"]
        .as_array()
        .is_some_and(|parts| parts.iter().any(|part| has_attachment(part, depth + 1)))
}

fn has_label(data: &Value, label: &str) -> bool {
    data["labelIds"]
        .as_array()
        .is_some_and(|labels| labels.iter().any(|value| value.as_str() == Some(label)))
}

fn header(part: &Value, name: &str) -> String {
    part["headers"]
        .as_array()
        .and_then(|headers| {
            headers.iter().find(|item| {
                item["name"]
                    .as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case(name))
            })
        })
        .and_then(|item| item["value"].as_str())
        .unwrap_or("")
        .to_string()
}

fn header_bytes(part: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    for name in ["From", "To", "Subject", "Date", "Content-Type"] {
        let value = header(part, name);
        if !value.is_empty() {
            bytes.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
    }
    bytes.extend_from_slice(b"\r\n");
    bytes
}

pub(super) fn authorize(
    account: &MailAccount,
    client_id: &str,
    client_secret: &str,
    gate: &Gate,
    sink: &EventSink,
) -> Result<Secret, String> {
    if !client_id.ends_with(".apps.googleusercontent.com")
        || client_id.len() > 1024
        || client_id.chars().any(char::is_whitespace)
        || client_secret.is_empty()
        || client_secret.len() > 4096
    {
        return Err("请填写自己的Google OAuth桌面应用客户端ID和密钥".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|_| "无法建立本机Google授权回调".to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "无法配置本机授权回调".to_string())?;
    let redirect = format!(
        "http://127.0.0.1:{}/oauth2callback",
        listener
            .local_addr()
            .map_err(|_| "无法读取本机授权端口".to_string())?
            .port()
    );
    let state = random_token()?;
    let verifier = random_token()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = form(&[
        ("client_id", client_id),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("login_hint", &account.address),
    ]);
    gate.emit(
        sink,
        MailEvent::AuthorizationUrl(format!(
            "https://accounts.google.com/o/oauth2/v2/auth?{query}"
        )),
    );
    let deadline = Instant::now() + Duration::from_secs(180);
    let code = loop {
        if !gate.active() {
            return Err("Google授权已取消".into());
        }
        if Instant::now() >= deadline {
            return Err("Google授权已超时，请编辑账户后重新授权".into());
        }
        match listener.accept() {
            Ok((mut socket, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                match callback(&mut socket, &state) {
                    Ok(Some(code)) => {
                        respond(&mut socket, true);
                        break code;
                    }
                    Ok(None) => {
                        respond(&mut socket, true);
                        return Err("Google授权已取消".into());
                    }
                    Err(_) => respond(&mut socket, false),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err("本机授权回调失败，请重试".into()),
        }
    };
    if !gate.active() {
        return Err("Google授权已取消".into());
    }
    let client = private_client()?;
    let exchange = form(&[
        ("code", &code),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", &redirect),
        ("code_verifier", &verifier),
        ("grant_type", "authorization_code"),
    ]);
    let response = client
        .post_form(TOKEN_URL, &exchange, NETWORK_TIMEOUT)
        .map_err(|_| "Google授权交换失败，请检查网络后重试".to_string())?;
    let mut secret = Secret {
        identity: account_identity(account),
        address: account.address.clone(),
        provider: format!("{:?}", account.provider),
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        ..Secret::default()
    };
    apply_token(&mut secret, response, true)?;
    if !gate.active() {
        return Err("Google授权已取消".into());
    }
    let bearer = format!("Bearer {}", secret.access_token);
    let profile = json(
        client
            .get_with_headers(
                &format!("{API}/profile"),
                &[("Authorization", &bearer)],
                NETWORK_TIMEOUT,
            )
            .map_err(|_| "无法核验Gmail账户，请检查网络后重试".to_string())?,
    )?;
    verify_address(&profile, account)?;
    Ok(secret)
}

fn verify_address(profile: &Value, account: &MailAccount) -> Result<(), String> {
    if !profile["emailAddress"]
        .as_str()
        .is_some_and(|address| address.eq_ignore_ascii_case(&account.address))
    {
        return Err("授权的Google邮箱与填写地址不一致，请选择对应账户重新授权".into());
    }
    Ok(())
}

fn apply_token(secret: &mut Secret, response: Response, initial: bool) -> Result<(), String> {
    if response.status == 400 || response.status == 401 {
        return Err("Google授权已失效或客户端配置不正确，请重新授权".into());
    }
    let token = json(response)?;
    secret.access_token = token["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("Google未返回访问令牌，请重新授权")?
        .to_string();
    if let Some(refresh) = token["refresh_token"].as_str() {
        secret.refresh_token = refresh.to_string();
    }
    if initial && secret.refresh_token.is_empty() {
        return Err("Google未授予离线访问，请重新授权并允许只读邮箱访问".into());
    }
    secret.expires_at = unix_now()
        + token["expires_in"]
            .as_i64()
            .unwrap_or(3600)
            .clamp(60, 86400);
    Ok(())
}

fn private_client() -> Result<Client, String> {
    Client::private(USER_AGENT).map_err(|_| "无法初始化安全邮件请求".into())
}

fn json(response: Response) -> Result<Value, String> {
    match response.status {
        200..=299 => {
            serde_json::from_str(&response.body).map_err(|_| "Google返回的数据格式无效".into())
        }
        401 => Err("Gmail授权已失效，请重新授权账户".into()),
        403 => Err(
            "Gmail拒绝访问，请确认OAuth应用已启用Gmail API、账户在测试用户中并已允许只读访问"
                .into(),
        ),
        429 | 503 => Err("Gmail暂时限制请求，稍后将自动重试".into()),
        _ => Err(format!(
            "Gmail请求失败（HTTP {}），请稍后重试",
            response.status
        )),
    }
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    #[cfg(target_os = "macos")]
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| "无法生成安全授权随机数".to_string())?;
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
            .ok()
            .map_err(|_| "无法生成安全授权随机数".to_string())?;
    }
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn callback(socket: &mut TcpStream, expected_state: &str) -> Result<Option<String>, ()> {
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;
    let mut request = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while request.len() < 8192 && Instant::now() < deadline {
        let mut buffer = [0u8; 1024];
        let count = socket.read(&mut buffer).map_err(|_| ())?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&request).map_err(|_| ())?;
    let mut line = request.lines().next().ok_or(())?.split_whitespace();
    if line.next() != Some("GET") {
        return Err(());
    }
    let (path, query) = line.next().ok_or(())?.split_once('?').ok_or(())?;
    if path != "/oauth2callback" {
        return Err(());
    }
    let mut state = None;
    let mut code = None;
    let mut denied = false;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or(())?;
        let value = decode(value)?;
        match key {
            "state" => {
                if state.replace(value).is_some() {
                    return Err(());
                }
            }
            "code" => {
                if code.replace(value).is_some() {
                    return Err(());
                }
            }
            "error" => denied = true,
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Err(());
    }
    if denied {
        return Ok(None);
    }
    Ok(Some(
        code.filter(|code| !code.is_empty() && code.len() <= 4096)
            .ok_or(())?,
    ))
}

fn respond(socket: &mut TcpStream, success: bool) {
    let body = if success {
        "授权处理已完成，请返回 Island For Mac 查看结果。可以关闭此页面。"
    } else {
        "此授权回调无效，请返回 Island For Mac 重新授权。"
    };
    let status = if success { "200 OK" } else { "400 Bad Request" };
    let _ = socket.set_write_timeout(Some(Duration::from_secs(2)));
    let _ = write!(
        socket,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn form(values: &[(&str, &str)]) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn decode(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = (bytes[index + 1] as char).to_digit(16).ok_or(())?;
                let low = (bytes[index + 2] as char).to_digit(16).ok_or(())?;
                decoded.push((high * 16 + low) as u8);
                index += 3;
            }
            b'%' => return Err(()),
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

#[cfg(test)]
mod form_tests {
    use super::form;

    #[test]
    fn oauth_form_preserves_empty_values_and_encodes_separators() {
        assert_eq!(
            form(&[("client_secret", ""), ("code", "中文 +&=")]),
            "client_secret=&code=%E4%B8%AD%E6%96%87%20%2B%26%3D"
        );
    }
}
