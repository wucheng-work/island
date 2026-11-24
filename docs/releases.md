# 版本控制与发布

## 一个版本源

根 `Cargo.toml` 的 workspace version 是 Mac、Windows 和更新清单唯一版本源。统一源码初始版本为 `1.0.0`；当前值用下述命令读取。`Cargo.lock` 的三个本地 crate 跟随它，Windows EXE 资源及 manifest、Mac `Info.plist` 和安装包名称由该版本生成。

```sh
python3 tools/release.py version
python3 tools/release.py check
python3 tools/release.py check --tag v1.0.0 --require-notes
```

每次明确开始一个新打包任务时执行一次：

```sh
python3 tools/release.py bump-patch
```

这只增加 workspace patch，并调用 `cargo update --workspace --offline` 更新锁文件，不提交、不建 tag、不推送、不发布。工具确认三个 workspace crate 已同步且第三方依赖锁没有变化；Cargo 失败或发生依赖变化时恢复这次修改的两个版本文件。执行前确保没有其他任务同时写版本文件。目标版本的 Git tag 已存在时拒绝复用；工具不查询远程是否发布，仍需在打包前核对该版本记录。

同一次任务生成的两个平台包共用版本，失败重试沿用已分配版本。源码修改和合并本身不加号。更新 `docs/release-notes/v<版本>.md`，写清真实变化和验证边界，再检查两端，不以修改文件名替代升版。不覆盖已发版本，也不使用带预发行后缀的版本进入稳定更新通道。

## 仓库与更新源

自己的仓库地址保存在 `release/update-source.json`：

```json
{"repository":"wucheng-work/island"}
```

客户端只从该配置派生：

```text
https://github.com/wucheng-work/island/releases/latest/download/latest.json
```

空仓库配置或没有受信公钥时关闭在线更新；没有任何上游仓库或网站的回退。正式发布时，配置仓库必须与工作流所在 `GITHUB_REPOSITORY` 一致，防止 fork 或误配为其他仓库发布清单。更新源支持公开 GitHub Release；不在客户端嵌入 GitHub token。

生产受信公钥在 `release/update-keys.txt`，现已配置本项目专用P-256公钥；不要拿测试 fixture 或上游密钥填进去。私钥只保存在本机源码目录外，尚未配置到GitHub Secrets。每行格式为：

```text
<16位小写key-id> <以04开头的65字节P-256公钥的十六进制>
```

`key-id` 是公钥原始65字节的 SHA-256 前16个十六进制字符。公钥必须随程序构建，私钥只保存在自己的 GitHub `release` environment 的 `UPDATE_FEED_KEY` secret 中，内容为 P-256/prime256v1 PEM 私钥。私钥不得提交、打印到日志或放入安装包；发布工具不会自行生成或更换私钥。本机已准备的私钥位于`~/Library/Application Support/Island/release-signing/update-feed.pem`，权限0600；将其安全备份并通过GitHub Secrets界面配置`UPDATE_FEED_KEY`，不要粘贴到聊天、issue或普通文件。首个交付程序必须包含当前公钥。密钥轮换时先让现有版本信任备用公钥，再切换签名，避免旧客户端失去更新通道。

## 更新清单

`tools/make_update_feed.py` 仅处理当前源码版本的完整本地产物：

- `Island-For-Windows-Setup-<版本>.exe`：NSIS 安装器，供 Windows 更新。
- `Island-For-Windows-<版本>-x64.zip`：便携发行包，也必须齐备。
- `Island-For-Mac-<版本>.dmg`：同时包含 arm64 与 x86_64 的 Mac 镜像；是否公证由本次明确的发行策略决定。

```sh
# 检查本地产物、版本和说明，输出尚未签名的payload，不写文件。
python3 tools/make_update_feed.py --assets dist --check

# UPDATE_FEED_KEY已由安全环境注入时，生成正式清单。
python3 tools/make_update_feed.py dist/latest.json --assets dist
```

脚本不调用上游 API，不从 GitHub临时补齐缺失包，也不接受其他名称的旧产物。它读取全部文件内容计算大小及 SHA-256，清单中两端链接都固定到配置仓库的 `v<版本>`；缺任何包就失败。`date` 默认使用生成时的 UTC 日期，也可用 `--date YYYY-MM-DD` 指定实际发布日期。

外层保留 schema 1 的签名契约：`payload` 为精确的 UTF-8 JSON 字符串，`signature` 是 ECDSA P-256/SHA-256 的64字节 `r || s` 的 Base64，`key` 指定受信公钥。签名前核对私钥确属 P-256且公钥已列入生产文件，签后用公钥验签。客户端还会校验清单、版本、来源及下载大小和哈希；清单签名不替代 macOS 软件签名与 Apple 公证。

## 检查与试构建

`ci.yml` 只执行源码检查、现有测试和示例页面预览，不写发布、网页、商店或统计分支。原上游 `mac.yml`、`pages.yml`、`store.yml`、`metrics.yml` 已改为仅手动提示，不会因 push 或定时任务访问上游发布流程。

在自己的 Actions 中手动运行 `Island release`，保持 `publish=false`，可试构建当前分支的两端产物。仍要求当前版本、锁文件、仓库配置和发行说明一致。默认 `notarize_mac=false`，Mac 试构建产物明确带 `-unsigned.dmg` 名称且未经公证；选择公证则须完成下述全部凭据与公证步骤。试构建只上传 Actions 产物。需要的 Pillow、dmgbuild 和 NSIS 沿用现有构建流程，没有新增客户端依赖。

## 正式发布

经明确授权后创建并推送 `v<版本>` tag。推送分支和标签都不会启动发布工作流。需要由 Actions 正式发布时，手动运行 `Island release`，选择该 tag 并启用 `publish`。tag 必须与项目版本一致，同时提供该版本的发行说明。若该 tag 已有公开 Release，准备阶段会跳过构建和部署。

使用本机生产密钥发布时，先对同一提交运行试构建，下载并核对两端产物和 Mac 签名策略，再在本机生成签名清单及校验表。将三份安装包、清单和校验表上传到该 tag 的 Release 草稿，重新下载核验后设为 latest。每个版本只选择一次正式发布入口。

发布更新清单必须在 `release` environment 配置 `UPDATE_FEED_KEY`。Mac 默认使用未公证发行策略，不需要购买 Apple Developer ID；如果选择公证，还须配置表中其余 Apple 凭据：

| Secret | 用途 |
| --- | --- |
| `UPDATE_FEED_KEY` | 与 `release/update-keys.txt` 匹配的更新清单 P-256 私钥 |
| `MACOS_CERTIFICATE_P12_BASE64` | 正式 Developer ID Application 证书及私钥的 P12 |
| `MACOS_CERTIFICATE_PASSWORD` | P12 密码 |
| `APP_STORE_CONNECT_API_KEY_P8` | Apple 公证 API 私钥 |
| `APP_STORE_CONNECT_KEY_ID` | 公证 API key ID |
| `APP_STORE_CONNECT_ISSUER_ID` | 公证 issuer ID |

限制该 environment 仅允许维护者批准的分支和 tag 访问，可配置人工审核。Mac 构建依然可访问 `release` 环境的签名配置，环境的部署记录只由最终发布任务创建。Mac 构建使用 `deployment: false`，此选项兼容必需审核者和等待计时器，环境附加了基于 GitHub App 的自定义部署保护规则时无法使用。没有受信生产公钥或对应 `UPDATE_FEED_KEY` 时，Actions 的公开发行会失败。

Mac 签名策略在准备阶段只计算一次，并同时用于构建与发布说明：

- 默认 `notarize_mac=false`：使用构建机测试签名的完整 DMG，以标准名称 `Island-For-Mac-<版本>.dmg` 公开发行。无需 Apple Developer ID，仍必须通过版本、完整资源、代码完整性、双平台产物和更新清单签名检查。Release 正文和工作流摘要明确说明“未经过 Apple 公证”，首次打开可能需在“系统设置 → 隐私与安全性”中允许；这是预先选择的发行策略，并非签名失败后的降级。
- 手动运行时选中 `notarize_mac=true`，或仓库变量 `ISLAND_MAC_NOTARIZE` 为字符串 `true` 时，必须使用 Developer ID 重签、完成 Apple 公证并装订票据。任一步失败就终止，不回退未公证包。两项按 OR 计算，仓库变量未设置时按 false 处理。

从同一个提交构建两端并运行现有检查，Windows 生成 NSIS 与便携包，Mac 生成 universal 应用并携带 MediaRemote 框架及全部提示音。根据上述策略选定包后，两端任务全通过才在发布任务中签 `latest.json` 并计算 `SHA256SUMS.txt`。Mac 在线更新会将用户引导到该 DMG，由用户确认后手动安装，不进行未确认替换。

发布任务以串行方式工作，拒绝不高于当前 latest 的版本，拒绝覆盖已有 Release 或先前失败留下的草稿。先创建草稿，上传全部三个包、签名清单及校验表；重新下载草稿内容逐文件验哈希后，最后一次操作才将草稿公开并设为 latest。此前任一步失败，现有 latest 保持，失败草稿需要维护者核对后手动处理，工作流不会自动删除或覆盖它。

当前 Windows 流程未配置 Authenticode，不宣称 EXE 已签名，接收方可能看到 Windows 来源提示。Mac 代码完整性签名、更新清单来源签名和 Apple 公证是不同的检查：默认免费发行路径不宣称通过 Apple 公证；选择公证时本机固定证书不能代替 Developer ID。GitHub 环境、所选公证流程及真正的在线下载/安装只有实际配置并发布后才能验收，源码静态检查不证明这些外部环节已经通过。

## 撤回与重试

不要修改同一 tag 的包和签名清单。发现问题时停止推广该版本，必要时由维护者将上一正常 Release 重新指定为 latest；已经安装较新版本的用户不会被自动降级。修复后使用更高的新版本发布。首发失败时若从未公开，先核对草稿内容及失败原因，再决定保留证据或手动删除草稿后重试同一版本。
