# 在线更新

Island使用自己的公开GitHub仓库`wucheng-work/island`。右键小岛选择“检查更新…”才联网；不在启动、待机、解锁或每小时自动查询。源码中没有上游更新源的生产回退。

## 使用流程

1. 右键选择“检查更新…”。检查正在进行时不重复启动请求；失败后可以重新点击。
2. 验证更新清单签名、版本及仓库后，显示新版本和摘要。选择“稍后”不下载；确认后下载当前平台安装包。
3. 下载大小和SHA-256与受信清单一致才允许继续。网络、签名、来源或哈希错误不会被当作“最新版本”或安装成功。
4. Mac打开已校验的DMG，由用户正常退出旧版并拖入“应用程序”替换。Windows再次确认后保存当前状态、正常退出并启动NSIS安装器；安装完成重新打开。

不静默覆盖应用，不改变数据、邮箱凭据或Codex通道。当前不提供“下次启动自动安装”；旧版本缓存不能作为新会话的安装授权。两端各自保留系统权限及平台安装方式。

## 信任与数据流

`release/update-source.json`只接受GitHub owner/repo，派生地址：

```text
https://github.com/wucheng-work/island/releases/latest/download/latest.json
```

生产公钥来自`release/update-keys.txt`，在构建时嵌入程序；本项目已准备专用P-256公钥。私钥保存在源码目录外，不能上传源码或随包分发。只有可信公钥的签名通过后才解析payload；清单内的发行说明和下载必须属于该仓库同一个`v<版本>`，不能指向上游或任意外部包。客户端不嵌入GitHub访问令牌。

清单外层有`payload`、`signature`、`key`。签名为精确UTF-8 payload上的ECDSA P-256/SHA-256，64字节r+s的Base64。payload schema为1，包含version、UTC日期date、headline、notes及windows/mac下载对象；每个下载对象有url、字节数size、小写sha256。Windows使用系统CNG验证，Mac使用系统Security框架；下载后再次核对文件大小和SHA-256。

只接受比当前程序更新的版本，不自动降级。生产公钥未配置或配置无效时菜单解释原因且不发网络请求。尚未发布更新清单时会提示尚无可用更新文件；这与“已是最新版本”不同。

## 发布与限制

版本、发行签名、GitHub Secrets、两端完整包和草稿验证流程见[发布说明](releases.md)。Mac更新包为universal DMG。默认可分发明确标注未公证的包；选择正式公证策略时需要Developer ID签名和Apple公证，失败不得降级。本机固定证书不替代它。Windows更新包为NSIS安装器，便携用户也可从Releases手动下载ZIP继续使用。

当前已准备客户端和发布配置，未上传源码、运行远程CI或公开Release，尚无真实在线升级验收。GitHub release environment还需配置更新私钥；选择Mac公证时还需配置Apple凭据；公钥必须在首个交付程序内，后续才能校验新版本。更新签名不替代操作系统软件签名。

下载没有单独的中途取消按钮；请求有明确超时。Mac成功的同版本镜像会校验后复用，失败的临时文件会清理；退出不会等待网络任务完成。
