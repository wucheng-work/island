//! User settings, stored as JSON next to the data store.

#[cfg(not(windows))]
use crate::alert_sounds::AlertSounds;
use crate::pomodoro::PomodoroConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleStyle {
    /// Small pill with a clock and battery ring.
    Clock,
    /// Small blank pill (like a MacBook notch).
    Minimal,
    /// A friendly pair of blinking eyes.
    Eyes,
    /// Nothing visible until the cursor reaches the top edge.
    Hidden,
    /// A wider pill like a status bar: the date and time, the volume and
    /// the battery (the Pill theme; the other themes show the clock).
    Status,
}

/// How the island looks. Pages and live activities work the same in every theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// The plain black island.
    #[default]
    Classic,
    /// Oyen, the Oyenza cat, lives in the island: he peeks over its edge
    /// while it rests, bobs to the music and takes Oyen's orange as accent.
    Oyen,
    /// Liquid glass: the island bends, frosts and tints what's under it.
    Glass,
    /// Pill: the island floats a little below the top edge as a black
    /// capsule, with its own layouts: the time and battery at rest, the song
    /// while music plays, a rounded card when it opens.
    Pill,
    /// Diorama: the Pill theme's floating island, and at rest a small
    /// window onto a pixel-art place that keeps the day's hours: a house, a
    /// tree, the sun and the moon on their way across, and a glass ball
    /// that rolls from the house to the tree when night falls
    /// (`crates/studio-diorama`). Everything else is Pill's.
    Diorama,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorChoice {
    Primary,
    /// The monitor under the cursor when Poni starts.
    Cursor,
}

/// Bumped when defaults change in a way existing files should pick up.
pub const SETTINGS_VERSION: u32 = 7;

fn version_zero() -> u32 {
    0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Missing in files written before versioning (treated as 0).
    #[serde(default = "version_zero")]
    pub settings_version: u32,
    pub theme: Theme,
    pub idle_style: IdleStyle,
    pub use_24h: bool,
    /// 保存用户选定的城市；空值表示尚未选择，不会自动请求定位。
    pub weather_city: String,
    /// 保存手选或点击定位取得的坐标，后续刷新和重启均直接复用。
    pub weather_city_coordinates: Option<(f64, f64)>,
    /// Delay before hover expands the island.
    pub hover_delay_ms: u32,
    /// Grace period after the cursor leaves before collapsing (0 = the
    /// island starts closing on the next frame).
    pub collapse_delay_ms: u32,
    /// Width of the expanded island in DIPs (bigger = roomier pages).
    pub expanded_width: f32,
    pub hide_in_fullscreen: bool,
    /// Step aside over maximized windows: the resting island turned into the
    /// Hidden look's invisible strip while an app window reached the top of
    /// the screen under it. 0.2.4 and 0.2.5 did it by default and it read as
    /// the island vanishing, so it's gone from the menu and Settings and
    /// always off (`sanitized`); the Hidden idle look is the way to keep the
    /// top edge clear. The key stays so files from those versions still load.
    pub step_aside: bool,
    pub monitor: MonitorChoice,
    /// Where the island sits along the top edge once dragged there: its
    /// centre as a share of the monitor's width (0 the left edge, 1 the
    /// right), kept on screen ([`crate::spot`]). None: the middle.
    pub island_x: Option<f32>,
    pub show_media_activity: bool,
    pub lyrics: bool,
    /// Shift synced lyrics: positive shows each line earlier (ms).
    pub lyrics_offset_ms: i32,
    /// 当前歌词的自定义 RGB 色；None 沿用主题默认色。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_color: Option<u32>,
    pub hud_volume: bool,
    pub hud_power: bool,
    pub hud_bluetooth: bool,
    /// 专注操作及阶段结束提示音。
    pub focus_sound: bool,
    /// 文件成功加入暂存区时的提示音。
    pub shelf_sound: bool,
    /// Codex 新消息及复制操作提示音。
    pub codex_sound: bool,
    /// Mac 模块音效沿用既有分组设置，Windows 保留各自的稳定字段。
    #[cfg(not(windows))]
    pub alert_sounds: AlertSounds,
    /// Windows 专注使用的内置提示音 ID。
    #[cfg(windows)]
    pub focus_sound_id: String,
    /// Windows 文件暂存区使用的内置提示音 ID。
    #[cfg(windows)]
    pub shelf_sound_id: String,
    /// Windows Codex 使用的内置提示音 ID。
    #[cfg(windows)]
    pub codex_sound_id: String,
    /// Windows 微信使用的内置提示音 ID。
    #[cfg(windows)]
    pub wechat_sound_id: String,
    /// Windows 邮件使用的内置提示音 ID；账户声音开关继续单独生效。
    #[cfg(windows)]
    pub mail_sound_id: String,
    /// Page shown first when expanding ("media", "today", "shelf", "focus", "system").
    pub start_page: String,
    /// Remember the last page instead of `start_page`.
    pub remember_page: bool,
    /// Tabs taken off the header's row in Settings, by page key: Music,
    /// Today, Shelf and Focus can go, and Notifications, which is off
    /// while its key is here. Keys this version doesn't know are kept, so a
    /// newer Studio's choices survive a save.
    pub hidden_tabs: Vec<String>,
    pub pomodoro: PomodoroConfig,
    /// Oyenza (once signed in): show sums and senders on the island straight
    /// away. Off: they wait for a click on Show, so a shared screen shows none.
    pub oyenza_show_amounts: bool,
    /// Keep the System page's charts going while it's closed (a light reading
    /// of the kernel counters once a second), so they're full when it opens.
    /// Only while System is on ([`Settings::system_history_on`]).
    pub system_history: bool,
    /// The System switch keeps its original storage key. Old add-on choices
    /// remain here as data, but no longer enable any other feature.
    pub addons: BTreeMap<String, bool>,
    /// Show over games: while a full-screen app is in front, the island
    /// shows its frame rate, the GPU's load and temperature and the CPU's
    /// load. Turning it on also keeps the island on top of full-screen apps.
    pub game_stats: bool,
    /// Show over games, readout only for a few seconds (when the game comes
    /// to the front, when the frame rate drops sharply, on Alt+Shift+F),
    /// then the island hides and Windows sends the game's frames straight to
    /// the screen. Off (the default), the readout stays while the game is in
    /// front. Its own name: the corner test build wrote `game_readout`
    /// ("peek" as its default) into the files of the people who tried it.
    pub game_peek: bool,
    /// Notifications' choices: which apps the island shows, and
    /// whether who it's from and the message show at once.
    pub notifications: crate::notifications::NotificationSettings,
    pub mail: crate::mail::MailSettings,
    /// 用户开启后才监听本机 Codex 消息；消息正文不会持久化。
    pub codex_enabled: bool,
    /// 收起状态是否显示正文，默认只显示通用提醒。
    pub codex_preview: bool,
    /// Windows 微信基础提醒与独立声音；不读取聊天内容。
    #[cfg(windows)]
    pub wechat_reminders: bool,
    #[cfg(windows)]
    pub wechat_sound: bool,
    /// 空值沿用主题字体；系统字体卸载后安全回退。
    #[cfg(windows)]
    pub ui_font: String,
    /// 已移除功能及较新版本的原始设置仍随文件保存，避免覆盖用户旧数据。
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            settings_version: SETTINGS_VERSION,
            theme: Theme::Classic,
            idle_style: IdleStyle::Clock,
            use_24h: true,
            weather_city: String::new(),
            weather_city_coordinates: None,
            hover_delay_ms: 220,
            collapse_delay_ms: 0,
            expanded_width: 620.0,
            hide_in_fullscreen: true,
            step_aside: false,
            monitor: MonitorChoice::Primary,
            island_x: None,
            show_media_activity: true,
            lyrics: true,
            lyrics_offset_ms: 0,
            lyrics_color: None,
            hud_volume: true,
            hud_power: true,
            hud_bluetooth: true,
            focus_sound: true,
            shelf_sound: true,
            codex_sound: true,
            #[cfg(not(windows))]
            alert_sounds: AlertSounds::default(),
            #[cfg(windows)]
            focus_sound_id: crate::sound_catalog::DEFAULT_FOCUS.into(),
            #[cfg(windows)]
            shelf_sound_id: crate::sound_catalog::DEFAULT_SHELF.into(),
            #[cfg(windows)]
            codex_sound_id: crate::sound_catalog::DEFAULT_CODEX.into(),
            #[cfg(windows)]
            wechat_sound_id: crate::sound_catalog::DEFAULT_WECHAT.into(),
            #[cfg(windows)]
            mail_sound_id: crate::sound_catalog::DEFAULT_MAIL.into(),
            start_page: "media".into(),
            remember_page: true,
            hidden_tabs: Vec::new(),
            pomodoro: PomodoroConfig::default(),
            oyenza_show_amounts: false,
            system_history: true,
            addons: BTreeMap::new(),
            game_stats: false,
            game_peek: false,
            notifications: crate::notifications::NotificationSettings::default(),
            mail: crate::mail::MailSettings::default(),
            codex_enabled: false,
            codex_preview: false,
            #[cfg(windows)]
            wechat_reminders: true,
            #[cfg(windows)]
            wechat_sound: true,
            #[cfg(windows)]
            ui_font: String::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Show over games needs the island on top of full-screen apps, so the
    /// two follow each other: turning the readout on picks Stay on top, and
    /// picking Hide afterwards turns the readout off. `before` is the state
    /// the change started from.
    pub fn pair_game_stats(&mut self, before: &Settings) {
        if !(self.game_stats && self.hide_in_fullscreen) {
            return;
        }
        if before.game_stats && !before.hide_in_fullscreen {
            self.game_stats = false;
        } else {
            self.hide_in_fullscreen = false;
        }
    }

    /// The System page is enabled in Settings.
    pub fn system_on(&self) -> bool {
        self.addons.get("system").copied().unwrap_or(false)
    }

    pub fn set_system(&mut self, on: bool) {
        self.addons.insert("system".into(), on);
    }

    /// The System page's readings go on while it's closed: its history
    /// switch, and only while System itself is on.
    pub fn system_history_on(&self) -> bool {
        self.system_on() && self.system_history
    }

    /// Clamp values a user may have hand-edited into nonsense.
    pub fn sanitized(mut self) -> Self {
        // Only untouched old defaults move.
        // v2: hover opens after a 220 ms dwell (140 ms caught pointers on
        // their way to browser tabs).
        if self.settings_version < 2 && self.hover_delay_ms == 140 {
            self.hover_delay_ms = 220;
        }
        // v3: the island closes as soon as the pointer leaves it (the old
        // 380 ms grace read as lag; coming back while it closes reopens it).
        if self.settings_version < 3 && self.collapse_delay_ms == 380 {
            self.collapse_delay_ms = 0;
        }
        // v4: Clipboard history became an add-on, off by default (it left the
        // app after 0.2.8). A Clipboard start page opens Music; its old data
        // is preserved with the other unknown fields.
        if self.settings_version < 4 && self.start_page == "clipboard" {
            self.start_page = "media".into();
        }
        // v5: Notifications left the add-ons (it's on wherever Windows lets the
        // copy read notifications, and its chip in Settings' tabs row switches
        // it off). One switched off by hand as an add-on stays off, as a hidden
        // tab. Its old key stays in the file, unread, as keys this version
        // doesn't know do.
        if self.settings_version < 5
            && self.addons.get("notifications") == Some(&false)
            && !self.hidden_tabs.iter().any(|k| k == "notifications")
        {
            self.hidden_tabs.push("notifications".into());
        }
        // v6：今天与任务合并。旧版本仍显示任务时，合并后的入口保持可见；
        // 两个旧入口都隐藏时，继续隐藏今天。迁移仅执行一次。
        if self.settings_version < 6 {
            let tasks_hidden = self.hidden_tabs.iter().any(|key| key == "tasks");
            self.hidden_tabs
                .retain(|key| key != "tasks" && (key != "today" || tasks_hidden));
        }
        // v7：全局声音拆成模块开关，旧版静音状态一次性迁移，保存时移除旧键。
        let old_sounds = self.extra.remove("sounds");
        if self.settings_version < 7
            && old_sounds.as_ref().and_then(serde_json::Value::as_bool) == Some(false)
        {
            self.focus_sound = false;
            self.shelf_sound = false;
            self.codex_sound = false;
            self.mail.sound = false;
        }
        // 兼容旧 Windows 版已拆分的声音设置，迁移后删除旧键。
        #[cfg(windows)]
        {
            if let Some(enabled) = self
                .extra
                .remove("focus_sounds")
                .and_then(|value| value.as_bool())
            {
                self.focus_sound = enabled;
            }
            if let Some(enabled) = self
                .extra
                .remove("shelf_sounds")
                .and_then(|value| value.as_bool())
            {
                self.shelf_sound = enabled;
            }
            // LOGFONT 最多保存 31 个 UTF-16 单元；保留完整字符，不截断代理对。
            let mut units = 0;
            self.ui_font = self
                .ui_font
                .trim()
                .chars()
                .filter(|ch| !ch.is_control())
                .take_while(|ch| {
                    units += ch.len_utf16();
                    units <= 31
                })
                .collect();
            // 旧设置缺少字段时由 Default 补齐；手动填写的未知 ID 在读取边界恢复默认。
            for (id, default) in [
                (
                    &mut self.focus_sound_id,
                    crate::sound_catalog::DEFAULT_FOCUS,
                ),
                (
                    &mut self.shelf_sound_id,
                    crate::sound_catalog::DEFAULT_SHELF,
                ),
                (
                    &mut self.codex_sound_id,
                    crate::sound_catalog::DEFAULT_CODEX,
                ),
                (
                    &mut self.wechat_sound_id,
                    crate::sound_catalog::DEFAULT_WECHAT,
                ),
                (&mut self.mail_sound_id, crate::sound_catalog::DEFAULT_MAIL),
            ] {
                if crate::sound_catalog::find(id).is_none() {
                    *id = default.into();
                }
            }
        }
        #[cfg(not(windows))]
        self.alert_sounds.sanitize();
        self.settings_version = self.settings_version.max(SETTINGS_VERSION);
        // Step aside is no longer offered: the island stays over maximized windows.
        self.step_aside = false;
        // Show over games needs the island on top of full-screen apps.
        if self.game_stats {
            self.hide_in_fullscreen = false;
        }
        self.hover_delay_ms = self.hover_delay_ms.min(2000);
        self.lyrics_offset_ms = self.lyrics_offset_ms.clamp(-5000, 5000);
        self.lyrics_color = self.lyrics_color.filter(|&rgb| rgb <= 0xFFFFFF);
        self.collapse_delay_ms = self.collapse_delay_ms.min(5000);
        if !self.expanded_width.is_finite() {
            self.expanded_width = 620.0;
        }
        self.expanded_width = self.expanded_width.clamp(480.0, 900.0);
        self.weather_city = self.weather_city.trim().chars().take(80).collect();
        self.weather_city_coordinates =
            self.weather_city_coordinates
                .filter(|(latitude, longitude)| {
                    !self.weather_city.is_empty()
                        && latitude.is_finite()
                        && longitude.is_finite()
                        && (-90.0..=90.0).contains(latitude)
                        && (-180.0..=180.0).contains(longitude)
                });
        self.island_x = self
            .island_x
            .filter(|x| x.is_finite())
            .map(|x| x.clamp(0.0, 1.0));
        let p = &mut self.pomodoro;
        p.focus_min = p.focus_min.clamp(1, 180);
        p.short_break_min = p.short_break_min.clamp(1, 60);
        p.long_break_min = p.long_break_min.clamp(1, 120);
        p.long_every = p.long_every.min(12);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        // 已移除功能的字段不参与当前设置，但再次保存时保留原始值。
        let s: Settings =
            serde_json::from_str(r#"{"idle_style":"eyes","water":{"goal_ml":1500}}"#).unwrap();
        assert_eq!(s.idle_style, IdleStyle::Eyes);
        assert_eq!(s.pomodoro.focus_min, 25);
        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back["water"]["goal_ml"], 1500);
    }

    #[test]
    fn system_switch_uses_the_existing_key_and_keeps_old_choices() {
        let mut s = Settings::default();
        assert!(!s.system_on());
        s.set_system(true);
        assert!(s.system_on());
        let s: Settings =
            serde_json::from_str(r#"{"addons":{"system":true,"sholat":true,"future":true}}"#)
                .unwrap();
        assert!(s.system_on());
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.addons.get("sholat"), Some(&true));
        assert_eq!(back.addons.get("future"), Some(&true));
        let old: Settings = serde_json::from_str(r#"{"settings_version":3}"#).unwrap();
        assert!(old.addons.is_empty());
    }

    #[test]
    fn theme_defaults_to_classic() {
        // Files written before themes existed keep the black island.
        let s: Settings = serde_json::from_str(r#"{"idle_style":"eyes"}"#).unwrap();
        assert_eq!(s.theme, Theme::Classic);
        let s: Settings = serde_json::from_str(r#"{"theme":"oyen"}"#).unwrap();
        assert_eq!(s.theme, Theme::Oyen);
        let s: Settings = serde_json::from_str(r#"{"theme":"glass"}"#).unwrap();
        assert_eq!(s.theme, Theme::Glass);
        let s: Settings = serde_json::from_str(r#"{"theme":"pill"}"#).unwrap();
        assert_eq!(s.theme, Theme::Pill);
        let s: Settings = serde_json::from_str(r#"{"theme":"diorama"}"#).unwrap();
        assert_eq!(s.theme, Theme::Diorama);
    }

    #[test]
    fn every_theme_is_written_by_its_name_and_read_back() {
        // No `_` arm: a theme added to the enum stops this compiling until
        // it has its name here, and then the loop below asks for it.
        let name = |t: Theme| match t {
            Theme::Classic => "classic",
            Theme::Oyen => "oyen",
            Theme::Glass => "glass",
            Theme::Pill => "pill",
            Theme::Diorama => "diorama",
        };
        for theme in [
            Theme::Classic,
            Theme::Oyen,
            Theme::Glass,
            Theme::Pill,
            Theme::Diorama,
        ] {
            let s = Settings {
                theme,
                ..Settings::default()
            };
            let j = serde_json::to_string(&s).unwrap();
            assert!(
                j.contains(&format!(r#""theme":"{}""#, name(theme))),
                "{theme:?}: {j}"
            );
            let back: Settings = serde_json::from_str(&j).unwrap();
            assert_eq!(back, s, "{theme:?}");
            // It survives the clean-up a loaded file goes through.
            assert_eq!(back.sanitized().theme, theme);
        }
        // A theme this version doesn't know is not read as another one: the
        // file doesn't parse, and `store::load_json` sets it aside for its
        // backup (or the defaults), as it does a damaged file.
        assert!(serde_json::from_str::<Settings>(r#"{"theme":"snowglobe"}"#).is_err());
    }

    #[test]
    fn sanitize_clamps() {
        let s = Settings {
            expanded_width: 5000.0,
            hover_delay_ms: 99999,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.expanded_width, 900.0);
        assert_eq!(s.hover_delay_ms, 2000);
    }

    #[test]
    fn migrates_old_hover_default_once() {
        let s: Settings = serde_json::from_str(r#"{"hover_delay_ms":140}"#).unwrap();
        assert_eq!(s.settings_version, 0);
        let s = s.sanitized();
        assert_eq!(
            (s.hover_delay_ms, s.settings_version),
            (220, SETTINGS_VERSION)
        );
        // A user's own choice is kept, and a current file isn't touched.
        let s: Settings = serde_json::from_str(r#"{"hover_delay_ms":90}"#).unwrap();
        assert_eq!(s.sanitized().hover_delay_ms, 90);
        let s: Settings =
            serde_json::from_str(r#"{"settings_version":2,"hover_delay_ms":140}"#).unwrap();
        assert_eq!(s.sanitized().hover_delay_ms, 140);
    }

    #[test]
    fn files_with_removed_settings_still_load_and_keep_them() {
        let s = serde_json::from_str::<Settings>(
            r#"{"settings_version":4,"clipboard_history":true,"theme":"oyen"}"#,
        )
        .unwrap()
        .sanitized();
        assert_eq!(s.theme, Theme::Oyen);
        assert_eq!(serde_json::to_value(&s).unwrap()["clipboard_history"], true);
        // A Clipboard start page from before 0.2.7 opens Music.
        let s =
            serde_json::from_str::<Settings>(r#"{"settings_version":3,"start_page":"clipboard"}"#)
                .unwrap()
                .sanitized();
        assert_eq!(
            (s.start_page.as_str(), s.settings_version),
            ("media", SETTINGS_VERSION)
        );
    }

    #[test]
    fn system_history_counts_only_while_system_is_on() {
        // Off by default, so nothing reads the counters at rest; its history
        // switch keeps its default for when it is switched on.
        let mut s = Settings::default();
        assert!(!s.system_on() && s.system_history && !s.system_history_on());
        s.set_system(true);
        assert!(s.system_on() && s.system_history_on());
        s.system_history = false;
        assert!(!s.system_history_on());
        // Files from before have no System switch: off.
        let old: Settings =
            serde_json::from_str(r#"{"settings_version":4,"system_history":true}"#).unwrap();
        assert!(!old.sanitized().system_on());
    }

    #[test]
    fn migrates_old_collapse_grace_once() {
        // Files from 0.1.7 and earlier hold the old 380 ms grace.
        let s =
            serde_json::from_str::<Settings>(r#"{"settings_version":2,"collapse_delay_ms":380}"#)
                .unwrap()
                .sanitized();
        assert_eq!(
            (s.collapse_delay_ms, s.settings_version),
            (0, SETTINGS_VERSION)
        );
        let s =
            serde_json::from_str::<Settings>(r#"{"collapse_delay_ms":380,"hover_delay_ms":140}"#)
                .unwrap()
                .sanitized();
        assert_eq!((s.collapse_delay_ms, s.hover_delay_ms), (0, 220));
        // A grace the user chose is kept, a current file isn't touched, and 0 stays 0.
        let s: Settings =
            serde_json::from_str(r#"{"settings_version":2,"collapse_delay_ms":600}"#).unwrap();
        assert_eq!(s.sanitized().collapse_delay_ms, 600);
        let s: Settings =
            serde_json::from_str(r#"{"settings_version":3,"collapse_delay_ms":380}"#).unwrap();
        assert_eq!(s.sanitized().collapse_delay_ms, 380);
        assert_eq!(Settings::default().sanitized().collapse_delay_ms, 0);
        // A file from a newer Poni keeps its version.
        let s: Settings = serde_json::from_str(r#"{"settings_version":9}"#).unwrap();
        assert_eq!(s.sanitized().settings_version, 9);
    }

    #[test]
    fn game_stats_and_full_screen_choice_follow_each_other() {
        // The default hides the island over full-screen apps; turning the
        // readout on keeps it on top instead.
        let before = Settings::default();
        let mut s = Settings {
            game_stats: true,
            ..before.clone()
        };
        s.pair_game_stats(&before);
        assert!(s.game_stats && !s.hide_in_fullscreen);
        // Picking Hide afterwards turns the readout off.
        let before = s.clone();
        let mut s = Settings {
            hide_in_fullscreen: true,
            ..before.clone()
        };
        s.pair_game_stats(&before);
        assert!(!s.game_stats && s.hide_in_fullscreen);
        // Stay on top alone, and the readout off, change nothing.
        let before = Settings {
            hide_in_fullscreen: false,
            ..Settings::default()
        };
        let mut s = before.clone();
        s.pair_game_stats(&before);
        assert_eq!(s, before);
        // A hand-edited file with both on keeps the readout, when changed
        // and when loaded.
        let mut s = Settings {
            game_stats: true,
            ..Settings::default()
        };
        s.pair_game_stats(&Settings::default());
        assert!(s.game_stats && !s.hide_in_fullscreen);
        let s: Settings =
            serde_json::from_str(r#"{"game_stats":true,"hide_in_fullscreen":true}"#).unwrap();
        let s = s.sanitized();
        assert!(s.game_stats && !s.hide_in_fullscreen);
    }

    #[test]
    fn the_readout_stays_unless_peeks_are_picked() {
        // Files from 0.2.0 and before, and from the corner test build (its
        // "peek" was its default, not a choice), get the readout that stays.
        for old in [
            r#"{"game_stats":true}"#,
            r#"{"game_stats":true,"game_readout":"peek"}"#,
            r#"{"game_readout":"corner"}"#,
        ] {
            let s: Settings = serde_json::from_str(old).unwrap();
            assert!(!s.game_peek, "{old}");
        }
        let s: Settings = serde_json::from_str(r#"{"game_stats":true,"game_peek":true}"#).unwrap();
        assert!(s.game_peek);
    }

    #[test]
    fn the_island_sits_in_the_middle_until_dragged() {
        assert_eq!(Settings::default().island_x, None);
        // Files from before the drag, and hand-edited ones, stay on screen.
        let old: Settings = serde_json::from_str(r#"{"settings_version":4}"#).unwrap();
        assert_eq!(old.sanitized().island_x, None);
        let far: Settings = serde_json::from_str(r#"{"island_x":3.5}"#).unwrap();
        assert_eq!(far.sanitized().island_x, Some(1.0));
        let moved: Settings = serde_json::from_str(r#"{"island_x":0.8}"#).unwrap();
        assert_eq!(moved.sanitized().island_x, Some(0.8));
    }

    #[test]
    fn the_island_never_steps_aside() {
        assert!(!Settings::default().step_aside);
        // 0.2.4 and 0.2.5 saved Step aside, their default: the island stays now.
        let old: Settings =
            serde_json::from_str(r#"{"settings_version":3,"step_aside":true}"#).unwrap();
        assert!(!old.sanitized().step_aside);
    }

    #[test]
    fn notifications_switched_off_as_an_add_on_stays_off() {
        // Until 0.2.24 Notifications was an add-on, off by default. One switched
        // off by hand stays off, as a hidden tab.
        let s = serde_json::from_str::<Settings>(
            r#"{"settings_version":4,"addons":{"notifications":false,"sholat":true}}"#,
        )
        .unwrap()
        .sanitized();
        assert_eq!(s.hidden_tabs, ["notifications"]);
        assert_eq!(s.addons.get("sholat"), Some(&true));
        // Switched on, or never touched: on, like everyone's now.
        for file in [
            r#"{"settings_version":4,"addons":{"notifications":true}}"#,
            r#"{"settings_version":4}"#,
        ] {
            let s = serde_json::from_str::<Settings>(file).unwrap().sanitized();
            assert!(s.hidden_tabs.is_empty(), "{file}");
        }
        // Once only: after that, the tabs row is its switch.
        let s = serde_json::from_str::<Settings>(
            r#"{"settings_version":5,"addons":{"notifications":false}}"#,
        )
        .unwrap()
        .sanitized();
        assert!(s.hidden_tabs.is_empty());
    }

    #[test]
    fn every_tab_shows_until_hidden() {
        assert!(Settings::default().hidden_tabs.is_empty());
        // Files from before, and a newer Studio's tab, which stays hidden
        // when this one saves the file.
        let old: Settings = serde_json::from_str(r#"{"settings_version":4}"#).unwrap();
        assert!(old.hidden_tabs.is_empty());
        let s: Settings = serde_json::from_str(r#"{"hidden_tabs":["today","future"]}"#).unwrap();
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.hidden_tabs, ["today", "future"]);
        let migrated = s.sanitized();
        assert_eq!(migrated.hidden_tabs, ["future"]);
        let both: Settings = serde_json::from_str(
            r#"{"settings_version":5,"hidden_tabs":["today","tasks","future"]}"#,
        )
        .unwrap();
        assert_eq!(both.sanitized().hidden_tabs, ["today", "future"]);
        let current: Settings =
            serde_json::from_str(r#"{"settings_version":6,"hidden_tabs":["today"]}"#).unwrap();
        assert_eq!(current.sanitized().hidden_tabs, ["today"]);
    }

    #[test]
    fn roundtrip() {
        let s = Settings {
            lyrics_color: Some(0xF06292),
            ..Settings::default()
        };
        let j = serde_json::to_string_pretty(&s).unwrap();
        let back: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn removed_feature_choices_round_trip_as_raw_data() {
        let old = serde_json::json!({
            "record": {"target":"area","camera":true,"microphone":false,"system_audio":true},
            "usage": {"notices":true,"hidden":["codex"]},
            "prayer": {"place":"jakarta","sound":"adzan"},
            "ielts_spice":"gaul",
            "lyrics_pronunciation":true
        });
        let s: Settings = serde_json::from_value(old.clone()).unwrap();
        let j = serde_json::to_string(&s).unwrap();
        let back: serde_json::Value = serde_json::from_str(&j).unwrap();
        for (key, value) in old.as_object().unwrap() {
            assert_eq!(&back[key], value);
        }
    }
}
