//! Synced lyrics from the current player's own service, with a disk cache.
//!
//! One worker thread does all the work; [`LyricsService::request`] only swaps
//! the queued job and returns. The latest request wins: a newer track replaces any queued job,
//! aborts the running lookup at its next step, and results for a track that
//! is no longer the newest are dropped.
//!
//! QQ Music and NetEase use unofficial web endpoints without account credentials.
//! Apple Music requires the user's saved token. Other players return no lyrics;
//! a failed lookup never switches to another player's lyrics service.

use crate::events::{AppEvent, AppleMusicStatus, EventSink, LyricsResult};
use crate::services::applemusic::{AppleMusic, Check, Miss};
use crate::services::http::{self, Client, FetchError, MtaGuard};
use crate::state::MediaSession;
use poni_core::lrclib::{self, LrclibRecord, TrackQuery};
use poni_core::nowplaying::netease_song_id;
use poni_core::sha256;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use pure::{duration_secs, lyrics_for, prune_victims, retry_delay, CacheEntry};

// The web endpoint returned code 2001 for OyenzaStudio's User-Agent.
const WEB_USER_AGENT: &str = "Mozilla/5.0";
/// A different album needs a much closer recording length before it can win.
const ALIAS_MAX_DRIFT_S: f64 = 2.0;
/// 429 without a usable `Retry-After`.
const DEFAULT_BACKOFF: Duration = Duration::from_secs(60);
/// Upper bound for any server-requested back-off.
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);
/// Delayed retries after transient failures (while the track is still current).
const MAX_RETRIES: u32 = 2;
/// Windows 缺少录音信息时必须检查完整候选，最多三页，避免模糊搜索无限展开。
const NETEASE_SEARCH_PAGE_SIZE: usize = 100;
const NETEASE_SEARCH_MAX_PAGES: usize = 3;
/// Cache size bounds: prune to `CACHE_KEEP` files once over `CACHE_MAX`.
const CACHE_MAX: usize = 2000;
const CACHE_KEEP: usize = 1600;
/// Re-check the cache size after this many writes (long-running sessions).
const PRUNE_EVERY_WRITES: u32 = 200;

pub struct LyricsService {
    shared: Arc<Shared>,
}

impl LyricsService {
    pub fn start(sink: EventSink) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("poni-lyrics".into())
            .spawn(move || worker_main(worker, sink))
            .map_err(|e| io::Error::other(format!("could not start the lyrics thread: {e}")))?;
        Ok(Self { shared })
    }

    /// Queue a lookup for `s` and return immediately (UI thread safe). The
    /// answer carries the returned generation as well as the track and player.
    pub fn request(&self, s: &MediaSession) -> u64 {
        let generation = {
            let mut q = self.shared.lock();
            q.generation = q.generation.wrapping_add(1);
            q.pending = Some(Job {
                generation: q.generation,
                track_key: s.track_key.clone(),
                app_id: s.app_id.clone(),
                content_id: s.content_id.clone(),
                query: TrackQuery {
                    title: s.title.clone(),
                    artist: s.artist.clone(),
                    album: s.album.clone(),
                    duration_s: duration_secs(s.timeline.duration_s),
                },
                attempt: 0,
            });
            q.retry = None;
            q.generation
        };
        self.shared.cv.notify_all();
        generation
    }

    /// Stop queued work and make any result already in flight stale.
    pub fn cancel(&self) {
        {
            let mut q = self.shared.lock();
            q.generation = q.generation.wrapping_add(1);
            q.pending = None;
            q.retry = None;
        }
        self.shared.cv.notify_all();
    }

    /// Pause automatic retries while the session or power state asks for rest.
    pub fn set_rest(&self, rest: bool) {
        {
            let mut q = self.shared.lock();
            if q.rest == rest {
                return;
            }
            q.rest = rest;
        }
        self.shared.cv.notify_all();
    }

    /// The Apple Music token on disk was saved or removed: the worker reloads
    /// it, checks it with Apple and reports `AppEvent::AppleMusic`.
    pub fn apple_token_changed(&self) {
        self.shared.lock().apple_changed = true;
        self.shared.cv.notify_all();
    }
}

impl Drop for LyricsService {
    fn drop(&mut self) {
        // The worker exits at its next wait; a request in flight finishes
        // (bounded by the HTTP timeout) on its own. Never join from the UI thread.
        self.shared.lock().shutdown = true;
        self.shared.cv.notify_all();
    }
}

// ------------------------------------------------------------------ queue

#[derive(Clone, Debug)]
struct Job {
    generation: u64,
    track_key: String,
    app_id: String,
    content_id: String,
    query: TrackQuery,
    /// 0 for a fresh request, n for the n-th delayed retry.
    attempt: u32,
}

impl Job {
    /// 只记录这两个播放器的本机诊断；不输出原始content_id、URL或响应正文。
    fn diagnostic(&self, stage: &str, detail: std::fmt::Arguments<'_>) {
        let player = if poni_core::nowplaying::is_qq_music(&self.app_id) {
            "qq"
        } else if poni_core::nowplaying::is_netease_music(&self.app_id) {
            "netease"
        } else {
            return;
        };
        crate::pinfo!("lyrics diag player={player} request={} attempt={} stage={stage} {detail}", self.generation, self.attempt);
    }

    fn diagnostic_candidate(&self, stage: &str, record: &LrclibRecord, reason: &str) {
        self.diagnostic(stage, format_args!(
            "reason={reason} candidate_id={} title={:?} artist={:?} album={:?} duration_s={:?}",
            record.id, diagnostic_text(&record.track_name), diagnostic_text(&record.artist_name),
            diagnostic_text(record.album_name.as_deref().unwrap_or_default()), record.duration,
        ));
    }
}

/// Debug格式会转义换行和控制字符，限长避免异常元数据挤满日志。
fn diagnostic_text(text: &str) -> String {
    let mut short: String = text.chars().take(120).collect();
    if text.chars().nth(120).is_some() {
        short.push('…');
    }
    short
}

/// What the worker should do next.
enum Work {
    Lookup(Job),
    /// Reload and check the Apple Music token.
    AppleToken,
}

#[derive(Default)]
struct Queue {
    /// The next job; a new request replaces it (latest wins).
    pending: Option<Job>,
    /// A delayed retry of the current track, run once its time comes.
    retry: Option<(Instant, Job)>,
    /// Every request has its own identity, even for the same song/player.
    generation: u64,
    /// The Apple Music token was saved or removed (handled before any job).
    apple_changed: bool,
    rest: bool,
    shutdown: bool,
}

#[derive(Default)]
struct Shared {
    q: Mutex<Queue>,
    cv: Condvar,
}

impl Shared {
    /// The queue holds plain data, so a panic elsewhere can't leave it
    /// inconsistent: recover from poisoning instead of panicking.
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.q.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn wait_for<'a>(&self, g: MutexGuard<'a, Queue>, timeout: Option<Duration>) -> MutexGuard<'a, Queue> {
        match timeout {
            Some(d) => match self.cv.wait_timeout(g, d) {
                Ok((g, _)) => g,
                Err(p) => p.into_inner().0,
            },
            None => self.cv.wait(g).unwrap_or_else(|p| p.into_inner()),
        }
    }
}

// ----------------------------------------------------------------- worker

/// Why a lookup stopped without an answer.
enum Halt {
    /// Superseded by a newer track, or shutting down: say nothing.
    Abandoned,
    /// Report the failure now; an eligible retry can still recover later.
    Failed { msg: String, retry_at: Option<Instant> },
}

fn worker_main(shared: Arc<Shared>, sink: EventSink) {
    // Declared first so it is dropped last, after the worker's HttpClient.
    let _com = MtaGuard::new();
    let cache_dir = crate::paths::lyrics_cache_dir();
    prune_cache(&cache_dir);
    let apple = AppleMusic::new();
    let mut w = Worker {
        shared, sink, cache_dir, apple,
        qq_client: None, qq_blocked_until: None,
        netease_client: None, netease_blocked_until: None,
        writes: 0,
    };
    while let Some(work) = w.next_job() {
        match work {
            Work::Lookup(job) => w.process(job),
            Work::AppleToken => w.apple_token_changed(),
        }
    }
}

struct Worker {
    shared: Arc<Shared>,
    sink: EventSink,
    cache_dir: PathBuf,
    apple: AppleMusic,
    /// Created on first use; reused so connections are pooled.
    qq_client: Option<Client>,
    /// A 429 window applies to every QQ endpoint, even after the track changes.
    qq_blocked_until: Option<Instant>,
    netease_client: Option<Client>,
    netease_blocked_until: Option<Instant>,
    /// Cache writes since the last prune.
    writes: u32,
}

impl Worker {
    /// Sleep until there is work: a changed Apple Music token first, then a
    /// queued job or a delayed retry that is due; `None` on shutdown.
    fn next_job(&self) -> Option<Work> {
        let mut q = self.shared.lock();
        loop {
            if q.shutdown {
                return None;
            }
            if std::mem::take(&mut q.apple_changed) {
                return Some(Work::AppleToken);
            }
            if let Some(job) = q.pending.take() {
                return Some(Work::Lookup(job));
            }
            if q.rest {
                q = self.shared.wait_for(q, None);
                continue;
            }
            let timeout = match &q.retry {
                Some((at, _)) => {
                    let now = Instant::now();
                    if *at <= now {
                        return q.retry.take().map(|(_, job)| Work::Lookup(job));
                    }
                    Some(*at - now)
                }
                None => None,
            };
            q = self.shared.wait_for(q, timeout);
        }
    }

    fn is_current(&self, job: &Job) -> bool {
        let q = self.shared.lock();
        !q.shutdown && q.generation == job.generation
    }

    fn deliver(&self, job: &Job, result: LyricsResult) {
        if self.is_current(job) {
            self.sink.send(AppEvent::Lyrics { track_key: job.track_key.clone(), app_id: job.app_id.clone(), generation: job.generation, result });
        }
    }

    fn process(&mut self, job: Job) {
        let started = Instant::now();
        job.diagnostic("start", format_args!(
            "title={:?} artist={:?} album={:?} duration_s={} netease_song_id={:?}",
            diagnostic_text(&job.query.title), diagnostic_text(&job.query.artist),
            diagnostic_text(&job.query.album), job.query.duration_s, netease_song_id(&job.content_id),
        ));
        match self.lookup(&job) {
            Ok(result) => {
                match &result {
                    LyricsResult::Found { lyrics } => job.diagnostic("finish", format_args!(
                        "result=found synced={} text_lines={} elapsed_ms={}", lyrics.synced, lyrics.text_lines(), started.elapsed().as_millis(),
                    )),
                    LyricsResult::Instrumental => job.diagnostic("finish", format_args!("result=instrumental elapsed_ms={}", started.elapsed().as_millis())),
                    LyricsResult::NotFound => job.diagnostic("finish", format_args!("result=not_found elapsed_ms={}", started.elapsed().as_millis())),
                    LyricsResult::Retrying(_) => job.diagnostic("retry", format_args!("result=pending elapsed_ms={}", started.elapsed().as_millis())),
                    LyricsResult::Error(_) => job.diagnostic("finish", format_args!("result=error elapsed_ms={}", started.elapsed().as_millis())),
                }
                self.deliver(&job, result);
            }
            Err(Halt::Abandoned) => job.diagnostic("finish", format_args!("result=cancelled reason=superseded_or_shutdown elapsed_ms={}", started.elapsed().as_millis())),
            Err(Halt::Failed { msg, retry_at }) => {
                if let Some(at) = retry_at {
                    if self.schedule_retry(&job, at) {
                        job.diagnostic("retry", format_args!("result=scheduled next_attempt={} wait_ms={} reason={:?}", job.attempt + 1, at.saturating_duration_since(Instant::now()).as_millis(), diagnostic_text(&msg)));
                        // 退避期间不是网络请求；说明上游故障，重试仍可用同一代次交付歌词。
                        self.deliver(&job, LyricsResult::Retrying(format!("{msg}，稍后自动重试")));
                        return;
                    }
                }
                job.diagnostic("finish", format_args!("result=failed elapsed_ms={} reason={:?}", started.elapsed().as_millis(), diagnostic_text(&msg)));
                self.deliver(&job, LyricsResult::Error(msg));
            }
        }
    }

    fn schedule_retry(&self, job: &Job, at: Instant) -> bool {
        if job.attempt >= MAX_RETRIES {
            return false;
        }
        let mut q = self.shared.lock();
        if q.shutdown || q.generation != job.generation || q.pending.is_some() {
            return false;
        }
        q.retry = Some((at, Job { attempt: job.attempt + 1, ..job.clone() }));
        // The worker itself is the only waiter and it is not waiting right
        // now, so no notify is needed.
        true
    }

    /// The user saved or removed their Apple Music token: reload it and tell
    /// the UI whether Apple accepts it.
    fn apple_token_changed(&mut self) {
        self.apple.reload_token();
        if !self.apple.has_token() {
            return;
        }
        let shared = self.shared.clone();
        let live = move || !shared.lock().shutdown;
        let status = match self.apple.check(&live) {
            Check::Accepted => AppleMusicStatus::Accepted,
            Check::Rejected => AppleMusicStatus::Rejected,
            Check::Unreachable => AppleMusicStatus::Unreachable,
        };
        self.sink.send(AppEvent::AppleMusic(status));
    }

    fn lookup(&mut self, job: &Job) -> Result<LyricsResult, Halt> {
        // Native player metadata describes a recording; keep version tags.
        let q = TrackQuery {
            title: job.query.title.trim().to_string(),
            artist: job.query.artist.trim().to_string(),
            album: job.query.album.trim().to_string(),
            duration_s: job.query.duration_s,
        };
        if q.title.trim().is_empty() {
            // Nothing to search for; answer so the UI doesn't sit on "loading".
            job.diagnostic("metadata", format_args!("reason=missing_title"));
            return Ok(LyricsResult::NotFound);
        }
        if poni_core::nowplaying::is_qq_music(&job.app_id) {
            return self.lookup_qq(job, &q);
        }
        if poni_core::nowplaying::is_netease_music(&job.app_id) {
            return self.lookup_netease(job, &q);
        }
        if (job.app_id == "com.apple.Music" || poni_core::nowplaying::is_apple_music(&job.app_id)) && self.apple.enabled() {
            return Ok(self.lookup_apple(job, &q)?.unwrap_or(LyricsResult::NotFound));
        }
        Ok(LyricsResult::NotFound)
    }

    /// Apple Music's synced lyrics, only for an Apple Music playback session.
    fn lookup_apple(&mut self, job: &Job, q: &TrackQuery) -> Result<Option<LyricsResult>, Halt> {
        let album_key = sha256::hex(&sha256::sha256(lrclib::fold(&q.album).as_bytes()));
        let key = format!("am-v2-{}-{album_key}", lrclib::cache_key(q));
        let path = self.cache_dir.join(format!("{key}.json"));
        if let Some(record) = read_cache(&path).and_then(|entry| entry.record) {
            if recording_matches(q, &record) {
                if let Some(lyrics) = record.full_synced_lyrics() {
                    return Ok(Some(to_result(&record, lyrics, job)));
                }
            }
        }
        let (shared, generation) = (self.shared.clone(), job.generation);
        let live = move || {
            let q = shared.lock();
            !q.shutdown && q.generation == generation
        };
        match self.apple.find(q, &live) {
            Ok(Some((record, lyrics))) => {
                crate::pinfo!("lyrics {key}: synced");
                self.store(&path, &CacheEntry::new(Some(record.clone()), crate::clock::unix_now()));
                Ok(Some(to_result(&record, lyrics, job)))
            }
            Ok(None) => {
                crate::pinfo!("lyrics {key}: no synced lyrics on Apple Music");
                Ok(None)
            }
            Err(Miss::Abandoned) => Err(Halt::Abandoned),
            Err(Miss::Rejected) => {
                self.sink.send(AppEvent::AppleMusic(AppleMusicStatus::Rejected));
                Err(Halt::Failed { msg: "Apple Music 授权已失效，请重新授权".into(), retry_at: None })
            }
            Err(Miss::NoAccess) => {
                self.sink.send(AppEvent::AppleMusic(AppleMusicStatus::NoAccess));
                Err(Halt::Failed { msg: "Apple Music 未允许访问这首歌的歌词".into(), retry_at: None })
            }
            Err(Miss::Unreachable(why)) => {
                crate::pwarn!("lyrics: Apple Music unavailable ({why})");
                Err(Halt::Failed { msg: "暂时无法连接 Apple Music 歌词服务".into(), retry_at: Some(Instant::now() + retry_delay(job.attempt)) })
            }
        }
    }

    fn lookup_qq(&mut self, job: &Job, q: &TrackQuery) -> Result<LyricsResult, Halt> {
        let album = lrclib::fold(&q.album);
        if q.artist.trim().is_empty() || (album.is_empty() && q.duration_s == 0) {
            job.diagnostic("metadata", format_args!("reason=insufficient_metadata artist_missing={} album_missing={} duration_missing={}", q.artist.trim().is_empty(), album.is_empty(), q.duration_s == 0));
            return Ok(LyricsResult::NotFound);
        }
        // A match from a compilation belongs to this query's album only.
        let legacy_key = format!("qq-{}", lrclib::cache_key(q));
        let album_key = sha256::hex(&sha256::sha256(album.as_bytes()));
        let key = format!("{legacy_key}-{album_key}");
        let path = self.cache_dir.join(format!("{key}.json"));
        if let Some(record) = read_cache(&path).and_then(|entry| entry.record) {
            if recording_matches(q, &record) {
                if let Some(lyrics) = record.full_synced_lyrics() {
                    job.diagnostic("cache", format_args!("result=hit kind=album candidate_id={}", record.id));
                    return Ok(to_result(&record, lyrics, job));
                }
            }
        }
        // Global legacy keys cannot prove uniqueness for another/unknown album.
        if !album.is_empty() {
            let legacy_path = self.cache_dir.join(format!("{legacy_key}.json"));
            if let Some(record) = read_cache(&legacy_path).and_then(|entry| entry.record) {
                if recording_matches(q, &record)
                    && record.album_name.as_deref().is_some_and(|name| lrclib::fold(name) == album)
                {
                    if let Some(lyrics) = record.full_synced_lyrics() {
                        job.diagnostic("cache", format_args!("result=hit kind=legacy candidate_id={}", record.id));
                        return Ok(to_result(&record, lyrics, job));
                    }
                }
            }
        }
        job.diagnostic("cache", format_args!("result=miss_or_rejected"));
        let mut best_album_match: Option<(QqSong, LrclibRecord, u32)> = None;
        let mut album_match_count = 0;
        let mut best_distance_count = 0;
        let mut other_album_matches = Vec::new();
        let mut seen_mids = HashSet::new();
        // QQ 的通用搜索会把精选集录音排除在前列；无本专辑匹配时再带专辑检索。
        // 歌曲搜索均无匹配时，再查同名歌手的唯一同名专辑曲目，补足旧搜索漏收的录音。
        // 所有结果按 mid 去重，仍校验歌名、歌手、版本和时长，不放宽匹配条件。
        let search_query = format!("{} {}", q.title, q.artist);
        let album_query = format!("{} {}", search_query, q.album);
        for round in 0..3 {
            if round > 0 && (album.is_empty() || best_album_match.is_some()) {
                break;
            }
            let songs = match round {
                0 => self.search_qq(job, &search_query)?,
                1 => {
                    crate::pinfo!("lyrics: QQ Music trying album-qualified search");
                    self.search_qq(job, &album_query)?
                }
                _ => {
                    if !other_album_matches.is_empty() {
                        break;
                    }
                    crate::pinfo!("lyrics: QQ Music trying exact album track list");
                    self.qq_album_songs(job, q)?
                }
            };
            let candidate_count = songs.len();
            let mut rejected = BTreeMap::new();
            let mut sampled = 0;
            let mut accepted = 0;
            for song in songs {
                if song.mid.trim().is_empty() {
                    return Err(qq_bad_reply(job, "search candidates"));
                }
                let record = LrclibRecord {
                    id: song.id,
                    // QQ's name can omit version tags that remain in title.
                    track_name: if song.title.trim().is_empty() { song.name.trim().to_string() } else { song.title.trim().to_string() },
                    artist_name: song.singer.iter().map(|singer| singer.name.trim()).collect::<Vec<_>>().join(" / "),
                    album_name: Some(song.album.name.trim().to_string()),
                    duration: Some(song.interval as f64),
                    ..Default::default()
                };
                if let Some(reason) = recording_mismatch(q, &record) {
                    *rejected.entry(reason).or_insert(0_usize) += 1;
                    if sampled < 3 {
                        job.diagnostic_candidate("candidate_rejected", &record, reason);
                        sampled += 1;
                    }
                    continue;
                }
                // 旧搜索可能缺少版本后缀；只对通过校验的录音去重，允许专辑补全同一mid。
                if !seen_mids.insert(song.mid.clone()) {
                    continue;
                }
                accepted += 1;
                if !album.is_empty() && record.album_name.as_deref().is_some_and(|name| lrclib::fold(name) == album) {
                    album_match_count += 1;
                    let difference = song.interval.abs_diff(q.duration_s);
                    if best_album_match.as_ref().is_none_or(|(_, _, previous)| difference < *previous) {
                        best_album_match = Some((song, record, difference));
                        best_distance_count = 1;
                    } else if best_album_match.as_ref().is_some_and(|(_, _, previous)| difference == *previous) {
                        best_distance_count += 1;
                    }
                } else if q.duration_s > 0 {
                    other_album_matches.push((song, record));
                }
            }
            job.diagnostic("search", format_args!("round={round} candidates={candidate_count} accepted_unique={accepted} rejected={rejected:?}"));
        }
        let selected = if let Some((song, record, _)) = best_album_match {
            if album_match_count == 1 || (q.duration_s > 0 && best_distance_count == 1) {
                Some((song, record))
            } else {
                job.diagnostic("selection", format_args!("reason=ambiguous_same_album album_matches={album_match_count} closest_matches={best_distance_count}"));
                crate::pinfo!("lyrics: QQ Music search has ambiguous matches within the requested album");
                None
            }
        } else if other_album_matches.len() == 1 {
            crate::pinfo!("lyrics: QQ Music search matched a unique recording from another album");
            other_album_matches.pop()
        } else {
            job.diagnostic("selection", format_args!("reason=no_unique_recording same_album=0 other_album={}", other_album_matches.len()));
            crate::pinfo!("lyrics: QQ Music search has no exact album and {} other recording matches", other_album_matches.len());
            None
        };
        let Some((song, mut record)) = selected else { return Ok(LyricsResult::NotFound) };
        job.diagnostic_candidate("selection", &record, "selected");
        // crypt=0 requests ordinary Base64-encoded LRC, without encrypted QRC.
        let request = serde_json::json!({
            "comm": {
                "ct": 24, "cv": 4747474, "platform": "yqq.json", "chid": "0",
                "g_tk": 5381, "format": "json", "inCharset": "utf-8",
                "outCharset": "utf-8", "notice": 0, "need_new_code": 1
            },
            "lyric": {
                "module": "music.musichallSong.PlayLyricInfo",
                "method": "GetPlayLyricInfo",
                "param": {
                    "crypt": 0, "lrc_t": 0, "qrc": 0, "qrc_t": 0,
                    "roma": 0, "roma_t": 0, "trans": 0, "trans_t": 0,
                    "needSingingAnnotations": false, "type": 1, "songMid": song.mid
                }
            }
        });
        let data = self.qq_call(job, &request, "lyric", None)?;
        let encoded = data.get("lyric").and_then(|value| value.as_str()).ok_or_else(|| qq_bad_reply(job, "lyric"))?;
        if encoded.trim().is_empty() {
            job.diagnostic("lyric", format_args!("reason=empty_lyric candidate_id={}", record.id));
            return Ok(LyricsResult::NotFound);
        }
        if data.get("crypt").and_then(|value| value.as_i64()).is_some_and(|crypt| crypt != 0) {
            job.diagnostic("lyric", format_args!("reason=unsupported_encryption candidate_id={}", record.id));
            return Err(Halt::Failed { msg: "QQ 音乐返回了不支持的歌词格式".into(), retry_at: None });
        }
        let text = poni_core::base64::decode(encoded).and_then(|bytes| String::from_utf8(bytes).ok());
        let Some(text) = text else {
            return Err(qq_bad_reply(job, "lyric encoding"));
        };
        record.synced_lyrics = Some(text);
        let Some(lyrics) = record.full_synced_lyrics() else {
            job.diagnostic("lyric", format_args!("reason=missing_timestamps_or_too_few_lines candidate_id={}", record.id));
            return Ok(LyricsResult::NotFound);
        };
        self.store(&path, &CacheEntry::new(Some(record.clone()), crate::clock::unix_now()));
        crate::pinfo!("lyrics {key}: synced from QQ Music");
        Ok(to_result(&record, lyrics, job))
    }

    /// 两种检索共用QQ搜索端点及既有备用端点，取消、超时与限流处理保持一致。
    fn search_qq(&mut self, job: &Job, query: &str) -> Result<Vec<QqSong>, Halt> {
        let search = serde_json::json!({
            "comm": { "ct": 19, "cv": 1859, "uin": "0" },
            "search": {
                "module": "music.search.SearchCgiService",
                "method": "DoSearchForQQMusicDesktop",
                "param": {
                    "query": query,
                    "num_per_page": 10,
                    "page_num": 1,
                    "search_type": 0
                }
            }
        });
        let data = self.qq_call(job, &search, "search", Some(query))?;
        let songs = data.pointer("/body/song/list").cloned().ok_or_else(|| qq_bad_reply(job, "search"))?;
        serde_json::from_value(songs).map_err(|_| qq_bad_reply(job, "search candidates"))
    }

    /// 旧歌曲搜索可能漏掉整张专辑的录音；只补查唯一且歌手一致的专辑，不猜测同名专辑。
    fn qq_album_songs(&mut self, job: &Job, q: &TrackQuery) -> Result<Vec<QqSong>, Halt> {
        let url = poni_core::urlenc::with_query(
            "https://c.y.qq.com/soso/fcgi-bin/client_search_cp",
            &[("w", &q.album), ("p", "1"), ("n", "10"), ("format", "json"), ("g_tk", "5381"), ("t", "8")],
        );
        let reply = self.qq_request(job, "album search", http::Method::Get, &url, None)?;
        if reply.get("code").and_then(|value| value.as_i64()) != Some(0) {
            return Err(qq_bad_reply(job, "album search"));
        }
        let albums = reply.pointer("/data/album/list").cloned().ok_or_else(|| qq_bad_reply(job, "album search"))?;
        let albums: Vec<QqAlbum> = serde_json::from_value(albums).map_err(|_| qq_bad_reply(job, "album candidates"))?;
        let album_name = lrclib::fold(&q.album);
        let artist = lrclib::fold(&q.artist);
        let mut mids = HashSet::new();
        let mut matches = albums.into_iter().filter(|album| {
            !album.mid.trim().is_empty()
                && lrclib::fold(&album.name) == album_name
                && album.singers.iter().any(|singer| lrclib::fold(&singer.name) == artist)
                && mids.insert(album.mid.clone())
        });
        let Some(album) = matches.next() else {
            job.diagnostic("album_search", format_args!("reason=no_exact_album_and_artist"));
            return Ok(Vec::new());
        };
        if matches.next().is_some() {
            job.diagnostic("album_search", format_args!("reason=ambiguous_album"));
            crate::pinfo!("lyrics: QQ Music album search has ambiguous matches");
            return Ok(Vec::new());
        }
        let request = serde_json::json!({
            "comm": { "ct": 24, "cv": 4747474, "format": "json" },
            "album": {
                "module": "music.musichallAlbum.AlbumSongList",
                "method": "GetAlbumSongList",
                "param": { "albumMid": album.mid, "begin": 0, "num": 100, "order": 2 }
            }
        });
        let data = self.qq_call(job, &request, "album", None)?;
        let album: QqAlbumSongs = serde_json::from_value(data).map_err(|_| qq_bad_reply(job, "album tracks"))?;
        // 未取得完整曲目时不能证明录音唯一，沿用无匹配结果而不猜测。
        if album.total != album.tracks.len() {
            job.diagnostic("album_tracks", format_args!("reason=incomplete_track_list expected={} received={}", album.total, album.tracks.len()));
            crate::pinfo!("lyrics: QQ Music returned a partial album track list");
            return Ok(Vec::new());
        }
        Ok(album.tracks.into_iter().map(|track| track.song).collect())
    }

    fn lookup_netease(&mut self, job: &Job, q: &TrackQuery) -> Result<LyricsResult, Halt> {
        let album = lrclib::fold(&q.album);
        let metadata_only = cfg!(target_os = "windows") && album.is_empty() && q.duration_s == 0;
        if q.artist.is_empty() || (!metadata_only && album.is_empty() && q.duration_s == 0) {
            job.diagnostic("metadata", format_args!("reason=insufficient_metadata artist_missing={} album_missing={} duration_missing={}", q.artist.is_empty(), album.is_empty(), q.duration_s == 0));
            return Ok(LyricsResult::NotFound);
        }
        if metadata_only {
            job.diagnostic("lookup", format_args!("path=metadata_only reason=missing_album_and_duration cache=bypassed"));
        }
        let album_key = sha256::hex(&sha256::sha256(album.as_bytes()));
        let song_id = netease_song_id(&job.content_id);
        // 临时 UUID 会在同曲恢复时变化；缓存只区分可信歌曲 ID，未知时保留严格元数据匹配。
        let content_key = song_id.map(|id| id.to_string()).unwrap_or_else(|| "search".into());
        let key = format!("netease-v3-{}-{album_key}-{content_key}", lrclib::cache_key(q));
        let path = self.cache_dir.join(format!("{key}.json"));
        // 没有录音标识时，每次重新检查候选歧义，不能把旧的唯一结果长期当成当前录音。
        let cached_record = if metadata_only { None } else { read_cache(&path).and_then(|entry| entry.record) };
        if let Some(record) = cached_record {
            if song_id.is_none_or(|id| record.id == id) && recording_matches(q, &record) {
                if let Some(lyrics) = record.full_synced_lyrics() {
                    job.diagnostic("cache", format_args!("result=hit candidate_id={}", record.id));
                    return Ok(to_result(&record, lyrics, job));
                }
            }
        }
        job.diagnostic("cache", format_args!("result=miss_or_rejected"));
        let selected = if let Some(id) = song_id {
            job.diagnostic("lookup", format_args!("path=player_song_id song_id={id}"));
            // 只认当前网易云播放器提供的明确歌曲 ID，并用详情核对录音元数据。
            let ids = format!("[{id}]");
            let url = poni_core::urlenc::with_query("https://music.163.com/api/song/detail", &[("ids", &ids)]);
            let data = self.netease_request(job, "song_detail", &url)?;
            let songs = data.get("songs").cloned().ok_or_else(|| netease_bad_reply(job, "song_detail"))?;
            let songs: Vec<NeteaseSong> = serde_json::from_value(songs).map_err(|_| netease_bad_reply(job, "song_detail"))?;
            match songs.into_iter().find(|song| song.id == id) {
                Some(song) => {
                    let record = song.into_record();
                    let mismatch = if metadata_only { recording_identity_mismatch(q, &record) } else { recording_mismatch(q, &record) };
                    if let Some(reason) = mismatch {
                        job.diagnostic_candidate("song_detail", &record, reason);
                        None
                    } else {
                        Some(record)
                    }
                }
                None => {
                    job.diagnostic("song_detail", format_args!("reason=requested_id_missing song_id={id}"));
                    None
                }
            }
        } else {
            job.diagnostic("lookup", format_args!("path=metadata_search reason=no_trusted_player_song_id"));
            self.search_netease(job, q, &album, metadata_only)?
        };
        let Some(mut record) = selected else {
            job.diagnostic("selection", format_args!("reason=no_matching_recording song_id={song_id:?}"));
            return Ok(LyricsResult::NotFound);
        };
        job.diagnostic_candidate("selection", &record, "selected");
        let id = record.id.to_string();
        let url = poni_core::urlenc::with_query(
            "https://music.163.com/api/song/lyric",
            &[("id", &id), ("lv", "-1"), ("kv", "-1"), ("tv", "-1")],
        );
        let data = self.netease_request(job, "lyric", &url)?;
        if data.get("nolyric").and_then(|v| v.as_bool()) == Some(true)
            || data.get("uncollected").and_then(|v| v.as_bool()) == Some(true)
        {
            job.diagnostic("lyric", format_args!("reason=provider_no_lyric candidate_id={} nolyric={:?} uncollected={:?}", record.id, data.get("nolyric").and_then(|value| value.as_bool()), data.get("uncollected").and_then(|value| value.as_bool())));
            return Ok(LyricsResult::NotFound);
        }
        // 只读取主歌词，忽略罗马音和翻译字段。
        let text = data.pointer("/lrc/lyric").and_then(|v| v.as_str()).ok_or_else(|| netease_bad_reply(job, "lyric"))?;
        record.synced_lyrics = Some(text.to_string());
        let Some(lyrics) = record.full_synced_lyrics() else {
            job.diagnostic("lyric", format_args!("reason=missing_timestamps_or_too_few_lines candidate_id={}", record.id));
            return Ok(LyricsResult::NotFound);
        };
        if !metadata_only {
            self.store(&path, &CacheEntry::new(Some(record.clone()), crate::clock::unix_now()));
        }
        crate::pinfo!("lyrics {key}: synced from NetEase Music");
        Ok(to_result(&record, lyrics, job))
    }

    fn search_netease(&mut self, job: &Job, q: &TrackQuery, album: &str, metadata_only: bool) -> Result<Option<LrclibRecord>, Halt> {
        let mut seen = HashSet::new();
        let mut same_album = Vec::new();
        let mut other_album = Vec::new();
        let initial_query = format!("{} {}", q.title, q.artist);
        let normalized_title = recording_title(&q.title);
        // 搜索词去掉通用现场后缀以覆盖Live/Live版别名，筛选时仍严格核对版本。
        let extra_query = if metadata_only {
            format!("{} {}", normalized_title.strip_suffix(" live").unwrap_or(&normalized_title), q.artist)
        } else {
            format!("{initial_query} {}", q.album)
        };
        for (round, query) in [&initial_query, &extra_query].into_iter().enumerate() {
            if round == 1 && (query == &initial_query || (!metadata_only && (album.is_empty() || !same_album.is_empty()))) {
                break;
            }
            let page_size = if metadata_only { NETEASE_SEARCH_PAGE_SIZE } else { 20 };
            let page_limit = if metadata_only { NETEASE_SEARCH_MAX_PAGES } else { 1 };
            let mut offset = 0;
            let mut query_ids = HashSet::new();
            let mut complete = !metadata_only;
            for page in 0..page_limit {
                let url = poni_core::urlenc::with_query(
                    "https://music.163.com/api/search/get",
                    &[("s", query), ("type", "1"), ("limit", &page_size.to_string()), ("offset", &offset.to_string())],
                );
                let data = self.netease_request(job, "search", &url)?;
                let total = data.pointer("/result/songCount").and_then(|v| v.as_u64());
                if total == Some(0) {
                    complete = true;
                    job.diagnostic("search", format_args!("round={} candidates=0 reason=empty_search", round + 1));
                    break;
                }
                if metadata_only && total.is_none_or(|count| count > (NETEASE_SEARCH_PAGE_SIZE * NETEASE_SEARCH_MAX_PAGES) as u64) {
                    job.diagnostic("search", format_args!("reason=incomplete_candidate_set total={total:?}"));
                    return Ok(None);
                }
                let songs = data.pointer("/result/songs").cloned().ok_or_else(|| netease_bad_reply(job, "search"))?;
                let songs: Vec<NeteaseSong> = serde_json::from_value(songs).map_err(|_| netease_bad_reply(job, "search"))?;
                let returned = songs.len();
                let mut identity_rejected = 0;
                let mut recording_rejected = 0;
                for song in songs {
                    if song.id <= 0 { return Err(netease_bad_reply(job, "search")); }
                    query_ids.insert(song.id);
                    if !seen.insert(song.id) { continue; }
                    let record = song.into_record();
                    if let Some(reason) = recording_identity_mismatch(q, &record) {
                        identity_rejected += 1;
                        if identity_rejected <= 3 {
                            job.diagnostic_candidate("candidate_rejected", &record, reason);
                        }
                        continue;
                    }
                    if !metadata_only {
                        if let Some(reason) = recording_mismatch(q, &record) {
                            recording_rejected += 1;
                            if recording_rejected <= 3 {
                                job.diagnostic_candidate("candidate_rejected", &record, reason);
                            }
                            continue;
                        }
                    }
                    if same_album.len() + other_album.len() < 3 {
                        job.diagnostic_candidate("candidate_accepted", &record, "matched");
                    }
                    if !album.is_empty() && record.album_name.as_deref().is_some_and(|a| lrclib::fold(a) == album) {
                        same_album.push(record);
                    } else {
                        other_album.push(record);
                    }
                }
                job.diagnostic("search", format_args!("round={} page={} total={total:?} returned={} identity_rejected={} recording_rejected={} matches={}", round + 1, page + 1, returned, identity_rejected, recording_rejected, same_album.len() + other_album.len()));
                if metadata_only && other_album.len() > 1 {
                    job.diagnostic("selection", format_args!("reason=ambiguous_recordings"));
                    return Ok(None);
                }
                if !metadata_only || total.is_some_and(|count| query_ids.len() as u64 >= count) {
                    complete = true;
                    break;
                }
                if returned == 0 { break; }
                offset += returned;
            }
            if !complete {
                job.diagnostic("search", format_args!("reason=partial_candidate_pages scanned={}", query_ids.len()));
                return Ok(None);
            }
        }
        job.diagnostic("selection", format_args!("same_album={} other_album={}", same_album.len(), other_album.len()));
        let selected = if !same_album.is_empty() {
            same_album.sort_by(|a, b| {
                let distance = |r: &LrclibRecord| (r.duration.unwrap_or(0.0) - q.duration_s as f64).abs();
                distance(a).total_cmp(&distance(b))
            });
            if same_album.len() == 1
                || (q.duration_s > 0
                    && (same_album[0].duration.unwrap_or(0.0) - q.duration_s as f64).abs()
                        < (same_album[1].duration.unwrap_or(0.0) - q.duration_s as f64).abs())
            {
                Some(same_album.remove(0))
            } else {
                job.diagnostic("selection", format_args!("reason=ambiguous_same_album"));
                None
            }
        } else if other_album.len() == 1 {
            other_album.pop()
        } else {
            job.diagnostic("selection", format_args!("reason=no_unique_recording"));
            None
        };
        Ok(selected)
    }

    fn netease_request(&mut self, job: &Job, stage: &str, url: &str) -> Result<serde_json::Value, Halt> {
        if !self.is_current(job) {
            return Err(Halt::Abandoned);
        }
        if let Some(until) = self.netease_blocked_until {
            if Instant::now() < until {
                job.diagnostic(stage, format_args!("reason=rate_limit_backoff wait_ms={}", until.saturating_duration_since(Instant::now()).as_millis()));
                return Err(Halt::Failed { msg: "网易云音乐请求过于频繁，请稍后重试".into(), retry_at: Some(until) });
            }
            self.netease_blocked_until = None;
        }
        if self.netease_client.is_none() {
            self.netease_client = Some(Client::private(WEB_USER_AGENT).map_err(|_| netease_bad_reply(job, stage))?);
        }
        let response = self.netease_client.as_ref().unwrap().get_with_headers(url, &[("Referer", "https://music.163.com/")], http::DEFAULT_TIMEOUT);
        if let Ok(reply) = &response {
            if reply.status == 429 {
                let delay = reply.retry_after_s.map(Duration::from_secs).unwrap_or(DEFAULT_BACKOFF).min(MAX_BACKOFF);
                self.netease_blocked_until = Some(Instant::now() + delay);
            }
        }
        if !self.is_current(job) {
            return Err(Halt::Abandoned);
        }
        let response = response.map_err(|error| {
            let reason = match error {
                FetchError::Timeout => "timeout",
                FetchError::Network(_) => "network_error",
                FetchError::Other(_) => "client_error",
            };
            job.diagnostic(stage, format_args!("reason={reason}"));
            Halt::Failed {
                msg: "暂时无法连接网易云音乐歌词服务".into(),
                retry_at: Some(Instant::now() + retry_delay(job.attempt)),
            }
        })?;
        job.diagnostic(stage, format_args!("http_status={}", response.status));
        if response.status != 200 {
            return Err(Halt::Failed {
                msg: format!("网易云音乐歌词服务错误 {}", response.status),
                retry_at: if response.status == 429 { self.netease_blocked_until } else {
                    (response.status >= 500).then(|| Instant::now() + retry_delay(job.attempt))
                },
            });
        }
        let data: serde_json::Value = serde_json::from_str(&response.body).map_err(|_| netease_bad_reply(job, stage))?;
        let code = data.get("code").and_then(|v| v.as_i64());
        job.diagnostic(stage, format_args!("api_code={code:?}"));
        match code {
            Some(200) => Ok(data),
            Some(429) => {
                let until = Instant::now() + DEFAULT_BACKOFF;
                self.netease_blocked_until = Some(until);
                Err(Halt::Failed { msg: "网易云音乐请求过于频繁，请稍后重试".into(), retry_at: Some(until) })
            }
            _ => Err(netease_bad_reply(job, stage)),
        }
    }

    fn qq_call(&mut self, job: &Job, body: &serde_json::Value, service: &str, search_query: Option<&str>) -> Result<serde_json::Value, Halt> {
        let reply = self.qq_request(job, service, http::Method::Post, "https://u.y.qq.com/cgi-bin/musicu.fcg", Some(&body.to_string()))?;
        let top_code = reply.get("code").and_then(|value| value.as_i64());
        let service_code = reply[service].get("code").and_then(|value| value.as_i64());
        job.diagnostic(service, format_args!("api_code={top_code:?} service_code={service_code:?}"));
        if top_code != Some(0) || service_code != Some(0) {
            crate::pwarn!("lyrics: QQ Music {service} rejected: top code {top_code:?}, service code {service_code:?}");
            if let Some(query) = search_query.filter(|_| service == "search" && (top_code == Some(2001) || service_code == Some(2001))) {
                // One alternate QQ search request; metadata matching stays unchanged.
                let url = poni_core::urlenc::with_query(
                    "https://c.y.qq.com/soso/fcgi-bin/client_search_cp",
                    &[("w", query), ("p", "1"), ("n", "10"), ("format", "json"), ("g_tk", "5381"), ("t", "0")],
                );
                job.diagnostic("search", format_args!("path=legacy_fallback reason=api_code_2001"));
                let legacy = self.qq_request(job, "legacy search", http::Method::Get, &url, None)?;
                let code = legacy.get("code").and_then(|value| value.as_i64());
                job.diagnostic("legacy_search", format_args!("api_code={code:?}"));
                if code != Some(0) {
                    crate::pwarn!("lyrics: QQ Music legacy search rejected: code {code:?}");
                    return Err(Halt::Failed { msg: "QQ 音乐暂时无法提供歌词".into(), retry_at: Some(Instant::now() + DEFAULT_BACKOFF) });
                }
                let songs = legacy.pointer("/data/song/list").and_then(|value| value.as_array()).ok_or_else(|| qq_bad_reply(job, "legacy search"))?;
                let songs: Vec<_> = songs.iter().map(|song| serde_json::json!({
                    "id": song["songid"],
                    "mid": song["songmid"],
                    "name": song["songname"],
                    "title": song["songname"],
                    "album": { "name": song["albumname"] },
                    "interval": song["interval"],
                    "singer": song["singer"]
                })).collect();
                crate::pinfo!("lyrics: QQ Music legacy search returned code 0");
                return Ok(serde_json::json!({ "body": { "song": { "list": songs } } }));
            }
            return Err(Halt::Failed { msg: "QQ 音乐暂时无法提供歌词".into(), retry_at: Some(Instant::now() + DEFAULT_BACKOFF) });
        }
        reply[service].get("data").cloned().ok_or_else(|| qq_bad_reply(job, service))
    }

    fn qq_request(&mut self, job: &Job, service: &str, method: http::Method, url: &str, body: Option<&str>) -> Result<serde_json::Value, Halt> {
        if !self.is_current(job) {
            return Err(Halt::Abandoned);
        }
        if let Some(until) = self.qq_blocked_until {
            if Instant::now() < until {
                job.diagnostic(service, format_args!("reason=rate_limit_backoff wait_ms={}", until.saturating_duration_since(Instant::now()).as_millis()));
                return Err(Halt::Failed { msg: "QQ 音乐请求过于频繁，请稍后重试".into(), retry_at: Some(until) });
            }
            self.qq_blocked_until = None;
        }
        if self.qq_client.is_none() {
            self.qq_client = Some(Client::private(WEB_USER_AGENT).map_err(|error| {
                crate::pwarn!("lyrics: QQ Music HTTP client failed: {error}");
                Halt::Failed { msg: "暂时无法连接 QQ 音乐歌词服务".into(), retry_at: Some(Instant::now() + retry_delay(job.attempt)) }
            })?);
        }
        let response = self.qq_client.as_ref().unwrap().send(
            method,
            url,
            &[("Referer", "https://y.qq.com/")],
            body,
            http::DEFAULT_TIMEOUT,
        );
        // A stale request can still be rate-limited; keep its window for the next job.
        if let Ok(reply) = &response {
            if reply.status == 429 {
                let delay = reply.retry_after_s.map(Duration::from_secs).unwrap_or(DEFAULT_BACKOFF).min(MAX_BACKOFF);
                self.qq_blocked_until = Some(Instant::now() + delay);
            }
        }
        if !self.is_current(job) {
            return Err(Halt::Abandoned);
        }
        let response = response.map_err(|error| {
            let reason = match &error {
                FetchError::Timeout => "timeout",
                FetchError::Network(_) => "network_error",
                FetchError::Other(_) => "client_error",
            };
            job.diagnostic(service, format_args!("reason={reason}"));
            let msg = match error {
                FetchError::Timeout => "QQ 音乐歌词请求超时",
                _ => "暂时无法连接 QQ 音乐歌词服务",
            };
            // GET search URLs contain song metadata; don't log system errors with URLs.
            crate::pwarn!("lyrics: QQ Music {service} request failed: {msg}");
            Halt::Failed { msg: msg.into(), retry_at: Some(Instant::now() + retry_delay(job.attempt)) }
        })?;
        job.diagnostic(service, format_args!("http_status={}", response.status));
        if response.status != 200 {
            crate::pwarn!("lyrics: QQ Music {service} answered HTTP {}", response.status);
            let retry_at = if response.status == 429 {
                self.qq_blocked_until
            } else {
                (response.status >= 500).then(|| Instant::now() + retry_delay(job.attempt))
            };
            return Err(Halt::Failed { msg: format!("QQ 音乐歌词服务错误 {}", response.status), retry_at });
        }
        serde_json::from_str(&response.body).map_err(|_| qq_bad_reply(job, service))
    }

    fn store(&mut self, path: &Path, entry: &CacheEntry) {
        write_cache(path, entry);
        self.writes += 1;
        if self.writes >= PRUNE_EVERY_WRITES {
            self.writes = 0;
            prune_cache(&self.cache_dir);
        }
    }
}

#[derive(serde::Deserialize)]
struct QqSong {
    id: i64,
    mid: String,
    name: String,
    #[serde(default)]
    title: String,
    interval: u32,
    album: CatalogName,
    singer: Vec<CatalogName>,
}

#[derive(serde::Deserialize)]
struct QqAlbum {
    #[serde(rename = "albumMID")]
    mid: String,
    #[serde(rename = "albumName")]
    name: String,
    #[serde(rename = "singer_list")]
    singers: Vec<CatalogName>,
}

#[derive(serde::Deserialize)]
struct QqAlbumSongs {
    #[serde(rename = "totalNum")]
    total: usize,
    #[serde(rename = "songList")]
    tracks: Vec<QqAlbumTrack>,
}

#[derive(serde::Deserialize)]
struct QqAlbumTrack {
    #[serde(rename = "songInfo")]
    song: QqSong,
}

#[derive(serde::Deserialize)]
struct CatalogName {
    name: String,
}

#[derive(serde::Deserialize)]
struct NeteaseSong {
    id: i64,
    name: String,
    duration: u64,
    album: CatalogName,
    artists: Vec<CatalogName>,
}

impl NeteaseSong {
    fn into_record(self) -> LrclibRecord {
        LrclibRecord {
            id: self.id,
            track_name: self.name,
            artist_name: self.artists.iter().map(|artist| artist.name.trim()).collect::<Vec<_>>().join(" / "),
            album_name: Some(self.album.name),
            duration: Some(self.duration as f64 / 1000.0),
            ..Default::default()
        }
    }
}

fn netease_bad_reply(job: &Job, stage: &str) -> Halt {
    job.diagnostic(stage, format_args!("reason=invalid_response"));
    crate::pwarn!("lyrics: NetEase Music returned an invalid response");
    Halt::Failed { msg: "无法读取网易云音乐歌词服务的响应".into(), retry_at: Some(Instant::now() + retry_delay(job.attempt)) }
}

fn qq_bad_reply(job: &Job, service: &str) -> Halt {
    job.diagnostic(service, format_args!("reason=invalid_response"));
    crate::pwarn!("lyrics: QQ Music {service} returned an invalid response");
    Halt::Failed {
        msg: "无法读取 QQ 音乐歌词服务的响应".into(),
        retry_at: Some(Instant::now() + retry_delay(job.attempt)),
    }
}

/// A title alone can match a cover or live version: require the known metadata.
fn recording_matches(q: &TrackQuery, record: &LrclibRecord) -> bool {
    recording_mismatch(q, record).is_none()
}

/// 校验和排查共用同一判断，不另写一套可能与真实匹配结果不同的诊断规则。
fn recording_mismatch(q: &TrackQuery, record: &LrclibRecord) -> Option<&'static str> {
    if let Some(reason) = recording_identity_mismatch(q, record) {
        return Some(reason);
    }
    let album = lrclib::fold(&q.album);
    let same_album = !album.is_empty() && record.album_name.as_deref().is_some_and(|name| lrclib::fold(name) == album);
    if q.duration_s > 0 {
        let max_drift = if same_album { lrclib::MAX_SYNC_DRIFT_S } else { ALIAS_MAX_DRIFT_S };
        let Some(duration) = record.duration.filter(|duration| *duration > 0.0) else {
            return Some("candidate_duration_missing");
        };
        if (duration - q.duration_s as f64).abs() <= max_drift {
            return None;
        }
        return Some("duration_out_of_range");
    }
    if same_album {
        None
    } else {
        Some("album_mismatch_without_duration")
    }
}

/// 身份校验只认当前录音的歌名、歌手和版本，供严格匹配及Windows缺省元数据路径共用。
fn recording_identity_mismatch(q: &TrackQuery, record: &LrclibRecord) -> Option<&'static str> {
    let same_title = if cfg!(windows) {
        recording_title(&q.title) == recording_title(&record.track_name)
    } else {
        lrclib::fold(&q.title) == lrclib::fold(&record.track_name)
    };
    if !same_title {
        return Some("title_or_version_mismatch");
    }
    let artist = lrclib::fold(&q.artist);
    if artist.is_empty()
        || (artist != lrclib::fold(&record.artist_name) && !record.artist_name.split(" / ").any(|name| lrclib::fold(name) == artist))
    {
        return Some("artist_mismatch");
    }
    None
}

/// 统一末尾通用现场标记；保留录音地点、年份、混音及其他版本限定。
fn recording_title(title: &str) -> String {
    let title = title.trim();
    for (open, close) in [('(', ')'), ('（', '）'), ('[', ']'), ('【', '】')] {
        if let Some((name, version)) = title.strip_suffix(close).and_then(|text| text.rsplit_once(open)) {
            if !name.trim().is_empty() && matches!(lrclib::fold(version).as_str(), "live" | "live版" | "现场" | "现场版") {
                return format!("{} live", lrclib::fold(name));
            }
        }
    }
    lrclib::fold(title)
}

fn to_result(record: &LrclibRecord, lyrics: poni_core::lrc::Lyrics, job: &Job) -> LyricsResult {
    if record.instrumental {
        return LyricsResult::Instrumental;
    }
    LyricsResult::Found { lyrics: lyrics_for(record, lyrics, job.query.duration_s) }
}

// ------------------------------------------------------------------ cache

fn read_cache(path: &Path) -> Option<CacheEntry> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            crate::pwarn!("lyrics cache: cannot read {}: {e}", path.display());
            // Invalid UTF-8 means a damaged file; anything else may be transient.
            if e.kind() == io::ErrorKind::InvalidData {
                let _ = fs::remove_file(path);
            }
            return None;
        }
    };
    match serde_json::from_str::<CacheEntry>(&text) {
        Ok(entry) if entry.v == CacheEntry::VERSION => Some(entry),
        Ok(_) => None,
        Err(e) => {
            crate::pwarn!("lyrics cache: dropping unreadable {}: {e}", path.display());
            let _ = fs::remove_file(path);
            None
        }
    }
}

fn write_cache(path: &Path, entry: &CacheEntry) {
    if let Err(e) = poni_core::store::write_json_atomic(path, entry) {
        crate::pwarn!("lyrics cache: cannot write {}: {e}", path.display());
    }
}

/// Remove leftovers of interrupted writes and keep the folder bounded
/// (oldest entries go first).
fn prune_cache(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        match path.extension().and_then(|e| e.to_str()) {
            Some("tmp") => {
                let _ = fs::remove_file(&path);
            }
            Some("json") => {
                let modified = entry.metadata().and_then(|m| m.modified()).ok();
                files.push((modified, path));
            }
            _ => {}
        }
    }
    let victims = prune_victims(files, CACHE_MAX, CACHE_KEEP);
    if !victims.is_empty() {
        crate::pinfo!("lyrics cache: pruning {} old entries", victims.len());
    }
    for path in victims {
        let _ = fs::remove_file(path);
    }
}

/// OS-free helpers (unit-tested on any host).
mod pure {
    use poni_core::lrc::{LyricLine, Lyrics};
    use poni_core::lrclib::{timing_fits, LrclibRecord, MIN_LINES};
    use serde::{Deserialize, Serialize};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    /// One file in the lyrics cache.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct CacheEntry {
        /// Format version.
        #[serde(default)]
        pub v: u32,
        /// Unix seconds when the lyrics service was asked.
        pub fetched_at: i64,
        /// The matched record; `None` records a miss.
        #[serde(default)]
        pub record: Option<LrclibRecord>,
    }

    impl CacheEntry {
        pub const VERSION: u32 = 1;
        pub fn new(record: Option<LrclibRecord>, now: i64) -> Self {
            Self { v: Self::VERSION, fetched_at: now, record }
        }
    }

    /// The lyrics to show while a `playing_s`-second track plays. Synced
    /// lyrics stay synced only when the record is about as long; otherwise
    /// (a music video's intro, a radio edit) they would light up the wrong
    /// lines, so they're shown unsynced.
    pub fn lyrics_for(r: &LrclibRecord, l: Lyrics, playing_s: u32) -> Lyrics {
        if !l.synced || timing_fits(r.duration, playing_s) {
            return l;
        }
        if let Some(text) = r.plain_lyrics.as_deref() {
            let plain = trim_blank_edges(Lyrics::plain(text));
            if plain.text_lines() >= MIN_LINES {
                return plain;
            }
        }
        let lines = l.lines.into_iter().map(|x| LyricLine { time_s: 0.0, text: x.text }).collect();
        trim_blank_edges(Lyrics { lines, synced: false })
    }

    /// Drop blank rows at the start and end of unsynced lyrics (blank rows
    /// in between are stanza breaks and stay).
    fn trim_blank_edges(mut l: Lyrics) -> Lyrics {
        if !l.synced {
            while l.lines.last().is_some_and(|x| x.text.trim().is_empty()) {
                l.lines.pop();
            }
            let lead = l.lines.iter().take_while(|x| x.text.trim().is_empty()).count();
            l.lines.drain(..lead);
        }
        l
    }

    /// Whole seconds for a recording's duration match; 0 when unknown.
    pub fn duration_secs(d: f64) -> u32 {
        if d.is_finite() && d > 0.5 {
            d.round().min(u32::MAX as f64) as u32
        } else {
            0
        }
    }

    /// Delay before the n-th delayed retry (0-based attempt that just failed).
    pub fn retry_delay(attempt: u32) -> Duration {
        Duration::from_secs(if attempt == 0 { 30 } else { 120 })
    }

    /// Which cache files to delete: none while `files.len() <= max`, else the
    /// oldest ones (unknown times first) down to `keep`.
    pub fn prune_victims(mut files: Vec<(Option<SystemTime>, PathBuf)>, max: usize, keep: usize) -> Vec<PathBuf> {
        if files.len() <= max {
            return Vec::new();
        }
        files.sort_by_key(|(t, _)| *t);
        let excess = files.len() - keep.min(files.len());
        files.into_iter().take(excess).map(|(_, p)| p).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Fixtures are original placeholder text, not real lyrics.
        fn rec(synced: Option<&str>, plain: Option<&str>, instrumental: bool) -> LrclibRecord {
            LrclibRecord {
                id: 1,
                track_name: "Paper Boats".into(),
                artist_name: "The Lanterns".into(),
                album_name: None,
                duration: Some(201.0),
                instrumental,
                plain_lyrics: plain.map(Into::into),
                synced_lyrics: synced.map(Into::into),
            }
        }

        const SYNCED: &str = "[00:01.00]first row\n[00:04.50]second row\n[00:08.00]third row\n[00:12.00]fourth row\n";
        const PLAIN: &str = "first row\nsecond row\nthird row\nfourth row";

        #[test]
        fn synced_only_when_the_lengths_match() {
            // rec() is 201 s long.
            let r = rec(Some(SYNCED), Some(PLAIN), false);
            assert!(lyrics_for(&r, r.full_synced_lyrics().unwrap(), 203).synced);
            assert!(lyrics_for(&r, r.full_synced_lyrics().unwrap(), 0).synced);
            // A music video 40 s longer: the plain text, unsynced.
            let l = lyrics_for(&r, r.full_synced_lyrics().unwrap(), 241);
            assert!(!l.synced);
            assert_eq!(l.lines.len(), 4);
            // Without plain text, the synced rows lose their times.
            let r = rec(Some(SYNCED), None, false);
            let l = lyrics_for(&r, r.full_synced_lyrics().unwrap(), 160);
            assert!(!l.synced);
            assert!(l.lines.iter().all(|x| x.time_s == 0.0));
            assert_eq!(l.lines[3].text, "fourth row");
        }

        #[test]
        fn cache_entry_roundtrips() {
            let e = CacheEntry::new(Some(rec(Some("[00:01.00]x"), Some("x"), false)), 42);
            let json = serde_json::to_string(&e).unwrap();
            let back: CacheEntry = serde_json::from_str(&json).unwrap();
            assert_eq!(back, e);
            // 旧缓存多余的 lookup 字段仍能读取，不改变版本与命中记录。
            let miss: CacheEntry = serde_json::from_str(r#"{"v":1,"lookup":2,"fetched_at":7,"record":null}"#).unwrap();
            assert_eq!(miss, CacheEntry::new(None, 7));
        }

        #[test]
        fn durations_round_or_zero() {
            assert_eq!(duration_secs(201.4), 201);
            assert_eq!(duration_secs(201.5), 202);
            assert_eq!(duration_secs(0.0), 0);
            assert_eq!(duration_secs(0.4), 0);
            assert_eq!(duration_secs(-3.0), 0);
            assert_eq!(duration_secs(f64::NAN), 0);
            assert_eq!(duration_secs(f64::INFINITY), 0);
        }

        #[test]
        fn retry_backoff_grows() {
            assert!(retry_delay(1) > retry_delay(0));
        }

        #[test]
        fn prunes_oldest_first() {
            use std::time::UNIX_EPOCH;
            let at = |s: u64| Some(UNIX_EPOCH + Duration::from_secs(s));
            let files = vec![
                (at(30), PathBuf::from("c")),
                (at(10), PathBuf::from("a")),
                (None, PathBuf::from("unknown")),
                (at(20), PathBuf::from("b")),
            ];
            assert!(prune_victims(files.clone(), 4, 2).is_empty());
            assert_eq!(prune_victims(files, 3, 2), vec![PathBuf::from("unknown"), PathBuf::from("a")]);
        }
    }
}
