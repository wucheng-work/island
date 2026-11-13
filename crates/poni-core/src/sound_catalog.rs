// 由 tools/make_windows_sounds.py 生成，请修改目录源文件后重新生成。
#[derive(Clone, Copy, Debug)]
pub struct BuiltinSound {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
}

pub const DEFAULT_FOCUS: &str = "modern-Pulse";
pub const DEFAULT_SHELF: &str = "classic-Telegraph";
pub const DEFAULT_CODEX: &str = "recent-Cheers";
pub const DEFAULT_WECHAT: &str = "classic-Tri-Tone";
pub const DEFAULT_MAIL: &str = "modern-Bamboo";

pub const SOUNDS: &[BuiltinSound] = &[
    BuiltinSound { id: "recent-Antic", name: "怪诞", category: "新款提示音" },
    BuiltinSound { id: "recent-Cheers", name: "欢呼", category: "新款提示音" },
    BuiltinSound { id: "recent-Droplet", name: "水滴", category: "新款提示音" },
    BuiltinSound { id: "recent-Handoff", name: "接力", category: "新款提示音" },
    BuiltinSound { id: "recent-Milestone", name: "重要事件", category: "新款提示音" },
    BuiltinSound { id: "recent-Passage", name: "通道", category: "新款提示音" },
    BuiltinSound { id: "recent-Portal", name: "门户", category: "新款提示音" },
    BuiltinSound { id: "recent-Rattle", name: "沙锤", category: "新款提示音" },
    BuiltinSound { id: "recent-Rebound", name: "反弹", category: "新款提示音" },
    BuiltinSound { id: "recent-Slide", name: "滑梯", category: "新款提示音" },
    BuiltinSound { id: "recent-Welcome", name: "欢迎", category: "新款提示音" },
    BuiltinSound { id: "modern-Aurora", name: "极光", category: "常用提示音" },
    BuiltinSound { id: "modern-Bamboo", name: "竹子", category: "常用提示音" },
    BuiltinSound { id: "modern-Chord", name: "和弦", category: "常用提示音" },
    BuiltinSound { id: "modern-Circles", name: "圆圈", category: "常用提示音" },
    BuiltinSound { id: "modern-Complete", name: "完成", category: "常用提示音" },
    BuiltinSound { id: "modern-Hello", name: "哈喽", category: "常用提示音" },
    BuiltinSound { id: "modern-Input", name: "输入", category: "常用提示音" },
    BuiltinSound { id: "modern-Keys", name: "按键", category: "常用提示音" },
    BuiltinSound { id: "modern-Note", name: "音符", category: "常用提示音" },
    BuiltinSound { id: "modern-Popcorn", name: "爆米花", category: "常用提示音" },
    BuiltinSound { id: "modern-Pulse", name: "脉冲", category: "常用提示音" },
    BuiltinSound { id: "modern-Synth", name: "合成", category: "常用提示音" },
    BuiltinSound { id: "classic-Telegraph", name: "敲打电报", category: "经典提示音" },
    BuiltinSound { id: "classic-Tri-Tone", name: "三全音", category: "经典提示音" },
];

pub fn find(id: &str) -> Option<&'static BuiltinSound> {
    SOUNDS.iter().find(|sound| sound.id == id)
}
