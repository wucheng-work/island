//! Mac services for media, lyrics, power, system monitoring and local files.

// This file is loaded with #[path], so its modules sit in services/ by path.
#[path = "../services/applemusic.rs"]
pub mod applemusic;
#[path = "services/audio.rs"]
pub mod audio;
#[path = "services/cf.rs"]
mod cf;
#[path = "services/clipboard.rs"]
pub mod clipboard;
#[path = "services/fullscreen.rs"]
pub mod fullscreen;
#[path = "services/host.rs"]
pub mod host;
#[path = "services/http.rs"]
pub mod http;
#[path = "services/login.rs"]
pub mod login;
#[path = "services/location.rs"]
pub mod location;
#[path = "services/mail.rs"]
pub mod mail;
#[path = "services/translation.rs"]
pub mod translation;
#[path = "../services/lyrics.rs"]
pub mod lyrics;
#[path = "services/media.rs"]
pub mod media;
#[path = "services/mediaremote.rs"]
mod mediaremote;
#[path = "services/power.rs"]
pub mod power;
#[path = "services/secret.rs"]
pub mod secret;
/// 一次性的暂存区文件内容检查。
#[path = "services/shelf.rs"]
pub mod shelf;
#[path = "services/smc.rs"]
pub mod smc;
#[path = "services/sysmon.rs"]
pub mod sysmon;
#[path = "services/sysstats.rs"]
pub mod sysstats;
#[path = "services/thumbs.rs"]
pub mod thumbs;
#[path = "services/weather.rs"]
pub mod weather;
#[path = "services/update.rs"]
pub mod update;

use crate::commands::{Sound, SoundSource};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSColorSpace, NSSound, NSWorkspace};
use objc2_foundation::{NSArray, NSFileManager, NSString, NSURL};
use poni_core::alert_sounds::{self, AlertSounds, SoundModule};
use poni_core::geom::Color;
use std::cell::RefCell;

pub mod imaging {
    //! What the Shelf page asks about its files.

    /// Size (DIPs) at which the Shelf draws a thumbnail tile.
    pub const THUMB_DIP: f32 = 52.0;

    #[allow(dead_code)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PathKind {
        File,
        Folder,
        Missing,
    }

    /// What is at a shelf path right now (a file system lookup; the Windows
    /// app caches this off the UI thread because its paths can be on a network).
    pub fn path_kind(path: &str) -> Option<PathKind> {
        Some(match std::fs::metadata(path) {
            Ok(m) if m.is_dir() => PathKind::Folder,
            Ok(_) => PathKind::File,
            Err(_) => PathKind::Missing,
        })
    }
}

/// The accent colour chosen in System Settings > Appearance.
pub fn system_accent() -> Option<Color> {
    let c = NSColor::controlAccentColor();
    let c = c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    Some(Color::rgba(
        c.redComponent() as f32,
        c.greenComponent() as f32,
        c.blueComponent() as f32,
        1.0,
    ))
}

/// Show a file selected in a Finder window.
pub fn reveal(path: &str) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
}

/// The app was called Oyen Poni before 0.2.0, and a copy from then has the same
/// bundle id, so Spotlight or the login item could start it instead of this one.
/// Moves "Oyen Poni.app" from /Applications or ~/Applications to the Trash, where
/// it can be put back, and returns the folder it was in.
pub fn trash_app_from_before_rename() -> Option<String> {
    let running = std::env::current_exe().ok()?;
    let mut dirs = vec![std::path::PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::PathBuf::from(home).join("Applications"));
    }
    for dir in dirs {
        let old = dir.join("Oyen Poni.app");
        if running.starts_with(&old) {
            continue;
        }
        let plist = std::fs::read_to_string(old.join("Contents/Info.plist")).unwrap_or_default();
        if !plist.contains("<string>com.oyenza.poni</string>") {
            continue;
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(&old.to_string_lossy()));
        match NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None) {
            Ok(()) => {
                pinfo!(
                    "moved {} to the Trash (the app from before the rename)",
                    old.display()
                );
                return Some(dir.to_string_lossy().into_owned());
            }
            Err(e) => pwarn!(
                "couldn't move {} to the Trash: {}",
                old.display(),
                e.localizedDescription()
            ),
        }
    }
    None
}

/// 显式点击附件后打开系统快速预览；不执行文件，也不获取远程内容。
pub fn preview_codex_attachment(path: &str) -> Result<(), String> {
    let file = std::path::Path::new(path);
    if !file.is_absolute() || !file.is_file() {
        return Err("附件已移动或清理，请在 Codex 中查看原文件".into());
    }
    let path = std::ffi::CString::new(path)
        .map_err(|_| "附件路径无效，请在 Codex 中查看原文件".to_string())?;
    // 命令在主线程执行；预览窗口由原生桥统一持有，重复点击只更换预览项。
    match unsafe { poni_preview_attachment(path.as_ptr()) } {
        0 => Ok(()),
        1 => Err("无法读取附件，请检查文件是否仍存在及访问权限".into()),
        _ => Err("无法打开系统预览，请稍后重试".into()),
    }
}

unsafe extern "C" {
    fn poni_preview_attachment(path: *const std::ffi::c_char) -> std::ffi::c_int;
    fn poni_close_attachment_preview();
}

/// 锁屏或显示器休眠时关闭附件，不在唤醒后恢复敏感预览。
pub fn close_attachment_preview() {
    unsafe { poni_close_attachment_preview() };
}

/// Open a URL (or a file path) with its default app. System Settings'
/// panes have URLs without "//" (`x-apple.systempreferences:…`).
pub fn open(target: &str) {
    let s = NSString::from_str(target);
    let is_url = target.contains("://") || target.starts_with("x-apple.systempreferences:");
    let url = if is_url {
        NSURL::URLWithString(&s)
    } else {
        Some(NSURL::fileURLWithPath(&s))
    };
    match url {
        Some(u) => {
            if !NSWorkspace::sharedWorkspace().openURL(&u) {
                pwarn!("could not open {target}");
            }
        }
        None => pwarn!("not a URL: {target}"),
    }
}

thread_local! {
    // 只保留最近播放的一款，连续试听不会把整套音频解码后常驻内存。
    static ALERT_SOUND: RefCell<Option<(String, Retained<NSSound>)>> = const { RefCell::new(None) };
}

/// 开关在命令生产处判断；平台按持久化的模块选择播放。
pub fn play(sound: Sound, settings: &AlertSounds) {
    let module = match sound {
        Sound::Chime | Sound::Tick(SoundSource::Focus) => SoundModule::Focus,
        Sound::Tick(SoundSource::Shelf) => SoundModule::Shelf,
        Sound::Tick(SoundSource::Codex) => SoundModule::Codex,
    };
    play_tone(settings.get(module));
}

/// 试听和真实提醒共用加载与播放链路；只接受内置目录中的固定标识。
pub fn play_tone(id: &str) -> bool {
    let Some(tone) = alert_sounds::tone(id) else {
        pwarn!("sound: unknown alert tone");
        return false;
    };
    ALERT_SOUND.with(|cell| {
        let mut active = cell.borrow_mut();
        if active.as_ref().is_none_or(|(current, _)| current != id) {
            let Some(bundle) = std::env::current_exe().ok().and_then(|exe| exe.parent()?.parent().map(std::path::Path::to_path_buf)) else {
                pwarn!("sound: cannot locate application resources");
                return false;
            };
            let path = bundle.join("Resources/Sounds").join(tone.file);
            #[cfg(debug_assertions)]
            let path = if path.is_file() { path } else { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds").join(tone.file) };
            let path = NSString::from_str(&path.to_string_lossy());
            let Some(sound) = NSSound::initWithContentsOfFile_byReference(NSSound::alloc(), &path, false) else {
                pwarn!("sound: cannot load {}", tone.id);
                return false;
            };
            if let Some((_, previous)) = active.take() {
                previous.stop();
            }
            *active = Some((id.to_string(), sound));
        }
        let Some((_, sound)) = active.as_ref() else { return false };
        if sound.isPlaying() {
            sound.stop();
        }
        sound.setCurrentTime(0.0);
        if !sound.play() {
            pwarn!("sound: could not play {}", tone.id);
            return false;
        }
        true
    })
}
