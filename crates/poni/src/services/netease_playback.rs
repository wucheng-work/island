//! 网易云 3.x 的原生播放事件。只读本机增量日志，不依赖可见窗口或进程内存偏移。
//! 歌曲、状态必须与所选系统媒体会话一致；只用真实 load/seek 事件建立位置。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use poni_core::lrclib::fold;
use serde_json::Value;
use windows::Win32::Foundation::{CloseHandle, BOOL, LPARAM, TRUE};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowThreadProcessId};

const MAX_READ: u64 = 4 << 20;
const MAX_LINE: usize = 256 << 10;

#[derive(Clone)]
struct Track {
    id: String,
    title: String,
    artist: String,
    artists: Vec<String>,
    album: String,
    duration_s: f64,
}

impl Track {
    fn parse(value: &Value) -> Option<Self> {
        let track = value.get("track").unwrap_or(value);
        let id = song_id(track.get("id")?)?;
        let title = fold(track.get("name")?.as_str()?);
        if title.is_empty() { return None; }
        let artists: Vec<String> = track.get("artists")?.as_array()?.iter()
            .filter_map(|artist| artist.get("name")?.as_str().map(fold)).collect();
        let duration_s = track.get("duration")?.as_f64()? / 1000.0;
        if !duration_s.is_finite() || duration_s <= 0.0 { return None; }
        Some(Self {
            id, title, artist: fold(&artists.join("/")), artists,
            album: track.pointer("/album/name").and_then(Value::as_str).unwrap_or_default().to_string(),
            duration_s,
        })
    }
}

pub struct Playback {
    pub id: String,
    pub album: String,
    pub duration_s: f64,
    pub position_s: Option<f64>,
}

#[derive(Default)]
struct Playing {
    id: String,
    track: Option<Track>,
    position_s: Option<f64>,
    playing: Option<bool>,
    anchored: bool,
    at_ms: u64,
    last_event_ms: u64,
}

impl Playing {
    fn position_at(&self, now_ms: u64) -> Option<f64> {
        let mut position = self.position_s?;
        if self.playing == Some(true) {
            position += now_ms.saturating_sub(self.at_ms) as f64 / 1000.0;
        }
        if let Some(track) = &self.track { position = position.min(track.duration_s); }
        Some(position)
    }

    fn line(&mut self, line: &str, pid: u32, now_ms: u64) {
        let Some((header, body)) = line.strip_prefix('[').and_then(|line| line.split_once(']')) else { return };
        let mut parts = header.split(':');
        if parts.next().and_then(|pid| pid.parse::<u32>().ok()) != Some(pid) { return; }
        // 此版本的 header 为 pid:tid:MMdd/HHmmss:开机毫秒数。使用同一时基
        // 回放历史事件，不能把读到文件的时刻当作实际开始播放的时刻。
        let Some(at_ms) = parts.nth(2).and_then(|value| value.parse::<u64>().ok()) else { return };
        if at_ms > now_ms.saturating_add(1000) || at_ms < self.last_event_ms { return; }
        if body.contains("\"exitApp\"") {
            *self = Self { last_event_ms: at_ms, ..Self::default() };
            return;
        }
        // 只处理白名单播放事件；音频地址、Cookie、正文等不进入状态或应用日志。
        let Some((_, body)) = body.split_once("【playing】,") else { return };
        let Some((event, args)) = body.trim().strip_prefix('"').and_then(|body| body.split_once("\",")) else { return };
        if matches!(event, "playOneTrackInPlayingList" | "checkPlayPrivilege") {
            let Some(track) = serde_json::from_str::<Value>(args).ok().as_ref().and_then(Track::parse) else { return };
            // 权限检查也用于预载下一首，不能拿它替换仍在播放的歌曲。
            if event == "checkPlayPrivilege" && self.id != track.id { return; }
            if self.id != track.id {
                *self = Self { id: track.id.clone(), last_event_ms: at_ms, ..Self::default() };
            }
            self.track = Some(track);
        } else if event.starts_with("native") && event.contains("load完成") {
            let Some(id) = serde_json::from_str::<Value>(args).ok().and_then(|value| song_id(value.get("songId")?)) else { return };
            if self.id != id { *self = Self { id, ..Self::default() }; }
            // seek/暂停恢复也会重新加载同一 ID，不能一律归零。
            // 已到曲尾的再次 load 才按单曲循环重新起表。
            let position = self.position_at(at_ms);
            let keep_position = self.anchored && position.is_some_and(|position| {
                self.track.as_ref().is_some_and(|track| position < track.duration_s - 0.5)
            });
            self.position_s = if keep_position { position } else { Some(0.0) };
            self.anchored = keep_position;
            self.playing = Some(true);
            self.at_ms = at_ms;
        } else if event == "setPlayingPosition" && !self.id.is_empty() {
            let Some(position) = args.split(',').next().and_then(|value| value.trim().parse::<f64>().ok())
                .filter(|position| position.is_finite() && *position >= 0.0) else { return };
            self.position_s = Some(position);
            self.anchored = true;
            self.at_ms = at_ms;
        } else if event.starts_with("native") && event.ends_with("state") && !self.id.is_empty() {
            let Ok(values) = serde_json::from_str::<Vec<Value>>(&format!("[{args}]")) else { return };
            let Some(state) = values.first().and_then(Value::as_u64).filter(|state| matches!(state, 1 | 2)) else { return };
            if let Some(play_id) = values.get(1).and_then(Value::as_str) {
                if play_id.split('_').next() != Some(self.id.as_str()) { return; }
            }
            self.position_s = self.position_at(at_ms);
            self.anchored = true;
            self.playing = Some(state == 1);
            self.at_ms = at_ms;
        } else {
            return;
        }
        self.last_event_ms = at_ms;
    }
}

#[derive(Default)]
pub struct NeteasePlayback {
    pid: u32,
    offset: u64,
    prefix: Vec<u8>,
    carry: Vec<u8>,
    skip_line: bool,
    state: Playing,
    queue_stamps: Vec<Option<(SystemTime, u64)>>,
    tracks: Vec<Track>,
    pub status: &'static str,
}

impl NeteasePlayback {
    pub fn read(&mut self, title: &str, artist: &str, playing: bool) -> Option<Playback> {
        self.status = "process_unavailable";
        let Some(pid) = netease_pid() else { *self = Self::default(); self.status = "process_unavailable"; return None };
        if self.pid != pid { *self = Self { pid, ..Self::default() }; }
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("NetEase/CloudMusic");
        self.status = "playback_log_unavailable";
        let now_ms = unsafe { GetTickCount64() };
        self.read_log(&root.join("cloudmusic.elog"), now_ms)?;
        self.status = "track_not_loaded";
        if self.state.id.is_empty() { return None; }
        if self.state.track.is_none() {
            self.load_queue(&root);
            self.state.track = self.tracks.iter().find(|track| track.id == self.state.id).cloned();
        }
        self.status = "recording_not_in_playing_list";
        let track = self.state.track.as_ref()?;
        self.status = "system_metadata_pending";
        let title = fold(title);
        let artist = fold(artist);
        // 系统会话可能比日志先更新，反之亦然。过渡期间不把上一首或预载曲
        // 的 ID、进度绑定到新歌；合唱沿用完整歌手串或成员匹配语义。
        if title != track.title || artist.is_empty() || (artist != track.artist && !track.artists.contains(&artist)) { return None; }
        let position_s = if self.state.playing == Some(playing) {
            let position = self.state.position_at(now_ms);
            self.status = if position.is_some() { "ready" } else { "position_unavailable" };
            position
        } else {
            // SMTC 与日志写入可相差一轮。歌曲身份仍有效，位置等状态一致后
            // 再使用；调用方有限重试，不能因暂停而丢掉同曲歌词。
            self.status = "playback_state_pending";
            None
        };
        Some(Playback { id: track.id.clone(), album: track.album.clone(), duration_s: track.duration_s, position_s })
    }

    fn read_log(&mut self, path: &Path, now_ms: u64) -> Option<()> {
        let mut file = File::open(path).ok()?;
        let size = file.metadata().ok()?.len();
        let mut prefix = vec![0; size.min(64) as usize];
        file.read_exact(&mut prefix).ok()?;
        if size < self.offset || !prefix.starts_with(&self.prefix) {
            // 截断或替换日志后重建状态；旧曲目和旧进度不能跨文件沿用。
            self.offset = 0;
            self.carry.clear();
            self.skip_line = false;
            self.state = Playing::default();
        }
        self.prefix = prefix;
        if size == self.offset { return Some(()); }
        let start = self.offset.max(size.saturating_sub(MAX_READ));
        if start > self.offset {
            // 启动或长时间休眠后最多回放 4 MiB，跳过被截断的首行。
            self.state = Playing::default();
            self.carry.clear();
            self.skip_line = true;
        }
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.take(size - start).read_to_end(&mut bytes).ok()?;
        self.offset = start + bytes.len() as u64;
        for byte in bytes.into_iter().map(decode) {
            if byte == b'\n' {
                if !self.skip_line {
                    if let Ok(line) = std::str::from_utf8(&self.carry) { self.state.line(line, self.pid, now_ms); }
                }
                self.carry.clear();
                self.skip_line = false;
            } else if !self.skip_line {
                if self.carry.len() < MAX_LINE { self.carry.push(byte); }
                else { self.carry.clear(); self.skip_line = true; }
            }
        }
        Some(())
    }

    fn load_queue(&mut self, root: &Path) {
        let paths = [root.join("webdata/file/playingList"), root.join("webdata/file/fmPlay")];
        let stamps: Vec<_> = paths.iter().map(|path| {
            let metadata = std::fs::metadata(path).ok()?;
            Some((metadata.modified().ok()?, metadata.len()))
        }).collect();
        if self.queue_stamps == stamps { return; }
        let mut tracks = Vec::new();
        let mut readable = true;
        for (index, (path, key)) in paths.iter().zip(["list", "queue"]).enumerate() {
            if stamps[index].is_none() { continue; }
            let value = (|| {
                let file = File::open(path).ok()?;
                if file.metadata().ok()?.len() > MAX_READ { return None; }
                let mut bytes = Vec::new();
                file.take(MAX_READ + 1).read_to_end(&mut bytes).ok()?;
                if bytes.len() as u64 > MAX_READ { return None; }
                serde_json::from_slice::<Value>(&bytes).ok()
            })();
            if let Some(list) = value.as_ref().and_then(|value| value.get(key)).and_then(Value::as_array) {
                tracks.extend(list.iter().filter_map(Track::parse));
            } else { readable = false; }
        }
        self.tracks = tracks;
        // 文件正写入或暂时不可读时不缓存失败，下次正常媒体刷新可重试。
        if readable { self.queue_stamps = stamps; }
    }
}

fn song_id(value: &Value) -> Option<String> {
    let id = value.as_str().map(str::to_string).or_else(|| value.as_u64().map(|id| id.to_string()))?;
    poni_core::nowplaying::netease_song_id(&id)?;
    Some(id)
}

// 逐字节映射参考岛仔及 netease-cloudmusic-detector-py；许可见 THIRD-PARTY-NOTICES.txt。
fn decode(byte: u8) -> u8 {
    let high = byte >> 4;
    let low = byte & 15;
    ((high ^ ((low + 8) & 15)) << 4) | ((byte >> 6) << 2) | (!high & 3)
}

fn netease_pid() -> Option<u32> {
    unsafe extern "system" fn visit(hwnd: windows::Win32::Foundation::HWND, parameter: LPARAM) -> BOOL {
        let result = &mut *(parameter.0 as *mut Option<u32>);
        if result.is_some() { return TRUE; }
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        if String::from_utf16_lossy(&class[..len]) != "OrpheusBrowserHost" { return TRUE; }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return TRUE };
        let mut path = [0u16; 1024];
        let mut size = path.len() as u32;
        let queried = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(path.as_mut_ptr()), &mut size);
        let _ = CloseHandle(process);
        if queried.is_ok() && String::from_utf16_lossy(&path[..size as usize]).rsplit('\\')
            .next().is_some_and(|name| name.eq_ignore_ascii_case("cloudmusic.exe")) {
            *result = Some(pid);
        }
        TRUE
    }
    let mut pid = None;
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut pid as *mut Option<u32> as isize)) };
    pid
}
