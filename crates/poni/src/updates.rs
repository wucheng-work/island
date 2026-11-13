//! Windows 更新状态。生产入口只接受本次运行中手动取得的签名发行信息，
//! 不从 update.json 恢复下载或安装授权，不安排自动检查。
//! 历史控制器方法保留数据格式兼容和原有离线检查，平台入口只调用 manual_*。

use crate::icons::Icon;
use crate::state::{AppState, InlineAction, Notice, NoticeAction, Said};
use crate::version::VERSION;
use poni_core::geom::{palette, Color};
use poni_core::store::{load_json, save_json_atomic};
use poni_core::update::{self, Memory, Release};
use poni_core::update::Download;
use std::path::PathBuf;

/// How long the island shows "Updated to" ("Update available" stays until
/// Update or Hide).
const UPDATED_S: f64 = 10.0;
/// How long the answer to Check for updates… shows on the island.
const ANSWER_S: f64 = 6.0;
/// After Check for updates… finds a new version, the Update sheet opens by
/// itself once its download is in, if that takes no longer than this; a
/// slow line leaves it to the island's notice.
const ASK_OPEN_S: u64 = 120;

pub struct Updater {
    path: PathBuf,
    memory: Memory,
    /// When to read the file next (Unix seconds); None while a read runs.
    due: Option<u64>,
    /// "Updated to" waits for the island to be free.
    updated: bool,
    /// Check for updates… was picked at this time (Unix seconds), and its
    /// answer is still to come.
    asked: Option<u64>,
    /// The title of the notice the answer put up while a download runs, so
    /// the sheet can take it down when it opens.
    told: Option<String>,
    /// The offered version's download, and the folder it downloads to.
    setup: Setup,
    setups: PathBuf,
    /// This copy can install a new version itself, so it downloads one
    /// before telling.
    installs: bool,
    /// 会话内签名验证结果，不能从本地缓存恢复。
    verified: Option<Release>,
    checking: bool,
}

/// Where the offered version's Setup download stands.
#[derive(Clone, Debug, PartialEq)]
enum Setup {
    /// Not asked for yet.
    None,
    Fetching,
    /// Ready to install at this path, checked by size and SHA-256.
    Ready(PathBuf),
    /// The download failed: the sheet offers the release page, and the
    /// next look at the file tries again.
    Failed,
}

impl Updater {
    /// 清除历史更新授权；启动时仅做本地状态处理，不检查、不下载。
    pub fn start(path: PathBuf, st: &mut AppState, _now: u64) -> Self {
        let (mut memory, _) = load_json::<Memory>(&path);
        memory.latest = None;
        memory.install_on_start.clear();
        memory.ran = VERSION.into();
        save(&path, &memory);
        st.update.offer = None;
        st.update.sheet = false;
        st.update.ready = false;
        st.update.next_start = false;
        st.update.said = None;
        st.update.looks = update::enabled();
        Self {
            path, memory, due: None, updated: false, asked: None, told: None,
            setup: Setup::None, setups: setup_dir(), installs: crate::services::update::can_install(),
            verified: None, checking: false,
        }
    }

    pub fn manual_busy(&self) -> bool {
        self.checking || matches!(self.setup, Setup::Fetching)
    }

    /// 每次重试重新验证 feed，不沿用上次失败前或磁盘缓存中的授权。
    pub fn manual_begin(&mut self, st: &mut AppState) -> bool {
        if self.manual_busy() {
            return false;
        }
        self.checking = true;
        self.verified = None;
        self.setup = Setup::None;
        st.update.offer = None;
        st.update.ready = false;
        true
    }

    /// 只由验证过签名的服务返回值调用，不把缓存视为可信发行信息。
    pub fn manual_checked(&mut self, st: &mut AppState, result: Result<Release, String>) -> Result<Option<Release>, String> {
        if !self.checking {
            return Err("更新检查已失效，请重新检查".into());
        }
        self.checking = false;
        let release = result?;
        if !update::newer(&release.version, VERSION) {
            return Ok(None);
        }
        crate::services::update::validate_release(&release)?;
        self.verified = Some(release.clone());
        st.update.offer = Some(release.clone());
        Ok(Some(release))
    }

    pub fn manual_download(&mut self) -> Result<(Download, PathBuf), String> {
        if self.manual_busy() {
            return Err("更新任务正在进行，请稍候".into());
        }
        let release = self.verified.as_ref().ok_or("请先手动检查并验证更新信息")?;
        let download = crate::services::update::validate_release(release)?.clone();
        let path = self.ready_path(&download);
        self.setup = Setup::Fetching;
        Ok((download, path))
    }

    pub fn manual_fetched(&mut self, st: &mut AppState, result: Result<PathBuf, String>) -> Result<Release, String> {
        if !matches!(self.setup, Setup::Fetching) {
            return Err("更新下载已失效，请重新检查".into());
        }
        self.setup = Setup::Failed;
        st.update.ready = false;
        let path = result?;
        let release = self.verified.as_ref().ok_or("本次运行尚未验证更新签名")?;
        let download = crate::services::update::validate_release(release)?;
        if path != self.ready_path(download) {
            return Err("安装器路径与本次验证的发行版不一致".into());
        }
        self.setup = Setup::Ready(path);
        st.update.ready = true;
        Ok(release.clone())
    }

    /// 安装前再次要求会话内验证结果，不接受仅来自 update.json 的数据。
    pub fn manual_ready(&self) -> Result<(PathBuf, Release), String> {
        let release = self.verified.as_ref().ok_or("本次运行尚未验证更新签名，请重新检查")?;
        let download = crate::services::update::validate_release(release)?;
        match &self.setup {
            Setup::Ready(path) if *path == self.ready_path(download) => Ok((path.clone(), release.clone())),
            _ => Err("安装器尚未准备好，请重新检查更新".into()),
        }
    }

    fn open(path: PathBuf, setups: PathBuf, st: &mut AppState, now: u64) -> Self {
        let (mut memory, _) = load_json::<Memory>(&path);
        let updated = memory.updated(VERSION);
        if memory.ran != VERSION {
            memory.ran = VERSION.to_string();
            save(&path, &memory);
        }
        let due = Some(memory.next_check(now));
        let u = Updater {
            path,
            memory,
            due,
            updated,
            asked: None,
            told: None,
            setup: Setup::None,
            setups,
            installs: crate::services::update::can_install(),
            verified: None,
            checking: false,
        };
        st.update.looks = true;
        u.show(st);
        u.clear_old_setups(st);
        let latest = u.memory.latest.as_ref().map_or("none", |r| r.version.as_str());
        crate::pinfo!("updates: latest known {latest}, offering {:?}, next look in {} s", st.update.offer.as_ref().map(|r| &r.version), due.unwrap_or(now).saturating_sub(now));
        u
    }

    /// Seconds until the next read (0 = now). None while a read runs.
    pub fn due_in(&self, now: u64) -> Option<u64> {
        Some(self.due?.saturating_sub(now))
    }

    /// The read starts now (on the add-on thread).
    pub fn begin(&mut self) {
        self.due = None;
    }

    /// Check for updates… (the menu), at `now`: its answer goes on the
    /// island. True when the app should start a read now, whatever the time
    /// and even on battery saver (someone asked); false while one already
    /// runs, whose answer is told instead.
    pub fn ask(&mut self, now: u64) -> bool {
        crate::pinfo!("updates: Check for updates… picked");
        self.asked = Some(now);
        self.due.is_some()
    }

    /// The file's answer, at `now` (Unix seconds). True when the app should
    /// open the island, which now shows the Update sheet: Check for updates…
    /// found a new version that can be installed.
    pub fn checked(&mut self, st: &mut AppState, r: Result<Release, String>, now: u64) -> bool {
        let read = r.is_ok();
        match r {
            Ok(rel) => {
                self.memory.read(rel, now);
                self.due = Some(now + update::CHECK_EVERY_S);
            }
            // The file is kept as last read; the next look is in an hour.
            Err(_) => self.due = Some(now + update::RETRY_S),
        }
        save(&self.path, &self.memory);
        self.show(st);
        if self.asked.is_some() { self.answer(st, read) } else { false }
    }

    /// The answer to Check for updates…, once the file is read: up to date,
    /// or couldn't look, on the island; a new version opens the Update sheet
    /// (true), on a copy that installs once its download is in
    /// ([`Updater::fetched`]). Asked
    /// with Settings' Check now (`said`), and Settings still open, the
    /// answer goes in its place there.
    fn answer(&mut self, st: &mut AppState, read: bool) -> bool {
        let in_settings = st.update.said.is_some();
        if !read {
            self.asked = None;
            if in_settings {
                st.update.said = Some(Said::Failed);
            } else {
                st.notify(Icon::Unlink, palette::ORANGE, "Couldn't check for updates", "Check the connection and try again.", ANSWER_S, None);
            }
            return false;
        }
        let Some(rel) = st.update.offer.clone() else {
            self.asked = None;
            if in_settings {
                st.update.said = Some(Said::UpToDate);
            } else {
                let subtitle = format!("{VERSION} 是最新版本。");
                st.notify(Icon::CheckCircle, palette::GREEN, "灵动岛已是最新版本", subtitle, ANSWER_S, None);
            }
            return false;
        };
        // Settings now has the new version's row, with Update.
        st.update.said = None;
        // A copy that installs: the sheet waits for the Setup download,
        // downloading now (or tried again after a failed download). Its new row
        // shows meanwhile; from the menu, a notice.
        if self.installs && !st.update.ready {
            if in_settings {
                return false;
            }
            let notice = Notice {
                icon: Icon::Download,
                color: st.accent,
                title: "Downloading".into(),
                title_dim: rel.version.clone(),
                subtitle: "Ready to install in a moment".into(),
                until: st.now + ASK_OPEN_S as f64,
                action: None,
                inline: None,
                reveal: None,
            };
            self.told = Some(notice.title.clone());
            st.notice = Some(notice);
            return false;
        }
        self.asked = None;
        self.open_sheet(st, &rel.version)
    }

    /// Open the Update sheet for `version` in answer to Check for updates…:
    /// its notice won't come up for it afterwards.
    fn open_sheet(&mut self, st: &mut AppState, version: &str) -> bool {
        if let Some(title) = self.told.take() {
            if st.notice.as_ref().is_some_and(|n| n.title == title) {
                st.notice = None;
            }
        }
        st.open_update();
        if self.memory.announced != version {
            self.memory.announced = version.to_string();
            save(&self.path, &self.memory);
        }
        true
    }

    /// The offered version was seen: Hide on its notice, or the Update
    /// sheet opened (from the notice, the menu or Settings). Its notice goes
    /// and doesn't come back for that version.
    pub fn seen(&mut self, st: &mut AppState) {
        if st.notice.as_ref().is_some_and(is_new_version_notice) {
            st.notice = None;
        }
        let Some(v) = st.update.offer.as_ref().map(|r| &r.version) else { return };
        if self.memory.announced != *v {
            self.memory.announced = v.clone();
            save(&self.path, &self.memory);
        }
    }

    /// The offered version's download, if it isn't ready yet or on its way
    /// and this copy can install it. The app runs it on the add-on thread
    /// and passes the answer to [`Updater::fetched`].
    pub fn want_setup(&mut self, st: &mut AppState) -> Option<(Download, PathBuf)> {
        if !self.installs {
            return None;
        }
        let d = st.update.offer.as_ref()?.download(false)?.clone();
        let path = self.setups.join(d.file_name());
        match &self.setup {
            Setup::Fetching => return None,
            Setup::Ready(p) if *p == self.ready_path(&d) => return None,
            _ => {}
        }
        self.setup = Setup::Fetching;
        self.show(st);
        Some((d, path))
    }

    /// The download ended. True when Check for updates… was waiting for it
    /// and the app should open the island. The sheet offers Install now,
    /// or the release page if the download failed.
    pub fn fetched(&mut self, st: &mut AppState, r: Result<PathBuf, String>) -> bool {
        self.setup = r.map_or(Setup::Failed, Setup::Ready);
        self.show(st);
        self.clear_old_setups(st);
        let Some(at) = self.asked.take() else { return false };
        let Some(version) = st.update.offer.as_ref().map(|r| r.version.clone()) else { return false };
        if unix_now().saturating_sub(at) > ASK_OPEN_S {
            // Too slow to open by itself now: the notice tells as usual.
            self.told = None;
            return false;
        }
        self.open_sheet(st, &version)
    }

    /// The checked Setup and the download it came from.
    pub fn ready_setup(&self, st: &AppState) -> Option<(PathBuf, Download)> {
        let d = st.update.offer.as_ref()?.download(false)?;
        match &self.setup {
            Setup::Ready(p) if *p == self.ready_path(d) => Some((p.clone(), d.clone())),
            _ => None,
        }
    }

    /// Where the downloaded Setup is ready to install.
    fn ready_path(&self, d: &Download) -> PathBuf {
        self.setups.join(d.file_name())
    }

    /// 历史设置格式的兼容入口。生产窗口不再调用，启动时会清除此字段。
    pub fn install_on_next_start(&mut self, st: &mut AppState) {
        let Some(v) = st.update.offer.as_ref().map(|r| r.version.clone()) else { return };
        crate::pinfo!("updates: {v} installs on the next start");
        self.memory.install_on_start = v;
        save(&self.path, &self.memory);
        self.show(st);
    }

    /// Downloads in the cache folder except the offered one's: older
    /// versions, or all once this version is the latest.
    fn clear_old_setups(&self, st: &AppState) {
        let keep: Vec<PathBuf> = st.update.offer.as_ref().and_then(|r| r.download(false)).map(|d| vec![self.setups.join(d.file_name()), self.ready_path(d)]).unwrap_or_default();
        let Ok(entries) = std::fs::read_dir(&self.setups) else { return };
        for e in entries.flatten() {
            let path = e.path();
            if !keep.contains(&path) {
                let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
            }
        }
    }

    /// The release page to open while the app can't install the update itself.
    pub fn download_page(&self, st: &AppState) -> Option<String> {
        let rel = st.update.offer.as_ref()?;
        Some(rel.notes.clone())
    }

    /// Put a waiting notice on the island if it is `free` (shown, nobody
    /// away, no other notice, closed): "Updated to" first, then a new
    /// version, until it is seen ([`Updater::seen`]). True when one went up.
    pub fn announce(&mut self, st: &mut AppState, free: bool) -> bool {
        if st.update.sheet {
            self.seen(st);
        }
        if !free {
            return false;
        }
        if self.updated {
            self.updated = false;
            let headline = self.memory.latest.as_ref().filter(|r| r.version == VERSION).map(|r| r.headline.clone());
            st.notice = Some(updated_notice(headline, st.now + UPDATED_S));
            return true;
        }
        if !self.memory.should_announce(VERSION, false) {
            return false;
        }
        // A copy that installs downloads first, then tells (a failed
        // download tells too, with the release page).
        if self.installs && matches!(self.setup, Setup::None | Setup::Fetching) {
            return false;
        }
        let Some(rel) = st.update.offer.clone() else { return false };
        crate::pinfo!("updates: telling about {}", rel.version);
        // No time limit: it stays until Update or Hide. Another notice may
        // take its place; it comes back once the island is free again.
        st.notice = Some(new_version_notice(&rel, st.accent, f64::INFINITY));
        true
    }

    /// The offer Settings and the menu show, from what is remembered (no
    /// sheet without one).
    fn show(&self, st: &mut AppState) {
        st.update.offer = self.memory.offer(VERSION, false).cloned();
        st.update.sheet &= st.update.offer.is_some();
        st.update.next_start = st.update.offer.as_ref().is_some_and(|r| r.version == self.memory.install_on_start);
        st.update.ready = self.ready_setup(st).is_some();
    }
}

/// The menu's line about new versions.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateLine {
    /// None: the Microsoft Store updates this copy.
    None,
    /// "Check for updates…", under About.
    Check,
    /// The update entry, first in the menu until it is installed.
    Offer(String),
}

/// What the menu says about new versions: the version on offer, or Check for
/// updates… while this copy looks for them itself (`looks`: it has an
/// [`Updater`]).
pub fn menu_line(st: &AppState, looks: bool) -> UpdateLine {
    match &st.update.offer {
        Some(r) => UpdateLine::Offer(r.version.clone()),
        None if looks => UpdateLine::Check,
        None => UpdateLine::None,
    }
}

/// Where a new version's download waits: `update` in the cache folder
/// (which the Windows uninstaller removes).
fn setup_dir() -> PathBuf {
    crate::paths::cache_dir().join("update")
}

/// "Update available 0.2.5" (the version dimmer) over the release's
/// headline, and Hide. Clicked, it opens the Update sheet.
pub fn new_version_notice(rel: &Release, accent: Color, until: f64) -> Notice {
    Notice {
        icon: Icon::Download,
        color: accent,
        title: "Update available".into(),
        title_dim: rel.version.clone(),
        subtitle: rel.headline.clone(),
        until,
        action: Some(NoticeAction::OpenUpdate),
        inline: Some(InlineAction::Hide),
        reveal: None,
    }
}

/// A new version's notice, which stays until it is seen.
fn is_new_version_notice(n: &Notice) -> bool {
    n.action == Some(NoticeAction::OpenUpdate)
}

/// "Updated to 0.2.5" once, with What's new.
pub fn updated_notice(headline: Option<String>, until: f64) -> Notice {
    Notice {
        icon: Icon::CheckCircle,
        color: palette::GREEN,
        title: format!("Updated to {VERSION}"),
        title_dim: String::new(),
        subtitle: headline.unwrap_or_else(|| "See what's new in this version".into()),
        until,
        action: None,
        inline: Some(InlineAction::WhatsNew),
        reveal: None,
    }
}

fn save(path: &PathBuf, memory: &Memory) {
    if let Err(e) = save_json_atomic(path, memory) {
        crate::pwarn!("updates: couldn't save {} ({e})", path.display());
    }
}

/// Unix seconds now.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use poni_core::settings::Settings;
    use poni_core::store::Store;

    /// A temporary folder for `update.json` and the Setups.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poni-updates-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("setups")).unwrap();
        dir
    }

    /// A later release than this build, remembered from an earlier look.
    fn remembered(dir: &std::path::Path, setup: &[u8]) -> PathBuf {
        let next = format!("{}.99", VERSION);
        let repo = "https://github.com/lucacadalora/oyenza-studio/releases";
        let d = Download {
            url: format!("{repo}/download/v{next}/Oyenza-Studio-Setup-{next}.exe"),
            size: setup.len() as u64,
            sha256: poni_core::sha256::hex(&poni_core::sha256::sha256(setup)),
        };
        let rel = Release { version: next.clone(), date: String::new(), headline: "Next".into(), notes: format!("{repo}/tag/v{next}"), windows: Some(d), mac: None };
        let memory = Memory { checked: 1, latest: Some(rel), ran: VERSION.into(), ..Memory::default() };
        let path = dir.join("update.json");
        save(&path, &memory);
        path
    }

    #[test]
    fn windows_downloads_first_then_tells() {
        let dir = folder("first");
        let path = remembered(&dir, b"setup bytes");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path, dir.join("setups"), &mut st, 10);
        assert!(st.update.offer.is_some() && !st.update.ready);
        // Nothing on the island before the Setup is asked for, or while it comes.
        assert!(!u.announce(&mut st, true));
        let (d, setup) = u.want_setup(&mut st).expect("the Setup to download");
        assert!(u.want_setup(&mut st).is_none(), "once");
        assert!(!u.announce(&mut st, true));
        std::fs::write(&setup, b"setup bytes").unwrap();
        u.fetched(&mut st, Ok(setup.clone()));
        assert!(st.update.ready);
        assert_eq!(u.ready_setup(&st).map(|(p, _)| p), Some(setup.clone()));
        assert!(u.announce(&mut st, true), "now it tells");
        // Install refuses a Setup that changed after the download.
        std::fs::write(&setup, b"setup bytez").unwrap();
        assert!(crate::services::update::install(&setup, &d).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_download_tells_with_the_release_page_and_tries_again() {
        let dir = folder("failed");
        let path = remembered(&dir, b"setup bytes");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path, dir.join("setups"), &mut st, 10);
        assert!(u.want_setup(&mut st).is_some());
        u.fetched(&mut st, Err("offline".into()));
        assert!(!st.update.ready && u.ready_setup(&st).is_none());
        assert!(u.announce(&mut st, true));
        assert!(u.want_setup(&mut st).is_some(), "the next look tries again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_for_updates_says_up_to_date_or_that_it_couldnt_look() {
        let dir = folder("ask");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(dir.join("update.json"), dir.join("setups"), &mut st, 10);
        let now = unix_now();
        assert!(u.ask(now), "a read starts at once");
        u.begin();
        assert!(!u.ask(now), "not a second one while it runs");
        let repo = "https://github.com/lucacadalora/oyenza-studio/releases";
        let this = Release { version: VERSION.into(), date: String::new(), headline: "This".into(), notes: format!("{repo}/tag/v{VERSION}"), windows: None, mac: None };
        assert!(!u.checked(&mut st, Ok(this), now));
        assert_eq!(st.notice.as_ref().map(|n| n.title.as_str()), Some("灵动岛已是最新版本"));
        let subtitle = format!("{VERSION} 是最新版本。");
        assert_eq!(st.notice.as_ref().map(|n| n.subtitle.as_str()), Some(subtitle.as_str()));
        st.notice = None;
        assert!(u.ask(now));
        u.begin();
        assert!(!u.checked(&mut st, Err("offline".into()), now));
        assert_eq!(st.notice.as_ref().map(|n| n.title.as_str()), Some("Couldn't check for updates"));
        // The hourly looks say nothing.
        st.notice = None;
        u.begin();
        assert!(!u.checked(&mut st, Err("offline".into()), now));
        assert!(st.notice.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_for_updates_opens_the_sheet_once_the_setup_is_in() {
        let dir = folder("ask-new");
        let path = remembered(&dir, b"setup bytes");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path, dir.join("setups"), &mut st, 10);
        let rel = st.update.offer.clone().expect("a later version");
        let now = unix_now();
        assert!(u.ask(now));
        u.begin();
        assert!(!u.checked(&mut st, Ok(rel.clone()), now), "the Setup comes first");
        let version = rel.version.as_str();
        assert_eq!(st.notice.as_ref().map(|n| (n.title.as_str(), n.title_dim.as_str())), Some(("Downloading", version)));
        let (_, setup) = u.want_setup(&mut st).expect("the Setup to download");
        std::fs::write(&setup, b"setup bytes").unwrap();
        assert!(u.fetched(&mut st, Ok(setup)), "now the sheet opens");
        assert!(st.update.sheet && st.update.ready && st.notice.is_none());
        assert!(!u.announce(&mut st, true), "and no notice for that version afterwards");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Download the offered Setup, so the island tells about it.
    fn fetch(u: &mut Updater, st: &mut AppState) {
        let (_, setup) = u.want_setup(st).expect("the Setup to download");
        std::fs::write(&setup, b"setup bytes").unwrap();
        u.fetched(st, Ok(setup));
    }

    #[test]
    fn a_new_versions_notice_stays_until_update_or_hide() {
        let dir = folder("stays");
        let path = remembered(&dir, b"setup bytes");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path.clone(), dir.join("setups"), &mut st, 10);
        assert!(st.update.looks, "a copy with an updater has Check now");
        fetch(&mut u, &mut st);
        assert!(u.announce(&mut st, true));
        assert!(st.notice.as_ref().is_some_and(|n| n.until.is_infinite()), "no time limit");
        // Another notice took its place: it comes back once the island is free.
        st.notice = None;
        assert!(u.announce(&mut st, true));
        // Still up at quit: the next start tells again.
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path.clone(), dir.join("setups"), &mut st, 10);
        fetch(&mut u, &mut st);
        assert!(u.announce(&mut st, true));
        // Hide: gone, and not again for this version, not even after a restart.
        u.seen(&mut st);
        assert!(st.notice.is_none());
        assert!(!u.announce(&mut st, true));
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path, dir.join("setups"), &mut st, 10);
        fetch(&mut u, &mut st);
        assert!(!u.announce(&mut st, true));
        assert!(st.update.offer.is_some(), "Settings and the menu still offer it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_update_sheet_counts_as_seen() {
        let dir = folder("sheet");
        let path = remembered(&dir, b"setup bytes");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path, dir.join("setups"), &mut st, 10);
        fetch(&mut u, &mut st);
        assert!(u.announce(&mut st, true));
        // The menu's "Update to" line (or Settings' Update) opens the sheet
        // while the notice is up: the notice goes with it.
        st.open_update();
        assert!(!u.announce(&mut st, false));
        assert!(st.notice.is_none());
        st.update.sheet = false;
        assert!(!u.announce(&mut st, true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_now_answers_in_settings_not_on_the_island() {
        let dir = folder("check-now");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(dir.join("update.json"), dir.join("setups"), &mut st, 10);
        let now = unix_now();
        let repo = "https://github.com/lucacadalora/oyenza-studio/releases";
        let this = Release { version: VERSION.into(), date: String::new(), headline: "This".into(), notes: format!("{repo}/tag/v{VERSION}"), windows: None, mac: None };
        st.update.said = Some(Said::Checking);
        assert!(u.ask(now));
        u.begin();
        assert!(!u.checked(&mut st, Ok(this), now));
        assert_eq!(st.update.said, Some(Said::UpToDate));
        assert!(st.notice.is_none());
        st.update.said = Some(Said::Checking);
        assert!(u.ask(now));
        u.begin();
        assert!(!u.checked(&mut st, Err("offline".into()), now));
        assert_eq!(st.update.said, Some(Said::Failed));
        assert!(st.notice.is_none());
        // A new version: Settings shows its row at once, and the sheet opens
        // once the Setup is in.
        let dir2 = folder("check-now-new");
        let path = remembered(&dir2, b"setup bytes");
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path, dir2.join("setups"), &mut st, 10);
        let rel = st.update.offer.clone().expect("a later version");
        st.update.said = Some(Said::Checking);
        assert!(u.ask(now));
        u.begin();
        assert!(!u.checked(&mut st, Ok(rel), now));
        assert!(st.update.said.is_none() && st.notice.is_none());
        let (_, setup) = u.want_setup(&mut st).expect("the Setup to download");
        std::fs::write(&setup, b"setup bytes").unwrap();
        assert!(u.fetched(&mut st, Ok(setup)), "the sheet opens");
        assert!(st.update.sheet);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn install_on_next_start_is_remembered_and_old_setups_go() {
        let dir = folder("next");
        let path = remembered(&dir, b"setup bytes");
        std::fs::write(dir.join("setups/Oyenza-Studio-Setup-0.0.1.exe"), b"old").unwrap();
        let mut st = AppState::new(Settings::default(), Store::default());
        let mut u = Updater::open(path.clone(), dir.join("setups"), &mut st, 10);
        assert!(!dir.join("setups/Oyenza-Studio-Setup-0.0.1.exe").exists(), "an older version's Setup goes at start");
        u.install_on_next_start(&mut st);
        assert!(st.update.next_start);
        let (memory, _) = load_json::<Memory>(&path);
        assert_eq!(memory.pending_install(VERSION).map(|r| r.version.clone()), st.update.offer.as_ref().map(|r| r.version.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
