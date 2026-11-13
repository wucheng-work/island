//! 邮件只转换成纯文本，不呈现HTML，也不请求图片、链接或附件。

use super::MAX_TEXT;
use crate::mail::{MailAttachment, MailMessage, MailPreview};
use mail_parser::{Address, MessageParser, MimeHeaders};

pub(super) fn metadata(raw: &[u8], id: String, unread: bool, received: i64) -> Result<MailMessage, String> {
    let message = MessageParser::default().parse_headers(raw).ok_or("无法解析邮件头")?;
    Ok(MailMessage {
        id,
        sender: addresses(message.from()),
        subject: clean(message.subject().unwrap_or("（无主题）"), 1000),
        snippet: String::new(),
        received_unix: if received > 0 { received } else { message.date().map_or(0, |date| date.to_timestamp()) },
        unread,
        attachments: message
            .content_type()
            .is_some_and(|content| content.ctype().eq_ignore_ascii_case("multipart") && content.subtype().is_some_and(|subtype| subtype.eq_ignore_ascii_case("mixed"))),
    })
}

pub(super) fn preview(raw: &[u8], mut summary: MailMessage, truncated: bool) -> Result<MailPreview, String> {
    let message = MessageParser::default().parse(raw).ok_or("无法解析邮件正文，请打开邮箱查看")?;
    let text = message.body_text(0).unwrap_or_default();
    let too_long = text.chars().count() > MAX_TEXT;
    let attachments: Vec<_> = message
        .attachments()
        .take(100)
        .map(|part| MailAttachment {
            name: clean(part.attachment_name().unwrap_or("未命名附件"), 1000),
            size: if truncated { None } else { Some(part.body.len() as u64) },
        })
        .collect();
    summary.attachments |= !attachments.is_empty();
    Ok(MailPreview {
        message: summary,
        to: addresses(message.to()),
        body: clean(&text, MAX_TEXT),
        attachments,
        truncated: truncated || too_long,
    })
}

pub(super) fn clean(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(limit)
        .collect::<String>()
        .trim()
        .to_string()
}

fn addresses(value: Option<&Address<'_>>) -> String {
    let Some(addresses) = value else { return String::new() };
    let mut formatted = Vec::new();
    for address in addresses.iter().take(50) {
        let name = address.name.as_deref().unwrap_or("");
        let email = address.address.as_deref().unwrap_or("");
        let value = if name.is_empty() {
            email.to_string()
        } else if email.is_empty() {
            name.to_string()
        } else {
            format!("{name} <{email}>")
        };
        formatted.push(clean(&value, 1000));
    }
    formatted.join(", ")
}
