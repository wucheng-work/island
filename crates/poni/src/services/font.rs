//! 使用系统字体选择器选择已安装字体，不改变界面的字号和字重层级。

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{DEFAULT_CHARSET, LOGFONTW, SYMBOL_CHARSET};
use windows::Win32::UI::Controls::Dialogs::{
    ChooseFontW, CommDlgExtendedError, CF_FORCEFONTEXIST, CF_INITTOLOGFONTSTRUCT, CF_NOSCRIPTSEL,
    CF_NOSIZESEL, CF_NOSTYLESEL, CF_NOVERTFONTS, CF_SCREENFONTS, CF_SCRIPTSONLY, CHOOSEFONTW,
};

/// 取消选择返回 `Ok(None)`；系统对话框失败时保留错误供界面提示。
pub fn choose(owner: HWND, current: &str) -> Result<Option<String>, String> {
    let mut font = LOGFONTW {
        lfCharSet: DEFAULT_CHARSET,
        ..Default::default()
    };
    let initial = if current.trim().is_empty() {
        "Segoe UI"
    } else {
        current.trim()
    };
    // LOGFONT 的字体名称固定为 32 个 UTF-16 单元，末尾保留空字符。
    let initial: Vec<u16> = initial.encode_utf16().collect();
    let count = initial.len().min(font.lfFaceName.len() - 1);
    font.lfFaceName[..count].copy_from_slice(&initial[..count]);
    let mut dialog = CHOOSEFONTW {
        lStructSize: std::mem::size_of::<CHOOSEFONTW>() as u32,
        hwndOwner: owner,
        lpLogFont: &mut font,
        Flags: CF_SCREENFONTS
            | CF_INITTOLOGFONTSTRUCT
            | CF_FORCEFONTEXIST
            | CF_SCRIPTSONLY
            | CF_NOSCRIPTSEL
            | CF_NOVERTFONTS
            | CF_NOSIZESEL
            | CF_NOSTYLESEL,
        ..Default::default()
    };
    if unsafe { ChooseFontW(&mut dialog) }.as_bool() {
        let length = font
            .lfFaceName
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(font.lfFaceName.len());
        let name = String::from_utf16_lossy(&font.lfFaceName[..length]);
        if name.trim().is_empty() || name.starts_with('@') || font.lfCharSet == SYMBOL_CHARSET {
            return Err("请选择普通文字字体；符号字体和竖排字体不能用于界面。".into());
        }
        return Ok(Some(name));
    }
    let error = unsafe { CommDlgExtendedError() }.0;
    if error == 0 {
        return Ok(None);
    }
    Err(format!(
        "无法打开字体选择器（错误 {error:#x}），请稍后重试。"
    ))
}
