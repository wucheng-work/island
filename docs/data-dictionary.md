# 数据字典

本文说明 Island 主要持久化数据及影响业务行为的运行态字段，供修改共享业务、系统适配和界面时查阅。以当前源码为准；不列出全部绘制坐标、鼠标命中区、动画和输入框临时字段。Mac 与 Windows 共用模型和规则，保留已有文件格式及系统凭据边界。

## 1. 单位、时间和身份

| 约定 | 含义 | 使用限制 |
| --- | --- | --- |
| `*_unix`、邮件令牌 `expires_at` | 自 Unix epoch 起的秒数 | 表示墙上时间，可转本地日期；不是毫秒 |
| `*_day` | 公历日期自 1970-01-01 起的天数 | 待办使用设备本地日期；不能直接用 Unix 秒除以 86400 替代 |
| `YYYY-MM-DD` | 本地公历日期字符串 | 专注统计键；不是 UTC 日期 |
| `now`、`sampled_at`、`ends_at` | 本次进程使用的单调时钟秒数 | 用于间隔和倒计时，不能和 Unix 时间比较或跨重启持久化 |
| `*_ms` | 毫秒 | 歌词偏移和交互延迟使用整数毫秒 |
| `*_min` | 分钟 | 专注设置；状态机开始时换算为秒 |
| 宽度、界面坐标 | DIP 逻辑点 | 不是显示器物理像素 |
| `level`、进度比例 | `0..=1` | 界面百分比需乘以 100 |
| `u32` RGB | `0xRRGGBB` | 无透明度通道；不把十进制 JSON 值解释成颜色字符串 |
| `id`、`revision`、`generation` | 各自作用域内的身份或版本 | 账户版本、歌词请求代次、消息 ID、歌曲 ID 不能混用 |

应用时钟来源见 [clock.rs](../crates/poni/src/clock.rs)，主要状态见 [state.rs](../crates/poni/src/state.rs)。Rust `String::chars()` 的长度限制按 Unicode 标量值计数，不等于 UTF-8 字节数或用户感知的字形数；只有明确写明字节或 UTF-16 的字段使用其他口径。

## 2. 文件位置、加载和兼容

路径入口：[Windows paths.rs](../crates/poni/src/paths.rs)、[Mac paths.rs](../crates/poni/src/mac/paths.rs)。合并源码不更名已有 `Poni` 数据目录，不自动迁移另一台设备的数据。

| 内容 | Mac | Windows | 性质 |
| --- | --- | --- | --- |
| 数据目录 | `~/Library/Application Support/Poni/` | `%APPDATA%\Poni\` | 用户数据 |
| 缓存目录 | `~/Library/Caches/Poni/` | `%LOCALAPPDATA%\Poni\` | 可重新获取的数据及本机缓存 |
| 主日志 | `~/Library/Logs/Poni/poni.log` | 缓存目录下 `poni.log` | 排查日志 |
| `settings.json` | 数据目录 | 数据目录 | 设置及非敏感邮箱配置 |
| `data.json` | 数据目录 | 数据目录 | 待办、暂存区引用、专注统计等 |
| `mail/<account.id>.json` | 数据目录 | 数据目录 | 单账户邮件摘要和同步游标 |
| `lyrics/` | 缓存目录 | 缓存目录 | 歌词缓存；不是业务 Store |

`PONI_HOME` 用于隔离预览或自测：指定后数据写到该目录，缓存写到其 `cache/`；Mac 日志写到其 `poni.log`，Windows 日志写到其 `cache/poni.log`。它不是设备同步或凭据迁移入口。安装包不应包含本机设置、待办、邮件索引或凭据。

### 设置与 Store 的文件语义

[store.rs](../crates/poni-core/src/store.rs) 的 `load_json` / `save_json_atomic` 同时服务设置和业务 Store：

| `LoadOutcome` | 含义 | 后续行为 |
| --- | --- | --- |
| `Loaded` | 正常读取并解析 | 可保存 |
| `Missing` | 文件不存在，使用默认值 | 可保存新文件 |
| `Recovered(String)` | 损坏文件已安全移到 `.corrupt-<unix>.json`；尝试备份，失败则使用默认值 | 可保存；保留恢复说明 |
| `Unreadable(String)` | 文件存在但不可读，或损坏后无法安全移走 | 原文件不动；本次会话禁止覆盖该路径 |

保存先写同目录临时文件并刷新，再保留 `<stem>.bak.json`，最后替换正式文件。邮件索引使用自己的临时文件保存流程，不能套用这里的完整备份协议。

`Settings.extra` 和 `Store.extra` 通过 `serde(flatten)` 保留各自**顶层**未知字段。它们不保证所有嵌套对象的未知字段都保留。历史字段保留不代表旧功能重新启用，也不代表另一平台的设置已生效。

## 3. 设置 `Settings`

定义、默认值、迁移和归一化均在 [settings.rs](../crates/poni-core/src/settings.rs)。当前 `SETTINGS_VERSION = 7`，它是设置结构版本，独立于 Cargo 中的应用版本。

### 外观、导航和系统行为

| 字段 | 类型 | 含义、默认值及规则 |
| --- | --- | --- |
| `settings_version` | `u32` | 缺失视为 0；迁移后至少为当前结构版本，不主动降低新版本号 |
| `theme` | `Theme` | JSON 为 `classic/oyen/glass/pill/diorama`；默认 `classic` |
| `idle_style` | `IdleStyle` | JSON 为 `clock/minimal/eyes/hidden/status`；默认 `clock` |
| `use_24h` | `bool` | 时钟是否使用 24 小时制；默认 `true` |
| `hover_delay_ms` | `u32` | 悬停展开延迟；默认 220，最大 2000 毫秒 |
| `collapse_delay_ms` | `u32` | 鼠标离开后的收起延迟；默认 0，最大 5000 毫秒 |
| `expanded_width` | `f32` | 展开宽度；默认 620，范围 480–900 DIP |
| `hide_in_fullscreen` | `bool` | 全屏时隐藏；默认 `true`，启用 `game_stats` 会强制关闭隐藏 |
| `step_aside` | `bool` | 历史兼容字段；归一化后固定为 `false` |
| `monitor` | `MonitorChoice` | JSON 为 `primary/cursor`；`cursor` 指启动时鼠标所在屏幕 |
| `island_x` | `Option<f32>` | 岛中心在屏幕宽度中的比例，范围 `0..=1`；`None` 居中，实际位置仍受屏幕边界约束 |
| `start_page` | `String` | 默认 `media`；启动页 key，具体可用页由导航归一化 |
| `remember_page` | `bool` | 默认 `true`；开启且 `Store.last_page` 非空时优先使用上次页面 |
| `hidden_tabs` | `Vec<String>` | 隐藏页的 key；未知 key 保留，避免旧版本保存时丢失新版本选择 |
| `hud_volume/hud_power/hud_bluetooth` | `bool` | 音量、电源、蓝牙岛上提示的独立开关 |
| `system_history` | `bool` | 默认 `true`；只有同时启用 `addons["system"]` 才维持系统历史采样 |
| `addons` | `BTreeMap<String, bool>` | 当前 `system` key 控制系统模块；保留的其他历史 key 不再启用旧扩展 |
| `game_stats/game_peek` | `bool` | Windows 游戏读数及短暂显示选择；不代表 Mac 具备同等系统能力 |
| `notifications` | `NotificationSettings` | 系统通知来源及隐私设置，结构见 [notifications.rs](../crates/poni-core/src/notifications.rs) |
| `oyenza_show_amounts` | `bool` | 历史 Oyenza 提醒的显示选择；保留数据不意味着恢复 Mac 账户登录入口 |
| `wechat_reminders/wechat_sound` | `bool`，仅 Windows | 微信基础提醒与声音；默认 `true`，不读取聊天内容 |
| `ui_font` | `String`，仅 Windows | 空值沿用主题字体；去除控制字符，最多 31 个 UTF-16 单元；失效字体回退 |
| `extra` | `BTreeMap<String, Value>` | 未识别的顶层设置；不应批量删除 |

导航消费链路见 [state.rs](../crates/poni/src/state.rs)、[pages/mod.rs](../crates/poni/src/pages/mod.rs)；高级设置键和界面见 [settings_model.rs](../crates/poni/src/settings_model.rs)、[settings_window.rs](../crates/poni/src/settings_window.rs)。

### 天气与音乐

| 字段 | 类型 | 含义、单位及规则 |
| --- | --- | --- |
| `weather_city` | `String` | 保存手选或定位所得城市；空值表示未选择，去首尾空白后最多 80 字符 |
| `weather_city_coordinates` | `Option<(f64, f64)>` | 顺序是**纬度、经度**，单位为度；有限值，纬度 `[-90,90]`、经度 `[-180,180]`，且城市非空才保留 |
| `show_media_activity` | `bool` | 媒体活动显示开关；默认 `true` |
| `lyrics` | `bool` | 歌词显示开关；默认 `true` |
| `lyrics_offset_ms` | `i32` | `[-5000,5000]` 毫秒；**正值使歌词提前显示**，不是延后 |
| `lyrics_color` | `Option<u32>` | 当前句 RGB 色，最大 `0xFFFFFF`；`None` 使用主题默认色 |
| `codex_enabled` | `bool` | 默认 `false`；开启后才监听本机 Codex 消息 |
| `codex_preview` | `bool` | 默认 `false`；控制收起状态的正文预览，不改变消息存储边界 |

### 提示音：共用目录，保留两端格式

声音开关与音效选择是两件事：`focus_sound`、`shelf_sound`、`codex_sound` 默认 `true`；邮件另由 `mail.sound` 和账户 `sound` 共同控制，Windows 微信另有开关。仅设置音效 ID 不会启用已关闭的声音。

音效 ID 来源是 [assets/sounds/catalog.json](../assets/sounds/catalog.json)，显示名和文件名不能作为稳定 ID。Mac 模型见 [alert_sounds.rs](../crates/poni-core/src/alert_sounds.rs)，Windows 生成模型见 [sound_catalog.rs](../crates/poni-core/src/sound_catalog.rs)。

| 模块 | Mac `alert_sounds` 内字段 | Windows 顶层字段 | 默认 ID |
| --- | --- | --- | --- |
| 专注 | `focus` | `focus_sound_id` | `modern-Pulse` |
| 文件暂存区 | `shelf` | `shelf_sound_id` | `classic-Telegraph` |
| Codex | `codex` | `codex_sound_id` | `recent-Cheers` |
| 邮件 | `mail` | `mail_sound_id` | `modern-Bamboo` |
| 微信 | 无 | `wechat_sound_id` | `classic-Tri-Tone` |

各字段类型都是 `String`。未知 ID 在归一化时恢复该模块默认值。Mac 使用包内 CAF/M4R，Windows 使用生成的 WAV，设置 ID 不含扩展名。合并保留两种 JSON 结构，不自动跨平台转换；另一端未知顶层字段可随 `extra` 保存，但不应用为本端音效。

已有迁移还包括：v7 之前的旧全局 `sounds=false` 迁移为关闭各模块及邮件声音；Windows 旧 `focus_sounds/shelf_sounds` 转为当前单数开关并消费旧键。新增音效或改默认值时同时检查目录、生成器、模型归一化、设置选择和实际播放消费者。

## 4. 业务文件 `Store`

根模型见 [store.rs](../crates/poni-core/src/store.rs)，当前 `STORE_VERSION = 1`。缺字段使用默认值；该版本不是应用发布版本。

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `version` | `u32` | 业务数据结构版本 |
| `tasks` | `TaskList` | 待办集合及下一个本地 ID |
| `shelf` | `Shelf` | 文件或目录引用集合 |
| `focus_sessions` | `BTreeMap<String, u32>` | 本地 `YYYY-MM-DD` → 自然完成的专注时段次数 |
| `last_page` | `String` | 最近展开页面的 key；与 `remember_page` 一起使用 |
| `water` | `Value` | 旧饮水历史，原样保留；`null` 时不输出 |
| `oyenza_noticed` | `Vec<String>` | 旧账单提醒去重键，兼容保留 |
| `extra` | `BTreeMap<String, Value>` | 未识别的顶层业务数据，随保存保留 |

### 待办 `TaskList` / `Task`

模型及日期可见性规则位于 [todo.rs](../crates/poni-core/src/todo.rs)，界面位于 [pages/tasks.rs](../crates/poni/src/pages/tasks.rs)。

| 字段 | 类型 | 含义、来源及规则 |
| --- | --- | --- |
| `TaskList.tasks` | `Vec<Task>` | 最多 200 条；完成项也计入容量 |
| `TaskList.next_id` | `u64` | 本机递增分配，添加时至少从 1 开始；不是跨设备全局 ID |
| `Task.id` | `u64` | 本地任务身份 |
| `Task.text` | `String` | 添加时合并空白，最多 200 字符；空白文本不添加 |
| `Task.done` | `bool` | 是否完成 |
| `Task.created_unix` | `i64` | 创建时 Unix 秒 |
| `Task.done_unix` | `Option<i64>` | 完成时 Unix 秒；取消完成后清空 |
| `Task.scheduled_day` | `Option<i64>` | 计划本地公历日，单位为 epoch 起的日期天数 |
| `Task.completed_day` | `Option<i64>` | 实际完成的本地公历日；取消完成后清空 |

旧任务缺少日期时，`AppState::new` 按设备本地时区从原创建/完成时间补齐日期，不修改原 Unix 时间戳。

日期规则由 `Task::visible_on` 统一决定：查看今天时，未完成项展示计划日不晚于今天的任务，完成项展示今天完成的任务；查看其他日期时按计划日展示。未完成项按创建时间及 ID 升序，完成项按完成时间降序。日历日期标记、数量和列表使用相同可见性规则，`DayTasks` 为派生内存结果，不另存日期标记。

### 文件暂存区 `Shelf`

模型及去重见 [shelf.rs](../crates/poni-core/src/shelf.rs)。

| 字段 | 类型 | 含义、单位及规则 |
| --- | --- | --- |
| `Shelf.items` | `Vec<ShelfItem>` | 最多 24 条，新加入的引用在前 |
| `ShelfItem.path` | `String` | 原文件或目录路径引用；**不复制文件内容**，跨系统复制 JSON 不会转换路径 |
| `ShelfItem.added_unix` | `i64` | 加入或再次置顶时的 Unix 秒 |
| `PreparedPaths.paths` | `Vec<String>`，仅内存 | 后台检查后交给添加操作的路径 |
| `PreparedPaths.duplicates` | `usize`，仅内存 | 本次被内容或实际路径去重的数量 |
| `PreparedPaths.unreadable` | `usize`，仅内存 | 本次内容检查读失败的数量，不代表所有路径都被拒绝 |

相同路径忽略大小写去重并置顶；实际路径相同也去重。同大小普通文件在后台流式计算 **SHA-256** 比较内容，不是 MD5；目录只比较路径。摘要不写入 Store，防止文件修改后复用旧摘要；读取失败不能据此认定文件内容相同。

## 5. 专注配置、运行状态与统计

状态机见 [pomodoro.rs](../crates/poni-core/src/pomodoro.rs)，完成记账见 [focus.rs](../crates/poni/src/focus.rs)。

`Settings.pomodoro` 持久化配置：

| 字段 | 类型 | 默认值、单位及归一化范围 |
| --- | --- | --- |
| `focus_min` | `u32` | 25 分钟；1–180 |
| `short_break_min` | `u32` | 5 分钟；1–60 |
| `long_break_min` | `u32` | 15 分钟；1–120 |
| `long_every` | `u32` | 4 次；0–12，0 禁用长休息 |
| `auto_start_breaks` | `bool` | `true`，自动开始休息 |
| `auto_start_focus` | `bool` | `false`，自动开始下一段专注 |

`Pomodoro` 运行状态只在内存中：

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `phase` | `Phase` | `Focus/ShortBreak/LongBreak`，当前阶段 |
| `run` | `RunState` | `Idle`、`Running { ends_at }` 或 `Paused { remaining }` |
| `ends_at` | `f64` | 当前进程单调时钟的结束秒数 |
| `remaining` | `f64` | 暂停时剩余秒数 |
| `cycle_done` | `u32` | 当前长休息周期内自然完成的专注次数 |
| `session_duration` | `f64`，内部字段 | 开始当前时段时固定的总秒数；运行或暂停时改配置仅影响后续时段 |

应用重启重新构造计时器，不从 `data.json` 恢复运行中倒计时。只有专注阶段**自然结束**才增加 `focus_sessions`；主动跳过及休息结束不增加。记账使用结束处理时的本地日期，保留近一年的统计范围。统计单位是完成次数，不能据此声称记录了实际专注分钟数。

## 6. 媒体、音量与歌词

以下主要状态均只在内存中，定义见 [state.rs](../crates/poni/src/state.rs)、[timeline.rs](../crates/poni-core/src/timeline.rs)。系统媒体适配器负责生产，媒体页和收起岛负责消费。

### `MediaSession` 与 `Timeline`

| 字段 | 类型 | 含义、来源及单位 |
| --- | --- | --- |
| `app_id/app_name` | `String` | 系统报告的播放器身份及显示名 |
| `content_id` | `String` | 播放器提供的原始歌曲标识，未提供时为空 |
| `title/artist/album` | `String` | 播放器元数据，用于显示和歌词匹配 |
| `track_key` | `String` | 当前歌曲的内部身份，由元数据及可用原始 ID 构造；不能当作歌词服务的歌曲 ID |
| `art/art_color` | `Option<Image>/Option<Color>` | 封面及派生配色，不写 Store |
| `can_prev/can_next/can_play_pause/can_seek` | `bool` | 系统报告的控制能力，界面不能假设全部可用 |
| `timeline.position_s` | `f64` | `sampled_at` 时歌曲内播放位置，秒 |
| `timeline.position_known` | `bool` | 播放器是否提供可信位置；正在播放不等于从 0 秒开始 |
| `timeline.duration_s` | `f64` | 歌曲总秒数，0 表示未知 |
| `timeline.sampled_at` | `f64` | 位置快照对应的单调时钟秒数 |
| `timeline.playing` | `bool` | 当前播放状态 |
| `timeline.rate` | `f64` | 播放倍速，1.0 为正常 |

可信位置在播放时按单调时间外推，并限制在歌曲范围内。位置未知时，界面不能把返回的 0 当作可信的当前句位置。`MediaSession::duration_secs()` 将有效时长四舍五入为整数秒用于匹配，未知返回 0；这与时间轴保留的浮点秒不是同一精度。

### `LyricsState` 与歌词状态

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `track_key` | `String` | 本次歌词对应的内部歌曲身份 |
| `request_generation` | `u64` | 请求代次；异步结果须匹配当前代次，防止旧歌覆盖新歌 |
| `request_duration_s` | `u32` | 当前请求用于消歧的整数时长，单位秒 |
| `status` | `LyricsStatus` | 本次获取结果或正在进行的状态 |
| `lyrics` | `Option<Lyrics>` | 已解析歌词，独立于查找状态 |
| `LyricLine.time_s` | `f64` | LRC 当前行在歌曲内的秒数，不是 Unix 时间 |
| `Lyrics.synced` | `bool` | 是否有真实同步时间戳；纯文本不能伪装为已同步 |

`LyricsStatus` 语义：`Idle` 未开始、`Loading` 查找中、`Found` 已找到、`Instrumental` 纯音乐、`NotFound` 未匹配、`Retrying(String)` 已安排退避重试并携带原因、`Error(String)` 本次失败。`Retrying` 不能改成永久加载提示，也不能在暂停恢复时丢弃已有退避任务。

QQ 音乐、网易云音乐、Apple Music 的匹配按对应服务处理；请求身份、录音时长、结果回收和缓存策略见 [services/lyrics.rs](../crates/poni/src/services/lyrics.rs)，失败排查见 [lyrics-diagnostics.md](lyrics-diagnostics.md)。歌词缓存独立于 Store，可重新生成；不要把缓存命中当成正在播放的证据。

`VolumeState` 的 `level: f32` 为系统默认输出设备 `0..=1` 音量，`muted` 为静音，`known/adjustable/can_mute` 表示读取及控制能力。它不是播放器自己的应用内音量，也不是提示音独立增益。

## 7. 邮件

配置见 [poni-core/mail.rs](../crates/poni-core/src/mail.rs)，界面状态见 [poni/mail.rs](../crates/poni/src/mail.rs)，连接和索引见 [mac/services/mail.rs](../crates/poni/src/mac/services/mail.rs)。后一个文件虽位于 `mac/`，当前是两端共用的邮件业务服务，系统凭据实现按平台选择。

### 设置和账户

| 字段 | 类型 | 含义、默认值及规则 |
| --- | --- | --- |
| `MailSettings.enabled` | `bool` | 邮件总开关，默认 `true`；关闭后停止收信，保留配置 |
| `MailSettings.sound` | `bool` | 邮件声音总开关，默认 `true` |
| `MailSettings.accounts` | `Vec<MailAccount>` | 非敏感账户配置；密码和令牌不在这里 |
| `quiet_during_focus` | `bool` | 默认 `false`；专注期间只累计新邮件，不自动弹出卡片 |
| `MailAccount.id` | `String` | 稳定账户身份；1–100 字节，仅 ASCII 字母、数字和 `-` |
| `name` | `String` | 账户显示名；非空，最多 40 字符 |
| `address` | `String` | 邮箱地址；最多 254 字节，无空白和控制字符，须有有效的单个 `@` 分隔 |
| `provider` | `MailProvider` | JSON：`qq/gmail/icloud/aruba/custom` |
| `host` | `String` | IMAP 主机；QQ `imap.qq.com`、iCloud `imap.mail.me.com`、Aruba `imaps.aruba.it`；Gmail 走 API |
| `port` | `u16` | 默认 993；非 Gmail 须非 0，连接使用 SSL/TLS |
| `enabled/notify/sound` | `bool` | 分别为账户启用、岛上提醒、账户声音；默认 `true` |
| `privacy` | `MailPrivacy` | JSON：`count_only/sender_and_subject/preview`，默认 `sender_and_subject` |
| `revision` | `u64` | 默认 1；账户编辑或开关变化时递增，拒绝旧连接和旧授权结果；不是邮件历史游标 |

账户保存边界由 `MailAccount::validate` 校验。非 Gmail 主机只接受 ASCII 字母、数字、`.`、`-`，最多 253 字节；Gmail 不使用 IMAP 主机和端口校验。

### 邮件索引 `MailboxCache`

持久化到 `mail/<account.id>.json`，只允许同一邮箱身份复用。首次连接建立基线，不把全部历史邮件作为新邮件提醒。

| 字段 | 类型 | 含义及口径 |
| --- | --- | --- |
| `identity` | `String` | 提供方、转小写地址、转小写主机、端口和 `INBOX` 组合；避免改邮箱后沿用另一邮箱游标 |
| `validity` | `String` | IMAP `UIDVALIDITY`，不是日期 |
| `max_uid` | `u32` | IMAP 已同步 UID 基线 |
| `history_id` | `String` | Gmail 历史游标，不是时间戳 |
| `initialized` | `bool` | 是否完成首次同步基线 |
| `messages` | `Vec<MailMessage>` | 本地最近邮件摘要索引 |

`save_cache` 使用临时文件、刷新和替换；Mac 文件权限为仅当前用户访问。缓存失败向调用方报告，不把失败当成功。

### `MailMessage` 与内存预览

| 字段 | 类型 | 持久化及含义 |
| --- | --- | --- |
| `MailMessage.id` | `String` | 索引保存；提供方邮件身份，须与账户一起定位 |
| `sender/subject/snippet` | `String` | 索引保存的发件人、主题和摘要；因此不能宣称本地完全不保存邮件内容 |
| `received_unix` | `i64` | 索引保存的 Unix 秒；Gmail 来源为 `internalDate` 毫秒换算，IMAP 可能使用邮件 Date 头，0 表示未知；并非统一精度的严格服务器收件时间 |
| `unread` | `bool` | 索引保存的邮箱未读标记快照 |
| `attachments` | `bool` | 索引保存的附件存在标记，不是附件文件 |
| `MailPreview.to/body` | `String` | 仅当前预览内存中的收件人和正文 |
| `MailPreview.attachments` | `Vec<MailAttachment>` | 仅内存的附件名称及可用大小，未下载附件实体 |
| `MailPreview.truncated` | `bool` | 正文被限制或截断，不能当完整邮件导出 |
| `MailAttachment.name` | `String` | 附件显示名 |
| `MailAttachment.size` | `Option<u64>` | 可用时为字节数，`None` 不是 0 字节 |

预览读取限制为 `MAX_BODY = 1 MiB`、`MAX_TEXT = 100000` 字符；不是无上限下载。Mac 系统翻译的 `MailTranslation` 及翻译开关只保存在内存，不改写原始邮件或索引。

### 未读、提醒和错误状态

`MailState.messages/statuses` 按账户 ID 保存当前内存快照。`unread_counts` 是上次成功同步的**服务器收件箱未读总数**；`unseen` 是**岛内尚未展示的提醒数**，查看岛上提醒不等于把服务器邮件标为已读。

`UnreadSummary` 只汇总启用账户：全部未知为 `Unknown`（显示 `—`），全部已知为 `Exact(n)`，部分未知为 `AtLeast(n)`（显示 `≥n`）。不能用已缓存摘要数量代替服务器未读总数。

`MailConnection`：`Off` 未启用/未连接，`Connecting` 建连中，`Syncing` 已连接并同步中，`Ready { unread }` 最近同步成功，`Paused` 暂停，`Authorizing` 正在保存或授权，`Error(String)` 失败；Mac 另有 `AuthorizationRequired(String)`，需要在高级设置手动授权。`Authorizing` 不一定打开浏览器，例如密码账户保存也使用该状态。

### 邮箱凭据边界

- Mac：系统登录钥匙串，service 为 `com.oyenza.poni.mail`，account 为账户 ID；后台读取保持静默，需要交互时由高级设置里的授权/保存操作发起。安装后不主动弹授权框。
- Windows：当前用户的 Credential Manager Generic 条目，目标为 `com.oyenza.poni.mail/<id>`；使用 `CRED_PERSIST_LOCAL_MACHINE` 在该用户、该设备上持久化，超过系统凭据大小上限报错，不回落明文文件。
- 邮箱密码、OAuth access/refresh token 和过期时间不写入 `settings.json`、`data.json`、邮件索引或安装包。此约定仅指邮箱凭据，不能推断所有播放器令牌都放在钥匙串中。
- 复制数据目录不会自动转移系统凭据；跨设备、跨系统应重新授权，不自动复制或迁移凭据。

实现入口：[邮件服务凭据读写](../crates/poni/src/mac/services/mail.rs)、[Windows mail_credentials.rs](../crates/poni/src/services/mail_credentials.rs)。

## 8. Codex 消息与会话

模型见 [codex.rs](../crates/poni/src/codex.rs)，历史与附件见 [codex_client.rs](../crates/poni/src/codex_client.rs)。以下为本机消息协议和当前应用内存状态，不能因为类型支持 serde 就视为已写入 `data.json`。

| 字段 | 类型 | 含义、来源及口径 |
| --- | --- | --- |
| `Message.id` | `String` | 单条本机提醒身份 |
| `thread_id/turn_id` | `String` | Codex 会话及轮次身份；空值不代表可发起会话回复 |
| `cwd` | `String` | 消息来源工作目录，不是会话标题 |
| `title/text` | `String` | 上游提供的提醒标题和正文 |
| `kind` | `Kind` | JSON：`completed/failed/approval/question` |
| `Entry.preview` | `String` | 收到时提取一次的正文摘要，最多 49 字符，含省略号；由 [markdown::summary](../crates/poni/src/codex_client/markdown.rs) 规则压缩，不是再次调用模型总结 |
| `Entry.received_unix` | `i64` | Island 本机收到提醒时的 Unix 秒，不是上游原消息创建时间 |
| `Entry.unread` | `bool` | 岛内提醒未读状态，与邮件未读无关 |
| `Entry.draft/status/confirmed` | `String/ReplyState/bool` | 单条待答草稿、回复状态及确认状态，只在内存 |
| `Conversation.name/name_loaded` | `String/bool` | 从 Codex 读取的会话名及是否完成读取；不拿 `cwd` 目录名替代真实会话名 |
| `Conversation.items/draft` | `Vec<ChatItem>/String` | 已加载对话及当前草稿，只在内存 |
| `loading/sending` | `Option<u64>` | 当前历史/发送请求 ID；用于拒绝过期结果，不是会话 ID |
| `next_cursor` | `Option<String>` | 上游历史分页游标，不是时间或消息序号 |
| `error/send_status/send_uncertain` | `String/String/bool` | 获取失败、发送说明及结果不确定状态；不确定不能当成功或自动重发 |
| `ChatItem.role/text/attachments` | `Role/String/Vec<ChatAttachment>` | 用户/助手角色、文本及附件；原始聊天历史仍由 Codex 保存 |
| `ChatAttachment.name/path/image` | `String/Option<String>/bool` | 名称、可用本地绝对路径、图片标记；无路径或文件不存在时不能预览 |

`ReplyState` 为 `ReadOnly/Waiting/Sending/Replied/Expired/Cancelled`。普通消息只有在 `ReadOnly`、非 `Approval` 且会话 ID 有效时可进入会话发送；确认按钮不代替 Codex 权限审批。

消息列表按非空 `thread_id` 合并展示，但同一会话的等待请求仍各自绑定原请求，不能被新提醒覆盖。请求 ID 是进程内递增值，不保证跨应用重启全局唯一。正文和完整对话不写业务 Store。

附件解析只接受符合规则的本地绝对路径，不执行附件。预览由系统适配器处理：Mac 使用原生预览，Windows 使用对应文件操作入口；不能把两端界面能力视为完全相同。

## 9. 修改字段时同步检查

修改持久化字段先确认归属：设置、Store、邮件索引或系统凭据；同时检查默认值、反序列化、归一化、迁移、保存失败处理和旧数据保留。修改时间或数量字段时，保持本文单位及统计口径一致。

共享业务需沿生产方到消费者检查：系统事件 → `AppState` / 业务模型 → 页面与收起岛 → 命令 → 系统服务。Mac 和 Windows 的入口分别在 [mac/app.rs](../crates/poni/src/mac/app.rs)、[app.rs](../crates/poni/src/app.rs)，不能只改共享页面而漏掉其中一端事件处理。

本字典覆盖主要用户数据和关键业务运行态，不替代每个模块的完整类型定义。新增业务字段或改变本页规则时应与源码一起更新；历史记录、第三方协议或生成目录的细节继续由对应模块文档维护。

## 独立更新配置

| 字段/文件 | 含义、类型与口径 |
| --- | --- |
| `release/update-source.json` / `repository` | 构建时读取的GitHub owner/repo字符串；不接受任意URL，不包含令牌 |
| `release/update-keys.txt` | 构建时嵌入的生产P-256公钥；每行key-id及65字节点的十六进制，不存私钥 |
| `latest.json` / `payload` | 精确UTF-8 JSON字符串，先验签再解析；schema=1 |
| `signature` / `key` | P-256/SHA-256签名r+s的Base64及受信key-id |
| payload `version` / `date` | 项目数字版本和生成时UTC日YYYY-MM-DD；客户端不自动降级 |
| payload `headline` / `notes` | 最多90字符摘要及该版本GitHub发行页；不来自本机消息或邮箱数据 |
| `windows` / `mac` | 平台下载对象：url固定到本仓库对应tag，size单位字节，sha256为文件小写十六进制摘要 |

两端手动检查的签名验证结果只用于本次运行；本地历史update.json或安装器缓存不是可信来源。Mac确认后打开DMG供用户替换；Windows确认后保存再安装。不启用历史下次启动自动安装字段。发布流程与实际边界见[发布说明](releases.md)。
