//! 按需打开的原生提示音选择器；选择、试听与保存分别由显式操作触发。

use crate::events::{AppEvent, EventSink};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSColor, NSEvent, NSEventModifierFlags, NSFont,
    NSMenu, NSMenuItem, NSPopUpButton, NSResponder, NSTextField, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use poni_core::alert_sounds::{self, SoundModule, ALERT_TONES};
use std::cell::RefCell;

struct Picker {
    module: SoundModule,
    sink: EventSink,
    choices: Retained<NSPopUpButton>,
    detail: Retained<NSTextField>,
}

struct Host {
    window: Retained<SoundPickerWindow>,
    view: Retained<SoundPickerView>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "IslandSoundPickerWindow"]
    struct SoundPickerWindow;

    unsafe impl NSObjectProtocol for SoundPickerWindow {}

    impl SoundPickerWindow {
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            let flags = event.modifierFlags();
            let key = event.charactersIgnoringModifiers().map(|value| value.to_string());
            let handled = flags.contains(NSEventModifierFlags::Command)
                && !flags.intersects(NSEventModifierFlags::Control | NSEventModifierFlags::Option)
                && key.as_deref() == Some("w");
            if handled {
                self.close();
            }
            handled || unsafe { msg_send![super(self), performKeyEquivalent: event] }
        }

        #[unsafe(method(close))]
        fn window_close(&self) {
            unsafe { let _: () = msg_send![super(self), close]; }
            let closed = self as *const Self as usize;
            // 关闭回调返回后再释放窗口；旧窗口的回调不会清掉随后新开的选择器。
            dispatch2::DispatchQueue::main().exec_async(move || {
                HOST.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    if slot.as_ref().is_some_and(|host| &*host.window as *const SoundPickerWindow as usize == closed) {
                        slot.take();
                    }
                });
            });
        }
    }
);

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "IslandSoundPickerView"]
    #[ivars = RefCell<Option<Picker>>]
    struct SoundPickerView;

    unsafe impl NSObjectProtocol for SoundPickerView {}

    impl SoundPickerView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(toneChanged:))]
        fn tone_changed(&self, _sender: &NSPopUpButton) {
            if let Some(picker) = self.ivars().borrow().as_ref() {
                picker.show_default();
            }
        }

        #[unsafe(method(previewTone:))]
        fn preview_tone(&self, _sender: &NSButton) {
            let request = self.ivars().borrow().as_ref().and_then(|picker| {
                picker.selected_id().map(|id| (picker.sink.clone(), id.to_string()))
            });
            if let Some((sink, id)) = request {
                sink.send(AppEvent::AlertTonePreview(id));
            }
        }

        #[unsafe(method(restoreDefaultTone:))]
        fn restore_default_tone(&self, _sender: &NSButton) {
            if let Some(picker) = self.ivars().borrow().as_ref() {
                if let Some(index) = ALERT_TONES.iter().position(|tone| tone.id == picker.module.default_id()) {
                    picker.choices.selectItemWithTag(index as isize);
                }
                picker.show_default();
            }
        }

        #[unsafe(method(useTone:))]
        fn use_tone(&self, _sender: &NSButton) {
            let request = self.ivars().borrow().as_ref().and_then(|picker| {
                picker.selected_id().map(|id| (picker.sink.clone(), picker.module, id.to_string()))
            });
            if let Some((sink, module, id)) = request {
                if let Some(window) = self.window() {
                    window.close();
                }
                sink.send(AppEvent::AlertToneSelected { module, id });
            }
        }

        #[unsafe(method(cancelTone:))]
        fn cancel_tone(&self, _sender: &NSButton) {
            if let Some(window) = self.window() {
                window.close();
            }
        }
    }
);

impl Picker {
    fn selected_id(&self) -> Option<&'static str> {
        let item = self.choices.selectedItem()?;
        let index = usize::try_from(item.tag()).ok()?;
        ALERT_TONES.get(index).map(|tone| tone.id)
    }

    fn show_default(&self) {
        let name = alert_sounds::tone(self.module.default_id())
            .map_or("默认提示音", |tone| tone.name);
        self.detail.setStringValue(&NSString::from_str(&format!(
            "默认：{name}。点击“使用此音效”后生效。"
        )));
        self.detail
            .setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
}

/// 由主 App 释放借用后调用；窗口仅在打开期间持有原生控件和事件接收端。
pub fn open(module: SoundModule, current: &str, sink: EventSink) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    close();
    let bounds = rect(0.0, 0.0, 480.0, 230.0);
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
    let window: Retained<SoundPickerWindow> = unsafe {
        msg_send![SoundPickerWindow::alloc(mtm), initWithContentRect: bounds, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
    };
    window.setTitle(&NSString::from_str(&format!("{} · 提示音", module.title())));
    window.setHidesOnDeactivate(false);
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    let this = SoundPickerView::alloc(mtm).set_ivars(RefCell::new(None));
    let view: Retained<SoundPickerView> = unsafe { msg_send![super(this), initWithFrame: bounds] };

    let heading = label(mtm, &view, "选择提示音", rect(24.0, 20.0, 432.0, 24.0));
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(16.0)));
    heading.setTextColor(Some(&NSColor::labelColor()));
    label(
        mtm,
        &view,
        "新款、常用及三全音、敲打电报随应用提供。试听音量随系统音量。",
        rect(24.0, 53.0, 432.0, 34.0),
    );

    let choices = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(20.0, 97.0, 338.0, 28.0),
        false,
    );
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let mut group = "";
    for (index, tone) in ALERT_TONES.iter().enumerate() {
        if tone.group != group {
            group = tone.group;
            let heading = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(group),
                    None,
                    &NSString::from_str(""),
                )
            };
            heading.setEnabled(false);
            heading.setTag(-1);
            menu.addItem(&heading);
        }
        let option = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(tone.name),
                None,
                &NSString::from_str(""),
            )
        };
        option.setTag(index as isize);
        option.setIndentationLevel(1);
        menu.addItem(&option);
    }
    choices.setMenu(Some(&menu));
    let selected = ALERT_TONES
        .iter()
        .position(|tone| tone.id == current)
        .or_else(|| {
            ALERT_TONES
                .iter()
                .position(|tone| tone.id == module.default_id())
        });
    if let Some(selected) = selected {
        choices.selectItemWithTag(selected as isize);
    }
    unsafe {
        choices.setTarget(Some(view.as_ref()));
        choices.setAction(Some(sel!(toneChanged:)));
    }
    view.addSubview(&choices);

    let preview = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("试听"),
            Some(view.as_ref()),
            Some(sel!(previewTone:)),
            mtm,
        )
    };
    preview.setFrame(rect(366.0, 96.0, 90.0, 30.0));
    view.addSubview(&preview);
    let detail = label(mtm, &view, "", rect(24.0, 138.0, 432.0, 34.0));

    let restore = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("恢复默认"),
            Some(view.as_ref()),
            Some(sel!(restoreDefaultTone:)),
            mtm,
        )
    };
    restore.setFrame(rect(20.0, 184.0, 108.0, 30.0));
    view.addSubview(&restore);
    let cancel = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("取消"),
            Some(view.as_ref()),
            Some(sel!(cancelTone:)),
            mtm,
        )
    };
    cancel.setFrame(rect(254.0, 184.0, 88.0, 30.0));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    view.addSubview(&cancel);
    let save = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("使用此音效"),
            Some(view.as_ref()),
            Some(sel!(useTone:)),
            mtm,
        )
    };
    save.setFrame(rect(348.0, 184.0, 112.0, 30.0));
    save.setKeyEquivalent(&NSString::from_str("\r"));
    view.addSubview(&save);
    unsafe {
        choices.setNextKeyView(Some(&preview));
        preview.setNextKeyView(Some(&restore));
        restore.setNextKeyView(Some(&cancel));
        cancel.setNextKeyView(Some(&save));
        save.setNextKeyView(Some(&choices));
    }

    *view.ivars().borrow_mut() = Some(Picker {
        module,
        sink,
        choices: choices.clone(),
        detail,
    });
    if let Some(picker) = view.ivars().borrow().as_ref() {
        picker.show_default();
    }
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&choices));
    window.center();
    HOST.with(|slot| {
        *slot.borrow_mut() = Some(Host {
            window: window.clone(),
            view,
        })
    });
    window.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(mtm).activate();
}

/// 锁屏、退出或切换模块时关闭，不提交尚未确认的选择。
pub fn close() {
    let window = HOST.with(|slot| slot.borrow().as_ref().map(|host| host.window.clone()));
    if let Some(window) = window {
        window.close();
    }
}

/// 试听失败直接显示在选择器内，不把失败当作已保存或已播放。
pub fn show_error(message: &str) {
    let view = HOST.with(|slot| slot.borrow().as_ref().map(|host| host.view.clone()));
    if let Some(view) = view {
        if let Some(picker) = view.ivars().borrow().as_ref() {
            picker.detail.setStringValue(&NSString::from_str(message));
            picker.detail.setTextColor(Some(&NSColor::systemRedColor()));
        }
    }
}

fn label(mtm: MainThreadMarker, view: &NSView, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    view.addSubview(&label);
    label
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}
