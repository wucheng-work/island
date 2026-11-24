# Notes for coding agents

Codex, Claude Code, Kimi or any other agent working in this repo: read this before you change the app, cut a release or change the website.

## Keep it light

灵动岛 sits on screen all day, so how little it costs is part of the product. The maintainer's rule (24 Oct 2026): keep it very light on CPU, memory and download size, and fast, however many features it gets. Every change follows these:

- **Nothing runs at rest unless the person switched it on or its page is open.** No new thread, timer, poll or network call that keeps going while the island is idle. React to Windows' own notifications where there are some. When something has to repeat, give the interval a reason in a comment and stop it as soon as nothing needs it.
- **Anything that repeats rests when nobody is there:** while the session is locked, the display is off or battery saver is on. The app already knows all three (`App::locked`, `App::display_off`, `st.power.saver`), and ambient animation already pauses for the first two.
- **Motion costs a frame every refresh.** Something that moves by itself on the closed island (the equalizer, Oyen's notes) redraws for as long as it shows. Add one only where people watch it, and let it stop.
- **Load heavy Windows parts when the feature first needs them, not at start.** Performance counters, graphics devices and WinRT classes bring their own libraries: the System page's counters alone add about 6 MB once loaded.
- **Keep big data out of poni.exe.** The add-on download framework and optional add-ons were removed in this build. `include_bytes!` is for small things the app can't start without, such as the icon and the Outfit font.
- **A new crate needs a reason and its size.** Build with `CARGO_PROFILE_RELEASE_STRIP=false cargo wbuild` before and after, and compare `x86_64-w64-mingw32-nm --size-sort -S -C` totals per crate. Code that runs for moments (a handshake, a parser, a file format) can be built for size in the workspace `Cargo.toml` (`[profile.release.package.<name>] opt-level = "s"`). Code that runs every frame stays at full speed.
- **Measure CPU in cycles, not milliseconds.** `TotalProcessorTime`, `GetProcessTimes` and Task Manager count in 15.6 ms clock ticks, so a reading that takes a fraction of a millisecond shows as zero. Use `QueryProcessCycleTime` or `QueryThreadCycleTime`.
- **Every PR says what it costs:** download size (the installer in the `windows-release-assets` artifact), memory at rest (`smoke-out/metrics.txt` in the `poni-screens` artifact), and anything new that runs at rest or on every frame. "Nothing new" is a fine answer.

## Names that stay

This checkout is the single source for Island For Mac and Island For Windows. Shared pages, state, business rules and lyrics lookup belong to one Rust workspace; use target `cfg` only for system adapters and platform capabilities. Keep Mac's `Island For Mac.app` / `com.oyenza.poni` and Windows' distributed `Island.exe` / `Island For Windows`. Cargo still builds the internal `poni` target. Do not change stable data folders, uninstall keys, Run value, `Local\Poni.Island`, window classes, credentials, crates, `PONI_*` variables or Codex channels. The application was derived from Oyenza Studio / Oyen Poni; preserve upstream copyright and third-party notices.

唯一后续开发入口是工作区根目录 `Island/`。历史源码副本已清理，来源见`UPSTREAM.md`，后续修改通过Git记录。临时隔离候选验证后合入这一入口，不能把整个平台源码复制出新的长期分支目录。平台能力差异允许保留，不能为了统一源码删除有效功能。

本机安装更新必须通过 `bash tools/sign_mac_app.sh <app>` 签名；`make_mac_app.sh` 已调用它。该入口读取本机固定证书配置（见 `docs/mac-signing.md`），不要用 `codesign --sign -` 覆盖本机签名，也不要在每次更新时重新生成证书。签名完成后核对 designated requirement 仍使用相同证书和 bundle id，避免使已有邮箱凭据在每次更新后重新授权。本机证书不是 Developer ID，分享发布仍走正式签名流程。

Never create a repository named `lucacadalora/poni`: GitHub forwards that old name to this repository, and the add-on downloads of the 0.1.x versions go through it.

## 版本控制与独立发布

当前仓库为`https://github.com/wucheng-work/island`；原网站、商店、签名身份和历史发行记录属于上游。来源与基点见`UPSTREAM.md`。默认分支为`main`，功能修改在职责明确的分支中进行；提交只纳入授权范围，保留其他修改。提交、推送、tag、发布和安装必须有用户明确授权。

根`Cargo.toml`是唯一版本源。每次新的打包任务patch+1，同任务双平台共用版本、失败重试不额外递增。合并初始版本为1.0.0，日常修改不自动加号。用`python3 tools/release.py version/check/bump-patch`读取、校验或分配版本；锁文件由Cargo生成，不能手工修改，不得改变第三方依赖锁来伪造通过。

发布需完成`docs/release-notes/v<版本>.md`，tag为`v<版本>`，程序版本、安装器资源、包名、更新清单必须一致，不可覆盖已公开版本。普通push/PR仅运行`ci.yml`；`release.yml`须手动运行并选择版本tag及启用`publish`才能发行。推送tag只登记版本。正式发布等两端包、生产签名清单和校验表齐备，草稿重新下载验证后最后公开为latest。完整规则见`docs/releases.md`。

更新仓库固定读取`release/update-source.json`，生产公钥只在`release/update-keys.txt`，私钥留在仓库外或GitHub Secrets。没有受信公钥时不联网、不宣称更新可用；不得使用上游样本公钥作为生产信任。Mac发行可明确选择未公证包，发布说明必须标清；选择公证时Developer ID和Apple公证任一步失败即中止，不降级。本机固定证书只用于本机更新。客户端由用户手动检查、确认下载和安装，不静默替换应用，不改用户数据目录或凭据标识。

## 长期维护和文档要求

- 代码优先服务于正确性、可读性和长期维护。按真实业务领域组织模块；函数职责清晰有限，不向已有大型平台入口继续堆入独立业务算法。仅在语义一致且确有复用时抽取公共逻辑，不引入没有实际价值的抽象层或框架。
- 类、函数、变量和文件名表达业务含义；关键规则、计算公式和数据口径应容易定位。复杂步骤使用明确命名和早返回，避免嵌套表达式、隐式全局状态与未说明副作用。
- 外部接口、核心业务和复杂计算注明用途、输入/输出、单位与异常；注释解释原因和容易误解的规则，不逐行翻译代码，不留过期注释或废弃代码。
- 使用明确类型与稳定返回结构。区分预期缺失、可恢复错误、认证/状态异常和必须中止的错误；不得吞异常或产出貌似成功的结果。日志包括必要业务阶段和上下文，不记录密码、令牌或敏感内容。
- 有副作用操作使用fetch/save/update/execute等清楚名称，纯计算保持输入输出明确。共享契约变化同步生产方、两端消费者、数据字典和文档。
- README必须与实际用途、环境、安装、启动和常见问题同步。主要模块职责见docs/modules.md；关键持久化和运行态字段的含义、类型、单位、来源和时间口径见docs/data-dictionary.md。文档描述已实现事实，不将规划写成实现。
- 不自动新增测试用例；复用现有检查。两端已有同一测试的条件分支合并须保留断言，不借合并削弱或虚构覆盖。
