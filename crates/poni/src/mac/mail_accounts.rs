//! 高级设置内的原生邮箱表单；密码只经过安全输入框和钥匙串。

use crate::events::{AppEvent, EventSink};
use crate::mail::{MailAction, MailCredentialInput};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSButton, NSColor, NSFont,
    NSPopUpButton, NSResponder, NSSecureTextField, NSTextField, NSView,
};
use objc2_foundation::{
    NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSUUID,
};
use poni_core::mail::{MailAccount, MailProvider};
use std::cell::RefCell;

pub const FORM_H: f32 = 400.0;
const FORM_W: f64 = 564.0;
const LABEL_X: f64 = 16.0;
const FIELD_X: f64 = 130.0;
const FIELD_W: f64 = FORM_W - FIELD_X - LABEL_X;

pub(crate) struct Editor {
    previous: Option<MailAccount>,
    account: MailAccount,
    sink: EventSink,
    provider: Retained<NSPopUpButton>,
    name: Retained<NSTextField>,
    address: Retained<NSTextField>,
    host: Retained<NSTextField>,
    host_label: Retained<NSTextField>,
    port: Retained<NSTextField>,
    port_label: Retained<NSTextField>,
    secret: Retained<NSSecureTextField>,
    secret_label: Retained<NSTextField>,
    detail: Retained<NSTextField>,
    error: Retained<NSTextField>,
    save: Retained<NSButton>,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "StudioMailAccountView"]
    #[ivars = RefCell<Option<Editor>>]
    pub(crate) struct MailAccountView;

    unsafe impl NSObjectProtocol for MailAccountView {}

    impl MailAccountView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(providerChanged:))]
        fn provider_changed(&self, _sender: &NSPopUpButton) {
            let mut slot = self.ivars().borrow_mut();
            let Some(editor) = slot.as_mut() else { return };
            let provider = MailProvider::ALL[editor.provider.indexOfSelectedItem() as usize];
            if editor.name.stringValue().to_string() == editor.account.provider.title() {
                editor.name.setStringValue(&NSString::from_str(provider.title()));
            }
            editor.account.provider = provider;
            editor.host.setStringValue(&NSString::from_str(provider.host()));
            editor.port.setStringValue(&NSString::from_str("993"));
            editor.secret.setStringValue(&NSString::from_str(""));
            editor.update_provider();
        }

        #[unsafe(method(saveAccount:))]
        fn save_account(&self, _sender: &NSButton) {
            if let Some(window) = self.window() {
                window.makeFirstResponder(None);
            }
            let result = {
                let mut slot = self.ivars().borrow_mut();
                let Some(editor) = slot.as_mut() else { return };
                match editor.save_input() {
                    Ok((account, credentials)) => Some((editor.sink.clone(), account, credentials)),
                    Err(error) => {
                        editor.error.setStringValue(&NSString::from_str(&error));
                        None
                    }
                }
            };
            if let Some((sink, account, credentials)) = result {
                crate::settings_host::clear_mail_editor();
                sink.send(AppEvent::MailAction(MailAction::CloseEditor));
                sink.send(AppEvent::MailAction(MailAction::Save { account, credentials }));
            }
        }

        #[unsafe(method(cancelAccount:))]
        fn cancel_account(&self, _sender: &NSButton) {
            let sink = self.ivars().borrow().as_ref().map(|editor| editor.sink.clone());
            crate::settings_host::clear_mail_editor();
            if let Some(sink) = sink {
                sink.send(AppEvent::MailAction(MailAction::CloseEditor));
            }
        }
    }
);

impl MailAccountView {
    pub fn clear_secrets(&self) {
        if let Some(editor) = self.ivars().borrow().as_ref() {
            editor.secret.setStringValue(&NSString::from_str(""));
        }
    }

    pub fn focus_name(&self, visible_top: f64, visible_bottom: f64) -> bool {
        if let Some(editor) = self.ivars().borrow().as_ref() {
            let frame = editor.name.frame();
            if frame.origin.y < visible_top || frame.origin.y + frame.size.height > visible_bottom {
                return false;
            }
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(&editor.name));
                return true;
            }
        }
        false
    }
}

impl Editor {
    fn update_provider(&self) {
        let google = self.account.provider == MailProvider::Gmail;
        self.host_label
            .setStringValue(&NSString::from_str(if google {
                "客户端 ID"
            } else {
                "IMAP 服务器"
            }));
        self.host
            .setPlaceholderString(Some(&NSString::from_str(if google {
                "Google 桌面应用客户端 ID"
            } else {
                "例如：imaps.aruba.it"
            })));
        self.port.setHidden(google);
        self.port_label.setHidden(google);
        self.secret_label
            .setStringValue(&NSString::from_str(if google {
                "客户端密钥"
            } else {
                "密码／授权码"
            }));
        self.save.setTitle(&NSString::from_str("保存并授权"));
        self.detail.setStringValue(&NSString::from_str(instructions(
            self.account.provider,
            self.previous.is_none(),
        )));
        self.error.setStringValue(&NSString::from_str(""));
    }

    fn save_input(&mut self) -> Result<(MailAccount, MailCredentialInput), String> {
        let mut edited = self.account.clone();
        edited.name = self.name.stringValue().to_string().trim().into();
        edited.address = self.address.stringValue().to_string().trim().into();
        let google = edited.provider == MailProvider::Gmail;
        if google {
            edited.host.clear();
            edited.port = 993;
        } else {
            edited.host = self.host.stringValue().to_string().trim().into();
            edited.port = self
                .port
                .stringValue()
                .to_string()
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or("端口需要是 1–65535 之间的整数")?;
        }
        edited.validate()?;
        let changed = self.previous.as_ref().is_some_and(|a| {
            a.provider != edited.provider
                || a.address != edited.address
                || a.host != edited.host
                || a.port != edited.port
        });
        let secret = self.secret.stringValue().to_string();
        let credentials = if google {
            let client_id = self.host.stringValue().to_string().trim().to_string();
            let client_secret = secret.trim().to_string();
            if client_id.is_empty()
                && client_secret.is_empty()
                && self.previous.is_some()
                && !changed
            {
                MailCredentialInput::Keep
            } else if client_id.is_empty() || client_secret.is_empty() {
                return Err(
                    "请填写 Google 桌面应用客户端 ID 和密钥；编辑时两项都留空可保留授权".into(),
                );
            } else if !client_id.ends_with(".apps.googleusercontent.com")
                || client_id.chars().any(char::is_whitespace)
            {
                return Err(
                    "客户端 ID 应以 .apps.googleusercontent.com 结尾，请使用桌面应用 OAuth 配置"
                        .into(),
                );
            } else {
                MailCredentialInput::Google {
                    client_id,
                    client_secret,
                }
            }
        } else if secret.is_empty() {
            if self.previous.is_none() || changed {
                return Err("请填写密码或授权码；更换邮箱地址或服务器时需要重新输入".into());
            }
            MailCredentialInput::Keep
        } else {
            MailCredentialInput::Password(secret)
        };
        Ok((edited, credentials))
    }
}

/// 在高级设置的邮件内容区编辑，不启动模态窗口或嵌套事件循环。
pub fn show(mtm: MainThreadMarker, previous: Option<MailAccount>, sink: EventSink) {
    let mut account = previous.clone().unwrap_or_default();
    if previous.is_none() {
        account.id = NSUUID::UUID().UUIDString().to_string().to_ascii_lowercase();
        account.name = account.provider.title().into();
    }
    account.revision = previous
        .as_ref()
        .map_or(1, |a| a.revision.saturating_add(1));
    let this = MailAccountView::alloc(mtm).set_ivars(RefCell::new(None));
    let form: Retained<MailAccountView> =
        unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, FORM_W, FORM_H as f64)] };
    // 表单保持透明，沿用设置列表 fill_group 的背景、描边和圆角。
    let heading = label(
        mtm,
        &form,
        if previous.is_none() {
            "添加邮箱账户"
        } else {
            "编辑邮箱账户"
        },
        rect(LABEL_X, 12.0, FORM_W - 2.0 * LABEL_X, 22.0),
    );
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    heading.setTextColor(Some(&NSColor::labelColor()));
    let detail = label(
        mtm,
        &form,
        "",
        rect(LABEL_X, 42.0, FORM_W - 2.0 * LABEL_X, 42.0),
    );
    detail.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label(mtm, &form, "邮箱类型", rect(LABEL_X, 98.0, 104.0, 22.0));
    let provider = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(FIELD_X, 94.0, FIELD_W, 28.0),
        false,
    );
    let titles: Vec<_> = MailProvider::ALL
        .iter()
        .map(|p| NSString::from_str(p.title()))
        .collect();
    provider.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
    provider.selectItemAtIndex(
        MailProvider::ALL
            .iter()
            .position(|p| *p == account.provider)
            .unwrap_or(0) as isize,
    );
    form.addSubview(&provider);
    let (_, name) = field(
        mtm,
        &form,
        "显示名称",
        &account.name,
        "例如：工作邮箱",
        132.0,
    );
    let (_, address) = field(
        mtm,
        &form,
        "邮箱地址",
        &account.address,
        "完整邮箱地址",
        168.0,
    );
    let (host_label, host) = field(
        mtm,
        &form,
        "IMAP 服务器",
        if account.provider == MailProvider::Gmail {
            ""
        } else {
            &account.host
        },
        "",
        204.0,
    );
    let (port_label, port) = field(mtm, &form, "端口", &account.port.to_string(), "993", 240.0);
    let secret_label = label(
        mtm,
        &form,
        "密码／授权码",
        rect(LABEL_X, 280.0, 104.0, 22.0),
    );
    let secret = NSSecureTextField::initWithFrame(
        NSSecureTextField::alloc(mtm),
        rect(FIELD_X, 276.0, FIELD_W, 28.0),
    );
    secret.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    secret.setPlaceholderString(Some(&NSString::from_str(if previous.is_none() {
        "仅保存到 macOS 钥匙串"
    } else {
        "留空保留现有凭据"
    })));
    form.addSubview(&secret);
    let error = label(
        mtm,
        &form,
        "",
        rect(LABEL_X, 312.0, FORM_W - 2.0 * LABEL_X, 36.0),
    );
    error.setTextColor(Some(&NSColor::systemRedColor()));
    let cancel = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("取消"),
            Some(form.as_ref()),
            Some(sel!(cancelAccount:)),
            mtm,
        )
    };
    cancel.setFrame(rect(FORM_W - 244.0, 354.0, 96.0, 30.0));
    form.addSubview(&cancel);
    let save = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("保存并授权"),
            Some(form.as_ref()),
            Some(sel!(saveAccount:)),
            mtm,
        )
    };
    save.setFrame(rect(FORM_W - 140.0, 354.0, 112.0, 30.0));
    form.addSubview(&save);
    unsafe {
        provider.setTarget(Some(form.as_ref()));
        provider.setAction(Some(sel!(providerChanged:)));
        provider.setNextKeyView(Some(&name));
        name.setNextKeyView(Some(&address));
        address.setNextKeyView(Some(&host));
        host.setNextKeyView(Some(&port));
        port.setNextKeyView(Some(&secret));
        secret.setNextKeyView(Some(&save));
        save.setNextKeyView(Some(&cancel));
        cancel.setNextKeyView(Some(&provider));
    }
    let editor = Editor {
        previous,
        account,
        sink,
        provider,
        name,
        address,
        host,
        host_label,
        port,
        port_label,
        secret,
        secret_label,
        detail,
        error,
        save,
    };
    editor.update_provider();
    *form.ivars().borrow_mut() = Some(editor);
    crate::settings_host::mount_mail_editor(form);
}

/// 仅移除Island中的账户、凭据与缓存，不修改邮箱服务器中的邮件。
pub fn confirm_remove(mtm: MainThreadMarker, account: MailAccount, sink: EventSink) {
    NSApplication::sharedApplication(mtm).activate();
    let alert = account_alert(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str("移除邮箱账户？"));
    alert.setInformativeText(&NSString::from_str(&format!(
        "{}\n{}\n\n移除此应用保存的账户、凭据和邮件缓存。邮箱服务器中的邮件会保留。",
        account.name, account.address,
    )));
    alert
        .addButtonWithTitle(&NSString::from_str("取消"))
        .setKeyEquivalent(&NSString::from_str("\u{1b}"));
    alert.addButtonWithTitle(&NSString::from_str("移除账户"));
    let response = alert.runModal();
    alert.window().orderOut(None);
    if response == NSAlertFirstButtonReturn + 1 {
        sink.send(AppEvent::MailAction(MailAction::Remove(account.id)));
    }
}

fn instructions(provider: MailProvider, is_new: bool) -> &'static str {
    match provider {
        MailProvider::Qq => "请在 QQ 邮箱中开启 IMAP，并填写邮箱授权码。连接使用 SSL/TLS。",
        MailProvider::Icloud => "请填写完整 iCloud 邮箱地址和 Apple 应用专用密码。连接使用 SSL/TLS。",
        MailProvider::Aruba => "默认连接 Aruba 普通公司邮箱。PEC 邮箱请将服务器改为 imaps.pec.aruba.it，开启两步验证时使用邮件客户端专用密码。",
        MailProvider::Custom => "填写服务商提供的 IMAP 服务器、SSL/TLS 端口和邮箱密码或授权码。",
        MailProvider::Gmail if is_new => "需要你在 Google Cloud 中创建的桌面应用 OAuth 客户端，并启用 Gmail API。保存后将在浏览器申请 Gmail 只读授权。客户端 ID 和密钥可从下载的配置中复制。",
        MailProvider::Gmail => "两项都留空可保留现有 Google 授权；重新授权时，请填写桌面应用客户端 ID 和密钥。更换邮箱地址需要重新授权，使用 Gmail API 只读权限。",
    }
}

fn label(mtm: MainThreadMarker, form: &NSView, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    label.setFrame(frame);
    form.addSubview(&label);
    label
}

fn field(
    mtm: MainThreadMarker,
    form: &NSView,
    title: &str,
    value: &str,
    placeholder: &str,
    y: f64,
) -> (Retained<NSTextField>, Retained<NSTextField>) {
    let caption = label(mtm, form, title, rect(LABEL_X, y + 4.0, 104.0, 22.0));
    let field = NSTextField::textFieldWithString(&NSString::from_str(value), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    field.setFrame(rect(FIELD_X, y, FIELD_W, 28.0));
    field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
    form.addSubview(&field);
    (caption, field)
}

fn account_alert(mtm: MainThreadMarker) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    if let Some(level) = crate::window::with_window(|window| window.panel.level() + 1) {
        alert.window().setLevel(level);
    }
    alert
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
