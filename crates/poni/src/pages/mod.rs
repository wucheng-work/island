//! Expanded-island pages. Each page module exposes:
//!
//! ```ignore
//! pub fn height(st: &AppState) -> f32;                        // content height (DIPs)
//! pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect);    // draw + handle input
//! ```
//!
//! Pages mutate `AppState` directly for local state and push
//! [`crate::commands::Command`]s for side effects (media control, opening
//! files, saving, …).

pub mod bluetooth;
pub mod card;
#[cfg(any(windows, target_os = "macos"))]
pub mod codex;
pub mod focus;
#[cfg(any(windows, target_os = "macos"))]
pub mod mail;
pub mod media;
pub mod notifications;
pub mod oyenza;
pub mod settings;
pub mod shelf;
pub mod system;
pub mod tasks;
pub mod today;

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, wid_n, Input, Ui};
use poni_core::datetime::fmt_clock;
use poni_core::geom::{lerp, palette, Color, Point, Rect};

/// Height of the header band (tabs + clock) above the page content. The
/// header row itself is a fixed 36 DIPs ([`header_rect`]); the rest is the
/// 12 DIP gap between the tab pills and the first line of content.
pub const HEADER_H: f32 = 50.0;
/// Height of the header row (the 28 DIP tab pills sit centred in it).
const HEADER_ROW_H: f32 = 36.0;
/// Horizontal padding inside the expanded island, also used below the content.
pub const PAD: f32 = 18.0;

/// 关闭歌词时音乐页收窄，重新开启后恢复用户设定的宽度。
pub fn expanded_width(page: PageKind, st: &AppState) -> f32 {
    if page == PageKind::Media && !st.settings.lyrics {
        st.settings.expanded_width.min(480.0)
    } else {
        st.settings.expanded_width
    }
}

/// The header row inside the expanded island `body`. Fixed size, so changing
/// [`HEADER_H`] only moves the content, never the tabs.
pub fn header_rect(body: Rect) -> Rect {
    Rect::new(
        body.x + PAD - 6.0,
        body.y + 6.0,
        body.w - 2.0 * PAD + 12.0,
        HEADER_ROW_H,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageKind {
    Media,
    Today,
    Shelf,
    Focus,
    #[cfg(any(windows, target_os = "macos"))]
    Mail,
    /// 保留给内部账户事件，不作为顶部标签。
    Oyenza,
    System,
    Bluetooth,
    Notifications,
    #[cfg(any(windows, target_os = "macos"))]
    Codex,
    /// 由右上角设置按钮打开。
    Settings,
}

impl PageKind {
    pub const ALL: [PageKind; 8 + 2 * cfg!(any(windows, target_os = "macos")) as usize] = [
        PageKind::Media,
        PageKind::Today,
        PageKind::Shelf,
        PageKind::Focus,
        #[cfg(any(windows, target_os = "macos"))]
        PageKind::Mail,
        PageKind::System,
        PageKind::Bluetooth,
        PageKind::Notifications,
        #[cfg(any(windows, target_os = "macos"))]
        PageKind::Codex,
        PageKind::Settings,
    ];

    pub fn key(self) -> &'static str {
        match self {
            PageKind::Media => "media",
            PageKind::Focus => "focus",
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Mail => "mail",
            PageKind::Shelf => "shelf",
            PageKind::Today => "today",
            PageKind::System => "system",
            PageKind::Bluetooth => "bluetooth",
            PageKind::Oyenza => "oyenza",
            PageKind::Notifications => "notifications",
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Codex => "codex",
            PageKind::Settings => "settings",
        }
    }

    pub fn from_key(s: &str) -> PageKind {
        let s = s.trim();
        if s.eq_ignore_ascii_case("tasks") {
            return PageKind::Today;
        }
        Self::ALL
            .into_iter()
            .find(|p| p.key().eq_ignore_ascii_case(s))
            .unwrap_or(PageKind::Media)
    }

    pub fn title(self) -> &'static str {
        match self {
            PageKind::Media => "Music",
            PageKind::Focus => "Focus",
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Mail => "邮件",
            PageKind::Shelf => "Shelf",
            PageKind::Today => "Today",
            PageKind::System => "System",
            PageKind::Bluetooth => "Bluetooth",
            PageKind::Oyenza => "Oyenza",
            PageKind::Notifications => "Notifications",
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Codex => "Codex",
            PageKind::Settings => "Settings",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            PageKind::Media => Icon::Music,
            PageKind::Focus => Icon::Timer,
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Mail => Icon::Mail,
            PageKind::Shelf => Icon::Tray,
            PageKind::Today => Icon::Calendar,
            PageKind::System => Icon::Gauge,
            PageKind::Bluetooth => Icon::Bluetooth,
            PageKind::Oyenza => Icon::Wallet,
            PageKind::Notifications => Icon::Bell,
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Codex => Icon::Codex,
            PageKind::Settings => Icon::Gear,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    /// Position in the tab row (for slide direction). Settings comes last:
    /// its gear sits right of the tabs.
    pub fn tab_index(self) -> usize {
        self.index()
    }

    /// Has a tab in the header's row (Settings and the hidden Oyenza account
    /// page are opened through other internal flows).
    pub fn tabbed(self) -> bool {
        !matches!(self, PageKind::Settings | PageKind::Oyenza)
    }

    /// 设置中可隐藏的基础标签。
    pub const HIDEABLE: [PageKind; 7 + 2 * cfg!(any(windows, target_os = "macos")) as usize] = [
        PageKind::Media,
        PageKind::Today,
        PageKind::Shelf,
        PageKind::Focus,
        PageKind::System,
        PageKind::Bluetooth,
        #[cfg(any(windows, target_os = "macos"))]
        PageKind::Mail,
        #[cfg(any(windows, target_os = "macos"))]
        PageKind::Codex,
        PageKind::Notifications,
    ];

    pub fn hideable(self) -> bool {
        Self::HIDEABLE.contains(&self)
    }
}

/// 当前可见的标签。系统页跟随设置开关，通知页跟随系统权限。
pub fn tabs(st: &AppState) -> Vec<PageKind> {
    row(st, Some(st.page))
}

/// The tabs Settings can switch off in this copy (its tabs row, and Tabs in
/// the Settings window): the island's own pages, and Notifications, the
/// last of [`PageKind::HIDEABLE`], only where the copy can read them
/// (`AppState::notify_can`).
pub fn hideable_tabs(st: &AppState) -> &'static [PageKind] {
    let all = &PageKind::HIDEABLE;
    if st.notify_can {
        all
    } else {
        &all[..all.len() - 1]
    }
}

/// 当前页面被关闭或隐藏后，返回首个可见标签。
pub fn first_tab(st: &AppState) -> PageKind {
    row(st, None)[0]
}

fn row(st: &AppState, open: Option<PageKind>) -> Vec<PageKind> {
    let mut tabs: Vec<PageKind> = PageKind::ALL
        .into_iter()
        .filter(|&p| match p {
            PageKind::System => st.system_on(),
            PageKind::Notifications => st.notifications_on(),
            p if p.hideable() => !st.tab_hidden(p) || open == Some(p),
            p => p.tabbed(),
        })
        .collect();
    // 旧设置允许隐藏全部基础页；移除拓展入口后仍需有一个可进入的页面。
    if tabs.is_empty() {
        tabs.push(PageKind::Media);
    }
    tabs
}

/// The tab after (or before) `p` among [`tabs`], wrapping around.
pub fn step_tab(p: PageKind, st: &AppState, forward: bool) -> PageKind {
    let tabs = tabs(st);
    let n = tabs.len();
    let i = tabs.iter().position(|t| *t == p).unwrap_or(0);
    tabs[if forward {
        (i + 1) % n
    } else {
        (i + n - 1) % n
    }]
}

pub fn content_height(p: PageKind, st: &AppState) -> f32 {
    let warning = if st.save_errors.any() { 56.0 } else { 0.0 };
    warning
        + match p {
            PageKind::Media => media::height(st),
            PageKind::Focus => focus::height(st),
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Mail => mail::height(st),
            PageKind::Shelf => shelf::height(st),
            PageKind::Today => today::height(st),
            PageKind::System => system::height(st),
            PageKind::Bluetooth => bluetooth::height(st),
            PageKind::Oyenza => oyenza::height(st),
            PageKind::Notifications => notifications::height(st),
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Codex => codex::height(st),
            PageKind::Settings => settings::height(st),
        }
}

/// The next time (mono seconds) the expanded `page` changes on its own and
/// needs a frame without input, if any (e.g. an Undo pill expiring). Meant
/// for the app's wake timer; until it asks, such changes show on the next
/// repaint.
pub fn next_wake(page: PageKind, st: &AppState) -> Option<f64> {
    let wake = match page {
        PageKind::Shelf => shelf::next_wake(st),
        PageKind::Oyenza => oyenza::next_wake(st),
        PageKind::Today => today::next_wake(st),
        _ => None,
    };
    // Oyen beside the pin in his theme blinks and flicks an ear now and then;
    // asleep at night, he stays still.
    let oyen = (!crate::activities::night(st)).then(|| crate::oyen::next_rest_move(st.now));
    match (wake, oyen) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// A page switch in progress: the page we came from, 0..1 progress, and the
/// slide direction (+1 = moving to a tab on the right).
#[derive(Clone, Copy, Debug)]
pub struct PageTransition {
    pub from: PageKind,
    pub t: f32,
    pub dir: f32,
}

/// How far (DIPs) pages slide during a switch.
const SLIDE: f32 = 20.0;
/// The old page is gone by this much of the switch...
const OUT_END: f32 = 0.4;
/// ...while the new one fades in over this window.
const IN_START: f32 = 0.1;
const IN_LEN: f32 = 0.5;
/// The header's selection pill and labels settle by this much of the switch.
const TABS_END: f32 = 0.6;

/// Draw the header and the active page inside the expanded island `body`.
/// During a page switch the old page fades out sliding away while the new
/// one fades in; the old page is drawn without input (it can't be clicked).
/// `body` is the island at its final size, so nothing reflows while it opens.
pub fn draw_expanded(
    ui: &mut Ui,
    st: &mut AppState,
    body: Rect,
    page: PageKind,
    transition: Option<PageTransition>,
) {
    if crate::pill::expanded::draw(ui, st, body, page, transition) {
        return;
    }
    let tr = transition.filter(|tr| tr.t < 1.0 && tr.from != page);
    let header = header_rect(body);
    let tip = draw_header(ui, st, header, page, tr);
    draw_pages(ui, st, body, header, page, tr);
    // Hover labels sit on top of the page, outside the content clip.
    if let Some(tip) = tip {
        draw_tip(ui, header, &tip);
    }
}

/// The page content's rect inside the expanded island `body`: below the
/// header band, `PAD` in from the sides and the bottom.
pub(crate) fn content_area(body: Rect) -> Rect {
    Rect::new(
        body.x + PAD,
        body.y + HEADER_H,
        body.w - 2.0 * PAD,
        (body.h - HEADER_H - PAD).max(0.0),
    )
}

/// The active page in [`content_area`], clipped to the island below the
/// `header` row. During the page switch `tr` (one in progress, as
/// [`draw_expanded`] filters it) the old page fades out sliding away while
/// the new one fades in. Every theme's frame draws its pages with this, so
/// a switch moves the same in all of them.
pub(crate) fn draw_pages(
    ui: &mut Ui,
    st: &mut AppState,
    body: Rect,
    header: Rect,
    page: PageKind,
    tr: Option<PageTransition>,
) {
    let mut area = content_area(body);
    // Content may reach a little above `area` (text boxes whose cap height,
    // not their line box, sits on the content top) but never into the tabs.
    let clip_top = header.bottom() + 2.0;
    ui.c.clip(Rect::new(
        body.x,
        clip_top,
        body.w,
        (body.bottom() - clip_top).max(0.0),
    ));
    // 页内保留轻柔底色，让内容与收起时的黑色胶囊形成层次。
    let panel = Rect::new(
        body.x + 8.0,
        clip_top + 2.0,
        body.w - 16.0,
        (body.bottom() - clip_top - 10.0).max(0.0),
    );
    let wash = if ui.c.look().light { 0.035 } else { 0.08 };
    ui.c.fill_rrect(panel, 20.0, palette::WARM_ACCENT.with_alpha(wash));
    if st.save_errors.any() {
        let warning = Rect::new(area.x, area.y, area.w, 46.0);
        let (retry, discard) = crate::ui::save_warning(ui, &st.save_errors, warning);
        if retry {
            st.cmd(Command::RetrySave);
        }
        if discard {
            st.cmd(Command::QuitWithoutSaving);
        }
        area.y += 56.0;
        area.h = (area.h - 56.0).max(0.0);
    }
    match tr {
        Some(tr) => {
            // Overlapping smoothstep fades: the summed opacity stays at or
            // above about 0.5, so dense pages don't dip into a muddy grey.
            let out_a = 1.0 - smoothstep(tr.t / OUT_END);
            let in_a = smoothstep((tr.t - IN_START) / IN_LEN);
            let base = ui.c.alpha();
            // Restore (never reset) the transform: the app may be scaling the
            // whole island content while it opens.
            let m0 = ui.c.save_transform();
            if out_a > 0.0 {
                // Outgoing page: ghost UI (no input, throwaway hit list).
                let mut ghost_input = Input::default();
                let mut ghost_hits = Vec::new();
                let now = ui.now;
                let mut ghost = Ui::new(&mut *ui.c, &mut ghost_input, &mut ghost_hits, now);
                ghost.c.set_alpha(base * out_a);
                ghost.c.translate(-tr.dir * SLIDE * (1.0 - out_a), 0.0);
                draw_page(&mut ghost, st, area, tr.from);
                ghost.c.restore_transform(m0);
            }
            // The new page always draws (even fully transparent) so its hit
            // regions and animation state stay continuous.
            ui.c.set_alpha(base * in_a);
            ui.c.translate(tr.dir * SLIDE * (1.0 - in_a), 0.0);
            draw_page(ui, st, area, page);
            ui.c.restore_transform(m0);
            ui.c.set_alpha(base);
            ui.request_anim();
        }
        None => draw_page(ui, st, area, page),
    }
    ui.c.unclip();
}

pub(crate) fn draw_page(ui: &mut Ui, st: &mut AppState, area: Rect, page: PageKind) {
    match page {
        PageKind::Media => media::draw(ui, st, area),
        PageKind::Focus => focus::draw(ui, st, area),
        #[cfg(any(windows, target_os = "macos"))]
        PageKind::Mail => mail::draw(ui, st, area),
        PageKind::Shelf => shelf::draw(ui, st, area),
        PageKind::Today => today::draw(ui, st, area),
        PageKind::System => system::draw(ui, st, area),
        PageKind::Bluetooth => bluetooth::draw(ui, st, area),
        PageKind::Oyenza => oyenza::draw(ui, st, area),
        PageKind::Notifications => notifications::draw(ui, st, area),
        #[cfg(any(windows, target_os = "macos"))]
        PageKind::Codex => codex::draw(ui, st, area),
        PageKind::Settings => settings::draw(ui, st, area),
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Round to the device-pixel grid.
pub(crate) fn snap(v: f32, scale: f32) -> f32 {
    (v * scale).round() / scale
}

// ---------------------------------------------------------------- header

const TAB_H: f32 = 28.0;
/// Inactive tabs are bare icons in a 30 DIP slot.
const TAB_W: f32 = 30.0;
const TAB_GAP: f32 = 2.0;
/// The active tab's label starts this far in and ends `TAB_LABEL_END` before its edge.
const TAB_LABEL_X: f32 = 28.0;
const TAB_LABEL_END: f32 = 12.0;
/// Clock to date, and date (or clock) to the Settings button, which sits
/// right of the pin.
const DATE_GAP: f32 = 12.0;
const PIN_GAP: f32 = 10.0;
const PIN: f32 = 28.0;
const GEAR: f32 = 28.0;
const GEAR_GAP: f32 = 2.0;
/// His theme: Oyen peeks up beside the pin, as he does beside the clock when
/// the island is closed.
const OYEN_W: f32 = 24.0;
const OYEN_GAP: f32 = 6.0;
/// Hover labels: dwell, fade-in, capsule metrics.
const TIP_DELAY: f64 = 0.4;
const TIP_FADE: f64 = 0.12;
pub(crate) const TIP_H: f32 = 22.0;
pub(crate) const TIP_PAD: f32 = 8.0;
const TIP_RADIUS: f32 = 8.0;
/// White 0.16 over the black island, made opaque so page text can't show through.
pub(crate) const TIP_FILL: Color = Color::rgba(0.16, 0.16, 0.16, 1.0);
/// `tab_hover` slots of the pin and Settings buttons (tabs use their place in the row).
const PIN_SLOT: usize = PageKind::ALL.len();
const GEAR_SLOT: usize = PIN_SLOT + 1;
/// The Settings button's widget id (the self-test clicks it).
pub const GEAR_ID: &str = "header.gear";

/// A hover label waiting to be painted above the page.
pub(crate) struct Tip {
    pub(crate) anchor: Rect,
    pub(crate) text: &'static str,
    pub(crate) alpha: f32,
}

/// Tab slots (x, width) of `tabs`, with `active` showing its label.
fn tab_layout(
    ui: &mut Ui,
    x0: f32,
    tabs: &[PageKind],
    active: PageKind,
    label_st: &TextStyle,
    scale: f32,
) -> Vec<(f32, f32)> {
    let mut x = x0;
    tabs.iter()
        .map(|&p| {
            let w = if p == active {
                snap(
                    TAB_LABEL_X + ui.c.measure(p.title(), label_st, 200.0).0 + TAB_LABEL_END,
                    scale,
                )
            } else {
                TAB_W
            };
            let slot = (snap(x, scale), w);
            x += w + TAB_GAP;
            slot
        })
        .collect()
}

/// Tabs on the left; pin, date and clock on the right. During a page switch
/// the tabs re-flow, the selection pill slides and the labels cross-fade.
/// Returns the hover label to paint once the page is drawn.
pub(crate) fn draw_header(
    ui: &mut Ui,
    st: &mut AppState,
    r: Rect,
    page: PageKind,
    tr: Option<PageTransition>,
) -> Option<Tip> {
    let scale = ui.c.scale().max(0.5);
    let cy = r.center().y;
    let label_st = TextStyle::body().weight(Weight::Semibold).color(st.accent);
    let from = tr.map_or(page, |t| t.from);
    let e = tr.map_or(1.0, |t| ease_out_cubic(t.t / TABS_END));
    let row = self::tabs(st);
    let lay_from = tab_layout(ui, r.x, &row, from, &label_st, scale);
    let lay_to = if from == page {
        lay_from.clone()
    } else {
        tab_layout(ui, r.x, &row, page, &label_st, scale)
    };
    let tabs: Vec<Rect> = (0..row.len())
        .map(|i| {
            Rect::new(
                lerp(lay_from[i].0, lay_to[i].0, e),
                cy - TAB_H / 2.0,
                lerp(lay_from[i].1, lay_to[i].1, e),
                TAB_H,
            )
        })
        .collect();
    // A page that isn't in the row counts as the first tab while its internal
    // transition finishes.
    let place = |p: PageKind| row.iter().position(|t| *t == p).unwrap_or(0);
    let (fi, pi) = (place(from), place(page));
    let tabs_end = tabs[tabs.len() - 1].right();
    // How much the selection sits on the gear (Settings) rather than on a tab.
    let on_gear = |p: PageKind| if p.tabbed() { 0.0 } else { 1.0 };
    let gear_k = lerp(on_gear(from), on_gear(page), e);

    // Selection pill, gliding from the old tab to the new one. To or from
    // Settings it stays on its tab and fades while the gear takes it.
    let (a, b) = (
        if from.tabbed() { fi } else { pi },
        if page.tabbed() { pi } else { fi },
    );
    let pill = Rect::new(
        lerp(lay_from[a].0, lay_to[b].0, e),
        cy - TAB_H / 2.0,
        lerp(lay_from[a].1, lay_to[b].1, e),
        TAB_H,
    );
    if gear_k < 1.0 {
        ui.c.with_alpha(1.0 - gear_k, |c| {
            c.fill_rrect(pill, TAB_H / 2.0, st.accent.with_alpha(0.18));
            c.stroke_rrect(pill, TAB_H / 2.0, st.accent.with_alpha(0.25), 1.0);
        });
    }

    let mut hover: Option<(usize, Rect)> = None;
    for (i, &p) in row.iter().enumerate() {
        let tr = tabs[i];
        let resp = ui.button(wid_n("tab", i as u64), tr);
        let active = p == page;
        if resp.hovered && !active {
            ui.c.fill_rrect(tr, TAB_H / 2.0, palette::SURFACE);
            hover = Some((i, tr));
        }
        let k = if active {
            e
        } else if from.tabbed() && i == fi {
            1.0 - e
        } else {
            0.0
        };
        let ic = if resp.hovered && !active {
            palette::TEXT
        } else {
            Color::lerp(palette::TEXT_DIM, st.accent, k)
        };
        // 原始图形在 24 格画布中的留白不同；按实际轮廓校正导航专用大小与中心。
        // 轮廓最长边约 14 DIP，密集的 ChatGPT 标志略缩小，点击区域仍使用原 tab。
        let (icon_size, shape_cx, shape_cy) = match p {
            PageKind::Media => (18.0, 11.5, 12.25),
            PageKind::Today => (18.0, 12.0, 11.5),
            PageKind::Shelf => (19.0, 12.0, 12.35),
            PageKind::Focus => (17.0, 12.25, 11.75),
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Mail => (16.0, 12.0, 12.0),
            PageKind::System => (18.0, 12.0, 12.65),
            PageKind::Bluetooth => (17.0, 12.0, 12.0),
            PageKind::Notifications => (18.0, 12.0, 12.25),
            #[cfg(any(windows, target_os = "macos"))]
            PageKind::Codex => (14.0, 12.0, 11.9),
            _ => (16.0, 12.0, 12.0),
        };
        let icon_scale = icon_size / 24.0;
        let ix = snap(tr.x + TAB_W / 2.0 - shape_cx * icon_scale, scale);
        let iy = snap(cy - shape_cy * icon_scale, scale);
        ui.c.icon(p.icon(), Rect::new(ix, iy, icon_size, icon_size), ic);
        if resp.clicked && !active {
            st.cmd(Command::SetPage(p));
        }
    }
    if from.tabbed() && (fi != pi || !page.tabbed()) {
        draw_tab_label(ui, from, tabs[fi], &label_st, 1.0 - e);
    }
    if page.tabbed() {
        draw_tab_label(ui, page, tabs[pi], &label_st, e);
    }

    // 日期和时间在所有页面使用同一布局，只在可用宽度不足时隐藏日期。
    let right = r.right() - 6.0;
    let time = fmt_clock(&st.local, st.settings.use_24h);
    let time_st = TextStyle::body().weight(Weight::Semibold).tabular().right();
    let tw = ui.c.measure(&time, &time_st, 200.0).0;
    ui.c.text(
        &time,
        Rect::new(right - tw - 4.0, r.y, tw + 4.0, r.h),
        &time_st,
    );
    let mut left = right - tw;

    let date = crate::localization::date_short(&st.local);
    let date_st = TextStyle::footnote().color(palette::TEXT_DIM).right();
    let dw = ui.c.measure(&date, &date_st, 200.0).0;
    let date_left = left - DATE_GAP - dw;
    if date_left > tabs_end + 40.0 {
        let date_r = Rect::new(date_left - 2.0, r.y, dw + 2.0, r.h);
        ui.c.text(&date, date_r, &date_st);
        left = date_left;
    }

    let gear_r = Rect::new(
        snap(left - PIN_GAP - GEAR, scale),
        cy - GEAR / 2.0,
        GEAR,
        GEAR,
    );
    let pin_r = Rect::new(
        snap(gear_r.x - GEAR_GAP - PIN, scale),
        cy - PIN / 2.0,
        PIN,
        PIN,
    );
    let oyen_cx = pin_r.x - OYEN_GAP - OYEN_W / 2.0;
    if crate::oyen::on(st.settings.theme) && oyen_cx - OYEN_W / 2.0 > tabs_end + 12.0 {
        // Eyes on the cursor while it is over the island; asleep at night.
        let look = ui
            .input
            .mouse
            .map(|p| ((p.x - r.center().x) / (r.w / 2.0)).clamp(-1.0, 1.0));
        let (frame, moving) = crate::oyen::rest(st.now, st.local.hour < 5, look);
        if moving {
            ui.request_anim();
        }
        crate::oyen::peek(ui.c, &frame, oyen_cx, cy + 9.0, OYEN_W, r.y);
    }
    if pin_r.x > tabs_end + 8.0 {
        let pinned = st.pinned;
        let (icon, color) = if pinned {
            (Icon::PinFill, st.accent)
        } else {
            (Icon::Pin, palette::TEXT_FAINT)
        };
        let resp = ui.icon_button(wid("pin"), pin_r, icon, color, 15.0);
        if resp.hovered {
            hover = Some((PIN_SLOT, pin_r));
        }
        if resp.clicked {
            st.cmd(Command::SetPinned(!pinned));
        }
    }
    // Settings: the gear is lit like a selected tab while its page shows,
    // and a second press goes back to the page it was pressed on. The page
    // has the everyday settings; its last row opens Advanced settings, the
    // window with every setting.
    if gear_r.x > tabs_end + 8.0 {
        if gear_k > 0.0 {
            ui.c.with_alpha(gear_k, |c| {
                c.fill_rrect(gear_r, GEAR / 2.0, st.accent.with_alpha(0.18))
            });
        }
        let color = Color::lerp(palette::TEXT_DIM, st.accent, gear_k);
        let resp = ui.icon_button(wid(GEAR_ID), gear_r, Icon::Gear, color, 16.0);
        if resp.hovered {
            hover = Some((GEAR_SLOT, gear_r));
        }
        if resp.clicked {
            if page == PageKind::Settings {
                let back = st
                    .ui
                    .before_settings
                    .take()
                    .filter(|p| row.contains(p))
                    .unwrap_or(row[0]);
                st.cmd(Command::SetPage(back));
            } else {
                st.ui.before_settings = Some(page);
                st.cmd(Command::SetPage(PageKind::Settings));
            }
        }
    }

    // Hover labels: after a short dwell over an inactive tab, the pin or the gear.
    let now = ui.now;
    st.ui.tab_hover = match (hover, st.ui.tab_hover) {
        (Some((i, _)), Some((j, t0))) if i == j => Some((j, t0)),
        (Some((i, _)), _) => Some((i, now)),
        (None, _) => None,
    };
    let (slot, anchor) = hover?;
    let since = now - st.ui.tab_hover.map_or(now, |h| h.1);
    if since < TIP_DELAY + TIP_FADE {
        ui.request_anim();
    }
    let alpha = ((since - TIP_DELAY) / TIP_FADE).clamp(0.0, 1.0) as f32;
    if alpha <= 0.0 {
        return None;
    }
    let text = match row.get(slot) {
        Some(p) => p.title(),
        None if slot == GEAR_SLOT && page == PageKind::Settings => "Close settings",
        None if slot == GEAR_SLOT => "Settings",
        None if st.pinned => "Unpin",
        None => "Keep open",
    };
    Some(Tip {
        anchor,
        text,
        alpha,
    })
}

/// A tab's label, faded by `alpha` and cut (not ellipsized) at the tab's
/// current right edge while the tab is narrower than its settled width.
fn draw_tab_label(ui: &mut Ui, p: PageKind, tab: Rect, st: &TextStyle, alpha: f32) {
    if alpha <= 0.01 {
        return;
    }
    let lx = tab.x + TAB_LABEL_X;
    let avail = tab.right() - TAB_LABEL_END + 2.0 - lx;
    if avail <= 0.5 {
        return;
    }
    let lw = ui.c.measure(p.title(), st, 200.0).0;
    let cut = avail < lw + 1.0;
    if cut {
        ui.c.clip(Rect::new(lx, tab.y, avail, tab.h));
    }
    ui.c.with_alpha(alpha, |c| {
        c.text(p.title(), Rect::new(lx, tab.y, lw + 2.0, tab.h), st)
    });
    if cut {
        ui.c.unclip();
    }
}

/// A small capsule centred under the hovered tab (kept within the header's width).
fn draw_tip(ui: &mut Ui, header: Rect, tip: &Tip) {
    let scale = ui.c.scale().max(0.5);
    let st = TextStyle::footnote().center();
    let tw = ui.c.measure(tip.text, &st, 300.0).0;
    let w = snap((tw + 2.0 * TIP_PAD).ceil(), scale);
    let x = (tip.anchor.center().x - w / 2.0).clamp(header.x, (header.right() - w).max(header.x));
    let r = Rect::new(snap(x, scale), snap(header.bottom() + 4.0, scale), w, TIP_H);
    ui.c.with_alpha(tip.alpha, |c| {
        c.fill_rrect(r, TIP_RADIUS, TIP_FILL);
        c.text(tip.text, r, &st);
    });
}

/// Shared helper: a centred, dim placeholder message with an icon.
pub fn empty_state(ui: &mut Ui, area: Rect, icon: Icon, title: &str, hint: &str) {
    let c = area.center();
    let badge = Point::new(c.x, c.y - 28.0);
    ui.c.fill_circle(badge, 21.0, palette::SURFACE);
    ui.c.icon(
        icon,
        Rect::from_center(badge, 23.0, 23.0),
        palette::TEXT_DIM,
    );
    ui.c.text(
        title,
        Rect::new(area.x, c.y + 1.0, area.w, 20.0),
        &TextStyle::headline().center(),
    );
    ui.c.text(
        hint,
        Rect::new(area.x, c.y + 22.0, area.w, 18.0),
        &TextStyle::footnote().color(palette::TEXT_DIM).center(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::OyenzaLink;
    use poni_core::settings::Settings;
    use poni_core::store::Store;

    fn state() -> AppState {
        AppState::new(Settings::default(), Store::default())
    }

    #[test]
    fn the_row_is_the_island_s_own_pages() {
        use PageKind::*;
        let st = state();
        assert_eq!(
            tabs(&st),
            [
                Media,
                Today,
                Shelf,
                Focus,
                #[cfg(any(windows, target_os = "macos"))]
                Mail,
                Bluetooth,
                #[cfg(any(windows, target_os = "macos"))]
                Codex
            ]
        );
        assert_eq!(PageKind::from_key("tasks"), Today);
        assert_eq!(PageKind::from_key("clipboard"), Media);
        assert_eq!(PageKind::from_key("oyenza"), Media);
    }

    #[test]
    fn hidden_tabs_leave_the_row_but_not_while_their_page_is_open() {
        use PageKind::*;
        let mut st = state();
        st.settings.hidden_tabs = vec![
            "today".into(),
            "tasks".into(),
            "oyenza".into(),
            "addons".into(),
        ];
        st.page = Focus;
        // 旧拓展键不影响保留页面的标签。
        assert_eq!(
            tabs(&st),
            [
                Media,
                Shelf,
                Focus,
                #[cfg(any(windows, target_os = "macos"))]
                Mail,
                Bluetooth,
                #[cfg(any(windows, target_os = "macos"))]
                Codex
            ]
        );
        assert_eq!(first_tab(&st), Media);
        // Opened from elsewhere (a meeting's notice opens Today): its tab
        // shows while it's open, and stepping on leaves it behind.
        st.page = Today;
        assert_eq!(
            tabs(&st),
            [
                Media,
                Today,
                Shelf,
                Focus,
                #[cfg(any(windows, target_os = "macos"))]
                Mail,
                Bluetooth,
                #[cfg(any(windows, target_os = "macos"))]
                Codex
            ]
        );
        st.page = step_tab(Today, &st, true);
        assert_eq!(st.page, Shelf);
        assert!(!tabs(&st).contains(&Today));
        // 全部标签隐藏时仍保留音乐入口。
        st.settings.hidden_tabs = PageKind::HIDEABLE
            .iter()
            .map(|p| p.key().to_string())
            .collect();
        st.page = Oyenza;
        assert_eq!(tabs(&st), [Media]);
        assert_eq!(first_tab(&st), Media);
        // A start on a hidden page opens on the first tab instead.
        let mut settings = poni_core::settings::Settings::default();
        settings.start_page = "today".into();
        settings.remember_page = false;
        settings.hidden_tabs = vec!["media".into(), "today".into()];
        assert_eq!(AppState::new(settings, Store::default()).page, Shelf);
    }

    #[test]
    fn settings_has_the_gear_not_a_tab() {
        let mut st = state();
        assert!(!tabs(&st).contains(&PageKind::Settings));
        assert!(!tabs(&st).contains(&PageKind::Oyenza));
        st.oyenza.link = OyenzaLink::Connected {
            account: "a•••@example.com".into(),
        };
        let row = tabs(&st);
        assert!(!row.contains(&PageKind::Oyenza) && !row.contains(&PageKind::Settings));
        // Stepping through the tabs never lands on Settings.
        let mut p = PageKind::Media;
        for _ in 0..2 * PageKind::ALL.len() {
            p = step_tab(p, &st, true);
            assert!(p.tabbed(), "stepped onto {p:?}");
        }
    }

    #[test]
    fn settings_keeps_its_key_and_slides_in_from_the_right() {
        assert_eq!(PageKind::from_key("settings"), PageKind::Settings);
        assert!(PageKind::ALL
            .iter()
            .all(|p| p.tab_index() <= PageKind::Settings.tab_index()));
    }

    #[test]
    fn system_has_a_tab_only_while_enabled_in_settings() {
        let mut st = state();
        assert!(!tabs(&st).contains(&PageKind::System));
        st.settings.set_system(true);
        assert!(tabs(&st).contains(&PageKind::System));
        let store = Store {
            last_page: "system".into(),
            ..Store::default()
        };
        let mut settings = Settings {
            remember_page: true,
            ..Settings::default()
        };
        assert_eq!(
            AppState::new(settings.clone(), store.clone()).page,
            PageKind::Media
        );
        settings.set_system(true);
        assert_eq!(AppState::new(settings, store).page, PageKind::System);
    }

    #[test]
    fn notifications_is_on_wherever_the_copy_can_read_them_until_switched_off() {
        // The download can't read other apps' notifications: no tab, nothing listens.
        let mut st = state();
        assert!(!st.notifications_on() && !tabs(&st).contains(&PageKind::Notifications));
        assert_eq!(
            hideable_tabs(&st),
            &PageKind::HIDEABLE[..PageKind::HIDEABLE.len() - 1],
            "Settings doesn't offer it"
        );
        // The Store copy: on for everyone, without an add-on to switch on.
        st.notify_can = true;
        assert!(st.notifications_on() && tabs(&st).contains(&PageKind::Notifications));
        assert_eq!(hideable_tabs(&st).last(), Some(&PageKind::Notifications));
        // Its chip in Settings' tabs row switches it off, tab and all, even
        // while its page is open.
        st.settings.hidden_tabs = vec!["notifications".into()];
        st.page = PageKind::Notifications;
        assert!(!st.notifications_on() && !tabs(&st).contains(&PageKind::Notifications));
        assert_eq!(first_tab(&st), PageKind::Media);
    }

    #[test]
    fn settings_never_makes_the_window_taller() {
        // The window fits the tallest page; System is always 256 DIPs (Settings
        // has three rows of switches since Clipboard history left).
        let mut st = state();
        assert!(content_height(PageKind::Settings, &st) <= content_height(PageKind::System, &st));
        // Nor its new version's row, nor the Update sheet in its place.
        let rel = poni_core::update::Release {
            version: "9.9.9".into(),
            date: String::new(),
            headline: "Something new".into(),
            notes: String::new(),
            windows: None,
            mac: None,
        };
        st.update.offer = Some(rel);
        let row = content_height(PageKind::Settings, &st);
        st.update.sheet = true;
        let sheet = content_height(PageKind::Settings, &st);
        assert!(
            row <= content_height(PageKind::System, &st) && sheet <= row,
            "{row} {sheet}"
        );
    }
}
