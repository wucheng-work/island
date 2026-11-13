//! 各模块使用内置 PCM 提示音，不依赖目标电脑安装 macOS 声音文件。

use crate::commands::Sound;
use poni_core::settings::Settings;
use poni_core::sound_catalog;
use windows::core::w;
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

#[path = "sound_data.rs"]
mod data;

pub fn play(s: Sound, settings: &Settings) {
    // 静态音频在异步播放期间始终有效，不依赖用户系统声音方案中的别名。
    let (selected, default) = match s {
        Sound::Chime | Sound::Tick => (settings.focus_sound_id.as_str(), sound_catalog::DEFAULT_FOCUS),
        Sound::Shelf => (settings.shelf_sound_id.as_str(), sound_catalog::DEFAULT_SHELF),
        Sound::Codex => (settings.codex_sound_id.as_str(), sound_catalog::DEFAULT_CODEX),
        Sound::Mail => (settings.mail_sound_id.as_str(), sound_catalog::DEFAULT_MAIL),
        Sound::WeChat => (settings.wechat_sound_id.as_str(), sound_catalog::DEFAULT_WECHAT),
    };
    let wave = data::find(selected).or_else(|| data::find(default)).expect("built-in default sound");
    unsafe {
        if !PlaySoundW(PCWSTR(wave.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT).as_bool() {
            let _ = PlaySoundW(w!("SystemNotification"), None, SND_ALIAS | SND_ASYNC);
        }
    }
}
