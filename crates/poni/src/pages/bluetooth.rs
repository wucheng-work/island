//! 共用蓝牙页面：展示系统提供的连接设备与电量，不主动扫描或连接设备。

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, Input, PillStyle, Ui};
use poni_core::bluetooth::DeviceKind;
use poni_core::geom::{palette, Color, Point, Rect};

const HEIGHT: f32 = 256.0;
const LIST_Y: f32 = 48.0;
const LIST_H: f32 = 168.0;
const ROW_H: f32 = 56.0;

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let paused = !st.bluetooth.active;
    let loading = st.bluetooth.loading;
    let has_snapshot = st.bluetooth.updated_at.is_some();
    let failed = st.bluetooth.error.is_some();
    let old_reading = has_snapshot && (paused || loading || failed || st.bluetooth.stale);
    let status = if paused {
        if has_snapshot { "刷新已暂停 · 显示上次读数".into() } else { "刷新已暂停".into() }
    } else if loading {
        if has_snapshot { "正在刷新 · 显示上次读数".into() } else { "正在读取设备…".into() }
    } else if failed {
        if has_snapshot { "刷新失败 · 显示上次读数".into() } else { "读取失败".into() }
    } else if st.bluetooth.stale {
        "显示上次读数".into()
    } else if st.bluetooth.warning.is_some() {
        format!("已连接 {} 台设备 · 部分电量暂不可用", st.bluetooth.devices.len())
    } else if st.bluetooth.devices.is_empty() {
        "等待设备连接".into()
    } else {
        format!("已连接 {} 台设备 · 每30秒刷新", st.bluetooth.devices.len())
    };

    let settings_w = ui.pill_width("蓝牙设置", true, true);
    let refresh_label = if failed && !loading { "重试" } else { "刷新" };
    let refresh_w = ui.pill_width(refresh_label, true, true);
    let refresh = Rect::new(area.right() - refresh_w, area.y, refresh_w, 28.0);
    let settings = Rect::new(refresh.x - 8.0 - settings_w, area.y, settings_w, 28.0);
    ui.c.text("蓝牙设备", Rect::new(area.x, area.y, (settings.x - area.x - 12.0).max(0.0), 24.0), &TextStyle::headline());
    let status_color = if failed && !loading && !paused { palette::ORANGE } else { palette::TEXT_DIM };
    ui.c.text_raw(&status, Rect::new(area.x, area.y + 27.0, area.w, 16.0), &TextStyle::footnote().color(status_color));

    if ui.pill(wid("bluetooth.settings"), settings, "蓝牙设置", Some(Icon::Gear), PillStyle::Utility).clicked {
        #[cfg(windows)]
        st.cmd(Command::OpenUrl("ms-settings:bluetooth".into()));
        #[cfg(target_os = "macos")]
        st.cmd(Command::OpenUrl("x-apple.systempreferences:com.apple.BluetoothSettings".into()));
    }
    if !loading && !paused {
        if ui.pill(wid("bluetooth.refresh"), refresh, refresh_label, Some(Icon::Reset), PillStyle::UtilityAccent(st.accent)).clicked {
            st.cmd(Command::RefreshBluetooth);
        }
    } else {
        // 仍由同一控件绘制禁用外观，但丢弃命中区，避免灰色按钮可点或出现手形指针。
        let mut input = Input::default();
        let mut hits = Vec::new();
        let now = ui.now;
        ui.c.with_alpha(0.45, |canvas| {
            let mut disabled = Ui::new(canvas, &mut input, &mut hits, now);
            disabled.pill(wid("bluetooth.refresh"), refresh, refresh_label, Some(Icon::Reset), PillStyle::Utility);
        });
    }

    let list = Rect::new(area.x, area.y + LIST_Y, area.w, LIST_H);
    ui.c.fill_rrect(list, 12.0, palette::SURFACE);
    if st.bluetooth.devices.is_empty() {
        let (icon, title, detail) = if old_reading {
            (Icon::Bluetooth, "上次读取未发现已连接设备", "")
        } else if paused {
            (Icon::Bluetooth, "蓝牙查询已暂停", "")
        } else if loading {
            (Icon::Bluetooth, "正在读取蓝牙设备", "")
        } else if let Some(error) = st.bluetooth.error.as_deref() {
            (Icon::Warning, "暂时无法读取蓝牙设备", error)
        } else {
            (Icon::Bluetooth, "暂无已连接设备", "请先在系统蓝牙设置中连接设备")
        };
        let color = if icon == Icon::Warning { palette::ORANGE } else { palette::TEXT_DIM };
        ui.c.icon(icon, Rect::from_center(Point::new(list.center().x, list.y + 43.0), 28.0, 28.0), color);
        ui.c.text(title, Rect::new(list.x + 16.0, list.y + 65.0, list.w - 32.0, 24.0), &TextStyle::body().center());
        let detail_rect = Rect::new(list.x + 20.0, list.y + 96.0, list.w - 40.0, 42.0);
        ui.c.clip(detail_rect);
        ui.c.text_raw(detail, detail_rect, &TextStyle::footnote().center().wrap().top().color(palette::TEXT_DIM));
        ui.c.unclip();
        st.bluetooth.scroll = 0.0;
    } else {
        let content_h = st.bluetooth.devices.len() as f32 * ROW_H;
        let max_scroll = (content_h - list.h).max(0.0);
        // 三行以内不消费滚轮，保留灵动岛既有的滚轮切页操作。
        if max_scroll > 0.0 {
            let wheel = ui.wheel_in(list);
            if wheel != 0.0 {
                st.bluetooth.scroll -= wheel * ROW_H;
                ui.request_repaint();
            }
        }
        st.bluetooth.scroll = st.bluetooth.scroll.clamp(0.0, max_scroll);
        ui.c.clip(list);
        for (index, device) in st.bluetooth.devices.iter().enumerate() {
            let row = Rect::new(list.x, list.y + index as f32 * ROW_H - st.bluetooth.scroll, list.w, ROW_H);
            if row.bottom() <= list.y || row.y >= list.bottom() {
                continue;
            }
            let icon = if device.kind == DeviceKind::Audio { Icon::Headphones } else { Icon::Bluetooth };
            ui.c.icon(icon, Rect::from_center(Point::new(row.x + 22.0, row.y + 28.0), 20.0, 20.0), palette::TEXT_DIM);
            let text_x = row.x + 44.0;
            let battery = Rect::new(row.right() - 100.0, row.y + 7.0, 88.0, 22.0);
            let name = Rect::new(text_x, row.y + 7.0, (battery.x - text_x - 12.0).max(0.0), 22.0);
            ui.c.clip(name);
            ui.c.text_raw(&device.name, name, &TextStyle::body().weight(Weight::Semibold));
            ui.c.unclip();
            if let Some(percent) = device.battery {
                let color = battery_color(percent);
                ui.c.text_raw(&format!("{percent}%"), battery, &TextStyle::body().weight(Weight::Semibold).tabular().right().color(color));
            } else if device.components.is_empty() {
                ui.c.text("电量未知", battery, &TextStyle::footnote().right().color(palette::TEXT_FAINT));
            }

            let detail = Rect::new(text_x, row.y + 31.0, row.right() - 12.0 - text_x, 18.0);
            if device.components.is_empty() {
                let kind = match device.kind {
                    DeviceKind::Audio => "音频设备",
                    DeviceKind::Keyboard => "键盘",
                    DeviceKind::Mouse => "鼠标",
                    DeviceKind::Gamepad => "游戏手柄",
                    DeviceKind::Other => "已连接",
                };
                ui.c.text(kind, detail, &TextStyle::footnote().color(palette::TEXT_DIM));
            } else {
                // 各部件独立绘制与着色；系统未给整机读数时，不从部件推算一个百分比。
                ui.c.clip(detail);
                let style = TextStyle::footnote().tabular();
                let mut x = detail.x;
                for (part_index, part) in device.components.iter().enumerate() {
                    if part_index > 0 {
                        let separator = " · ";
                        let width = ui.c.measure_raw(separator, &style, 400.0).0;
                        ui.c.text_raw(separator, Rect::new(x, detail.y, width + 1.0, detail.h), &style.color(palette::TEXT_DIM));
                        x += width;
                    }
                    let label = format!("{} {}%", part.label, part.percent);
                    let width = ui.c.measure_raw(&label, &style, 10_000.0).0;
                    ui.c.text_raw(&label, Rect::new(x, detail.y, width + 1.0, detail.h), &style.color(battery_color(part.percent)));
                    x += width;
                    if x >= detail.right() {
                        break;
                    }
                }
                ui.c.unclip();
            }
            if index + 1 < st.bluetooth.devices.len() {
                ui.c.line(Point::new(text_x, row.bottom()), Point::new(row.right() - 12.0, row.bottom()), palette::BORDER, 1.0);
            }
        }
        ui.c.unclip();
        if max_scroll > 0.0 {
            let bar_h = (list.h * list.h / content_h).max(18.0);
            let y = list.y + (list.h - bar_h) * st.bluetooth.scroll / max_scroll;
            ui.c.fill_rrect(Rect::new(list.right() - 5.0, y, 2.0, bar_h), 1.0, Color::white(0.22));
        }
    }

    let (note, warning) = if paused {
        (if st.power.saver { "关闭省电模式后会自动刷新" } else { "恢复使用后会自动刷新" }, false)
    } else if loading {
        (if has_snapshot { "电量由设备上报，可能存在延迟" } else { "请稍候" }, false)
    } else if failed && has_snapshot {
        (st.bluetooth.error.as_deref().unwrap_or_default(), false)
    } else if failed {
        ("", false)
    } else if let Some(warning) = st.bluetooth.warning.as_deref() {
        (warning, true)
    } else {
        ("部分设备不会提供电量", false)
    };
    let mut note_rect = Rect::new(area.x, area.y + 224.0, area.w, 32.0);
    let color = if warning { palette::ORANGE } else { palette::TEXT_FAINT };
    if warning {
        ui.c.icon(Icon::Warning, Rect::new(note_rect.x, note_rect.y + 1.0, 14.0, 14.0), color);
        note_rect.x += 20.0;
        note_rect.w -= 20.0;
    }
    ui.c.clip(note_rect);
    ui.c.text_raw(note, note_rect, &TextStyle::caption().wrap().top().line_spacing(1.25).color(color));
    ui.c.unclip();
}

fn battery_color(percent: u8) -> Color {
    if percent <= 10 {
        palette::RED
    } else if percent <= 20 {
        palette::ORANGE
    } else {
        palette::TEXT
    }
}
