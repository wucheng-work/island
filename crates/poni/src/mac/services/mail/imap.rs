//! QQ、iCloud、Aruba和自定义邮箱。全程TLS，INBOX以只读方式打开。

use super::{mime, Gate, MailboxCache, Secret, MAX_BODY, NETWORK_TIMEOUT};
use crate::mail::MailPreview;
use native_tls::{TlsConnector, TlsStream};
use poni_core::mail::MailAccount;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Instant;

pub(super) struct Connection {
    session: ::imap::Session<TlsStream<TcpStream>>,
}

impl Connection {
    pub(super) fn new(
        account: &MailAccount,
        gate: &Arc<Gate>,
        secret: Secret,
    ) -> Result<Self, String> {
        let addresses = (account.host.as_str(), account.port)
            .to_socket_addrs()
            .map_err(|_| "无法解析邮箱服务器，请检查服务器地址和网络".to_string())?;
        let deadline = Instant::now() + NETWORK_TIMEOUT;
        let mut connected = None;
        for address in addresses.take(8) {
            if !gate.active() {
                return Err("操作已取消".into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            if let Ok(stream) = TcpStream::connect_timeout(&address, remaining) {
                connected = Some(stream);
                break;
            }
        }
        let tcp = connected.ok_or("无法连接邮箱服务器，请检查网络、服务器及端口")?;
        tcp.set_read_timeout(Some(NETWORK_TIMEOUT))
            .map_err(|_| "无法设置邮箱读取超时".to_string())?;
        tcp.set_write_timeout(Some(NETWORK_TIMEOUT))
            .map_err(|_| "无法设置邮箱发送超时".to_string())?;
        let cancel_socket = tcp
            .try_clone()
            .map_err(|_| "无法创建可取消的邮箱连接".to_string())?;
        *gate
            .socket
            .lock()
            .map_err(|_| "邮箱连接已失效".to_string())? = Some(cancel_socket);
        if !gate.active() {
            return Err("操作已取消".into());
        }
        // 沿用系统信任库与主机名校验，不能为自定义服务器跳过证书验证。
        let tls = TlsConnector::new()
            .map_err(|_| "无法初始化安全邮箱连接".to_string())?
            .connect(&account.host, tcp)
            .map_err(|_| "邮箱TLS连接失败，请检查服务器证书、地址及系统时间".to_string())?;
        let mut client = ::imap::Client::new(tls);
        client
            .read_greeting()
            .map_err(|_| "邮箱服务器未返回有效的IMAP响应".to_string())?;
        let session = client
            .login(&account.address, &secret.password)
            .map_err(|_| "邮箱认证失败，请检查IMAP开关、授权码或应用专用密码".to_string())?;
        Ok(Self { session })
    }

    pub(super) fn sync(&mut self, cache: &mut MailboxCache) -> Result<(Vec<String>, u32), String> {
        let mailbox = self
            .session
            .examine("INBOX")
            .map_err(|_| "无法读取收件箱，请检查IMAP是否启用".to_string())?;
        let validity = mailbox
            .uid_validity
            .ok_or("服务器未提供UIDVALIDITY，无法安全建立收信索引")?
            .to_string();
        let baseline = !cache.initialized || cache.validity != validity;
        let old_max = if baseline { 0 } else { cache.max_uid };
        let mut new_ids = Vec::new();
        let mut messages = Vec::new();
        let mut max_uid = old_max;
        if mailbox.exists > 0 {
            // 只取最后200封的有界邮件头；不枚举整个大邮箱，也不下载历史正文。
            let first = mailbox.exists.saturating_sub(199).max(1);
            let sequence = format!("{first}:{}", mailbox.exists);
            let fetched = self
                .session
                .fetch(
                    sequence,
                    "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER]<0.32768>)",
                )
                .map_err(|_| "读取邮件列表失败，请重试收信".to_string())?;
            for item in fetched.iter() {
                if item.message < first || item.message > mailbox.exists {
                    continue;
                }
                let Some(header) = item.header() else {
                    // imap会把服务器主动推送的FLAGS也放入FETCH结果，通知不带邮件头。
                    // 带UID的记录须有同轮完整响应，避免把真正缺失邮件头当通知吞掉。
                    if let Some(uid) = item.uid {
                        let has_header = fetched.iter().any(|response| {
                            response.uid == Some(uid)
                                && response.header().is_some()
                                && response.message >= first
                                && response.message <= mailbox.exists
                        });
                        if !has_header {
                            return Err("服务器未返回邮件头".into());
                        }
                    }
                    continue;
                };
                let uid = item.uid.ok_or("服务器返回的邮件缺少UID")?;
                let id = format!("{validity}:{uid}");
                let unread = !item.flags().contains(&::imap::types::Flag::Seen);
                let mut summary = mime::metadata(
                    header,
                    id.clone(),
                    unread,
                    item.internal_date().map_or(0, |date| date.timestamp()),
                )?;
                if let Some(old) = cache.messages.iter().find(|message| message.id == id) {
                    summary.snippet.clone_from(&old.snippet);
                }
                if !baseline && uid > old_max {
                    new_ids.push(id);
                }
                max_uid = max_uid.max(uid);
                messages.push(summary);
            }
        }
        // imap 2.4.1 把 STATUS 数据放入事件队列，返回的 Mailbox 不包含总未读数。
        // 当前列表已由本轮完整快照校对，先清掉旧事件，避免误用上轮 STATUS。
        while self.session.unsolicited_responses.try_recv().is_ok() {}
        self.session
            .status("INBOX", "(UNSEEN)")
            .map_err(|_| "读取邮箱未读数量失败".to_string())?;
        let mut unread = None;
        for response in self.session.unsolicited_responses.try_iter() {
            let ::imap::types::UnsolicitedResponse::Status {
                mailbox,
                attributes,
            } = response
            else {
                continue;
            };
            if !mailbox.eq_ignore_ascii_case("INBOX") {
                continue;
            }
            for attribute in attributes {
                if let ::imap::types::StatusAttribute::Unseen(count) = attribute {
                    unread = Some(count);
                }
            }
        }
        let unread = unread.ok_or("服务器未返回未读数量")?;
        messages.sort_by(|left, right| {
            right
                .received_unix
                .cmp(&left.received_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        cache.validity = validity;
        cache.max_uid = max_uid;
        cache.initialized = true;
        cache.messages = messages;
        Ok((new_ids, unread))
    }

    pub(super) fn preview(
        &mut self,
        id: &str,
        cache: &MailboxCache,
    ) -> Result<MailPreview, String> {
        let (validity, uid) = id.split_once(':').ok_or("邮件标识无效，请重新收信")?;
        if validity != cache.validity || !cache.initialized {
            return Err("邮箱索引已更新，请重新选择邮件".into());
        }
        let live = self
            .session
            .examine("INBOX")
            .map_err(|_| "无法核验收件箱，请重试收信".to_string())?;
        if live.uid_validity.map(|value| value.to_string()).as_deref() != Some(validity) {
            return Err("邮箱索引已更新，请重新收信后选择邮件".into());
        }
        let uid: u32 = uid.parse().map_err(|_| "邮件标识无效".to_string())?;
        let summary = cache
            .messages
            .iter()
            .find(|message| message.id == id)
            .cloned()
            .ok_or("邮件已不在最近收件列表，请重新收信")?;
        let query = format!("(UID RFC822.SIZE BODY.PEEK[]<0.{MAX_BODY}>)");
        let fetched = self
            .session
            .uid_fetch(uid.to_string(), query)
            .map_err(|_| "邮件预览获取失败，请重试或打开邮箱查看".to_string())?;
        // 同UID的FLAGS通知可能排在正文响应前，预览必须选择实际携带正文的记录。
        let (item, raw) = fetched
            .iter()
            .filter(|item| item.uid == Some(uid))
            .find_map(|item| item.body().map(|raw| (item, raw)))
            .ok_or_else(|| {
                if fetched.iter().any(|item| item.uid == Some(uid)) {
                    "邮箱服务器没有返回邮件正文".to_string()
                } else {
                    "邮件已被移出或删除，请重新收信".to_string()
                }
            })?;
        let truncated =
            item.size.is_some_and(|size| size as usize > raw.len()) || raw.len() >= MAX_BODY;
        mime::preview(raw, summary, truncated)
    }
}
