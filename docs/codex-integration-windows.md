# Codex 提醒、快捷回复和对话（Windows）

在“设置 → Codex 提醒”开启接入，可接收本轮结束和审批提醒、查看原会话历史及 Markdown 内容、向原会话排队发送回复，也可回答 `island_ask` 提出的普通问题。此版本不会接管会话执行，不会替你批准权限。

## 准备

1. 将 Island 放到固定文件夹，例如 `C:\Apps\Island\Island.exe`，运行后开启“启用 Codex 提醒”。移动程序后需要更新下面配置中的路径。
2. 提醒与 MCP 回答只需要 Island 本身。历史读取和原会话快捷回复还需要本机安装 Codex CLI，并支持 `queue`、`app-server` 和 `thread/items/list`；按所装 Codex 版本的官方说明安装/更新。
3. 程序查找 PATH 中的 `codex.exe`、Codex 桌面资源目录以及默认 npm 全局安装目录中的原生 exe。仅有 `codex.cmd` 而没有可找到的原生 exe 时，请将原生 `codex.exe` 所在目录加入用户 PATH 后重新启动 Island。不会通过 shell 拼接和执行回复文本。

开启后使用只允许当前 Windows 用户访问的命名管道，不监听网络端口，不读取 Codex 登录凭据。关闭接入或退出应用会结束等待、清除岛内消息和草稿；已交给 Codex 的队列消息不会撤回。同一 Windows 用户下的其他进程仍处于同一本机信任范围。

## 本轮结束和审批提醒

在 `%USERPROFILE%\.codex\hooks.json` 的现有 `hooks` 对象中合并以下内容，保留已有设置。若修改过 `CODEX_HOME`，使用该目录中的文件。路径以你的实际 exe 为准，JSON 中的反斜杠必须转义。

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"C:\\Apps\\Island\\Island.exe\" --codex-hook",
            "timeout": 5
          }
        ]
      }
    ],
    "PermissionRequest": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"C:\\Apps\\Island\\Island.exe\" --codex-hook",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

在 Codex 中用 `/hooks` 审阅并信任新定义，按当前版本要求重启或新开会话使配置生效。Island 不会改写配置或替你确认信任。若 Hooks 功能曾关闭，需在已有 `[features]` 表内启用 `hooks = true`。

`Stop` 表示当前轮次停止，不等于整个任务完成。`PermissionRequest` 只提醒你回 Codex 审批。未开启 Island 或消息格式无效时，Hook 安静退出，输出 `{}`，不阻断 Codex。

旧版 Codex 如支持顶层 `notify`，也可用以下配置接收 `agent-turn-complete`。它与上述 Stop Hook 选一种，避免重复提醒；保留 TOML 其他内容。

```toml
notify = ['C:\Apps\Island\Island.exe', '--codex-hook']
```

## 岛内普通问题和回答

在 `%USERPROFILE%\.codex\config.toml` 中增加独立表，不覆盖原有配置；TOML 单引号路径中反斜杠无需转义。

```toml
[mcp_servers.island]
command = 'C:\Apps\Island\Island.exe'
args = ["--codex-mcp"]
startup_timeout_sec = 10
tool_timeout_sec = 660
```

重启 Codex 或新开会话后，应能看到 `island_notify` 和 `island_ask`。前者发送摘要，后者等待你明确输入普通问题的回答。可在原会话要求：

```text
需要我回答普通澄清问题时，请调用 island_ask，把完整问题写入 text。不要用它处理权限审批、密码或专用确认；取消或超时不表示同意。
```

两种工具的 `title`、`text` 必填；`thread_id`、`turn_id`、`cwd` 仅在已知时提供，不猜测 ID。普通问题最长等待 10 分钟；回答限 1–2000 字，必须点击发送。取消、超时、断线和关闭接入均不视为同意。

## 提醒和对话操作

- 点击收起时的普通提醒或审批提醒正文，打开桌面端的对应本机会话。缺少有效会话 ID 或系统无法打开链接时，Island 会显示原因，可从消息列表查看原提醒。点击岛内提问会展开该问题。
- 结束提醒中的“确认”只把固定文本“确认”加入对应原会话队列。权限审批不显示这个操作。
- 点击消息卡片查看完整对话；输入自定义内容后点击发送。回车确认中文候选词不会自动发送。
- “更早消息”和“刷新”按需读取真实历史；读取失败会保留现有内容和草稿。关闭页面后不轮询历史。
- “已加入 Codex 队列”表示 CLI 接受排队，不代表模型已经处理。结果待核对时先检查原会话，确定未收到再明确重试，程序不会自动重发。
- Windows 附件卡片在资源管理器中选中文件，由你决定如何查看；不会通过默认关联直接执行附件。Markdown 正文、表格和代码块直接在岛内显示。
- 岛内页面的“打开 Codex”打开桌面端的对应本机会话，无法打开时显示提示。
- “清空已结束”仅清理岛内记录；待回答、发送中、结果未知和有草稿的入口会保留，不删除 Codex 原会话。

“显示消息预览”关闭时隐藏提醒和列表正文、名称；锁屏/熄屏撤下预览。消息、历史与草稿仅在内存中保留；明确发送的文本会进入 Codex 自己的会话。提示音在“设置 → Codex 提醒”单独控制。

## 排查

先确认 Island 运行且接入已开启、exe 路径与配置一致，再检查 Hook 信任或 MCP 配置。只有提醒可用而历史/回复失败时，检查 Codex CLI 安装及版本支持。程序不会另起执行器恢复会话；原会话闲置时请在 Codex 中打开它。隔离运行可设置 `PONI_HOME`；Island 与辅助进程必须使用完全相同的值，才能连接同一命名管道。

此版本已完成 Windows 交叉编译检查；Windows 下真实 Codex 安装布局、Hooks 触发、历史和队列投递仍需在目标电脑验收。不要用编译成功代替真实投递确认。
