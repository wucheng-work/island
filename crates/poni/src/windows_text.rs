//! 将自绘输入框的合成文字和真实光标位置接入 Windows 输入法。
//! 只响应窗口消息及已有绘制帧，不创建计时器、线程或额外轮询。

use crate::ui::{Composition, Input, KeyInput, WidgetId};
use poni_core::geom::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, HBITMAP};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Default)]
pub struct WindowsText {
    owner: HWND,
    field: Option<WidgetId>,
    composing: bool,
    caret_size: Option<(i32, i32)>,
    positioned_at: Option<(RECT, POINT)>,
    candidate_indices: u32,
    high_surrogate: Option<u16>,
}

impl WindowsText {
    /// 在绘制完文字、取得系统键盘焦点后同步位置。DIP 只在此处转换成像素。
    pub fn sync(&mut self, hwnd: HWND, input: &mut Input, scale: f32) {
        if unsafe { GetFocus() } != hwnd {
            if self.owner == hwnd {
                self.blur(hwnd, input);
            }
            return;
        }
        let Some(anchor) = input
            .ime_anchor
            .as_ref()
            .filter(|a| input.focused == Some(a.id))
        else {
            self.blur(hwnd, input);
            return;
        };
        let id = anchor.id;
        if self.field != Some(id) || self.owner != hwnd {
            self.blur(self.owner, input);
            self.owner = hwnd;
            self.field = Some(id);
            self.candidate_indices = 1;
        }
        let Some(anchor) = input.ime_anchor.as_ref() else {
            return;
        };
        let caret = pixels(anchor.caret_rect, scale);
        let size = (
            (caret.right - caret.left).max(1),
            (caret.bottom - caret.top).max(1),
        );
        let owns_caret = caret_owner() == hwnd;
        if self.caret_size != Some(size) || !owns_caret {
            unsafe {
                if owns_caret {
                    let _ = DestroyCaret();
                }
                // 系统光标保持隐藏，仅向 TSF/旧输入法报告位置；可见光标仍由 UI 绘制。
                self.caret_size = CreateCaret(hwnd, HBITMAP::default(), size.0, size.1)
                    .ok()
                    .map(|_| size);
            }
        }
        unsafe {
            let _ = SetCaretPos(caret.left, caret.top);
        }
        let mut origin = POINT::default();
        if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
            return;
        }
        if self.positioned_at != Some((caret, origin)) {
            self.positioned_at = Some((caret, origin));
            self.position(hwnd, caret);
        }
    }

    /// 失焦、切换输入框或关闭窗口时取消未确认文字，避免提交到另一个输入框。
    pub fn blur(&mut self, hwnd: HWND, input: &mut Input) {
        if self.owner != hwnd {
            return;
        }
        let composing = std::mem::take(&mut self.composing);
        self.field = None;
        self.positioned_at = None;
        input.composition = None;
        self.high_surrogate = None;
        unsafe {
            // SetFocus 的重入消息可能被窗口宿主略过，不能销毁另一窗口新建的光标。
            if self.caret_size.take().is_some() && caret_owner() == hwnd {
                let _ = DestroyCaret();
            }
            if composing && !hwnd.is_invalid() && GetFocus() == hwnd {
                let context = ImmGetContext(hwnd);
                if !context.is_invalid() {
                    let _ = ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
                    let _ = ImmReleaseContext(hwnd, context);
                }
            }
        }
        self.owner = HWND::default();
    }

    /// 返回 Some 表示已消费；返回 None 时仍交给原窗口过程及 DefWindowProc。
    pub fn message(
        &mut self,
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
        input: &mut Input,
        scale: f32,
    ) -> Option<LRESULT> {
        if msg == WM_KILLFOCUS {
            self.blur(hwnd, input);
            return None;
        }
        if input.focused.is_none() {
            return None;
        }
        match msg {
            WM_CHAR => {
                let unit = wp.0 as u32;
                let character = if (0xD800..0xDC00).contains(&unit) {
                    self.high_surrogate = Some(unit as u16);
                    None
                } else if (0xDC00..0xE000).contains(&unit) {
                    self.high_surrogate.take().and_then(|high| {
                        char::decode_utf16([high, unit as u16])
                            .next()
                            .and_then(Result::ok)
                    })
                } else {
                    self.high_surrogate = None;
                    char::from_u32(unit)
                };
                if let Some(character) = character.filter(|c| !c.is_control()) {
                    input.keys.push(KeyInput::Char(character));
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                if self.composing || wp.0 == VK_PROCESSKEY.0 as usize {
                    // 不落入窗口的 Esc 关闭、Enter 保存等应用级快捷键。
                    return Some(unsafe { DefWindowProcW(hwnd, msg, wp, lp) });
                }
                let key = match VIRTUAL_KEY(wp.0 as u16) {
                    VK_BACK => KeyInput::Backspace,
                    VK_DELETE => KeyInput::Delete,
                    VK_RETURN => KeyInput::Enter,
                    VK_ESCAPE => KeyInput::Escape,
                    VK_LEFT => KeyInput::Left,
                    VK_RIGHT => KeyInput::Right,
                    VK_UP => KeyInput::Up,
                    VK_DOWN => KeyInput::Down,
                    VK_HOME => KeyInput::Home,
                    VK_END => KeyInput::End,
                    VK_V if unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 => {
                        if let Some(text) = crate::services::clipboard::read_text(hwnd) {
                            input.keys.extend(
                                text.chars().filter(|c| !c.is_control()).map(KeyInput::Char),
                            );
                        }
                        return Some(LRESULT(0));
                    }
                    _ => return None,
                };
                input.keys.push(key);
                Some(LRESULT(0))
            }
            WM_SETFOCUS | WM_INPUTLANGCHANGE => {
                self.positioned_at = None;
                self.sync(hwnd, input, scale);
                None
            }
            WM_IME_SETCONTEXT => {
                // 合成文字由本应用绘制；候选词仍使用系统输入法的原生窗口。
                let flags = LPARAM(lp.0 & !(ISC_SHOWUICOMPOSITIONWINDOW as isize));
                Some(unsafe { DefWindowProcW(hwnd, msg, wp, flags) })
            }
            WM_IME_STARTCOMPOSITION => {
                self.sync(hwnd, input, scale);
                self.composing = true;
                input.composition = Some(Composition {
                    text: String::new(),
                    selection: (0, 0),
                });
                Some(LRESULT(0))
            }
            WM_IME_COMPOSITION => {
                let context = unsafe { ImmGetContext(hwnd) };
                if context.is_invalid() {
                    return None;
                }
                let flags = lp.0 as u32;
                if flags & GCS_RESULTSTR.0 != 0 {
                    let result = composition_string(context, GCS_RESULTSTR);
                    input.keys.extend(
                        result
                            .chars()
                            .filter(|c| !c.is_control())
                            .map(KeyInput::Char),
                    );
                    input.composition = None;
                }
                if flags & (GCS_COMPSTR.0 | GCS_CURSORPOS.0) != 0 {
                    let text = composition_string(context, GCS_COMPSTR);
                    let utf16_cursor =
                        unsafe { ImmGetCompositionStringW(context, GCS_CURSORPOS, None, 0) }.max(0)
                            as usize;
                    let before: Vec<u16> = text.encode_utf16().take(utf16_cursor).collect();
                    let cursor = String::from_utf16_lossy(&before).chars().count();
                    self.composing = true;
                    input.composition = Some(Composition {
                        text,
                        selection: (cursor, 0),
                    });
                } else if flags == 0 {
                    input.composition = None;
                    self.composing = false;
                }
                unsafe {
                    let _ = ImmReleaseContext(hwnd, context);
                }
                // 已读取 GCS_RESULTSTR，不能再交给 DefWindowProc 转发成 WM_CHAR。
                Some(LRESULT(0))
            }
            WM_IME_ENDCOMPOSITION => {
                self.composing = false;
                input.composition = None;
                Some(LRESULT(0))
            }
            WM_IME_NOTIFY
                if wp.0 == IMN_OPENCANDIDATE as usize || wp.0 == IMN_CHANGECANDIDATE as usize =>
            {
                self.candidate_indices |= lp.0 as u32;
                self.positioned_at = None;
                self.sync(hwnd, input, scale);
                // 默认 IME 窗口负责显示和更新候选词。
                None
            }
            WM_IME_NOTIFY if wp.0 == IMN_CLOSECANDIDATE as usize => {
                self.candidate_indices &= !(lp.0 as u32);
                None
            }
            WM_IME_REQUEST if wp.0 == IMR_QUERYCHARPOSITION as usize => {
                Some(unsafe { query_character(hwnd, lp, input, scale) })
            }
            _ => None,
        }
    }

    fn position(&self, hwnd: HWND, caret: RECT) {
        unsafe {
            let context = ImmGetContext(hwnd);
            if context.is_invalid() {
                return;
            }
            let _ = ImmSetCompositionWindow(
                context,
                &COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: POINT {
                        x: caret.left,
                        y: caret.top,
                    },
                    rcArea: RECT::default(),
                },
            );
            for index in 0..32 {
                if (self.candidate_indices | 1) & (1 << index) == 0 {
                    continue;
                }
                let _ = ImmSetCandidateWindow(
                    context,
                    &CANDIDATEFORM {
                        dwIndex: index,
                        dwStyle: CFS_EXCLUDE,
                        ptCurrentPos: POINT {
                            x: caret.left,
                            y: caret.bottom,
                        },
                        rcArea: caret,
                    },
                );
            }
            let _ = ImmReleaseContext(hwnd, context);
        }
    }
}

fn caret_owner() -> HWND {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(GetCurrentThreadId(), &mut info) }.is_ok() {
        info.hwndCaret
    } else {
        HWND::default()
    }
}

fn pixels(rect: Rect, scale: f32) -> RECT {
    RECT {
        left: (rect.x * scale).round() as i32,
        top: (rect.y * scale).round() as i32,
        right: (rect.right() * scale).round() as i32,
        bottom: (rect.bottom() * scale).round() as i32,
    }
}

fn composition_string(context: HIMC, kind: IME_COMPOSITION_STRING) -> String {
    let bytes = unsafe { ImmGetCompositionStringW(context, kind, None, 0) };
    if bytes <= 0 {
        return String::new();
    }
    let mut text = vec![0u16; bytes as usize / 2];
    let written = unsafe {
        ImmGetCompositionStringW(context, kind, Some(text.as_mut_ptr().cast()), bytes as u32)
    };
    if written <= 0 {
        return String::new();
    }
    text.truncate(written as usize / 2);
    String::from_utf16_lossy(&text)
}

unsafe fn query_character(hwnd: HWND, lp: LPARAM, input: &Input, scale: f32) -> LRESULT {
    let Some(position) = (lp.0 as *mut IMECHARPOSITION).as_mut() else {
        return LRESULT(0);
    };
    if position.dwSize < std::mem::size_of::<IMECHARPOSITION>() as u32 {
        return LRESULT(0);
    }
    let Some(anchor) = input
        .ime_anchor
        .as_ref()
        .filter(|a| input.focused == Some(a.id))
    else {
        return LRESULT(0);
    };
    let rect = if anchor.composition_positions.is_empty() && position.dwCharPos == 0 {
        anchor.caret_rect
    } else {
        let Some(rect) = anchor
            .composition_positions
            .get(position.dwCharPos as usize)
        else {
            return LRESULT(0);
        };
        *rect
    };
    let caret = pixels(rect, scale);
    let document = pixels(anchor.text_rect, scale);
    let mut origin = POINT::default();
    if !ClientToScreen(hwnd, &mut origin).as_bool() {
        return LRESULT(0);
    }
    position.pt = POINT {
        x: origin.x + caret.left,
        y: origin.y + caret.top,
    };
    position.cLineHeight = (caret.bottom - caret.top).max(1) as u32;
    position.rcDocument = RECT {
        left: origin.x + document.left,
        top: origin.y + document.top,
        right: origin.x + document.right,
        bottom: origin.y + document.bottom,
    };
    LRESULT(1)
}
