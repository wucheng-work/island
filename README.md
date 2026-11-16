# Island

一套 Rust 源码，同时维护 **Island For Mac** 和 **Island For Windows**。提供对应播放器的同步歌词、日历与农历、待办、天气、专注计时、文件暂存、蓝牙设备电量、系统监控、多邮箱预览和 Codex 提醒与对话。系统权限、通知、输入法和原生窗口由对应平台适配。

本项目基于 [Oyenza Studio](https://github.com/lucacadalora/oyenza-studio) 定制。上游网站、商店身份及历史发行记录不代表本地定制版本。

项目仓库：[wucheng-work/island](https://github.com/wucheng-work/island)。当前源码版本以`Cargo.toml`为准；版本控制与独立发布见[发布说明](docs/releases.md)。

## 开发入口

只维护一份 `Island/` 源码。原 IFM、IFW 的历史副本已清理，来源见[来源说明](UPSTREAM.md)，后续历史通过 Git 维护。两端共用 `crates/poni-core`、页面、状态及业务实现，使用 Rust 的目标平台条件选择系统适配；不复制出第二套应用源码。

开发目录、平台边界、现有构建检查命令与打包规则见 [开发说明](docs/development.md)。版本由根 `Cargo.toml` 定义；本次合并按用户要求统一为 `1.0.0`。每次新打包任务递增一次 patch 版本，同次任务的两个平台包共用该版本，同次失败重试不重复递增。

## 使用与升级

| 平台 | 安装包与程序 | 系统要求 |
| --- | --- | --- |
| macOS | `Island-For-Mac-<版本>-*.dmg`，内含 `Island For Mac.app` | macOS 14+；架构以安装包标注为准 |
| Windows | `Island-For-Windows-<版本>-x64.zip`，内含 `Island.exe` | Windows 10 / 11，64 位 |

升级前正常退出旧小岛，再替换应用文件。Mac 将应用拖入“应用程序”；Windows 将 ZIP 完整解压到固定文件夹后运行 `Island.exe`，不要直接在压缩包内运行。两端均停用上游更新。右键小岛选择“检查更新…”使用本仓库的更新链路；须先配置生产公钥并发布正式版本，未配置时提示原因、不联网。Mac下载校验后打开DMG，由用户正常退出旧版再替换；Windows确认下载、校验后再确认安装。详见[在线更新](docs/updates.md)。

Windows 从 0.0.7 起统一使用 `Island.exe`。旧快捷方式及外部 Codex 配置若指向 `poni.exe`，需要更新为新路径；程序内部仍保留兼容标识。已有开机启动记录会更新程序路径，原先没有注册的启动项不会自动开启。

本机 Mac 更新沿用固定签名证书，签名说明见 [Mac 签名](docs/mac-signing.md)。本地签名不等于 Apple 公证；是否经过公证以具体交付说明为准。

Windows 详情见 [使用说明](docs/Windows使用说明.md) 与 [Codex 接入说明](docs/codex-integration-windows.md)。Mac 邮件授权见 [手动授权说明](docs/mac-credentials.md)，歌词排查见 [诊断说明](docs/lyrics-diagnostics.md)。

展开小岛后进入“蓝牙”，查看已连接设备及系统提供的电量；可点“刷新”或打开系统蓝牙设置。仅在页面可见时采集，锁屏、熄屏、省电或隐藏时暂停。左右耳和充电盒只在系统提供时显示，设备不提供电量时显示“电量未知”。详细来源和限制见[蓝牙电量说明](docs/bluetooth.md)。

## 数据兼容

| 内容 | macOS | Windows |
| --- | --- | --- |
| 设置、待办、暂存区引用与专注记录 | `~/Library/Application Support/Poni/` | `%APPDATA%\Poni\` |
| 缓存 | `~/Library/Caches/Poni/` | `%LOCALAPPDATA%\Poni\` |
| 应用日志 | `~/Library/Logs/Poni/poni.log` | `%LOCALAPPDATA%\Poni\poni.log` |
| 邮箱凭据 | 系统钥匙串 | 当前用户的凭据管理器 |

统一源码不改变原数据目录、邮箱凭据、单实例标识、开机启动注册名和 Codex 通道，也不迁移或合并两台设备的数据。分享时只发送程序包，不附带自己的数据、日志或凭据。详见 [隐私说明](PRIVACY.md)。

## 常见问题

- **从哪里启动？** Mac从“应用程序”打开`Island For Mac`；Windows先完整解压，再运行`Island.exe`。不要同时启动旧版和新版。
- **Mac首次打开被拦截？** 核对包来源和应用名称后，在“系统设置 → 隐私与安全性”选择“仍要打开”；本地固定签名不代表Apple公证。
- **邮件没有提示？** 在高级设置的邮件页启用并配置账户；Mac需要时点“授权”，QQ/Apple/公司邮箱使用服务商提供的IMAP授权码或应用专用密码，Gmail按Google授权流程连接。真实密码与令牌不写入普通配置文件。
- **QQ歌词服务返回500，要等多久？** 现有规则为30秒后重试、再次失败再等120秒，最多两次；接口未恢复时仍可能失败。查看[歌词排查说明](docs/lyrics-diagnostics.md)，不要记录或分享含凭据的日志。
- **合并源码会同步两台电脑的待办吗？** 不会。源码共用不等于数据同步，两端继续使用各自本机数据目录。
- **蓝牙已连接但电量未知？** 部分设备或驱动不向系统提供电量；本功能不会估算数值。页面显示的是系统读数，可能有延迟；读取失败或暂停时会注明“上次读数”。

主要业务模块和入口调用链见[模块说明](docs/modules.md)，字段含义、单位、来源与时间口径见[数据字典](docs/data-dictionary.md)。

## 验证范围

编译和现有单元测试只证明各自检查范围。Windows 播放器、输入法、多显示器、自动启动和安装卸载需在 Windows 实际验证；Mac 权限、邮箱收信、播放器歌词和 Codex 实际投递需在 Mac 验证。蓝牙设备识别、电量和部件兼容性也需对应系统与实物设备验证。离屏预览使用示例数据，不替代真实使用验收。

## 来源与许可

代码沿用 [MIT license](LICENSE)，第三方许可见 [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt)。保留原作者及第三方版权署名。旧上游文档可能描述已移除的功能，应以当前实现及本目录说明为准。
