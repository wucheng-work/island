//! Codex 消息只驻留内存；每条草稿和等待状态绑定原始请求。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// 请求编号跨关闭、重开接入保持唯一，旧进程结果不能写进新的草稿。
pub fn request_id() -> u64 {
    NEXT_REQUEST.fetch_add(1, Ordering::Relaxed)
}

/// Codex 桌面端只接受由有效本机会话 ID 生成的链接。
pub fn thread_url(thread_id: &str) -> Option<String> {
    crate::codex_client::valid_thread_id(thread_id).then(|| format!("codex://threads/{thread_id}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Completed,
    Failed,
    Approval,
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub turn_id: String,
    #[serde(default)]
    pub cwd: String,
    pub title: String,
    pub text: String,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyState {
    ReadOnly,
    Waiting,
    Sending,
    Replied,
    Expired,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub message: Message,
    /// 收到消息时提取一次，供小岛及列表复用，不在每帧重新解析。
    pub preview: String,
    pub received_unix: i64,
    pub draft: String,
    pub status: ReplyState,
    pub confirmed: bool,
    pub unread: bool,
}

impl Entry {
    pub fn can_chat(&self) -> bool {
        self.status == ReplyState::ReadOnly
            && self.message.kind != Kind::Approval
            && crate::codex_client::valid_thread_id(&self.message.thread_id)
    }
}

#[derive(Default)]
pub struct Conversation {
    pub name: String,
    pub name_loaded: bool,
    pub show_progress: bool,
    pub items: Vec<crate::codex_client::ChatItem>,
    pub draft: String,
    pub loading: Option<u64>,
    pub loading_older: bool,
    pub refresh_after_load: bool,
    pub sending: Option<u64>,
    pub confirmation: Option<String>,
    pub next_cursor: Option<String>,
    pub loaded: bool,
    pub error: String,
    pub send_status: String,
    pub send_uncertain: bool,
    pub scroll: f32,
    pub scroll_to_end: bool,
}

impl Conversation {
    pub fn title(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else if self.name_loaded {
            "未命名会话"
        } else {
            "Codex 会话"
        }
    }
}

#[derive(Default)]
pub struct Inbox {
    pub entries: Vec<Entry>,
    pub selected: Option<String>,
    pub list_scroll: f32,
    pub message_scroll: f32,
    pub bridge_status: String,
    pub connected: bool,
    pub compact: bool,
    pub conversations: HashMap<String, Conversation>,
}

pub struct MessageGroup {
    pub representative: usize,
    pub members: Vec<usize>,
}

impl Inbox {
    /// 普通提醒和审批指向各自的会话，岛内提问仍绑定原请求。
    pub fn notice_thread_id(&self, notice_id: &str) -> Option<&str> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.message.id == notice_id)?;
        if entry.message.kind == Kind::Question {
            return None;
        }
        Some(&entry.message.thread_id)
    }

    pub fn groups(&self) -> Vec<MessageGroup> {
        let mut groups: Vec<MessageGroup> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let group = groups.iter_mut().find(|group| {
                !entry.message.thread_id.is_empty()
                    && self.entries[group.members[0]].message.thread_id == entry.message.thread_id
            });
            if let Some(group) = group {
                group.members.push(index);
            } else {
                groups.push(MessageGroup {
                    representative: index,
                    members: vec![index],
                });
            }
        }
        for group in &mut groups {
            // 同一会话的待答请求仍独立绑定工具连接，不能被新提醒盖住。
            let pending = group.members.iter().find(|&&index| {
                matches!(
                    self.entries[index].status,
                    ReplyState::Waiting | ReplyState::Sending
                )
            });
            if let Some(index) = pending {
                group.representative = *index;
            } else if let Some(index) = group
                .members
                .iter()
                .find(|&&index| self.entries[index].unread && !self.entries[index].can_chat())
            {
                // 旧审批或单条问题尚未查看时，仍需有独立的正文入口。
                group.representative = *index;
            }
        }
        groups
    }
}
