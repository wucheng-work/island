//! 岛内常用设置，与完整设置窗口共用控件和即时保存行为。
//! 点击标题区“全部设置”后收起小岛，再打开独立设置窗口。

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::pill::status;
use crate::state::{AppState, Said};
use crate::ui::{wid, wid_n, PillStyle, Ui};
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::settings::{IdleStyle, Theme};

/// 岛内快捷设置的内容高度；全部设置由独立窗口承载。
const HEIGHT: f32 = 244.0;
const ROW_BLEED: f32 = 8.0;
const SEG_INSET: f32 = 2.0;
const SEG_PAD: f32 = 11.0;
const SWITCH_W: f32 = 30.0;
const CHECK_PAD: f32 = 5.0;
const CHECK_H: f32 = 22.0;
const SWITCH_H: f32 = 18.0;
const KNOB_INSET: f32 = 2.0;
/// The tabs row's chips, like the Record page's inputs.
const CHIP_H: f32 = 24.0;
const CHIP_PAD: f32 = 8.0;
const CHIP_ICON: f32 = 14.0;
const CHIP_ICON_GAP: f32 = 5.0;
const CHIP_GAP: f32 = 6.0;

// Glass: on Windows from a copy of the screen, on a Mac macOS's own glass.
const THEMES: [(Theme, &str); 5] = [
    (Theme::Classic, "Classic"),
    (Theme::Oyen, "Oyen"),
    (Theme::Glass, "Glass"),
    (Theme::Pill, "Pill"),
    (Theme::Diorama, "Diorama"),
];

/// An idle look's name in the picker (`eyes` is Oyen in his theme).
fn idle_name(s: IdleStyle, eyes: &'static str) -> &'static str {
    match s {
        IdleStyle::Clock => "Clock",
        IdleStyle::Eyes => eyes,
        IdleStyle::Minimal => "Minimal",
        IdleStyle::Hidden => "Hidden",
        IdleStyle::Status => "Status",
    }
}

/// The idle looks `theme` offers, as the picker names them.
fn idle_names(theme: Theme) -> Vec<&'static str> {
    // In the Oyen theme the eyes are Oyen's own (as in the tray menu).
    let eyes = if theme == Theme::Oyen { "Oyen" } else { "Eyes" };
    status::looks(theme)
        .iter()
        .map(|s| idle_name(*s, eyes))
        .collect()
}

/// The switches, as the tray menu names them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Switch {
    Lyrics,
    Autostart,
}

const ROWS: usize = if cfg!(target_os = "macos") { 3 } else { 2 };

/// The full-screen choices as the tray menu names them, and the
/// `hide_in_fullscreen` each one sets.
const FULLSCREEN: [(bool, &str); 2] = [(true, "Hide"), (false, "Stay on top")];

/// 标题区“全部设置”入口（自测用此ID点击）。
pub const ADVANCED_ID: &str = "settings.advanced";

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

/// 岛内只呈现常用设置；完整设置入口在标题区，不再伪装成底部开关行。
pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let card = super::card::pill_card(st, area);
    let accent = st.accent;
    let caption = TextStyle::caption()
        .weight(Weight::Semibold)
        .color(palette::TEXT_DIM);
    ui.c.text(
        "快捷设置",
        Rect::new(area.x + 2.0, area.y, 160.0, 17.0),
        &TextStyle::headline(),
    );
    ui.c.text(
        "常用偏好，即时生效",
        Rect::new(area.x + 2.0, area.y + 18.0, 200.0, 13.0),
        &TextStyle::caption().color(palette::TEXT_DIM),
    );
    let advanced = Rect::new(area.right() - 96.0, area.y + 2.0, 96.0, 28.0);
    let response = ui.button(wid(ADVANCED_ID), advanced);
    let fill = if response.pressed {
        0.12
    } else if response.hovered {
        0.08
    } else {
        0.045
    };
    super::card::fill_box(ui, card, advanced, 8.0, Color::white(fill));
    super::card::stroke_box(ui, card, advanced, 8.0, Color::white(0.10), 1.0);
    ui.c.text(
        "全部设置",
        Rect::new(advanced.x + 12.0, advanced.y, 58.0, advanced.h),
        &TextStyle::footnote().weight(Weight::Semibold),
    );
    ui.c.icon(
        Icon::Open,
        Rect::new(
            advanced.right() - 23.0,
            advanced.center().y - 6.0,
            12.0,
            12.0,
        ),
        palette::TEXT_DIM,
    );
    if response.clicked {
        st.cmd(Command::OpenSettings);
    }

    let appearance = Rect::new(area.x, area.y + 40.0, area.w, 76.0);
    super::card::fill_box(ui, card, appearance, 12.0, Color::white(0.035));
    super::card::stroke_box(ui, card, appearance, 12.0, Color::white(0.065), 1.0);
    let control_x = appearance.x + 94.0;
    let control_w = (appearance.right() - 12.0 - control_x).max(0.0);
    ui.c.text(
        "主题",
        Rect::new(appearance.x + 12.0, appearance.y + 6.0, 74.0, 28.0),
        &TextStyle::body(),
    );
    ui.c.text(
        "待机样式",
        Rect::new(appearance.x + 12.0, appearance.y + 42.0, 74.0, 28.0),
        &TextStyle::body(),
    );
    let theme_names: Vec<&str> = THEMES.iter().map(|t| t.1).collect();
    let theme_sel = THEMES
        .iter()
        .position(|t| t.0 == st.settings.theme)
        .unwrap_or(0);
    if let Some(index) = quick_picker(
        ui,
        "settings.theme",
        Rect::new(control_x, appearance.y + 6.0, control_w, 28.0),
        &theme_names,
        theme_sel,
        accent,
    ) {
        st.settings.theme = THEMES[index].0;
        st.cmd(Command::SaveSettings);
    }
    let looks = status::looks(st.settings.theme);
    let look_names = idle_names(st.settings.theme);
    let look_sel = looks
        .iter()
        .position(|s| *s == status::shown(st.settings.theme, st.settings.idle_style))
        .unwrap_or(0);
    if let Some(index) = quick_picker(
        ui,
        "settings.idle",
        Rect::new(control_x, appearance.y + 42.0, control_w, 28.0),
        &look_names,
        look_sel,
        accent,
    ) {
        st.settings.idle_style = looks[index];
        st.cmd(Command::SaveSettings);
    }

    let gap = 12.0;
    let width = (area.w - gap) / 2.0;
    for (column, title) in ["音乐与歌词", "灵动岛行为"].iter().enumerate() {
        let group = Rect::new(
            area.x + column as f32 * (width + gap),
            area.y + 124.0,
            width,
            76.0,
        );
        super::card::fill_box(ui, card, group, 12.0, Color::white(0.035));
        super::card::stroke_box(ui, card, group, 12.0, Color::white(0.065), 1.0);
        ui.c.text(
            title,
            Rect::new(group.x + 12.0, group.y + 7.0, group.w - 24.0, 13.0),
            &caption,
        );
        let first = Rect::new(group.x + 12.0, group.y + 24.0, group.w - 24.0, 24.0);
        let second = Rect::new(first.x, group.y + 49.0, first.w, 24.0);
        if column == 0 {
            if switch_row_on(
                ui,
                wid_n("settings.switch", 0),
                first,
                "Synced lyrics",
                Some(st.settings.lyrics),
                accent,
                card,
                false,
            ) {
                flip(st, Switch::Lyrics, Some(st.settings.lyrics));
            }
            draw_lyrics_color(ui, st, second);
        } else {
            ui.c.text(
                "全屏时",
                Rect::new(first.x, first.y, 64.0, first.h),
                &TextStyle::body(),
            );
            let selection = FULLSCREEN
                .iter()
                .position(|choice| choice.0 == st.settings.hide_in_fullscreen)
                .unwrap_or(0);
            let picker = Rect::new(first.x + 68.0, first.y, (first.w - 68.0).max(0.0), first.h);
            if let Some(index) = quick_picker(
                ui,
                "settings.fullscreen",
                picker,
                &FULLSCREEN.map(|choice| choice.1),
                selection,
                accent,
            ) {
                st.settings.hide_in_fullscreen = FULLSCREEN[index].0;
                st.cmd(Command::SaveSettings);
            }
            if switch_row_on(
                ui,
                wid_n("settings.switch", ROWS as u64 + 1),
                second,
                if cfg!(windows) {
                    "Start with Windows"
                } else {
                    "Start at login"
                },
                st.ui.autostart,
                accent,
                card,
                false,
            ) {
                flip(st, Switch::Autostart, st.ui.autostart);
            }
        }
    }
    let tabs = Rect::new(area.x, area.y + 208.0, area.w, 36.0);
    super::card::fill_box(ui, card, tabs, 10.0, Color::white(0.025));
    super::card::stroke_box(ui, card, tabs, 10.0, Color::white(0.05), 1.0);
    draw_tabs(ui, st, tabs.inset(12.0, 0.0));
}

/// 固定列宽和中性轨道，让主题、待机及全屏选择的选中态保持一致。
fn quick_picker(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    names: &[&str],
    selected: usize,
    accent: Color,
) -> Option<usize> {
    ui.c.fill_rrect(rect, 8.0, Color::black(0.14));
    let cell_w = (rect.w - 4.0) / names.len() as f32;
    let mut picked = None;
    for (index, name) in names.iter().enumerate() {
        let cell = Rect::new(
            rect.x + 2.0 + index as f32 * cell_w,
            rect.y + 2.0,
            cell_w,
            rect.h - 4.0,
        );
        let response = ui.button(wid_n(id, index as u64), cell);
        let color = if index == selected {
            ui.c.fill_rrect(cell, 6.0, accent.with_alpha(0.18));
            ui.c.stroke_rrect(cell, 6.0, accent.with_alpha(0.28), 1.0);
            accent
        } else {
            if response.hovered || response.pressed {
                ui.c.fill_rrect(
                    cell,
                    6.0,
                    Color::white(if response.pressed { 0.10 } else { 0.055 }),
                );
            }
            palette::TEXT_DIM
        };
        ui.c.text(
            name,
            cell,
            &TextStyle::footnote()
                .weight(Weight::Semibold)
                .color(color)
                .center(),
        );
        if response.clicked && index != selected {
            picked = Some(index);
        }
    }
    picked
}

fn draw_lyrics_color(ui: &mut Ui, st: &mut AppState, row: Rect) {
    let reset = Rect::new(row.right() - 24.0, row.center().y - 12.0, 24.0, 24.0);
    let width = ui.pill_width("更改", false, true);
    let choose = Rect::new(reset.x - 6.0 - width, row.center().y - 12.0, width, 24.0);
    let swatch = Point::new(choose.x - 14.0, row.center().y);
    ui.c.text(
        "当前歌词颜色",
        Rect::new(row.x, row.y, swatch.x - 16.0 - row.x, row.h),
        &TextStyle::body(),
    );
    let color = crate::text::lyrics_color(st.settings.lyrics_color, ui.c.look());
    ui.c.unthemed(|c| c.fill_circle(swatch, 7.0, color));
    ui.c.stroke_circle(swatch, 7.0, palette::TEXT_DIM, 1.0);
    if ui
        .pill(
            wid("settings.lyrics-color"),
            choose,
            "更改",
            None,
            PillStyle::Utility,
        )
        .clicked
    {
        st.cmd(Command::ChooseLyricsColor);
    }
    if st.settings.lyrics_color.is_some() {
        if ui
            .icon_button(
                wid("settings.lyrics-color-reset"),
                reset,
                Icon::Reset,
                palette::TEXT_DIM,
                14.0,
            )
            .clicked
        {
            st.settings.lyrics_color = None;
            st.cmd(Command::SaveSettings);
        }
    } else {
        ui.c.icon(
            Icon::Reset,
            Rect::from_center(reset.center(), 14.0, 14.0),
            palette::DISABLED,
        );
    }
}

/// "Tabs", then a chip for each of the island's own pages: on, its icon and
/// name; off (no tab in the row), the icon struck through and dim, as the
/// Record page marks an input that's off. A click flips it. Icons only
/// where the names don't fit. Notifications' chip shows only where the copy
/// can read them, and switches the whole of it off, not just its tab.
fn draw_tabs(ui: &mut Ui, st: &mut AppState, r: Rect) {
    let body = TextStyle::body();
    let label_w = ui.c.measure("Tabs", &body, 200.0).0.ceil();
    ui.c.text("Tabs", Rect::new(r.x, r.y, label_w + 1.0, r.h), &body);
    let chip_st = TextStyle::footnote().weight(Weight::Semibold);
    let pages = super::hideable_tabs(st);
    let gaps = CHIP_GAP * (pages.len() - 1) as f32;
    let full: Vec<f32> = pages
        .iter()
        .map(|p| {
            (CHIP_PAD
                + CHIP_ICON
                + CHIP_ICON_GAP
                + ui.c.measure(p.title(), &chip_st, 200.0).0
                + CHIP_PAD)
                .ceil()
        })
        .collect();
    let names = full.iter().sum::<f32>() + gaps <= r.w - label_w - 12.0;
    let widths: Vec<f32> = if names {
        full
    } else {
        vec![CHIP_H + 8.0; pages.len()]
    };
    let mut x = r.right() - widths.iter().sum::<f32>() - gaps;
    let y = r.center().y - CHIP_H / 2.0;
    let mut flip = None;
    for (i, (&p, w)) in pages.iter().zip(widths).enumerate() {
        let c = Rect::new(x, y, w, CHIP_H);
        x += w + CHIP_GAP;
        let on = !st.tab_hidden(p);
        let resp = ui.button(wid_n("settings.tab", i as u64), c);
        if resp.clicked {
            flip = Some(p);
        }
        let fill = match (on, resp.pressed, resp.hovered) {
            (_, true, _) => st.accent.with_alpha(0.24),
            (true, _, true) => st.accent.with_alpha(0.20),
            (true, _, false) => Color::white(0.055),
            (false, _, true) => Color::white(0.09),
            (false, _, false) => Color::white(0.035),
        };
        ui.c.fill_rrect(c, CHIP_H / 2.0, fill);
        let border = Color::white(if on { 0.10 } else { 0.05 });
        ui.c.stroke_rrect(c, CHIP_H / 2.0, border, 1.0);
        let color = if on { palette::TEXT } else { palette::TEXT_DIM };
        let ix = if names {
            c.x + CHIP_PAD
        } else {
            c.center().x - CHIP_ICON / 2.0
        };
        let ir = Rect::new(ix, c.center().y - CHIP_ICON / 2.0, CHIP_ICON, CHIP_ICON);
        ui.c.icon(p.icon(), ir, color);
        if !on {
            ui.c.line(
                Point::new(ir.x, ir.bottom()),
                Point::new(ir.right(), ir.y),
                color,
                1.5,
            );
        }
        if names {
            let tx = ix + CHIP_ICON + CHIP_ICON_GAP;
            ui.c.text(
                p.title(),
                Rect::new(tx, c.y, (c.right() - tx).max(0.0), c.h),
                &chip_st.color(color),
            );
        }
    }
    if let Some(p) = flip {
        crate::settings_model::apply(
            st,
            crate::settings_model::Key::Tab(p),
            crate::settings_model::Act::Toggle,
        );
    }
}

/// Where Check now goes in the version's row, at its right end: while this
/// copy looks for new versions itself and none is offered (then the last
/// row has Update). Words with a wash on hover, not a pill.
pub(crate) fn check_slot(ui: &mut Ui, st: &AppState, row: Rect) -> Option<Rect> {
    if !st.update.looks || st.update.offer.is_some() {
        return None;
    }
    let (text, _) = check_look(st.update.said, st.accent);
    // Checking… and Up to date are words only: no room for a wash, so they
    // take less of the label's, ending where Check now's words end.
    let pad = if answered(st.update.said) {
        0.0
    } else {
        CHECK_PAD
    };
    let w = (ui.c.measure(text, &check_text(), 300.0).0 + 2.0 * pad).ceil();
    let right = row.right() - (CHECK_PAD - pad);
    Some(Rect::new(
        right - w,
        row.center().y - CHECK_H / 2.0,
        w,
        CHECK_H,
    ))
}

/// Check now said something that isn't a button: checking, or up to date.
fn answered(said: Option<Said>) -> bool {
    matches!(said, Some(Said::Checking | Said::UpToDate))
}

/// Check now, or what it found: checking, up to date, or (it couldn't
/// look) Try again, which looks again.
pub(crate) fn draw_check(ui: &mut Ui, st: &mut AppState, b: Rect) {
    let said = st.update.said;
    let (text, color) = check_look(said, st.accent);
    if answered(said) {
        ui.c.text(text, b, &check_text().color(color).right());
        return;
    }
    let resp = ui.button(wid("settings.check"), b);
    if resp.hovered || resp.pressed {
        ui.c.fill_rrect(
            b,
            b.h / 2.0,
            Color::white(if resp.pressed { 0.14 } else { 0.08 }),
        );
    }
    ui.c.text(text, b, &check_text().color(color).center());
    if resp.clicked {
        st.update.said = Some(Said::Checking);
        st.cmd(Command::CheckForUpdates);
    }
}

/// Check now's words and colour for what it has said so far.
fn check_look(said: Option<Said>, accent: Color) -> (&'static str, Color) {
    match said {
        None => ("Check now", accent),
        Some(Said::Checking) => ("Checking…", palette::TEXT_DIM),
        Some(Said::UpToDate) => ("Up to date", palette::GREEN),
        Some(Said::Failed) => ("Try again", palette::ORANGE),
    }
}

fn check_text() -> TextStyle {
    TextStyle::footnote().weight(Weight::Semibold)
}

fn flip(st: &mut AppState, sw: Switch, on: Option<bool>) {
    let s = &mut st.settings;
    match sw {
        Switch::Lyrics => s.lyrics = !s.lyrics,
        Switch::Autostart => {
            if let Some(on) = on {
                st.cmd(Command::SetAutostart(!on));
            }
            return;
        }
    }
    st.cmd(Command::SaveSettings);
}

// ---------------------------------------------------------------- widgets

/// Choices side by side in a track `h` tall, the chosen one lifted (the
/// Focus page's picker, a size smaller). Returns the choice clicked, if any.
pub(crate) fn picker(
    ui: &mut Ui,
    id: &str,
    x: f32,
    y: f32,
    h: f32,
    names: &[&str],
    sel: usize,
) -> Option<usize> {
    picker_padded(ui, id, x, y, h, names, sel, SEG_PAD)
}

/// [`picker`] with `pad` around each choice's name.
#[allow(clippy::too_many_arguments)]
fn picker_padded(
    ui: &mut Ui,
    id: &str,
    x: f32,
    y: f32,
    h: f32,
    names: &[&str],
    sel: usize,
    pad: f32,
) -> Option<usize> {
    let st = picker_text();
    let inner_h = h - 2.0 * SEG_INSET;
    let mut cx = x + SEG_INSET;
    let cells: Vec<Rect> = names
        .iter()
        .map(|n| {
            let w = choice_width(ui, n, pad);
            let cell = Rect::new(cx, y + SEG_INSET, w, inner_h);
            cx += w;
            cell
        })
        .collect();
    let track = Rect::new(x, y, cx + SEG_INSET - x, h);
    ui.c.fill_rrect(track, h / 2.0, Color::white(0.045));
    ui.c.stroke_rrect(track, h / 2.0, Color::white(0.075), 1.0);
    let mut picked = None;
    for (i, (name, cell)) in names.iter().zip(&cells).enumerate() {
        let color = if i == sel {
            ui.c.fill_rrect(*cell, inner_h / 2.0, palette::BLUE.with_alpha(0.16));
            ui.c.stroke_rrect(*cell, inner_h / 2.0, palette::BLUE.with_alpha(0.20), 1.0);
            palette::BLUE
        } else {
            let resp = ui.button(wid_n(id, i as u64), Rect::new(cell.x, y, cell.w, h));
            if resp.clicked {
                picked = Some(i);
            }
            if resp.pressed || resp.hovered {
                ui.c.fill_rrect(
                    *cell,
                    inner_h / 2.0,
                    Color::white(if resp.pressed { 0.10 } else { 0.06 }),
                );
                palette::TEXT
            } else {
                palette::TEXT_DIM
            }
        };
        ui.c.text(name, *cell, &st.color(color).center());
    }
    picked
}

fn picker_text() -> TextStyle {
    TextStyle::footnote().weight(Weight::Semibold)
}

/// One choice in a picker: its name and the padding around it.
fn choice_width(ui: &mut Ui, name: &str, pad: f32) -> f32 {
    (2.0 * pad + ui.c.measure(name, &picker_text(), 300.0).0).ceil()
}

/// A label with a switch at the row's right end; the whole row toggles.
/// `None` = not known yet: drawn off and dimmed, and it doesn't click.
/// `raw_label` preserves an app name or other text supplied from outside the UI.
/// Returns true when clicked.
pub(crate) fn switch_row(
    ui: &mut Ui,
    id: u64,
    r: Rect,
    label: &str,
    on: Option<bool>,
    accent: Color,
    raw_label: bool,
) -> bool {
    switch_row_on(ui, id, r, label, on, accent, None, raw_label)
}

/// [`switch_row`] on the open card (`card` from `card::pill_card`): in
/// Pill, a bottom row's hover wash follows the card's rounder corners.
fn switch_row_on(
    ui: &mut Ui,
    id: u64,
    r: Rect,
    label: &str,
    on: Option<bool>,
    accent: Color,
    card: Option<(Rect, f32)>,
    raw_label: bool,
) -> bool {
    let hit = Rect::new(r.x - ROW_BLEED, r.y, r.w + 2.0 * ROW_BLEED, r.h);
    let resp = if on.is_some() {
        ui.button(id, hit)
    } else {
        Default::default()
    };
    if resp.hovered || resp.pressed {
        super::card::fill_box(
            ui,
            card,
            hit.inset(0.0, 1.0),
            8.0,
            Color::white(if resp.pressed { 0.10 } else { 0.06 }),
        );
    }
    let sw = Rect::new(
        r.right() - SWITCH_W,
        r.center().y - SWITCH_H / 2.0,
        SWITCH_W,
        SWITCH_H,
    );
    let label_rect = Rect::new(r.x, r.y, (sw.x - 10.0 - r.x).max(0.0), r.h);
    if raw_label {
        ui.c.text_raw(label, label_rect, &TextStyle::body());
    } else {
        ui.c.text(label, label_rect, &TextStyle::body());
    }
    let alpha = if on.is_some() { 1.0 } else { 0.5 };
    ui.c.with_alpha(alpha, |c| draw_switch(c, sw, on == Some(true), accent));
    resp.clicked
}

/// A switch's track and knob (also the Add-ons page's).
pub(crate) fn draw_switch(c: &mut crate::gfx::Canvas, r: Rect, on: bool, accent: Color) {
    c.fill_rrect(r, r.h / 2.0, if on { accent } else { palette::TRACK });
    c.stroke_rrect(
        r,
        r.h / 2.0,
        if on {
            accent.with_alpha(0.3)
        } else {
            Color::white(0.16)
        },
        1.0,
    );
    let d = r.h - 2.0 * KNOB_INSET;
    let x = if on {
        r.right() - KNOB_INSET - d / 2.0
    } else {
        r.x + KNOB_INSET + d / 2.0
    };
    c.fill_circle(
        Point::new(x, r.center().y + 0.7),
        d / 2.0,
        Color::black(0.18),
    );
    c.fill_circle(Point::new(x, r.center().y), d / 2.0, palette::ON_ACCENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a theme comes in the picker. No `_` arm: a theme added to the
    /// enum stops this compiling until it has a place, and then the test
    /// below asks the picker for it.
    fn place(t: Theme) -> usize {
        match t {
            Theme::Classic => 0,
            Theme::Oyen => 1,
            Theme::Glass => 2,
            Theme::Pill => 3,
            Theme::Diorama => 4,
        }
    }

    #[test]
    fn the_theme_picker_has_every_theme_by_its_name() {
        assert_eq!(
            THEMES.map(|t| t.1),
            ["Classic", "Oyen", "Glass", "Pill", "Diorama"]
        );
        for (i, (theme, name)) in THEMES.iter().enumerate() {
            assert_eq!(place(*theme), i, "{name}");
        }
    }

    #[test]
    fn the_idle_look_picker_names_each_themes_own_looks() {
        for theme in [Theme::Classic, Theme::Glass] {
            assert_eq!(
                idle_names(theme),
                ["Clock", "Eyes", "Minimal", "Hidden"],
                "{theme:?}"
            );
        }
        // Diorama rests as its picture, with the time or alone: no eyes.
        assert_eq!(idle_names(Theme::Diorama), ["Clock", "Minimal", "Hidden"]);
        // In his theme the eyes are Oyen's own, and the status bar is Pill's.
        assert_eq!(
            idle_names(Theme::Oyen),
            ["Clock", "Oyen", "Minimal", "Hidden"]
        );
        assert_eq!(
            idle_names(Theme::Pill),
            ["Clock", "Eyes", "Minimal", "Hidden", "Status"]
        );
        // A name for every look a theme offers, one choice each.
        for (theme, _) in THEMES {
            assert_eq!(
                idle_names(theme).len(),
                status::looks(theme).len(),
                "{theme:?}"
            );
        }
    }
}
