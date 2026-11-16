//! 多邮箱账户的非敏感配置；密码和授权令牌只保存到系统钥匙串。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailProvider {
    #[default]
    Qq,
    Gmail,
    Icloud,
    Aruba,
    Custom,
}

impl MailProvider {
    pub const ALL: [Self; 5] = [
        Self::Qq,
        Self::Gmail,
        Self::Icloud,
        Self::Aruba,
        Self::Custom,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Qq => "QQ 邮箱",
            Self::Gmail => "Gmail",
            Self::Icloud => "iCloud 邮箱",
            Self::Aruba => "Aruba 公司邮箱",
            Self::Custom => "其他 IMAP 邮箱",
        }
    }

    pub fn host(self) -> &'static str {
        match self {
            Self::Qq => "imap.qq.com",
            Self::Icloud => "imap.mail.me.com",
            Self::Aruba => "imaps.aruba.it",
            Self::Gmail | Self::Custom => "",
        }
    }

    pub fn web_url(self) -> &'static str {
        match self {
            Self::Qq => "https://mail.qq.com/",
            Self::Gmail => "https://mail.google.com/mail/u/",
            Self::Icloud => "https://www.icloud.com/mail/",
            Self::Aruba => "https://webmail.aruba.it/",
            Self::Custom => "",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailPrivacy {
    CountOnly,
    #[default]
    SenderAndSubject,
    Preview,
}

impl MailPrivacy {
    pub fn title(self) -> &'static str {
        match self {
            Self::CountOnly => "仅显示新邮件数量",
            Self::SenderAndSubject => "发件人与主题",
            Self::Preview => "发件人、主题与摘要",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailAccount {
    pub id: String,
    pub name: String,
    pub address: String,
    pub provider: MailProvider,
    pub host: String,
    pub port: u16,
    pub enabled: bool,
    pub notify: bool,
    pub sound: bool,
    pub privacy: MailPrivacy,
    /// 编辑或切换账户后拒绝旧连接返回的事件。
    pub revision: u64,
}

impl Default for MailAccount {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            address: String::new(),
            provider: MailProvider::Qq,
            host: MailProvider::Qq.host().into(),
            port: 993,
            enabled: true,
            notify: true,
            sound: true,
            privacy: MailPrivacy::default(),
            revision: 1,
        }
    }
}

impl MailAccount {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 100
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("账户标识无效，请重新添加账户".into());
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 40 {
            return Err("请输入 1–40 字的账户名称".into());
        }
        let Some((local, domain)) = self.address.split_once('@') else {
            return Err("请输入完整邮箱地址".into());
        };
        if local.is_empty()
            || domain.is_empty()
            || domain.contains('@')
            || self.address.len() > 254
            || self.address.chars().any(char::is_whitespace)
            || self.address.chars().any(char::is_control)
        {
            return Err("请输入有效的邮箱地址".into());
        }
        if self.provider != MailProvider::Gmail
            && (self.host.is_empty()
                || self.host.len() > 253
                || !self
                    .host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                || self.port == 0)
        {
            return Err("请填写有效的 IMAP 服务器和端口，连接使用 SSL/TLS".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailSettings {
    /// 总开关：关闭后隐藏账户并停止收信，旧配置保持启用。
    pub enabled: bool,
    /// 邮件提示音总开关，账户声音开关仍各自生效。
    pub sound: bool,
    pub accounts: Vec<MailAccount>,
    /// 专注计时期间只累计新邮件，不自动弹出卡片。
    pub quiet_during_focus: bool,
}

impl Default for MailSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sound: true,
            accounts: Vec::new(),
            quiet_during_focus: false,
        }
    }
}
