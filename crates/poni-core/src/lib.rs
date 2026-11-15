//! Platform-independent models and logic for Poni.
//!
//! Everything here is pure Rust with no OS calls so it can be unit-tested on
//! any machine. The Windows crate (`poni`) owns all I/O and rendering and
//! feeds clocks/dates into these types. `store` uses `std` I/O for the JSON
//! files, which still runs anywhere.

pub mod agenda;
#[cfg(not(windows))]
pub mod alert_sounds;
pub mod applemusic;
pub mod base64;
pub mod datetime;
/// 跨平台共享的界面尺度。
pub mod design;
pub mod filesize;
pub mod frames;
pub mod frametime;
pub mod geom;
pub mod glass;
pub mod hdr;
pub mod look;
pub mod lrc;
pub mod lrclib;
pub mod lunar;
pub mod mail;
pub mod mediaremote;
pub mod notifications;
pub mod nowplaying;
pub mod oyen;
pub mod oyenza;
pub mod peek;
pub mod pomodoro;
pub mod power;
pub mod settings;
pub mod sha256;
pub mod shelf;
// 保留提示音目录生成器的排版。
#[cfg(windows)]
#[rustfmt::skip]
pub mod sound_catalog;
pub mod spot;
pub mod spring;
pub mod store;
pub mod svgpath;
pub mod sysmon;
pub mod timeline;
pub mod todo;
pub mod ttml;
pub mod update;
pub mod urlenc;
