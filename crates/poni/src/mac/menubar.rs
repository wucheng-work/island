//! The menu bar item (the palm island) and its menu, also shown on a right-click
//! of the island. It carries what works on the Mac so far; lyrics and the
//! Apple Music token join it with the music slice.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength,
};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSObject, NSObjectProtocol, NSSize, NSString};
use poni_core::settings::{IdleStyle, Settings, Theme};
use std::cell::RefCell;

pub use crate::services::login::Status as Login;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCmd {
    /// "Advanced settings…": the Settings window.
    ShowSettings,
    TogglePinned,
    /// "Move island to the centre": back from where it was dragged along the top edge.
    CentreIsland,
    SetTheme(Theme),
    SetIdleStyle(IdleStyle),
    /// true: hide in full-screen apps; false: stay on top of them.
    SetHideFullscreen(bool),
    ToggleLogin,
    ToggleLyrics,
    AppleMusicToken,
    AppleMusicHelp,
    AppleMusicForget,
    OpenSettings,
    ReloadSettings,
    OpenDataFolder,
    CheckUpdates,
    Quit,
}

pub enum MenuItem {
    Item {
        label: String,
        cmd: MenuCmd,
        checked: bool,
        key: &'static str,
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
        key: "",
    }
}

/// Move island to the centre shows while `s.island_x` is set (the app
/// passes it only where the island moves).
pub fn menu_items(s: &Settings, pinned: bool, login: Login) -> Vec<MenuItem> {
    let theme = |label: &str, t: Theme| item(label, MenuCmd::SetTheme(t), s.theme == t);
    let fullscreen = |label: &str, hide: bool| {
        item(
            label,
            MenuCmd::SetHideFullscreen(hide),
            s.hide_in_fullscreen == hide,
        )
    };
    // The theme's own idle looks (the status bar is Pill's), the one it
    // draws ticked: outside Pill a picked status bar shows as the clock.
    let shown = crate::pill::status::shown(s.theme, s.idle_style);
    let style = |st: IdleStyle| {
        let label = match st {
            IdleStyle::Clock => "时钟",
            // In the Oyen theme the eyes are Oyen's own.
            IdleStyle::Eyes if s.theme == Theme::Oyen => "Oyen",
            IdleStyle::Eyes => "眼睛",
            IdleStyle::Minimal => "极简",
            IdleStyle::Hidden => "悬停时显示",
            IdleStyle::Status => "状态栏",
        };
        item(label, MenuCmd::SetIdleStyle(st), shown == st)
    };
    let mut items = vec![
        MenuItem::Item {
            label: "高级设置…".into(),
            cmd: MenuCmd::ShowSettings,
            checked: false,
            key: ",",
        },
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
        MenuItem::Sub {
            label: "待机样式".into(),
            items: crate::pill::status::looks(s.theme)
                .iter()
                .map(|l| style(*l))
                .collect(),
        },
        item("同步歌词", MenuCmd::ToggleLyrics, s.lyrics),
        apple_music_menu(),
        MenuItem::Sub {
            label: "全屏应用中".into(),
            items: vec![fullscreen("隐藏", true), fullscreen("置顶显示", false)],
        },
        match login {
            Login::NeedsApproval => item(
                "登录时启动（需在系统设置中允许）",
                MenuCmd::ToggleLogin,
                false,
            ),
            Login::On => item("登录时启动", MenuCmd::ToggleLogin, true),
            _ => item("登录时启动", MenuCmd::ToggleLogin, false),
        },
        MenuItem::Separator,
        item("打开设置文件", MenuCmd::OpenSettings, false),
        item("重新加载设置", MenuCmd::ReloadSettings, false),
        item("打开数据文件夹", MenuCmd::OpenDataFolder, false),
        item("检查更新…", MenuCmd::CheckUpdates, false),
        MenuItem::Separator,
        MenuItem::Item {
            label: "退出 Island For Mac".into(),
            cmd: MenuCmd::Quit,
            checked: false,
            key: "q",
        },
    ];
    // Only while the island sits off the middle, under Keep island open.
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
            item("将灵动岛移到屏幕中央", MenuCmd::CentreIsland, false),
        );
    }
    items
}

/// Opt-in lyrics from Apple Music with the person's own token.
fn apple_music_menu() -> MenuItem {
    let on = crate::paths::apple_music_token_path().exists();
    let mut items = vec![
        item("从剪贴板导入令牌", MenuCmd::AppleMusicToken, false),
        item("如何获取令牌", MenuCmd::AppleMusicHelp, false),
    ];
    if on {
        items.push(MenuItem::Separator);
        items.push(item("移除令牌", MenuCmd::AppleMusicForget, false));
    }
    MenuItem::Sub {
        label: if on {
            "Apple Music 歌词：已启用".into()
        } else {
            "Apple Music 歌词".into()
        },
        items,
    }
}

define_class!(
    /// Receives the menu's clicks; each item's tag indexes the built commands.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PoniMenuTarget"]
    pub struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &NSMenuItem) {
            let cmd = CMDS.with(|c| c.borrow().get(sender.tag() as usize).copied());
            if let Some(cmd) = cmd {
                crate::app::on_menu(cmd);
            }
        }
    }
);

thread_local! {
    /// The commands behind the current menu's items (index = item tag).
    static CMDS: RefCell<Vec<MenuCmd>> = const { RefCell::new(Vec::new()) };
    /// The status item, for opening its menu from code (the smoke test).
    static ITEM: RefCell<Option<Retained<NSStatusItem>>> = const { RefCell::new(None) };
}

/// Open the menu bar item's menu, as a click on it would. Runs the menu's own
/// tracking loop, so call it with the app not borrowed.
pub fn open_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let item = ITEM.with(|i| i.borrow().clone());
    if let Some(button) = item.and_then(|i| i.button(mtm)) {
        unsafe { button.performClick(None) };
    }
}

pub struct MenuBar {
    item: Retained<NSStatusItem>,
    target: Retained<MenuTarget>,
    pub menu: Retained<NSMenu>,
}

impl MenuBar {
    pub fn add(mtm: MainThreadMarker, s: &Settings, pinned: bool, login: Login) -> MenuBar {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            match mark_image(mtm) {
                Some(img) => button.setImage(Some(&img)),
                None => button.setTitle(&NSString::from_str("Island For Mac")),
            }
            button.setToolTip(Some(&NSString::from_str("Island For Mac")));
        }
        let target: Retained<MenuTarget> =
            unsafe { objc2::msg_send![MenuTarget::alloc(mtm), init] };
        let menu = build(mtm, &target, &menu_items(s, pinned, login));
        item.setMenu(Some(&menu));
        ITEM.with(|i| *i.borrow_mut() = Some(item.clone()));
        MenuBar { item, target, menu }
    }

    /// Rebuild the menu (ticks follow the settings).
    pub fn refresh(&mut self, mtm: MainThreadMarker, s: &Settings, pinned: bool, login: Login) {
        self.menu = build(mtm, &self.target, &menu_items(s, pinned, login));
        self.item.setMenu(Some(&self.menu));
    }
}

fn build(mtm: MainThreadMarker, target: &MenuTarget, items: &[MenuItem]) -> Retained<NSMenu> {
    let mut cmds = Vec::new();
    let menu = build_into(mtm, target, items, &mut cmds);
    CMDS.with(|c| *c.borrow_mut() = cmds);
    menu
}

fn build_into(
    mtm: MainThreadMarker,
    target: &MenuTarget,
    items: &[MenuItem],
    cmds: &mut Vec<MenuCmd>,
) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(""));
    menu.setAutoenablesItems(false);
    for it in items {
        match it {
            MenuItem::Item {
                label,
                cmd,
                checked,
                key,
            } => {
                let label = crate::localization::translate(label);
                let mi = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(label.as_ref()),
                        Some(sel!(pick:)),
                        &NSString::from_str(key),
                    )
                };
                let t: &AnyObject = target.as_ref();
                unsafe { mi.setTarget(Some(t)) };
                mi.setTag(cmds.len() as isize);
                mi.setState(if *checked {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                cmds.push(*cmd);
                menu.addItem(&mi);
            }
            MenuItem::Sub { label, items } => {
                let sub = build_into(mtm, target, items, cmds);
                let label = crate::localization::translate(label);
                let mi = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(label.as_ref()),
                        None,
                        &NSString::from_str(""),
                    )
                };
                mi.setSubmenu(Some(&sub));
                menu.addItem(&mi);
            }
            MenuItem::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
        }
    }
    menu
}

/// The app icon's palm island as a one-colour template image.
/// macOS tints it for light and dark menu bars; no colour image is embedded.
fn mark_image(mtm: MainThreadMarker) -> Option<Retained<NSImage>> {
    const PT: f64 = 18.0;
    const SCALE: f32 = 2.0;
    let px = (PT as f32 * SCALE) as usize;
    let bmp = crate::gfx::Bitmap::new(px, px, SCALE)?;
    let ctx: &CGContext = bmp.ctx();
    let parse = |d: &str| {
        poni_core::svgpath::parse(d)
            .ok()
            .map(|segs| crate::gfx::cg_path(&segs))
    };
    let island = parse("M1 15 Q4 12 8 12 Q12 12 16.5 15 Q9 17 1 15 Z")?;
    let trunk = parse("M7 13 Q9 8 10 5 L11 5 Q10.5 9 9.5 13 Z")?;
    let leaves = parse("M10.5 5 Q7 0.5 4 3 Q7 3 9.5 5 Q5 3.5 2 7 Q6 5 9.5 5.5 Q6 6 5.5 10 Q8 7 10.5 5.5 Q12 8 14.5 9 Q14 6 11.5 5 Q14 5 17 7 Q16 3.5 11.5 4.5 Q13 2 15.5 2 Q12 0.5 10.5 5 Z")?;
    CGContext::set_rgb_fill_color(Some(ctx), 0.0, 0.0, 0.0, 1.0);
    for path in [&island, &trunk, &leaves] {
        CGContext::begin_path(Some(ctx));
        CGContext::add_path(Some(ctx), Some(path));
        CGContext::fill_path(Some(ctx));
    }
    let cg = bmp.image()?;
    let img = NSImage::initWithCGImage_size(
        NSImage::alloc(),
        &cg,
        NSSize::new(PT as CGFloat, PT as CGFloat),
    );
    img.setTemplate(true);
    let _ = mtm;
    Some(img)
}
