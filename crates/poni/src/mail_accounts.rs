//! Windows 邮箱账户表单。真实 EDIT 控件负责密码遮挡和输入法候选定位。
//! 窗口只在主动编辑时存在，凭据只通过内存交给后台服务。

use crate::events::{AppEvent, EventSink};
use crate::mail::{MailAction, MailCredentialInput};
use poni_core::mail::{MailAccount, MailProvider};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, SetBkColor, SetTextColor, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, FF_DONTCARE, HBRUSH, HDC, HFONT,
    OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{SetWindowTheme, EM_SETLIMITTEXT};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;

// Windows 使用独立表单，不为 Cocoa 的内嵌编辑区预留空白。
pub const FORM_H: f32 = 0.0;
const CLASS: PCWSTR = w!("Island.MailAccount");
const ID_PROVIDER: usize = 10;
const ID_SAVE: usize = 1;
const ID_CANCEL: usize = 2;

struct Editor {
    previous: Option<MailAccount>,
    account: MailAccount,
    provider: HWND,
    name: HWND,
    address: HWND,
    host: HWND,
    host_label: HWND,
    port: HWND,
    port_label: HWND,
    secret: HWND,
    secret_label: HWND,
    detail: HWND,
    error: HWND,
    save: HWND,
    font: HFONT,
    brush: HBRUSH,
    light: bool,
    result: Option<(MailAccount, MailCredentialInput)>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.font);
            let _ = DeleteObject(self.brush);
        }
    }
}

impl Editor {
    fn update_provider(&self) {
        let google = self.account.provider == MailProvider::Gmail;
        unsafe {
            set_text(
                self.host_label,
                if google {
                    "客户端 ID"
                } else {
                    "IMAP 服务器"
                },
            );
            set_text(
                self.secret_label,
                if google {
                    "客户端密钥"
                } else {
                    "密码／授权码"
                },
            );
            set_text(
                self.save,
                if google {
                    "保存并授权"
                } else {
                    "保存账户"
                },
            );
            let show = if google { SW_HIDE } else { SW_SHOW };
            let _ = ShowWindow(self.port, show);
            let _ = ShowWindow(self.port_label, show);
            set_text(self.detail, match self.account.provider {
                MailProvider::Qq => "在 QQ 邮箱开启 IMAP，填写邮箱授权码。使用 SSL/TLS 连接。",
                MailProvider::Icloud => "填写完整 iCloud 邮箱地址和 Apple 应用专用密码。使用 SSL/TLS 连接。",
                MailProvider::Aruba => "普通公司邮箱默认 imaps.aruba.it。PEC 请改为 imaps.pec.aruba.it；开启两步验证时使用邮件客户端专用密码。",
                MailProvider::Custom => "填写服务商提供的 IMAP 服务器、SSL/TLS 端口和密码或授权码。",
                MailProvider::Gmail => "使用 Google Cloud 中启用 Gmail API 的桌面应用 OAuth 客户端。保存后打开浏览器申请只读授权；编辑时 ID 和密钥都留空可保留授权。",
            });
            set_text(self.error, "");
        }
    }

    fn save_input(&self) -> Result<(MailAccount, MailCredentialInput), String> {
        let mut account = self.account.clone();
        account.name = text(self.name).trim().into();
        account.address = text(self.address).trim().into();
        let google = account.provider == MailProvider::Gmail;
        if google {
            account.host.clear();
            account.port = 993;
        } else {
            account.host = text(self.host).trim().into();
            account.port = text(self.port)
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or("端口需要是 1–65535 之间的整数")?;
        }
        account.validate()?;
        let changed = self.previous.as_ref().is_some_and(|old| {
            old.provider != account.provider
                || old.address != account.address
                || old.host != account.host
                || old.port != account.port
        });
        let secret = text(self.secret);
        let credentials = if google {
            let client_id = text(self.host).trim().to_string();
            let client_secret = secret.trim().to_string();
            if client_id.is_empty()
                && client_secret.is_empty()
                && self.previous.is_some()
                && !changed
            {
                MailCredentialInput::Keep
            } else if client_id.is_empty() || client_secret.is_empty() {
                return Err("请填写 Google 客户端 ID 和密钥；编辑时两项都留空可保留授权".into());
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
                return Err("请填写密码或授权码；更换邮箱地址或服务器需要重新输入".into());
            }
            MailCredentialInput::Keep
        } else {
            MailCredentialInput::Password(secret)
        };
        Ok((account, credentials))
    }
}

/// 在 App 借用结束后运行，表单消息循环不会丢失主窗口的服务事件。
pub fn show(owner: HWND, previous: Option<MailAccount>, sink: EventSink, font_family: &str) {
    let result = unsafe { open_editor(owner, previous, font_family) };
    sink.send(AppEvent::MailAction(MailAction::CloseEditor));
    match result {
        Ok(Some((account, credentials))) => sink.send(AppEvent::MailAction(MailAction::Save {
            account,
            credentials,
        })),
        Ok(None) => {}
        Err(_) => {
            // 不打印凭据；窗口创建失败在设置页明确报告。
            crate::pwarn!("mail: unable to open account editor");
            sink.send(AppEvent::MailEditorError(
                "无法打开邮箱账户表单，请重新尝试".into(),
            ));
        }
    }
}

unsafe fn open_editor(
    owner: HWND,
    previous: Option<MailAccount>,
    font_family: &str,
) -> windows::core::Result<Option<(MailAccount, MailCredentialInput)>> {
    let instance = GetModuleHandleW(None)?;
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hIcon: LoadIconW(instance, PCWSTR(1 as _)).unwrap_or_default(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    // 同一进程重复打开时窗口类已注册，可以继续创建。
    let _ = RegisterClassExW(&class);
    let mut account = previous.clone().unwrap_or_default();
    if previous.is_none() {
        account.id = format!("{:?}", CoCreateGuid()?)
            .trim_matches(['{', '}'])
            .to_ascii_lowercase();
        account.name = account.provider.title().into();
    }
    account.revision = previous
        .as_ref()
        .map_or(1, |old| old.revision.saturating_add(1));
    let dpi = GetDpiForWindow(owner).max(96);
    let scale = dpi as f32 / 96.0;
    let px = |value: i32| (value as f32 * scale).round() as i32;
    let mut bounds = RECT {
        left: 0,
        top: 0,
        right: px(596),
        bottom: px(470),
    };
    AdjustWindowRectEx(
        &mut bounds,
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
        false,
        WINDOW_EX_STYLE(0),
    )?;
    let mut parent = RECT::default();
    GetWindowRect(owner, &mut parent)?;
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    let hwnd = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        CLASS,
        w!("邮箱账户 · Island"),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
        parent.left + (parent.right - parent.left - width) / 2,
        (parent.top + (parent.bottom - parent.top - height) / 2).max(0),
        width,
        height,
        owner,
        None,
        instance,
        None,
    )?;
    let light = crate::services::apps_use_light_theme();
    let dark = i32::from(!light);
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &dark as *const _ as _,
        std::mem::size_of::<i32>() as u32,
    );
    let font_family = wide(if font_family.is_empty() {
        "Segoe UI"
    } else {
        font_family
    });
    let font = CreateFontW(
        -px(14),
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(font_family.as_ptr()),
    );
    let brush = CreateSolidBrush(if light {
        COLORREF(0x00faf9f7)
    } else {
        COLORREF(0x00272522)
    });
    let create = |class: PCWSTR,
                  value: &str,
                  style: WINDOW_STYLE,
                  x,
                  y,
                  w,
                  h,
                  id: usize|
     -> windows::core::Result<HWND> {
        let value = wide(value);
        let control = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(value.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            px(x),
            px(y),
            px(w),
            px(h),
            hwnd,
            HMENU(id as _),
            instance,
            None,
        )?;
        SendMessageW(control, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        if !light {
            let _ = SetWindowTheme(control, w!("DarkMode_Explorer"), None);
        }
        Ok(control)
    };
    let controls = (|| -> windows::core::Result<Editor> {
        create(
            w!("STATIC"),
            if previous.is_none() {
                "添加邮箱账户"
            } else {
                "编辑邮箱账户"
            },
            WINDOW_STYLE(0),
            24,
            18,
            548,
            24,
            0,
        )?;
        let detail = create(w!("STATIC"), "", WINDOW_STYLE(0), 24, 50, 548, 58, 0)?;
        create(
            w!("STATIC"),
            "邮箱类型",
            WINDOW_STYLE(0),
            24,
            123,
            106,
            24,
            0,
        )?;
        let provider = create(
            w!("COMBOBOX"),
            "",
            WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0),
            146,
            116,
            426,
            200,
            ID_PROVIDER,
        )?;
        for provider_kind in MailProvider::ALL {
            let value = wide(provider_kind.title());
            SendMessageW(
                provider,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(value.as_ptr() as isize),
            );
        }
        SendMessageW(
            provider,
            CB_SETCURSEL,
            WPARAM(
                MailProvider::ALL
                    .iter()
                    .position(|p| *p == account.provider)
                    .unwrap_or(0),
            ),
            LPARAM(0),
        );
        let field =
            |title: &str, value: &str, y, secret: bool| -> windows::core::Result<(HWND, HWND)> {
                let label = create(w!("STATIC"), title, WINDOW_STYLE(0), 24, y + 6, 112, 24, 0)?;
                let style = WS_TABSTOP
                    | WS_BORDER
                    | WINDOW_STYLE(
                        ES_AUTOHSCROLL as u32 | if secret { ES_PASSWORD as u32 } else { 0 },
                    );
                let edit = create(w!("EDIT"), value, style, 146, y, 426, 30, 0)?;
                SendMessageW(
                    edit,
                    EM_SETLIMITTEXT,
                    WPARAM(if secret { 2048 } else { 512 }),
                    LPARAM(0),
                );
                Ok((label, edit))
            };
        let (_, name) = field("显示名称", &account.name, 156, false)?;
        let (_, address) = field("邮箱地址", &account.address, 196, false)?;
        let (host_label, host) = field(
            "IMAP 服务器",
            if account.provider == MailProvider::Gmail {
                ""
            } else {
                &account.host
            },
            236,
            false,
        )?;
        let (port_label, port) = field("端口", &account.port.to_string(), 276, false)?;
        let (secret_label, secret) = field("密码／授权码", "", 316, true)?;
        let cue = wide(if previous.is_none() {
            "仅保存到 Windows 凭据管理器"
        } else {
            "留空保留现有凭据"
        });
        SendMessageW(secret, 0x1501, WPARAM(0), LPARAM(cue.as_ptr() as isize)); // EM_SETCUEBANNER
        let error = create(w!("STATIC"), "", WINDOW_STYLE(0), 24, 356, 548, 48, 0)?;
        create(
            w!("BUTTON"),
            "取消",
            WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            332,
            420,
            104,
            30,
            ID_CANCEL,
        )?;
        let save = create(
            w!("BUTTON"),
            "保存账户",
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            448,
            420,
            124,
            30,
            ID_SAVE,
        )?;
        Ok(Editor {
            previous,
            account,
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
            font,
            brush,
            light,
            result: None,
        })
    })();
    let mut editor = match controls {
        Ok(editor) => Box::new(editor),
        Err(error) => {
            let _ = DestroyWindow(hwnd);
            let _ = DeleteObject(font);
            let _ = DeleteObject(brush);
            return Err(error);
        }
    };
    editor.update_provider();
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, editor.as_mut() as *mut Editor as isize);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
    let _ = SetFocus(editor.name);
    let mut message = MSG::default();
    while IsWindow(hwnd).as_bool() {
        let status = GetMessageW(&mut message, None, 0, 0).0;
        if status <= 0 {
            let _ = DestroyWindow(hwnd);
            if status == 0 {
                PostQuitMessage(message.wParam.0 as i32);
            }
            break;
        }
        if !IsDialogMessageW(hwnd, &message).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    Ok(editor.result.take())
}

unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let editor = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Editor;
    if editor.is_null() {
        return DefWindowProcW(hwnd, message, wp, lp);
    }
    match message {
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            let notification = (wp.0 >> 16) & 0xffff;
            if id == ID_PROVIDER && notification == CBN_SELCHANGE as usize {
                let selected =
                    SendMessageW((*editor).provider, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as usize;
                if let Some(provider) = MailProvider::ALL.get(selected).copied() {
                    if text((*editor).name) == (*editor).account.provider.title() {
                        set_text((*editor).name, provider.title());
                    }
                    (*editor).account.provider = provider;
                    set_text((*editor).host, provider.host());
                    set_text((*editor).port, "993");
                    set_text((*editor).secret, "");
                    (*editor).update_provider();
                }
            } else if id == ID_CANCEL {
                let _ = DestroyWindow(hwnd);
            } else if id == ID_SAVE && notification == BN_CLICKED as usize {
                match (*editor).save_input() {
                    Ok(result) => {
                        (*editor).result = Some(result);
                        set_text((*editor).secret, "");
                        let _ = DestroyWindow(hwnd);
                    }
                    Err(error) => set_text((*editor).error, &error),
                }
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN => {
            let dc = HDC(wp.0 as _);
            let background = if (*editor).light {
                COLORREF(0x00faf9f7)
            } else {
                COLORREF(0x00272522)
            };
            let foreground = if HWND(lp.0 as _) == (*editor).error {
                COLORREF(0x004147d9)
            } else if (*editor).light {
                COLORREF(0x00362f29)
            } else {
                COLORREF(0x00efebe6)
            };
            SetBkColor(dc, background);
            SetTextColor(dc, foreground);
            LRESULT((*editor).brush.0 as isize)
        }
        WM_ERASEBKGND => {
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            windows::Win32::Graphics::Gdi::FillRect(HDC(wp.0 as _), &rect, (*editor).brush);
            LRESULT(1)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, message, wp, lp)
        }
        _ => DefWindowProcW(hwnd, message, wp, lp),
    }
}

pub fn confirm_remove(owner: HWND, account: MailAccount, sink: EventSink) {
    let prompt = wide(&format!(
        "{}\n{}\n\n移除 Island 保存的账户、凭据和邮件缓存。服务器中的邮件会保留。",
        account.name, account.address
    ));
    let response = unsafe {
        MessageBoxW(
            owner,
            PCWSTR(prompt.as_ptr()),
            w!("移除邮箱账户？"),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };
    if response == IDYES {
        sink.send(AppEvent::MailAction(MailAction::Remove(account.id)));
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

unsafe fn set_text(hwnd: HWND, value: &str) {
    let value = wide(value);
    let _ = SetWindowTextW(hwnd, PCWSTR(value.as_ptr()));
}

fn text(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut value = vec![0; length + 1];
        let read = GetWindowTextW(hwnd, &mut value).max(0) as usize;
        String::from_utf16_lossy(&value[..read])
    }
}
