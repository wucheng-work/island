# 单一源码开发与构建

## 唯一入口

后续只在工作区根目录 `Island/` 维护源码。历史源码副本已清理，来源见[来源说明](../UPSTREAM.md)，后续历史通过Git维护。临时隔离候选验证后按文件合入唯一入口，不能用旧目录整体覆盖新源码。

同一 Cargo workspace 包含 `poni-core`、`studio-diorama`、`poni`。内部 crate 和 `poni` 构建目标名称保留；分发时 Mac 使用 `Island For Mac.app`，Windows 将内部 `poni.exe` 复制为 `Island.exe`。

## 目录与责任

| 目录 | 维护内容 |
| --- | --- |
| `crates/poni-core/src/` | 共享模型、日期、歌词匹配及业务规则 |
| `crates/poni/src/pages/`、`pill/` | 共享页面与岛体内容，局部平台能力以 `cfg` 区分 |
| `crates/poni/src/mac/` | AppKit、CoreText、系统媒体、钥匙串、定位及原生桥 |
| `crates/poni/src/app.rs`、`services/`、`window.rs` 等 | Windows 入口和 Win32/WinRT 适配；其中歌词等共享模块由 Mac 显式引用 |
| `assets/sounds/` | Mac 原格式音效与目录 |
| `crates/poni/res/` | Windows 图标、manifest、已生成的 WAV 音效 |
| `installer/mac/`、`tools/make_mac_*.sh` | Mac 应用与镜像打包 |
| `installer/poni.nsi`、`poni.iss`、`installer/msix/` | Windows 安装格式；`tools/package.sh` 生成便携包及可用的 NSIS 包 |

共享文件改动需同时检查直接调用方和两端配置分支。系统 API、权限、编码与音频格式保留各自适配；不为合并源代码更改稳定持久化字段、凭据标识或数据路径。平台能力可以不同，不应以统一名义删除现有功能。

## 现有检查

在有对应工具链的环境运行，沿用已有依赖和锁文件，不自动新增测试：

```sh
# 在Mac检查并运行现有workspace测试。
cargo check --workspace --all-targets --offline --locked
cargo test --workspace --offline --locked

# Windows交叉检查；需要已安装Windows GNU目标和原有交叉工具链。
cargo check -p poni --all-targets --target x86_64-pc-windows-gnu --offline --locked

# 两个平台各自的release构建。
cargo build -p poni --release --target aarch64-apple-darwin --offline --locked
cargo build -p poni --release --target x86_64-pc-windows-gnu --offline --locked
```

Mac Intel 构建目标为 `x86_64-apple-darwin`，只在安装了该目标及兼容 SDK 的环境构建。Windows 本机使用对应 MSVC 或 GNU 目标。默认输出目录是 `target/`，设置 `CARGO_TARGET_DIR` 时以该值为准。

Mac 原生定位、附件预览与翻译桥由 `build.rs` 根据 Mac 目标编译并链接，Windows 不构建这些桥。Windows EXE 资源版本由 Cargo 生成。现有 CI 的 Windows 与 Mac 工作流分别检查同一份源码；本地检查不启动或修改外部工作流。

Mac 构建需要 Xcode 26 或更新版本提供的 macOS 26 SDK 与 Swift 6.2 或更新编译器，邮件翻译桥使用该 SDK 的接口；应用最低部署版本仍为 macOS 14，邮件翻译只在 macOS 26 或更新版本启用。GitHub 的 `macos-15` runner 默认 Xcode 16，因此检查和发布流程通过 `DEVELOPER_DIR` 明确选择镜像已安装的 Xcode 26.3。

## 版本与打包

每次明确请求新安装包时，将根 `Cargo.toml` 中 patch 加 1，再运行 Cargo 更新锁文件，核对只变更三个工作区包版本。同次任务的 Mac 与 Windows 包使用同一版本；失败重试沿用已经分配的版本。单纯合并或修改源码不代表打包，不自动加号。

CI 的 `PONI_VERSION` 从 Cargo.toml 读取；Windows manifest 与 EXE 资源、Mac `Info.plist` 也由当前 Cargo 版本填充。NSIS 从打包脚本接收该版本；Inno Setup 默认读取所选 EXE 的产品版本，也可明确传入。面向具体版本的使用说明、安装包名及应用内显示必须一致，不得只改变包名。不要手改生成的锁文件或 OUT_DIR 资源。

Windows 沿用 `tools/package.sh`；若安装了 NSIS，脚本同时生成安装器。单独调用 NSIS 时必须传 `-DVERSION=<Cargo版本>`。脚本支持既有 `NSIS_HOST` 和 `SELFTEST_HOST`，只有明确授权对应远程操作时才设置。Inno Setup 可传 `/DAppVersion=<项目版本>`；MSIX 使用 `tools/make_msix.ps1 -Version <项目版本>`。

Mac 使用 `tools/make_mac_app.sh <arm64二进制> <Intel二进制或-> <图标或-> <输出目录>`。完整播放器识别需要已有 MediaRemote adapter 构建目录，通过 `MEDIAREMOTE_BUILD` 指定；不要将缺少该框架的包当作完整版本交付。脚本复制内置音效并调用固定签名入口 `tools/sign_mac_app.sh`，不可用临时 ad-hoc 签名覆盖本机固定身份。完整构建和签名配置见 [Mac签名说明](mac-signing.md)。

打包仅携带程序、所需资源和许可说明，不复制任何真实用户数据、凭据或日志。交付前验证目标架构、版本、包内容、可执行文件哈希、签名及对应源码清单。源码合并不授权安装、提交、推送、打标签或外部发布。

## 保留的系统边界

- Mac 使用原生窗口、MediaRemote、钥匙串及按需定位；Windows 使用 Win32/WinRT、媒体会话和凭据管理器。
- Windows 的网易云本地播放事件补充、微信提醒、系统字体和输入法属于 Windows 能力。
- Mac 的原生 Quick Look 预览、系统邮件翻译和声音选择窗口保留 Mac 实现。
- 同源不等于两端所有系统权限及功能表现相同。共享业务保持一致，系统能力及真实验收仍按平台分别检查。

## Git与独立更新

唯一源码仓库只管理`Island/`，远程为`https://github.com/wucheng-work/island`。构建产物、历史快照、用户数据和签名私钥不入库；保留`UPSTREAM.md`与版权。用`python3 tools/release.py check`核对版本；新打包任务执行一次`python3 tools/release.py bump-patch`，由Cargo更新锁文件。普通push/PR只执行检查，正式发行按[发布说明](releases.md)执行，更新实现见[在线更新](updates.md)。本地检查不隐式触发提交、推送、tag、安装或公开发布。
