//! 用户主动打开的系统调色板，不启动后台工作。

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::UI::Controls::Dialogs::{
    ChooseColorW, CommDlgExtendedError, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW,
};

pub fn choose(owner: HWND, rgb: u32) -> Result<Option<u32>, String> {
    let mut custom = [COLORREF(0x00ffffff); 16];
    let mut dialog = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: COLORREF((rgb & 0x00ff00) | (rgb >> 16) | ((rgb & 0xff) << 16)),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    if unsafe { ChooseColorW(&mut dialog) }.as_bool() {
        let color = dialog.rgbResult.0;
        return Ok(Some(
            (color & 0x00ff00) | ((color & 0xff) << 16) | ((color >> 16) & 0xff),
        ));
    }
    let error = unsafe { CommDlgExtendedError() };
    if error.0 == 0 {
        Ok(None)
    } else {
        Err(format!("无法打开调色板（{}）", error.0))
    }
}
