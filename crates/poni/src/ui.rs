//! Immediate-mode widgets on top of [`Canvas`].
//!
//! Pages call `ui.button(id, rect)` etc. while drawing; every interactive
//! region is recorded as a [`Hit`]. Mouse presses/releases from the window are
//! resolved against the previous frame's hits, so a click shows up as
//! `clicked == true` on the next frame (the app always repaints after input).

use crate::gfx::{Canvas, TextStyle, Weight};
use crate::icons::Icon;
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::design::{control, radius, space};

pub type WidgetId = u64;

/// Stable widget id from a string (FNV-1a).
pub fn wid(s: &str) -> WidgetId {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Widget id for the `n`-th item of a list.
pub fn wid_n(s: &str, n: u64) -> WidgetId {
    wid(s) ^ n.wrapping_mul(0x9E3779B97F4A7C15)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyInput {
    Char(char),
    Backspace,
    Delete,
    Enter,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Ctrl+V (paste from clipboard; the app supplies the text as chars).
    SelectAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Arrow,
    Hand,
    Text,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub id: WidgetId,
    pub rect: Rect,
    pub cursor: CursorKind,
}

/// macOS 的输入法文档使用字符下标；与 AppKit 的 UTF-16 下标在窗口边界转换。
#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
pub struct TextEdit {
    pub id: WidgetId,
    pub text: String,
    pub selection: (usize, usize),
    pub marked: Option<(usize, usize)>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
pub struct TextSession {
    pub edit: TextEdit,
    pub caret_rect: Rect,
    pub max_chars: usize,
}

#[cfg(any(windows, target_os = "macos"))]
#[derive(Debug)]
pub struct Composition {
    pub(crate) text: String,
    pub(crate) selection: (usize, usize),
}

/// Windows 输入法使用最终窗口坐标；字符位置按 UTF-16 编码单元排列。
#[cfg(windows)]
#[derive(Debug)]
pub struct ImeAnchor {
    pub id: WidgetId,
    pub caret_rect: Rect,
    pub text_rect: Rect,
    pub composition_positions: Vec<Rect>,
}

/// Input state owned by the app and shared with widgets.
#[derive(Debug, Default)]
pub struct Input {
    /// Cursor position in canvas DIPs (None when outside the island).
    pub mouse: Option<Point>,
    /// Left button is held.
    pub down: bool,
    /// Widget under the cursor when the button went down.
    pub pressed_id: Option<WidgetId>,
    /// The left button went down since the last frame (valid for one frame).
    pub just_pressed: bool,
    pub press_pos: Option<Point>,
    /// Click resolved on mouse-up (valid for exactly one frame).
    pub clicked: Option<(WidgetId, MouseButton)>,
    /// Widget that was held when the left button came up, and where it came
    /// up, even off the widget (a slider drag; valid for exactly one frame).
    pub released: Option<(WidgetId, Point)>,
    /// Mouse-wheel notches this frame (+ = away from user).
    pub wheel: f32,
    /// Keys typed while a text field has focus.
    pub keys: Vec<KeyInput>,
    /// Widget that owns keyboard focus.
    pub focused: Option<WidgetId>,
    /// Caret position (chars) inside the focused field.
    pub caret: usize,
    #[cfg(target_os = "macos")]
    pub text_edit: Option<TextEdit>,
    #[cfg(target_os = "macos")]
    pub text_session: Option<TextSession>,
    #[cfg(any(windows, target_os = "macos"))]
    pub composition: Option<Composition>,
    #[cfg(windows)]
    pub ime_anchor: Option<ImeAnchor>,
    #[cfg(target_os = "macos")]
    pub selection_len: usize,
}

impl Input {
    /// Called after each frame: one-shot inputs are consumed.
    pub fn end_frame(&mut self) {
        self.clicked = None;
        self.released = None;
        self.just_pressed = false;
        self.wheel = 0.0;
        self.keys.clear();
        #[cfg(target_os = "macos")]
        {
            self.text_edit = None;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Resp {
    pub hovered: bool,
    /// Held down on this widget.
    pub pressed: bool,
    pub clicked: bool,
    pub rclicked: bool,
    /// While pressed: distance moved since the press (DIPs).
    pub drag: Option<(f32, f32)>,
}

/// What a seekable [`Ui::progress_bar`] did this frame (fractions 0..=1).
#[derive(Clone, Copy, Debug, Default)]
pub struct Scrub {
    /// Held down: where the pointer is along the bar.
    pub held: Option<f32>,
    /// Let go: where to seek.
    pub released: Option<f32>,
    /// The press on the bar began this frame.
    pub began: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextInputResp {
    pub focused: bool,
    pub submitted: bool,
    pub cancelled: bool,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum InputLook {
    Box,
    Row { accent: Color, icon: Option<Icon> },
}

/// The button system: two heights and three looks.
pub const PILL_SMALL: f32 = control::SMALL;
pub const PILL_REGULAR: f32 = control::REGULAR;
/// Gap between neighbouring capsules.
pub const PILL_GAP: f32 = space::SM;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PillStyle {
    /// Secondary utility (Clear, Undo, …): neutral fill, dim label that brightens on hover.
    Utility,
    /// A page action: neutral fill, full-white label.
    Neutral,
    /// The page's one primary action: accent at 0.20 fill, accent label.
    Primary(Color),
    /// A utility capsule whose label is in an accent colour (e.g. "Undo").
    UtilityAccent(Color),
}

pub struct Ui<'a, 'g> {
    pub c: &'a mut Canvas<'g>,
    pub input: &'a mut Input,
    pub hits: &'a mut Vec<Hit>,
    /// Monotonic seconds.
    pub now: f64,
    repaint: bool,
    animating: bool,
    live: bool,
    wants_focus: Option<bool>,
}

impl<'a, 'g> Ui<'a, 'g> {
    pub fn new(c: &'a mut Canvas<'g>, input: &'a mut Input, hits: &'a mut Vec<Hit>, now: f64) -> Self {
        Self { c, input, hits, now, repaint: false, animating: false, live: false, wants_focus: None }
    }

    /// Ask for one more frame (e.g. after a state change).
    pub fn request_repaint(&mut self) {
        self.repaint = true;
    }

    /// Ask for continuous frames while something moves (e.g. a lyric scroll).
    pub fn request_anim(&mut self) {
        self.animating = true;
    }

    /// Ask for slow ambient frames (~30 fps) while something drifts steadily,
    /// e.g. a progress bar while music plays. Paused when the screen is off.
    pub fn request_live(&mut self) {
        self.live = true;
    }

    pub fn live_requested(&self) -> bool {
        self.live
    }

    pub fn repaint_requested(&self) -> bool {
        self.repaint
    }

    pub fn anim_requested(&self) -> bool {
        self.animating
    }

    /// `Some(true)` = a text field wants keyboard focus, `Some(false)` = release it.
    pub fn focus_request(&self) -> Option<bool> {
        self.wants_focus
    }

    pub fn hovered(&self, r: Rect) -> bool {
        self.input.mouse.is_some_and(|m| r.contains(m))
    }

    /// Register an interactive region and get its state.
    pub fn interact(&mut self, id: WidgetId, r: Rect, cursor: CursorKind) -> Resp {
        self.hits.push(Hit { id, rect: r, cursor });
        let hovered = self.hovered(r);
        let pressed = self.input.down && self.input.pressed_id == Some(id);
        let clicked = self.input.clicked == Some((id, MouseButton::Left));
        let rclicked = self.input.clicked == Some((id, MouseButton::Right));
        if clicked || rclicked {
            self.repaint = true;
        }
        let drag = if pressed {
            match (self.input.press_pos, self.input.mouse) {
                (Some(a), Some(b)) => Some((b.x - a.x, b.y - a.y)),
                _ => None,
            }
        } else {
            None
        };
        Resp { hovered, pressed, clicked, rclicked, drag }
    }

    /// Invisible clickable area.
    pub fn button(&mut self, id: WidgetId, r: Rect) -> Resp {
        self.interact(id, r, CursorKind::Hand)
    }

    /// Round icon button with hover/pressed background.
    pub fn icon_button(&mut self, id: WidgetId, r: Rect, icon: Icon, color: Color, icon_size: f32) -> Resp {
        let resp = self.button(id, r);
        let bg = if resp.pressed {
            palette::SURFACE_PRESSED
        } else if resp.hovered {
            palette::SURFACE_HOVER
        } else {
            Color::white(0.0)
        };
        if bg.a > 0.0 {
            self.c.fill_rrect(r, r.h.min(r.w) / 2.0, bg);
        }
        let ir = Rect::from_center(r.center(), icon_size, icon_size);
        let col = if resp.pressed { color.mul_alpha(0.8) } else { color };
        self.c.icon(icon, ir, col);
        resp
    }

    /// Width a [`Ui::pill`] needs for `label` (and an optional icon).
    pub fn pill_width(&mut self, label: &str, icon: bool, small: bool) -> f32 {
        let (size, isz, pad) = if small { (crate::gfx::ts::FOOTNOTE, control::ICON_SMALL, space::MD) } else { (crate::gfx::ts::BODY, control::ICON, space::LG) };
        let tw = if label.is_empty() { 0.0 } else { self.c.measure(label, &TextStyle::new(size).weight(Weight::Semibold), 400.0).0 };
        let iw = if icon { isz + if label.is_empty() { 0.0 } else { 6.0 } } else { 0.0 };
        (2.0 * pad + iw + tw).ceil()
    }

    /// A capsule button from the button system. Height decides the size:
    /// `PILL_SMALL` (28) = 12 pt label / 13 icon, `PILL_REGULAR` (34) = 13 / 14.
    pub fn pill(&mut self, id: WidgetId, r: Rect, label: &str, icon: Option<Icon>, style: PillStyle) -> Resp {
        let resp = self.button(id, r);
        let small = r.h <= 30.0;
        let (fill, text) = match style {
            PillStyle::Utility => {
                let f = if resp.pressed { Color::white(0.18) } else if resp.hovered { Color::white(0.12) } else { Color::white(0.065) };
                (f, if resp.hovered || resp.pressed { palette::TEXT } else { palette::TEXT_DIM })
            }
            PillStyle::Neutral => {
                let f = if resp.pressed { Color::white(0.18) } else if resp.hovered { Color::white(0.12) } else { Color::white(0.065) };
                (f, palette::TEXT)
            }
            PillStyle::Primary(c) => {
                let a = if resp.pressed { 0.34 } else if resp.hovered { 0.26 } else { 0.18 };
                (c.with_alpha(a), c)
            }
            PillStyle::UtilityAccent(c) => {
                let f = c.with_alpha(if resp.pressed { 0.20 } else if resp.hovered { 0.14 } else { 0.08 });
                (f, c)
            }
        };
        let corner = radius::CONTROL.min(r.h / 2.0);
        self.c.fill_rrect(r, corner, fill);
        self.c.stroke_rrect(r, corner, text.mul_alpha(0.10), 1.0);
        let (size, isz) = if small { (crate::gfx::ts::FOOTNOTE, control::ICON_SMALL) } else { (crate::gfx::ts::BODY, control::ICON) };
        let st = TextStyle::new(size).weight(Weight::Semibold).color(text);
        // 限定内容宽度后再居中；窄按钮的文字交给 Canvas 省略，不挤出图标。
        let inner = r.inset(space::SM.min(r.w.max(0.0) / 4.0), 0.0);
        let icon_w = if icon.is_some() { isz.min(inner.w.max(0.0)) } else { 0.0 };
        let gap = if icon.is_some() && !label.is_empty() { control::ICON_GAP.min((inner.w - icon_w).max(0.0)) } else { 0.0 };
        let text_w = if label.is_empty() { 0.0 } else { self.c.measure(label, &st, 400.0).0.min((inner.w - icon_w - gap).max(0.0)) };
        let x = inner.x + (inner.w - icon_w - gap - text_w).max(0.0) / 2.0;
        if let Some(i) = icon {
            self.c.icon(i, Rect::new(x, r.center().y - icon_w / 2.0, icon_w, icon_w), text);
        }
        if text_w > 0.0 {
            // Core Text 在窄到连省略号也放不下时可能保留原行，裁剪仍守住按钮边界。
            let label_rect = Rect::new(x + icon_w + gap, r.y, text_w, r.h);
            self.c.clip(label_rect);
            self.c.text(label, label_rect, &st);
            self.c.unclip();
        }
        resp
    }

    /// Mouse-wheel notches over `r` (consumed so only one widget scrolls).
    pub fn wheel_in(&mut self, r: Rect) -> f32 {
        if self.hovered(r) && self.input.wheel != 0.0 {
            let w = self.input.wheel;
            self.input.wheel = 0.0;
            w
        } else {
            0.0
        }
    }

    /// Single-line text field. Click to focus, Enter submits, Esc cancels.
    pub fn text_input(&mut self, id: WidgetId, r: Rect, buf: &mut String, placeholder: &str, max_chars: usize) -> TextInputResp {
        self.text_input_styled(id, r, buf, placeholder, max_chars, InputLook::Box)
    }

    /// A list-row style field: no box until hovered/focused, a leading icon in
    /// the same column as list checkboxes (centred at x+18, text at x+36), and
    /// an accent focus ring.
    pub fn text_input_row(&mut self, id: WidgetId, r: Rect, buf: &mut String, placeholder: &str, max_chars: usize, accent: Color, icon: Option<Icon>) -> TextInputResp {
        self.text_input_styled(id, r, buf, placeholder, max_chars, InputLook::Row { accent, icon })
    }

    fn text_input_styled(&mut self, id: WidgetId, r: Rect, buf: &mut String, placeholder: &str, max_chars: usize, look: InputLook) -> TextInputResp {
        let resp = self.interact(id, r, CursorKind::Text);
        let mut out = TextInputResp::default();
        if resp.clicked && self.input.focused != Some(id) {
            self.input.focused = Some(id);
            self.input.caret = buf.chars().count();
            #[cfg(any(windows, target_os = "macos"))]
            {
                self.input.composition = None;
            }
            #[cfg(target_os = "macos")]
            {
                self.input.selection_len = 0;
            }
            self.wants_focus = Some(true);
        }
        let focused = self.input.focused == Some(id);
        out.focused = focused;
        if focused {
            #[cfg(target_os = "macos")]
            if let Some(edit) = self.input.text_edit.take().filter(|e| e.id == id) {
                let mut chars: Vec<char> = edit.text.chars().collect();
                self.input.composition = edit.marked.map(|(start, len)| Composition {
                    text: chars.drain(start..start + len).collect(),
                    selection: (edit.selection.0.saturating_sub(start), edit.selection.1),
                });
                let committed: String = chars.into_iter().collect();
                out.changed = *buf != committed;
                *buf = committed;
                self.input.caret = edit.marked.map_or(edit.selection.0, |m| m.0);
                self.input.selection_len = if edit.marked.is_none() { edit.selection.1 } else { 0 };
            }
            let keys = std::mem::take(&mut self.input.keys);
            let mut chars: Vec<char> = buf.chars().collect();
            let mut caret = self.input.caret.min(chars.len());
            for k in keys {
                // 合成中的编辑键由输入法消费；已确认文字仍通过 Char 入队。
                #[cfg(windows)]
                if self.input.composition.is_some() && !matches!(k, KeyInput::Char(_)) {
                    continue;
                }
                match k {
                    KeyInput::Char(c) if !c.is_control() => {
                        if chars.len() < max_chars {
                            chars.insert(caret, c);
                            caret += 1;
                            out.changed = true;
                        }
                    }
                    KeyInput::Backspace if caret > 0 => {
                        caret -= 1;
                        chars.remove(caret);
                        out.changed = true;
                    }
                    KeyInput::Delete if caret < chars.len() => {
                        chars.remove(caret);
                        out.changed = true;
                    }
                    KeyInput::Left => caret = caret.saturating_sub(1),
                    KeyInput::Right => caret = (caret + 1).min(chars.len()),
                    KeyInput::Home => caret = 0,
                    KeyInput::End | KeyInput::SelectAll => caret = chars.len(),
                    KeyInput::Enter => {
                        #[cfg(target_os = "macos")]
                        if self.input.composition.is_some() {
                            continue;
                        }
                        out.submitted = true;
                    }
                    KeyInput::Escape => {
                        out.cancelled = true;
                    }
                    _ => {}
                }
            }
            *buf = chars.into_iter().collect();
            self.input.caret = caret;
            if out.cancelled {
                self.input.focused = None;
                self.wants_focus = Some(false);
            }
            if out.changed || out.submitted || out.cancelled {
                self.repaint = true;
            }
        }
        // Draw.
        let inner = match look {
            InputLook::Box => {
                let bg = if focused { Color::white(0.13) } else if resp.hovered { Color::white(0.1) } else { palette::SURFACE };
                self.c.fill_rrect(r, radius::CONTROL, bg);
                if focused {
                    self.c.stroke_rrect(r, radius::CONTROL, palette::BLUE, 1.0);
                }
                r.inset(10.0, 0.0)
            }
            InputLook::Row { accent, icon } => {
                if focused || resp.hovered {
                    self.c.fill_rrect(r, radius::CONTROL, if focused { Color::white(0.10) } else { palette::SURFACE });
                }
                if focused {
                    self.c.stroke_rrect(r, radius::CONTROL, accent, 1.5);
                }
                if let Some(i) = icon {
                    let col = if buf.is_empty() { palette::TEXT_FAINT } else { accent };
                    self.c.icon(i, Rect::from_center(Point::new(r.x + 18.0, r.center().y), 16.0, 16.0), col);
                }
                Rect::new(r.x + 36.0, r.y, (r.w - 46.0).max(0.0), r.h)
            }
        };
        let st = TextStyle::new(crate::gfx::ts::BODY);
        let display = std::borrow::Cow::Borrowed(buf.as_str());
        let display_caret = self.input.caret;
        #[cfg(any(windows, target_os = "macos"))]
        let mut display = display;
        #[cfg(any(windows, target_os = "macos"))]
        let mut display_caret = display_caret;
        #[cfg(any(windows, target_os = "macos"))]
        let mut marked = None;
        #[cfg(any(windows, target_os = "macos"))]
        if focused {
            if let Some(composition) = &self.input.composition {
                let mut chars: Vec<char> = display.chars().collect();
                let start = self.input.caret.min(chars.len());
                let len = composition.text.chars().count();
                chars.splice(start..start, composition.text.chars());
                display = std::borrow::Cow::Owned(chars.into_iter().collect());
                display_caret = start + composition.selection.0;
                marked = Some((start, len));
            }
        }
        // 编辑时按自然宽度绘制整行并裁剪视口，文字、选区和输入法锚点共用同一偏移。
        let caret_w = if focused {
            let before: String = display.chars().take(display_caret).collect();
            self.c.measure_raw(&before, &st, 100_000.0).0
        } else {
            0.0
        };
        let scroll = if focused { (caret_w + 2.0 - inner.w).max(0.0) } else { 0.0 };
        let text_x = inner.x - scroll;
        self.c.clip(inner);
        #[cfg(target_os = "macos")]
        if focused && self.input.composition.is_none() && self.input.selection_len > 0 {
            let prefix: String = display.chars().take(display_caret).collect();
            let through: String = display.chars().take(display_caret + self.input.selection_len).collect();
            let left = text_x + self.c.measure_raw(&prefix, &st, 100_000.0).0;
            let right = text_x + self.c.measure_raw(&through, &st, 100_000.0).0;
            self.c.fill_rrect(Rect::new(left, r.center().y - 10.0, right - left, 20.0), 2.0, Color::white(0.20));
        }
        if display.is_empty() {
            self.c.text(placeholder, inner, &st.color(palette::TEXT_FAINT));
        } else if focused {
            let text_w = self.c.measure_raw(&display, &st, 100_000.0).0;
            self.c.text_raw(&display, Rect::new(text_x, inner.y, text_w.max(inner.w) + 2.0, inner.h), &st);
        } else {
            self.c.text_raw(&display, inner, &st);
        }
        if focused {
            let x = text_x + caret_w + 0.5;
            let cy = r.center().y;
            self.c.line(Point::new(x, cy - 8.0), Point::new(x, cy + 8.0), Color::white(0.9), 1.5);
            #[cfg(any(windows, target_os = "macos"))]
            {
                if let Some((start, len)) = marked {
                    let prefix: String = display.chars().take(start).collect();
                    let through: String = display.chars().take(start + len).collect();
                    let left = text_x + self.c.measure_raw(&prefix, &st, 100_000.0).0;
                    let right = text_x + self.c.measure_raw(&through, &st, 100_000.0).0;
                    self.c.line(Point::new(left, cy + 10.0), Point::new(right, cy + 10.0), Color::white(0.7), 1.0);
                }
            }
            #[cfg(target_os = "macos")]
            {
                let selection_len = self.input.composition.as_ref().map_or(self.input.selection_len, |c| c.selection.1);
                self.input.text_session = Some(TextSession {
                    edit: TextEdit { id, text: display.into_owned(), selection: (display_caret, selection_len), marked },
                    caret_rect: Rect::new(x, cy - 9.0, 1.5, 18.0),
                    max_chars,
                });
            }
            #[cfg(windows)]
            {
                let caret_rect = self.c.map_rect_to_window(Rect::new(x, cy - 9.0, 1.5, 18.0));
                let mut composition_positions = Vec::new();
                if let Some((start, len)) = marked {
                    let mut prefix: String = display.chars().take(start).collect();
                    for ch in display.chars().skip(start).take(len) {
                        let left = text_x + self.c.measure_raw(&prefix, &st, 100_000.0).0;
                        let position = self.c.map_rect_to_window(Rect::new(
                            left.clamp(inner.x, inner.right()),
                            cy - 9.0,
                            1.5,
                            18.0,
                        ));
                        // UTF-16 代理对的两个编码单元属于同一个可见字符。
                        composition_positions.extend(std::iter::repeat_n(position, ch.len_utf16()));
                        prefix.push(ch);
                    }
                    let left = text_x + self.c.measure_raw(&prefix, &st, 100_000.0).0;
                    composition_positions.push(self.c.map_rect_to_window(Rect::new(
                        left.clamp(inner.x, inner.right()),
                        cy - 9.0,
                        1.5,
                        18.0,
                    )));
                }
                self.input.ime_anchor = Some(ImeAnchor {
                    id,
                    caret_rect,
                    text_rect: self.c.map_rect_to_window(inner),
                    composition_positions,
                });
            }
        }
        self.c.unclip();
        out
    }

    /// Horizontal progress bar. When `seekable` it can be clicked or dragged:
    /// while held it shows (and reports) the fraction under the pointer, and
    /// the release reports where to seek, even if the pointer slid off the bar.
    pub fn progress_bar(&mut self, id: WidgetId, r: Rect, progress: f32, fill: Color, seekable: bool) -> Scrub {
        let hit = Rect::new(r.x, r.y - 6.0, r.w, r.h + 12.0);
        let resp = if seekable { self.button(id, hit) } else { Resp::default() };
        let frac = |x: f32| if r.w > 0.0 { ((x - r.x) / r.w).clamp(0.0, 1.0) } else { 0.0 };
        let held = if resp.pressed { self.input.mouse.map(|m| frac(m.x)) } else { None };
        let began = resp.pressed && self.input.just_pressed;
        let released = self.input.released.filter(|(rid, _)| seekable && *rid == id).map(|(_, p)| frac(p.x));
        let thick = if (resp.hovered || held.is_some()) && seekable { r.h + 2.0 } else { r.h };
        let bar = Rect::new(r.x, r.center().y - thick / 2.0, r.w, thick);
        self.c.fill_rrect(bar, thick / 2.0, palette::TRACK);
        let p = held.or(released).unwrap_or(progress).clamp(0.0, 1.0);
        if p > 0.0 {
            self.c.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * p).max(thick), bar.h), thick / 2.0, fill);
        }
        if released.is_some() {
            self.repaint = true;
        }
        Scrub { held, released, began }
    }

    /// Release keyboard focus (e.g. after submitting).
    pub fn blur(&mut self) {
        #[cfg(windows)]
        {
            self.input.composition = None;
            self.input.ime_anchor = None;
        }
        #[cfg(target_os = "macos")]
        {
            self.input.composition = None;
            self.input.text_edit = None;
            self.input.text_session = None;
            self.input.selection_len = 0;
        }
        if self.input.focused.is_some() {
            self.input.focused = None;
            self.wants_focus = Some(false);
        }
    }
}

/// 两个设置视图共用的分段选择器宽度；与26 DIP控件的文字和内边距保持一致。
pub(crate) fn picker_width(ui: &mut Ui, names: &[&str]) -> f32 {
    let style = TextStyle::footnote().weight(Weight::Semibold);
    4.0 + names.iter().map(|name| (22.0 + ui.c.measure(name, &style, 300.0).0).ceil()).sum::<f32>()
}

/// 保存故障时显示原因及操作。正常状态不占空间，不创建定时器或自动重试。
pub fn save_warning(ui: &mut Ui, errors: &crate::persistence::SaveErrors, area: Rect) -> (bool, bool) {
    let title = if errors.quit_blocked {
        "退出已取消：更改未保存"
    } else {
        match (errors.settings.is_some(), errors.store.is_some()) {
            (true, true) => "设置与数据尚未保存",
            (true, false) => "设置尚未保存",
            _ => "待办等数据尚未保存",
        }
    };
    let protected = errors.settings == Some(crate::persistence::SaveError::Protected) || errors.store == Some(crate::persistence::SaveError::Protected);
    let detail = if protected { "旧文件受保护，请先备份并修复后重启" } else { "请检查磁盘空间及数据文件夹权限" };
    ui.c.fill_rrect(area, 8.0, palette::ORANGE.with_alpha(0.10));
    let discard = Rect::new(area.right() - 102.0, area.y + 9.0, 94.0, 28.0);
    let retry_right = if errors.quit_blocked { discard.x - 6.0 } else { area.right() - 8.0 };
    let retry = Rect::new(retry_right - 64.0, area.y + 9.0, 64.0, 28.0);
    let text_right = if errors.retryable() { retry.x - 8.0 } else if errors.quit_blocked { discard.x - 8.0 } else { area.right() - 8.0 };
    let width = (text_right - area.x - 10.0).max(0.0);
    ui.c.text(title, Rect::new(area.x + 10.0, area.y + 4.0, width, 20.0), &TextStyle::footnote().weight(Weight::Semibold).color(palette::ORANGE));
    ui.c.text(detail, Rect::new(area.x + 10.0, area.y + 24.0, width, 18.0), &TextStyle::new(11.0).color(palette::TEXT_DIM));
    let retry_clicked = errors.retryable() && ui.pill(wid("save.retry"), retry, "重试", None, PillStyle::Utility).clicked;
    let discard_clicked = errors.quit_blocked && ui.pill(wid("save.discard-quit"), discard, "放弃并退出", None, PillStyle::UtilityAccent(palette::RED)).clicked;
    (retry_clicked, discard_clicked)
}
