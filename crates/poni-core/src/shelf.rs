//! The Shelf: a temporary stash of file/folder references dropped onto the island.

use serde::{Deserialize, Serialize};
use std::io::{self, Read};
use std::path::PathBuf;

pub const MAX_SHELF_ITEMS: usize = 24;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShelfItem {
    pub path: String,
    pub added_unix: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shelf {
    pub items: Vec<ShelfItem>,
}

/// 后台内容检查的结果；不会持久化摘要，避免原文件修改后误用旧摘要。
pub struct PreparedPaths {
    pub paths: Vec<String>,
    pub duplicates: usize,
    pub unreadable: usize,
}

struct FileEntry {
    path: String,
    canonical: Option<PathBuf>,
    size: Option<u64>,
    digest: Option<[u8; 32]>,
    checked: bool,
}

impl FileEntry {
    fn new(path: String) -> Self {
        let size = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len());
        Self {
            canonical: std::fs::canonicalize(&path).ok(),
            path,
            size,
            digest: None,
            checked: false,
        }
    }

    fn digest(&mut self, unreadable: &mut usize) -> Option<[u8; 32]> {
        if !self.checked {
            self.checked = true;
            match file_digest(&self.path) {
                Ok(digest) => self.digest = Some(digest),
                Err(_) => *unreadable += 1,
            }
        }
        self.digest
    }
}

impl Shelf {
    /// 仅在后台调用：同大小文件再做流式内容比较，文件夹只比较实际路径。
    pub fn prepare_paths(&self, paths: Vec<String>) -> PreparedPaths {
        let mut result = PreparedPaths {
            paths: Vec::new(),
            duplicates: 0,
            unreadable: 0,
        };
        let mut known: Vec<FileEntry> = self
            .items
            .iter()
            .map(|item| FileEntry::new(item.path.clone()))
            .collect();
        for path in paths {
            if path.is_empty() {
                continue;
            }
            let key = path.to_lowercase();
            if let Some(index) = known
                .iter()
                .position(|item| item.path.to_lowercase() == key)
            {
                // 同一路径继续交给 add_paths，保留移到最前且新增数为零的行为。
                let item = known.remove(index);
                known.insert(0, item);
                result.paths.push(path);
                continue;
            }
            let mut file = FileEntry::new(path.clone());
            let mut duplicate = false;
            for item in &mut known {
                if file.canonical.is_some() && file.canonical == item.canonical {
                    duplicate = true;
                    break;
                }
                if file.size.is_none() || file.size != item.size {
                    continue;
                }
                let Some(digest) = file.digest(&mut result.unreadable) else {
                    continue;
                };
                if item.digest(&mut result.unreadable) == Some(digest) {
                    duplicate = true;
                    break;
                }
            }
            if duplicate {
                result.duplicates += 1;
                continue;
            }
            result.paths.push(path);
            known.insert(0, file);
            // 与 add_paths 的淘汰顺序一致，后面的文件仍可替代已被淘汰的引用。
            known.truncate(MAX_SHELF_ITEMS);
        }
        result
    }

    /// Add paths (newest first), de-duplicated case-insensitively. Returns how many were new.
    pub fn add_paths<S: AsRef<str>>(&mut self, paths: &[S], now_unix: i64) -> usize {
        let mut added = 0;
        for p in paths {
            // Paths arrive from the file picker or drag/drop; spaces can be part of a file name.
            let p = p.as_ref();
            if p.is_empty() {
                continue;
            }
            let key = p.to_lowercase();
            if let Some(i) = self
                .items
                .iter()
                .position(|it| it.path.to_lowercase() == key)
            {
                let it = self.items.remove(i);
                self.items.insert(
                    0,
                    ShelfItem {
                        added_unix: now_unix,
                        ..it
                    },
                );
                continue;
            }
            self.items.insert(
                0,
                ShelfItem {
                    path: p.to_string(),
                    added_unix: now_unix,
                },
            );
            added += 1;
        }
        self.items.truncate(MAX_SHELF_ITEMS);
        added
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let before = self.items.len();
        let key = path.to_lowercase();
        self.items.retain(|it| it.path.to_lowercase() != key);
        before != self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn paths(&self) -> Vec<String> {
        self.items.iter().map(|i| i.path.clone()).collect()
    }
}

fn file_digest(path: &str) -> io::Result<[u8; 32]> {
    let mut file = std::fs::File::open(path)?;
    let before = file.metadata()?;
    let mut hash = crate::sha256::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let after = file.metadata()?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "file changed while checking its content",
        ));
    }
    Ok(hash.finish())
}

/// Last path component for display ("C:\a\b\report.pdf" → "report.pdf").
pub fn display_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let name = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if name.is_empty() {
        path.to_string()
    } else {
        name.to_string()
    }
}

/// Lower-case extension without the dot ("" for none / folders).
pub fn extension(path: &str) -> String {
    let name = display_name(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 8 => {
            ext.to_lowercase()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedupes_and_orders() {
        let mut s = Shelf::default();
        assert_eq!(s.add_paths(&["C:\\a.txt", "C:\\b.png"], 1), 2);
        assert_eq!(s.items[0].path, "C:\\b.png");
        assert_eq!(s.add_paths(&["c:\\A.TXT"], 2), 0);
        assert_eq!(s.items[0].path, "C:\\a.txt");
        assert_eq!(s.items.len(), 2);
        assert!(s.remove("C:\\B.png"));
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn caps_items() {
        let mut s = Shelf::default();
        let paths: Vec<String> = (0..40).map(|i| format!("C:\\f{i}")).collect();
        s.add_paths(&paths, 0);
        assert_eq!(s.items.len(), MAX_SHELF_ITEMS);
        assert_eq!(s.items[0].path, "C:\\f39");
    }

    #[test]
    fn names_and_extensions() {
        assert_eq!(
            display_name("C:\\Users\\x\\report.final.PDF"),
            "report.final.PDF"
        );
        assert_eq!(extension("C:\\Users\\x\\report.final.PDF"), "pdf");
        assert_eq!(display_name("C:\\Projects\\"), "Projects");
        assert_eq!(extension("C:\\Projects"), "");
        assert_eq!(extension("C:\\.gitignore"), "");
    }
}
