# 业务模块与调用链

本项目是Rust桌面应用，不单独运行HTTP后端。共享业务位于core、状态、服务与页面模块，AppKit/Win32等只提供系统适配。下列职责与当前代码对应，不表示已完成所有旧模块重构。

| 领域 | 主要位置 | 职责和关键规则 |
| --- | --- | --- |
| 应用入口与系统事件 | `crates/poni/src/main.rs`、`mac/app.rs`、`app.rs` | 选择系统实现，接收事件，在界面帧之后执行页面命令；平台窗口和权限生命周期各自管理 |
| 音乐播放与歌词 | `poni-core/src/nowplaying.rs`、`timeline.rs`、`lrclib.rs`、`lrc.rs`；`poni/src/services/lyrics.rs`、各端媒体服务 | 识别播放器、维护可信进度、按对应服务严格匹配录音并交付歌词；曲目/播放器/请求代次共同隔离结果 |
| 日历与待办 | `poni-core/src/todo.rs`、`datetime.rs`、`lunar.rs`；`poni/src/pages/today.rs`、`tasks.rs` | 日期选择、农历、当天任务与未完成任务展示；日期筛选和标题使用同一选中日期 |
| 专注计时 | `poni-core/src/pomodoro.rs`、`poni/src/focus.rs`、`pages/focus.rs` | 工作/休息阶段转换、完成事件和日统计；统计依据阶段结束事件，单位见数据字典 |
| 文件暂存区 | `poni-core/src/shelf.rs`、`poni/src/pages/shelf.rs`与两端拖放/文件适配 | 维护暂存项、重复判定和撤销；真实文件操作由平台执行，UI不伪造成功 |
| 蓝牙设备电量 | `poni-core/src/bluetooth.rs`；`poni/src/bluetooth.rs`、`pages/bluetooth.rs`、两端`services/bluetooth_devices.rs` | 共用设备快照、页面状态和按需查询生命周期；系统适配提供已连接设备及真实可用电量，未知与读取失败分开处理 |
| 多邮箱 | `poni-core/src/mail.rs`、`poni/src/mail.rs`、`mac/services/mail.rs`及`mail/`、`pages/mail.rs` | 账户配置、收信/预览、缓存与取消；同一收信业务供两端使用，凭据存取/系统交互按平台处理 |
| Codex提醒与对话 | `poni/src/codex.rs`、`codex_bridge.rs`、`codex_client.rs`及子模块、`pages/codex/` | 接收本机消息、按会话合并、读取对话和附件、发送用户明确点击的回复；传输协议共用，Unix socket/Windows管道和进程适配各自实现 |
| 天气 | `poni/src/state.rs`、`poni/src/mac/services/weather.rs`、各端定位服务 | 城市搜索/当前位置、天气请求和缓存；天气请求共用，定位权限和系统定位各自实现 |
| 设置与持久化 | `poni-core/src/settings.rs`、`store.rs`；`poni/src/persistence.rs`、`settings_model.rs`、`settings_window.rs` | 读取边界校验、保持未知兼容字段、原子保存及失败反馈；界面设置与业务字段直接映射 |
| 提示音 | `poni-core/src/alert_sounds.rs`、`sound_catalog.rs`；`assets/sounds/`、两端声音服务 | 共用音效目录，Mac播放原格式、Windows播放已生成PCM；沿用各平台已持久化选择，不在合并时迁移用户配置 |
| 系统监控与平台能力 | `poni-core/src/sysmon.rs`、两端监控/电源/通知服务 | 系统数据采集和能力展示；Windows微信/系统通知及Mac原生预览等保持平台边界 |

## 顺着入口读业务

### 歌词

`main → 对应平台App → 媒体事件 → LyricsService::request → Worker::process/lookup → QQ/网易云/Apple Music匹配 → AppEvent::Lyrics → AppState.lyrics → pages/media与pill/lyric`。

`generation`、`app_id`和`track_key`必须匹配当前请求才接收。`Retrying`是可恢复错误，不是成功或持续查找；同代次后续`Found`可恢复。当前网络暂时失败最多延后重试两次，间隔30秒、120秒；切歌和取消后旧结果丢弃。具体筛选规则集中在共享歌词服务与core匹配模块，平台播放器会话选择留在媒体适配。

### 邮件

`设置模型/账户编辑 → MailAction → MailService::submit/configure → 每账户工作线程 → Backend(IMAP/Gmail) → MailEvent → 平台App → MailState → 邮件页面`。

账户revision与活动gate防止过时操作覆盖新配置。凭据由加载边界读取一次再传给后端；Mac后台访问禁止弹授权框，手动授权是显式动作；Windows沿用凭据管理器与刷新恢复。网络、认证、取消和缓存写入失败保持不同结果，不将错误拼成成功预览。

### Codex

`Hook/CLI → codex_bridge → Codex事件 → Inbox → 页面/收起摘要 → 用户回复命令 → codex_client → 原会话`。

读取对话和发送回复是不同操作；读取调用不得隐式启动新对话。会话ID是关联键，显示标题不是身份凭据。附件解析和Markdown显示共用，打开/预览文件由平台适配，不能因预览失败丢失正文。

### 数据保存

`页面产生SaveSettings/SaveStore → 平台App保存编排 → persistence::save → core::store原子写入`。

`LoadOutcome::Unreadable`禁止本次会话覆盖原文件；损坏文件与备份恢复另有状态。UI显示保存错误，重试继续真实保存链路。不得为了合并代码删除旧字段或忽略失败。

### 蓝牙设备电量

`展开/切页/系统状态 → App::sync_bluetooth → BluetoothMonitor → 平台fetch_devices → AppEvent::BluetoothDevicesStarted/BluetoothDevices → BluetoothState → pages/bluetooth`；手动刷新通过`Command::RefreshBluetooth`进入相同链路。

只在蓝牙页面实际可见且未锁屏、熄屏或省电时启动后台查询；完成后等待30秒再查，手动刷新可提前唤醒。关闭、切页或暂停时取消查询并停止等待，应用退出前等待已取消的工作线程回收系统资源。每次活动会话分配`generation`，旧会话结果不能覆盖重开页面后的状态。

成功替换快照；部分来源失败保留可用设备并提示；读取失败保留上次成功读数并标为旧数据。Mac读取结构化系统蓝牙报告、IOKit HID电量与系统附件电源信息；Windows先读系统PnP电量缓存，再为缺少读数的已连接BLE设备查询标准电量服务。没有电量时保留设备，不将未知变成0%，不从左右耳推算整机电量。与既有耳机连接HUD分别维护，原HUD筛选与防抖不变。来源、兼容限制和参考项目见[蓝牙电量说明](bluetooth.md)。

## 维护边界

- 改共享模型或命令时，同查生产方、两端消费者、持久化兼容和失败恢复；平台判断放在实际系统边界。
- 只有业务语义相同的逻辑才共用。当前声音字段和系统授权行为不同，保留原结构，避免无必要的数据迁移。
- 原有大型App文件保留平台编排职责。本次没有将整个旧项目重写，后续修改涉及某业务时沿该边界整理，不继续向入口堆入独立业务算法。
- 不增加闲置线程、轮询或新依赖来掩盖系统差异。已有检查与构建入口见[开发说明](development.md)，字段口径见[数据字典](data-dictionary.md)。

## 版本与在线更新

- `tools/release.py`负责读取/校验统一版本、显式增加patch并让Cargo更新锁文件；不提交或发布。
- `tools/make_update_feed.py`从两端完整本地产物生成签名清单；`.github/workflows/release.yml`负责同提交构建、草稿上传、重新下载复核和最后公开。
- `poni_core::update`定义仓库、公钥信任、清单结构及版本规则；不联网、不读真实凭据。
- Mac入口为`mac/menubar.rs → mac/app.rs → mac/services/update.rs → mac/events.rs → Deferred原生确认`；网络在短任务中，Security验签与大小/SHA通过后打开DMG。
- Windows入口为`tray.rs → app/update.rs → services/update_worker.rs → services/update.rs → AppEvent::Update → Deferred原生确认`；`updates.rs`只将本次签名结果交给手动更新流程，安装前保存。

两端菜单手动触发，无启动或待机轮询。独立发布配置和数据口径见`docs/releases.md`、`docs/updates.md`和`docs/data-dictionary.md`。
