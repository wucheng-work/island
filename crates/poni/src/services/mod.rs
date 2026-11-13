//! Background services. Each one reports to the UI thread through an
//! [`EventSink`]; a failing service is logged and skipped, never fatal.

pub mod update_worker;
pub mod applemusic;
pub mod applemusic_ui;
pub mod audio;
pub mod color;
pub mod location;
#[path = "../mac/services/weather.rs"]
pub mod weather;
pub mod backdrop;
pub mod bluetooth;
pub mod clipboard;
pub mod frames;
pub mod fullscreen;
pub mod http;
pub mod imaging;
pub mod lyrics;
pub mod media;
mod netease_playback;
#[path = "../mac/services/mail.rs"]
pub mod mail;
pub mod oyenza;
pub mod power;
pub mod psu;
pub mod secret;
pub mod sound;
pub mod sound_picker;
pub mod notifications;
pub mod sysmon;
pub mod update;
pub mod wechat;
pub mod font;

use crate::commands::{MediaCmd, OyenzaCmd};
use crate::events::EventSink;
use crate::state::MediaSession;
use poni_core::geom::Color;
use poni_core::settings::Settings;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};

pub struct Services {
    media: Option<media::MediaService>,
    lyrics: Option<lyrics::LyricsService>,
    audio: Option<audio::AudioService>,
    _bluetooth: Option<bluetooth::BluetoothService>,
    fullscreen: Option<fullscreen::FullscreenWatcher>,
    oyenza: Option<oyenza::OyenzaService>,
    updates: Option<update_worker::UpdateWorker>,
}

fn started<T, E: std::fmt::Display>(name: &str, r: Result<T, E>) -> Option<T> {
    match r {
        Ok(v) => {
            crate::pinfo!("service {name}: started");
            Some(v)
        }
        Err(e) => {
            crate::pwarn!("service {name}: unavailable ({e})");
            None
        }
    }
}

impl Services {
    pub fn start(sink: &EventSink, _island: HWND, helper: HWND, _settings: &Settings, activity: oyenza::Activity) -> Self {
        let media = started("media", media::MediaService::start(sink.clone()));
        let lyrics = started("lyrics", lyrics::LyricsService::start(sink.clone()));
        let audio = started("audio", audio::AudioService::start(sink.clone()));
        let bluetooth = started("bluetooth", bluetooth::BluetoothService::start(sink.clone()));
        let fullscreen = started("fullscreen", fullscreen::FullscreenWatcher::start(helper, sink.clone()));
        // Idle (no calls) until the person signs in to Oyenza from the tray.
        let oyenza = started("oyenza", oyenza::OyenzaService::start(sink.clone(), activity));
        Self { media, lyrics, audio, _bluetooth: bluetooth, fullscreen, oyenza, updates: None }
    }

    pub fn stop(self, _helper: HWND) {
        if let Some(f) = self.fullscreen {
            f.stop();
        }
    }

    /// 用户主动检查后才创建更新线程；启动应用不会增加线程或网络请求。
    pub fn run_job(&mut self, job: update_worker::Job) -> bool {
        if self.updates.is_none() {
            self.updates = started("updates", update_worker::UpdateWorker::start());
        }
        self.updates.as_ref().map(|worker| worker.run(job)).is_some()
    }

    /// Step aside on (the band along the top edge of the island's monitor,
    /// screen pixels) or off: the full-screen watcher reports what reaches it.
    pub fn watch_top(&self, band: Option<RECT>) {
        if let Some(f) = &self.fullscreen {
            f.watch_top(band);
        }
    }

    /// Explorer restarted: AppBar registrations are lost and must be redone.
    pub fn on_taskbar_created(&self) {
        if let Some(f) = &self.fullscreen {
            f.on_taskbar_created();
        }
    }

    pub fn set_output_volume(&self, level: f32) -> bool {
        self.audio.as_ref().is_some_and(|audio| audio.set_volume(level))
    }

    pub fn set_output_mute(&self, muted: bool) -> bool {
        self.audio.as_ref().is_some_and(|audio| audio.set_mute(muted))
    }

    pub fn media_cmd(&self, c: MediaCmd) {
        if let Some(m) = &self.media {
            m.send(c);
        }
    }

    pub fn refresh_media(&self, reason: &'static str) {
        if let Some(media) = &self.media {
            media.refresh(reason);
        }
    }

    pub fn request_lyrics(&self, s: &MediaSession) -> Option<u64> {
        self.lyrics.as_ref().map(|l| l.request(s))
    }

    pub fn cancel_lyrics(&self) {
        if let Some(l) = &self.lyrics {
            l.cancel();
        }
    }

    /// 暂停媒体和歌词的延迟重试，恢复使用后重新检查媒体会话。
    pub fn set_media_rest(&self, rest: bool) {
        if let Some(media) = &self.media {
            media.set_rest(rest);
        }
        if let Some(l) = &self.lyrics {
            l.set_rest(rest);
        }
    }

    pub fn oyenza(&self, c: OyenzaCmd) {
        if let Some(o) = &self.oyenza {
            o.send(c);
        }
    }

    /// The PC's use or power changed (the Oyenza worker paces itself by it).
    pub fn oyenza_activity(&self, a: oyenza::Activity) {
        if let Some(o) = &self.oyenza {
            o.set_activity(a);
        }
    }

    /// The Apple Music token on disk was saved or removed.
    pub fn apple_music_token_changed(&self) {
        if let Some(l) = &self.lyrics {
            l.apple_token_changed();
        }
    }

    /// Handle the AppBar callback message; `Some(true)` when a full-screen
    /// app covers the island's monitor.
    pub fn on_appbar_message(&self, wp: WPARAM, lp: LPARAM, monitor: isize) -> Option<bool> {
        self.fullscreen.as_ref().and_then(|f| f.on_appbar_message(wp, lp, monitor))
    }
}

/// Windows apps are set to light mode (Settings › Personalization › Colors).
pub fn apps_use_light_theme() -> bool {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    // The system background is white in light mode and black in dark mode.
    UISettings::new().and_then(|s| s.GetColorValue(UIColorType::Background)).map(|c| c.R as u32 + c.G as u32 + c.B as u32 > 382).unwrap_or(true)
}

/// The Windows accent colour as it is: the Glass theme's text and fills sit
/// on light and dark glass alike, so it isn't lightened for black.
pub fn system_accent_base() -> Option<Color> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let s = UISettings::new().ok()?;
    let c = s.GetColorValue(UIColorType::Accent).ok()?;
    Some(Color::rgba(c.R as f32 / 255.0, c.G as f32 / 255.0, c.B as f32 / 255.0, 1.0))
}

/// The Windows accent colour, lightened for use on the black island.
pub fn system_accent() -> Option<Color> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let s = UISettings::new().ok()?;
    let c = s.GetColorValue(UIColorType::AccentLight2).ok()?;
    Some(Color::rgba(c.R as f32 / 255.0, c.G as f32 / 255.0, c.B as f32 / 255.0, 1.0).for_island_accent())
}
