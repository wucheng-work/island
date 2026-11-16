//! Notification-area icon and its menu (also shown on right-click of the island).

use crate::events::WM_APP_TRAY;
use crate::state::OyenzaLink;
use crate::updates::UpdateLine;
use poni_core::settings::{IdleStyle, Settings, Theme};
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Tray {
    hwnd: HWND,
}

impl Tray {
    pub fn add(hwnd: HWND) -> Result<Tray> {
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            // Ask for the small-icon size so Windows picks poni.ico's own 16-24 px
            // frames; LoadIconW gives the 32 px one and shrinks it (blurry).
            let (cx, cy) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
            let icon = LoadImageW(hinst, PCWSTR(1 as _), IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR)
                .map(|h| HICON(h.0))
                .or_else(|_| LoadIconW(hinst, PCWSTR(1 as _)))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_APP_TRAY,
                hIcon: icon,
                ..Default::default()
            };
            let tip: Vec<u16> = format!("Island For Windows {}", crate::version::VERSION)
                .encode_utf16()
                .collect();
            nid.szTip[..tip.len()].copy_from_slice(&tip);
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                return Err(windows::core::Error::from_win32());
            }
        }
        Ok(Tray { hwnd })
    }

    pub fn remove(self) {
        unsafe {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCmd {
    ToggleIdleStyle(IdleStyle),
    SetTheme(Theme),
    ToggleSystemHistory,
    ToggleLyrics,
    /// true: hide in full-screen apps; false: stay on top of them.
    SetHideFullscreen(bool),
    /// Show over games (frame rate, GPU and CPU on the island over a game).
    ToggleGameStats,
    /// Show over games for a few seconds at a time instead of all along.
    ToggleGamePeek,
    ToggleWeChatReminders,
    TogglePinned,
    /// "Move island to the centre": back from where it was dragged.
    CentreIsland,
    ToggleAutostart,
    /// "Advanced settings…": the Settings window.
    ShowSettings,
    OpenSettings,
    ReloadSettings,
    OpenDataFolder,
    AppleMusicToken,
    AppleMusicHelp,
    AppleMusicForget,
    OyenzaSignIn,
    OyenzaCancel,
    OyenzaDisconnect,
    OyenzaOpen,
    ToggleOyenzaAmounts,
    /// "Update to 灵动岛 0.2.5…" (`crate::updates`).
    Update,
    /// "Check for updates…": read the update file now (`crate::updates`).
    CheckForUpdates,
    Quit,
}

pub enum MenuItem {
    Item {
        label: String,
        cmd: MenuCmd,
        checked: bool,
    },
    /// A greyed line that only says something ("Connected as …").
    Info {
        label: String,
    },
    Sub {
        label: String,
        items: Vec<MenuItem>,
    },
    Separator,
}

fn item(label: &str, cmd: MenuCmd, checked: bool) -> MenuItem {
    MenuItem::Item {
        label: label.to_string(),
        cmd,
        checked,
    }
}

/// An idle look as the menu names it. In the Oyen theme the eyes are Oyen's own.
fn idle_label(s: IdleStyle, theme: Theme) -> &'static str {
    match s {
        IdleStyle::Clock => "时钟",
        IdleStyle::Eyes if theme == Theme::Oyen => "Oyen",
        IdleStyle::Eyes => "眼睛",
        IdleStyle::Minimal => "简洁",
        IdleStyle::Hidden => "悬停时显示",
        IdleStyle::Status => "状态栏",
    }
}

/// 仅提供本项目的手动检查入口，未配置签名公钥时不触发检查。
pub fn menu_items(
    s: &Settings,
    pinned: bool,
    _oyenza: &OyenzaLink,
    _update: UpdateLine,
) -> Vec<MenuItem> {
    // The look the theme draws is the one checked: outside Pill, Status shows the clock.
    let shown = crate::pill::status::shown(s.theme, s.idle_style);
    let style = |st: IdleStyle| {
        item(
            idle_label(st, s.theme),
            MenuCmd::ToggleIdleStyle(st),
            shown == st,
        )
    };
    let theme = |label: &str, t: Theme| item(label, MenuCmd::SetTheme(t), s.theme == t);
    let fullscreen = |label: &str, hide: bool| {
        item(
            label,
            MenuCmd::SetHideFullscreen(hide),
            s.hide_in_fullscreen == hide,
        )
    };
    let mut items = vec![
        item("高级设置…", MenuCmd::ShowSettings, false),
        MenuItem::Separator,
        item("保持灵动岛展开", MenuCmd::TogglePinned, pinned),
        MenuItem::Sub {
            label: "主题".into(),
            items: vec![
                theme("经典", Theme::Classic),
                theme("Oyen", Theme::Oyen),
                theme("玻璃", Theme::Glass),
                theme("胶囊", Theme::Pill),
                theme("立体场景", Theme::Diorama),
            ],
        },
        // The status bar is listed only in Pill, as on the Settings page: the
        // other themes would draw the clock for it (Diorama its picture).
        MenuItem::Sub {
            label: "待机样式".into(),
            items: crate::pill::status::looks(s.theme)
                .iter()
                .map(|l| style(*l))
                .collect(),
        },
        MenuItem::Separator,
        item("同步歌词", MenuCmd::ToggleLyrics, s.lyrics),
        apple_music_menu(),
        item(
            "微信消息提醒",
            MenuCmd::ToggleWeChatReminders,
            s.wechat_reminders,
        ),
        MenuItem::Sub {
            label: "全屏应用中".into(),
            items: vec![
                fullscreen("隐藏", true),
                fullscreen("保持置顶", false),
                MenuItem::Separator,
                item(
                    "在游戏上方显示 FPS、GPU 和 CPU",
                    MenuCmd::ToggleGameStats,
                    s.game_stats,
                ),
                item(
                    "仅显示几秒（不增加延迟）",
                    MenuCmd::ToggleGamePeek,
                    s.game_peek,
                ),
            ],
        },
        item(
            "登录 Windows 时启动",
            MenuCmd::ToggleAutostart,
            crate::autostart::is_enabled(),
        ),
        MenuItem::Separator,
        item("打开设置文件", MenuCmd::OpenSettings, false),
        item("重新加载设置", MenuCmd::ReloadSettings, false),
        item("打开数据文件夹", MenuCmd::OpenDataFolder, false),
        if poni_core::update::enabled() && !crate::autostart::packaged() {
            item("检查更新…", MenuCmd::CheckForUpdates, false)
        } else {
            MenuItem::Info {
                label: "在线更新尚未配置".into(),
            }
        },
        MenuItem::Separator,
        item("退出 Island For Windows", MenuCmd::Quit, false),
    ];
    // Only once the island was dragged off the middle: the way back, under Keep island open.
    if s.island_x.is_some() {
        let at = items
            .iter()
            .position(|i| {
                matches!(
                    i,
                    MenuItem::Item {
                        cmd: MenuCmd::TogglePinned,
                        ..
                    }
                )
            })
            .map_or(items.len(), |i| i + 1);
        items.insert(
            at,
            item("将灵动岛移回居中位置", MenuCmd::CentreIsland, false),
        );
    }
    // Only while System is enabled in Settings: its page's history, before WeChat reminders.
    if s.system_on() {
        let at = items
            .iter()
            .position(|i| {
                matches!(
                    i,
                    MenuItem::Item {
                        cmd: MenuCmd::ToggleWeChatReminders,
                        ..
                    }
                )
            })
            .unwrap_or(items.len());
        items.insert(
            at,
            item(
                "保留系统页历史记录",
                MenuCmd::ToggleSystemHistory,
                s.system_history,
            ),
        );
    }
    items
}

/// Opt-in lyrics from Apple Music with the user's own token.
fn apple_music_menu() -> MenuItem {
    let on = crate::paths::apple_music_token_path().exists();
    let mut items = vec![
        item("使用剪贴板中的令牌", MenuCmd::AppleMusicToken, false),
        item("如何获取令牌", MenuCmd::AppleMusicHelp, false),
    ];
    if on {
        items.push(MenuItem::Separator);
        items.push(item("移除令牌", MenuCmd::AppleMusicForget, false));
    }
    MenuItem::Sub {
        label: if on {
            "Apple Music 歌词：已开启".into()
        } else {
            "Apple Music 歌词".into()
        },
        items,
    }
}

fn build(menu: HMENU, items: &[MenuItem], cmds: &mut Vec<MenuCmd>) {
    for it in items {
        unsafe {
            match it {
                MenuItem::Item {
                    label,
                    cmd,
                    checked,
                } => {
                    cmds.push(*cmd);
                    let id = cmds.len();
                    let w: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let flags = MF_STRING | if *checked { MF_CHECKED } else { MF_UNCHECKED };
                    let _ = AppendMenuW(menu, flags, id, PCWSTR(w.as_ptr()));
                }
                MenuItem::Info { label } => {
                    let w: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let _ = AppendMenuW(
                        menu,
                        MF_STRING | MF_GRAYED | MF_DISABLED,
                        0,
                        PCWSTR(w.as_ptr()),
                    );
                }
                MenuItem::Sub { label, items } => {
                    if let Ok(sub) = CreatePopupMenu() {
                        build(sub, items, cmds);
                        let w: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(w.as_ptr()));
                    }
                }
                MenuItem::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
            }
        }
    }
}

/// Show the menu at `pt` (screen pixels); returns the chosen command.
pub fn show_menu(owner: HWND, pt: POINT, items: &[MenuItem]) -> Option<MenuCmd> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let mut cmds = Vec::new();
        build(menu, items, &mut cmds);
        // Required so the menu closes when clicking elsewhere; afterwards the
        // previous app gets its focus back.
        let prev = GetForegroundWindow();
        let _ = SetForegroundWindow(owner);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            owner,
            None,
        );
        let _ = PostMessageW(owner, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        let idx = chosen.0 as usize;
        let cmd = if idx == 0 {
            None
        } else {
            cmds.get(idx - 1).copied()
        };
        let launches = matches!(
            cmd,
            Some(
                MenuCmd::ShowSettings
                    | MenuCmd::OpenSettings
                    | MenuCmd::OpenDataFolder
                    | MenuCmd::AppleMusicHelp
                    | MenuCmd::OyenzaOpen
            )
        );
        if !launches
            && !prev.is_invalid()
            && prev != owner
            && IsWindow(prev).as_bool()
            && GetForegroundWindow() == owner
        {
            let _ = SetForegroundWindow(prev);
        }
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fullscreen_choices(s: &Settings) -> Vec<(String, MenuCmd, bool)> {
        let items = menu_items(s, false, &OyenzaLink::Off, UpdateLine::None);
        let Some(MenuItem::Sub { items, .. }) = items
            .into_iter()
            .find(|i| matches!(i, MenuItem::Sub { label, .. } if label == "全屏应用中"))
        else {
            panic!("no full-screen submenu");
        };
        items
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Item {
                    label,
                    cmd,
                    checked,
                } => Some((label, cmd, checked)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn settings_stays_first_regardless_of_update_state() {
        let s = Settings::default();
        let first = |update: UpdateLine| match menu_items(&s, false, &OyenzaLink::Off, update)
            .into_iter()
            .next()
        {
            Some(MenuItem::Item { label, cmd, .. }) => Some((label, cmd)),
            _ => None,
        };
        assert_eq!(
            first(UpdateLine::Offer("0.2.5".into())),
            Some(("高级设置…".to_string(), MenuCmd::ShowSettings))
        );
        assert_eq!(
            first(UpdateLine::None).map(|(_, cmd)| cmd),
            Some(MenuCmd::ShowSettings)
        );
        assert_eq!(
            first(UpdateLine::Check).map(|(_, cmd)| cmd),
            Some(MenuCmd::ShowSettings)
        );
    }

    #[test]
    fn custom_build_only_exposes_its_own_manual_update_entry() {
        let cmds = |update: UpdateLine| -> Vec<MenuCmd> {
            menu_items(&Settings::default(), false, &OyenzaLink::Off, update)
                .into_iter()
                .filter_map(|i| match i {
                    MenuItem::Item { cmd, .. } => Some(cmd),
                    _ => None,
                })
                .collect()
        };
        for update in [
            UpdateLine::Check,
            UpdateLine::Offer("0.2.5".into()),
            UpdateLine::None,
        ] {
            let items = cmds(update);
            assert_eq!(
                items.contains(&MenuCmd::CheckForUpdates),
                poni_core::update::enabled() && !crate::autostart::packaged()
            );
            assert!(!items.contains(&MenuCmd::Update));
            assert!(!items.contains(&MenuCmd::OyenzaSignIn));
            assert!(!items.contains(&MenuCmd::OyenzaOpen));
            assert_eq!(items.last(), Some(&MenuCmd::Quit));
        }
    }

    #[test]
    fn the_way_back_to_the_centre_shows_once_the_island_moved() {
        let cmds = |s: &Settings| -> Vec<MenuCmd> {
            menu_items(s, false, &OyenzaLink::Off, UpdateLine::None)
                .into_iter()
                .filter_map(|i| match i {
                    MenuItem::Item { cmd, .. } => Some(cmd),
                    _ => None,
                })
                .collect()
        };
        assert!(!cmds(&Settings::default()).contains(&MenuCmd::CentreIsland));
        let moved = cmds(&Settings {
            island_x: Some(0.8),
            ..Settings::default()
        });
        let pin = moved
            .iter()
            .position(|c| *c == MenuCmd::TogglePinned)
            .expect("no Keep island open");
        assert_eq!(moved.get(pin + 1), Some(&MenuCmd::CentreIsland));
    }

    #[test]
    fn system_history_is_in_the_menu_only_while_system_is_on() {
        let has = |s: &Settings| {
            menu_items(s, false, &OyenzaLink::Off, UpdateLine::None)
                .iter()
                .position(|i| {
                    matches!(
                        i,
                        MenuItem::Item {
                            cmd: MenuCmd::ToggleSystemHistory,
                            ..
                        }
                    )
                })
        };
        let mut s = Settings::default();
        assert_eq!(has(&s), None, "System is off by default");
        s.set_system(true);
        let at = has(&s).expect("no System history item");
        // 系统历史记录仍位于常用开关组内。
        let items = menu_items(&s, false, &OyenzaLink::Off, UpdateLine::None);
        assert!(matches!(
            &items[at + 1],
            MenuItem::Item {
                cmd: MenuCmd::ToggleWeChatReminders,
                ..
            }
        ));
    }

    fn idle_choices(s: &Settings) -> Vec<(String, bool)> {
        let items = menu_items(s, false, &OyenzaLink::Off, UpdateLine::None);
        let Some(MenuItem::Sub { items, .. }) = items
            .into_iter()
            .find(|i| matches!(i, MenuItem::Sub { label, .. } if label == "待机样式"))
        else {
            panic!("no idle look submenu");
        };
        items
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Item { label, checked, .. } => Some((label, checked)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn status_bar_is_listed_only_in_the_pill_theme() {
        let pill = Settings {
            theme: Theme::Pill,
            idle_style: IdleStyle::Status,
            ..Settings::default()
        };
        let labels: Vec<String> = idle_choices(&pill).into_iter().map(|c| c.0).collect();
        assert_eq!(labels, ["时钟", "眼睛", "简洁", "悬停时显示", "状态栏"]);
        assert_eq!(
            idle_choices(&pill).last(),
            Some(&("状态栏".to_string(), true))
        );
        // Another theme draws the clock for the status bar: Clock is checked,
        // and Status comes back with Pill.
        for theme in [Theme::Classic, Theme::Oyen, Theme::Glass, Theme::Diorama] {
            let s = Settings {
                theme,
                ..pill.clone()
            };
            let choices = idle_choices(&s);
            assert_eq!(
                choices.len(),
                if theme == Theme::Diorama { 3 } else { 4 },
                "{theme:?}"
            );
            assert_eq!(choices[0], ("时钟".to_string(), true), "{theme:?}");
            assert!(choices.iter().skip(1).all(|c| !c.1), "{theme:?}");
            assert!(choices.iter().all(|c| c.0 != "状态栏"), "{theme:?}");
        }
        // Diorama's looks by name: its picture with the time, the picture
        // alone, and nothing until the pointer comes. It has no eyes: a
        // setting left on Eyes shows the picture alone, so Minimal is checked.
        let diorama = Settings {
            theme: Theme::Diorama,
            idle_style: IdleStyle::Minimal,
            ..Settings::default()
        };
        let looks = [
            ("时钟".to_string(), false),
            ("简洁".to_string(), true),
            ("悬停时显示".to_string(), false),
        ];
        assert_eq!(idle_choices(&diorama), looks);
        assert_eq!(
            idle_choices(&Settings {
                idle_style: IdleStyle::Eyes,
                ..diorama
            }),
            looks
        );
    }

    /// Where a theme comes in the menu. No `_` arm: a theme added to the
    /// enum stops this compiling until it has a place, and then the test
    /// below asks the menu for it.
    fn place(t: Theme) -> usize {
        match t {
            Theme::Classic => 0,
            Theme::Oyen => 1,
            Theme::Glass => 2,
            Theme::Pill => 3,
            Theme::Diorama => 4,
        }
    }

    fn theme_choices(s: &Settings) -> Vec<(String, MenuCmd, bool)> {
        let items = menu_items(s, false, &OyenzaLink::Off, UpdateLine::None);
        let Some(MenuItem::Sub { items, .. }) = items
            .into_iter()
            .find(|i| matches!(i, MenuItem::Sub { label, .. } if label == "主题"))
        else {
            panic!("no theme submenu");
        };
        items
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Item {
                    label,
                    cmd,
                    checked,
                } => Some((label, cmd, checked)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_theme_submenu_has_every_theme_and_checks_the_one_in_use() {
        let names = ["经典", "Oyen", "玻璃", "胶囊", "立体场景"];
        let choices = theme_choices(&Settings::default());
        assert_eq!(
            choices.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            names
        );
        for (i, (label, cmd, _)) in choices.iter().enumerate() {
            let MenuCmd::SetTheme(theme) = *cmd else {
                panic!("{label} doesn't set a theme")
            };
            assert_eq!(place(theme), i, "{label}");
            // Picked, it is the one checked.
            let checked: Vec<bool> = theme_choices(&Settings {
                theme,
                ..Settings::default()
            })
            .into_iter()
            .map(|c| c.2)
            .collect();
            assert_eq!(
                checked,
                (0..names.len()).map(|j| j == i).collect::<Vec<_>>(),
                "{label}"
            );
        }
    }

    #[test]
    fn start_with_windows_follows_the_full_screen_submenu() {
        // Over maximized windows (Step aside) is gone: nothing sits between them.
        let items = menu_items(
            &Settings::default(),
            false,
            &OyenzaLink::Off,
            UpdateLine::None,
        );
        let fs = items
            .iter()
            .position(|i| matches!(i, MenuItem::Sub { label, .. } if label == "全屏应用中"))
            .expect("full-screen submenu");
        assert!(matches!(
            items.get(fs + 1),
            Some(MenuItem::Item {
                cmd: MenuCmd::ToggleAutostart,
                ..
            })
        ));
    }

    #[test]
    fn full_screen_submenu_checks_the_current_choice() {
        let mut s = Settings::default();
        for hide in [true, false] {
            s.hide_in_fullscreen = hide;
            let choices = fullscreen_choices(&s);
            assert_eq!(
                choices,
                vec![
                    ("隐藏".to_string(), MenuCmd::SetHideFullscreen(true), hide),
                    (
                        "保持置顶".to_string(),
                        MenuCmd::SetHideFullscreen(false),
                        !hide
                    ),
                    (
                        "在游戏上方显示 FPS、GPU 和 CPU".to_string(),
                        MenuCmd::ToggleGameStats,
                        false
                    ),
                    (
                        "仅显示几秒（不增加延迟）".to_string(),
                        MenuCmd::ToggleGamePeek,
                        false
                    ),
                ]
            );
        }
    }
}
