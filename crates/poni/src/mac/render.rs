//! `poni --render <dir> [--theme classic|oyen|glass|pill|diorama] [--notch] [--codex|--bluetooth]`: draw the island's
//! states with sample data into PNGs, offscreen, through the Mac renderer.
//! CI runs it on a Mac with no notch and no person at the screen, so this is
//! how the Mac drawing gets looked at: every page and activity, each theme,
//! with and without a (simulated) MacBook notch.
//!
//! Each picture is the island over a desktop: a soft wallpaper gradient and a
//! light menu bar. With `--notch` the camera notch is painted on top, black,
//! the way it hides that part of the island on a real MacBook. Glass is
//! macOS's own view on the real window, so here a smoked stand-in takes its
//! place (the smoke run's screenshots show the real one). Pill floats below
//! the top edge, or around a notch rests inside it and floats below the menu
//! bar for everything else (notch.rs). Around a
//! notch, the last picture every theme has is the Eyes look's hide and seek
//! with the camera as it moves, a frame a row (`render_peek`). Diorama is
//! Pill with a picture in its resting pill: after those, its run draws that
//! picture through a day (`DIORAMA_SCENES`), floating below the menu bar
//! around a notch too.
//!
//! The music scenes use made-up data to test the drawing; the Mac app shows
//! music once the media slice is built. The System page's scenes show a
//! made-up minute of a MacBook too, with real app icons from Finder.
//! `--codex` 只绘制 Codex 页的离线示例，不启动消息监听或连接 Codex。
//! `--bluetooth` 只绘制蓝牙页的虚构设备与状态，不读取系统设备或启动查询。

use crate::activities::{self, Mode};
use crate::gfx::{Bitmap, Gfx, ImageData};
use crate::icons::Icon;
use crate::island::{Island, SHOULDER};
use crate::mail::{MailAttachment, MailConnection, MailMessage, MailPreview};
use crate::notch::{self, Notch};
use crate::pages::{self, PageKind};
use crate::state::{AppState, Hud, HudKind, InlineAction, LyricsState, LyricsStatus, MediaSession, NoticeAction, OyenzaLink, OyenzaState, PowerState, VolumeState};
use crate::ui::{Hit, Input};
use poni_core::bluetooth::{BatteryComponent, BluetoothDevice, DeviceKind};
use poni_core::datetime::LocalDateTime;
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::lrc::parse_lrc;
use poni_core::mail::{MailAccount, MailPrivacy, MailProvider};
use poni_core::oyenza::{Activity, Bill, Day, Seen, Txn, WalletRef};
use poni_core::pomodoro::Phase;
use poni_core::power::PowerReading;
use poni_core::settings::{IdleStyle, Settings, Theme};
use poni_core::store::Store;
use poni_core::sysmon::{
    level_pressure, AppUse, BatteryReading, CoreKind, CoreLoad, CpuReading, DiskActivity, Drive, GpuReading, MemoryReading, Module, Monitor, NetReading, PressureLevel, Reading, HISTORY_LEN,
};
use poni_core::timeline::Timeline;
use std::path::{Path, PathBuf};

/// Pixels per DIP (a Retina screen).
const SCALE: f32 = 2.0;
/// Room around the island in each picture (DIPs).
const MARGIN_X: f32 = 36.0;
const MARGIN_BOTTOM: f32 = 40.0;
/// The menu bar of a Mac without a notch (DIPs).
const MENU_BAR: f32 = 24.0;
/// A fixed moment for every picture: 24 Oct 2026, 09:41.
const NOW_MONO: f64 = 1000.0;
const NOW_UNIX: i64 = 1_792_834_860;

const TITLE: &str = "Paper Boats";
const ARTIST: &str = "The Lanterns";
const ALBUM: &str = "Harbor Lights";
const LYRICS_LRC: &str = "\
[00:38.00]Folded up the morning into something small
[00:43.50]Set it on the water by the harbor wall
[00:49.00]Ink along the edges of a name I used to know
[00:54.50]Watched it find the current, watched it go
[01:00.00]Paper boats, carry what I couldn't say
[01:05.50]Over the grey and out across the bay
[01:11.00]If the tide is gentle they will find the light
";
/// The same song in Japanese (the Windows self-test's, written for the test;
/// the smoke run plays it too).
pub const LYRICS_JA: &str = "\
[00:38.00]朝をたたんで小さな形にした
[00:43.50]港の壁のそばで水に浮かべた
[00:49.00]端ににじんだインク、昔の名前
[00:54.50]流れを見つけて、遠くへ行った
[01:00.00]紙の舟よ、言えなかった言葉を運んで
[01:05.50]灰色の海をこえて湾の向こうへ
[01:11.00]潮がやさしければ光を見つける
[01:16.50]桟橋のランタンは夜通し灯る
[01:22.00]小さな帆はどれも出せない手紙
[01:27.50]小さな波はどれも誰かの想い
[01:33.00]だから また ひとつ おって ながすよ
[01:38.50]紙の舟よ、いつかそこで会おう OK
";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Scene {
    IdleClock,
    IdleEyes,
    Hover,
    Timer,
    Media,
    FocusNotice,
    VolumeHud,
    ChargingHud,
    LowBatteryHud,
    Page(PageKind),
    ShelfItems,
    System(Module),
    IdleStatus,
    MediaNoLine,
    /// 普通日语歌词，不包含注音或额外资源。
    Japanese,
    HeadphonesHud,
    Oyenza,
    OyenzaCode,
    OyenzaSignedOut,
    OyenzaNotice,
    SettingsSignedIn,
    /// 合并后的日历和任务：有在线事件、选中日期、空任务及最窄宽度。
    TodayDay,
    TodayPicked,
    TodayEmpty,
    TodayNarrow,
    UpdateSheet,
    UpdateReady,
    MailEmpty,
    MailInbox,
    MailPreview,
    MailAccountMenu,
    MediaLyricsOff,
    SaveFailed,
    QuitSaveFailed,
    SaveProtected,
    CodexDisabled,
    CodexMessages,
    CodexWaiting,
    CodexExpired,
    Bluetooth(BluetoothScene),
    Diorama(Rest),
}

/// 蓝牙页的离线展示数据；只核对实际共享 Canvas，不代表真实设备读数。
#[derive(Clone, Copy, Debug, PartialEq)]
enum BluetoothScene {
    Devices,
    Unknown,
    Loading,
    Refreshing,
    Empty,
    Failed,
    FailedStale,
    Warning,
    Paused,
    PausedEmpty,
    Saver,
    StaleEmpty,
    Scrolled,
    Navigation,
    NavigationCodex,
}

impl BluetoothScene {
    fn name(self) -> &'static str {
        match self {
            Self::Devices => "devices",
            Self::Unknown => "unknown",
            Self::Loading => "loading",
            Self::Refreshing => "refreshing",
            Self::Empty => "empty",
            Self::Failed => "failed",
            Self::FailedStale => "failed-stale",
            Self::Warning => "warning",
            Self::Paused => "paused",
            Self::PausedEmpty => "paused-empty",
            Self::Saver => "saver",
            Self::StaleEmpty => "stale-empty",
            Self::Scrolled => "scrolled",
            Self::Navigation => "navigation",
            Self::NavigationCodex => "navigation-codex",
        }
    }
}

/// What a picture of the Diorama theme's resting pill shows: dawn (06:10),
/// the morning (10:08), the golden hour (17:40), night (21:30) and the
/// small hours (02:30); then the morning written on a 12-hour clock, as the
/// Minimal look (the place alone) and under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Rest {
    Dawn,
    Morning,
    GoldenHour,
    Night,
    SmallHours,
    TwelveHour,
    Minimal,
    Hover,
}

impl Rest {
    /// The picture's time of day: its hour and minute.
    fn time(self) -> (u8, u8) {
        match self {
            Rest::Dawn => (6, 10),
            Rest::Morning | Rest::TwelveHour | Rest::Minimal | Rest::Hover => (10, 8),
            Rest::GoldenHour => (17, 40),
            Rest::Night => (21, 30),
            Rest::SmallHours => (2, 30),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Rest::Dawn => "idle-dawn",
            Rest::Morning => "idle-morning",
            Rest::GoldenHour => "idle-golden-hour",
            Rest::Night => "idle-night",
            Rest::SmallHours => "idle-small-hours",
            Rest::TwelveHour => "idle-12-hour",
            Rest::Minimal => "idle-minimal",
            Rest::Hover => "idle-clock-hover",
        }
    }
}

const SCENES: [Scene; 15] = [
    Scene::IdleClock,
    Scene::IdleEyes,
    Scene::Hover,
    Scene::Timer,
    Scene::Media,
    Scene::FocusNotice,
    Scene::VolumeHud,
    Scene::Page(PageKind::Focus),
    Scene::Page(PageKind::Today),
    Scene::Page(PageKind::Media),
    Scene::Page(PageKind::Shelf),
    Scene::OyenzaSignedOut,
    Scene::ChargingHud,
    Scene::LowBatteryHud,
    Scene::ShelfItems,
];

const SYSTEM_SCENES: [Scene; 6] =
    [Scene::System(Module::Cpu), Scene::System(Module::Gpu), Scene::System(Module::Memory), Scene::System(Module::Drives), Scene::System(Module::Network), Scene::System(Module::Battery)];

const PILL_SCENES: [Scene; 2] = [Scene::IdleStatus, Scene::MediaNoLine];
const SETTINGS_SCENES: [Scene; 1] = [Scene::Page(PageKind::Settings)];
const HEADPHONE_SCENES: [Scene; 1] = [Scene::HeadphonesHud];
const OYENZA_SCENES: [Scene; 4] = [Scene::Oyenza, Scene::OyenzaCode, Scene::OyenzaNotice, Scene::SettingsSignedIn];
const POWER_SCENES: [Scene; 1] = [Scene::System(Module::Power)];
const TODAY_SCENES: [Scene; 4] = [Scene::TodayDay, Scene::TodayPicked, Scene::TodayEmpty, Scene::TodayNarrow];
const UPDATE_SCENES: [Scene; 2] = [Scene::UpdateSheet, Scene::UpdateReady];
const MAIL_SCENES: [Scene; 3] = [Scene::MailEmpty, Scene::MailInbox, Scene::MailPreview];
const CODEX_SCENES: [Scene; 4] = [Scene::CodexDisabled, Scene::CodexMessages, Scene::CodexWaiting, Scene::CodexExpired];
const BLUETOOTH_SCENES: [BluetoothScene; 15] = [
    BluetoothScene::Devices,
    BluetoothScene::Unknown,
    BluetoothScene::Loading,
    BluetoothScene::Refreshing,
    BluetoothScene::Empty,
    BluetoothScene::Failed,
    BluetoothScene::FailedStale,
    BluetoothScene::Warning,
    BluetoothScene::Paused,
    BluetoothScene::PausedEmpty,
    BluetoothScene::Saver,
    BluetoothScene::StaleEmpty,
    BluetoothScene::Scrolled,
    BluetoothScene::Navigation,
    BluetoothScene::NavigationCodex,
];

/// The Diorama theme's resting pill, in that theme only, after the peek
/// strip's number (which a run without a notch leaves free), so every other
/// picture keeps its number.
const DIORAMA_SCENES: [Scene; 8] = [
    Scene::Diorama(Rest::Dawn),
    Scene::Diorama(Rest::Morning),
    Scene::Diorama(Rest::GoldenHour),
    Scene::Diorama(Rest::Night),
    Scene::Diorama(Rest::SmallHours),
    Scene::Diorama(Rest::TwelveHour),
    Scene::Diorama(Rest::Minimal),
    Scene::Diorama(Rest::Hover),
];

impl Scene {
    fn name(self) -> String {
        match self {
            Scene::IdleClock => "idle-clock".into(),
            Scene::IdleEyes => "idle-eyes".into(),
            Scene::Hover => "idle-hover".into(),
            Scene::Timer => "timer-activity".into(),
            Scene::Media => "media-activity".into(),
            Scene::FocusNotice => "focus-notice".into(),
            Scene::VolumeHud => "volume-hud".into(),
            Scene::ChargingHud => "charging-hud".into(),
            Scene::LowBatteryHud => "low-battery-hud".into(),
            Scene::ShelfItems => "page-shelf-items".into(),
            Scene::Page(p) => format!("page-{}", p.key()),
            Scene::System(Module::Cpu) => "page-system".into(),
            Scene::System(m) => format!("page-system-{}", m.title().to_lowercase()),
            Scene::IdleStatus => "idle-status".into(),
            Scene::MediaNoLine => "media-no-line".into(),
            Scene::Japanese => "page-media-japanese".into(),
            Scene::HeadphonesHud => "headphones-hud".into(),
            Scene::Oyenza => "page-oyenza".into(),
            Scene::OyenzaSignedOut => "page-oyenza-signed-out".into(),
            Scene::OyenzaCode => "page-oyenza-code".into(),
            Scene::OyenzaNotice => "oyenza-notice".into(),
            Scene::SettingsSignedIn => "page-settings-signed-in".into(),
            Scene::TodayDay => "page-today-day".into(),
            Scene::TodayPicked => "page-today-picked".into(),
            Scene::TodayEmpty => "page-today-empty".into(),
            Scene::TodayNarrow => "page-today-narrow".into(),
            Scene::UpdateSheet => "page-update-sheet".into(),
            Scene::UpdateReady => "page-update-ready".into(),
            Scene::MailEmpty => "page-mail-empty".into(),
            Scene::MailInbox => "page-mail-inbox".into(),
            Scene::MailPreview => "page-mail-preview".into(),
            Scene::MailAccountMenu => "page-mail-accounts".into(),
            Scene::MediaLyricsOff => "page-media-lyrics-off".into(),
            Scene::SaveFailed => "page-save-failed".into(),
            Scene::QuitSaveFailed => "page-quit-save-failed".into(),
            Scene::SaveProtected => "page-save-protected".into(),
            Scene::CodexDisabled => "codex-disabled".into(),
            Scene::CodexMessages => "codex-messages".into(),
            Scene::CodexWaiting => "codex-waiting".into(),
            Scene::CodexExpired => "codex-expired-draft".into(),
            Scene::Bluetooth(scene) => format!("bluetooth-{}", scene.name()),
            Scene::Diorama(r) => r.name().into(),
        }
    }
}

pub fn run(dir: &str, args: &[String]) -> i32 {
    let out = PathBuf::from(dir);
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("render: {}: {e}", out.display());
        return 2;
    }
    let theme = match args.iter().position(|a| a == "--theme").and_then(|i| args.get(i + 1)).map(|s| s.as_str()) {
        Some("oyen") => Theme::Oyen,
        Some("glass") => Theme::Glass,
        Some("pill") => Theme::Pill,
        Some("diorama") => Theme::Diorama,
        _ => Theme::Classic,
    };
    // 与产品实际支持的宽度一致，离屏验收无需改写用户设置。
    let width = match args.iter().position(|a| a == "--width") {
        Some(index) => match args.get(index + 1).and_then(|value| value.parse::<f32>().ok()) {
            Some(value) if value.is_finite() && (480.0..=900.0).contains(&value) => Some(value),
            _ => { eprintln!("render: --width must be between 480 and 900 DIPs"); return 2; }
        },
        None => None,
    };
    let notch = args.iter().any(|a| a == "--notch").then_some(Notch::MACBOOK);
    let name = match theme {
        Theme::Classic => "",
        Theme::Oyen => "-oyen",
        Theme::Glass => "-glass",
        Theme::Pill => "-pill",
        Theme::Diorama => "-diorama",
    };
    let suffix = format!("{name}{}", if notch.is_some() { "-notch" } else { "" });
    // A home of its own, so saved settings on this Mac don't change the pictures.
    let home = std::env::temp_dir().join(format!("poni-render-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::env::set_var("PONI_HOME", &home);
    let mut failed = 0;
    let mut gfx = Gfx::new(SCALE);
    if args.iter().any(|arg| arg == "--mail-error-preview") {
        for light in [false, true] {
            let path = out.join(if light { "mail-error-light.png" } else { "mail-error-dark.png" });
            let error = Some(MailConnection::Error("邮箱认证失败，请检查 IMAP 开关、授权码或应用专用密码".into()));
            if let Err(error) = render_settings(&mut gfx, crate::settings_model::Section::Mail, theme, light, false, false, error, &path) {
                eprintln!("render: {error}");
                failed += 1;
            }
        }
        let _ = std::fs::remove_dir_all(&home);
        return if failed > 0 { 1 } else { 0 };
    }
    if args.iter().any(|arg| arg == "--codex") {
        for scene in CODEX_SCENES {
            let path = out.join(format!("{}{suffix}.png", scene.name()));
            match render_scene(&mut gfx, scene, theme, notch, width, &path) {
                Ok(()) => println!("render: {}", path.display()),
                Err(e) => {
                    eprintln!("render: {}: {e}", path.display());
                    failed += 1;
                }
            }
        }
        let _ = std::fs::remove_dir_all(&home);
        return if failed > 0 { 1 } else { 0 };
    }
    if args.iter().any(|arg| arg == "--bluetooth") {
        for scene in BLUETOOTH_SCENES {
            let scene = Scene::Bluetooth(scene);
            let path = out.join(format!("{}{suffix}.png", scene.name()));
            match render_scene(&mut gfx, scene, theme, notch, width, &path) {
                Ok(()) => println!("render: {}", path.display()),
                Err(error) => {
                    eprintln!("render: {}: {error}", path.display());
                    failed += 1;
                }
            }
        }
        let _ = std::fs::remove_dir_all(&home);
        return if failed > 0 { 1 } else { 0 };
    }
    for (i, scene) in SCENES.into_iter().chain(SYSTEM_SCENES).enumerate() {
        let path = out.join(format!("{:02}-{}{suffix}.png", i + 1, scene.name()));
        match render_scene(&mut gfx, scene, theme, notch, width, &path) {
            Ok(()) => println!("render: {}", path.display()),
            Err(e) => {
                eprintln!("render: {}: {e}", path.display());
                failed += 1;
            }
        }
    }
    // Pill: how the floating outline moves (stills can't show it), then its own looks.
    if crate::pill::on(theme) {
        let n = SCENES.len() + SYSTEM_SCENES.len() + 1;
        let path = out.join(format!("{n:02}-open-close-frames{suffix}.png"));
        match render_morph(&mut gfx, theme, notch, &path) {
            Ok(()) => println!("render: {}", path.display()),
            Err(e) => {
                eprintln!("render: {}: {e}", path.display());
                failed += 1;
            }
        }
        for (k, scene) in PILL_SCENES.into_iter().enumerate() {
            // The status bar is a look only the Pill theme offers.
            if scene == Scene::IdleStatus && !crate::pill::status::looks(theme).contains(&IdleStyle::Status) {
                continue;
            }
            let path = out.join(format!("{:02}-{}{suffix}.png", n + 1 + k, scene.name()));
            match render_scene(&mut gfx, scene, theme, notch, width, &path) {
                Ok(()) => println!("render: {}", path.display()),
                Err(e) => {
                    eprintln!("render: {}: {e}", path.display());
                    failed += 1;
                }
            }
        }
    }
    // The remaining pages and ordinary Japanese lyrics in every theme.
    let first = SCENES.len() + SYSTEM_SCENES.len() + 2 + PILL_SCENES.len();
    let mut next = first;
    let later = SETTINGS_SCENES.into_iter().chain(HEADPHONE_SCENES).chain(OYENZA_SCENES).chain(POWER_SCENES).chain(TODAY_SCENES).chain(UPDATE_SCENES).chain([Scene::Japanese]);
    for (k, scene) in later.enumerate() {
        let path = out.join(format!("{:02}-{}{suffix}.png", first + k, scene.name()));
        match render_scene(&mut gfx, scene, theme, notch, width, &path) {
            Ok(()) => println!("render: {}", path.display()),
            Err(e) => {
                eprintln!("render: {}: {e}", path.display());
                failed += 1;
            }
        }
        next = first + k + 1;
    }
    // The Settings window's sections, dark, two of them light, and Island
    // with Stay on top picked.
    for (k, (section, light, on_top, system_on)) in settings_shots().into_iter().enumerate() {
        let mode = if light { "-light" } else { "" };
        let pick = if on_top { "-on-top" } else { "" };
        let system = if system_on { "-system-on" } else { "" };
        let path = out.join(format!("{:02}-settings-window-{}{pick}{system}{mode}{suffix}.png", next + k, section.key()));
        match render_settings(&mut gfx, section, theme, light, on_top, system_on, None, &path) {
            Ok(()) => println!("render: {}", path.display()),
            Err(e) => {
                eprintln!("render: {}: {e}", path.display());
                failed += 1;
            }
        }
    }
    let next = next + settings_shots().len();
    // Around a notch, last of every theme's: the Eyes look's hide and seek with the camera.
    if let Some(n) = notch {
        let path = out.join(format!("{next:02}-peek-frames{suffix}.png"));
        match render_peek(&mut gfx, theme, n, &path) {
            Ok(()) => println!("render: {}", path.display()),
            Err(e) => {
                eprintln!("render: {}: {e}", path.display());
                failed += 1;
            }
        }
    }
    // Diorama's resting pill through a day, after the peek strip's number.
    if theme == Theme::Diorama {
        for (k, scene) in DIORAMA_SCENES.into_iter().enumerate() {
            let path = out.join(format!("{:02}-{}{suffix}.png", next + 1 + k, scene.name()));
            match render_scene(&mut gfx, scene, theme, notch, width, &path) {
                Ok(()) => println!("render: {}", path.display()),
                Err(e) => {
                    eprintln!("render: {}: {e}", path.display());
                    failed += 1;
                }
            }
        }
    }
    // 新邮件场景放在既有编号之后，保留之前各场景的图片路径。
    let mail_first = next + 1 + DIORAMA_SCENES.len();
    for (index, scene) in MAIL_SCENES.into_iter().chain([Scene::MailAccountMenu, Scene::MediaLyricsOff, Scene::SaveFailed, Scene::QuitSaveFailed, Scene::SaveProtected]).enumerate() {
        let path = out.join(format!("{:02}-{}{suffix}.png", mail_first + index, scene.name()));
        match render_scene(&mut gfx, scene, theme, notch, width, &path) {
            Ok(()) => println!("render: {}", path.display()),
            Err(error) => {
                eprintln!("render: {}: {error}", path.display());
                failed += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&home);
    if failed > 0 {
        1
    } else {
        0
    }
}

/// The Settings window's pictures: every section dark, General and
/// Appearance light too, and Island with In full-screen apps set to Stay on
/// top. Each is (section, light, Stay on top, System enabled).
fn settings_shots() -> Vec<(crate::settings_model::Section, bool, bool, bool)> {
    use crate::settings_model::Section;
    let mut v: Vec<_> = Section::ALL.into_iter().map(|s| (s, false, false, false)).collect();
    v.extend([(Section::General, true, false, false), (Section::General, false, false, true), (Section::Appearance, true, false, false), (Section::Island, false, true, false)]);
    v
}

/// One section of the Settings window as the Mac shows it, light or dark,
/// with the system's window buttons where macOS puts them (the real window
/// draws those itself; the smoke test pictures that one). `on_top`: In
/// full-screen apps set to Stay on top instead of Hide; `system_on` shows
/// the System switch enabled with its history setting.
fn render_settings(gfx: &mut Gfx, section: crate::settings_model::Section, theme: Theme, light: bool, on_top: bool, system_on: bool, mail_error: Option<MailConnection>, path: &Path) -> Result<(), String> {
    use crate::settings_host::{HEIGHT, TITLEBAR, WIDTH};
    let (w, h) = (WIDTH as f32, HEIGHT as f32);
    let (w_px, h_px) = ((w * SCALE) as usize, (h * SCALE) as usize);
    let bmp = Bitmap::new(w_px, h_px, SCALE).ok_or("no bitmap")?;
    let mut st = AppState::new(Settings { theme, hide_in_fullscreen: !on_top, ..Settings::default() }, Store::default());
    st.settings.set_system(system_on);
    st.accent = crate::app::theme_accent(theme);
    st.ui.autostart = Some(true);
    if section == crate::settings_model::Section::Mail {
        sample_mail(&mut st);
        if let Some(error) = mail_error {
            if let Some(account) = st.settings.mail.accounts.first() {
                st.mail.expanded_accounts.insert(account.id.clone());
                st.mail.statuses.insert(account.id.clone(), error);
            }
        }
    }
    let mut view = crate::settings_window::View::new(section);
    let frame = crate::settings_window::Frame { card: Rect::new(0.0, 0.0, w, h), own_chrome: false, top: TITLEBAR, still: true };
    gfx.set_look(poni_core::look::glass(light));
    {
        let mut c = gfx.canvas(bmp.ctx());
        let _ = view.draw(&mut c, &mut st, NOW_MONO, frame, light);
        // Close, minimise and zoom, 12 points across and 20 apart from 20 in.
        c.unthemed(|c| {
            for (i, rgb) in [0xFF5F57u32, 0xFEBC2E, 0x28C840].into_iter().enumerate() {
                c.fill_circle(Point::new(20.0 + 6.0 + i as f32 * 20.0, TITLEBAR / 2.0), 6.0, Color::hex(rgb));
            }
        });
    }
    let px = bmp.pixels();
    crate::png::save_pbgra(path, w_px as u32, h_px as u32, &px)
}

/// The island opening from rest onto the Today page (left column) and
/// closing again (right), a frame every few sixtieths of a second, stepped on
/// the app's own springs: for the Pill theme, how the floating outline moves
/// and, around a notch, how it lets go of the edge and comes back.
fn render_morph(gfx: &mut Gfx, theme: Theme, notch: Option<Notch>, path: &Path) -> Result<(), String> {
    /// Sixtieths of a second after the change, one row each.
    const FRAMES: [usize; 12] = [0, 2, 4, 6, 8, 10, 13, 16, 20, 25, 30, 45];
    const DT: f32 = 1.0 / 60.0;
    /// The strip is drawn at one pixel per DIP (it is long).
    const STRIP_SCALE: f32 = 1.0;
    // Diorama's resting pill, where the island closes to: its ball at rest.
    crate::pill::diorama::settle();
    let mut st = base_state(theme);
    st.page = PageKind::Today;
    let (rest, open) = (Mode::Idle, Mode::Expanded(PageKind::Today));
    let size = |m: &Mode, st: &AppState| {
        let g = activities::geom(m, st);
        notch.map_or(g, |n| notch::geom(m, st, n, g))
    };
    let gap_at = |m: &Mode, st: &AppState| notch.map_or(Some(crate::pill::TOP_GAP), |n| notch::pill_gap(m, st, n));
    let ends = [(rest, size(&rest, &st), gap_at(&rest, &st)), (open, size(&open, &st), gap_at(&open, &st))];
    let menu_h = notch.map_or(MENU_BAR, |n| n.h);
    let cell_w = st.settings.expanded_width + 2.0 * MARGIN_X;
    let cell_h = (notch.map_or(0.0, |n| n.h) + crate::pill::TOP_GAP + ends[1].1.h * 1.06 + 28.0).ceil();
    let (w_px, h_px) = ((2.0 * cell_w * STRIP_SCALE).ceil() as usize, (FRAMES.len() as f32 * cell_h * STRIP_SCALE).ceil() as usize);
    let bmp = Bitmap::new(w_px, h_px, STRIP_SCALE).ok_or("no bitmap context")?;
    let look = if notch.is_some() { poni_core::look::around_notch(theme) } else { poni_core::look::of(theme) };
    for (col, (from, to)) in [(0usize, 1usize), (1, 0)].into_iter().enumerate() {
        let (_, g_from, gap_from) = ends[from];
        let (to_mode, g_to, gap_to) = ends[to];
        let mut island = Island::new(g_from);
        island.set_floating(gap_from);
        island.snap(g_from);
        // A first (still) frame, so the new gap below slides instead of snapping.
        island.step(0.0, NOW_MONO);
        island.set_target(g_to, NOW_MONO, true);
        island.set_floating(gap_to);
        let (mut k, mut now) = (0usize, NOW_MONO);
        for (row, &f) in FRAMES.iter().enumerate() {
            while k < f {
                island.step(DT, now);
                now += DT as f64;
                k += 1;
            }
            let (x, y) = (col as f32 * cell_w, row as f32 * cell_h);
            let cx = cell_w / 2.0;
            let body = island.body_rect_at(cx);
            let geom = island.current();
            let shoulder = notch.map_or(SHOULDER, |n| notch::shoulder(n, geom.w));
            let detach = notch::detach(island.top(), notch);
            gfx.set_look(&poni_core::look::CLASSIC);
            {
                let mut c = gfx.canvas(bmp.ctx());
                c.translate(x, y);
                c.clip(Rect::new(0.0, 0.0, cell_w, cell_h));
                let steps = 24;
                for i in 0..steps {
                    let t = i as f32 / (steps - 1) as f32;
                    let shade = Color::lerp(Color::hex(0x2B4F86), Color::hex(0xF0708A), t);
                    c.fill_rect(Rect::new(0.0, cell_h * i as f32 / steps as f32, cell_w, cell_h / steps as f32 + 0.5), shade);
                }
                c.fill_rect(Rect::new(0.0, 0.0, cell_w, menu_h), Color::rgba(0.96, 0.96, 0.97, 0.78));
                c.unclip();
                c.reset_transform();
            }
            gfx.set_look(look);
            {
                let mut c = gfx.canvas(bmp.ctx());
                c.translate(x, y);
                // Each frame keeps to its cell (the notch is drawn from above it).
                c.clip(Rect::new(0.0, 0.0, cell_w, cell_h));
                let border = Some(c.look().border);
                if island.floating() {
                    notch::draw_pill_body(&mut c, body, geom.r, shoulder, detach, palette::ISLAND, border);
                    c.push_outline_mask(&notch::pill_outline(body, geom.r, shoulder, detach));
                } else {
                    c.notch(body, geom.r, shoulder, palette::ISLAND, border);
                    c.push_shape_mask(body, geom.r, shoulder);
                }
                // The incoming content as it fades in (the app also fades the old one out).
                let lay = match to_mode {
                    Mode::Expanded(_) => Rect::new(body.center().x - g_to.w / 2.0, body.y, g_to.w, g_to.h),
                    _ => body,
                };
                let (mut input, mut hits) = (Input::default(), Vec::<Hit>::new());
                let alpha = island.content_alpha();
                c.with_alpha(alpha, |c| crate::app::draw_content(c, &mut input, &mut hits, &mut st, to_mode, lay, body, None, now, notch));
                c.pop_shape_mask();
                if let Some(n) = notch {
                    c.unthemed(|c| c.fill_rrect(Rect::new(cx - n.w / 2.0, -12.0, n.w, n.h + 12.0), 10.0, Color::black(1.0)));
                }
                let label = format!("{} {:.0} ms", if to == 1 { "open" } else { "close" }, f as f32 * 1000.0 / 60.0);
                c.unthemed(|c| c.text(&label, Rect::new(10.0, cell_h - 22.0, 160.0, 16.0), &crate::gfx::TextStyle::caption().color(Color::white(0.92))));
                c.unclip();
                c.reset_transform();
            }
        }
    }
    let px = bmp.pixels();
    crate::png::save_pbgra(path, w_px as u32, h_px as u32, &px)
}

/// The Eyes look around notch `n` as it moves (stills can't show it), a
/// frame a row, a fifteenth of a second apart: the pointer (drawn, so each
/// frame says what the eyes follow) wanders right of the camera and crosses
/// over; the eyes (Oyen, in his theme) duck behind the camera and come out
/// on its left, follow it up into the menu bar and back under the camera.
fn render_peek(gfx: &mut Gfx, theme: Theme, n: Notch, path: &Path) -> Result<(), String> {
    /// The pointer from the island's top centre (DIPs) at each time
    /// (seconds), in a straight line from one to the next.
    const WAY: [(f64, f32, f32); 7] = [(0.0, 250.0, 118.0), (0.6, 170.0, 70.0), (1.2, -60.0, 95.0), (1.5, -130.0, 105.0), (2.4, -235.0, 80.0), (3.0, -215.0, 14.0), (3.6, -40.0, 118.0)];
    const FPS: f64 = 15.0;
    const CELL: (f32, f32) = (560.0, 140.0);
    let mut st = base_state(theme);
    st.settings.idle_style = IdleStyle::Eyes;
    let mode = Mode::Idle;
    let g = notch::geom(&mode, &st, n, activities::geom(&mode, &st));
    let frames = (WAY[WAY.len() - 1].0 * FPS).round() as usize + 1;
    let (w_px, h_px) = ((CELL.0 * SCALE).ceil() as usize, (frames as f32 * CELL.1 * SCALE).ceil() as usize);
    let bmp = Bitmap::new(w_px, h_px, SCALE).ok_or("no bitmap context")?;
    let look = poni_core::look::around_notch(theme);
    let at = |t: f64| {
        let i = WAY.iter().rposition(|w| w.0 <= t).unwrap_or(0).min(WAY.len() - 2);
        let (a, b) = (WAY[i], WAY[i + 1]);
        let k = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) as f32;
        Point::new(a.1 + (b.1 - a.1) * k, a.2 + (b.2 - a.2) * k)
    };
    for f in 0..frames {
        let t = f as f64 / FPS;
        st.now = NOW_MONO + t;
        let p = at(t);
        st.ui.peek.point(p, st.now, n.w / 2.0);
        let (x, y) = (0.0, f as f32 * CELL.1);
        let cx = CELL.0 / 2.0;
        let mut island = Island::new(g);
        island.snap(g);
        let body = island.body_rect_at(cx);
        gfx.set_look(&poni_core::look::CLASSIC);
        {
            let mut c = gfx.canvas(bmp.ctx());
            c.translate(x, y);
            c.clip(Rect::new(0.0, 0.0, CELL.0, CELL.1));
            let steps = 24;
            for i in 0..steps {
                let k = i as f32 / (steps - 1) as f32;
                let shade = Color::lerp(Color::hex(0x2B4F86), Color::hex(0xF0708A), k);
                c.fill_rect(Rect::new(0.0, CELL.1 * i as f32 / steps as f32, CELL.0, CELL.1 / steps as f32 + 0.5), shade);
            }
            c.fill_rect(Rect::new(0.0, 0.0, CELL.0, n.h), Color::rgba(0.96, 0.96, 0.97, 0.78));
            c.unclip();
            c.reset_transform();
        }
        gfx.set_look(look);
        {
            let mut c = gfx.canvas(bmp.ctx());
            c.translate(x, y);
            c.clip(Rect::new(0.0, 0.0, CELL.0, CELL.1));
            let shoulder = notch::shoulder(n, g.w);
            if look.material == poni_core::look::Material::Glass {
                crate::glass::draw_body(&mut c, body, g.r, shoulder, Some(n), false, true);
            } else {
                let border = Some(c.look().border);
                c.notch(body, g.r, shoulder, palette::ISLAND, border);
            }
            let (mut input, mut hits) = (Input::default(), Vec::<Hit>::new());
            let now = st.now;
            crate::app::draw_content(&mut c, &mut input, &mut hits, &mut st, mode, body, body, None, now, Some(n));
            c.unthemed(|c| {
                c.fill_rrect(Rect::new(cx - n.w / 2.0, -12.0, n.w, n.h + 12.0), 10.0, Color::black(1.0));
                pointer_arrow(c, Point::new(cx + p.x, p.y));
            });
            c.unclip();
            c.reset_transform();
        }
    }
    let px = bmp.pixels();
    crate::png::save_pbgra(path, w_px as u32, h_px as u32, &px)
}

/// The Mac's arrow pointer, its tip at `tip`: black, outlined in white.
fn pointer_arrow(c: &mut crate::gfx::Canvas, tip: Point) {
    use poni_core::svgpath::Seg;
    const ARROW: [(f32, f32); 7] = [(0.0, 0.0), (0.0, 16.0), (4.0, 12.3), (6.9, 18.6), (9.6, 17.4), (6.8, 11.3), (12.0, 11.3)];
    let mut segs: Vec<Seg> = ARROW.iter().enumerate().map(|(i, (dx, dy))| if i == 0 { Seg::MoveTo(tip.x + dx, tip.y + dy) } else { Seg::LineTo(tip.x + dx, tip.y + dy) }).collect();
    segs.push(Seg::Close);
    c.path(&segs, Some(Color::white(1.0)), Some((Color::white(1.0), 2.4)));
    c.path(&segs, Some(Color::black(1.0)), None);
}

fn base_state(theme: Theme) -> AppState {
    let settings = Settings { theme, focus_sound: false, shelf_sound: false, codex_sound: false, show_media_activity: true, lyrics: true, ..Default::default() };
    let mut st = AppState::new(settings, Store::default());
    st.now = NOW_MONO;
    st.unix = NOW_UNIX;
    st.local = LocalDateTime { year: 2026, month: 10, day: 24, hour: 9, minute: 41, second: 0 };
    st.accent = if theme == Theme::Oyen { crate::oyen::ORANGE } else { palette::BLUE };
    st.volume = VolumeState { level: 0.64, muted: false, known: true, adjustable: true, can_mute: true };
    st.power = PowerState { has_battery: true, percent: 76, charging: false, on_ac: false, saver: false };
    // 既有今天场景使用离线天气，核对温度与图标，不发起网络请求。
    st.weather.snapshot = Some(crate::state::WeatherSnapshot { place: "杭州".into(), temperature_c: 21.0, code: 2, is_day: true, updated_unix: NOW_UNIX });
    seed(&mut st);
    st
}

/// The picture's clock at `hour`:`minute` of its day, instead of 09:41.
fn set_clock(st: &mut AppState, hour: u8, minute: u8) {
    let was = st.local.hour as i64 * 3600 + st.local.minute as i64 * 60;
    st.local.hour = hour;
    st.local.minute = minute;
    st.unix += hour as i64 * 3600 + minute as i64 * 60 - was;
}

fn sample_bluetooth(st: &mut AppState, scene: BluetoothScene) {
    st.bluetooth.active = true;
    st.bluetooth.updated_at = Some(NOW_MONO - 30.0);
    st.bluetooth.devices = vec![
        BluetoothDevice {
            id: "render-audio".into(),
            name: "吴程的 AirPods Pro（第二代）".into(),
            kind: DeviceKind::Audio,
            battery: None,
            components: vec![
                BatteryComponent { label: "左耳".into(), percent: 82 },
                BatteryComponent { label: "右耳".into(), percent: 9 },
                BatteryComponent { label: "充电盒".into(), percent: 66 },
            ],
        },
        BluetoothDevice {
            id: "render-mouse".into(),
            name: "Logitech MX Master 3S 无线鼠标与办公设备".into(),
            kind: DeviceKind::Mouse,
            battery: Some(0),
            components: Vec::new(),
        },
        BluetoothDevice {
            id: "render-keyboard".into(),
            name: "Magic Keyboard with Touch ID — Extended Workstation".into(),
            kind: DeviceKind::Keyboard,
            battery: Some(100),
            components: Vec::new(),
        },
    ];
    match scene {
        BluetoothScene::Devices => {}
        BluetoothScene::Unknown => {
            st.bluetooth.devices[0].components.clear();
            st.bluetooth.devices[1].battery = Some(20);
            st.bluetooth.devices[2].battery = None;
        }
        BluetoothScene::Loading => {
            st.bluetooth.devices.clear();
            st.bluetooth.updated_at = None;
            st.bluetooth.loading = true;
        }
        BluetoothScene::Refreshing => {
            st.bluetooth.loading = true;
            st.bluetooth.stale = true;
        }
        BluetoothScene::Empty => st.bluetooth.devices.clear(),
        BluetoothScene::Failed | BluetoothScene::FailedStale => {
            st.bluetooth.error = Some("暂时无法读取蓝牙设备，请检查系统蓝牙状态后重试。".into());
            if scene == BluetoothScene::Failed {
                st.bluetooth.devices.clear();
                st.bluetooth.updated_at = None;
            } else {
                st.bluetooth.stale = true;
            }
        }
        BluetoothScene::Warning => {
            st.bluetooth.warning = Some("部分设备的电量暂时无法读取，请稍后刷新。".into());
            st.bluetooth.devices[1].battery = None;
        }
        BluetoothScene::Paused | BluetoothScene::PausedEmpty | BluetoothScene::Saver => {
            st.bluetooth.active = false;
            st.bluetooth.stale = true;
            if scene == BluetoothScene::PausedEmpty {
                st.bluetooth.devices.clear();
                st.bluetooth.updated_at = None;
                st.bluetooth.stale = false;
            }
            st.power.saver = scene == BluetoothScene::Saver;
        }
        BluetoothScene::StaleEmpty => {
            st.bluetooth.devices.clear();
            st.bluetooth.loading = true;
            st.bluetooth.stale = true;
        }
        BluetoothScene::Scrolled => {
            st.bluetooth.devices.extend([
                BluetoothDevice { id: "render-gamepad".into(), name: "无线游戏手柄 · 客厅".into(), kind: DeviceKind::Gamepad, battery: None, components: Vec::new() },
                BluetoothDevice { id: "render-other".into(), name: "会议室 Bluetooth 输入设备（长名称展示）".into(), kind: DeviceKind::Other, battery: Some(4), components: Vec::new() },
            ]);
            st.bluetooth.scroll = 2.0 * 56.0;
        }
        BluetoothScene::Navigation | BluetoothScene::NavigationCodex => {
            st.settings.set_system(true);
            st.notify_can = true;
            st.settings.use_24h = false;
            set_clock(st, 11, 59);
        }
    }
}

/// A later version than this build, with its DMG, for the Update sheet.
fn fake_release() -> poni_core::update::Release {
    let mut parts: Vec<u64> = crate::version::VERSION.split('.').filter_map(|p| p.parse().ok()).collect();
    if let Some(last) = parts.last_mut() {
        *last += 1;
    }
    let version = parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".");
    let repo = "https://github.com/lucacadalora/oyenza-studio/releases";
    poni_core::update::Release {
        date: "2026-10-27".into(),
        headline: "Updates install themselves on a Mac".into(),
        notes: format!("{repo}/tag/v{version}"),
        windows: None,
        mac: Some(poni_core::update::Download { url: format!("{repo}/download/v{version}/Oyenza-Studio-{version}-mac.dmg"), size: 4_300_000, sha256: "0".repeat(64) }),
        version,
    }
}

/// Oyenza for its scenes, all made up: today's and yesterday's
/// transactions (today's newest just landed, so it glows), bills due
/// tomorrow and later; or, signing in, the code to type in oyenza.app.
pub(crate) fn fake_oyenza(st: &AppState, connecting: bool) -> OyenzaState {
    let iso = |t: i64| {
        let (y, m, d) = poni_core::datetime::civil_from_days(t.div_euclid(86_400));
        let s = t.rem_euclid(86_400);
        format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s % 3600 / 60, s % 60)
    };
    let midnight = st.unix - (st.local.hour as i64 * 3600 + st.local.minute as i64 * 60 + st.local.second as i64);
    let at = |days_ago: i64, h: i64, m: i64| iso(midnight - days_ago * 86_400 + h * 3600 + m * 60);
    let txn = |id: &str, kind: &str, title: &str, text: &str, wallet: (&str, &str), at: String, auto: bool| Txn {
        id: id.into(),
        kind: kind.into(),
        title: title.into(),
        amount: 1.0,
        text: text.into(),
        category: None,
        wallet: Some(WalletRef { name: wallet.0.into(), color: wallet.1.into() }),
        to_wallet: None,
        at,
        auto,
    };
    let (bca, gopay, ovo) = (("BCA", "#0060AF"), ("GoPay", "#00AED6"), ("OVO", "#4C3494"));
    let day = |n: i64| st.local.add_days(n).day_key();
    let today = Day {
        date: day(0),
        count: 3,
        in_sum: 1_250_000.0,
        out: 63_000.0,
        in_text: "Rp 1.250.000".into(),
        out_text: "Rp 63.000".into(),
        rows: vec![
            txn("t3", "income", "Gaji Oktober", "Rp 1.250.000", bca, at(0, 9, 38), true),
            txn("t2", "expense", "Kopi susu", "Rp 25.000", gopay, at(0, 8, 51), false),
            txn("t1", "expense", "Grab ke kantor", "Rp 38.000", ovo, at(0, 8, 2), false),
        ],
    };
    let yesterday = Day {
        date: day(-1),
        count: 2,
        in_sum: 0.0,
        out: 390_000.0,
        in_text: "Rp 0".into(),
        out_text: "Rp 390.000".into(),
        rows: vec![txn("y2", "expense", "Makan siang", "Rp 40.000", gopay, at(1, 12, 40), false), txn("y1", "expense", "Listrik PLN", "Rp 350.000", bca, at(1, 10, 2), true)],
    };
    let empty = |n: i64| Day { date: day(-n), count: 0, in_sum: 0.0, out: 0.0, in_text: "Rp 0".into(), out_text: "Rp 0".into(), rows: Vec::new() };
    let days = vec![today, yesterday, empty(2), empty(3), empty(4), empty(5), empty(6)];
    // Everything was on the page a while ago except today's newest, which just landed.
    let mut seen = Seen::default();
    let mut before = days.clone();
    before[0].rows.remove(0);
    seen.note(&before, st.now - 120.0);
    seen.note(&days, st.now - 12.0);
    let link = if connecting {
        OyenzaLink::Connecting { code: "KPJX-TMNQ".into(), url: "https://oyenza.app/connect?code=KPJX-TMNQ".into(), until: st.now + 581.0 }
    } else {
        OyenzaLink::Connected { account: "a\u{2022}\u{2022}\u{2022}@example.com".into() }
    };
    let bill = |kind: &str, name: &str, due: String, text: Option<&str>| Bill { kind: kind.into(), name: name.into(), due, amount: text.map(|_| 1.0), text: text.map(Into::into) };
    OyenzaState {
        link,
        activity: Some(Activity { cursor: "42".into(), today: day(0), days }),
        seen,
        bills: vec![bill("subscription", "Spotify", day(1), Some("Rp 54.990")), bill("card", "Kartu Kredit BCA", day(1), None), bill("subscription", "Netflix", day(5), Some("Rp 54.000"))],
        bills_today: st.day_key(),
        updated_unix: Some(st.unix - 8),
        ..OyenzaState::default()
    }
}

fn seed(st: &mut AppState) {
    let unix = st.unix;
    let day_start = st.day_start_unix();
    let day = poni_core::datetime::days_from_civil(st.local.year, st.local.month, st.local.day);
    let done = st.store.tasks.add("Water the balcony plants", unix - 5 * 3600, day);
    st.store.tasks.add("Sketch the onboarding screens", unix - 2 * 86_400, day - 2);
    st.store.tasks.add("Book the ferry for Saturday", unix - 2 * 3600, day);
    if let Some(id) = done {
        st.store.tasks.toggle(id, (unix - 1800).max(day_start + 60), day);
    }
    st.store.focus_sessions.insert(st.day_key(), 2);
}

fn media(st: &mut AppState) {
    let art = ImageData::new(240, 240, make_art(240));
    let color = art.accent_color();
    st.media = Some(MediaSession {
        app_id: "com.apple.Music".into(),
        app_name: "Music".into(),
        content_id: String::new(),
        title: TITLE.into(),
        artist: ARTIST.into(),
        album: ALBUM.into(),
        art: Some(art),
        art_color: Some(color),
        timeline: Timeline::new(64.0, 201.0, NOW_MONO, true),
        can_prev: true,
        can_next: true,
        can_play_pause: true,
        can_seek: true,
        track_key: MediaSession::make_track_key(TITLE, ARTIST, ALBUM),
    });
    st.lyrics = LyricsState { track_key: MediaSession::make_track_key(TITLE, ARTIST, ALBUM), status: LyricsStatus::Found, lyrics: Some(parse_lrc(LYRICS_LRC)), ..Default::default() };
}

/// 仅在内存中填入虚构邮件，列表、预览和设置共用同一组模型。
fn sample_mail(st: &mut AppState) {
    st.settings.mail.accounts = vec![
        MailAccount {
            id: "demo-mail-qq".into(),
            name: "日常邮箱".into(),
            address: "daily@example.com".into(),
            provider: MailProvider::Qq,
            host: MailProvider::Qq.host().into(),
            privacy: MailPrivacy::CountOnly,
            ..MailAccount::default()
        },
        MailAccount {
            id: "demo-mail-gmail".into(),
            name: "订阅与通知".into(),
            address: "updates@example.com".into(),
            provider: MailProvider::Gmail,
            host: String::new(),
            privacy: MailPrivacy::SenderAndSubject,
            ..MailAccount::default()
        },
        MailAccount {
            id: "demo-mail-aruba".into(),
            name: "工作邮箱".into(),
            address: "team@example.com".into(),
            provider: MailProvider::Aruba,
            host: MailProvider::Aruba.host().into(),
            privacy: MailPrivacy::Preview,
            ..MailAccount::default()
        },
    ];
    st.settings.mail.quiet_during_focus = true;
    for (account_id, messages) in [
        (
            "demo-mail-qq",
            vec![
                MailMessage {
                    id: "qq-101".into(),
                    sender: "林间书店 <books@example.com>".into(),
                    subject: "周末阅读清单已更新".into(),
                    snippet: "三本适合慢慢读的新书，留一点时间给自己。".into(),
                    received_unix: st.unix - 120,
                    unread: true,
                    attachments: false,
                },
                MailMessage {
                    id: "qq-100".into(),
                    sender: "出行服务 <travel@example.com>".into(),
                    subject: "示例行程确认单".into(),
                    snippet: "行程安排与注意事项已整理到附件。".into(),
                    received_unix: st.unix - 14_400,
                    unread: false,
                    attachments: true,
                },
            ],
        ),
        (
            "demo-mail-gmail",
            vec![
                MailMessage {
                    id: "gmail-202".into(),
                    sender: "设计周刊 <design@example.com>".into(),
                    subject: "本周灵感：让信息更轻一点".into(),
                    snippet: "界面留白、层次与色彩的六个观察。".into(),
                    received_unix: st.unix - 900,
                    unread: true,
                    attachments: false,
                },
                MailMessage {
                    id: "gmail-201".into(),
                    sender: "社区通讯 <community@example.com>".into(),
                    subject: "欢迎查看本月社区动态".into(),
                    snippet: "新的讨论、活动与实用资源。".into(),
                    received_unix: st.unix - 86_400,
                    unread: false,
                    attachments: false,
                },
            ],
        ),
        (
            "demo-mail-aruba",
            vec![
                MailMessage {
                    id: "aruba-303".into(),
                    sender: "项目协作组 <project@example.com>".into(),
                    subject: "下周安排与项目进度（示例）".into(),
                    snippet: "请查看会议时间、待确认事项与附件中的进度说明。".into(),
                    received_unix: st.unix - 300,
                    unread: true,
                    attachments: true,
                },
                MailMessage {
                    id: "aruba-302".into(),
                    sender: "行政服务 <office@example.com>".into(),
                    subject: "办公信息更新（示例）".into(),
                    snippet: "本周办公安排与公共资源说明。".into(),
                    received_unix: st.unix - 3_600,
                    unread: false,
                    attachments: false,
                },
            ],
        ),
    ] {
        let unread = messages.iter().filter(|message| message.unread).count() as u32;
        st.mail.messages.insert(account_id.into(), messages);
        st.mail.unread_counts.insert(account_id.into(), unread);
        st.mail.statuses.insert(account_id.into(), MailConnection::Ready { unread });
    }
}

fn render_scene(gfx: &mut Gfx, scene: Scene, theme: Theme, notch: Option<Notch>, width: Option<f32>, path: &Path) -> Result<(), String> {
    // Diorama's glass ball rests where this picture's time has it, whatever
    // the picture before it showed.
    crate::pill::diorama::settle();
    let mut st = base_state(theme);
    let mut hovering = false;
    let mode = match scene {
        Scene::IdleClock => Mode::Idle,
        // Around a notch the eyes look at the pointer: down at it in the
        // window under the menu bar (Oyen rests again a while after it
        // moved), and over the notch while it hovers there.
        Scene::IdleEyes => {
            st.settings.idle_style = IdleStyle::Eyes;
            if let Some(n) = notch {
                st.ui.peek.point(Point::new(240.0, 320.0), NOW_MONO - 10.0, n.w / 2.0);
            }
            Mode::Idle
        }
        Scene::Hover => {
            st.settings.idle_style = IdleStyle::Eyes;
            hovering = true;
            if let Some(n) = notch {
                st.ui.peek.point(Point::new(20.0, n.h / 2.0), NOW_MONO - 0.5, n.w / 2.0);
            }
            Mode::Idle
        }
        Scene::Timer => {
            media(&mut st);
            st.pomodoro.phase = Phase::Focus;
            st.pomodoro.start(NOW_MONO + 1104.5 - st.pomodoro.current_duration());
            Mode::Activity(activities::Activity::MediaTimer)
        }
        Scene::Media => {
            media(&mut st);
            Mode::Activity(activities::Activity::Media)
        }
        Scene::FocusNotice => {
            st.pomodoro.phase = Phase::ShortBreak;
            st.notify(Icon::Coffee, palette::GREEN, "专注完成", "休息 5 分钟吧", 30.0, None);
            st.set_notice_inline(crate::state::InlineAction::StartFocus);
            Mode::Notice
        }
        Scene::VolumeHud => {
            st.hud = Some(Hud { kind: HudKind::Volume { level: 0.64, muted: false }, until: NOW_MONO + 30.0 });
            st.ui.hud_level = 0.64;
            Mode::Hud(activities::HudTag::Volume)
        }
        Scene::ChargingHud => {
            st.hud = Some(Hud { kind: HudKind::Charging { percent: 76, plugged: true }, until: NOW_MONO + 30.0 });
            Mode::Hud(activities::HudTag::Charging)
        }
        Scene::LowBatteryHud => {
            st.hud = Some(Hud { kind: HudKind::LowBattery { percent: 19 }, until: NOW_MONO + 30.0 });
            Mode::Hud(activities::HudTag::LowBattery)
        }
        Scene::HeadphonesHud => {
            st.hud = Some(Hud { kind: HudKind::Bluetooth { name: "AirPods Pro".into(), connected: true, battery: None, audio: true }, until: NOW_MONO + 30.0 });
            Mode::Hud(activities::HudTag::Bluetooth)
        }
        Scene::ShelfItems => {
            // Real files, with tiles from the Mac's own thumbnail code.
            let dir = std::env::temp_dir().join("poni-render-shelf");
            let folder = dir.join("Project assets");
            let _ = std::fs::create_dir_all(&folder);
            let notes = dir.join("Trip notes.txt");
            let _ = std::fs::write(&notes, "Weekend trip\n");
            let sketch = dir.join("Harbor sketch.png");
            let _ = crate::png::save_pbgra(&sketch, 240, 240, &make_art(240));
            let px = (crate::services::imaging::THUMB_DIP * SCALE).round() as u32;
            for (k, p) in [&folder, &notes, &sketch].into_iter().enumerate() {
                let path = p.to_string_lossy().into_owned();
                st.store.shelf.add_paths(&[path.as_str()], NOW_UNIX - 3600 + k as i64 * 600);
                let tile = crate::services::thumbs::make(&path, px);
                println!("render: shelf tile for {}: {}", p.display(), if tile.is_some() { "drawn" } else { "missing" });
                st.shelf_thumbs.insert(path, tile);
            }
            st.page = PageKind::Shelf;
            Mode::Expanded(PageKind::Shelf)
        }
        Scene::Page(p) => {
            if p == PageKind::Media {
                media(&mut st);
                // 复用普通音乐场景核对关闭歌词的紧凑布局；日语场景保留歌词列。
                st.settings.lyrics = false;
            }
            if p == PageKind::Focus {
                st.pomodoro.phase = Phase::Focus;
                st.pomodoro.start(NOW_MONO + 1104.5 - st.pomodoro.current_duration());
            }
            st.page = p;
            Mode::Expanded(p)
        }
        Scene::MediaLyricsOff => {
            media(&mut st);
            st.settings.lyrics = false;
            st.page = PageKind::Media;
            Mode::Expanded(PageKind::Media)
        }
        Scene::MailEmpty | Scene::MailInbox | Scene::MailPreview | Scene::MailAccountMenu => {
            if scene != Scene::MailEmpty {
                sample_mail(&mut st);
            }
            if scene == Scene::MailAccountMenu {
                st.mail.account_filter_open = true;
                st.mail.statuses.insert("demo-mail-gmail".into(), MailConnection::Syncing);
                st.mail.statuses.insert("demo-mail-aruba".into(), MailConnection::Error("示例连接失败".into()));
                st.mail.unread_counts.insert("demo-mail-qq".into(), 2);
                st.mail.unread_counts.insert("demo-mail-gmail".into(), 7);
                st.mail.unread_counts.insert("demo-mail-aruba".into(), 12);
            }
            if scene == Scene::MailPreview {
                let account_id = "demo-mail-aruba".to_string();
                let message = st.mail.messages[&account_id][0].clone();
                st.mail.selected_message = Some((account_id, message.id.clone()));
                st.mail.preview = Some(Ok(MailPreview {
                    message,
                    to: "团队成员 <team@example.com>".into(),
                    body: "大家好：\n\n这是一封用于检查邮件预览排版的虚构邮件。\n\n下周安排\n周一上午整理需求，周三下午进行方案讨论。会议开始前，请先阅读附件中的项目进度说明。\n\n待确认事项\n1. 本轮功能的验收范围。\n2. 下次沟通的时间与参与人员。\n3. 中文长段落、换行和滚动时是否清晰易读。\n\n附件仅展示文件名称，不会自动下载。所有地址均为 example.com 示例地址。\n\n祝工作顺利！\n项目协作组".into(),
                    attachments: vec![
                        MailAttachment { name: "项目进度说明（示例）.pdf".into(), size: Some(248_000) },
                        MailAttachment { name: "会议安排（示例）.ics".into(), size: Some(1_500) },
                    ],
                    truncated: false,
                }));
                // 展示正文末段及中文附件名，同时保留可回滚查看的长正文。
                st.mail.preview_scroll = 340.0;
            }
            st.page = PageKind::Mail;
            Mode::Expanded(PageKind::Mail)
        }
        Scene::UpdateSheet | Scene::UpdateReady => {
            st.update = crate::state::UpdateView { offer: Some(fake_release()), sheet: true, ready: scene == Scene::UpdateReady, ..Default::default() };
            st.page = PageKind::Settings;
            Mode::Expanded(PageKind::Settings)
        }
        Scene::TodayDay | Scene::TodayPicked | Scene::TodayEmpty | Scene::TodayNarrow => {
            use poni_core::agenda::{day_num, sample};
            if scene == Scene::TodayEmpty {
                st.store.tasks = Default::default();
            } else {
                st.oyenza = fake_oyenza(&st, false);
                let today = st.day_key();
                let now_min = st.local.hour as i32 * 60 + st.local.minute as i32;
                st.ui.today_pick = (scene == Scene::TodayPicked).then(|| day_num(&today).unwrap_or(0) + 10);
                st.oyenza.agenda = Some(sample(&today, now_min));
                st.oyenza.agenda_state = crate::state::AgendaState::Ready;
                st.oyenza.agenda_unix = Some(st.unix - 120);
            }
            if scene == Scene::TodayNarrow {
                st.settings.expanded_width = 480.0;
            }
            st.page = PageKind::Today;
            Mode::Expanded(PageKind::Today)
        }
        Scene::CodexDisabled | Scene::CodexMessages | Scene::CodexWaiting | Scene::CodexExpired => {
            use crate::codex::{Entry, Kind, Message, ReplyState};
            st.settings.codex_enabled = scene != Scene::CodexDisabled;
            st.codex.connected = st.settings.codex_enabled;
            st.settings.codex_preview = true;
            st.codex.bridge_status = "本机接入已就绪".into();
            if st.settings.codex_enabled {
                let samples = [
                    (
                        Kind::Question,
                        "确认日历交互",
                        "日期可以使用圆形底色标记待办。你希望已完成待办的日期也保留标记吗？\n\n回答只会返回这条正在等待的请求；需要审批的操作仍由 Codex 处理。",
                        "/Projects/Island",
                    ),
                    (Kind::Completed, "日历样式调整完成", "已调整日期间距和待办标记，保留现有农历显示与选中日期。", "/Projects/Island"),
                    (Kind::Approval, "等待检查操作", "请打开 Codex 查看命令内容，再决定是否允许执行。", "/Projects/Website"),
                    (Kind::Failed, "构建未完成", "本次构建发现错误，请回到 Codex 查看完整输出。", "/Projects/Notes"),
                ];
                for (index, (kind, title, text, cwd)) in samples.into_iter().enumerate() {
                    st.codex.entries.push(Entry {
                        preview: crate::codex_client::markdown::summary(text),
                        message: Message {
                            id: format!("render-codex-{index}"),
                            thread_id: format!("render-thread-{index}"),
                            turn_id: format!("render-turn-{index}"),
                            cwd: cwd.into(),
                            title: title.into(),
                            text: text.into(),
                            kind,
                        },
                        received_unix: NOW_UNIX - index as i64 * 120,
                        confirmed: false,
                        unread: true,
                        draft: if kind == Kind::Question { "仅保留未完成待办的日期标记。".into() } else { String::new() },
                        status: if kind == Kind::Question { ReplyState::Waiting } else { ReplyState::ReadOnly },
                    });
                }
                if matches!(scene, Scene::CodexWaiting | Scene::CodexExpired) {
                    st.codex.selected = Some(st.codex.entries[0].message.id.clone());
                    if scene == Scene::CodexExpired {
                        st.codex.entries[0].status = ReplyState::Expired;
                    }
                }
            }
            st.page = PageKind::Codex;
            Mode::Expanded(PageKind::Codex)
        }
        Scene::Bluetooth(scene) => {
            sample_bluetooth(&mut st, scene);
            let page = if scene == BluetoothScene::NavigationCodex { PageKind::Codex } else { PageKind::Bluetooth };
            st.page = page;
            Mode::Expanded(page)
        }
        Scene::SaveFailed | Scene::QuitSaveFailed | Scene::SaveProtected => {
            st.save_errors.settings = Some(if scene == Scene::SaveProtected { crate::persistence::SaveError::Protected } else { crate::persistence::SaveError::Write });
            st.save_errors.store = Some(crate::persistence::SaveError::Write);
            st.save_errors.quit_blocked = scene == Scene::QuitSaveFailed;
            st.page = PageKind::Settings;
            Mode::Expanded(PageKind::Settings)
        }
        Scene::Oyenza | Scene::OyenzaCode => {
            st.oyenza = fake_oyenza(&st, scene == Scene::OyenzaCode);
            st.page = PageKind::Oyenza;
            Mode::Expanded(PageKind::Oyenza)
        }
        Scene::OyenzaSignedOut => {
            st.page = PageKind::Oyenza;
            Mode::Expanded(PageKind::Oyenza)
        }
        Scene::OyenzaNotice => {
            st.oyenza = fake_oyenza(&st, false);
            st.notify(Icon::MoneyIn, palette::GREEN, "Money in \u{b7} BCA", "Amount and sender hidden", 30.0, Some(NoticeAction::OpenPage(PageKind::Oyenza)));
            st.set_notice_inline(InlineAction::ShowAmount);
            Mode::Notice
        }
        Scene::SettingsSignedIn => {
            st.oyenza = fake_oyenza(&st, false);
            st.page = PageKind::Settings;
            Mode::Expanded(PageKind::Settings)
        }
        Scene::IdleStatus => {
            st.settings.idle_style = IdleStyle::Status;
            Mode::Idle
        }
        Scene::MediaNoLine => {
            // A long title: Pill floats it below a notch (the other themes
            // keep words out from under the camera).
            media(&mut st);
            if let Some(m) = st.media.as_mut() {
                m.title = "Paper Boats (Harbor Lights Session)".into();
            }
            st.settings.lyrics = false;
            st.lyrics = LyricsState::default();
            Mode::Activity(activities::Activity::Media)
        }
        Scene::System(m) => {
            st.settings.set_system(true);
            // Power is a desktop Mac's: no battery, the SMC's whole-Mac watts.
            st.system = sample_system(m == Module::Power);
            st.ui.system_module = m;
            if let Some(r) = st.system.latest.clone() {
                for a in r.top_cpu.iter().chain(&r.top_memory).chain(&r.top_io) {
                    if let Some(p) = &a.path {
                        let icon = app_icon(p);
                        st.app_icons.insert(p.clone(), icon);
                    }
                }
            }
            st.page = PageKind::System;
            Mode::Expanded(PageKind::System)
        }
        Scene::Japanese => {
            media(&mut st);
            st.lyrics.lyrics = Some(parse_lrc(LYRICS_JA));
            // 现有多语种场景同时核对歌词列与自定义颜色。
            st.settings.lyrics_color = Some(0xF06292);
            st.page = PageKind::Media;
            Mode::Expanded(PageKind::Media)
        }
        Scene::Diorama(shows) => {
            let (hour, minute) = shows.time();
            set_clock(&mut st, hour, minute);
            match shows {
                Rest::Dawn | Rest::Morning | Rest::GoldenHour | Rest::Night | Rest::SmallHours => {}
                Rest::TwelveHour => st.settings.use_24h = false,
                Rest::Minimal => st.settings.idle_style = IdleStyle::Minimal,
                Rest::Hover => hovering = true,
            }
            Mode::Idle
        }
    };

    // The island at rest at its size for this state (as the app sizes it).
    // The Pill theme floats it below the top edge, where the app puts it.
    if let Some(width) = width {
        st.settings.expanded_width = width;
    }
    let size = |st: &AppState| {
        let mut g = activities::geom(&mode, st);
        if let Some(n) = notch {
            g = notch::geom(&mode, st, n, g);
        }
        let gap = if crate::pill::on(theme) { notch.map_or(Some(crate::pill::TOP_GAP), |n| notch::pill_gap(&mode, st, n)) } else { None };
        if hovering {
            g = if gap.is_some() { crate::pill::motion::hover_lift(g) } else { crate::island::Geom::new(g.w + 12.0, g.h + 4.0, g.r + 2.0) };
        }
        (g, gap)
    };
    // Pill's surfaces size themselves from what they last drew (a measured
    // text width), and Shelf's new tiles fade in over their first frames.
    // Draw those scenes before the picture's time so they show settled content.
    if crate::pill::on(theme) || scene == Scene::ShelfItems {
        for t in [NOW_MONO - 0.4, NOW_MONO - 0.2] {
            st.now = t;
            let (g, gap) = size(&st);
            warm_up(gfx, &mut st, mode, notch, g, gap, hovering)?;
        }
        st.now = NOW_MONO;
    }
    let (g, gap) = size(&st);
    let mut island = Island::new(g);
    island.set_floating(gap);
    island.snap(g);

    let menu_h = notch.map_or(MENU_BAR, |n| n.h);
    let w_dip = (st.settings.expanded_width + 2.0 * MARGIN_X).max(g.w + 2.0 * (SHOULDER + MARGIN_X));
    let h_dip = (island.top() + g.h).max(menu_h) + MARGIN_BOTTOM + if matches!(mode, Mode::Expanded(_)) { 0.0 } else { 60.0 };
    let (w_px, h_px) = ((w_dip * SCALE).ceil() as usize, (h_dip * SCALE).ceil() as usize);
    let bmp = Bitmap::new(w_px, h_px, SCALE).ok_or("no bitmap context")?;
    let cx = w_dip / 2.0;
    let body = island.body_rect_at(cx);

    // The desktop: wallpaper and the menu bar.
    gfx.set_look(&poni_core::look::CLASSIC);
    {
        let mut c = gfx.canvas(bmp.ctx());
        let steps = 48;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let col = Color::lerp(Color::hex(0x2B4F86), Color::hex(0xF0708A), t);
            let y = h_dip * i as f32 / steps as f32;
            c.fill_rect(Rect::new(0.0, y, w_dip, h_dip / steps as f32 + 0.5), col);
        }
        c.fill_rect(Rect::new(0.0, 0.0, w_dip, menu_h), Color::rgba(0.96, 0.96, 0.97, 0.78));
    }

    // The island, drawn by the app's own content code (around a notch in
    // the look that meets its black).
    let look = if notch.is_some() { poni_core::look::around_notch(theme) } else { poni_core::look::of(theme) };
    gfx.set_look(look);
    {
        let mut c = gfx.canvas(bmp.ctx());
        let shoulder = notch.map_or(SHOULDER, |n| notch::shoulder(n, g.w));
        let inside = notch.is_some_and(|_| notch::fit(&mode, &st) == notch::Fit::Inside);
        if look.material == poni_core::look::Material::Glass {
            crate::glass::draw_body(&mut c, body, g.r, shoulder, notch, inside, true);
        } else {
            let border = if inside { None } else { Some(c.look().border) };
            if island.floating() {
                crate::pill::shape::draw_body(&mut c, body, g.r, palette::ISLAND, border);
            } else {
                c.notch(body, g.r, shoulder, palette::ISLAND, border);
            }
        }
        let mut input = Input::default();
        if hovering {
            input.mouse = Some(Point::new(body.center().x + 20.0, body.center().y));
        }
        let mut hits: Vec<Hit> = Vec::new();
        crate::app::draw_content(&mut c, &mut input, &mut hits, &mut st, mode, body, body, None, NOW_MONO, notch);
        if matches!(scene, Scene::Bluetooth(_)) {
            // 展示同时记录实际命中矩形，便于核对窄导航及禁用按钮；不执行系统操作。
            let row = pages::tabs(&st);
            let last_tab_id = crate::ui::wid_n("tab", (row.len() - 1) as u64);
            let rect = |id| hits.iter().find(|hit| hit.id == id).map(|hit| hit.rect);
            let layout = format!(
                "page={} width={} tabs={} devices={} active={} loading={} stale={} scroll={}\nlast_tab={:?}\ngear={:?}\nrefresh={:?}\nbluetooth_settings={:?}\n",
                st.page.key(), st.settings.expanded_width, row.len(), st.bluetooth.devices.len(), st.bluetooth.active, st.bluetooth.loading, st.bluetooth.stale, st.bluetooth.scroll,
                rect(last_tab_id), rect(crate::ui::wid(pages::GEAR_ID)), rect(crate::ui::wid("bluetooth.refresh")), rect(crate::ui::wid("bluetooth.settings")),
            );
            std::fs::write(path.with_extension("layout.txt"), layout).map_err(|error| error.to_string())?;
        }
    }

    // The camera notch hides its part of the island on a real MacBook.
    if let Some(n) = notch {
        let mut c = gfx.canvas(bmp.ctx());
        c.unthemed(|c| c.fill_rrect(Rect::new(cx - n.w / 2.0, -12.0, n.w, n.h + 12.0), 10.0, Color::black(1.0)));
    }

    let px = bmp.pixels();
    crate::png::save_pbgra(path, w_px as u32, h_px as u32, &px)?;
    let _ = pages::HEADER_H;
    Ok(())
}

/// One frame of `mode` at `st.now`, drawn offscreen and thrown away, for the
/// state the drawing leaves behind (see `render_scene`).
#[allow(clippy::too_many_arguments)]
fn warm_up(gfx: &mut Gfx, st: &mut AppState, mode: Mode, notch: Option<Notch>, g: crate::island::Geom, gap: Option<f32>, hovering: bool) -> Result<(), String> {
    let mut island = Island::new(g);
    island.set_floating(gap);
    island.snap(g);
    let w = (st.settings.expanded_width + 2.0 * MARGIN_X).max(g.w + 2.0 * (SHOULDER + MARGIN_X));
    let h = island.top() + g.h + MARGIN_BOTTOM;
    let bmp = Bitmap::new((w * SCALE).ceil() as usize, (h * SCALE).ceil() as usize, SCALE).ok_or("no bitmap context")?;
    let body = island.body_rect_at(w / 2.0);
    let theme = st.settings.theme;
    gfx.set_look(if notch.is_some() { poni_core::look::around_notch(theme) } else { poni_core::look::of(theme) });
    let mut c = gfx.canvas(bmp.ctx());
    let mut input = Input::default();
    if hovering {
        input.mouse = Some(Point::new(body.center().x + 20.0, body.center().y));
    }
    let mut hits: Vec<Hit> = Vec::new();
    let now = st.now;
    crate::app::draw_content(&mut c, &mut input, &mut hits, st, mode, body, body, None, now, notch);
    Ok(())
}

// ---------------------------------------------------------------- the System page's sample minute

thread_local! {
    /// Each sample app's icon, drawn once a run.
    static ICONS: std::cell::RefCell<std::collections::HashMap<String, Option<crate::gfx::Image>>> = Default::default();
}

/// An app's icon the way the app gets it (`Command::LoadAppIcon`).
fn app_icon(path: &str) -> Option<crate::gfx::Image> {
    ICONS.with(|c| {
        c.borrow_mut()
            .entry(path.to_string())
            .or_insert_with(|| {
                let px = pages::system::icon_px(SCALE);
                let icon = crate::services::thumbs::app_icon(path, px).map(|i| pages::system::shrink_icon(&i, px));
                println!("render: app icon for {path}: {}", if icon.is_some() { "drawn" } else { "missing" });
                icon
            })
            .clone()
    })
}

/// Apps of the sample minute: name, bundle, CPU share, memory, disk bytes/s.
const SAMPLE_APPS: [(&str, &str, f32, u64, f64); 8] = [
    ("Safari", "/Applications/Safari.app", 0.091, 2_150 << 20, 1.2e6),
    ("Music", "/System/Applications/Music.app", 0.042, 412 << 20, 4.1e5),
    ("Mail", "/System/Applications/Mail.app", 0.018, 280 << 20, 8.2e5),
    ("Finder", "/System/Library/CoreServices/Finder.app", 0.013, 96 << 20, 4.0e4),
    ("Messages", "/System/Applications/Messages.app", 0.009, 210 << 20, 1.2e4),
    ("Xcode", "/Applications/Xcode.app", 0.008, 940 << 20, 9.6e4),
    ("Photos", "/System/Applications/Photos.app", 0.004, 520 << 20, 3.1e6),
    ("Terminal", "/System/Applications/Utilities/Terminal.app", 0.002, 88 << 20, 3.0e3),
];

fn sample_app(i: usize) -> AppUse {
    let (name, path, cpu, memory, io) = SAMPLE_APPS[i];
    AppUse { image: path.to_lowercase(), name: name.into(), path: Some(path.into()), pid: 400 + i as u64, cpu, memory, io, processes: if i == 0 { 9 } else { 1 } }
}

/// The System page's sample minute (all made up): a MacBook Air with an M2,
/// its four efficiency cores numbered before its four performance cores, as
/// Apple numbers them.
fn sample_system(desktop: bool) -> Monitor {
    const MIB: f64 = 1024.0 * 1024.0;
    let mut m = Monitor::default();
    for i in 0..HISTORY_LEN {
        let t = i as f32;
        let last = i + 1 == HISTORY_LEN;
        let spike = if (38..45).contains(&i) { 0.22 * ((t - 38.0) / 6.0 * std::f32::consts::PI).sin() } else { 0.0 };
        let cpu = if last { 0.23 } else { (0.19 + 0.05 * (t / 4.3).sin() + 0.03 * (t / 1.7 + 1.1).sin() + spike).clamp(0.03, 0.95) };
        let gpu = if last { 0.31 } else { (0.24 + 0.08 * (t / 5.1 + 0.4).sin() + 0.04 * (t / 2.3).sin() + if (18..30).contains(&i) { 0.3 } else { 0.0 }).clamp(0.02, 0.98) };
        let burst = |at: usize, len: usize, peak: f64| if (at..at + len).contains(&i) { peak * (((i - at) as f64 + 0.5) / len as f64 * std::f64::consts::PI).sin() } else { 0.0 };
        let read = if last { 1.2 * MIB } else { 0.15 * MIB + burst(12, 6, 6.5 * MIB) + burst(41, 4, 2.8 * MIB) };
        let write = if last { 0.33 * MIB } else { 0.05 * MIB + burst(20, 9, 1.9 * MIB) };
        let down = if last { 1.2 * MIB } else { 0.08 * MIB + burst(5, 14, 3.4 * MIB) + burst(46, 10, 1.6 * MIB) };
        let up = if last { 0.082 * MIB } else { 0.02 * MIB + burst(25, 8, 0.6 * MIB) };
        let mut r = sample_reading(cpu, gpu, (read, write), (down, up), t);
        if desktop {
            // What the office Mac Studio's SMC read at rest (7.8-8.2 W), rising with the graphics.
            let w = if last { 8.1 } else { 7.4 + 24.0 * (gpu - 0.2).max(0.0) + 3.0 * cpu };
            r.battery = None;
            r.power = Some(PowerReading { total_w: w, cpu_w: None, gpu_w: None, rest_w: w, psu: None });
        }
        m.push(r);
    }
    m
}

fn sample_reading(cpu: f32, gpu: f32, (read, write): (f64, f64), (down, up): (f64, f64), t: f32) -> Reading {
    const GIB: u64 = 1 << 30;
    let total = 16 * GIB;
    let cores = (0..8)
        .map(|k| {
            let p = k >= 4;
            let wave = 0.5 + 0.5 * (t / 3.0 + k as f32 * 1.7).sin();
            let load = if p { cpu * (0.6 + 1.1 * wave) } else { cpu * (0.3 + 0.5 * wave) };
            CoreLoad { load: load.clamp(0.02, 1.0), kind: if p { CoreKind::Performance } else { CoreKind::Efficiency } }
        })
        .collect();
    let (app, wired, compressed, cached) = (66 * GIB / 10, 21 * GIB / 10, 9 * GIB / 10, 42 * GIB / 10);
    let used = app + wired + compressed;
    let pressure = level_pressure(PressureLevel::Normal, 0.41);
    let apps = |order: [usize; 8]| order.iter().map(|&i| sample_app(i)).collect::<Vec<_>>();
    Reading {
        cpu: Some(CpuReading { load: cpu, user: cpu * 0.62, kernel: cpu * 0.38, cores, logical: 8, uptime_s: 3 * 86_400 + 4 * 3600 + 12 * 60, ..CpuReading::default() }),
        gpu: Some(GpuReading { name: "Apple M2".into(), util: gpu, engine_3d: gpu * 0.94, tiler: gpu * 0.41, mem_used: 13 * GIB / 10, mem_total: total, integrated: true, ..GpuReading::default() }),
        memory: Some(MemoryReading {
            total,
            used,
            committed: (f64::from(pressure) * total as f64).round() as u64,
            commit_limit: total,
            cached,
            wired,
            compressed,
            free: total - used - cached,
            page_used: 3 * GIB / 10,
            page_total: GIB,
        }),
        drives: vec![
            Drive { root: "Macintosh HD".into(), label: String::new(), total: 460 * GIB, free: 212 * GIB, system: true },
            Drive { root: "Backup".into(), label: String::new(), total: 931 * GIB, free: 640 * GIB, system: false },
        ],
        disk: Some(DiskActivity { read_bps: read, write_bps: write, active: None }),
        network: Some(NetReading {
            online: true,
            adapter: "Wi-Fi".into(),
            ipv4: Some("192.168.1.42".into()),
            mac: "3C:22:FB:0A:11:9E".into(),
            link_bps: 866_700_000,
            down_bps: down,
            up_bps: up,
            received: 34 * GIB / 10,
            sent: 410 << 20,
        }),
        battery: Some(BatteryReading {
            percent: 76,
            secs_left: Some(2 * 3600 + 14 * 60),
            design_mwh: Some(52_600),
            full_mwh: Some(48_390),
            cycles: Some(193),
            rate_mw: Some(-8_200),
            ..BatteryReading::default()
        }),
        top_cpu: apps([0, 1, 2, 3, 4, 5, 6, 7]),
        top_memory: apps([0, 5, 6, 1, 2, 4, 3, 7]),
        top_io: apps([6, 0, 2, 1, 5, 3, 4, 7]),
        power: None,
    }
}

// ---------------------------------------------------------------- sample album art

/// Procedural album art: a harbor sunset with a paper boat (premultiplied
/// BGRA, opaque). The same picture the Windows self-test uses.
pub(crate) fn make_art(size: u32) -> Vec<u8> {
    const SS: usize = 3;
    let n = size.max(1) as usize;
    let mut px = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0.0f32; 3];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / n as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / n as f32;
                    for (a, c) in acc.iter_mut().zip(art_sample(u, v)) {
                        *a += c;
                    }
                }
            }
            let o = (y * n + x) * 4;
            let to8 = |c: f32| ((c / (SS * SS) as f32).clamp(0.0, 1.0) * 255.0).round() as u8;
            px[o] = to8(acc[2]);
            px[o + 1] = to8(acc[1]);
            px[o + 2] = to8(acc[0]);
            px[o + 3] = 255;
        }
    }
    px
}

fn rgb(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 0xff) as f32 / 255.0, ((hex >> 8) & 0xff) as f32 / 255.0, (hex & 0xff) as f32 / 255.0]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let edge = |p0: (f32, f32), p1: (f32, f32)| (p1.0 - p0.0) * (p.1 - p0.1) - (p1.1 - p0.1) * (p.0 - p0.0);
    let (d1, d2, d3) = (edge(a, b), edge(b, c), edge(c, a));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

fn art_sample(u: f32, v: f32) -> [f32; 3] {
    const HORIZON: f32 = 0.64;
    let (sun_x, sun_y, sun_r) = (0.66, HORIZON - 0.03, 0.15);
    let mut c = if v < HORIZON { mix(rgb(0xFFB37A), rgb(0xE0607E), v / HORIZON) } else { mix(rgb(0x2F6F8F), rgb(0x13233F), (v - HORIZON) / (1.0 - HORIZON)) };
    let d = ((u - sun_x).powi(2) + (v - sun_y).powi(2)).sqrt();
    if v < HORIZON && d < sun_r {
        c = mix(c, rgb(0xFFE3A3), 0.92);
    }
    if v > HORIZON + 0.015 && v < HORIZON + 0.24 {
        let depth = (v - HORIZON) / 0.24;
        let band = ((v - HORIZON) * 42.0).fract();
        if band < 0.4 && (u - sun_x).abs() < 0.12 * (1.0 - depth) {
            c = mix(c, rgb(0xFFD08A), 0.6 * (1.0 - depth));
        }
    }
    let paper = rgb(0xF7F3EA);
    let hull = v > 0.70 && v < 0.78 && {
        let t = (v - 0.70) / 0.08;
        u > 0.17 + 0.06 * t && u < 0.51 - 0.06 * t
    };
    let sail = in_triangle((u, v), (0.34, 0.46), (0.45, 0.70), (0.24, 0.70));
    if hull {
        c = mix(paper, rgb(0xD8D0C2), ((v - 0.70) / 0.08) * 0.5);
    }
    if sail {
        c = if u > 0.34 { mix(paper, rgb(0xD9D2C4), 0.55) } else { paper };
    }
    c
}

#[cfg(test)]
#[path = "render/interaction_tests.rs"]
mod interaction_tests;
