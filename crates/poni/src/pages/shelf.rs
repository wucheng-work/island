//! Shelf page: files dropped onto the island, kept as references.
//!
//! Click a tile to open it, right-click to show it in Explorer, press and
//! drag to take it back out. Thumbnails arrive through `st.shelf_thumbs`
//! (requested with `Command::LoadThumb`); the thumbnail worker also reports
//! whether each file still exists, so moved or deleted items are greyed out
//! without touching the disk on the UI thread.

use crate::commands::Command;
use crate::dragdrop;
use crate::gfx::{Image, TextStyle, Weight};
use crate::icons::Icon;
use crate::services::imaging::{self, PathKind};
use crate::state::AppState;
use crate::ui::{wid, wid_n, PillStyle, Ui, WidgetId, PILL_GAP, PILL_SMALL};
use poni_core::design::radius;
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::shelf::{display_name, extension, ShelfItem, MAX_SHELF_ITEMS};
use std::cell::RefCell;
use std::collections::HashMap;

const PAGE_H: f32 = 228.0;
/// Breathing room below the page tabs (the header already leaves the gap).
const TOP: f32 = 0.0;
/// Summary row: title, capacity and the small utility pills.
const HEAD_H: f32 = PILL_SMALL;
const HEAD_GAP: f32 = 28.0;
const FOOT_H: f32 = 24.0;
const TILE_W: f32 = 124.0;
const TILE_GAP: f32 = 10.0;
const TILE_R: f32 = radius::PANEL;
const THUMB: f32 = imaging::THUMB_DIP;
const THUMB_R: f32 = 12.0;
const FADE_W: f32 = 30.0;
/// Press-and-move distance (DIPs) that turns a press into a drag-out.
const DRAG_START: f32 = 6.0;
const UNDO_SECS: f64 = 8.0;
/// Minimum time between existence re-checks of one file: when the page
/// (re)appears, and when a tile gets hovered.
const RECHECK_SHOWN_SECS: f64 = 15.0;
const RECHECK_HOVER_SECS: f64 = 4.0;
const APPEAR_SECS: f64 = 0.28;
const GLOW_SECS: f32 = 0.14;
/// Drop-zone outline: one solid hairline, the same idle and while dragging.
const ZONE_STROKE: f32 = 1.5;
const DROP_LABEL: &str = "松开即可加入暂存区";

pub fn height(_st: &AppState) -> f32 {
    PAGE_H
}

/// Transient page state (UI thread only).
#[derive(Default)]
struct Local {
    last_draw: f64,
    anim_t: f64,
    /// Displayed scroll offset, easing toward `st.ui.shelf_scroll`.
    scroll_vis: f32,
    /// 0..1 drop-highlight while files hover over the island.
    glow: f32,
    /// key → time the item first appeared (new tiles fade in).
    seen: HashMap<String, f64>,
    seeded: bool,
    /// key → last thumbnail we drew (bridges re-checks without flicker).
    last_img: HashMap<String, Image>,
    /// key → time of the last existence re-check.
    checked: HashMap<String, f64>,
    hovered: Option<WidgetId>,
    drag_started: bool,
    /// Path being dragged out; re-checked once the drag ends (it may have moved).
    after_drag: Option<String>,
    /// Shelf contents before the last removal, for "Undo".
    undo: Option<(Vec<ShelfItem>, f64)>,
    count: usize,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

enum Action {
    Open(String),
    Reveal(String),
    DragOut(String),
    Recheck(String),
    Remove(String),
    Clear,
    Undo,
}

fn key(path: &str) -> String {
    path.to_lowercase()
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    LOCAL.with(|l| {
        if let Ok(mut l) = l.try_borrow_mut() {
            draw_page(ui, st, area, &mut l);
        }
    });
}

fn draw_page(ui: &mut Ui, st: &mut AppState, area: Rect, l: &mut Local) {
    let now = st.now;
    let dt = (now - l.anim_t).clamp(0.0, 0.1) as f32;
    l.anim_t = now;
    let shown = now - l.last_draw > 0.75;
    l.last_draw = now;
    if !ui.input.down {
        l.drag_started = false;
    }

    let items = st.store.shelf.items.clone();
    sync(st, l, &items, now, shown);

    // Drop highlight fades in/out while files hover over the island.
    let target = if st.drag_hover { 1.0 } else { 0.0 };
    let step = dt / GLOW_SECS;
    if shown || (target - l.glow).abs() <= step {
        l.glow = target;
    } else {
        l.glow += step.copysign(target - l.glow);
        ui.request_anim();
    }

    let mut actions = Vec::new();
    // Pill: the boxes that reach the card's bottom corners follow them.
    let card = super::card::pill_card(st, area);
    if items.is_empty() {
        draw_empty(ui, st, l, area, card, &mut actions);
    } else {
        draw_items(ui, st, l, area, &items, dt, shown, card, &mut actions);
    }
    apply(st, l, &items, actions);
}

/// Keep thumbnails and local caches in step with the shelf contents.
fn sync(st: &mut AppState, l: &mut Local, items: &[ShelfItem], now: f64, shown: bool) {
    // Every item needs a thumbnail request (this also heals path-casing
    // differences between what was dropped and what the shelf kept).
    for it in items {
        if !st.shelf_thumbs.contains_key(&it.path) {
            st.cmd(Command::LoadThumb(it.path.clone()));
        }
    }
    // Forget state of items that left the shelf (keeping an undo snapshot warm).
    let undo_live = l.undo.as_ref().is_some_and(|(_, t)| now - t < UNDO_SECS);
    if !undo_live {
        l.undo = None;
    }
    let undo_items: &[ShelfItem] = l.undo.as_ref().map(|(v, _)| v.as_slice()).unwrap_or(&[]);
    if st.shelf_thumbs.len() > items.len() + undo_items.len() {
        st.shelf_thumbs.retain(|p, _| items.iter().chain(undo_items).any(|i| &i.path == p));
    }
    let keep: Vec<String> = items.iter().chain(undo_items).map(|i| key(&i.path)).collect();
    if l.seen.len() > keep.len() {
        l.seen.retain(|k, _| keep.contains(k));
    }
    if l.last_img.len() > keep.len() {
        l.last_img.retain(|k, _| keep.contains(k));
    }
    if l.checked.len() > keep.len() {
        l.checked.retain(|k, _| keep.contains(k));
    }

    // New items fade in at the front of the row; the first draw seeds silently.
    if !l.seeded {
        for it in items {
            l.seen.insert(key(&it.path), f64::MIN);
        }
        l.seeded = true;
    } else {
        for it in items {
            l.seen.entry(key(&it.path)).or_insert(now);
        }
    }
    if items.len() > l.count {
        st.ui.shelf_scroll = 0.0;
        l.undo = None;
    }
    l.count = items.len();

    // Files may have been moved or deleted while the page was out of view.
    if shown {
        for it in items {
            recheck(st, l, &it.path, now, RECHECK_SHOWN_SECS);
        }
    }
    if let Some(p) = l.after_drag.clone() {
        if !dragdrop::is_dragging_out() {
            l.after_drag = None;
            recheck(st, l, &p, now, 0.0);
        }
    }
}

/// Ask the worker to look at `path` again (existence + thumbnail) unless it
/// did so within `min_age` seconds. Unchanged files come back from its
/// cache, so this is cheap.
fn recheck(st: &mut AppState, l: &mut Local, path: &str, now: f64, min_age: f64) {
    // Not seen by the worker yet: its first answer is still on the way.
    if imaging::path_kind(path).is_none() {
        return;
    }
    let k = key(path);
    if l.checked.get(&k).is_some_and(|t| now - t < min_age) {
        return;
    }
    if let Some(Some(img)) = st.shelf_thumbs.remove(path) {
        l.last_img.insert(k.clone(), img);
    }
    l.checked.insert(k, now);
    st.cmd(Command::LoadThumb(path.to_string()));
}

fn apply(st: &mut AppState, l: &mut Local, items: &[ShelfItem], actions: Vec<Action>) {
    let now = st.now;
    for a in actions {
        match a {
            Action::Open(p) => st.cmd(Command::OpenPath(p)),
            Action::Reveal(p) => st.cmd(Command::RevealPath(p)),
            Action::DragOut(p) => {
                l.after_drag = Some(p.clone());
                st.cmd(Command::DragOut(vec![p]));
            }
            Action::Recheck(p) => recheck(st, l, &p, now, 0.0),
            Action::Remove(p) => {
                l.undo = Some((items.to_vec(), now));
                if st.store.shelf.remove(&p) {
                    st.save();
                }
                l.count = st.store.shelf.items.len();
            }
            Action::Clear => {
                l.undo = Some((items.to_vec(), now));
                st.store.shelf.clear();
                st.ui.shelf_scroll = 0.0;
                l.count = 0;
                st.save();
            }
            Action::Undo => {
                if let Some((snapshot, _)) = l.undo.take() {
                    st.store.shelf.items = snapshot;
                    l.count = st.store.shelf.items.len();
                    st.save();
                }
            }
        }
    }
}

// ---------------------------------------------------------------- items

#[allow(clippy::too_many_arguments)]
fn draw_items(
    ui: &mut Ui,
    st: &mut AppState,
    l: &mut Local,
    area: Rect,
    items: &[ShelfItem],
    dt: f32,
    shown: bool,
    card: Option<(Rect, f32)>,
    actions: &mut Vec<Action>,
) {
    let now = st.now;
    let head = Rect::new(area.x, area.y + TOP, area.w, HEAD_H);
    let row_y = head.bottom() + HEAD_GAP;
    let row = Rect::new(area.x, row_y, area.w, (area.bottom() - FOOT_H - 8.0 - row_y).max(0.0));
    let n = items.len();

    // Horizontal scrolling (the wheel only gets consumed when there is
    // something to scroll, so it keeps switching pages otherwise).
    let content_w = n as f32 * TILE_W + n.saturating_sub(1) as f32 * TILE_GAP;
    let max_scroll = (content_w - row.w).max(0.0);
    if max_scroll > 0.0 {
        let w = ui.wheel_in(row);
        if w != 0.0 {
            st.ui.shelf_scroll -= w * (TILE_W + TILE_GAP);
        }
    }
    st.ui.shelf_scroll = st.ui.shelf_scroll.clamp(0.0, max_scroll);
    let target = st.ui.shelf_scroll;
    if shown || (l.scroll_vis - target).abs() < 0.3 {
        l.scroll_vis = target;
    } else {
        l.scroll_vis += (target - l.scroll_vis) * (1.0 - (-dt * 16.0).exp());
        ui.request_anim();
    }
    let scroll = l.scroll_vis;
    let glow = l.glow;

    let mut hovered: Option<(WidgetId, usize)> = None;
    let tile_h = (row.h - 4.0).max(0.0);
    ui.c.clip(Rect::new(row.x, row.y - 2.0, row.w, row.h + 4.0));
    for (i, it) in items.iter().enumerate() {
        let x = row.x + i as f32 * (TILE_W + TILE_GAP) - scroll;
        if x >= row.right() || x + TILE_W <= row.x {
            continue;
        }
        let k = key(&it.path);
        let since = now - l.seen.get(&k).copied().unwrap_or(f64::MIN);
        let appear = ease_out((since / APPEAR_SECS).clamp(0.0, 1.0) as f32);
        if appear < 1.0 {
            ui.request_anim();
        }
        let tile = Rect::new(x, row.y + (1.0 - appear) * 8.0, TILE_W, tile_h);
        let kind = imaging::path_kind(&it.path);
        let missing = kind == Some(PathKind::Missing);
        let tint = file_color(&it.path, kind);
        let id = wid_n("shelf.tile", wid(&k));
        let resp = ui.button(id, intersect(tile, row));
        let hot = resp.hovered && glow < 0.5;
        if hot {
            hovered = Some((id, i));
        }

        let img = match st.shelf_thumbs.get(&it.path) {
            Some(Some(img)) => {
                if l.last_img.get(&k).map(|c| c.id) != Some(img.id) {
                    l.last_img.insert(k.clone(), img.clone());
                }
                Some(img.clone())
            }
            _ => l.last_img.get(&k).cloned(),
        };

        let base = ui.c.alpha();
        // Tiles step back while files hover, so the drop zone reads clearly.
        ui.c.set_alpha(base * appear * (1.0 - glow));
        let fill = if resp.pressed {
            palette::SURFACE_PRESSED
        } else if hot {
            palette::SURFACE_HOVER
        } else {
            palette::SURFACE
        };
        ui.c.fill_rrect(tile, TILE_R, fill);
        ui.c.stroke_rrect(tile, TILE_R, if hot { st.accent.with_alpha(0.32) } else { palette::BORDER }, 1.0);
        let thumb = Rect::new(tile.x + (TILE_W - THUMB) / 2.0, tile.y + 12.0, THUMB, THUMB);
        let tile_a = ui.c.alpha();
        // Missing files: everything but the caption is greyed out.
        ui.c.set_alpha(tile_a * if missing { 0.38 } else { 1.0 });
        ui.c.fill_rrect(thumb, THUMB_R, tint.with_alpha(0.10));
        match &img {
            Some(im) => ui.c.image(im, thumb, THUMB_R),
            None => {
                let folder = kind == Some(PathKind::Folder) || (kind.is_none() && extension(&it.path).is_empty());
                let icon = if folder { Icon::Folder } else { Icon::File };
                ui.c.icon(icon, Rect::from_center(thumb.center(), 24.0, 24.0), tint);
            }
        }
        let name_r = Rect::new(tile.x + 10.0, thumb.bottom() + 8.0, TILE_W - 20.0, 30.0);
        // Keep two lines of the original name; the macOS text renderer needs an explicit clip.
        ui.c.clip(name_r);
        ui.c.text_raw(&display_name(&it.path), name_r, &TextStyle::footnote().weight(Weight::Medium).center().wrap().top().line_spacing(1.25));
        ui.c.unclip();
        ui.c.set_alpha(tile_a);
        let cap_r = Rect::new(name_r.x, name_r.bottom() + 5.0, name_r.w, 22.0);
        if hot {
            let action = ui.button(wid_n("shelf.reveal", wid(&k)), intersect(cap_r, row));
            let fill = if action.pressed { palette::SURFACE_PRESSED } else if action.hovered { palette::SURFACE_HOVER } else { palette::SURFACE };
            ui.c.fill_rrect(cap_r, 8.0, fill);
            let label = if missing { "重新检查" } else { "显示位置" };
            ui.c.text(label, cap_r, &TextStyle::caption().weight(Weight::Medium).color(st.accent).center());
            if action.clicked || action.rclicked {
                actions.push(if missing { Action::Recheck(it.path.clone()) } else { Action::Reveal(it.path.clone()) });
            }
        } else if missing {
            ui.c.text("文件已移动或删除", cap_r, &TextStyle::caption().weight(Weight::Medium).color(palette::TEXT_DIM).center());
        } else {
            ui.c.text_raw(&caption(&it.path, kind), cap_r, &TextStyle::caption().color(palette::TEXT_DIM).center());
        }

        // Remove button on hover, tucked onto the thumbnail's corner.
        if hot {
            let c = Point::new(thumb.right() - 2.0, thumb.y + 2.0);
            let cr = ui.button(wid_n("shelf.remove", wid(&k)), intersect(Rect::from_center(c, 22.0, 22.0), row));
            let fill = if cr.pressed {
                Color::white(0.42)
            } else if cr.hovered {
                Color::white(0.34)
            } else {
                Color::white(0.24)
            };
            ui.c.fill_circle(c, 11.0, palette::ISLAND);
            ui.c.fill_circle(c, 9.0, fill);
            ui.c.icon(Icon::Close, Rect::from_center(c, 10.0, 10.0), palette::TEXT);
            if cr.clicked {
                actions.push(Action::Remove(it.path.clone()));
            }
        }
        ui.c.set_alpha(base);

        if resp.clicked {
            actions.push(if missing { Action::Recheck(it.path.clone()) } else { Action::Open(it.path.clone()) });
        }
        if resp.rclicked {
            actions.push(if missing { Action::Recheck(it.path.clone()) } else { Action::Reveal(it.path.clone()) });
        }
        if resp.pressed && !missing && !l.drag_started {
            if let Some((dx, dy)) = resp.drag {
                if dx * dx + dy * dy > DRAG_START * DRAG_START {
                    l.drag_started = true;
                    actions.push(Action::DragOut(it.path.clone()));
                }
            }
        }
    }

    // Soft edges where tiles scroll out of view.
    let fade_h = Rect::new(row.x, row.y - 2.0, FADE_W, row.h + 4.0);
    if scroll > 0.5 {
        fade_edge(ui, fade_h, true);
    }
    if scroll < max_scroll - 0.5 {
        fade_edge(ui, Rect::new(row.right() - FADE_W, fade_h.y, FADE_W, fade_h.h), false);
    }
    ui.c.unclip();

    // Hovering a tile re-checks that file now and then (it may have moved).
    let hovered_id = hovered.map(|(id, _)| id);
    if hovered_id != l.hovered {
        l.hovered = hovered_id;
        if let Some((_, i)) = hovered {
            if let Some(it) = items.get(i) {
                recheck(st, l, &it.path, now, RECHECK_HOVER_SECS);
            }
        }
    }

    if glow > 0.01 {
        draw_drop_overlay(ui, st, Rect::new(row.x, row.y, row.w, tile_h), glow, card);
    }
    draw_header(ui, st, l, head, items, actions);
    draw_footer(ui, area, items, hovered.map(|(_, i)| i));
}

fn draw_header(ui: &mut Ui, st: &AppState, l: &Local, head: Rect, items: &[ShelfItem], actions: &mut Vec<Action>) {
    let n = items.len();
    ui.c.icon(Icon::Tray, Rect::new(head.x + 5.0, head.y + 5.0, 18.0, 18.0), palette::TEXT_DIM);
    let title = "文件暂存区";
    let title_style = TextStyle::headline();
    let title_w = ui.c.measure(title, &title_style, head.w).0;
    ui.c.text(title, Rect::new(head.x + 36.0, head.y, title_w, head.h), &title_style);
    let capacity = Rect::new(head.x + 36.0 + title_w + 10.0, head.y + 4.0, 48.0, 20.0);
    ui.c.text(&format!("{n} / {MAX_SHELF_ITEMS}"), capacity, &TextStyle::caption().tabular().color(palette::TEXT_FAINT).center());

    // Small utility pills on the right: Clear, plus Undo for a while after a removal.
    let mut right = head.right();
    if n > 0 {
        let clear_w = ui.pill_width("清空", true, true);
        let clear_r = Rect::new(right - clear_w, head.y, clear_w, PILL_SMALL);
        if ui.pill(wid("shelf.clear"), clear_r, "清空", Some(Icon::Trash), PillStyle::Utility).clicked {
            actions.push(Action::Clear);
        }
        right = clear_r.x - PILL_GAP;
    }
    if l.undo.as_ref().is_some_and(|(_, t)| st.now - t < UNDO_SECS) {
        let undo_w = ui.pill_width("撤销", true, true);
        let undo_r = Rect::new(right - undo_w, head.y, undo_w, PILL_SMALL);
        if ui.pill(wid("shelf.undo"), undo_r, "撤销", Some(Icon::Undo), PillStyle::UtilityAccent(st.accent)).clicked {
            actions.push(Action::Undo);
        }
    }
    ui.c.text(
        "拖入暂存 · 单击打开 · 拖出取用",
        Rect::new(head.x, head.bottom() + 3.0, head.w, 18.0),
        &TextStyle::footnote().color(palette::TEXT_DIM),
    );
}

/// 文件类型色只用于缩略图和占位图标，主表面与文件名保持中性。
fn file_color(path: &str, kind: Option<PathKind>) -> Color {
    if kind == Some(PathKind::Folder) {
        return palette::ORANGE;
    }
    match extension(path).as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "heic" | "heif" | "webp" | "avif" | "svg" => palette::BLUE,
        "mp3" | "m4a" | "flac" | "wav" | "mp4" | "mov" | "mkv" => palette::PURPLE,
        "xls" | "xlsx" | "csv" | "numbers" => palette::GREEN,
        "zip" | "7z" | "rar" | "gz" | "tar" => palette::ORANGE,
        _ => palette::BLUE,
    }
}

fn draw_footer(ui: &mut Ui, area: Rect, items: &[ShelfItem], hovered: Option<usize>) {
    let hint = match hovered.and_then(|i| items.get(i)) {
        Some(it) if imaging::path_kind(&it.path) == Some(PathKind::Missing) => {
            (format!("已移动或删除 · 原位置：{}", parent_dir(&it.path)), palette::TEXT_DIM)
        }
        Some(it) => (parent_dir(&it.path), palette::TEXT_DIM),
        None if items.len() >= MAX_SHELF_ITEMS => ("已满 · 新文件会替换最早的暂存项".to_string(), palette::TEXT_FAINT),
        None => ("文件保留在原位置，移除暂存项不会删除原文件".to_string(), palette::TEXT_FAINT),
    };
    let hint_r = Rect::new(area.x, area.bottom() - FOOT_H, area.w, FOOT_H);
    ui.c.text_raw(&hint.0, hint_r, &TextStyle::caption().color(hint.1));
}

/// Extension or file kind — the small line under a tile's name.
fn caption(path: &str, kind: Option<PathKind>) -> String {
    if kind == Some(PathKind::Folder) {
        return "文件夹".into();
    }
    let ext = extension(path);
    if !ext.is_empty() {
        ext.to_uppercase()
    } else if kind == Some(PathKind::File) {
        "文件".into()
    } else {
        String::new()
    }
}

/// Folder that contains `path` ("C:\a\b.txt" → "C:\a", "C:\b.txt" → "C:\").
fn parent_dir(path: &str) -> String {
    let t = path.trim_end_matches(['\\', '/']);
    match t.rfind(['\\', '/']) {
        Some(i) if i > 0 => {
            let p = &t[..i];
            if p.ends_with(':') {
                format!("{p}\\")
            } else {
                p.to_string()
            }
        }
        _ => t.to_string(),
    }
}

fn draw_drop_overlay(ui: &mut Ui, st: &AppState, zone: Rect, glow: f32, card: Option<(Rect, f32)>) {
    let base = ui.c.alpha();
    ui.c.set_alpha(base * glow);
    drop_zone(ui, card, zone, 16.0, st.accent.with_alpha(0.10), st.accent.with_alpha(0.9));
    // One centred group: Plus + label.
    let ls = TextStyle::body().weight(Weight::Semibold);
    let tw = ui.c.measure(DROP_LABEL, &ls, zone.w).0;
    let (isz, gap) = (16.0, 6.0);
    let c = zone.center();
    let x = c.x - (isz + gap + tw) / 2.0;
    ui.c.icon(Icon::Plus, Rect::new(x, c.y - isz / 2.0, isz, isz), st.accent);
    ui.c.text(DROP_LABEL, Rect::new(x + isz + gap, c.y - 10.0, tw + 2.0, 20.0), &ls);
    ui.c.set_alpha(base);
}

// ---------------------------------------------------------------- empty state

fn draw_empty(ui: &mut Ui, st: &AppState, l: &Local, area: Rect, card: Option<(Rect, f32)>, actions: &mut Vec<Action>) {
    let g = l.glow;
    let accent = st.accent;
    let head = Rect::new(area.x, area.y + TOP, area.w, HEAD_H);
    draw_header(ui, st, l, head, &[], actions);
    // Same bottom edge as the drop overlay over a filled shelf.
    let zone_y = head.bottom() + HEAD_GAP;
    let zone = Rect::new(area.x, zone_y, area.w, (area.bottom() - FOOT_H - 12.0 - zone_y).max(0.0));
    let fill = Color::lerp(palette::SURFACE.mul_alpha(0.5), accent.with_alpha(0.13), g);
    let stroke = Color::lerp(palette::BORDER, accent.with_alpha(0.9), g);
    drop_zone(ui, card, zone, 18.0, fill, stroke);

    let undo_live = g < 0.5 && l.undo.as_ref().is_some_and(|(_, t)| st.now - t < UNDO_SECS);
    let (icon, title, hint) = if g >= 0.5 {
        (Icon::Plus, DROP_LABEL, "文件和文件夹都可以，最多暂存 24 项")
    } else if undo_live {
        (Icon::Tray, "暂存区已清空", "可在上方撤销，或继续拖入文件")
    } else {
        (Icon::Tray, "随手一放，稍后取用", "拖入文件或文件夹，跨应用取用更方便")
    };
    // Icon, title and hint share the same centred drop target.
    let c = zone.center();
    let icon_col = Color::lerp(palette::TEXT_DIM, accent, g);
    ui.c.icon(icon, Rect::from_center(Point::new(c.x, c.y - 22.0), 26.0, 26.0), icon_col);
    ui.c.text(title, Rect::new(zone.x, c.y - 4.0, zone.w, 20.0), &TextStyle::headline().center());
    ui.c.text(hint, Rect::new(zone.x + 16.0, c.y + 16.0, zone.w - 32.0, 18.0), &TextStyle::footnote().color(palette::TEXT_DIM).center());
    draw_footer(ui, area, &[], None);
}

// ---------------------------------------------------------------- drawing helpers

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// Fade content into the black island at a row edge. The canvas has no
/// gradients, so nested rectangles build a smooth ramp (nested rather than
/// side by side so no anti-aliased seams show between bands).
fn fade_edge(ui: &mut Ui, r: Rect, from_left: bool) {
    const BANDS: usize = 10;
    let alpha = |j: usize| -> f32 {
        if j >= BANDS {
            return 0.0;
        }
        let t = 1.0 - (j as f32 + 0.5) / BANDS as f32;
        t * t * (3.0 - 2.0 * t)
    };
    for j in (0..BANDS).rev() {
        // Band j is covered by rects j..BANDS; pick each rect's alpha so the
        // composite over band j equals alpha(j).
        let b = 1.0 - (1.0 - alpha(j)) / (1.0 - alpha(j + 1));
        let w = r.w * (j + 1) as f32 / BANDS as f32;
        let band = if from_left { Rect::new(r.x, r.y, w, r.h) } else { Rect::new(r.right() - w, r.y, w, r.h) };
        ui.c.fill_rect(band, Color::black(b.clamp(0.0, 1.0)));
    }
}

/// Drop zone: a fill plus one solid hairline whose outer edge follows the
/// fill's corners exactly (no fill peeking past the stroke). On the Pill card
/// (`card`) its corners in the card's rounded corners follow those.
fn drop_zone(ui: &mut Ui, card: Option<(Rect, f32)>, zone: Rect, radius: f32, fill: Color, stroke: Color) {
    super::card::fill_box(ui, card, zone, radius, fill);
    super::card::stroke_box(ui, card, zone, radius, stroke, ZONE_STROKE);
}

/// Redraw when the Undo pill expires.
pub fn next_wake(_st: &AppState) -> Option<f64> {
    LOCAL.with(|l| l.borrow().undo.as_ref().map(|(_, t)| t + UNDO_SECS))
}
