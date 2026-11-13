//! 两个平台共用的字体样式契约。
//!
//! 字号来自 core 设计尺度；具体字体族、CJK 回退、主题字体与排版测量由平台 Canvas 负责。
//! 保留统一的字重、对齐、行距、换行及用户选色语义，避免两套 builder 逐渐分歧。

use poni_core::geom::Color;
use poni_core::design::type_scale as ts;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: Weight,
    pub color: Color,
    /// 用户选择的颜色直接绘制，保留主题字体但不替换颜色。
    pub preserve_color: bool,
    pub align: Align,
    /// Fixed-width digits (timers, clocks) so numbers don't jiggle.
    pub tabular: bool,
    /// Wrap onto several lines instead of a single ellipsized line.
    pub wrap: bool,
    /// Vertically centre inside the rect (default) or top-align.
    pub vcenter: bool,
    /// Uniform line height as a multiple of the size (0 = font default).
    pub line: f32,
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
impl TextStyle {
    pub fn new(size: f32) -> Self {
        Self { size, weight: Weight::Regular, color: poni_core::geom::palette::TEXT, preserve_color: false, align: Align::Left, tabular: false, wrap: false, vcenter: true, line: 0.0 }
    }
    pub fn caption() -> Self {
        Self::new(ts::CAPTION)
    }
    pub fn footnote() -> Self {
        Self::new(ts::FOOTNOTE)
    }
    pub fn body() -> Self {
        Self::new(ts::BODY)
    }
    pub fn headline() -> Self {
        Self::new(ts::HEADLINE).weight(Weight::Semibold)
    }
    pub fn value() -> Self {
        Self::new(ts::VALUE).weight(Weight::Semibold)
    }
    pub fn display() -> Self {
        Self::new(ts::DISPLAY).weight(Weight::Semibold)
    }
    /// Uniform line spacing (e.g. 1.2) so wrapped lines read as one unit.
    pub fn line_spacing(mut self, mult: f32) -> Self {
        self.line = mult;
        self
    }
    pub fn weight(mut self, w: Weight) -> Self {
        self.weight = w;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self.preserve_color = false;
        self
    }
    pub fn exact_color(mut self, c: Color) -> Self {
        self.color = c;
        self.preserve_color = true;
        self
    }
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    pub fn center(self) -> Self {
        self.align(Align::Center)
    }
    pub fn right(self) -> Self {
        self.align(Align::Right)
    }
    pub fn tabular(mut self) -> Self {
        self.tabular = true;
        self
    }
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
    pub fn top(mut self) -> Self {
        self.vcenter = false;
        self
    }
}

/// 自选歌词色保持原值；默认色只在这里按当前主题解析，绘制时不再二次映射。
pub fn lyrics_color(rgb: Option<u32>, look: &poni_core::look::Look) -> Color {
    rgb.map(Color::hex).unwrap_or_else(|| look.paint_text(poni_core::geom::palette::BLUE))
}
