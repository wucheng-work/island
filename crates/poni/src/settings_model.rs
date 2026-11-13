//! 设置的内容模型与配置操作。
//!
//! Section/Block/Item 描述可显示内容，apply 把用户意图写入现有状态和命令队列。
//! 不接触窗口、绘图或输入事件，原生承载窗口共用这一份设置契约。

use crate::commands::Command;
#[cfg(windows)]
use crate::commands::SoundModule;
#[cfg(target_os = "macos")]
use poni_core::alert_sounds::{self, SoundModule as AlertSoundModule};
use crate::icons::Icon;
use crate::pages::PageKind;
use crate::pill::status;
use crate::state::AppState;
#[cfg(windows)]
use crate::ui::wid_n;
use poni_core::geom::{palette, Color};
use poni_core::settings::{IdleStyle, MonitorChoice, Theme};

/// The window's sections, in the sidebar's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    #[cfg(any(windows, target_os = "macos"))]
    Codex,
    General,
    Appearance,
    Island,
    Music,
    Shelf,
    #[cfg(any(windows, target_os = "macos"))]
    Mail,
    Popups,
    Tabs,
    Focus,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::General,
        Section::Appearance,
        Section::Island,
        Section::Music,
        Section::Shelf,
        #[cfg(any(windows, target_os = "macos"))]
        Section::Mail,
        Section::Popups,
        Section::Tabs,
        Section::Focus,
        #[cfg(any(windows, target_os = "macos"))]
        Section::Codex,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Island => "Island",
            Section::Music => "Music and lyrics",
            Section::Shelf => "文件暂存区",
            #[cfg(any(windows, target_os = "macos"))]
            Section::Mail => "邮件",
            Section::Popups => "Pop-ups",
            Section::Tabs => "Tabs",
            Section::Focus => "Focus",
            #[cfg(any(windows, target_os = "macos"))]
            Section::Codex => "Codex 提醒",
        }
    }

    /// For the self-test's file names.
    pub fn key(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::Appearance => "appearance",
            Section::Island => "island",
            Section::Music => "music",
            Section::Shelf => "shelf",
            #[cfg(any(windows, target_os = "macos"))]
            Section::Mail => "mail",
            Section::Popups => "popups",
            Section::Tabs => "tabs",
            Section::Focus => "focus",
            #[cfg(any(windows, target_os = "macos"))]
            Section::Codex => "codex",
        }
    }

    pub(super) fn icon(self) -> Icon {
        match self {
            Section::General => Icon::Gear,
            Section::Appearance => Icon::Eye,
            Section::Island => Icon::AppWindow,
            Section::Music => Icon::Music,
            Section::Shelf => Icon::Tray,
            #[cfg(any(windows, target_os = "macos"))]
            Section::Mail => Icon::Mail,
            Section::Popups => Icon::Volume,
            Section::Tabs => Icon::App,
            Section::Focus => Icon::Timer,
            #[cfg(any(windows, target_os = "macos"))]
            Section::Codex => Icon::Codex,
        }
    }

}

/// The sections the sidebar lists now.
pub fn sections(_st: &AppState) -> Vec<Section> {
    Section::ALL.to_vec()
}

// ---------------------------------------------------------------- rows

/// What a row changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    #[cfg(any(windows, target_os = "macos"))]
    CodexEnabled,
    #[cfg(any(windows, target_os = "macos"))]
    CodexPreview,
    #[cfg(any(windows, target_os = "macos"))]
    CodexGuide,
    Autostart,
    FocusSound,
    ShelfSound,
    #[cfg(windows)]
    WeChatReminders,
    #[cfg(windows)]
    WeChatSound,
    #[cfg(windows)]
    ChooseFont,
    #[cfg(windows)]
    ResetFont,
    #[cfg(windows)]
    ChooseSound(SoundModule),
    #[cfg(any(windows, target_os = "macos"))]
    CodexSound,
    #[cfg(target_os = "macos")]
    AlertTone(AlertSoundModule),
    Clock24,
    SystemHistory,
    IdleLook,
    HoverDelay,
    CloseDelay,
    Width,
    Display,
    Fullscreen,
    GameStats,
    GamePeek,
    Lyrics,
    MediaActivity,
    LyricsOffset,
    LyricsColor,
    ResetLyricsColor,
    HudVolume,
    HudPower,
    HudBluetooth,
    RememberPage,
    StartPage,
    Tab(PageKind),
    FocusMin,
    ShortBreak,
    LongBreak,
    LongEvery,
    AutoBreaks,
    AutoFocus,
    DataFolder,
    #[cfg(any(windows, target_os = "macos"))]
    MailAdd,
    #[cfg(any(windows, target_os = "macos"))]
    MailMaster,
    #[cfg(any(windows, target_os = "macos"))]
    MailSoundMaster,
    #[cfg(any(windows, target_os = "macos"))]
    MailDetails(usize),
    #[cfg(any(windows, target_os = "macos"))]
    MailQuiet,
    #[cfg(any(windows, target_os = "macos"))]
    MailEdit(usize),
    #[cfg(target_os = "macos")]
    MailAuthorize(usize),
    #[cfg(any(windows, target_os = "macos"))]
    MailEnabled(usize),
    #[cfg(any(windows, target_os = "macos"))]
    MailNotify(usize),
    #[cfg(any(windows, target_os = "macos"))]
    MailSound(usize),
    #[cfg(any(windows, target_os = "macos"))]
    MailPrivacy(usize),
    #[cfg(any(windows, target_os = "macos"))]
    MailRefresh(usize),
}

/// A row's control, at its right end.
#[derive(Clone, Debug, PartialEq)]
pub enum Ctl {
    /// `None` = not known yet (Start with Windows until it's read): dimmed.
    Switch(Option<bool>),
    /// A picker; `None` = none of its choices (a value typed into the file).
    Choice(Vec<&'static str>, Option<usize>),
    /// − and + around the value.
    Stepper(String),
    /// A button with this label.
    Button(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub key: Key,
    pub label: String,
    pub sub: Option<String>,
    pub ctl: Ctl,
    /// 账户摘要等行右侧的状态圆点与简短文字。
    pub status: Option<(String, Color)>,
    /// 需要醒目呈现的失败，正常说明仍使用次要文字。
    pub error: bool,
}

fn item(key: Key, label: &str, sub: Option<&str>, ctl: Ctl) -> Item {
    Item { key, label: label.to_string(), sub: sub.map(str::to_string), ctl, status: None, error: false }
}

fn switch(key: Key, label: &str, sub: Option<&str>, on: bool) -> Item {
    item(key, label, sub, Ctl::Switch(Some(on)))
}

#[cfg(windows)]
fn sound_choice(module: SoundModule, selected: &str) -> Item {
    let name = poni_core::sound_catalog::find(selected).map_or("选择声音", |sound| sound.name);
    item(Key::ChooseSound(module), "提示音类型", Some("从内置声音中选择，选择后立即试听"), Ctl::Button(name))
}

#[cfg(target_os = "macos")]
fn alert_tone(st: &AppState, module: AlertSoundModule) -> Item {
    let id = st.settings.alert_sounds.get(module);
    let name = alert_sounds::tone(id).map_or("默认提示音", |tone| tone.name);
    item(Key::AlertTone(module), "提示音样式", Some(name), Ctl::Button("更改…"))
}

/// A section's content, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// Rows on one card, with a caption above it.
    Group(Option<&'static str>, Vec<Item>),
    /// 账户摘要、展开详情与原生编辑表单共用一张卡片。
    #[cfg(any(windows, target_os = "macos"))]
    MailAccount { title: Option<&'static str>, items: Vec<Item>, editor_after: Option<usize> },
    /// This copy's version, Check now, and a new version when there is one.
    Version,
    /// The themes as cards with a small picture of each.
    Themes,
}

/// What a click on a row's control did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Toggle,
    Pick(usize),
    Step(i32),
    Press,
}

/// How long the pointer rests on the island before it opens (ms).
const HOVER: [(u32, &str); 3] = [(120, "Quick"), (220, "Normal"), (450, "Slow")];
/// How long it stays open after the pointer leaves (ms).
const CLOSE: [(u32, &str); 3] = [(0, "At once"), (400, "Short"), (1000, "Long")];
const DISPLAYS: [(MonitorChoice, &str); 2] = [(MonitorChoice::Primary, "Main display"), (MonitorChoice::Cursor, "Under the pointer at startup")];
const FULLSCREEN: [(bool, &str); 2] = [(true, "Hide"), (false, "Stay on top")];
/// Start with Windows, or at login on a Mac (its menu bar says so).
const AUTOSTART: (&str, &str) = if cfg!(target_os = "macos") { ("Start at login", "登录时打开 Island For Mac") } else { ("Start with Windows", "登录时打开 Island For Windows") };
const WIDTH_STEP: f32 = 20.0;
const OFFSET_STEP: i32 = 100;

fn chosen<T: PartialEq>(list: &[(T, &str)], v: T) -> Option<usize> {
    list.iter().position(|(x, _)| *x == v)
}

fn names<T>(list: &[(T, &'static str)]) -> Vec<&'static str> {
    list.iter().map(|(_, n)| *n).collect()
}

/// The preset nearest to `ms` (a hand-edited file's value lands on one).
fn nearest(list: &[(u32, &str)], ms: u32) -> Option<usize> {
    list.iter().enumerate().min_by_key(|(_, (v, _))| v.abs_diff(ms)).map(|(i, _)| i)
}

fn idle_name(s: IdleStyle, theme: Theme) -> &'static str {
    match s {
        IdleStyle::Clock => "Clock",
        IdleStyle::Eyes if theme == Theme::Oyen => "Oyen",
        IdleStyle::Eyes => "Eyes",
        IdleStyle::Minimal => "Minimal",
        IdleStyle::Hidden => "Hidden",
        IdleStyle::Status => "Status",
    }
}

/// The pages the island may open on: its tabs, but not the Add-ons "+".
pub(super) fn start_pages(st: &AppState) -> Vec<PageKind> {
    crate::pages::tabs(st)
}

fn minutes(m: u32) -> String {
    format!("{m} 分钟")
}

/// The lyrics' shift as people say it.
pub(super) fn offset_text(ms: i32) -> String {
    match ms {
        0 => "On time".into(),
        _ => format!("{}{:.1} 秒", if ms > 0 { "−" } else { "+" }, ms.unsigned_abs() as f32 / 1000.0),
    }
}

/// What `section` shows for `st`.
pub fn blocks(section: Section, st: &AppState) -> Vec<Block> {
    let s = &st.settings;
    match section {
        #[cfg(any(windows, target_os = "macos"))]
        Section::Codex => vec![
            Block::Group(
                Some("提醒与隐私"),
                vec![
                    switch(Key::CodexEnabled, "启用 Codex 提醒", Some("仅监听本机；关闭时清空消息并取消等待"), s.codex_enabled),
                    switch(Key::CodexSound, "提示音", Some("新消息及复制内容时播放短提示音"), s.codex_sound),
                    #[cfg(windows)]
                    sound_choice(SoundModule::Codex, &s.codex_sound_id),
                    #[cfg(target_os = "macos")]
                    alert_tone(st, AlertSoundModule::Codex),
                    switch(Key::CodexPreview, "显示消息预览", Some("开启后在提醒和列表显示正文；锁屏时隐藏"), s.codex_preview),
                ],
            ),
            Block::Group(Some("接入"), vec![item(Key::CodexGuide, "连接 Codex", Some("配置完成提醒与岛内快捷回答"), Ctl::Button("接入说明"))]),
        ],
        Section::General => {
            let mut rows = vec![
                item(Key::Autostart, AUTOSTART.0, Some(AUTOSTART.1), Ctl::Switch(st.ui.autostart)),
                switch(Key::Clock24, "24-hour clock", None, s.use_24h),
            ];
            if s.system_on() {
                rows.push(switch(Key::SystemHistory, "Keep System page history", Some("The charts keep the last minute while the page is closed"), s.system_history));
            }
            vec![
                Block::Version,
                Block::Group(None, rows),
                Block::Group(Some("Files"), vec![item(Key::DataFolder, "Data folder", Some("Tasks, the Shelf and your Focus history"), Ctl::Button("Open"))]),
            ]
        }
        Section::Appearance => {
            let looks = status::looks(s.theme);
            let look_names = looks.iter().map(|l| idle_name(*l, s.theme)).collect();
            let shown = status::shown(s.theme, s.idle_style);
            vec![
                Block::Themes,
                #[cfg(windows)]
                Block::Group(Some("字体"), vec![
                    item(Key::ChooseFont, "界面字体", Some(if s.ui_font.is_empty() { "跟随主题，自动回退中文字体" } else { &s.ui_font }), Ctl::Button("选择字体")),
                    item(Key::ResetFont, "恢复主题字体", Some("保留原有字号和粗细层级"), Ctl::Button("恢复默认")),
                ]),
                Block::Group(Some("Island at rest"), vec![item(Key::IdleLook, "Idle look", Some("What the closed island shows"), Ctl::Choice(look_names, looks.iter().position(|l| *l == shown)))]),
            ]
        }
        Section::Island => {
            let mut shows = vec![item(
                Key::Display,
                "Display",
                Some(if cfg!(target_os = "macos") { "Chosen at startup or when changed; does not follow the pointer" } else { "The screen the island hangs on" }),
                Ctl::Choice(names(&DISPLAYS), chosen(&DISPLAYS, s.monitor)),
            )];
            shows.push(item(Key::Fullscreen, "In full-screen apps", Some("Games and videos that fill the screen"), Ctl::Choice(names(&FULLSCREEN), chosen(&FULLSCREEN, s.hide_in_fullscreen))));
            if cfg!(windows) {
                shows.push(switch(Key::GameStats, "Show FPS, GPU and CPU over games", Some("Keeps the island on top of full-screen apps"), s.game_stats));
                shows.push(switch(Key::GamePeek, "Only for a few seconds", Some("Then the island hides and adds no delay to the game"), s.game_peek));
            }
            vec![
                Block::Group(
                    Some("Opening and closing"),
                    vec![
                        item(Key::HoverDelay, "Opens after", Some("How long the pointer rests on the island first"), Ctl::Choice(names(&HOVER), nearest(&HOVER, s.hover_delay_ms))),
                        item(Key::CloseDelay, "Closes", Some("Once the pointer leaves the open island"), Ctl::Choice(names(&CLOSE), nearest(&CLOSE, s.collapse_delay_ms))),
                        item(Key::Width, "Width when open", Some("Wider gives the pages more room"), Ctl::Stepper(format!("{:.0}", s.expanded_width))),
                    ],
                ),
                Block::Group(Some("Where it shows"), shows),
            ]
        }
        Section::Music => vec![
            Block::Group(
                Some("Lyrics"),
                vec![
                    switch(Key::Lyrics, "Synced lyrics", Some("Lyrics come from the current player's service"), s.lyrics),
                    item(Key::LyricsOffset, "Timing", Some("Shift every line earlier or later"), Ctl::Stepper(offset_text(s.lyrics_offset_ms))),
                    item(Key::LyricsColor, "当前歌词颜色", Some("音乐页与收起状态使用相同颜色"), Ctl::Button("选择颜色")),
                    item(Key::ResetLyricsColor, "恢复歌词默认颜色", Some("默认蓝色会随主题适配"), Ctl::Button("恢复默认")),
                ],
            ),
            Block::Group(Some("Closed island"), vec![switch(Key::MediaActivity, "Show what's playing", Some("The song and its cover on the closed island"), s.show_media_activity)]),
        ],
        Section::Shelf => vec![Block::Group(Some("声音"), vec![
            switch(Key::ShelfSound, "添加文件提示音", Some("新文件成功加入暂存区时播放；重复文件不播放"), s.shelf_sound),
            #[cfg(windows)]
            sound_choice(SoundModule::Shelf, &s.shelf_sound_id),
            #[cfg(target_os = "macos")]
            alert_tone(st, AlertSoundModule::Shelf),
        ])],
        #[cfg(any(windows, target_os = "macos"))]
        Section::Mail => mail_blocks(st),
        Section::Popups => vec![Block::Group(
            Some("The island shows a pop-up for"),
            vec![
                switch(Key::HudVolume, "Volume", Some("When you change the volume or mute"), s.hud_volume),
                switch(Key::HudPower, "Charger and battery", Some("When you plug in, unplug or run low"), s.hud_power),
                switch(Key::HudBluetooth, "Bluetooth", Some("When headphones and other devices connect"), s.hud_bluetooth),
            ],
        ),
        #[cfg(windows)]
        Block::Group(Some("微信"), vec![
            switch(Key::WeChatReminders, "微信消息提醒", Some("微信任务栏闪烁时显示通用提示，不读取聊天内容"), s.wechat_reminders),
            switch(Key::WeChatSound, "微信提示音", Some("新提醒出现时播放；同一轮闪烁只响一次"), s.wechat_sound),
            sound_choice(SoundModule::WeChat, &s.wechat_sound_id),
        ])],
        Section::Tabs => {
            let pages = start_pages(st);
            let start = PageKind::from_key(&s.start_page);
            let start_name = if pages.contains(&start) { start.title() } else { pages.first().map_or("Music", |p| p.title()) };
            let mut open = vec![switch(Key::RememberPage, "Open where you left off", Some("The page you last had open"), s.remember_page)];
            if !s.remember_page {
                open.push(item(Key::StartPage, "Opens on", None, Ctl::Stepper(start_name.to_string())));
            }
            // Notifications only in a copy that can read them; off, nothing listens.
            let tabs = crate::pages::hideable_tabs(st)
                .iter()
                .map(|&p| {
                    let sub = (p == PageKind::Notifications).then_some("Messages from WhatsApp, Telegram and your phone");
                    item(Key::Tab(p), p.title(), sub, Ctl::Switch(Some(!st.tab_hidden(p))))
                })
                .collect();
            vec![Block::Group(Some("When the island opens"), open), Block::Group(Some("Tabs in the island"), tabs)]
        }
        Section::Focus => {
            let p = &s.pomodoro;
            let every = match p.long_every {
                0 => "Never".to_string(),
                1 => "1 次".to_string(),
                n => format!("{n} 次"),
            };
            vec![
                Block::Group(Some("声音"), vec![
                    switch(Key::FocusSound, "专注提示音", Some("开始、暂停、重置、切换阶段及计时结束时播放"), s.focus_sound),
                    #[cfg(windows)]
                    sound_choice(SoundModule::Focus, &s.focus_sound_id),
                    #[cfg(target_os = "macos")]
                    alert_tone(st, AlertSoundModule::Focus),
                ]),
                Block::Group(
                    Some("Lengths"),
                    vec![
                        item(Key::FocusMin, "Focus", Some("Duration changes apply to the next session"), Ctl::Stepper(minutes(p.focus_min))),
                        item(Key::ShortBreak, "Short break", Some("Duration changes apply to the next session"), Ctl::Stepper(minutes(p.short_break_min))),
                        item(Key::LongBreak, "Long break", Some("Duration changes apply to the next session"), Ctl::Stepper(minutes(p.long_break_min))),
                        item(Key::LongEvery, "Long break after", Some("Focus sessions before a long break"), Ctl::Stepper(every)),
                    ],
                ),
                Block::Group(
                    Some("Starting by itself"),
                    vec![
                        switch(Key::AutoBreaks, "Breaks", Some("A break starts when a focus session ends"), p.auto_start_breaks),
                        switch(Key::AutoFocus, "Focus sessions", Some("The next session starts when a break ends"), p.auto_start_focus),
                    ],
                ),
            ]
        }
    }
}

/// The widget id of `key`'s row in `section` (the self-test's film).
#[cfg(windows)]
pub fn row_id(section: Section, st: &AppState, key: Key) -> Option<u64> {
    blocks(section, st).iter().enumerate().find_map(|(bi, b)| match b {
        Block::Group(_, items) => items.iter().position(|it| it.key == key).map(|ri| wid_n(&format!("settingswin.row.{bi}"), ri as u64)),
        _ => None,
    })
}

#[cfg(any(windows, target_os = "macos"))]
fn mail_blocks(st: &AppState) -> Vec<Block> {
    use crate::mail::MailEditor;
    use poni_core::mail::MailPrivacy;

    let mut controls = vec![switch(Key::MailMaster, "启用邮件", Some("关闭后停止收信，保留已添加的账户"), st.settings.mail.enabled)];
    if st.settings.mail.enabled {
        controls.push(switch(Key::MailSoundMaster, "邮件提示音", Some("关闭后仍显示通知，账户声音开关继续单独生效"), st.settings.mail.sound));
        #[cfg(windows)]
        controls.push(sound_choice(SoundModule::Mail, &st.settings.mail_sound_id));
        #[cfg(target_os = "macos")]
        controls.push(alert_tone(st, AlertSoundModule::Mail));
        controls.push(switch(Key::MailQuiet, "专注时静默提醒", Some("专注结束后再显示新邮件提醒"), st.settings.mail.quiet_during_focus));
    }
    // 相关开关放在同一张卡片，账户与展开表单继续使用各自的卡片。
    let mut blocks = vec![Block::Group(Some("收信与提醒"), controls)];
    if !st.settings.mail.enabled {
        return blocks;
    }
    let mut add = item(
        Key::MailAdd,
        "添加邮箱",
        Some(st.mail.account_error.as_deref().unwrap_or("QQ、Gmail、iCloud、Aruba，支持多个账户")),
        Ctl::Button(if st.mail.editor == Some(MailEditor::New) { "收起" } else { "添加" }),
    );
    add.error = st.mail.account_error.is_some();
    blocks.push(Block::MailAccount {
        title: Some("邮箱账户"),
        items: vec![add],
        editor_after: if st.mail.editor == Some(MailEditor::New) { Some(0) } else { None },
    });
    for (index, account) in st.settings.mail.accounts.iter().enumerate() {
        let expanded = st.mail.expanded_accounts.contains(&account.id);
        let state = st.mail.statuses.get(&account.id).cloned().unwrap_or_default();
        let (status_label, status_color) = if account.enabled {
            (state.short_label(), state.color())
        } else {
            ("未启用", palette::TEXT_DIM)
        };
        let mut header = item(Key::MailDetails(index), &account.name, Some(&account.address), Ctl::Button("移除"));
        header.status = Some((status_label.into(), status_color));
        if !expanded {
            blocks.push(Block::MailAccount { title: None, items: vec![header], editor_after: None });
            continue;
        }
        if st.mail.busy_accounts.contains(&account.id) {
            blocks.push(Block::MailAccount {
                title: None,
                items: vec![
                    header,
                    item(Key::MailRefresh(index), "账户操作", Some(if cfg!(target_os = "macos") { "正在保存、授权或移除账户，请稍候…" } else { "正在保存或移除账户，请稍候…" }), Ctl::Button("等待中")),
                ],
                editor_after: None,
            });
            continue;
        }
        let status = state.label();
        #[cfg(target_os = "macos")]
        let needs_authorization = matches!(state, crate::mail::MailConnection::AuthorizationRequired(_));
        let mut connection = item(Key::MailRefresh(index), "连接状态", Some(&status), Ctl::Button("刷新"));
        #[cfg(target_os = "macos")]
        if needs_authorization && account.enabled {
            connection = item(Key::MailAuthorize(index), "等待手动授权", Some(&status), Ctl::Button("授权"));
        }
        connection.error = account.enabled && matches!(state, crate::mail::MailConnection::Error(_));
        if connection.error {
            connection.label = "连接失败".into();
        }
        let privacy = match account.privacy {
            MailPrivacy::CountOnly => 0,
            MailPrivacy::SenderAndSubject => 1,
            MailPrivacy::Preview => 2,
        };
        let mut items = vec![
            header,
            item(Key::MailEdit(index), "账户信息", Some("修改邮箱地址、服务器或授权信息"), Ctl::Button(if st.mail.editor == Some(MailEditor::Edit(account.id.clone())) { "收起" } else { "编辑" })),
            connection,
        ];
        #[cfg(target_os = "macos")]
        if account.enabled && !needs_authorization {
            items.push(item(Key::MailAuthorize(index), "钥匙串授权", Some("仅手动授权会弹窗，后台收信和刷新不会请求授权"), Ctl::Button("授权")));
        }
        items.extend([
            switch(Key::MailEnabled(index), "启用此邮箱", Some("关闭后停止此账户的后台收信"), account.enabled),
            switch(Key::MailNotify(index), "新邮件提醒", None, account.notify),
            switch(Key::MailSound(index), "此邮箱提示音", Some("同时受上方邮件提示音开关控制"), account.sound),
            item(Key::MailPrivacy(index), "提醒内容", None, Ctl::Choice(vec!["仅数量", "发件人与主题", "含正文摘要"], Some(privacy))),
        ]);
        let editing = st.mail.editor == Some(MailEditor::Edit(account.id.clone()));
        blocks.push(Block::MailAccount { title: None, items, editor_after: if editing { Some(1) } else { None } });
    }
    blocks
}

/// The widget id of `section`'s row in the sidebar (the self-test's film).
#[cfg(windows)]
pub fn nav_id(st: &AppState, section: Section) -> Option<u64> {
    sections(st).iter().position(|s| *s == section).map(|i| wid_n("settingswin.nav", i as u64))
}

/// Step a number of minutes: by 5, by 1 below 5.
pub(super) fn step_min(v: u32, d: i32, lo: u32, hi: u32) -> u32 {
    let by = if (d < 0 && v <= 5) || (d > 0 && v < 5) { 1 } else { 5 };
    (v as i64 + d as i64 * by).clamp(lo as i64, hi as i64) as u32
}

/// Carry out `act` on the row `key`. Settings save through
/// `Command::SaveSettings`, as the island's pages save them.
pub fn apply(st: &mut AppState, key: Key, act: Act) {
    #[cfg(any(windows, target_os = "macos"))]
    {
        use crate::mail::MailAction;
        use poni_core::mail::MailPrivacy;

        #[cfg(target_os = "macos")]
        if let Key::AlertTone(module) = key {
            st.cmd(Command::ChooseAlertTone(module));
            return;
        }

        let account_index = match key {
            Key::MailEdit(i) | Key::MailRefresh(i) | Key::MailEnabled(i) | Key::MailNotify(i) | Key::MailSound(i) | Key::MailPrivacy(i) => Some(i),
            #[cfg(target_os = "macos")]
            Key::MailAuthorize(i) => Some(i),
            _ => None,
        };
        if account_index.and_then(|i| st.settings.mail.accounts.get(i)).is_some_and(|a| st.mail.busy_accounts.contains(&a.id)) {
            return;
        }

        let action = match key {
            Key::MailAdd => Some(MailAction::EditAccount(None)),
            Key::MailEdit(index) => st.settings.mail.accounts.get(index).map(|account| MailAction::EditAccount(Some(account.id.clone()))),
            Key::MailRefresh(index) => st.settings.mail.accounts.get(index).map(|account| MailAction::Refresh(Some(account.id.clone()))),
            #[cfg(target_os = "macos")]
            Key::MailAuthorize(index) => st.settings.mail.accounts.get(index).filter(|account| account.enabled).map(|account| MailAction::Authorize(account.id.clone())),
            // 标题行单击切换详情，右侧“移除”按钮进入原有确认流程。
            Key::MailDetails(index) if matches!(act, Act::Press) => st.settings.mail.accounts.get(index).map(|account| MailAction::ConfirmRemove(account.id.clone())),
            _ => None,
        };
        if let Some(action) = action {
            st.cmd(Command::Mail(action));
            return;
        }
        match key {
            Key::MailDetails(index) => {
                if let Some(account) = st.settings.mail.accounts.get(index) {
                    let id = account.id.clone();
                    if !st.mail.expanded_accounts.remove(&id) {
                        st.mail.expanded_accounts.insert(id);
                    } else if st.mail.editor.as_ref() == Some(&crate::mail::MailEditor::Edit(id)) {
                        st.cmd(Command::Mail(MailAction::CloseEditor));
                    }
                }
                return;
            }
            Key::MailMaster => {
                if !st.mail.busy_accounts.is_empty() {
                    st.mail.account_error = Some("请等待账户保存或移除完成后再切换邮件功能".into());
                    return;
                }
                st.settings.mail.enabled = !st.settings.mail.enabled;
                st.cmd(Command::SaveSettings);
                return;
            }
            Key::MailEnabled(index) | Key::MailNotify(index) | Key::MailSound(index) | Key::MailPrivacy(index) => {
                if let Some(account) = st.settings.mail.accounts.get_mut(index) {
                    match key {
                        Key::MailEnabled(_) => account.enabled = !account.enabled,
                        Key::MailNotify(_) => account.notify = !account.notify,
                        Key::MailSound(_) => account.sound = !account.sound,
                        Key::MailPrivacy(_) => match act {
                            Act::Pick(0) => account.privacy = MailPrivacy::CountOnly,
                            Act::Pick(1) => account.privacy = MailPrivacy::SenderAndSubject,
                            Act::Pick(2) => account.privacy = MailPrivacy::Preview,
                            _ => return,
                        },
                        _ => {}
                    }
                    account.revision = account.revision.wrapping_add(1);
                    st.cmd(Command::SaveSettings);
                }
                return;
            }
            Key::MailQuiet => {
                st.settings.mail.quiet_during_focus = !st.settings.mail.quiet_during_focus;
                st.cmd(Command::SaveSettings);
                return;
            }
            Key::MailSoundMaster => {
                st.settings.mail.sound = !st.settings.mail.sound;
                st.cmd(Command::SaveSettings);
                return;
            }
            Key::MailAdd | Key::MailEdit(_) | Key::MailRefresh(_) => return,
            #[cfg(target_os = "macos")]
            Key::MailAuthorize(_) => return,
            _ => {}
        }
    }
    let theme = st.settings.theme;
    let s = &mut st.settings;
    let step = if let Act::Step(d) = act { d } else { 0 };
    match key {
        #[cfg(any(windows, target_os = "macos"))]
        Key::CodexEnabled => s.codex_enabled = !s.codex_enabled,
        #[cfg(any(windows, target_os = "macos"))]
        Key::CodexPreview => s.codex_preview = !s.codex_preview,
        #[cfg(any(windows, target_os = "macos"))]
        Key::CodexGuide => {
            st.cmd(Command::CodexGuide);
            return;
        }
        Key::Autostart => {
            if let Some(on) = st.ui.autostart {
                st.cmd(Command::SetAutostart(!on));
            }
            return;
        }
        Key::FocusSound => s.focus_sound = !s.focus_sound,
        Key::ShelfSound => s.shelf_sound = !s.shelf_sound,
        #[cfg(windows)]
        Key::WeChatReminders => s.wechat_reminders = !s.wechat_reminders,
        #[cfg(windows)]
        Key::WeChatSound => s.wechat_sound = !s.wechat_sound,
        #[cfg(windows)]
        Key::ChooseFont => {
            st.cmd(Command::ChooseFont);
            return;
        }
        #[cfg(windows)]
        Key::ChooseSound(module) => {
            st.cmd(Command::ChooseSound(module));
            return;
        }
        #[cfg(windows)]
        Key::ResetFont => s.ui_font.clear(),
        #[cfg(any(windows, target_os = "macos"))]
        Key::CodexSound => s.codex_sound = !s.codex_sound,
        #[cfg(target_os = "macos")]
        Key::AlertTone(_) | Key::MailAuthorize(_) => return,
        Key::Clock24 => s.use_24h = !s.use_24h,
        Key::SystemHistory => s.system_history = !s.system_history,
        Key::IdleLook => {
            let looks = status::looks(theme);
            match act {
                Act::Pick(i) if i < looks.len() => s.idle_style = looks[i],
                _ => return,
            }
        }
        Key::HoverDelay => match act {
            Act::Pick(i) if i < HOVER.len() => s.hover_delay_ms = HOVER[i].0,
            _ => return,
        },
        Key::CloseDelay => match act {
            Act::Pick(i) if i < CLOSE.len() => s.collapse_delay_ms = CLOSE[i].0,
            _ => return,
        },
        Key::Width => s.expanded_width = (s.expanded_width + step as f32 * WIDTH_STEP).clamp(480.0, 900.0),
        Key::Display => match act {
            Act::Pick(i) if i < DISPLAYS.len() => s.monitor = DISPLAYS[i].0,
            _ => return,
        },
        Key::Fullscreen => match act {
            Act::Pick(i) if i < FULLSCREEN.len() => s.hide_in_fullscreen = FULLSCREEN[i].0,
            _ => return,
        },
        Key::GameStats => s.game_stats = !s.game_stats,
        Key::GamePeek => s.game_peek = !s.game_peek,
        Key::Lyrics => s.lyrics = !s.lyrics,
        Key::MediaActivity => s.show_media_activity = !s.show_media_activity,
        // + shows the lines later, as people hear a late line.
        Key::LyricsOffset => s.lyrics_offset_ms = (s.lyrics_offset_ms - step * OFFSET_STEP).clamp(-5000, 5000),
        Key::LyricsColor => {
            st.cmd(Command::ChooseLyricsColor);
            return;
        }
        Key::ResetLyricsColor => s.lyrics_color = None,
        Key::HudVolume => s.hud_volume = !s.hud_volume,
        Key::HudPower => s.hud_power = !s.hud_power,
        Key::HudBluetooth => s.hud_bluetooth = !s.hud_bluetooth,
        Key::RememberPage => s.remember_page = !s.remember_page,
        Key::StartPage => {
            let pages = start_pages(st);
            if pages.is_empty() || step == 0 {
                return;
            }
            let at = pages.iter().position(|p| p.key() == st.settings.start_page).unwrap_or(0) as i32;
            let next = (at + step).rem_euclid(pages.len() as i32) as usize;
            st.settings.start_page = pages[next].key().to_string();
        }
        Key::Tab(p) => {
            if p == PageKind::System {
                s.set_system(!s.system_on());
            } else {
                let hidden = &mut s.hidden_tabs;
                match hidden.iter().position(|k| k == p.key()) {
                    Some(i) => {
                        hidden.remove(i);
                    }
                    None => hidden.push(p.key().to_string()),
                }
            }
        }
        Key::FocusMin => s.pomodoro.focus_min = step_min(s.pomodoro.focus_min, step, 1, 180),
        Key::ShortBreak => s.pomodoro.short_break_min = (s.pomodoro.short_break_min as i32 + step).clamp(1, 60) as u32,
        Key::LongBreak => s.pomodoro.long_break_min = step_min(s.pomodoro.long_break_min, step, 1, 120),
        Key::LongEvery => s.pomodoro.long_every = (s.pomodoro.long_every as i32 + step).clamp(0, 12) as u32,
        Key::AutoBreaks => s.pomodoro.auto_start_breaks = !s.pomodoro.auto_start_breaks,
        Key::AutoFocus => s.pomodoro.auto_start_focus = !s.pomodoro.auto_start_focus,
        Key::DataFolder => {
            st.cmd(Command::OpenPath(crate::paths::data_dir().to_string_lossy().into_owned()));
            return;
        }
        #[cfg(any(windows, target_os = "macos"))]
        Key::MailAdd
        | Key::MailMaster
        | Key::MailSoundMaster
        | Key::MailDetails(_)
        | Key::MailQuiet
        | Key::MailEdit(_)
        | Key::MailEnabled(_)
        | Key::MailNotify(_)
        | Key::MailSound(_)
        | Key::MailPrivacy(_)
        | Key::MailRefresh(_)
        => return,
    }
    st.cmd(Command::SaveSettings);
}
