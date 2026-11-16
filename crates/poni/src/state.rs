//! All state shared by pages, activities and the app loop.
//!
//! Owned by the UI thread. Services never touch it directly; they send
//! [`crate::events::AppEvent`]s that the app applies here.

use crate::commands::{Command, MediaCmd, Sound, SoundSource};
use crate::gfx::Image;
use crate::icons::Icon;
use crate::pages::PageKind;
use poni_core::datetime::LocalDateTime;
use poni_core::geom::{palette, Color};
use poni_core::lrc::Lyrics;
use poni_core::oyenza::{Activity, Bill, DayPick, Saved, Seen};
use poni_core::pomodoro::Pomodoro;
use poni_core::settings::Settings;
use poni_core::store::Store;
use poni_core::timeline::Timeline;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct MediaSession {
    /// AppUserModelId of the player (e.g. `Spotify.exe`, `Chrome`).
    pub app_id: String,
    /// Friendly player name ("Spotify", "Chrome", …).
    pub app_name: String,
    /// 播放器提供的原始歌曲标识，未提供时为空。
    pub content_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: Option<Image>,
    /// Accent derived from the art (for progress bars / the activity glow).
    pub art_color: Option<Color>,
    pub timeline: Timeline,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_play_pause: bool,
    pub can_seek: bool,
    /// Identity of the track: metadata and the player's content id when known.
    pub track_key: String,
}

impl MediaSession {
    pub fn make_track_key(title: &str, artist: &str, album: &str) -> String {
        format!("{title}\u{1}{artist}\u{1}{album}")
    }
    pub fn is_playing(&self) -> bool {
        self.timeline.playing
    }
    /// Whole seconds used by the lyrics lookup; zero means unknown.
    pub fn duration_secs(&self) -> u32 {
        let duration = self.timeline.duration_s;
        if duration.is_finite() && duration > 0.5 {
            duration.round().min(u32::MAX as f64) as u32
        } else {
            0
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LyricsStatus {
    Idle,
    Loading,
    Found,
    Instrumental,
    NotFound,
    /// 保留正在等待的退避任务，暂停恢复不重新排队。
    Retrying(String),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct LyricsState {
    pub track_key: String,
    /// Identity and recording length used by the current lookup.
    pub request_generation: u64,
    pub request_duration_s: u32,
    pub status: LyricsStatus,
    pub lyrics: Option<Lyrics>,
}

impl Default for LyricsState {
    fn default() -> Self {
        Self {
            track_key: String::new(),
            request_generation: 0,
            request_duration_s: 0,
            status: LyricsStatus::Idle,
            lyrics: None,
        }
    }
}

impl LyricsState {
    /// A useful duration correction or resuming after an error deserves a new
    /// lookup. Progress updates and temporary missing durations do not.
    pub fn needs_refetch(&self, media: &MediaSession, was_playing: bool) -> bool {
        let duration_s = media.duration_secs();
        // 同源录音匹配使用时长消歧，播放器修正时长后需要重新匹配。
        let native_player = media.app_id == "com.apple.Music"
            || poni_core::nowplaying::is_apple_music(&media.app_id)
            || poni_core::nowplaying::is_qq_music(&media.app_id)
            || poni_core::nowplaying::is_netease_music(&media.app_id);
        let max_drift_s = if native_player {
            0.0
        } else {
            poni_core::lrclib::MAX_SYNC_DRIFT_S
        };
        let duration_changed = duration_s > 0
            && (self.request_duration_s == 0
                || duration_s.abs_diff(self.request_duration_s) as f64 > max_drift_s);
        duration_changed
            || (!was_playing && media.is_playing() && matches!(self.status, LyricsStatus::Error(_)))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumeState {
    pub level: f32,
    pub muted: bool,
    pub known: bool,
    pub adjustable: bool,
    pub can_mute: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PowerState {
    pub has_battery: bool,
    /// 0..=100 (meaningful only with a battery).
    pub percent: u8,
    pub charging: bool,
    pub on_ac: bool,
    pub saver: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HudKind {
    Volume {
        level: f32,
        muted: bool,
    },
    Charging {
        percent: u8,
        plugged: bool,
    },
    Bluetooth {
        name: String,
        connected: bool,
        battery: Option<u8>,
        audio: bool,
    },
    LowBattery {
        percent: u8,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hud {
    pub kind: HudKind,
    /// Monotonic time when it disappears.
    pub until: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NoticeAction {
    OpenSaveErrors,
    OpenPage(PageKind),
    /// 精确定位 Codex 消息，避免不同会话的提醒共用上次选择。
    #[cfg(any(windows, target_os = "macos"))]
    OpenCodex(String),
    /// Open Focus and start the (idle) timer.
    StartFocus,
    /// Open the Update sheet in Settings (a new version's notice).
    OpenUpdate,
    /// Open the app another app's notification came from (its
    /// AppUserModelID; Notifications). The island stays closed.
    OpenApp(String),
    /// Windows微信提醒只打开对应窗口，不读取消息内容。
    #[cfg(windows)]
    OpenWeChat(isize),
}

/// A one-click action shown as a small pill inside a notice (doesn't expand).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineAction {
    /// Start the idle Pomodoro phase.
    StartFocus,
    /// Put the notice's hidden words in place: an Oyenza sum (and sender),
    /// or a message and who it's from (Notifications).
    ShowAmount,
    /// Put the notice away now (a new version's: the menu and Settings
    /// keep offering it).
    Hide,
    /// Open this version's release notes ("Updated to").
    WhatsNew,
    /// Open the app a notification came from (the notice's `OpenApp`).
    Open,
    /// 向普通 Codex 会话发送固定文本“确认”，不处理权限审批。
    #[cfg(any(windows, target_os = "macos"))]
    CodexConfirm,
}

/// Whether this copy may read other apps' notifications (the Notifications
/// add-on, `services::notifications`), for its page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotifyAccess {
    /// Not looked at yet: the add-on is off, or just came on.
    #[default]
    Unknown,
    /// This copy has no package identity (the download from GitHub), and
    /// Windows lets only a packaged app read notifications.
    NoPackage,
    /// Windows hasn't been asked yet, or its question was closed unanswered.
    Ask,
    /// Windows' question is on screen.
    Asking,
    /// Asked, and Windows came back without an answer (the question was
    /// closed, or never showed): ask again, or turn it on in Windows' settings.
    Unanswered,
    /// Turned off for Oyenza Studio in Windows' settings.
    Denied,
    /// Windows lets it read them: new ones show on the island.
    Allowed,
    /// Windows' notification listener didn't answer (an older Windows, or a
    /// failure the log names).
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub icon: Icon,
    pub color: Color,
    pub title: String,
    /// Dimmer words after the title, on its line ("0.2.5" after "Update
    /// available"); empty for most notices.
    pub title_dim: String,
    pub subtitle: String,
    pub until: f64,
    pub action: Option<NoticeAction>,
    pub inline: Option<InlineAction>,
    /// The subtitle Show puts in place of `subtitle` ([`InlineAction::ShowAmount`]).
    pub reveal: Option<String>,
}

impl Notice {
    /// No time limit (a new version's): it stays until clicked or put away,
    /// and lets a volume change or a game's readout show over it meanwhile.
    pub fn stays(&self) -> bool {
        self.until.is_infinite()
    }
}

/// What the app knows about new versions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateView {
    /// A later release than this copy, with a download for this system.
    pub offer: Option<poni_core::update::Release>,
    /// Settings shows the Update sheet in place of its rows (until the
    /// island closes or the page changes).
    pub sheet: bool,
    /// Its Setup is downloaded and checked (Windows): the sheet offers
    /// Install now and Install on next start.
    pub ready: bool,
    /// Install on next start was picked for it.
    pub next_start: bool,
    /// This copy reads the update file itself (not a Microsoft Store copy):
    /// Settings has Check now.
    pub looks: bool,
    /// What Settings says in Check now's place since it was clicked, until
    /// the island closes or the page changes.
    pub said: Option<Said>,
}

/// Check now's answer in Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    Checking,
    UpToDate,
    Failed,
}

/// Transient per-page UI state (scroll offsets, text being typed, …).
#[derive(Clone, Debug, Default)]
pub struct PageUiState {
    pub volume_error: bool,
    pub volume_drag: Option<f32>,
    pub task_input: String,
    /// 切换日期时保留各日尚未提交的输入，避免草稿被添加到另一日。
    pub task_drafts: HashMap<i64, String>,
    pub task_scroll: f32,
    pub shelf_scroll: f32,
    /// 日历中选中的日期（自 1970-01-01 起的天数）。
    pub today_pick: Option<i64>,
    /// 日历浏览的年月；None 跟随当前月份，跨日后也会自动更新。
    pub today_month: Option<(i32, u8)>,
    pub today_month_picker: bool,
    pub weather_city_input: String,
    pub weather_city_editing: bool,
    pub weather_city_search_due: Option<f64>,
    /// Smoothly animated lyric scroll position (line index as float).
    pub lyric_pos: f32,
    pub lyric_track: String,
    /// Volume HUD bar position, gliding toward the real level.
    pub hud_level: f32,
    /// Collapsed media pill shows the track title until this mono time.
    pub peek_until: f64,
    /// Header tab under the pointer and since when (for hover labels).
    pub tab_hover: Option<(usize, f64)>,
    /// The module the System page shows under its strip.
    pub system_module: poni_core::sysmon::Module,
    /// The last System reading had a GPU (keeps its chip while the next one is on its way).
    pub system_gpu: bool,
    /// The last full System reading had the whole PC's power (keeps its chip
    /// while the page's first, light reading shows).
    pub system_power: bool,
    /// The page the Settings button was pressed on (pressing it again goes back).
    pub before_settings: Option<PageKind>,
    /// Start with Windows, as last read off the UI thread (None until Settings first opens).
    pub autostart: Option<bool>,
    /// Oyen theme: the music notes floating up in the media pill.
    pub notes: crate::oyen::Notes,
    /// The Eyes look on a MacBook's notch: the pointer the eyes follow and
    /// the side of the camera they peek out from (`mac/notch.rs`).
    #[cfg(target_os = "macos")]
    pub peek: poni_core::peek::Peek,
}

#[derive(Clone, Debug)]
pub struct WeatherCity {
    pub name: String,
    pub region: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl WeatherCity {
    pub fn label(&self) -> String {
        let mut parts = vec![self.name.as_str()];
        if !self.region.is_empty() && self.region != self.name {
            parts.push(&self.region);
        }
        if !self.country.is_empty() && self.country != self.region {
            parts.push(&self.country);
        }
        parts.join(" · ")
    }
}

#[derive(Clone, Debug)]
pub struct WeatherSnapshot {
    pub place: String,
    pub temperature_c: f32,
    pub code: u8,
    pub is_day: bool,
    pub updated_unix: i64,
}

#[derive(Clone, Debug, Default)]
pub struct WeatherView {
    pub snapshot: Option<WeatherSnapshot>,
    pub message: String,
    pub loading: bool,
    pub request_id: u64,
    pub city_candidates: Vec<WeatherCity>,
    pub city_search_loading: bool,
    pub city_search_message: String,
    pub city_search_id: u64,
}

/// Whether Windows lets Oyenza Studio read a game's frames. Its event tracing
/// is open to admins and to the Performance Log Users group; the one-time
/// OK adds the account to that group, which counts from the next sign-in.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FpsAccess {
    /// Not asked yet.
    #[default]
    Unknown,
    /// Being checked.
    Checking,
    /// Frames can be read.
    Ready,
    /// The one-time OK is needed.
    NeedsOk,
    /// Windows' prompt for it is open.
    Asking,
    /// The account is in the group; Windows applies it at the next sign-in.
    SignOut,
    /// The OK didn't go through (why, in a few words).
    Failed(String),
}

/// Show over games: the readout in the island over a full-screen app
/// (`activities::Activity::Game`) and the System page's Games view.
#[derive(Clone, Debug, Default)]
pub struct GameState {
    /// The readout is on screen: Show over games is on and a full-screen
    /// app is in front.
    pub active: bool,
    /// The full-screen app ("cs2.exe").
    pub app: Option<String>,
    /// Its frame rate, while it draws and the frames can be read.
    pub rate: Option<poni_core::frames::FrameRate>,
    /// Frames per second over the last minute in front.
    pub history: poni_core::sysmon::Series,
    pub access: FpsAccess,
}

/// Oyenza sign-in, as the island sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum OyenzaLink {
    #[default]
    Off,
    /// A code on screen, waiting for approval in oyenza.app, until mono time `until`.
    Connecting {
        code: String,
        url: String,
        until: f64,
    },
    Connected {
        account: String,
    },
}

/// Where the Today page's day stands (`OyenzaState::agenda`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgendaState {
    /// Not asked yet since signing in or opening.
    #[default]
    Waiting,
    /// `agenda` holds the latest answer.
    Ready,
    /// The last ask failed; `agenda` may still hold an older answer.
    Failed,
    /// The server has no day for Today (from before it, or switched off there).
    Unavailable,
}

/// What the Oyenza tab and its notices show (`pages::oyenza`), and the Today
/// page's day (`pages::today`). Oyen's moods never read any of it: money
/// never makes him happier or a bill sadder.
#[derive(Clone, Debug, Default)]
pub struct OyenzaState {
    pub link: OyenzaLink,
    /// The last 7 days of transactions (asked while the page is open).
    pub activity: Option<Activity>,
    /// When each row was first seen, so what lands while you look glows.
    pub seen: Seen,
    /// The day the page shows (Today when it opens).
    pub day: DayPick,
    /// How far that day's list is scrolled (DIPs).
    pub scroll: f32,
    pub bills: Vec<Bill>,
    /// The server's today for `bills` (YYYY-MM-DD).
    pub bills_today: String,
    /// Unix time of Oyenza's last answer ("Updated 13:49").
    pub updated_unix: Option<i64>,
    /// The spend being typed.
    pub input: String,
    /// A note is on its way to the server.
    pub saving: bool,
    /// What was just saved, with Undo until mono time `.1`.
    pub saved: Option<(Vec<Saved>, f64)>,
    /// Why the last note wasn't saved (until the text changes).
    pub error: Option<String>,
    /// A short line ("Taken back") until mono time `.1`.
    pub flash: Option<(String, f64)>,
    /// "Show amounts" in the tab, until the island closes.
    pub reveal: bool,
    /// A notice that came while a full-screen app hid the island, and when.
    pub held: Option<(Notice, f64)>,
    /// Arrivals already shown as a notice (`oyenza::unseen`), so none repeats.
    pub shown: Vec<String>,
    /// Pinned before a sign-in code pinned the island open.
    pub pinned_before: Option<bool>,
    /// The Today page's day (`poni_core::agenda`), kept in memory only, and
    /// the unix time it came.
    pub agenda: Option<poni_core::agenda::Answer>,
    pub agenda_unix: Option<i64>,
    pub agenda_state: AgendaState,
    /// Signing in started from the Today page: back to it once connected.
    pub from_today: bool,
}

pub struct AppState {
    pub settings: Settings,
    pub store: Store,
    /// Monotonic seconds at the current frame.
    pub now: f64,
    pub local: LocalDateTime,
    pub unix: i64,
    pub media: Option<MediaSession>,
    pub lyrics: LyricsState,
    pub pomodoro: Pomodoro,
    pub volume: VolumeState,
    pub power: PowerState,
    /// 蓝牙页面最近一次设备快照；不写入用户配置。
    pub bluetooth: crate::bluetooth::BluetoothState,
    pub hud: Option<Hud>,
    pub notice: Option<Notice>,
    /// The System page's readings and their last minute (empty while it's closed).
    pub system: poni_core::sysmon::Monitor,
    pub weather: WeatherView,
    #[cfg(any(windows, target_os = "macos"))]
    pub mail: crate::mail::MailState,
    #[cfg(any(windows, target_os = "macos"))]
    pub codex: crate::codex::Inbox,
    /// Show over games.
    pub game: GameState,
    pub shelf_thumbs: HashMap<String, Option<Image>>,
    /// Icons of the apps the System page lists, by executable path (None = loading or none).
    pub app_icons: HashMap<String, Option<Image>>,
    pub ui: PageUiState,
    /// System accent colour (falls back to blue).
    pub accent: Color,
    /// The page currently shown when expanded.
    pub page: PageKind,
    /// When playback went from playing to paused (the pill lingers 30 s).
    pub media_paused_at: Option<f64>,
    pub pinned: bool,
    /// Stepped aside (Settings › Over maximized windows): an app window sits
    /// under where the island rests, so the closed island rests as an
    /// invisible strip at the top edge (`activities::Mode::Aside`). Windows
    /// sets it; the Mac never does.
    pub aside: bool,
    /// Files are being dragged over the island right now.
    pub drag_hover: bool,
    pub oyenza: OyenzaState,
    /// New versions (`crate::updates`): what Settings, the menu and the
    /// island's notice show.
    pub update: UpdateView,
    /// Whether Windows lets the island read notifications.
    pub notify_access: NotifyAccess,
    /// This copy can read other apps' notifications: Windows lets only an app
    /// with a package identity, the Microsoft Store copy, do it. The download
    /// can't, so it offers no Notifications at all. Set once at start (the
    /// self-test sets it per scene).
    pub notify_can: bool,
    /// Side effects requested during the frame.
    pub cmds: Vec<Command>,
    pub save_errors: crate::persistence::SaveErrors,
}

impl AppState {
    pub fn new(settings: Settings, mut store: Store) -> Self {
        // 旧待办没有计划日期，按设备本地时区的创建日迁移；不改写原始时间戳。
        let mut task_dates_changed = false;
        for task in &mut store.tasks.tasks {
            if task.scheduled_day.is_none() {
                let date = crate::clock::local_at(task.created_unix);
                task.scheduled_day = Some(poni_core::datetime::days_from_civil(
                    date.year, date.month, date.day,
                ));
                task_dates_changed = true;
            }
            if task.done && task.completed_day.is_none() {
                if let Some(unix) = task.done_unix {
                    let date = crate::clock::local_at(unix);
                    task.completed_day = Some(poni_core::datetime::days_from_civil(
                        date.year, date.month, date.day,
                    ));
                    task_dates_changed = true;
                }
            }
        }
        let pomodoro = Pomodoro::new(settings.pomodoro.clone());
        let page = PageKind::from_key(if settings.remember_page && !store.last_page.is_empty() {
            &store.last_page
        } else {
            &settings.start_page
        });
        let mut st = Self {
            settings,
            store,
            now: 0.0,
            local: LocalDateTime::default(),
            unix: 0,
            media: None,
            lyrics: LyricsState::default(),
            pomodoro,
            volume: VolumeState::default(),
            power: PowerState::default(),
            bluetooth: crate::bluetooth::BluetoothState::default(),
            hud: None,
            notice: None,
            system: poni_core::sysmon::Monitor::default(),
            weather: WeatherView::default(),
            #[cfg(any(windows, target_os = "macos"))]
            mail: crate::mail::MailState::default(),
            #[cfg(any(windows, target_os = "macos"))]
            codex: crate::codex::Inbox::default(),
            game: GameState::default(),
            shelf_thumbs: HashMap::new(),
            app_icons: HashMap::new(),
            ui: PageUiState::default(),
            accent: palette::BLUE,
            page,
            media_paused_at: None,
            pinned: false,
            aside: false,
            drag_hover: false,
            oyenza: OyenzaState::default(),
            update: UpdateView::default(),
            notify_access: NotifyAccess::Unknown,
            notify_can: false,
            cmds: Vec::new(),
            save_errors: crate::persistence::SaveErrors::default(),
        };
        let off = match st.page {
            PageKind::System => !st.system_on(),
            PageKind::Notifications => !st.notifications_on(),
            p => st.tab_hidden(p),
        };
        if off {
            st.page = crate::pages::first_tab(&st);
        }
        if task_dates_changed {
            st.save();
        }
        st
    }

    /// 系统监控启用时显示系统页，并允许读取监控数据。
    pub fn system_on(&self) -> bool {
        self.settings.system_on()
    }

    /// Notifications is on: its tab shows, and new notifications from the
    /// apps picked there show on the island. On by default wherever this copy
    /// can read them (`notify_can`); its chip in Settings' tabs row switches
    /// it off.
    pub fn notifications_on(&self) -> bool {
        self.notify_can && !self.tab_hidden(PageKind::Notifications)
    }

    /// Settings took this page's tab off the header's row (Music, Today,
    /// Shelf, Focus or Notifications).
    pub fn tab_hidden(&self, p: PageKind) -> bool {
        if p == PageKind::System {
            return !self.settings.system_on();
        }
        p.hideable() && self.settings.hidden_tabs.iter().any(|k| k == p.key())
    }

    /// Show `n` now or, while a full-screen app hides the island (`hidden`), keep
    /// it for when the island is back, the way money notices wait
    /// (`OyenzaState::held`, shown then if no older than `HELD_MAX_S`). One
    /// notice waits at a time, and a money or bill notice already waiting stays.
    pub fn show_or_hold(&mut self, n: Notice, hidden: bool) {
        if !hidden {
            self.notice = Some(n);
        } else if self
            .oyenza
            .held
            .as_ref()
            .is_none_or(|(_, at)| self.now - at > poni_core::oyenza::HELD_MAX_S)
        {
            self.oyenza.held = Some((n, self.now));
        }
    }

    pub fn cmd(&mut self, c: Command) {
        self.cmds.push(c);
    }

    pub fn media_cmd(&mut self, c: MediaCmd) {
        self.cmds.push(Command::Media(c));
    }

    /// 记录每个文件的保存结果；失败一直保留到该文件保存成功或设置恢复原值。
    pub fn saved(
        &mut self,
        file: crate::persistence::SaveFile,
        result: Result<(), crate::persistence::SaveError>,
    ) -> bool {
        use crate::persistence::SaveFile;
        let before = self.save_errors.clone();
        let error = result.err();
        match file {
            SaveFile::Settings => self.save_errors.settings = error,
            SaveFile::Store => self.save_errors.store = error,
        }
        if !self.save_errors.any() {
            self.save_errors.quit_blocked = false;
            if self
                .notice
                .as_ref()
                .is_some_and(|notice| notice.action == Some(NoticeAction::OpenSaveErrors))
            {
                self.notice = None;
            }
        }
        if !before.any() && self.save_errors.any() && self.notice.is_none() {
            self.notify(
                Icon::Gear,
                palette::ORANGE,
                "更改尚未保存",
                "点击查看原因并重试；当前内存数据已保留",
                8.0,
                Some(NoticeAction::OpenSaveErrors),
            );
        }
        before != self.save_errors
    }

    /// Mark the persisted store as changed.
    pub fn save(&mut self) {
        if !self.cmds.contains(&Command::SaveStore) {
            self.cmds.push(Command::SaveStore);
        }
    }

    pub fn chime(&mut self) {
        if self.settings.focus_sound {
            self.cmds.push(Command::Play(Sound::Chime));
        }
    }

    /// 成功操作的提示音，仅受该操作所属模块的开关控制。
    pub fn tick(&mut self, source: SoundSource) {
        let enabled = match source {
            SoundSource::Focus => self.settings.focus_sound,
            SoundSource::Shelf => self.settings.shelf_sound,
            SoundSource::Codex => self.settings.codex_sound,
        };
        if enabled {
            #[cfg(windows)]
            let sound = match source {
                SoundSource::Focus => Sound::Tick,
                SoundSource::Shelf => Sound::Shelf,
                SoundSource::Codex => Sound::Codex,
            };
            #[cfg(not(windows))]
            let sound = Sound::Tick(source);
            self.cmds.push(Command::Play(sound));
        }
    }

    /// Show a notice pill for `secs` seconds.
    pub fn notify(
        &mut self,
        icon: Icon,
        color: Color,
        title: impl Into<String>,
        subtitle: impl Into<String>,
        secs: f64,
        action: Option<NoticeAction>,
    ) {
        self.notice = Some(Notice {
            icon,
            color,
            title: title.into(),
            title_dim: String::new(),
            subtitle: subtitle.into(),
            until: self.now + secs,
            action,
            inline: None,
            reveal: None,
        });
    }

    /// The Update sheet: Settings, with the sheet in place of its rows (the
    /// gear goes back to the page it was opened from).
    pub fn open_update(&mut self) {
        if self.page != PageKind::Settings {
            self.ui.before_settings = Some(self.page);
        }
        self.page = PageKind::Settings;
        self.update.sheet = true;
    }

    /// Give the current notice a one-click inline action pill.
    pub fn set_notice_inline(&mut self, a: InlineAction) {
        if let Some(n) = self.notice.as_mut() {
            n.inline = Some(a);
        }
    }

    pub fn show_hud(&mut self, kind: HudKind, secs: f64) {
        self.hud = Some(Hud {
            kind,
            until: self.now + secs,
        });
    }

    pub fn day_key(&self) -> String {
        self.local.day_key()
    }

    pub fn day_start_unix(&self) -> i64 {
        crate::clock::day_start_unix(&self.local, self.unix)
    }

    /// Accent for media UI: art colour when available, else the system accent.
    /// The Oyen theme keeps to Oyen's orange.
    pub fn media_accent(&self) -> Color {
        if self.settings.theme == poni_core::settings::Theme::Oyen {
            return self.accent;
        }
        self.media
            .as_ref()
            .and_then(|m| m.art_color)
            .unwrap_or(self.accent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_waits_while_a_full_screen_app_hides_the_island() {
        let mut st = AppState::new(Settings::default(), Store::default());
        st.now = 100.0;
        let notice = |title: &str| Notice {
            icon: Icon::Check,
            color: palette::GREEN,
            title: title.into(),
            title_dim: String::new(),
            subtitle: String::new(),
            until: 0.0,
            action: None,
            inline: None,
            reveal: None,
        };
        let held = |st: &AppState| {
            st.oyenza
                .held
                .as_ref()
                .map(|(n, at)| (n.title.clone(), *at))
        };
        // Hidden: it waits for the island instead of expiring unseen.
        st.show_or_hold(notice("goal"), true);
        assert!(st.notice.is_none());
        assert_eq!(held(&st), Some(("goal".into(), 100.0)));
        // A money notice already waiting stays...
        st.oyenza.held = Some((notice("money"), 100.0));
        st.show_or_hold(notice("goal"), true);
        assert_eq!(held(&st), Some(("money".into(), 100.0)));
        // ...unless it's too old to show any more.
        st.now = 100.0 + poni_core::oyenza::HELD_MAX_S + 1.0;
        st.show_or_hold(notice("goal"), true);
        assert_eq!(held(&st).map(|h| h.0), Some("goal".into()));
        // The island on screen: it shows at once.
        st.show_or_hold(notice("now"), false);
        assert_eq!(st.notice.as_ref().map(|n| n.title.as_str()), Some("now"));
    }
}
