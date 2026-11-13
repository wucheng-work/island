//! 当前播放器共用的歌词记录、名称比较和稳定缓存键。
//!
//! 模块名沿用既有缓存格式；网络查询由对应播放器服务负责。

use crate::lrc::{parse_lrc, Lyrics};
use serde::{Deserialize, Serialize};

/// 少于四行正文视为占位内容，不作为完整同步歌词。
pub const MIN_LINES: usize = 4;
/// 同专辑录音允许的时长偏差。
pub const MAX_SYNC_DRIFT_S: f64 = 6.0;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackQuery {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Whole seconds; 0 when unknown.
    pub duration_s: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrclibRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub track_name: String,
    #[serde(default)]
    pub artist_name: String,
    #[serde(default)]
    pub album_name: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub instrumental: bool,
    #[serde(default)]
    pub plain_lyrics: Option<String>,
    #[serde(default)]
    pub synced_lyrics: Option<String>,
}

impl LrclibRecord {
    /// 完整同步歌词只解析一次，调用方直接复用结果用于显示。
    pub fn full_synced_lyrics(&self) -> Option<Lyrics> {
        let lyrics = parse_lrc(self.synced_lyrics.as_deref()?);
        if lyrics.synced && lyrics.text_lines() >= MIN_LINES {
            Some(lyrics)
        } else {
            None
        }
    }
}

/// Whether lyrics timed for a `record_s`-second recording line up with a
/// `playing_s`-second track. Unknown lengths pass.
pub fn timing_fits(record_s: Option<f64>, playing_s: u32) -> bool {
    match record_s {
        Some(d) if d > 0.0 && playing_s > 0 => (d - playing_s as f64).abs() <= MAX_SYNC_DRIFT_S,
        _ => true,
    }
}

/// Lowercase, strip punctuation and collapse whitespace for comparisons.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim_end().to_string()
}

/// Stable file-name-safe cache key for a query.
pub fn cache_key(q: &TrackQuery) -> String {
    // FNV-1a 64-bit over the folded fields.
    let mut h: u64 = 0xcbf29ce484222325;
    for part in [fold(&q.artist), fold(&q.title), q.duration_s.to_string()] {
        for b in part.bytes().chain(std::iter::once(0u8)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(title: &str, artist: &str, album: &str, d: u32) -> TrackQuery {
        TrackQuery { title: title.into(), artist: artist.into(), album: album.into(), duration_s: d }
    }

    #[test]
    fn stubs_have_no_full_synced_lyrics() {
        let rec = |synced: &str| LrclibRecord { synced_lyrics: Some(synced.into()), ..Default::default() };
        assert!(rec("[00:00.00]Paper Boats").full_synced_lyrics().is_none());
        assert!(rec("[00:01.00]one\n[00:02.00]two\n[00:03.00]three\n[00:04.00]four").full_synced_lyrics().is_some());
        assert!(rec("[00:01.00]one\n[00:02.00]\n[00:03.00]\n[00:04.00]two").full_synced_lyrics().is_none());
    }

    #[test]
    fn timing_fits_close_lengths_only() {
        assert!(timing_fits(Some(201.0), 205));
        assert!(timing_fits(Some(201.0), 195));
        assert!(!timing_fits(Some(201.0), 230));
        assert!(!timing_fits(Some(201.0), 180));
        // Unknown on either side: nothing to compare.
        assert!(timing_fits(None, 230));
        assert!(timing_fits(Some(201.0), 0));
        assert!(timing_fits(Some(0.0), 230));
    }

    #[test]
    fn record_tolerates_missing_fields() {
        let record: LrclibRecord = serde_json::from_str(r#"{"id":7,"trackName":"T","artistName":"A"}"#).unwrap();
        assert_eq!(record.id, 7);
        assert!(record.full_synced_lyrics().is_none());
    }

    #[test]
    fn cache_key_is_stable_and_safe() {
        let a = cache_key(&q("Paper Boats", "The Lanterns", "x", 201));
        let b = cache_key(&q("paper boats!", "the lanterns", "y", 201));
        assert_eq!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
