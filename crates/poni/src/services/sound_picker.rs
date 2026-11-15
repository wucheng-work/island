//! 用户主动选择内置提示音时打开分类菜单，不启动后台工作。

use crate::commands::SoundModule;
use poni_core::sound_catalog::{self, SOUNDS};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetForegroundWindow, IsWindow,
    PostMessageW, SetForegroundWindow, TrackPopupMenu, MF_CHECKED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

/// 取消时不改变选择；返回目录 ID，由调用方保存并试听。
pub fn choose(owner: HWND, module: SoundModule, current: &str) -> Result<Option<String>, String> {
    unsafe {
        let menu = CreatePopupMenu().map_err(|error| format!("无法打开提示音菜单：{error}"))?;
        let result = (|| -> windows::core::Result<Option<String>> {
            let default_name = sound_catalog::find(module.default_id())
                .map_or("默认提示音", |sound| sound.name);
            let default_label: Vec<u16> = format!("恢复默认：{default_name}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let default_id = SOUNDS.len() + 1;
            AppendMenuW(menu, MF_STRING, default_id, PCWSTR(default_label.as_ptr()))?;
            AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())?;

            let mut categories = Vec::new();
            for sound in SOUNDS {
                if !categories.contains(&sound.category) {
                    categories.push(sound.category);
                }
            }
            for category in categories {
                let submenu = CreatePopupMenu()?;
                let built = (|| -> windows::core::Result<()> {
                    for (index, sound) in SOUNDS
                        .iter()
                        .enumerate()
                        .filter(|(_, sound)| sound.category == category)
                    {
                        let label: Vec<u16> = sound.name.encode_utf16().chain(Some(0)).collect();
                        let mut flags = MF_STRING;
                        if sound.id == current {
                            flags |= MF_CHECKED;
                        }
                        AppendMenuW(submenu, flags, index + 1, PCWSTR(label.as_ptr()))?;
                    }
                    let label: Vec<u16> = category.encode_utf16().chain(Some(0)).collect();
                    AppendMenuW(menu, MF_POPUP, submenu.0 as usize, PCWSTR(label.as_ptr()))
                })();
                if let Err(error) = built {
                    // 尚未挂到父菜单的句柄必须单独释放；成功后由父菜单统一释放。
                    let _ = DestroyMenu(submenu);
                    return Err(error);
                }
            }

            let mut position = POINT::default();
            GetCursorPos(&mut position)?;
            let previous = GetForegroundWindow();
            let _ = SetForegroundWindow(owner);
            let selected = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                position.x,
                position.y,
                0,
                owner,
                None,
            )
            .0 as usize;
            let _ = PostMessageW(owner, WM_NULL, WPARAM(0), LPARAM(0));
            if !previous.is_invalid()
                && previous != owner
                && IsWindow(previous).as_bool()
                && GetForegroundWindow() == owner
            {
                let _ = SetForegroundWindow(previous);
            }
            if selected == default_id {
                return Ok(Some(module.default_id().into()));
            }
            Ok(selected
                .checked_sub(1)
                .and_then(|index| SOUNDS.get(index))
                .map(|sound| sound.id.into()))
        })();
        // 包括取消、选择和构建失败，父菜单及已挂载子菜单都在这里释放。
        let _ = DestroyMenu(menu);
        result.map_err(|error| format!("无法打开提示音菜单：{error}"))
    }
}
