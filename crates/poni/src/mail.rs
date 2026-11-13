//! 邮件服务、原生账户表单与岛内页面之间的数据契约。

use poni_core::mail::MailAccount;
use poni_core::geom::{palette, Color};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MailMessage {
    pub id: String,
    pub sender: String,
    pub subject: String,
    pub snippet: String,
    pub received_unix: i64,
    pub unread: bool,
    pub attachments: bool,
}

/// 仅保存在当前预览内存中的系统译文，不改写原始邮件或本地邮件索引。
#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailTranslation {
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailAttachment {
    pub name: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailPreview {
    pub message: MailMessage,
    pub to: String,
    pub body: String,
    pub attachments: Vec<MailAttachment>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MailConnection {
    #[default]
    Off,
    Connecting,
    Syncing,
    Ready {
        unread: u32,
    },
    Paused,
    Authorizing,
    #[cfg(target_os = "macos")]
    AuthorizationRequired(String),
    Error(String),
}

impl MailConnection {
    /// 页面和设置共用连接状态，避免同一账户显示不同的含义。
    pub fn label(&self) -> String {
        match self {
            Self::Off => "未启用或尚未连接".into(),
            Self::Connecting => "正在连接邮箱…".into(),
            Self::Syncing => "已连接 · 正在检查新邮件…".into(),
            Self::Ready { unread } => format!("已连接 · {unread} 封未读"),
            Self::Paused => "已暂停，恢复使用后继续收信".into(),
            // 密码账户保存凭据时也使用此状态，不一定会打开浏览器。
            Self::Authorizing => "正在保存或授权账户…".into(),
            #[cfg(target_os = "macos")]
            Self::AuthorizationRequired(message) => message.clone(),
            Self::Error(message) => message.clone(),
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Off => "未连接",
            Self::Connecting => "连接中",
            Self::Syncing => "同步中",
            Self::Ready { .. } => "已连接",
            Self::Paused => "已暂停",
            Self::Authorizing => "处理中",
            #[cfg(target_os = "macos")]
            Self::AuthorizationRequired(_) => "待授权",
            Self::Error(_) => "连接失败",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Ready { .. } => palette::GREEN,
            Self::Connecting | Self::Syncing | Self::Authorizing => palette::ORANGE,
            Self::Error(_) => palette::RED,
            #[cfg(target_os = "macos")]
            Self::AuthorizationRequired(_) => palette::TEXT_DIM,
            Self::Off | Self::Paused => palette::TEXT_DIM,
        }
    }
}

/// 未同步账户的数量未知；已知部分只能表示未读数的下限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnreadSummary {
    Unknown,
    Exact(u64),
    AtLeast(u64),
}

impl UnreadSummary {
    pub fn count(self) -> Option<u64> {
        match self {
            Self::Unknown => None,
            Self::Exact(count) | Self::AtLeast(count) => Some(count),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Unknown => "—".into(),
            Self::Exact(count) => count.to_string(),
            Self::AtLeast(count) => format!("≥{count}"),
        }
    }
}

#[derive(Clone, Default)]
pub enum MailCredentialInput {
    #[default]
    Keep,
    Password(String),
    Google {
        client_id: String,
        client_secret: String,
    },
}

impl std::fmt::Debug for MailCredentialInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Keep => "Keep",
            Self::Password(_) => "Password([redacted])",
            Self::Google { .. } => "Google([redacted])",
        })
    }
}

impl PartialEq for MailCredentialInput {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Keep, Self::Keep) => true,
            (Self::Password(a), Self::Password(b)) => a == b,
            (Self::Google { client_id: a, client_secret: b }, Self::Google { client_id: c, client_secret: d }) => a == c && b == d,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MailAction {
    EditAccount(Option<String>),
    CloseEditor,
    ConfirmRemove(String),
    Save { account: MailAccount, credentials: MailCredentialInput },
    Remove(String),
    #[cfg(target_os = "macos")]
    Authorize(String),
    Refresh(Option<String>),
    Preview { account_id: String, message_id: String },
    ClosePreview,
    #[cfg(target_os = "macos")]
    Translate,
    #[cfg(target_os = "macos")]
    CancelTranslation,
    OpenMailbox(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailEditor {
    New,
    Edit(String),
}

pub enum MailEvent {
    AccountSaved(MailAccount),
    AccountRemoved(String),
    #[cfg(target_os = "macos")]
    Authorized { account_id: String, revision: u64, result: Result<(), String> },
    AccountError { account_id: String, revision: Option<u64>, error: String },
    Status { account_id: String, revision: u64, state: MailConnection },
    Cached { account_id: String, revision: u64, messages: Vec<MailMessage> },
    Synced { account_id: String, revision: u64, messages: Vec<MailMessage>, new_ids: Vec<String>, unread: u32 },
    Preview { account_id: String, revision: u64, message_id: String, result: Result<MailPreview, String> },
    #[cfg(target_os = "macos")]
    Translated { account_id: String, revision: u64, message_id: String, request_id: u64, result: Result<MailTranslation, String> },
    AuthorizationUrl(String),
}

#[derive(Default)]
pub struct MailState {
    /// 设置窗口内临时展开的账户；不持久化，默认全部折叠。
    pub expanded_accounts: HashSet<String>,
    pub editor: Option<MailEditor>,
    pub editor_rect: Option<poni_core::geom::Rect>,
    pub editor_clip: Option<poni_core::geom::Rect>,
    pub messages: HashMap<String, Vec<MailMessage>>,
    pub statuses: HashMap<String, MailConnection>,
    /// 上次成功同步的服务器收件箱未读总数，独立于连接状态和岛内提醒。
    pub unread_counts: HashMap<String, u32>,
    pub selected_account: Option<String>,
    /// 岛内账户下拉菜单的临时状态，不写入账户配置。
    pub account_filter_open: bool,
    pub account_filter_scroll: f32,
    pub selected_message: Option<(String, String)>,
    pub preview: Option<Result<MailPreview, String>>,
    pub preview_loading: bool,
    #[cfg(target_os = "macos")]
    pub translation: Option<Result<MailTranslation, String>>,
    #[cfg(target_os = "macos")]
    pub translation_loading: bool,
    #[cfg(target_os = "macos")]
    pub show_translation: bool,
    pub scroll: f32,
    pub preview_scroll: f32,
    /// 仅用于岛内提示，独立于邮箱服务器的未读标记。
    pub unseen: HashMap<String, u32>,
    pub account_error: Option<String>,
    pub busy_accounts: HashSet<String>,
    /// 当前手动授权操作的版本，用于忽略旧结果，不写入账户配置。
    #[cfg(target_os = "macos")]
    pub authorizing_accounts: HashMap<String, u64>,
}

impl MailState {
    /// 只汇总已启用账户的服务器未读数，不请求网络或混入岛内提醒数量。
    pub fn unread_summary(&self, accounts: &[MailAccount], account_id: Option<&str>) -> UnreadSummary {
        if let Some(id) = account_id {
            return self.unread_counts.get(id).map_or(UnreadSummary::Unknown, |count| UnreadSummary::Exact(u64::from(*count)));
        }
        let mut total = 0_u64;
        let mut known = 0;
        let mut enabled = 0;
        for account in accounts.iter().filter(|account| account.enabled) {
            enabled += 1;
            if let Some(count) = self.unread_counts.get(&account.id) {
                total += u64::from(*count);
                known += 1;
            }
        }
        if enabled > 0 && known == 0 {
            return UnreadSummary::Unknown;
        }
        if known < enabled {
            return UnreadSummary::AtLeast(total);
        }
        UnreadSummary::Exact(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_total_distinguishes_unknown_partial_and_complete_counts() {
        let accounts = vec![
            MailAccount { id: "first".into(), ..MailAccount::default() },
            MailAccount { id: "second".into(), ..MailAccount::default() },
            MailAccount { id: "disabled".into(), enabled: false, ..MailAccount::default() },
        ];
        let mut state = MailState::default();
        state.unread_counts.insert("disabled".into(), 99);
        assert_eq!(state.unread_summary(&accounts, None), UnreadSummary::Unknown);
        state.unread_counts.insert("first".into(), 7);
        assert_eq!(state.unread_summary(&accounts, None), UnreadSummary::AtLeast(7));
        assert_eq!(state.unread_summary(&accounts, None).label(), "≥7");
        state.unread_counts.insert("second".into(), 3);
        assert_eq!(state.unread_summary(&accounts, None), UnreadSummary::Exact(10));
        assert_eq!(state.unread_summary(&accounts, None).count(), Some(10));
    }

    #[test]
    fn unread_selection_preserves_zero_and_does_not_use_notice_counts() {
        let mut state = MailState::default();
        state.unseen.insert("mailbox".into(), 5);
        assert_eq!(state.unread_summary(&[], Some("mailbox")), UnreadSummary::Unknown);
        state.unread_counts.insert("mailbox".into(), 0);
        assert_eq!(state.unread_summary(&[], Some("mailbox")), UnreadSummary::Exact(0));
        assert_eq!(state.unread_summary(&[], None), UnreadSummary::Exact(0));
    }

    #[test]
    fn unread_total_keeps_large_server_counts_without_truncating() {
        let accounts = vec![
            MailAccount { id: "first".into(), ..MailAccount::default() },
            MailAccount { id: "second".into(), ..MailAccount::default() },
        ];
        let mut state = MailState::default();
        state.unread_counts.insert("first".into(), u32::MAX);
        state.unread_counts.insert("second".into(), 1);
        assert_eq!(state.unread_summary(&accounts, None), UnreadSummary::Exact(u64::from(u32::MAX) + 1));
    }
}
