//! The Settings window on the Mac: a real window, with the system's title
//! bar buttons over the sidebar as in System Settings, that shows the
//! Settings content (`settings_window::View`). The island's Settings page
//! (its Advanced settings row) and the menu bar item's Advanced settings…
//! open it, ⌘W and its close button close it, and it is light or dark as
//! the Mac is. The window is made when it opens and let go when it closes,
//! so nothing of it stays at rest.
//!
//! Everything here runs on the main thread. The window and its view live in
//! a thread-local; the view draws and forwards input through the app
//! (`crate::app::draw_settings`, `settings_input`). While something in it
//! glides (a switch, a highlight, the content after a wheel), a display link
//! on the view draws a frame each refresh at the screen's own rate (120 Hz
//! on ProMotion, 144 Hz on a 144 Hz display); at rest it is paused.

use crate::events::{AppEvent, EventSink};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AllocAnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSClipView, NSEvent, NSEventModifierFlags, NSGraphicsContext,
    NSResponder, NSScreen, NSTextView, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow, NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_app_kit::{NSColor, NSColorPanel, NSColorSpace};
use objc2_foundation::{ns_string, NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use std::cell::RefCell;

/// The window's content size (points).
pub const WIDTH: f64 = crate::settings_window::CARD_W as f64;
pub const HEIGHT: f64 = 600.0;
/// The title bar: the window's buttons sit in it, over the sidebar.
pub const TITLEBAR: f32 = 28.0;

/// Input the view forwards to the app (points in the view, y down).
#[derive(Clone, Copy, Debug)]
pub enum Input {
    Down {
        x: f32,
        y: f32,
    },
    Up {
        x: f32,
        y: f32,
    },
    Move {
        x: f32,
        y: f32,
    },
    Leave,
    /// A mouse wheel, in notches: the content glides.
    Wheel {
        x: f32,
        y: f32,
        notches: f32,
    },
    /// A trackpad (or a smooth wheel), in points: the content follows.
    Scroll {
        x: f32,
        y: f32,
        points: f32,
    },
}

define_class!(
    /// The Settings window: tells the app when it closes.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "StudioSettingsWindow"]
    pub struct SettingsWindow;

    unsafe impl NSObjectProtocol for SettingsWindow {}

    impl SettingsWindow {
        // 菜单栏应用没有标准编辑菜单，原生表单的编辑快捷键需转交当前文本编辑器。
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            let flags = event.modifierFlags();
            let mut handled = false;
            if flags.contains(NSEventModifierFlags::Command) && !flags.intersects(NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if let Some(responder) = self.firstResponder().filter(|r| r.isKindOfClass(NSTextView::class())) {
                    let key = event.charactersIgnoringModifiers().map(|s| s.to_string().to_ascii_lowercase()).unwrap_or_default();
                    let shift = flags.contains(NSEventModifierFlags::Shift);
                    let action = match (key.as_str(), shift) {
                        ("v", false) => Some(sel!(paste:)),
                        ("a", false) => Some(sel!(selectAll:)),
                        ("c", false) => Some(sel!(copy:)),
                        ("x", false) => Some(sel!(cut:)),
                        ("z", false) => Some(sel!(undo:)),
                        ("z", true) => Some(sel!(redo:)),
                        _ => None,
                    };
                    if let Some(action) = action {
                        // 仍由系统安全文本编辑器执行，不读取密码或绕过其复制限制。
                        handled = unsafe { responder.tryToPerform_with(action, None) };
                    }
                }
            }
            handled || unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }

        #[unsafe(method(close))]
        fn close(&self) {
            unsafe {
                let _: () = msg_send![super(self), close];
            }
            crate::app::settings_closed();
        }
    }
);

define_class!(
    /// Draws the Settings content (y down, like the Windows canvas) and
    /// forwards input.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "StudioSettingsView"]
    pub struct SettingsView;

    unsafe impl NSObjectProtocol for SettingsView {}

    impl SettingsView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        // The view moves the window itself, from its empty parts.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(ctx) = NSGraphicsContext::currentContext() {
                crate::app::draw_settings(&ctx.CGContext());
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let (x, y) = self.local(event);
            if crate::app::settings_input(Input::Down { x, y }) {
                // Nothing there takes the press: the window moves with the drag.
                if let Some(win) = self.window() {
                    win.performWindowDragWithEvent(event);
                }
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let (x, y) = self.local(event);
            crate::app::settings_input(Input::Up { x, y });
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let (x, y) = self.local(event);
            crate::app::settings_input(Input::Move { x, y });
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let (x, y) = self.local(event);
            crate::app::settings_input(Input::Move { x, y });
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            crate::app::settings_input(Input::Leave);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            // A trackpad moves the content by its own points, its momentum
            // included; a wheel by notches, which glide.
            let dy = event.scrollingDeltaY() as f32;
            if dy != 0.0 {
                let (x, y) = self.local(event);
                let input = if event.hasPreciseScrollingDeltas() { Input::Scroll { x, y, points: dy } } else { Input::Wheel { x, y, notches: dy } };
                crate::app::settings_input(input);
            }
        }

        /// The display link's tick: the next frame of what glides.
        #[unsafe(method(step:))]
        fn step(&self, link: &CADisplayLink) {
            crate::app::settings_step(link.timestamp(), link.targetTimestamp());
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            // ⌘W closes it, as any Mac window.
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            let text = event.charactersIgnoringModifiers().map(|s| s.to_string()).unwrap_or_default();
            if command && text.eq_ignore_ascii_case("w") {
                if let Some(win) = self.window() {
                    win.performClose(None);
                }
            }
        }

        // Light or dark: the Mac switched, so the window draws again.
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) {
            self.setNeedsDisplay(true);
        }
    }
);

impl SettingsView {
    /// An event's position in the view (points, y down).
    fn local(&self, event: &NSEvent) -> (f32, f32) {
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        (p.x as f32, p.y as f32)
    }
}

define_class!(
    #[unsafe(super(NSClipView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "StudioMailEditorClip"]
    struct MailEditorClip;

    unsafe impl NSObjectProtocol for MailEditorClip {}

    impl MailEditorClip {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let view = HOST.with(|h| h.borrow().as_ref().map(|host| host.view.clone()));
            let Some(view) = view else { return };
            let (x, y) = view.local(event);
            let dy = event.scrollingDeltaY() as f32;
            let input = if event.hasPreciseScrollingDeltas() { Input::Scroll { x, y, points: dy } } else { Input::Wheel { x, y, notches: dy } };
            crate::app::settings_input(input);
        }
    }
);

struct Host {
    window: Retained<SettingsWindow>,
    view: Retained<SettingsView>,
    /// Frames while something glides (macOS 14's view display link).
    link: Option<Retained<CADisplayLink>>,
    mail_editor: Option<Retained<crate::mail_accounts::MailAccountView>>,
    mail_clip: Option<Retained<MailEditorClip>>,
    mail_focus_pending: bool,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    // NSColorPanel 不持有 target；只在首次打开时创建，随后复用。
    static COLOR_TARGET: RefCell<Option<Retained<LyricColorTarget>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "StudioLyricColorTarget"]
    #[ivars = EventSink]
    struct LyricColorTarget;

    unsafe impl NSObjectProtocol for LyricColorTarget {}

    impl LyricColorTarget {
        #[unsafe(method(lyricColorChanged:))]
        fn changed(&self, sender: &NSColorPanel) {
            let Some(color) = sender.color().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else { return };
            let red = (color.redComponent().clamp(0.0, 1.0) * 255.0).round() as u32;
            let green = (color.greenComponent().clamp(0.0, 1.0) * 255.0).round() as u32;
            let blue = (color.blueComponent().clamp(0.0, 1.0) * 255.0).round() as u32;
            self.ivars().send(AppEvent::LyricsColor((red << 16) | (green << 8) | blue));
        }
    }
);

/// 在 app 借用释放后打开，避免 AppKit 的同步回调重入。
pub fn choose_lyrics_color(color: poni_core::geom::Color, sink: EventSink) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let panel = NSColorPanel::sharedColorPanel(mtm);
    unsafe {
        panel.setTarget(None);
        panel.setAction(None);
    }
    panel.setColor(&NSColor::colorWithSRGBRed_green_blue_alpha(color.r as f64, color.g as f64, color.b as f64, 1.0));
    panel.setShowsAlpha(false);
    // 释放鼠标时才保存所选颜色，避免拖动色轮时反复写设置文件。
    panel.setContinuous(false);
    panel.setTitle(ns_string!("当前歌词颜色"));
    let target = COLOR_TARGET.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.get_or_insert_with(|| {
            let this = LyricColorTarget::alloc(mtm).set_ivars(sink);
            unsafe { msg_send![super(this), init] }
        })
        .clone()
    });
    unsafe {
        panel.setTarget(Some(target.as_ref()));
        panel.setAction(Some(sel!(lyricColorChanged:)));
    }
    panel.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(mtm).activate();
}

/// The window is open.
pub fn is_open() -> bool {
    HOST.with(|h| h.borrow().is_some())
}

/// Make the window (not shown yet: `show`), centred on the screen with the
/// menu bar. Returns its backing scale.
pub fn create(mtm: MainThreadMarker) -> f32 {
    if is_open() {
        return scale();
    }
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT));
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable | NSWindowStyleMask::FullSizeContentView;
    let window: Retained<SettingsWindow> = unsafe { msg_send![SettingsWindow::alloc(mtm), initWithContentRect: rect, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false] };
    window.setTitle(ns_string!("Island For Mac · 高级设置"));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setTitlebarAppearsTransparent(true);
    window.setAcceptsMouseMovedEvents(true);
    // 切换应用时保持打开，沿用普通窗口层级，让其他应用正常显示在前面。
    window.setHidesOnDeactivate(false);
    unsafe { window.setReleasedWhenClosed(false) };

    let view: Retained<SettingsView> = unsafe { msg_send![SettingsView::alloc(mtm), initWithFrame: rect] };
    // Hover: the pointer's moves over the view and its leaving it.
    let options = NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::MouseMoved | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect;
    let owner: &AnyObject = view.as_ref();
    let area = unsafe { NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), rect, options, Some(owner), None) };
    view.addTrackingArea(&area);
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&view));
    window.center();
    let has_link: bool = unsafe { msg_send![&*view, respondsToSelector: sel!(displayLinkWithTarget:selector:)] };
    let link = has_link.then(|| {
        let target: &AnyObject = view.as_ref();
        let link = unsafe { view.displayLinkWithTarget_selector(target, sel!(step:)) };
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        link.setPaused(true);
        link
    });
    let s = window.backingScaleFactor() as f32;
    HOST.with(|h| *h.borrow_mut() = Some(Host { window, view, link, mail_editor: None, mail_clip: None, mail_focus_pending: false }));
    s
}

/// Frames at the screen's own rate (on), or none (off: everything is still).
pub fn set_frames(on: bool) {
    HOST.with(|h| {
        let host = h.borrow();
        let Some(host) = host.as_ref() else { return };
        let Some(link) = host.link.as_ref() else { return };
        if on && link.isPaused() {
            // As fast as the screen goes: 120 on ProMotion, 144 on a 144 Hz display.
            let fps = host.window.screen().map_or(120, |s| s.maximumFramesPerSecond()).max(60) as f32;
            link.setPreferredFrameRateRange(CAFrameRateRange { minimum: 60.0, maximum: fps, preferred: fps });
        }
        link.setPaused(!on);
    });
}

/// The window can draw frames while things glide (it has a display link).
pub fn has_frames() -> bool {
    HOST.with(|h| h.borrow().as_ref().is_some_and(|host| host.link.is_some()))
}

/// The Mac asks for less motion (Accessibility › Display › Reduce motion).
pub fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// Bring the window to the front, and the app with it.
pub fn show() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    HOST.with(|h| {
        if let Some(host) = h.borrow().as_ref() {
            host.window.makeKeyAndOrderFront(None);
        }
    });
    NSApplication::sharedApplication(mtm).activate();
}

/// Close it, as its close button does (`SettingsWindow::close` tells the app).
pub fn close() {
    let window = HOST.with(|h| h.borrow().as_ref().map(|host| host.window.clone()));
    if let Some(w) = window {
        unsafe {
            let _: () = msg_send![&*w, close];
        }
    }
}

/// Let go of the closed window. AppKit may still hold it for the rest of
/// this turn of the run loop, so the last reference goes with its
/// autorelease pool, not here.
pub fn release() {
    clear_mail_editor();
    if let Some(host) = HOST.with(|h| h.borrow_mut().take()) {
        // The link holds the view: let go of it first.
        if let Some(link) = host.link.as_ref() {
            link.invalidate();
        }
        let _ = Retained::autorelease_ptr(host.window);
        let _ = Retained::autorelease_ptr(host.view);
    }
}

/// 把原生账户表单挂到设置列表的裁切视图中，位置由每帧布局决定。
pub(crate) fn mount_mail_editor(editor: Retained<crate::mail_accounts::MailAccountView>) {
    clear_mail_editor();
    let Some(mtm) = MainThreadMarker::new() else { return };
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
    let clip: Retained<MailEditorClip> = unsafe { msg_send![MailEditorClip::alloc(mtm), initWithFrame: frame] };
    clip.setDrawsBackground(false);
    clip.setHidden(true);
    clip.setDocumentView(Some(&editor));
    let host = HOST.with(|h| {
        let mut host = h.borrow_mut();
        let host = host.as_mut()?;
        host.mail_editor = Some(editor.clone());
        host.mail_clip = Some(clip.clone());
        host.mail_focus_pending = true;
        Some((host.window.clone(), host.view.clone()))
    });
    let Some((window, view)) = host else { return };
    // AppKit 可同步重绘并重新读取 HOST，调用前必须释放其可变借用。
    view.addSubview(&clip);
    window.makeKeyAndOrderFront(None);
    view.setNeedsDisplay(true);
}

/// 设置列表滚动后移动表单并裁切，控件不会越过标题或盖住相邻设置。
pub fn layout_mail_editor(rect: Option<poni_core::geom::Rect>, viewport: Option<poni_core::geom::Rect>) {
    let visible = rect.zip(viewport).and_then(|(rect, viewport)| {
        let x = rect.x.max(viewport.x);
        let y = rect.y.max(viewport.y);
        let right = rect.right().min(viewport.right());
        let bottom = rect.bottom().min(viewport.bottom());
        (right > x && bottom > y).then_some((rect, poni_core::geom::Rect::new(x, y, right - x, bottom - y)))
    });
    let host = HOST.with(|h| {
        let mut host = h.borrow_mut();
        let host = host.as_mut()?;
        let editor = host.mail_editor.as_ref()?.clone();
        let clip = host.mail_clip.as_ref()?.clone();
        let focus = host.mail_focus_pending;
        Some((editor, clip, focus))
    });
    let Some((editor, clip, focus)) = host else { return };
    let Some((rect, visible)) = visible else {
        if !clip.isHidden() {
            clip.setHidden(true);
        }
        return;
    };
    let frame = NSRect::new(NSPoint::new(visible.x as f64, visible.y as f64), NSSize::new(visible.w as f64, visible.h as f64));
    if clip.frame() != frame {
        clip.setFrame(frame);
    }
    let document = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(rect.w as f64, rect.h as f64));
    if editor.frame() != document {
        editor.setFrame(document);
    }
    let origin = NSPoint::new((visible.x - rect.x) as f64, (visible.y - rect.y) as f64);
    if clip.bounds().origin != origin {
        clip.scrollToPoint(origin);
    }
    if clip.isHidden() {
        clip.setHidden(false);
    }
    // 字段完整进入可视区后才聚焦，避免 field editor 自行滚动裁切视图。
    if focus && editor.focus_name(origin.y, origin.y + visible.h as f64) {
        HOST.with(|h| {
            if let Some(host) = h.borrow_mut().as_mut() {
                host.mail_focus_pending = false;
            }
        });
    }
}

/// 收起表单、离开邮件设置或关闭窗口时擦除尚未保存的密码。
pub fn clear_mail_editor() {
    let host = HOST.with(|h| {
        let mut host = h.borrow_mut();
        let host = host.as_mut()?;
        let editor = host.mail_editor.take()?;
        let clip = host.mail_clip.take();
        host.mail_focus_pending = false;
        Some((host.window.clone(), host.view.clone(), editor, clip))
    });
    let Some((window, view, editor, clip)) = host else { return };
    window.makeFirstResponder(Some(&view));
    editor.clear_secrets();
    if let Some(clip) = clip {
        clip.setDocumentView(None);
        clip.removeFromSuperview();
    }
    view.setNeedsDisplay(true);
}

/// Draw `r` of the window again (DIPs), on the next display pass: a frame
/// of a glide redraws only what moves.
pub fn redraw_in(r: poni_core::geom::Rect) {
    HOST.with(|h| {
        if let Some(host) = h.borrow().as_ref() {
            let nr = NSRect::new(NSPoint::new(r.x as f64, r.y as f64), NSSize::new(r.w as f64, r.h as f64));
            host.view.setNeedsDisplayInRect(nr);
        }
    });
}

/// Draw the window again (on the next display pass).
pub fn redraw() {
    HOST.with(|h| {
        if let Some(host) = h.borrow().as_ref() {
            host.view.setNeedsDisplay(true);
        }
    });
}

/// The window's backing scale (2 on a Retina screen).
pub fn scale() -> f32 {
    HOST.with(|h| h.borrow().as_ref().map(|host| host.window.backingScaleFactor() as f32)).unwrap_or(2.0)
}

/// Light or dark, as the window shows now (the Mac's mode, or the smoke
/// test's pick).
pub fn is_light() -> bool {
    HOST.with(|h| {
        let host = h.borrow();
        let Some(host) = host.as_ref() else { return false };
        let names = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe { NSAppearanceNameDarkAqua }]);
        let best = host.view.effectiveAppearance().bestMatchFromAppearancesWithNames(&names);
        best.is_none_or(|n| !n.isEqualToString(unsafe { NSAppearanceNameDarkAqua }))
    })
}

/// The smoke test's light (or dark) window, whatever mode the Mac is in.
pub fn force_appearance(light: bool) {
    HOST.with(|h| {
        if let Some(host) = h.borrow().as_ref() {
            let name = if light { unsafe { NSAppearanceNameAqua } } else { unsafe { NSAppearanceNameDarkAqua } };
            host.window.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
            host.view.setNeedsDisplay(true);
        }
    });
}

/// Where the window is, in the window list's coordinates (points, origin at
/// the top left of the main display, y down): the smoke test's picture of it.
pub fn frame_top_left() -> Option<(f64, f64, f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    let primary_h = NSScreen::screens(mtm).iter().next().map(|s| s.frame().size.height)?;
    HOST.with(|h| {
        h.borrow().as_ref().map(|host| {
            let f = host.window.frame();
            (f.origin.x, primary_h - (f.origin.y + f.size.height), f.size.width, f.size.height)
        })
    })
}
