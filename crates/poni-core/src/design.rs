//! 跨平台绘制使用的设计尺度；颜色仍由 geom::palette 与 Look 负责主题映射。
//! 页面保留业务特有布局，通用控件不再分别维护字号、间距和圆角。

pub mod type_scale {
    pub const CAPTION: f32 = 11.0;
    pub const FOOTNOTE: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const HEADLINE: f32 = 15.0;
    pub const VALUE: f32 = 18.0;
    pub const DISPLAY: f32 = 30.0;
}

pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

pub mod radius {
    pub const CONTROL: f32 = 8.0;
    pub const PANEL: f32 = 12.0;
    pub const WINDOW: f32 = 14.0;
}

pub mod control {
    pub const SMALL: f32 = 28.0;
    pub const REGULAR: f32 = 34.0;
    pub const ICON: f32 = 14.0;
    pub const ICON_SMALL: f32 = 13.0;
    pub const ICON_GAP: f32 = 6.0;
    pub const ROW: f32 = 40.0;
    pub const ROW_DETAIL: f32 = 52.0;
}
