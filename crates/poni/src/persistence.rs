//! 两个平台共用的保存结果，不改变原子写入、备份或启动读取保护。

use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFile {
    Settings,
    Store,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// 启动时未能安全读取，禁止用当前内存数据覆盖旧文件。
    Protected,
    Write,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveErrors {
    pub settings: Option<SaveError>,
    pub store: Option<SaveError>,
    /// 退出时保存失败，等待用户重试或明确选择放弃。
    pub quit_blocked: bool,
}

impl SaveErrors {
    pub fn any(&self) -> bool {
        self.settings.is_some() || self.store.is_some()
    }

    pub fn retryable(&self) -> bool {
        self.settings == Some(SaveError::Write) || self.store == Some(SaveError::Write)
    }
}

pub fn save<T: Serialize>(path: &Path, value: &T, writable: bool) -> Result<(), SaveError> {
    if !writable {
        pwarn!("{} couldn't be read at start; not overwriting it", path.display());
        return Err(SaveError::Protected);
    }
    poni_core::store::save_json_atomic(path, value).map_err(|error| {
        pwarn!("saving {} failed: {error}", path.display());
        SaveError::Write
    })
}
