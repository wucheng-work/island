//! 邮箱密码及 OAuth 令牌只存放在当前 Windows 用户的凭据管理器。

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_MAX_CREDENTIAL_BLOB_SIZE,
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

pub struct CredentialError(i32);

impl CredentialError {
    pub fn code(&self) -> i32 {
        self.0
    }
}

fn target(service: &str, id: &str) -> Vec<u16> {
    format!("{service}/{id}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

pub fn get_generic_password(service: &str, id: &str) -> Result<Vec<u8>, CredentialError> {
    let name = target(service, id);
    let mut credential = std::ptr::null_mut();
    unsafe {
        CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, 0, &mut credential)
            .map_err(|error| CredentialError(error.code().0 & 0xffff))?;
        let value = &*credential;
        let bytes = if value.CredentialBlobSize == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(value.CredentialBlob, value.CredentialBlobSize as usize)
                .to_vec()
        };
        CredFree(credential.cast());
        Ok(bytes)
    }
}

pub fn set_generic_password(service: &str, id: &str, bytes: &[u8]) -> Result<(), CredentialError> {
    // 系统上限由 Credential Manager 定义，超限时保持原凭据，不落地明文文件。
    if bytes.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        return Err(CredentialError(87));
    }
    let mut name = target(service, id);
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(name.as_mut_ptr()),
        CredentialBlobSize: bytes.len() as u32,
        CredentialBlob: bytes.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0).map_err(|error| CredentialError(error.code().0 & 0xffff)) }
}

pub fn delete_generic_password(service: &str, id: &str) -> Result<(), CredentialError> {
    let name = target(service, id);
    unsafe {
        match CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, 0) {
            Ok(()) => Ok(()),
            Err(error) if error.code().0 & 0xffff == ERROR_NOT_FOUND.0 as i32 => Ok(()),
            Err(error) => Err(CredentialError(error.code().0 & 0xffff)),
        }
    }
}
