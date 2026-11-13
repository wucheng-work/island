//! Side effects requested by pages; executed by the app after each frame.

use crate::pages::PageKind;

#[derive(Clone, Debug, PartialEq)]
pub enum MediaCmd {
    PlayPause,
    Next,
    Prev,
    /// Seek to absolute position in seconds.
    Seek(f64),
    /// The pointer reached the progress bar or the back button: get ready
    /// (Apple Music's own controls are looked up before the press, not during it).
    Prepare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// Gentle chime (timer finished, reminder).
    Chime,
    /// Windows 通过声音变体选择模块；Mac 操作保留所属模块。
    #[cfg(windows)]
    Tick,
    #[cfg(not(windows))]
    Tick(SoundSource),
    /// Windows 文件暂存区使用“敲打电报”。
    #[cfg(windows)]
    Shelf,
    /// Windows Codex 使用“欢呼”。
    #[cfg(windows)]
    Codex,
    /// Windows 邮件使用“竹子”。
    #[cfg(windows)]
    Mail,
    /// Windows 微信使用“三全音”（Tri-Tone）。
    #[cfg(windows)]
    WeChat,
}

/// 明确声音所属模块，避免依赖当前打开的页面判断是否静音。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundSource {
    Focus,
    Shelf,
    Codex,
}

/// Windows 提示音选择所属模块，与自动提醒的声音开关分别保存。
#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundModule {
    Focus,
    Shelf,
    Codex,
    WeChat,
    Mail,
}

#[cfg(windows)]
impl SoundModule {
    pub fn default_id(self) -> &'static str {
        use poni_core::sound_catalog;

        match self {
            Self::Focus => sound_catalog::DEFAULT_FOCUS,
            Self::Shelf => sound_catalog::DEFAULT_SHELF,
            Self::Codex => sound_catalog::DEFAULT_CODEX,
            Self::WeChat => sound_catalog::DEFAULT_WECHAT,
            Self::Mail => sound_catalog::DEFAULT_MAIL,
        }
    }
}

/// Requests for the Oyenza worker (`services::oyenza`).
#[derive(Clone, Debug, PartialEq)]
pub enum OyenzaCmd {
    /// Ask for a sign-in code to show.
    SignIn,
    /// Stop waiting for the code's approval.
    Cancel,
    /// Sign this PC out (and tell the server).
    Disconnect,
    /// Save a spend typed in the island.
    Note(String),
    /// Take back a spend just saved (its id).
    Undo(String),
    /// The Oyenza tab opened: today's money and the bills, now.
    Refresh,
    /// Today's Try again: the day, now (unless just asked for).
    RefreshDay,
}

/// Requests for Notifications (`services::notifications`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyCmd {
    /// Ask Windows to let the island read notifications (its question).
    Ask,
    /// Look again at what Windows allows: the page opened, and the person
    /// may have changed it in Windows' settings meanwhile.
    Check,
    /// Open Windows' settings for which apps may read notifications.
    OpenWindowsSettings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    #[cfg(windows)]
    ChooseFont,
    #[cfg(windows)]
    ChooseSound(SoundModule),
    #[cfg(any(windows, target_os = "macos"))]
    CodexReply {
        id: String,
        text: String,
    },
    #[cfg(any(windows, target_os = "macos"))]
    CodexCancel(String),
    #[cfg(any(windows, target_os = "macos"))]
    CodexClearFinished,
    #[cfg(any(windows, target_os = "macos"))]
    CodexBack,
    #[cfg(any(windows, target_os = "macos"))]
    CodexSelect { id: String, compact: bool },
    #[cfg(any(windows, target_os = "macos"))]
    CodexHistory { thread_id: String, older: bool },
    #[cfg(any(windows, target_os = "macos"))]
    CodexSend { thread_id: String, text: String },
    #[cfg(any(windows, target_os = "macos"))]
    CodexConfirm(String),
    #[cfg(any(windows, target_os = "macos"))]
    CodexConfirmQueue(String),
    #[cfg(any(windows, target_os = "macos"))]
    CodexOpen(String),
    #[cfg(any(windows, target_os = "macos"))]
    CodexCopy(String),
    /// 附件卡片使用系统快速预览（Mac）或定位文件（Windows），不执行附件。
    #[cfg(any(windows, target_os = "macos"))]
    CodexPreviewAttachment(String),
    #[cfg(any(windows, target_os = "macos"))]
    CodexSetup,
    #[cfg(any(windows, target_os = "macos"))]
    CodexGuide,
    #[cfg(any(windows, target_os = "macos"))]
    Mail(crate::mail::MailAction),
    #[cfg(any(windows, target_os = "macos"))]
    MailSetup,
    #[cfg(target_os = "macos")]
    OpenAppleMail,
    Media(MediaCmd),
    /// 调整当前默认输出设备的系统音量（0..=1）。
    SetOutputVolume(f32),
    /// 切换系统静音状态，不改动设备记住的音量。
    SetOutputMute(bool),
    /// Open a file/folder with its default handler.
    OpenPath(String),
    /// Show a file in Explorer.
    RevealPath(String),
    /// Begin an OLE drag of these paths out of the island.
    DragOut(Vec<String>),
    /// Request a thumbnail for a shelf item (async; arrives as `AppEvent::ShelfThumb`).
    LoadThumb(String),
    /// Persist `AppState::store` soon.
    SaveStore,
    /// 手动重试失败的保存，不重放设置副作用或账户授权。
    RetrySave,
    /// 只在退出保存失败后的明确操作中使用。
    QuitWithoutSaving,
    /// Persist `AppState::settings` now (after a page toggled a setting).
    SaveSettings,
    /// 打开系统颜色选择器，调整当前歌词颜色。
    ChooseLyricsColor,
    /// 为指定模块选择提示音，不影响声音开关。
    #[cfg(target_os = "macos")]
    ChooseAlertTone(poni_core::alert_sounds::SoundModule),
    SearchWeatherCities {
        query: String,
        search_id: u64,
    },
    /// 选择城市后保存到本机，并取消此前尚未完成的定位或天气请求。
    SetWeatherCity { city: String, coordinates: (f64, f64) },
    /// 仅用户点击“定位”时申请系统权限并获取一次位置。
    LocateWeather,
    /// 用户手动重试已保存城市的天气，不请求系统定位。
    RefreshWeather,
    /// Open the Settings window, or bring it forward (the gear, Windows).
    OpenSettings,
    SetPage(PageKind),
    /// Keep the island open even when the cursor leaves.
    SetPinned(bool),
    Collapse,
    Play(Sound),
    /// Open a URL in the default browser.
    OpenUrl(String),
    /// Update to the version the update file offers: install it (Windows,
    /// once its Setup is downloaded), or its release page or DMG.
    Update,
    /// Install it the next time the app starts (Windows).
    InstallOnNextStart,
    /// Settings' Check now: read the update file now, as the menu's Check
    /// for updates… does (`AppState::update.said` takes the answer).
    CheckForUpdates,
    /// Start with Windows on or off (the Settings page's switch).
    SetAutostart(bool),
    /// Request an app's icon for the System page (arrives in `AppState::app_icons`).
    LoadAppIcon(String),
    /// Show over games: find out whether Windows lets Poni read frames
    /// (the answer lands in `AppState::game.access`).
    CheckFps,
    /// Show over games: ask Windows once for the right to read frames.
    AllowFps,
    Oyenza(OyenzaCmd),
    /// Notifications.
    Notify(NotifyCmd),
}
