//! Messages from service threads to the UI (main) thread.
//!
//! Services hold an [`EventSink`]. `send` pushes onto an mpsc channel and
//! queues a drain on the main dispatch queue; the main thread then applies
//! every waiting event. The event types match the Windows app's, so the
//! services that arrive in later slices plug in the same way.

use crate::gfx::Image;
use crate::state::{MediaSession, PowerState, VolumeState, WeatherCity, WeatherSnapshot};
use poni_core::lrc::Lyrics;
use poni_core::timeline::Timeline;
use std::sync::mpsc::Sender;

#[derive(Clone)]
pub struct EventSink {
    tx: Sender<AppEvent>,
}

impl EventSink {
    pub fn new(tx: Sender<AppEvent>) -> Self {
        Self { tx }
    }

    pub fn send(&self, ev: AppEvent) {
        if self.tx.send(ev).is_ok() {
            dispatch2::DispatchQueue::main().exec_async(|| {
                crate::app::drain_events();
            });
        }
    }
}

/// What Apple Music said about the user's token.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppleMusicStatus {
    Accepted,
    Rejected,
    Unreachable,
    NoAccess,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum LyricsResult {
    Found { lyrics: Lyrics },
    Instrumental,
    NotFound,
    /// 已安排自动重试；与需要手动恢复的最终错误分开。
    Retrying(String),
    Error(String),
}

#[allow(dead_code)]
pub enum AppEvent {
    Update(crate::services::update::Event),
    /// 显式点击试听，不改变模块声音选择或声音开关。
    AlertTonePreview(String),
    /// 原生选择器确认使用，保存该模块的提示音 ID。
    AlertToneSelected { module: poni_core::alert_sounds::SoundModule, id: String },
    Codex(crate::codex_bridge::BridgeEvent),
    Mail(crate::mail::MailEvent),
    MailAction(crate::mail::MailAction),
    /// The current media session changed (None = nothing playing / no session).
    Media(Option<MediaSession>),
    /// Only the timeline/playback state of the current track changed.
    MediaTimeline {
        track_key: String,
        timeline: Timeline,
    },
    /// Album art finished decoding for a track.
    MediaArt {
        track_key: String,
        art: Option<Image>,
    },
    /// Lyrics belong to both the track and the player that requested them.
    Lyrics {
        track_key: String,
        app_id: String,
        generation: u64,
        result: LyricsResult,
    },
    AppleMusic(AppleMusicStatus),
    /// 系统颜色选择器返回的当前歌词 RGB 色。
    LyricsColor(u32),
    /// Master volume changed (0..=1). `switched`: the output device changed
    /// (headphones connected, say), not its volume.
    Volume {
        state: VolumeState,
        switched: bool,
    },
    Power(PowerState),
    Bluetooth {
        name: String,
        connected: bool,
        battery: Option<u8>,
        audio: bool,
    },
    BluetoothDevicesStarted(u64),
    BluetoothDevices {
        generation: u64,
        result: Result<poni_core::bluetooth::BluetoothSnapshot, String>,
    },
    /// A full-screen app took over (or left) the island's screen.
    Fullscreen(bool),
    /// Files dropped on the island.
    ShelfAdd(Vec<String>),
    /// 后台校验完成；只有暂存区仍与快照一致时才提交，避免撤销/删除期间用旧结果。
    ShelfPrepared {
        before: Vec<poni_core::shelf::ShelfItem>,
        requested: Vec<String>,
        result: poni_core::shelf::PreparedPaths,
    },
    /// Files are being dragged over the island (true) or left (false).
    DragHover(bool),
    ShelfThumb {
        path: String,
        image: Option<Image>,
    },
    /// The System page's latest reading and the minute behind it
    /// (`services::sysmon`, each second while the page is open).
    System(Box<poni_core::sysmon::Monitor>),
    WeatherLocation {
        request_id: u64,
        result: Result<(f64, f64, String), String>,
    },
    WeatherForecast {
        request_id: u64,
        result: Result<WeatherSnapshot, String>,
    },
    WeatherCityResults {
        search_id: u64,
        result: Result<Vec<WeatherCity>, String>,
    },
    /// An app's icon for the System page, by its path (`Command::LoadAppIcon`).
    AppIcon {
        path: String,
        image: Option<Image>,
    },
    /// The displays went to sleep (true) or woke.
    ScreensAsleep(bool),
    /// The Mac was locked (true) or unlocked.
    Locked(bool),
    /// Free-form log line from a service thread.
    Log(String),
}
