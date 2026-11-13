//! 用户主动请求的本机邮件翻译；离开预览时释放句柄并取消，结果经事件队列返回。

use crate::events::{AppEvent, EventSink};
use crate::mail::{MailEvent, MailPreview, MailTranslation};
use poni_core::mail::MailAccount;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr::NonNull;

extern "C" {
    fn poni_translation_start(
        subject: *const c_char,
        body: *const c_char,
        callback: extern "C" fn(*mut c_void, i32, *const c_char, *const c_char, *const c_char),
        context: *mut c_void,
    ) -> *mut c_void;
    fn poni_translation_cancel(handle: *mut c_void);
}

struct Context {
    account_id: String,
    revision: u64,
    message_id: String,
    request_id: u64,
    sink: EventSink,
}

pub struct Request {
    handle: NonNull<c_void>,
}

pub fn start(
    request_id: u64,
    account: &MailAccount,
    preview: &MailPreview,
    sink: EventSink,
) -> Result<Request, String> {
    let subject = CString::new(preview.message.subject.as_str())
        .map_err(|_| "邮件主题包含无法翻译的字符".to_string())?;
    let body = CString::new(preview.body.as_str())
        .map_err(|_| "邮件正文包含无法翻译的字符".to_string())?;
    let context = Box::into_raw(Box::new(Context {
        account_id: account.id.clone(),
        revision: account.revision,
        message_id: preview.message.id.clone(),
        request_id,
        sink,
    }));
    // 原生入口同步复制两个字符串；Context 的唯一回收方是结果回调，取消也会回调一次。
    let handle = unsafe {
        poni_translation_start(
            subject.as_ptr(),
            body.as_ptr(),
            translation_result,
            context.cast(),
        )
    };
    match NonNull::new(handle) {
        Some(handle) => Ok(Request { handle }),
        None => {
            drop(unsafe { Box::from_raw(context) });
            Err("无法启动 macOS 本机翻译".to_string())
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        // 原生句柄的引用在此释放；已完成的请求只释放句柄，不重复回调。
        unsafe { poni_translation_cancel(self.handle.as_ptr()) }
    }
}

extern "C" fn translation_result(
    context: *mut c_void,
    status: i32,
    subject: *const c_char,
    body: *const c_char,
    error: *const c_char,
) {
    let context = unsafe { Box::from_raw(context.cast::<Context>()) };
    let result = if status == 0 {
        Ok(MailTranslation {
            subject: string_from_native(subject),
            body: string_from_native(body),
        })
    } else {
        Err(string_from_native(error))
    };
    context.sink.send(AppEvent::Mail(MailEvent::Translated {
        account_id: context.account_id,
        revision: context.revision,
        message_id: context.message_id,
        request_id: context.request_id,
        result,
    }));
}

fn string_from_native(value: *const c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}
