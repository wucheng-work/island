//! The island's window on the Mac: a borderless, non-activating panel above
//! the menu bar, sized once for the largest island and placed at the top
//! centre of a screen (over the camera notch on MacBooks that have one), or
//! where the island was dragged along the top edge. Its
//! view draws the island and forwards input; a display link on the view
//! paces frames to the screen's refresh (120 Hz on ProMotion screens).
//!
//! Everything here runs on the main thread. The panel, view and display link
//! live in a thread-local so the app can redraw without holding itself.

use crate::notch::Notch;
use crate::ui::{TextEdit, TextSession};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSDragOperation, NSDraggingContext, NSDraggingInfo, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSGraphicsContext, NSPanel, NSPasteboardTypeFileURL, NSPasteboardURLReadingFileURLsOnlyKey, NSResponder, NSScreen, NSView,
    NSRunningApplication, NSTextInputClient, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSNotFound, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, NSURL};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use poni_core::geom::Rect;
use poni_core::settings::MonitorChoice;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

/// The panel takes the keyboard only while a text field asks for it; any
/// other click leaves the keyboard with the app the person is typing in.
static WANT_KEY: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The last mouse-down or drag in the view: a drag out of the Shelf
    /// starts from it.
    static LAST_MOUSE: RefCell<Option<Retained<NSEvent>>> = const { RefCell::new(None) };
    /// A notch the smoke test gives the screen (CI's Macs have none), so its
    /// pictures show the island the way a MacBook shows it.
    static FAKE_NOTCH: Cell<Option<Notch>> = const { Cell::new(None) };
    /// 确认候选的同一次按键，即使先 insertText 再换行，也不能提交待办。
    static COMPOSING_KEY: Cell<bool> = const { Cell::new(false) };
}

/// File paths a drag carries (empty when it carries none).
fn dragged_paths(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let pb = info.draggingPasteboard();
    let class: &AnyClass = NSURL::class();
    let classes = NSArray::from_slice(&[class]);
    let yes = NSNumber::numberWithBool(true);
    let opts = NSDictionary::from_slices(&[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }], &[yes.as_ref() as &AnyObject]);
    let objs = unsafe { pb.readObjectsForClasses_options(&classes, Some(&opts)) };
    objs.map(|a| a.iter().filter_map(|o| o.downcast::<NSURL>().ok()).filter_map(|u| u.path()).map(|p| p.to_string()).collect()).unwrap_or_default()
}

/// Above the menu bar (24) and its status items, below pop-up menus (101).
const ISLAND_LEVEL: isize = 24 + 3;

/// What the island knows about the screen it sits on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenInfo {
    /// The screen's frame in AppKit's global coordinates (points, y up).
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f32,
    /// The camera notch, when the screen has one.
    pub notch: Option<Notch>,
    /// The notch's centre (or the screen's), from the screen's left edge.
    pub cx: f64,
}

/// Choose the primary screen or the screen under the pointer at this moment.
/// The caller only asks at startup, when the choice changes or screens change.
pub fn pick_screen(mtm: MainThreadMarker, monitor: MonitorChoice) -> Option<ScreenInfo> {
    let screens = NSScreen::screens(mtm);
    let primary = screens.firstObject()?;
    let screen = match monitor {
        MonitorChoice::Primary => primary,
        MonitorChoice::Cursor => {
            let p = NSEvent::mouseLocation();
            screens
                .iter()
                .find(|screen| {
                    let f = screen.frame();
                    p.x >= f.origin.x && p.x < f.origin.x + f.size.width && p.y >= f.origin.y && p.y < f.origin.y + f.size.height
                })
                .unwrap_or(primary)
        }
    };
    let info = screen_info(&screen);
    Some(ScreenInfo { notch: FAKE_NOTCH.get().or(info.notch), ..info })
}

/// Pretend the screen has a notch (the smoke test), or stop pretending.
pub fn fake_notch(n: Option<Notch>) {
    FAKE_NOTCH.set(n);
}

/// The smoke test is pretending a notch.
pub fn faking_notch() -> bool {
    FAKE_NOTCH.get().is_some()
}

fn screen_info(s: &NSScreen) -> ScreenInfo {
    let f = s.frame();
    let inset = s.safeAreaInsets().top;
    let (l, r) = (s.auxiliaryTopLeftArea(), s.auxiliaryTopRightArea());
    let notch_w = f.size.width - l.size.width - r.size.width;
    let notch = (inset > 0.0 && notch_w > 40.0 && notch_w < f.size.width / 2.0).then(|| Notch { w: notch_w as f32, h: inset as f32 });
    let cx = if notch.is_some() { l.size.width + notch_w / 2.0 } else { f.size.width / 2.0 };
    ScreenInfo { x: f.origin.x, y: f.origin.y, w: f.size.width, h: f.size.height, scale: s.backingScaleFactor() as f32, notch, cx }
}

// ---------------------------------------------------------------- classes

define_class!(
    /// The island stays non-activating except while a text field explicitly
    /// asks for the keyboard.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PoniIslandPanel"]
    pub struct IslandPanel;

    unsafe impl NSObjectProtocol for IslandPanel {}

    impl IslandPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            WANT_KEY.load(Ordering::Relaxed)
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            false
        }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            unsafe {
                let _: () = msg_send![super(self), resignKeyWindow];
            }
            crate::app::on_resign_key();
        }
    }
);

/// Input the view forwards to the app.
#[derive(Clone, Debug)]
pub enum ViewInput {
    Down { x: f32, y: f32, double: bool },
    Up { x: f32, y: f32 },
    RightUp { x: f32, y: f32 },
    Wheel { notches: f32 },
    Key { code: u16, text: String, command: bool },
    Text(TextEdit),
}

define_class!(
    /// Draws the island (y down, like the Windows canvas) and forwards input.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PoniIslandView"]
    #[ivars = RefCell<Option<TextSession>>]
    pub struct IslandView;

    unsafe impl NSObjectProtocol for IslandView {}

    impl IslandView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            false
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            if let Some(ctx) = NSGraphicsContext::currentContext() {
                let cg = ctx.CGContext();
                let r = Rect::new(dirty.origin.x as f32, dirty.origin.y as f32, dirty.size.width as f32, dirty.size.height as f32);
                crate::app::draw_into(&cg, r);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            LAST_MOUSE.with(|m| *m.borrow_mut() = Some(event.retain()));
            let p = self.local(event);
            crate::app::view_input(ViewInput::Down { x: p.0, y: p.1, double: event.clickCount() >= 2 }, Some(event));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            LAST_MOUSE.with(|m| *m.borrow_mut() = Some(event.retain()));
            crate::app::pointer_moved();
            // A Shelf item dragged far enough asked to leave the island.
            crate::app::start_pending_drag();
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let p = self.local(event);
            crate::app::view_input(ViewInput::Up { x: p.0, y: p.1 }, Some(event));
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            let p = self.local(event);
            crate::app::view_input(ViewInput::RightUp { x: p.0, y: p.1 }, Some(event));
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            // Windows counts wheel notches; a trackpad's points become fractions of one.
            let dy = event.scrollingDeltaY() as f32;
            let notches = if event.hasPreciseScrollingDeltas() { dy / 24.0 } else { dy };
            if notches != 0.0 {
                crate::app::view_input(ViewInput::Wheel { notches }, Some(event));
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if self.ivars().borrow().is_some() {
                let command = event.modifierFlags().contains(objc2_app_kit::NSEventModifierFlags::Command);
                let text = event.characters().map(|s| s.to_string()).unwrap_or_default();
                if command && text.eq_ignore_ascii_case("v") {
                    self.edit_command(sel!(paste:));
                    return;
                }
                if command && text.eq_ignore_ascii_case("a") {
                    self.edit_command(sel!(selectAll:));
                    return;
                }
                // 让系统输入法先处理组合、选词及确认，不能把原始拼音当作正文。
                COMPOSING_KEY.set(self.hasMarkedText());
                self.interpretKeyEvents(&NSArray::from_slice(&[event]));
                COMPOSING_KEY.set(false);
                return;
            }
            let text = event.characters().map(|s| s.to_string()).unwrap_or_default();
            let command = event.modifierFlags().contains(objc2_app_kit::NSEventModifierFlags::Command);
            crate::app::view_input(ViewInput::Key { code: event.keyCode(), text, command }, Some(event));
        }

        /// NSResponder 的标准键绑定仍可能使用单参数入口。
        #[unsafe(method(insertText:))]
        fn insert_plain_text(&self, string: &AnyObject) {
            self.replace_text(&input_string(string), NSRange::new(NSNotFound as usize, 0), None);
        }

        /// The display link's tick.
        #[unsafe(method(step:))]
        fn step(&self, link: &CADisplayLink) {
            crate::app::on_frame_tick(link.timestamp(), link.targetTimestamp());
        }

        // Files dragged onto the island go to the Shelf.
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if crate::dragdrop::is_dragging_out() || dragged_paths(sender).is_empty() {
                return NSDragOperation::None;
            }
            crate::app::post(crate::events::AppEvent::DragHover(true));
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            crate::app::pointer_moved();
            if crate::dragdrop::is_dragging_out() {
                NSDragOperation::None
            } else {
                NSDragOperation::Copy
            }
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            crate::app::post(crate::events::AppEvent::DragHover(false));
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_for_drag(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            true
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let paths = dragged_paths(sender);
            let any = !paths.is_empty();
            if any {
                crate::app::post(crate::events::AppEvent::ShelfAdd(paths));
            }
            any
        }
    }

    unsafe impl NSTextInputClient for IslandView {
        #[unsafe(method(insertText:replacementRange:))]
        unsafe fn insert_text(&self, string: &AnyObject, range: NSRange) {
            self.replace_text(&input_string(string), range, None);
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        unsafe fn set_marked_text(&self, string: &AnyObject, selected: NSRange, range: NSRange) {
            self.replace_text(&input_string(string), range, Some(selected));
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            let edit = self.ivars().borrow_mut().as_mut().map(|s| {
                s.edit.marked = None;
                s.edit.clone()
            });
            if let Some(edit) = edit {
                crate::app::view_input(ViewInput::Text(edit), None);
            }
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            self.ivars().borrow().as_ref().is_some_and(|s| s.edit.marked.is_some_and(|r| r.1 > 0))
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            self.ivars().borrow().as_ref().and_then(|s| s.edit.marked.map(|r| utf16_range(&s.edit.text, r))).unwrap_or(NSRange::new(NSNotFound as usize, 0))
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            self.ivars().borrow().as_ref().map_or(NSRange::new(NSNotFound as usize, 0), |s| utf16_range(&s.edit.text, s.edit.selection))
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        unsafe fn attributed_substring(&self, range: NSRange, actual: NSRangePointer) -> Option<Retained<NSAttributedString>> {
            let state = self.ivars().borrow();
            state.as_ref().filter(|s| range.location != NSNotFound as usize && range.location <= s.edit.text.encode_utf16().count()).map(|s| {
                let chars = char_range(&s.edit.text, range);
                if !actual.is_null() {
                    unsafe { *actual = utf16_range(&s.edit.text, chars) };
                }
                let text: String = s.edit.text.chars().skip(chars.0).take(chars.1).collect();
                NSAttributedString::initWithString(NSAttributedString::alloc(), &NSString::from_str(&text))
            })
        }

        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        unsafe fn first_rect(&self, _range: NSRange, actual: NSRangePointer) -> NSRect {
            let (r, selection) = self.ivars().borrow().as_ref().map(|s| (s.caret_rect, utf16_range(&s.edit.text, s.edit.selection)))
                .unwrap_or((Rect::new(0.0, 0.0, 1.0, 18.0), NSRange::new(NSNotFound as usize, 0)));
            if !actual.is_null() {
                unsafe { *actual = selection };
            }
            let local = NSRect::new(NSPoint::new(r.x as f64, r.y as f64), NSSize::new(r.w as f64, r.h as f64));
            let window_rect = self.convertRect_toView(local, None);
            self.window().map_or(window_rect, |w| w.convertRectToScreen(window_rect))
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index(&self, _point: NSPoint) -> usize {
            self.selectedRange().location
        }

        #[unsafe(method(windowLevel))]
        fn window_level(&self) -> isize {
            ISLAND_LEVEL
        }

        #[unsafe(method(doCommandBySelector:))]
        unsafe fn do_command(&self, selector: Sel) {
            self.edit_command(selector);
        }
    }

    // Shelf items dragged out of the island.
    unsafe impl NSDraggingSource for IslandView {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operations(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn drag_ended(&self, _session: &NSDraggingSession, _point: NSPoint, _op: NSDragOperation) {
            crate::dragdrop::set_dragging_out(false);
            crate::app::pointer_moved();
        }
    }
);

impl IslandView {
    /// An event's position in the view (DIPs, y down).
    fn local(&self, event: &NSEvent) -> (f32, f32) {
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        (p.x as f32, p.y as f32)
    }

    fn replace_text(&self, text: &str, range: NSRange, marked_selection: Option<NSRange>) {
        let edit = {
            let mut state = self.ivars().borrow_mut();
            let Some(s) = state.as_mut() else { return };
            let (start, len) = if range.location == NSNotFound as usize {
                s.edit.marked.unwrap_or(s.edit.selection)
            } else {
                char_range(&s.edit.text, range)
            };
            let mut chars: Vec<char> = s.edit.text.chars().collect();
            let room = s.max_chars.saturating_sub(chars.len() - len);
            // 组合拼音可比最终汉字长，只在正式上屏时应用字段长度限制。
            let limit = if marked_selection.is_some() { usize::MAX } else { room };
            let replacement: String = text.chars().filter(|c| !c.is_control()).take(limit).collect();
            let count = replacement.chars().count();
            chars.splice(start..start + len, replacement.chars());
            s.edit.text = chars.into_iter().collect();
            if let Some(selected) = marked_selection {
                let selected = char_range(&replacement, selected);
                s.edit.marked = (count > 0).then_some((start, count));
                s.edit.selection = (start + selected.0, selected.1);
            } else {
                s.edit.marked = None;
                s.edit.selection = (start + count, 0);
            }
            s.edit.clone()
        };
        crate::app::view_input(ViewInput::Text(edit), None);
    }

    fn edit_command(&self, selector: Sel) {
        if selector == sel!(paste:) {
            if let Some(text) = crate::services::clipboard::read_text() {
                self.replace_text(&text, NSRange::new(NSNotFound as usize, 0), None);
            }
            return;
        }
        if selector == sel!(cancelOperation:) && self.cancel_composition() {
            return;
        }
        if selector == sel!(selectAll:) {
            // 全选针对已提交正文；不能把组合区的相对选区混入文档选区。
            self.cancel_composition();
        }
        let mut key = None;
        let edit = {
            let mut state = self.ivars().borrow_mut();
            let Some(s) = state.as_mut() else { return };
            let mut chars: Vec<char> = s.edit.text.chars().collect();
            let (start, len) = s.edit.selection;
            let end = start + len;
            let (first, last) = s.edit.marked.map_or((0, chars.len()), |(start, len)| (start, start + len));
            if selector == sel!(insertNewline:) || selector == sel!(insertLineBreak:) {
                // 候选确认通常由输入法消费；若仍回调换行，只提交没有组合态的正文。
                if s.edit.marked.is_none() && !COMPOSING_KEY.get() { key = Some(36); }
            } else if selector == sel!(cancelOperation:) {
                key = Some(53);
            } else if selector == sel!(deleteBackward:) || selector == sel!(deleteForward:) {
                let (left, right) = if len > 0 {
                    (start, end)
                } else if selector == sel!(deleteBackward:) {
                    (start.saturating_sub(1), start)
                } else {
                    (start, (start + 1).min(chars.len()))
                };
                chars.drain(left..right);
                s.edit.selection = (left, 0);
                s.edit.marked = None;
            } else if selector == sel!(moveLeft:) || selector == sel!(moveBackward:) {
                s.edit.selection = (if len > 0 { start } else { start.saturating_sub(1).max(first) }, 0);
            } else if selector == sel!(moveRight:) || selector == sel!(moveForward:) {
                s.edit.selection = (if len > 0 { end } else { (start + 1).min(last) }, 0);
            } else if selector == sel!(moveToBeginningOfLine:) || selector == sel!(moveToBeginningOfDocument:) {
                s.edit.selection = (first, 0);
            } else if selector == sel!(moveToEndOfLine:) || selector == sel!(moveToEndOfDocument:) {
                s.edit.selection = (last, 0);
            } else if selector == sel!(selectAll:) {
                s.edit.selection = (first, last - first);
            } else if selector == sel!(moveUp:) {
                key = Some(126);
            } else if selector == sel!(moveDown:) {
                key = Some(125);
            } else {
                return;
            }
            s.edit.text = chars.into_iter().collect();
            s.edit.clone()
        };
        crate::app::view_input(ViewInput::Text(edit), None);
        if let Some(code) = key {
            crate::app::view_input(ViewInput::Key { code, text: String::new(), command: false }, None);
        }
    }

    /// 丢弃未确认文字，同时让输入法关闭候选；正文不受影响。
    fn cancel_composition(&self) -> bool {
        let edit = {
            let mut state = self.ivars().borrow_mut();
            let Some(s) = state.as_mut() else { return false };
            let Some((start, len)) = s.edit.marked.take() else { return false };
            let mut chars: Vec<char> = s.edit.text.chars().collect();
            chars.drain(start..start + len);
            s.edit.text = chars.into_iter().collect();
            s.edit.selection = (start, 0);
            s.edit.clone()
        };
        if let Some(context) = self.inputContext() {
            context.discardMarkedText();
        }
        crate::app::view_input(ViewInput::Text(edit), None);
        true
    }
}

fn input_string(string: &AnyObject) -> String {
    if let Some(s) = string.downcast_ref::<NSString>() {
        s.to_string()
    } else if let Some(s) = string.downcast_ref::<NSAttributedString>() {
        s.string().to_string()
    } else {
        String::new()
    }
}

fn utf16_range(text: &str, range: (usize, usize)) -> NSRange {
    let start = text.chars().take(range.0).map(char::len_utf16).sum();
    let len = text.chars().skip(range.0).take(range.1).map(char::len_utf16).sum();
    NSRange::new(start, len)
}

fn char_range(text: &str, range: NSRange) -> (usize, usize) {
    let mut offset = 0;
    let mut start = 0;
    let mut end: usize = 0;
    for c in text.chars() {
        offset += c.len_utf16();
        if offset <= range.location { start += 1; }
        if offset <= range.location.saturating_add(range.length) { end += 1; }
    }
    (start, end.saturating_sub(start))
}

// ---------------------------------------------------------------- the window

pub struct IslandWindow {
    pub panel: Retained<IslandPanel>,
    /// The panel's content: the island view, and under it the Glass theme's
    /// glass when it shows (mac/glass.rs).
    pub root: Retained<NSView>,
    pub view: Retained<IslandView>,
    link: Option<Retained<CADisplayLink>>,
    /// Frame-paced fallback timer when the display link isn't available.
    fallback: Option<Retained<objc2_foundation::NSTimer>>,
    pub ignoring: bool,
    /// The panel's frame in global coordinates (points, y up).
    pub frame: NSRect,
    /// 开始编辑前的应用；只在主动结束编辑且本应用仍持有焦点时交还。
    previous_application: Option<Retained<NSRunningApplication>>,
}

thread_local! {
    static WINDOW: RefCell<Option<IslandWindow>> = const { RefCell::new(None) };
}

pub fn with_window<R>(f: impl FnOnce(&mut IslandWindow) -> R) -> Option<R> {
    WINDOW.with(|w| w.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Make the panel and its view (hidden until the first frame is drawn).
pub fn create(mtm: MainThreadMarker) {
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 200.0));
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel: Retained<IslandPanel> = unsafe {
        msg_send![IslandPanel::alloc(mtm), initWithContentRect: rect, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
    };
    panel.setLevel(ISLAND_LEVEL);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    panel.setHidesOnDeactivate(false);
    panel.setMovable(false);
    panel.setAcceptsMouseMovedEvents(true);
    panel.setIgnoresMouseEvents(true);
    unsafe { panel.setReleasedWhenClosed(false) };

    let this = IslandView::alloc(mtm).set_ivars(RefCell::new(None));
    let view: Retained<IslandView> = unsafe { msg_send![super(this), initWithFrame: rect] };
    view.setWantsLayer(true);
    view.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    let root = NSView::initWithFrame(NSView::alloc(mtm), rect);
    root.setWantsLayer(true);
    root.addSubview(&view);
    panel.setContentView(Some(&root));

    // The display link drives frames at the screen's refresh rate (macOS 14+).
    let has_link: bool = unsafe { msg_send![&*view, respondsToSelector: sel!(displayLinkWithTarget:selector:)] };
    let link = has_link.then(|| {
        let target: &AnyObject = view.as_ref();
        let link = unsafe { view.displayLinkWithTarget_selector(target, sel!(step:)) };
        link.setPreferredFrameRateRange(CAFrameRateRange { minimum: 60.0, maximum: 120.0, preferred: 120.0 });
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        link.setPaused(true);
        link
    });
    if link.is_none() {
        pwarn!("no display link on this macOS; frames come from a 60 Hz timer");
    }
    let frame = panel.frame();
    WINDOW.with(|w| *w.borrow_mut() = Some(IslandWindow { panel, root, view, link, fallback: None, ignoring: true, frame, previous_application: None }));
}

/// Place the panel: `w`×`h` DIPs hanging from the top of `screen`, its left
/// edge `left` DIPs from the screen's (the app centres it on the notch, or
/// the middle, unless the island was dragged along the top edge).
pub fn place(screen: &ScreenInfo, left: f64, w: f64, h: f64) {
    with_window(|win| {
        let x = (screen.x + left).round();
        let y = screen.y + screen.h - h;
        let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
        win.panel.setFrame_display(frame, false);
        win.frame = frame;
    });
}

pub fn show() {
    with_window(|win| win.panel.orderFrontRegardless());
}

/// Take the island off the screen (a full-screen app is in front and Hide
/// is picked) or put it back. macOS carries a panel that is on every Space
/// onto a full-screen app's Space as it opens, so hiding is the app's job,
/// as on Windows.
pub fn set_shown(shown: bool) {
    with_window(|win| {
        if shown {
            win.panel.orderFrontRegardless();
        } else {
            win.panel.orderOut(None);
        }
    });
}

/// The island is on the screen (not taken off it for a full-screen app).
pub fn is_shown() -> bool {
    with_window(|win| win.panel.isVisible()).unwrap_or(false)
}

/// The screen the island sits on, in the window list's coordinates (points,
/// origin at the top left of the main display, y down).
pub fn screen_rect_top_left() -> Option<NSRect> {
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let primary_h = screens.iter().next().map(|s| s.frame().size.height)?;
    let f = with_window(|win| win.frame)?;
    let centre = NSPoint::new(f.origin.x + f.size.width / 2.0, f.origin.y + f.size.height / 2.0);
    let s = screens.iter().map(|s| s.frame()).find(|r| {
        centre.x >= r.origin.x && centre.x < r.origin.x + r.size.width && centre.y >= r.origin.y && centre.y <= r.origin.y + r.size.height
    })?;
    Some(NSRect::new(NSPoint::new(s.origin.x, primary_h - (s.origin.y + s.size.height)), s.size))
}

/// Let clicks through to the apps below (true) or take them (false).
pub fn set_ignoring(ignore: bool) {
    with_window(|win| {
        if win.ignoring != ignore {
            win.ignoring = ignore;
            win.panel.setIgnoresMouseEvents(ignore);
        }
    });
}

/// Keep frames coming (true) or rest (false).
pub fn set_frames(active: bool) {
    let need_fallback = with_window(|win| match &win.link {
        Some(l) => {
            l.setPaused(!active);
            false
        }
        None => {
            if !active {
                if let Some(t) = win.fallback.take() {
                    t.invalidate();
                }
                false
            } else {
                win.fallback.is_none()
            }
        }
    })
    .unwrap_or(false);
    if need_fallback {
        let block = block2::RcBlock::new(|_t: std::ptr::NonNull<objc2_foundation::NSTimer>| {
            let now = crate::clock::mono();
            crate::app::on_frame_tick(now, now + 1.0 / 60.0);
        });
        let timer = unsafe { objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0 / 60.0, true, &block) };
        with_window(|win| win.fallback = Some(timer));
    }
}

/// Redraw the part of the view that changed (DIPs), right now.
pub fn redraw(dirty: Rect) {
    let view = with_window(|win| win.view.clone());
    if let Some(view) = view {
        let r = NSRect::new(NSPoint::new(dirty.x as f64, dirty.y as f64), NSSize::new(dirty.w as f64, dirty.h as f64));
        view.setNeedsDisplayInRect(r);
        view.displayIfNeeded();
    }
}

/// The pointer in the view's coordinates (DIPs, y down from the panel's top).
pub fn pointer() -> Option<(f32, f32)> {
    let p = NSEvent::mouseLocation();
    with_window(|win| {
        let f = win.frame;
        ((p.x - f.origin.x) as f32, (f.origin.y + f.size.height - p.y) as f32)
    })
}

/// 只有明确进入文本编辑时才激活应用，避免画出光标却仍由其他应用收键。
pub fn take_keyboard() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    WANT_KEY.store(true, Ordering::Relaxed);
    let previous = NSWorkspace::sharedWorkspace().frontmostApplication()
        .filter(|app| app.processIdentifier() != std::process::id() as i32);
    let window = with_window(|win| {
        win.previous_application = previous;
        (win.panel.clone(), win.view.clone())
    });
    let Some((panel, view)) = window else { return };
    // 与设置窗口沿用同一激活方式；AppKit 可能回调，不能持有 WINDOW 借用。
    panel.makeKeyAndOrderFront(None);
    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    let accepted = panel.makeFirstResponder(Some(&view));
    let first = panel.firstResponder().is_some_and(|responder| responder.isEqual(Some(&view)));
    pinfo!("input focus: active={} key={} accepted={accepted} first={first}", app.isActive(), panel.isKeyWindow());
}

/// 主动结束编辑时交还键盘；系统已将焦点给了其他应用时，不再改变其选择。
pub fn release_keyboard() {
    WANT_KEY.store(false, Ordering::Relaxed);
    sync_text_input(None);
    let window = with_window(|win| {
        (win.panel.clone(), win.previous_application.take())
    });
    let Some((panel, previous)) = window else { return };
    let was_key = panel.isKeyWindow();
    let active = NSRunningApplication::currentApplication().isActive();
    if was_key {
        panel.orderOut(None);
        panel.orderFrontRegardless();
    }
    // 原生 resignKeyWindow 后 was_key 已是 false；用户点了别的应用时不抢焦点。
    if was_key && active {
        if let Some(previous) = previous {
            let restored = previous.activateWithOptions(NSApplicationActivationOptions::empty());
            pinfo!("input focus: restore={restored}");
        }
    }
}

/// 绘制完成后同步活动字段；切换字段或失焦会取消旧候选，不能带入另一条待办。
pub fn sync_text_input(session: Option<TextSession>) {
    let Some(view) = view() else { return };
    let active = session.is_some();
    let reset = {
        let mut state = view.ivars().borrow_mut();
        let changed = state.as_ref().map(|s| s.edit.id) != session.as_ref().map(|s| s.edit.id);
        if changed {
            *state = None;
        }
        changed
    };
    if reset {
        if let Some(context) = view.inputContext() {
            context.discardMarkedText();
        }
    }
    *view.ivars().borrow_mut() = session;
    if active {
        if let Some(context) = view.inputContext() {
            context.invalidateCharacterCoordinates();
        }
    }
}

/// The island view, for menus popped up over it.
pub fn view() -> Option<Retained<IslandView>> {
    with_window(|win| win.view.clone())
}

/// The panel's rectangle in screen points with the origin at the top left of
/// the main display (what `screencapture -R` takes).
pub fn frame_top_left() -> Option<(f64, f64, f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    let primary_h = NSScreen::screens(mtm).iter().next().map(|s| s.frame().size.height)?;
    with_window(|win| {
        let f = win.frame;
        (f.origin.x, primary_h - (f.origin.y + f.size.height), f.size.width, f.size.height)
    })
}

/// The top `h` DIPs of `screen`, in the same coordinates as `frame_top_left`
/// (the smoke run pictures where the island sits along the top edge).
pub fn screen_top_left(screen: &ScreenInfo, h: f64) -> Option<(f64, f64, f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    let primary_h = NSScreen::screens(mtm).iter().next().map(|s| s.frame().size.height)?;
    Some((screen.x, primary_h - (screen.y + screen.h), screen.w, h))
}

/// Start dragging `paths` out of the island, from the press in progress.
/// Returns false when there is no press to start from.
pub fn begin_drag(paths: &[String]) -> bool {
    let (Some(view), Some(event)) = (view(), LAST_MOUSE.with(|m| m.borrow().clone())) else { return false };
    let at = view.convertPoint_fromView(event.locationInWindow(), None);
    let items: Vec<Retained<NSDraggingItem>> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(p));
            let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*url));
            let icon = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(p));
            let frame = NSRect::new(NSPoint::new(at.x - 24.0 + 6.0 * i as f64, at.y - 24.0 + 6.0 * i as f64), NSSize::new(48.0, 48.0));
            unsafe { item.setDraggingFrame_contents(frame, Some(&icon)) };
            item
        })
        .collect();
    if items.is_empty() {
        return false;
    }
    crate::dragdrop::set_dragging_out(true);
    let _session = view.beginDraggingSessionWithItems_event_source(&NSArray::from_retained_slice(&items), &event, ProtocolObject::from_ref(&*view));
    true
}
