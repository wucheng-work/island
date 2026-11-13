//! 内置提示音目录与各模块的选择；音频随 Mac 应用携带，不嵌入可执行文件。

use serde::{Deserialize, Serialize};

/// 可分别选择音效的模块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundModule {
    Focus,
    Shelf,
    Codex,
    Mail,
}

impl SoundModule {
    pub const ALL: [Self; 4] = [Self::Focus, Self::Shelf, Self::Codex, Self::Mail];

    pub fn title(self) -> &'static str {
        match self {
            Self::Focus => "专注",
            Self::Shelf => "文件暂存区",
            Self::Codex => "Codex",
            Self::Mail => "邮件",
        }
    }

    pub fn default_id(self) -> &'static str {
        match self {
            Self::Focus => "modern-Pulse",
            Self::Shelf => "classic-Telegraph",
            Self::Codex => "recent-Cheers",
            Self::Mail => "modern-Bamboo",
        }
    }
}

/// 稳定标识和包内文件名分开保存，设置不依赖系统音频所在的路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertTone {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub file: &'static str,
}

/// 与 assets/sounds/catalog.json 的名称、分组和文件一一对应。
pub const ALERT_TONES: &[AlertTone] = &[
    AlertTone {
        id: "recent-Antic",
        name: "怪诞",
        group: "新款提示音",
        file: "recent-Antic.caf",
    },
    AlertTone {
        id: "recent-Cheers",
        name: "欢呼",
        group: "新款提示音",
        file: "recent-Cheers.caf",
    },
    AlertTone {
        id: "recent-Droplet",
        name: "水滴",
        group: "新款提示音",
        file: "recent-Droplet.caf",
    },
    AlertTone {
        id: "recent-Handoff",
        name: "接力",
        group: "新款提示音",
        file: "recent-Handoff.caf",
    },
    AlertTone {
        id: "recent-Milestone",
        name: "重要事件",
        group: "新款提示音",
        file: "recent-Milestone.caf",
    },
    AlertTone {
        id: "recent-Passage",
        name: "通道",
        group: "新款提示音",
        file: "recent-Passage.caf",
    },
    AlertTone {
        id: "recent-Portal",
        name: "门户",
        group: "新款提示音",
        file: "recent-Portal.caf",
    },
    AlertTone {
        id: "recent-Rattle",
        name: "沙锤",
        group: "新款提示音",
        file: "recent-Rattle.caf",
    },
    AlertTone {
        id: "recent-Rebound",
        name: "反弹",
        group: "新款提示音",
        file: "recent-Rebound.caf",
    },
    AlertTone {
        id: "recent-Slide",
        name: "滑梯",
        group: "新款提示音",
        file: "recent-Slide.caf",
    },
    AlertTone {
        id: "recent-Welcome",
        name: "欢迎",
        group: "新款提示音",
        file: "recent-Welcome.caf",
    },
    AlertTone {
        id: "modern-Aurora",
        name: "极光",
        group: "常用提示音",
        file: "modern-Aurora.m4r",
    },
    AlertTone {
        id: "modern-Bamboo",
        name: "竹子",
        group: "常用提示音",
        file: "modern-Bamboo.m4r",
    },
    AlertTone {
        id: "modern-Chord",
        name: "和弦",
        group: "常用提示音",
        file: "modern-Chord.m4r",
    },
    AlertTone {
        id: "modern-Circles",
        name: "圆圈",
        group: "常用提示音",
        file: "modern-Circles.m4r",
    },
    AlertTone {
        id: "modern-Complete",
        name: "完成",
        group: "常用提示音",
        file: "modern-Complete.m4r",
    },
    AlertTone {
        id: "modern-Hello",
        name: "哈喽",
        group: "常用提示音",
        file: "modern-Hello.m4r",
    },
    AlertTone {
        id: "modern-Input",
        name: "输入",
        group: "常用提示音",
        file: "modern-Input.m4r",
    },
    AlertTone {
        id: "modern-Keys",
        name: "按键",
        group: "常用提示音",
        file: "modern-Keys.m4r",
    },
    AlertTone {
        id: "modern-Note",
        name: "音符",
        group: "常用提示音",
        file: "modern-Note.m4r",
    },
    AlertTone {
        id: "modern-Popcorn",
        name: "爆米花",
        group: "常用提示音",
        file: "modern-Popcorn.m4r",
    },
    AlertTone {
        id: "modern-Pulse",
        name: "脉冲",
        group: "常用提示音",
        file: "modern-Pulse.m4r",
    },
    AlertTone {
        id: "modern-Synth",
        name: "合成",
        group: "常用提示音",
        file: "modern-Synth.m4r",
    },
    AlertTone {
        id: "classic-Telegraph",
        name: "敲打电报",
        group: "经典提示音",
        file: "classic-Telegraph.m4r",
    },
    AlertTone {
        id: "classic-Tri-Tone",
        name: "三全音",
        group: "经典提示音",
        file: "classic-Tri-Tone.m4r",
    },
];

pub fn tone(id: &str) -> Option<&'static AlertTone> {
    ALERT_TONES.iter().find(|tone| tone.id == id)
}

/// 缺少某个字段的旧设置使用该模块的默认音效。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertSounds {
    pub focus: String,
    pub shelf: String,
    pub codex: String,
    pub mail: String,
}

impl Default for AlertSounds {
    fn default() -> Self {
        Self {
            focus: SoundModule::Focus.default_id().into(),
            shelf: SoundModule::Shelf.default_id().into(),
            codex: SoundModule::Codex.default_id().into(),
            mail: SoundModule::Mail.default_id().into(),
        }
    }
}

impl AlertSounds {
    pub fn get(&self, module: SoundModule) -> &str {
        match module {
            SoundModule::Focus => &self.focus,
            SoundModule::Shelf => &self.shelf,
            SoundModule::Codex => &self.codex,
            SoundModule::Mail => &self.mail,
        }
    }

    pub fn set(&mut self, module: SoundModule, id: &str) {
        let selected = match module {
            SoundModule::Focus => &mut self.focus,
            SoundModule::Shelf => &mut self.shelf,
            SoundModule::Codex => &mut self.codex,
            SoundModule::Mail => &mut self.mail,
        };
        *selected = id.into();
    }

    /// 设置文件是外部输入，未知音效在读取时统一恢复为模块默认值。
    pub fn sanitize(&mut self) {
        for module in SoundModule::ALL {
            if tone(self.get(module)).is_none() {
                self.set(module, module.default_id());
            }
        }
    }
}
