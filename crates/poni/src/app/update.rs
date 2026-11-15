//! 手动在线更新：签名检查、确认下载、确认安装。原生对话框在 App 借用外运行，
//! 避免 MessageBox 的嵌套消息循环阻塞媒体和保存事件。

use super::{with_app, App};
use crate::icons::Icon;
use crate::services;
use poni_core::geom::palette;
use poni_core::update::{self, Release};
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONINFORMATION, MB_YESNO,
};

pub enum Prompt {
    Download(Release),
    Install(Release),
}

/// 只由窗口处理器在释放 App 借用后调用；默认选择“否”，下载和安装分别确认。
pub(super) fn run_prompt(mut prompt: Prompt) {
    loop {
        let Some(owner) = with_app(|app| app.helper) else {
            return;
        };
        let message = match &prompt {
            Prompt::Download(release) => format!(
                "发现新版本 {}\n当前版本 {}\n\n{}\n\n是否下载更新？下载完成后会再次询问是否安装。",
                release.version, crate::version::VERSION, release.headline
            ),
            Prompt::Install(release) => format!(
                "Island For Windows {} 已下载，签名信息、大小和 SHA-256 校验通过。\n\n是否保存当前状态、退出并安装？",
                release.version
            ),
        };
        let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
        let accepted = unsafe {
            MessageBoxW(
                owner,
                PCWSTR(message.as_ptr()),
                w!("Island For Windows 更新"),
                MB_YESNO | MB_ICONINFORMATION | MB_DEFBUTTON2,
            ) == IDYES
        };
        let next = with_app(|app| {
            app.update_prompt_open = false;
            match (prompt, accepted) {
                (Prompt::Download(_), true) => app.fetch_setup(),
                (Prompt::Install(_), true) => app.update_now(),
                (Prompt::Download(_), false) => {
                    app.update_feedback("已取消下载", "需要时可在右键菜单重新检查更新。", false)
                }
                (Prompt::Install(_), false) => app.update_feedback(
                    "已取消安装",
                    "当前版本继续运行；下次检查时会重新验证更新。",
                    false,
                ),
            }
            app.request_frame();
            app.take_update_prompt()
        })
        .flatten();
        let Some(next) = next else { return };
        prompt = next;
    }
}

impl App {
    pub(super) fn take_update_prompt(&mut self) -> Option<Prompt> {
        if self.update_prompt_open {
            return None;
        }
        let prompt = self.update_prompt.take()?;
        self.update_prompt_open = true;
        Some(prompt)
    }

    pub(super) fn update_feedback(&mut self, title: &str, message: &str, success: bool) {
        let (icon, color) = if success {
            (Icon::CheckCircle, palette::GREEN)
        } else {
            (Icon::Download, palette::ORANGE)
        };
        self.st.notify(icon, color, title, message, 7.0, None);
        self.request_frame();
    }

    pub(super) fn check_for_updates(&mut self) {
        if !update::enabled() {
            self.update_feedback(
                "在线更新尚未配置",
                "更新仓库和可信公钥配置齐备后才能检查；本次未联网。",
                false,
            );
            return;
        }
        if self.update_prompt_open
            || self.update_prompt.is_some()
            || self
                .updater
                .as_ref()
                .is_some_and(|updater| updater.manual_busy())
        {
            self.update_feedback(
                "更新任务正在进行",
                "请等待当前检查、下载或确认操作完成。",
                false,
            );
            return;
        }
        let Some(updater) = self.updater.as_mut() else {
            self.update_feedback(
                "当前版本无法在线更新",
                "请使用项目发行页面中的安装包。",
                false,
            );
            return;
        };
        if !updater.manual_begin(&mut self.st) {
            return;
        }
        self.update_feedback("正在检查更新", "正在验证本项目发布的签名更新信息。", true);
        let job = services::update::check_job(self.sink.clone());
        if !self
            .services
            .as_mut()
            .is_some_and(|services| services.run_job(job))
        {
            self.on_update_checked(Err("无法启动更新服务，请重试".into()));
        }
    }

    pub(super) fn on_update_checked(&mut self, result: Result<Release, String>) {
        let Some(updater) = self.updater.as_mut() else {
            return;
        };
        match updater.manual_checked(&mut self.st, result) {
            Ok(Some(release)) => self.update_prompt = Some(Prompt::Download(release)),
            Ok(None) => self.update_feedback(
                "已是最新版本",
                &format!("当前版本 {}。", crate::version::VERSION),
                true,
            ),
            Err(message) => self.update_feedback("无法检查更新", &message, false),
        }
    }

    fn fetch_setup(&mut self) {
        let Some(updater) = self.updater.as_mut() else {
            return;
        };
        let (download, path) = match updater.manual_download() {
            Ok(download) => download,
            Err(message) => {
                self.update_feedback("无法下载更新", &message, false);
                return;
            }
        };
        self.update_feedback("正在下载更新", "下载完成并校验后会询问是否安装。", true);
        let job = services::update::download_job(self.sink.clone(), download, path);
        if !self
            .services
            .as_mut()
            .is_some_and(|services| services.run_job(job))
        {
            self.on_update_downloaded(Err("无法启动更新服务，请重试".into()));
        }
    }

    pub(super) fn on_update_downloaded(&mut self, result: Result<PathBuf, String>) {
        let Some(updater) = self.updater.as_mut() else {
            return;
        };
        match updater.manual_fetched(&mut self.st, result) {
            Ok(release) => self.update_prompt = Some(Prompt::Install(release)),
            Err(message) => self.update_feedback(
                "更新下载失败",
                &format!("{message}\n请在右键菜单重新检查更新。"),
                false,
            ),
        }
    }

    pub(super) fn update_now(&mut self) {
        let ready = self
            .updater
            .as_ref()
            .ok_or_else(|| "当前版本未启用在线更新".to_string())
            .and_then(|updater| updater.manual_ready());
        let (setup, release) = match ready {
            Ok(ready) => ready,
            Err(message) => {
                self.update_feedback("无法安装更新", &message, false);
                return;
            }
        };
        let download = match services::update::validate_release(&release) {
            Ok(download) => download,
            Err(message) => {
                self.update_feedback("无法安装更新", &message, false);
                return;
            }
        };
        // 必须先保存成功，再启动会关闭当前进程的安装器。
        self.save_now();
        self.save_settings();
        if self.st.save_errors.any() {
            self.st.save_errors.quit_blocked = true;
            if let Some(window) = self.settings_win.as_mut() {
                window.view.dirty = true;
            }
            self.expand();
            self.update_feedback(
                "暂未安装更新",
                "当前数据保存失败，请处理保存提示后重新检查更新。",
                false,
            );
            return;
        }
        match services::update::install(&setup, download) {
            Ok(()) => {
                crate::pinfo!("updates: installing verified version {}", release.version);
                self.finish_quit(0);
            }
            Err(message) => self.update_feedback(
                "无法启动安装器",
                &format!("{message}\n当前版本继续运行，请重新检查更新。"),
                false,
            ),
        }
    }
}
