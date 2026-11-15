//! 根据微信任务栏闪烁显示通用提醒；不读取消息，没有常驻线程或轮询。

use crate::events::WM_APP_WECHAT;
use std::collections::HashMap;
use std::sync::OnceLock;
use windows::core::{w, Result, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DeregisterShellHookWindow, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    PostMessageW, RegisterShellHookWindow, RegisterWindowMessageW, SetForegroundWindow,
    ShowWindowAsync, SW_RESTORE, SW_SHOW,
};

const HSHELL_WINDOWDESTROYED: usize = 2;
const HSHELL_WINDOWACTIVATED: usize = 4;
const HSHELL_RUDEAPPACTIVATED: usize = 0x8004;
const HSHELL_FLASH: usize = 0x8006;

fn message_id() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("SHELLHOOK")) })
}

/// 先转换成普通应用消息，避免嵌套消息循环借用App时丢失事件。
pub fn forward_message(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> bool {
    let shell = message_id();
    if shell == 0 || msg != shell {
        return false;
    }
    if matches!(
        wp.0,
        HSHELL_FLASH | HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED | HSHELL_WINDOWDESTROYED
    ) {
        if let Err(e) = unsafe { PostMessageW(hwnd, WM_APP_WECHAT, wp, lp) } {
            crate::pwarn!("wechat: couldn't queue attention event ({e})");
        }
    }
    true
}

pub struct Watcher {
    hwnd: HWND,
    /// 已提示窗口及其微信进程；查看其中一个进程不清除另一个实例的提醒。
    pending: HashMap<isize, u32>,
}

pub enum Attention {
    New,
    /// 已查看或已关闭的窗口；调用方同步撤回当前与全屏暂存的提醒。
    Dismissed(Vec<isize>),
}

impl Watcher {
    pub fn start(hwnd: HWND) -> Result<Self> {
        if message_id() == 0 || !unsafe { RegisterShellHookWindow(hwnd) }.as_bool() {
            return Err(windows::core::Error::from_win32());
        }
        crate::pinfo!("wechat: attention listener enabled");
        Ok(Self {
            hwnd,
            pending: HashMap::new(),
        })
    }

    pub fn attention(&mut self, kind: usize, hwnd: HWND, allow_new: bool) -> Option<Attention> {
        if kind == HSHELL_WINDOWDESTROYED {
            return self
                .pending
                .remove(&(hwnd.0 as isize))
                .map(|_| Attention::Dismissed(vec![hwnd.0 as isize]));
        }
        if matches!(kind, HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED) {
            if self.pending.is_empty() {
                return None;
            }
            return self.viewed(wechat_process_id(hwnd)?);
        }
        if kind != HSHELL_FLASH || !allow_new {
            return None;
        }
        let pid = wechat_process_id(hwnd)?;
        // 微信的聊天窗口和主窗口可能不同，不能只比较窗口句柄。
        if window_process_id(unsafe { GetForegroundWindow() }) == pid {
            return self.viewed(pid);
        }
        if self.pending.contains_key(&(hwnd.0 as isize)) {
            return None;
        }
        // 闪烁次数不是消息条数；同一轮只提示一次，查看微信后重新允许提醒。
        self.pending.insert(hwnd.0 as isize, pid);
        Some(Attention::New)
    }

    fn viewed(&mut self, pid: u32) -> Option<Attention> {
        let mut handles = Vec::new();
        for (handle, owner) in &self.pending {
            if *owner == pid {
                handles.push(*handle);
            }
        }
        if handles.is_empty() {
            return None;
        }
        self.pending.retain(|_, owner| *owner != pid);
        Some(Attention::Dismissed(handles))
    }

    /// 全屏结束可能先于激活事件入队；恢复提醒前再核对它是否仍未查看。
    pub fn can_restore(&self, handle: isize) -> bool {
        let Some(pid) = self.pending.get(&handle) else {
            return false;
        };
        window_process_id(HWND(handle as _)) == *pid
            && window_process_id(unsafe { GetForegroundWindow() }) != *pid
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if !unsafe { DeregisterShellHookWindow(self.hwnd) }.as_bool() {
            crate::pwarn!("wechat: couldn't remove the attention listener");
        }
        crate::pinfo!("wechat: attention listener disabled");
    }
}

/// 点击提醒时再次确认进程，避免已销毁窗口的句柄被其他应用复用。
pub fn open_window(handle: isize) {
    let hwnd = HWND(handle as _);
    if wechat_process_id(hwnd).is_none() {
        return;
    }
    unsafe {
        let show = if IsIconic(hwnd).as_bool() {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        // 微信窗口属于另一个线程；异步恢复，避免对方无响应时阻塞小岛。
        if !ShowWindowAsync(hwnd, show).as_bool() {
            crate::pwarn!("wechat: couldn't show the window");
            return;
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            crate::pwarn!("wechat: Windows didn't activate the window");
        }
    }
}

fn window_process_id(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn wechat_process_id(hwnd: HWND) -> Option<u32> {
    let pid = window_process_id(hwnd);
    if pid == 0 {
        return None;
    }
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return None;
    };
    let mut buf = [0_u16; 1024];
    let mut len = buf.len() as u32;
    let read = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    if read.is_err() {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    let matched = path.rsplit(['\\', '/']).next().is_some_and(|name| {
        name.eq_ignore_ascii_case("WeChat.exe") || name.eq_ignore_ascii_case("Weixin.exe")
    });
    matched.then_some(pid)
}
