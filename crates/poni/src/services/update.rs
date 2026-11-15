//! 用户手动检查本项目的 GitHub 签名更新源。CNG 验证签名后才信任版本信息；
//! 下载和执行还要校验仓库、版本、安装器文件名、大小及 SHA-256。

use crate::events::{AppEvent, EventSink};
use crate::services::http::Client;
use crate::services::update_worker::Job;
use poni_core::update::{self, Release};
use std::time::Duration;

/// Budget for the file: it is small, but a slow line still gets it.
const TIMEOUT: Duration = Duration::from_secs(20);
/// Budget for the Setup (about 2.5 MB): a slow line's few minutes.
const SETUP_TIMEOUT: Duration = Duration::from_secs(300);

/// What the update file said.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateEvent {
    /// The latest release (signature checked), or why the file couldn't be
    /// read or believed.
    Checked(Result<Release, String>),
    /// The new Setup is on disk and checked, or why not.
    Downloaded(Result<std::path::PathBuf, String>),
}

/// A job for the update thread: read the file, answer with `AppEvent::Update`.
pub fn check_job(sink: EventSink) -> Job {
    Box::new(move |client: Result<&Client, &str>| {
        let r = client.map_err(str::to_string).and_then(check);
        match &r {
            Ok(rel) => crate::pinfo!(
                "updates: the latest release is {} ({})",
                rel.version,
                rel.date
            ),
            Err(e) => crate::pinfo!("updates: couldn't read the update file ({e})"),
        }
        sink.send(AppEvent::Update(UpdateEvent::Checked(r)));
    })
}

fn check(client: &Client) -> Result<Release, String> {
    if !update::enabled() {
        return Err("在线更新尚未配置可信公钥，未发送网络请求".into());
    }
    let url = update::source()?.feed_url();
    let (status, body) = client
        .get_bytes(&url, TIMEOUT, update::MAX_FILE_BYTES, |_| {})
        .map_err(|e| e.to_string())?;
    if status == 404 {
        return Err("尚未发布正式版本，请稍后再检查".into());
    }
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    let release =
        believe(std::str::from_utf8(&body).map_err(|_| "更新信息不是有效文本".to_string())?)?;
    if update::newer(&release.version, crate::version::VERSION) {
        validate_release(&release)?;
    }
    Ok(release)
}

/// The file's text to the release it describes, once its signature checks out.
pub fn believe(text: &str) -> Result<Release, String> {
    #[cfg(not(test))]
    let signed = update::parse_file(text)?;
    #[cfg(test)]
    let signed = update::parse_file_with_keys(text, FIXTURE_KEYS)?;
    if !verify(signed.payload.as_bytes(), &signed.signature, &signed.key) {
        return Err("签名验证失败（signature），已忽略更新信息".into());
    }
    #[cfg(not(test))]
    let release = update::parse_payload(&signed.payload)?;
    #[cfg(test)]
    let release =
        update::parse_payload_for_repository(&signed.payload, "lucacadalora/oyenza-studio")?;
    Ok(release)
}

// 原有离线签名样本不使用也不扩展生产信任目录。
#[cfg(test)]
const FIXTURE_KEYS: &str = include_str!("../../../poni-core/data/update-keys.txt");

/// 只接受当前仓库指定版本的 Windows 安装器，不允许用任意哈希文件替代。
pub fn validate_release(release: &Release) -> Result<&update::Download, String> {
    let download = release
        .windows
        .as_ref()
        .ok_or("该发行版缺少 Windows 安装器")?;
    validate_download(download)?;
    if download.version() != Some(release.version.as_str()) {
        return Err("安装器版本与签名更新信息不一致".into());
    }
    Ok(download)
}

fn validate_download(download: &update::Download) -> Result<(), String> {
    if !update::enabled() {
        return Err("在线更新尚未配置可信公钥".into());
    }
    let version = download.version().ok_or("安装器不属于本项目更新仓库")?;
    if !update::newer(version, crate::version::VERSION) {
        return Err("安装器版本必须高于当前版本".into());
    }
    let expected = format!("Island-For-Windows-Setup-{version}.exe");
    if download.file_name() != expected
        || download.size == 0
        || download.sha256.len() != 64
        || !download
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Windows 安装器名称、大小或摘要无效".into());
    }
    Ok(())
}

/// A job for the update thread: put the Setup `d` at `path`, unless it is
/// there already, and answer with `UpdateEvent::Downloaded`.
pub fn download_job(sink: EventSink, d: update::Download, path: std::path::PathBuf) -> Job {
    Box::new(move |client: Result<&Client, &str>| {
        let r = download(client, &d, &path);
        let r = r.map(|()| path);
        match &r {
            Ok(_) => crate::pinfo!("updates: {} is ready", d.file_name()),
            Err(e) => crate::pinfo!("updates: couldn't download {} ({e})", d.file_name()),
        }
        sink.send(AppEvent::Update(UpdateEvent::Downloaded(r)));
    })
}

fn download(
    client: Result<&Client, &str>,
    d: &update::Download,
    path: &std::path::Path,
) -> Result<(), String> {
    use std::fs;
    validate_download(d)?;
    if path.file_name().and_then(|name| name.to_str()) != Some(d.file_name()) {
        return Err("安装器保存路径与签名文件名不一致".into());
    }
    // Downloaded on an earlier run: checked again, no request.
    if fs::read(path).is_ok_and(|b| d.matches(&b)) {
        return Ok(());
    }
    let (status, bytes) = client?
        .get_bytes(
            &d.url,
            SETUP_TIMEOUT,
            d.size.saturating_add(64 * 1024),
            |_| {},
        )
        .map_err(|e| e.to_string())?;
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    if !d.matches(&bytes) {
        return Err(format!(
            "下载内容校验失败（实际 {} 字节，预期 {} 字节）",
            bytes.len(),
            d.size
        ));
    }
    let dir = path.parent().ok_or("no folder")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = path.with_extension("part");
    fs::write(&part, &bytes)
        .and_then(|()| fs::rename(&part, path))
        .map_err(|e| {
            let _ = fs::remove_file(&part);
            e.to_string()
        })
}

/// Run the Setup at `setup` silently, once it still matches `d`. Like any
/// install it closes this copy (it saves first); it keeps Start with
/// Windows as it is and opens the new version when done (`/reopen`,
/// installer/poni.nsi).
#[cfg(windows)]
pub fn install(setup: &std::path::Path, d: &update::Download) -> Result<(), String> {
    validate_download(d)?;
    if setup.file_name().and_then(|name| name.to_str()) != Some(d.file_name()) {
        return Err("安装器文件名与签名信息不一致".into());
    }
    if !std::fs::read(setup).is_ok_and(|b| d.matches(&b)) {
        return Err("安装器已丢失或内容被更改，请重新检查更新".into());
    }
    std::process::Command::new(setup)
        .args(["/S", "/reopen"])
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Windows Setup is responsible for replacing the installed application.
pub fn can_install() -> bool {
    true
}

/// ECDSA P-256 over SHA-256: `signature` is r and s, `key` an uncompressed point.
#[cfg(windows)]
fn verify(message: &[u8], signature: &[u8; 64], key: &[u8; 65]) -> bool {
    use windows::Win32::Security::Cryptography::{
        BCryptCloseAlgorithmProvider, BCryptDestroyKey, BCryptImportKeyPair,
        BCryptOpenAlgorithmProvider, BCryptVerifySignature, BCRYPT_ALG_HANDLE,
        BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM, BCRYPT_ECDSA_PUBLIC_P256_MAGIC,
        BCRYPT_FLAGS, BCRYPT_KEY_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
    };
    // BCRYPT_ECCKEY_BLOB: magic, key length, then X and Y.
    let mut blob = Vec::with_capacity(72);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&32u32.to_le_bytes());
    blob.extend_from_slice(&key[1..]);
    let hash = poni_core::sha256::sha256(message);
    unsafe {
        let mut alg = BCRYPT_ALG_HANDLE::default();
        if BCryptOpenAlgorithmProvider(
            &mut alg,
            BCRYPT_ECDSA_P256_ALGORITHM,
            None,
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        )
        .is_err()
        {
            return false;
        }
        let mut handle = BCRYPT_KEY_HANDLE::default();
        let ok = BCryptImportKeyPair(
            alg,
            BCRYPT_KEY_HANDLE::default(),
            BCRYPT_ECCPUBLIC_BLOB,
            &mut handle,
            &blob,
            0,
        )
        .is_ok()
            && BCryptVerifySignature(handle, None, &hash, signature, BCRYPT_FLAGS(0)).is_ok();
        if !handle.is_invalid() {
            let _ = BCryptDestroyKey(handle);
        }
        let _ = BCryptCloseAlgorithmProvider(alg, 0);
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real file, signed with the signing key for v0.2.3.
    const SAMPLE: &str = include_str!("../../../poni-core/data/update-sample.json");

    #[test]
    fn a_signed_file_is_believed() {
        let r = believe(SAMPLE).expect("the sample's signature checks out");
        assert_eq!(
            (r.version.as_str(), r.headline.as_str()),
            ("0.2.3", "Sing along from the closed island")
        );
        assert_eq!(r.windows.as_ref().map(|d| d.size), Some(2457657));
    }

    #[test]
    fn a_changed_file_is_not() {
        // One character of the payload, or of the signature.
        let changed = SAMPLE.replacen("2457657", "2457658", 1);
        assert!(
            believe(&changed).unwrap_err().contains("signature"),
            "payload changed"
        );
        let signed = update::parse_file_with_keys(SAMPLE, FIXTURE_KEYS).unwrap();
        let mut sig = signed.signature;
        sig[10] ^= 1;
        assert!(!verify(signed.payload.as_bytes(), &sig, &signed.key));
        assert!(verify(
            signed.payload.as_bytes(),
            &signed.signature,
            &signed.key
        ));
        // The spare key didn't sign it.
        let spare_file = SAMPLE.replace("4cb3280eaea65b50", "f835f77c60bd2b7f");
        let spare = update::parse_file_with_keys(&spare_file, FIXTURE_KEYS)
            .unwrap()
            .key;
        assert!(!verify(
            signed.payload.as_bytes(),
            &signed.signature,
            &spare
        ));
    }
}
