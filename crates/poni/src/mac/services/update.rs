//! 用户主动检查的Mac更新。关闭配置时不联网，没有自动轮询或常驻线程。
//! 签名通过后才读取下载地址；安装始终交给用户，不替换正在运行的应用。

use crate::events::{AppEvent, EventSink};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSApplication, NSWorkspace};
use objc2_foundation::{NSString, NSURL};
use poni_core::update::{self, Download, Release, Signed};
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub enum Event {
    Checked(Result<Option<Release>, String>),
    DownloadRequested(Release),
    Cancelled,
    Downloaded(Result<PathBuf, String>),
}

/// 短任务只在菜单点击后创建，网络和磁盘校验都不占用主线程。
pub fn start_check(sink: EventSink) -> Result<(), String> {
    std::thread::Builder::new()
        .name("island-update-check".into())
        .spawn(move || {
            sink.send(AppEvent::Update(Event::Checked(check())));
        })
        .map(|_| ())
        .map_err(|_| "无法启动更新检查，请稍后重试".into())
}

pub fn start_download(release: Release, sink: EventSink) -> Result<(), String> {
    std::thread::Builder::new()
        .name("island-update-download".into())
        .spawn(move || {
            sink.send(AppEvent::Update(Event::Downloaded(download(&release))));
        })
        .map(|_| ())
        .map_err(|_| "无法启动更新下载，请稍后重试".into())
}

fn check() -> Result<Option<Release>, String> {
    let source = update::source()?;
    if !update::enabled() {
        return Err("尚未配置在线更新，请等待配置自己的GitHub仓库和更新签名公钥".into());
    }
    let directory = temporary_directory()?;
    let path = directory.join("latest.json");
    let result = (|| {
        fetch(&source.feed_url(), &path, update::MAX_FILE_BYTES, 30)?;
        let bytes = fs::read(&path).map_err(|_| "无法读取更新信息，请重试".to_string())?;
        if bytes.len() as u64 > update::MAX_FILE_BYTES {
            return Err("更新信息过大，已拒绝读取".into());
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| "更新信息格式无效".to_string())?;
        let signed = update::parse_file(text)
            .map_err(|_| "更新签名或信任公钥无效，已停止更新".to_string())?;
        if !verify_signature(&signed) {
            return Err("更新签名校验失败，已停止更新".into());
        }
        let release = update::parse_payload(&signed.payload)
            .map_err(|_| "更新信息或下载地址无效，已停止更新".to_string())?;
        if !update::newer(&release.version, crate::version::VERSION) {
            return Ok(None);
        }
        let asset = release
            .mac
            .as_ref()
            .ok_or("新版尚未提供Mac安装包，请稍后检查")?;
        if asset.file_name() != format!("Island-For-Mac-{}.dmg", release.version) {
            return Err("Mac安装包名称与更新版本不一致，已停止更新".into());
        }
        Ok(Some(release))
    })();
    let _ = fs::remove_dir_all(directory);
    result
}

fn download(release: &Release) -> Result<PathBuf, String> {
    let asset = release.mac.as_ref().ok_or("新版尚未提供Mac安装包")?;
    let cached = crate::paths::cache_dir()
        .join("updates")
        .join(asset.file_name());
    // 同一版本重复下载时复用已校验文件，避免用户取消安装后不断累积镜像。
    if cached.is_file() && verify_download(&cached, asset).is_ok() {
        return Ok(cached);
    }
    let directory = temporary_directory()?;
    let partial = directory.join("download.part");
    let result = (|| {
        fetch(&asset.url, &partial, asset.size, 600)?;
        verify_download(&partial, asset)?;
        fs::rename(&partial, &cached).map_err(|_| "无法保存已校验的安装包，请重试".to_string())?;
        Ok(cached)
    })();
    let _ = fs::remove_dir_all(directory);
    result
}

fn temporary_directory() -> Result<PathBuf, String> {
    let root = crate::paths::cache_dir().join("updates");
    fs::create_dir_all(&root).map_err(|_| "无法创建更新下载目录".to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间不可用".to_string())?
        .as_nanos();
    let path = root.join(format!("{}-{now}", std::process::id()));
    fs::create_dir(&path).map_err(|_| "无法创建临时更新目录".to_string())?;
    Ok(path)
}

/// GitHub资源使用HTTPS重定向；禁用curl配置文件，避免本地配置改变安全选项。
fn fetch(url: &str, path: &Path, max_bytes: u64, seconds: u32) -> Result<(), String> {
    let response = Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-redirs",
            "5",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
        ])
        .args([
            "--max-time",
            &seconds.to_string(),
            "--max-filesize",
            &max_bytes.to_string(),
            "--output",
        ])
        .arg(path)
        .args(["--write-out", "%{http_code}"])
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "无法启动更新下载，请稍后重试".to_string())?;
    let status = String::from_utf8_lossy(&response.stdout);
    if !response.status.success() {
        crate::pwarn!(
            "updates: request failed (curl {:?}, HTTP {})",
            response.status.code(),
            status
        );
        if status == "404" {
            return Err("尚未发布可用的更新文件，请稍后再试".into());
        }
        if response.status.code() == Some(28) {
            return Err("获取更新超时，请检查网络后重试".into());
        }
        return Err("无法获取更新，请检查网络后重试".into());
    }
    let size = fs::metadata(path)
        .map_err(|_| "无法读取更新下载文件".to_string())?
        .len();
    if size > max_bytes {
        return Err("下载文件超过更新声明的大小，已拒绝使用".into());
    }
    Ok(())
}

fn verify_download(path: &Path, asset: &Download) -> Result<(), String> {
    let mut file = File::open(path).map_err(|_| "无法读取安装包".to_string())?;
    if file
        .metadata()
        .map_err(|_| "无法读取安装包大小".to_string())?
        .len()
        != asset.size
    {
        return Err("安装包大小校验失败，请重新下载".into());
    }
    let mut hash = poni_core::sha256::Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "安装包读取失败，请重新下载".to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if poni_core::sha256::hex(&hash.finish()) != asset.sha256 {
        return Err("安装包SHA-256校验失败，已拒绝打开".into());
    }
    Ok(())
}

/// 更新源采用P-256固定宽度r+s；Security框架的X9.62接口读取DER序列。
fn verify_signature(signed: &Signed) -> bool {
    let mut integers = Vec::with_capacity(70);
    for value in signed.signature.chunks_exact(32) {
        let start = value.iter().position(|byte| *byte != 0).unwrap_or(31);
        let value = &value[start..];
        let pad = value[0] & 0x80 != 0;
        integers.extend_from_slice(&[2, (value.len() + usize::from(pad)) as u8]);
        if pad {
            integers.push(0);
        }
        integers.extend_from_slice(value);
    }
    let mut signature = vec![0x30, integers.len() as u8];
    signature.extend(integers);
    unsafe {
        let keys = [kSecAttrKeyType, kSecAttrKeyClass];
        let values = [kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeyClassPublic];
        let attributes = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            2,
            std::ptr::null(),
            std::ptr::null(),
        );
        let point = CFDataCreate(
            std::ptr::null(),
            signed.key.as_ptr(),
            signed.key.len() as isize,
        );
        if attributes.is_null() || point.is_null() {
            if !attributes.is_null() {
                CFRelease(attributes);
            }
            if !point.is_null() {
                CFRelease(point);
            }
            return false;
        }
        let mut error = std::ptr::null();
        let key = SecKeyCreateWithData(point, attributes, &mut error);
        CFRelease(point);
        CFRelease(attributes);
        if !error.is_null() {
            CFRelease(error);
        }
        if key.is_null() {
            return false;
        }
        let data = CFDataCreate(
            std::ptr::null(),
            signed.payload.as_ptr(),
            signed.payload.len() as isize,
        );
        let signature_data = CFDataCreate(
            std::ptr::null(),
            signature.as_ptr(),
            signature.len() as isize,
        );
        let valid = if data.is_null() || signature_data.is_null() {
            false
        } else {
            let mut error = std::ptr::null();
            let valid = SecKeyVerifySignature(
                key,
                kSecKeyAlgorithmECDSASignatureMessageX962SHA256,
                data,
                signature_data,
                &mut error,
            ) != 0;
            if !error.is_null() {
                CFRelease(error);
            }
            valid
        };
        if !data.is_null() {
            CFRelease(data);
        }
        if !signature_data.is_null() {
            CFRelease(signature_data);
        }
        CFRelease(key);
        valid
    }
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecAttrKeyType: *const c_void;
    static kSecAttrKeyClass: *const c_void;
    static kSecAttrKeyTypeECSECPrimeRandom: *const c_void;
    static kSecAttrKeyClassPublic: *const c_void;
    static kSecKeyAlgorithmECDSASignatureMessageX962SHA256: *const c_void;
    fn SecKeyCreateWithData(
        data: *const c_void,
        attributes: *const c_void,
        error: *mut *const c_void,
    ) -> *const c_void;
    fn SecKeyVerifySignature(
        key: *const c_void,
        algorithm: *const c_void,
        data: *const c_void,
        signature: *const c_void,
        error: *mut *const c_void,
    ) -> u8;
}
extern "C" {
    fn CFDataCreate(allocator: *const c_void, bytes: *const u8, count: isize) -> *const c_void;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
}

fn update_alert(mtm: MainThreadMarker) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    if let Some(level) = crate::window::with_window(|window| window.panel.level() + 1) {
        alert.window().setLevel(level);
    }
    alert
}

pub fn message(title: &str, detail: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(mtm).activate();
    let alert = update_alert(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(&NSString::from_str("好"));
    alert.runModal();
    alert.window().orderOut(None);
}

/// 确认后才开始下载；在应用状态借用结束后打开模态窗口。
pub fn offer(release: Release, sink: EventSink) {
    let Some(mtm) = MainThreadMarker::new() else {
        sink.send(AppEvent::Update(Event::Cancelled));
        return;
    };
    NSApplication::sharedApplication(mtm).activate();
    let alert = update_alert(mtm);
    alert.setMessageText(&NSString::from_str(&format!(
        "发现新版本 {}",
        release.version
    )));
    alert.setInformativeText(&NSString::from_str(&format!(
        "{}\n\n下载后将校验并打开安装镜像。请正常退出旧版，再拖入“应用程序”替换；现有数据会保留。",
        release.headline
    )));
    alert
        .addButtonWithTitle(&NSString::from_str("稍后"))
        .setKeyEquivalent(&NSString::from_str("\u{1b}"));
    alert.addButtonWithTitle(&NSString::from_str("下载更新"));
    let answer = alert.runModal();
    alert.window().orderOut(None);
    let event = if answer == NSAlertFirstButtonReturn + 1 {
        Event::DownloadRequested(release)
    } else {
        Event::Cancelled
    };
    sink.send(AppEvent::Update(event));
}

pub fn open_download(path: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    if !NSWorkspace::sharedWorkspace().openURL(&url) {
        message(
            "无法打开安装镜像",
            &format!("请在Finder中打开已校验的安装包：\n{}", path.display()),
        );
    }
}
