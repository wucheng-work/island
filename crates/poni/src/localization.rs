//! 简体中文界面的轻量本地化层。
//!
//! Oyenza 的页面是即时绘制的，文字来自很多共享页面和活动模块。把翻译
//! 放在渲染边界，可以覆盖顶部灵动岛、页面、提示和设置窗口，同时避免在
//! 业务代码里到处复制一套语言判断。未收录的文字保留原文，歌曲名、歌词、
//! 文件名和账户内容也不会被误翻译。

use std::borrow::Cow;
use poni_core::datetime::LocalDateTime;

/// 顶部日期与待机状态栏共用的中文短日期。
pub fn date_short(t: &LocalDateTime) -> String {
    const WEEKDAYS: [&str; 7] = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    format!("{} {}月{}日", WEEKDAYS[t.weekday() as usize], t.month, t.day)
}

/// 将应用内置的英文 UI 文案翻译为简体中文。
///
/// 这是灵动岛的中文构建，暂时固定为简体中文；后续如果加入语言
/// 选择，只需要把这里的语言来源改成设置项即可。
pub fn translate(s: &str) -> Cow<'_, str> {
    let translated = match s {
        // 页面和顶部灵动岛
        "Music" => "音乐",
        "Focus" => "专注",
        "Shelf" => "暂存架",
        "Today" => "今天",
        "System" => "系统",
        "Oyenza" => "Oyenza",
        "Notifications" => "通知",
        "Settings" => "设置",
        "Close settings" => "关闭设置",
        "Keep open" => "保持展开",
        "Unpin" => "取消固定",

        // 通用按钮和交互
        "Open" => "打开",
        "Close" => "关闭",
        "Save" => "保存",
        "Saved" => "已保存",
        "Clear" => "清除",
        "Undo" => "撤销",
        "Cancel" => "取消",
        "Start" => "开始",
        "Pause" => "暂停",
        "Resume" => "继续",
        "Stop" => "停止",
        "Retry" => "重试",
        "Try again" => "再试一次",
        "Update" => "更新",
        "Update available" => "有可用更新",
        "Version" => "版本",
        "Install now" => "立即安装",
        "Download" => "下载",
        "Remove" => "移除",
        "Connect" => "连接",
        "Connected" => "已连接",
        "Disconnected" => "已断开",
        "Sign in" => "登录",
        "Sign out" => "退出登录",
        "Show" => "显示",
        "Hide" => "隐藏",
        "Done" => "完成",
        "OK" => "好",
        "Copy" => "复制",
        "Delete" => "删除",
        "Add" => "添加",
        "Edit" => "编辑",
        "Next" => "下一步",
        "Back" => "返回",
        "Allow" => "允许",
        "Join" => "加入",
        "Open oyenza.app" => "打开 oyenza.app",
        "Open settings file" => "打开设置文件",
        "Open data folder" => "打开数据文件夹",

        // 音乐和歌词
        "Nothing playing" => "当前没有播放内容",
        "Unknown title" => "未知标题",
        "Paused" => "已暂停",
        "PAUSED" => "已暂停",
        "Instrumental" => "纯音乐",
        "Lyrics unavailable" => "歌词不可用",
        "Finding lyrics…" => "正在查找歌词…",
        "Unsynced" => "未同步",
        "Synced lyrics aren't available" => "暂未获取到同步歌词",
        "Synced lyrics" => "同步歌词",
        "Show what's playing" => "显示正在播放",
        "Music and lyrics" => "音乐和歌词",
        "Timing" => "时间偏移",
        "Play" => "播放",
        "Next track" => "下一首",
        "Previous track" => "上一首",

        // 账户和系统概览
        "Offline" => "离线",
        "Updated" => "已更新",
        "until" => "结束于",
        "Check the connection, then try again." => "请检查连接后重试。",
        "The Oyenza account connection is unavailable in this version." => "此版本已停用 Oyenza 账户连接。",
        "Oyenza account connection unavailable" => "Oyenza 账户连接不可用",
        "CPU" => "CPU",
        "Memory" => "内存",
        "Disk" => "磁盘",
        "Battery" => "电池",
        "Uptime" => "运行时间",
        "Idle" => "空闲",
        "Light load" => "轻负载",
        "Moderate" => "中等负载",
        "Busy" => "繁忙",
        "Very busy" => "高负载",
        "free" => "可用",
        "Charging" => "充电中",
        "Fully charged" => "已充满",
        "Plugged in" => "已接通电源",
        "Low battery" => "电量低",
        "Battery saver" => "省电模式",
        "On battery" => "使用电池",
        "Muted" => "已静音",
        "updated" => "更新于",
        "left" => "剩余",
        "today" => "今天",
        "days" => "天",

        // 任务、专注和暂存架
        "Add a task" => "添加任务",
        "New task" => "新任务",
        "No tasks" => "暂无任务",
        "Tomorrow" => "明天",
        "Overdue" => "已逾期",
        "Focus session" => "专注时段",
        "Focus ·" => "专注 ·",
        "Short break" => "短休息",
        "Long break" => "长休息",
        "Break" => "休息",
        "Start focus" => "开始专注",
        "Take a break" => "休息一下",
        "Reset" => "重置",
        "Skip" => "跳过",
        "No focus sessions yet today" => "今天还没有专注时段",
        "Break's over" => "休息结束",
        "Click to start the next focus" => "点击开始下一次专注",
        "Open the Shelf" => "打开暂存架",
        "Shelf is empty" => "暂存架为空",
        "Task list is full" => "任务列表已满",
        "Add a task, Enter to save" => "添加任务，按回车保存",
        "Nothing on your list" => "列表中还没有任务",
        "Done tasks clear at the end of the day" => "已完成任务会在每天结束时清除",
        "Clear done" => "清除已完成",
        "All done for today" => "今天全部完成",
        "List is full — clear done tasks to add more" => "列表已满，请清除已完成任务后再添加",
        "Shelf cleared" => "暂存架已清空",
        "Drop files here" => "将文件拖到这里",
        "Drag files or folders onto the island to keep them close" => "将文件或文件夹拖到灵动岛，方便随时取用",
        "Oyenza Studio keeps a link — the files stay where they are" => "仅保存文件引用，原文件保留在原位置",
        "Drop to add to Shelf" => "拖放到这里添加到暂存架",
        "Missing" => "文件不存在",
        "Folder" => "文件夹",
        "File" => "文件",

        // 系统提示和通知
        "THE ISLAND SHOWS" => "灵动岛显示",
        "APPS" => "应用",
        "No disk activity" => "没有磁盘活动",
        "Upload" => "上传",
        "Status" => "状态",
        "Address" => "地址",
        "No network adapter is connected" => "未连接网络适配器",
        "Power adapter" => "电源适配器",
        "Fan" => "风扇",
        "Stopped" => "已停止",
        "Show over games" => "在游戏上方显示",
        "Allow FPS" => "允许读取 FPS",
        "No frames yet" => "暂时没有帧率数据",
        "Start a game in borderless full screen" => "启动无边框全屏游戏",
        "No game in front" => "前台没有游戏",
        "Update to" => "更新到",
        "Open Anyway" => "仍要打开",

        // 设置窗口和菜单
        "General" => "通用",
        "Appearance" => "外观",
        "Island" => "灵动岛",
        "Pop-ups" => "弹出提示",
        "Tabs" => "标签页",
        "Advanced settings" => "高级设置",
        "Theme" => "主题",
        "THEME" => "主题",
        "IDLE LOOK" => "待机样式",
        "Classic" => "经典",
        "Oyen" => "Oyen",
        "Glass" => "玻璃",
        "Pill" => "胶囊",
        "Diorama" => "立体场景",
        "Idle look" => "待机样式",
        "Clock" => "时钟",
        "Eyes" => "眼睛",
        "Minimal" => "简洁",
        "Hidden until hover" => "悬停时显示",
        "Hidden" => "隐藏",
        "Status bar" => "状态栏",
        "Display" => "显示器",
        "Main display" => "主显示器",
        "Under the pointer" => "鼠标所在显示器",
        "Under the pointer at startup" => "启动时鼠标所在显示器",
        "The screen the island hangs on" => "灵动岛所在的屏幕",
        "Chosen at startup or when changed; does not follow the pointer" => "启动或切换时选屏，不随鼠标移动",
        "In full-screen apps" => "全屏应用中",
        "Stay on top" => "保持置顶",
        "Start at login" => "登录时启动",
        "Start with Windows" => "登录 Windows 时启动",
        "Open where you left off" => "打开上次所在位置",
        "When the island opens" => "打开灵动岛时",
        "Tabs in the island" => "灵动岛中的标签页",
        "Never" => "从不",
        "Off" => "关闭",
        "At once" => "立即",
        "Short" => "短",
        "Long" => "长",
        "Quick" => "快速",
        "Normal" => "正常",
        "Slow" => "慢速",
        "Sounds" => "声音",
        "Sound" => "声音",
        "Lyrics" => "歌词",
        "Check now" => "立即检查",
        "Checking…" => "正在检查…",
        "Up to date" => "已是最新",
        "What's new" => "更新内容",
        "WHAT'S NEW" => "更新内容",
        "Not now" => "暂不",
        "Data folder" => "数据文件夹",
        "Disconnect" => "断开连接",
        "Show amounts" => "显示金额",
        "Signed in as" => "登录身份：",
        "Update ready to install" => "更新已准备好安装",
        "A new version of Oyenza Studio is ready to be installed." => "Island For Mac 新版本已准备好安装。",
        "Installs on next start" => "已设为下次启动时安装",
        "Set: before it next opens" => "设置为下次打开前安装",
        "Before it next opens" => "下次打开前安装",
        "Closes, updates and opens again" => "关闭应用、完成更新后重新打开",
        "Downloads the DMG: open it and drag Oyenza Studio to Applications." => {
            "下载 DMG，打开后将 Island For Mac 拖到“应用程序”。"
        }
        "Opens its release page: run the Setup there, your settings stay." => {
            "打开发布页面，在那里运行安装程序，设置会保留。"
        }
        "See what's new in Oyenza Studio" => "查看 Island For Mac 的更新内容",
        "Install on next start" => "下次启动时安装",
        "Set for next start" => "设为下次启动",
        "On next start" => "下次启动时",
        "Oyenza Studio" => "Island For Mac",
        "24-hour clock" => "24 小时制",
        "Opens Oyenza Studio when you log in" => "登录时打开 Island For Mac",
        "A soft sound for timers, drops and reminders" => "计时器、拖放和提醒使用轻柔提示音",

        // 设置窗口的说明文案。
        "A black island at the top edge" => "屏幕顶部的黑色灵动岛",
        "Oyen lives in the island" => "Oyen 住在灵动岛里",
        "Frosted glass over your screen" => "屏幕上的磨砂玻璃",
        "Floats just below the top edge" => "悬浮在屏幕顶部下方",
        "A little place that keeps your day" => "陪伴你一天的小小场景",
        "Opens Oyenza Studio when you sign in" => "登录时打开 Island For Mac",
        "On time" => "无偏移",
        "Keep System page history" => "保留系统页历史",
        "The charts keep the last minute while the page is closed" => "页面关闭时仍保留最近一分钟的图表数据",
        "Island at rest" => "待机状态",
        "What the closed island shows" => "灵动岛收起时显示的内容",
        "Games and videos that fill the screen" => "占满屏幕的游戏和视频",
        "Show FPS, GPU and CPU over games" => "在游戏上方显示 FPS、GPU 和 CPU",
        "Keeps the island on top of full-screen apps" => "让灵动岛显示在全屏应用上方",
        "Only for a few seconds" => "仅显示几秒",
        "Then the island hides and adds no delay to the game" => "随后隐藏灵动岛，不影响游戏响应",
        "Opening and closing" => "展开与收起",
        "Opens after" => "展开延迟",
        "How long the pointer rests on the island first" => "鼠标停留多久后展开灵动岛",
        "Closes" => "收起延迟",
        "Once the pointer leaves the open island" => "鼠标离开展开的灵动岛后",
        "Width when open" => "展开宽度",
        "Wider gives the pages more room" => "增加宽度可容纳更多页面内容",
        "Where it shows" => "显示位置",
        "Lyrics come from the current player's service" => "仅使用当前播放器提供的歌词",
        "Shift every line earlier or later" => "让所有歌词提前或延后显示",
        "Closed island" => "收起状态",
        "The song and its cover on the closed island" => "在收起的灵动岛上显示歌曲和封面",
        "The island shows a pop-up for" => "显示以下弹出提示",
        "Volume" => "音量",
        "When you change the volume or mute" => "调整音量或静音时",
        "Charger and battery" => "充电与电池",
        "When you plug in, unplug or run low" => "接通、断开电源或电量不足时",
        "Bluetooth" => "蓝牙",
        "When headphones and other devices connect" => "耳机等设备连接时",
        "The page you last had open" => "上次打开的页面",
        "Opens on" => "默认打开页面",
        "Messages from WhatsApp, Telegram and your phone" => "来自 WhatsApp、Telegram 和手机的消息",
        "Lengths" => "时长",
        "Duration changes apply to the next session" => "修改后从下一时段开始生效",
        "Long break after" => "长休息间隔",
        "Focus sessions before a long break" => "完成多少次专注后进入长休息",
        "Starting by itself" => "自动开始",
        "Breaks" => "休息",
        "Focus sessions" => "专注时段",
        "A break starts when a focus session ends" => "专注结束后自动开始休息",
        "The next session starts when a break ends" => "休息结束后自动开始下一次专注",
        "Files" => "文件",
        "Tasks, the Shelf and your Focus history" => "任务、文件暂存区和专注历史",
        "Permission required" => "需要权限",

        // 系统详情。
        "Drives" => "磁盘",
        "Network" => "网络",
        "Power" => "功耗",
        "Temperature" => "温度",
        "User" => "用户",
        "Efficiency cores" => "能效核心",
        "Performance cores" => "性能核心",
        "DETAILS" => "详细信息",
        "USAGE HISTORY" => "使用记录",
        "PER CORE" => "各核心",
        "Not reported" => "未提供",
        "Shared memory" => "共享内存",
        "Renderer" => "渲染器",
        "Tiler" => "分块器",
        "ENGINES" => "引擎",
        "ADAPTER" => "适配器",
        "Built into the chip" => "芯片集成",
        "Built into the CPU" => "CPU 集成",
        "Dedicated" => "独立显卡",
        "Type" => "类型",
        "Sensors" => "传感器",
        "Swap" => "交换空间",
        "Page file" => "分页文件",
        "App" => "应用",
        "Wired" => "联动内存",
        "Compressed" => "已压缩",
        "Cached" => "已缓存",
        "Free" => "空闲",
        "USED" => "已使用",
        "DRIVES" => "磁盘",
        "Reading…" => "读取中…",
        "Read" => "读取",
        "Write" => "写入",
        "Active time" => "活跃时间",
        "ACTIVITY" => "活动",
        "Interface" => "接口",
        "Link speed" => "连接速度",
        "SINCE STARTUP" => "启动以来",
        "Received" => "已接收",
        "Sent" => "已发送",
        "LAST MINUTE" => "最近一分钟",
        "Peak download" => "下载峰值",
        "Peak upload" => "上传峰值",
        "Discharging" => "放电中",
        "Not charging" => "未充电",
        "Level" => "电量",
        "Source" => "来源",
        "SOURCE" => "来源",
        "State" => "状态",
        "Time left" => "剩余时间",
        "Low Power Mode" => "低电量模式",
        "On" => "已开启",
        "HEALTH" => "健康度",
        "Full charge" => "满充容量",
        "Design" => "设计容量",
        "POWER" => "功耗",
        "Using" => "当前功耗",
        "Cycles" => "循环次数",
        "Whole Mac" => "整机",
        "CPU load" => "CPU 负载",
        "Graphics load" => "图形负载",
        "THIS MAC" => "这台 Mac",
        "POWER HISTORY" => "功耗记录",
        "THIS MINUTE" => "本分钟",
        "TOP PROCESSES" => "主要进程",
        "The Mac's own sensor" => "Mac 内置传感器",
        "What its System Management Controller measures, the number Stats shows as System total. macOS doesn't split it by part." => "系统管理控制器测得的整机功耗，与 Stats 的系统总功耗一致。macOS 不提供各部件的单独数值。",
        "Live CPU, GPU, memory, drives, network and power, and the apps using them." => "实时查看 CPU、GPU、内存、磁盘、网络、功耗及相关应用。",
        // 页面中的说明和状态。
        "Show in Finder" => "在访达中显示",
        "Show in folder" => "显示所在文件夹",
        "Video" => "视频",
        "Paste" => "粘贴",
        "Clear finished" => "清除已完成",
        "Downloading" => "下载中",
        "tomorrow" => "明天",
        "Card bill" => "信用卡账单",
        "Renews" => "续订",
        "Due" => "到期",
        "Can't reach Oyenza" => "无法连接 Oyenza",
        "You're signed out of Oyenza" => "Oyenza 已退出登录",

        _ => s,
    };

    if translated != s {
        Cow::Borrowed(translated)
    } else {
        translate_dynamic(s)
    }
}

fn translate_dynamic(s: &str) -> Cow<'_, str> {
    if let Some(span) = s.strip_suffix(" left").and_then(|span| translate_span(span, true)) {
        return Cow::Owned(format!("剩余 {span}"));
    }
    if let Some(span) = s.strip_prefix("in ").and_then(|span| translate_span(span, false)) {
        return Cow::Owned(format!("{span}后"));
    }
    if let Some(rest) = s.strip_prefix("Focus · ") {
        if let Some(minutes) = rest.strip_suffix(" min") {
            return Cow::Owned(format!("专注 · {minutes} 分钟"));
        }
    }
    if let Some(rest) = s.strip_prefix("until ") {
        return Cow::Owned(format!("结束于 {rest}"));
    }
    if let Some(version) = s.strip_prefix("Update to ") {
        return Cow::Owned(format!("更新到 {version}"));
    }
    if let Some(version) = s.strip_prefix("Update available ") {
        return Cow::Owned(format!("有可用更新 {version}"));
    }
    if let Some(version) = s.strip_prefix("Download ") {
        return Cow::Owned(format!("下载 {version}"));
    }
    if let Some(size) = s.strip_prefix("A new version of Oyenza Studio is ready to download (").and_then(|s| s.strip_suffix(").")) {
        return Cow::Owned(format!("Island For Mac 新版本可供下载（{size}）。"));
    }
    if let Some(version) = s.strip_prefix("See what's new in Oyenza Studio ") {
        return Cow::Owned(format!("查看 Island For Mac {version} 的更新内容"));
    }
    if let Some(version) = s.strip_prefix("Version ") {
        return Cow::Owned(format!("版本 {version}"));
    }
    if let Some(clock) = s.strip_prefix("Since ") {
        return Cow::Owned(format!("自 {clock}"));
    }
    if let Some(when) = s.strip_prefix("Updated ") {
        return Cow::Owned(format!("更新于 {when}"));
    }
    if let Some(when) = s.strip_prefix("Live · updated ") {
        return Cow::Owned(format!("直播 · 更新于 {when}"));
    }
    if let Some(count) = s.strip_suffix(" focus sessions today") {
        return Cow::Owned(format!("今天专注了 {count} 次"));
    }
    if s == "1 focus session today" {
        return Cow::Borrowed("今天专注了 1 次");
    }
    if let Some(minutes) = s.strip_suffix(" min focus started") {
        return Cow::Owned(format!("已开始 {minutes} 分钟专注"));
    }
    if let Some(rest) = s.strip_prefix("Focus done — ") {
        return Cow::Owned(format!("专注完成 — {rest}"));
    }
    if let Some(rest) = s.strip_prefix("Cycle done — ") {
        return Cow::Owned(format!("循环完成 — {rest}"));
    }
    if let Some(rest) = s.strip_prefix("Signed in as ") {
        return Cow::Owned(format!("登录身份：{rest}"));
    }
    if let Some(rest) = s.strip_prefix("List is full (") {
        return Cow::Owned(format!("列表已满（{rest}"));
    }
    if let Some(rest) = s.strip_suffix(" done today") {
        if rest.contains('/') {
            return Cow::Owned(format!("今天完成 {rest}"));
        }
    }
    Cow::Borrowed(s)
}

/// 只识别业务层输出的数值时长，避免把含有 in/left 的普通标题误当状态。
fn translate_span(s: &str, seconds: bool) -> Option<String> {
    if let Some(minutes) = s.strip_suffix(" min") {
        if let Some((hours, minutes)) = minutes.split_once(" h ") {
            if is_digits(hours) && is_digits(minutes) {
                return Some(format!("{hours} 小时 {minutes} 分钟"));
            }
            return None;
        }
        if is_digits(minutes) {
            return Some(format!("{minutes} 分钟"));
        }
    }
    if let Some(hours) = s.strip_suffix(" h") {
        if is_digits(hours) {
            return Some(format!("{hours} 小时"));
        }
    }
    if seconds {
        if let Some(value) = s.strip_suffix(" s") {
            if is_digits(value) {
                return Some(format!("{value} 秒"));
            }
        }
    }
    None
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::translate;

    #[test]
    fn translates_common_labels_and_preserves_dynamic_content() {
        assert_eq!(translate("Today"), "今天");
        assert_eq!(translate("Drop files here"), "将文件拖到这里");
        assert_eq!(translate("Drag files or folders onto the island to keep them close"), "将文件或文件夹拖到灵动岛，方便随时取用");
        assert_eq!(translate("A song title"), "A song title");
        assert_eq!(translate("文件 and 名称.png"), "文件 and 名称.png");
        assert_eq!(translate("中文歌词"), "中文歌词");
    }
}
