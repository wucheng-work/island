//! 完整月历、公历与农历，以及跟随所选日期的待办。

use crate::commands::Command;
use crate::gfx::Canvas;
use crate::gfx::{Image, TextStyle, Weight};
use crate::state::AppState;
use crate::ui::{wid, wid_n, PillStyle, Ui};
use poni_core::agenda::{self, Mark, Source, Sources, Weeks};
use poni_core::datetime::{
    civil_from_days, days_from_civil, days_in_month, month_grid, weekday_from_days,
};
use poni_core::design::radius;
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::lunar;

const GRID_W: f32 = 224.0;
const CALENDAR_PAD: f32 = 10.0;
const CELL_W: f32 = (GRID_W - 2.0 * CALENDAR_PAD) / 7.0;
const CELL_H: f32 = 30.0;
const GRID_Y: f32 = 80.0;
const INITIALS_H: f32 = 16.0;
// 保留六周完整月历，底部高度收至操作提示和图例，不额外预留空行。
const FOOTER_Y: f32 = GRID_Y + INITIALS_H + 6.0 * CELL_H + 6.0;
const LEGEND_Y: f32 = FOOTER_Y + 18.0;
const HEIGHT: f32 = LEGEND_Y + 16.0 + 8.0;
const COL_GAP: f32 = 20.0;
const TASKS_TOP: f32 = 28.0;
const WEATHER_H: f32 = 90.0;
const CITY_ROW_H: f32 = 26.0;
const LOGO: f32 = 14.0;
const GMAIL_RED: Color = Color::hex(0xEA4335);
const CALENDAR_BLUE: Color = Color::hex(0x4285F4);
const WEEKDAY_LONG: [&str; 7] = [
    "星期日",
    "星期一",
    "星期二",
    "星期三",
    "星期四",
    "星期五",
    "星期六",
];
const INITIALS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

pub fn height(st: &AppState) -> f32 {
    HEIGHT + weather_height(st) - WEATHER_H
}

pub fn next_wake(st: &AppState) -> Option<f64> {
    let mut wake = super::tasks::next_wake(st);
    if st.ui.weather_city_editing {
        if let Some(due) = st.ui.weather_city_search_due {
            wake = Some(wake.map_or(due, |time| time.min(due)));
        }
    }
    wake
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let today = days_from_civil(st.local.year, st.local.month, st.local.day);
    let day_key = st.day_key();
    let answer = st.oyenza.agenda.as_ref().filter(|a| a.day == day_key);
    let weeks = answer.map(|a| {
        let now_min = st.local.hour as i32 * 60 + st.local.minute as i32;
        agenda::weeks(
            a,
            &st.oyenza.bills,
            &st.oyenza.bills_today,
            &day_key,
            now_min,
            false,
            st.ui.today_pick,
        )
    });
    let days = weeks.as_ref().zip(answer).map(|(w, a)| (w, a.apps));
    let left = Rect::new(area.x, area.y, GRID_W, HEIGHT);
    draw_calendar(ui, st, left, today, days);

    let x = left.right() + COL_GAP;
    let right_w = (area.right() - x).max(0.0);
    let weather_h = weather_height(st);
    let tasks_offset = draw_weather(ui, st, Rect::new(x, area.y, right_w, weather_h)) + 8.0;
    let selected = st.ui.today_pick.unwrap_or(today);
    let (year, month, date) = civil_from_days(selected);
    let title = format!("{year}年{month}月{date}日待办");
    ui.c.text(
        &title,
        Rect::new(x, area.y + tasks_offset, right_w, 20.0),
        &TextStyle::headline(),
    );
    super::tasks::draw(
        ui,
        st,
        Rect::new(
            x,
            area.y + tasks_offset + TASKS_TOP,
            right_w,
            area.h - tasks_offset - TASKS_TOP,
        ),
    );
}

fn weather_height(st: &AppState) -> f32 {
    if st.ui.weather_city_editing {
        64.0 + CITY_ROW_H * st.weather.city_candidates.len().max(1) as f32
    } else {
        WEATHER_H
    }
}

fn draw_weather(ui: &mut Ui, st: &mut AppState, area: Rect) -> f32 {
    let sky = palette::BLUE;
    ui.c.fill_rrect(area, radius::PANEL, Color::white(0.035));

    if st.ui.weather_city_editing {
        // 输入框止于右侧操作按钮之前，两个控件之间保留8 DIP。
        let input = Rect::new(area.x + 8.0, area.y + 8.0, area.w - 82.0, 30.0);
        let response = ui.text_input(
            wid("weather.city.input"),
            input,
            &mut st.ui.weather_city_input,
            "输入城市名称",
            80,
        );
        if response.changed {
            st.weather.city_search_id = st.weather.city_search_id.wrapping_add(1);
            st.weather.city_candidates.clear();
            st.weather.city_search_loading = false;
            st.weather.city_search_message.clear();
            st.ui.weather_city_search_due =
                (st.ui.weather_city_input.trim().chars().count() >= 2).then_some(st.now + 0.35);
        }
        if response.cancelled {
            st.ui.weather_city_editing = false;
            st.ui.weather_city_search_due = None;
            st.weather.city_search_id = st.weather.city_search_id.wrapping_add(1);
            st.weather.city_candidates.clear();
            ui.blur();
        } else {
            let mut chosen = None;
            if response.submitted {
                if st.weather.city_candidates.is_empty() {
                    st.ui.weather_city_search_due = Some(st.now);
                } else {
                    chosen = st.weather.city_candidates.first().cloned();
                }
            }
            if st
                .ui
                .weather_city_search_due
                .is_some_and(|due| due <= st.now)
            {
                st.ui.weather_city_search_due = None;
                let query = st.ui.weather_city_input.trim();
                if query.chars().count() >= 2 {
                    st.weather.city_search_loading = true;
                    st.cmd(Command::SearchWeatherCities {
                        query: query.to_string(),
                        search_id: st.weather.city_search_id,
                    });
                } else {
                    st.weather.city_search_message = "请至少输入两个字".to_string();
                }
            }
            if st.weather.city_candidates.is_empty() {
                let message = if st.weather.city_search_loading {
                    "正在搜索城市…"
                } else if st.weather.city_search_message.is_empty() {
                    "输入城市名称，选择下方候选地点"
                } else {
                    &st.weather.city_search_message
                };
                ui.c.text(
                    message,
                    Rect::new(area.x + 10.0, area.y + 43.0, area.w - 20.0, CITY_ROW_H),
                    &TextStyle::footnote().color(palette::TEXT_DIM),
                );
            } else {
                for (index, city) in st.weather.city_candidates.iter().enumerate() {
                    let row = Rect::new(
                        area.x + 8.0,
                        area.y + 41.0 + CITY_ROW_H * index as f32,
                        area.w - 16.0,
                        CITY_ROW_H,
                    );
                    let hit = ui.button(wid_n("weather.city.choice", index as u64), row);
                    if hit.hovered {
                        ui.c.fill_rrect(row, 7.0, palette::SURFACE_HOVER);
                    }
                    ui.c.text(
                        &city.label(),
                        row.inset(8.0, 0.0),
                        &TextStyle::footnote().color(palette::TEXT),
                    );
                    if hit.clicked {
                        chosen = Some(city.clone());
                    }
                }
            }
            if let Some(city) = chosen {
                st.cmd(Command::SetWeatherCity {
                    city: city.label(),
                    coordinates: (city.latitude, city.longitude),
                });
                st.ui.weather_city_editing = false;
                st.ui.weather_city_search_due = None;
                st.weather.city_search_id = st.weather.city_search_id.wrapping_add(1);
                st.weather.city_candidates.clear();
                ui.blur();
            }
        }
    } else {
        let retry = !st.weather.loading
            && !st.weather.message.is_empty()
            && !st.settings.weather_city.is_empty();
        let city_area = Rect::new(
            area.x + 14.0,
            area.y + 8.0,
            area.w - if retry { 146.0 } else { 90.0 },
            23.0,
        );
        if retry
            && ui
                .pill(
                    wid("weather.retry"),
                    Rect::new(area.right() - 118.0, area.y + 9.0, 46.0, 23.0),
                    "重试",
                    None,
                    PillStyle::UtilityAccent(sky),
                )
                .clicked
        {
            st.cmd(Command::RefreshWeather);
        }
        let city = ui.button(wid("weather.city"), city_area);
        if city.clicked {
            // 重新打开时输入会恢复已保存城市，旧查询与候选不能沿用。
            st.weather.city_search_id = st.weather.city_search_id.wrapping_add(1);
            st.weather.city_candidates.clear();
            st.weather.city_search_loading = false;
            st.weather.city_search_message.clear();
            st.ui.weather_city_search_due = None;
            st.ui.weather_city_input = st
                .settings
                .weather_city
                .split(" · ")
                .next()
                .unwrap_or("")
                .to_string();
            st.ui.weather_city_editing = true;
        }
        if let Some(snapshot) = st
            .weather
            .snapshot
            .as_ref()
            .filter(|_| st.weather.message.is_empty())
        {
            ui.c.text(
                &snapshot.place,
                city_area,
                &TextStyle::footnote()
                    .weight(Weight::Semibold)
                    .color(palette::TEXT_DIM),
            );
            let compact = area.w < 260.0;
            let temperature = Rect::new(area.x + 14.0, area.y + 29.0, 104.0, 38.0);
            ui.c.text(
                &format!("{:.0}°C", snapshot.temperature_c),
                temperature,
                &TextStyle::new(32.0).weight(Weight::Semibold),
            );
            let condition = if compact {
                Rect::new(area.x + 16.0, area.y + 66.0, area.w - 76.0, 17.0)
            } else {
                Rect::new(area.x + 122.0, area.y + 43.0, area.w - 184.0, 23.0)
            };
            ui.c.text(
                weather_name(snapshot.code, snapshot.is_day),
                condition,
                &TextStyle::footnote().weight(Weight::Semibold).color(sky),
            );
            let icon_center = Point::new(area.right() - 33.0, area.y + 58.0);
            draw_weather_icon(ui.c, icon_center, snapshot.code, snapshot.is_day);
        } else {
            let detail = if st.weather.loading {
                "正在获取天气…"
            } else if st.weather.message.is_empty() {
                "手选城市后自动保存，也可点击定位"
            } else {
                &st.weather.message
            };
            let title = if st.settings.weather_city.is_empty() {
                "选择城市 ›"
            } else {
                &st.settings.weather_city
            };
            ui.c.text(
                title,
                city_area,
                &TextStyle::footnote().weight(Weight::Semibold).color(sky),
            );
            ui.c.text(
                detail,
                Rect::new(area.x + 14.0, area.y + 42.0, area.w - 28.0, 30.0),
                &TextStyle::footnote().color(palette::TEXT_DIM).wrap(),
            );
        }
    }

    let action = if st.ui.weather_city_editing {
        "取消"
    } else {
        "定位"
    };
    if ui
        .pill(
            wid("weather.location"),
            Rect::new(area.right() - 66.0, area.y + 9.0, 54.0, 23.0),
            action,
            None,
            PillStyle::UtilityAccent(sky),
        )
        .clicked
    {
        if st.ui.weather_city_editing {
            st.ui.weather_city_editing = false;
            st.ui.weather_city_search_due = None;
            st.weather.city_search_id = st.weather.city_search_id.wrapping_add(1);
            st.weather.city_candidates.clear();
            ui.blur();
        } else {
            st.cmd(Command::LocateWeather);
        }
    }
    area.h
}

fn weather_name(code: u8, is_day: bool) -> &'static str {
    match code {
        0 if !is_day => "晴夜",
        0 => "晴",
        1 | 2 => "多云",
        3 => "阴",
        45 | 48 => "雾",
        51..=57 => "毛毛雨",
        61..=67 | 80..=82 => "雨",
        71..=77 | 85 | 86 => "雪",
        95..=99 => "雷雨",
        _ => "天气变化",
    }
}

fn draw_weather_icon(c: &mut Canvas<'_>, center: Point, code: u8, is_day: bool) {
    // 天气用静态符号表达状态，避免仅为装饰让整个日历每秒重绘四次。
    let phase = 0.4_f32;
    let yellow = Color::hex(0xFFD66B);
    let cloud = Color::hex(0xB7CDE1);
    let blue = Color::hex(0x65BFFF);

    if code == 0 && !is_day {
        use poni_core::svgpath::Seg;
        c.path(
            &[
                Seg::MoveTo(center.x + 3.0, center.y - 12.0),
                Seg::CubicTo(
                    center.x - 17.0,
                    center.y - 12.0,
                    center.x - 17.0,
                    center.y + 12.0,
                    center.x + 3.0,
                    center.y + 12.0,
                ),
                Seg::CubicTo(
                    center.x - 7.0,
                    center.y + 5.0,
                    center.x - 7.0,
                    center.y - 5.0,
                    center.x + 3.0,
                    center.y - 12.0,
                ),
                Seg::Close,
            ],
            Some(yellow),
            None,
        );
        let glow = 0.6 + 0.4 * phase.sin().abs();
        c.fill_circle(
            Point::new(center.x + 13.0, center.y - 10.0),
            2.0,
            yellow.with_alpha(glow),
        );
        c.fill_circle(
            Point::new(center.x + 16.0, center.y + 5.0),
            1.3,
            yellow.with_alpha(1.0 - glow * 0.4),
        );
        return;
    }

    if code <= 2 && is_day {
        let sun = if code == 0 {
            center
        } else {
            Point::new(center.x - 8.0, center.y - 7.0)
        };
        c.fill_circle(sun, 8.5, yellow);
        for ray in 0..8 {
            let angle = phase + ray as f32 * std::f32::consts::TAU / 8.0;
            let direction = Point::new(angle.cos(), angle.sin());
            c.line(
                Point::new(sun.x + direction.x * 12.0, sun.y + direction.y * 12.0),
                Point::new(sun.x + direction.x * 16.0, sun.y + direction.y * 16.0),
                yellow,
                1.8,
            );
        }
        if code == 0 {
            return;
        }
    }

    let drift = phase.sin() * 2.0;
    let cloud_center = Point::new(center.x + 2.0 + drift, center.y - 3.0);
    c.fill_circle(Point::new(cloud_center.x - 8.0, cloud_center.y), 7.0, cloud);
    c.fill_circle(Point::new(cloud_center.x, cloud_center.y - 4.0), 9.0, cloud);
    c.fill_circle(Point::new(cloud_center.x + 9.0, cloud_center.y), 6.0, cloud);
    c.fill_rrect(
        Rect::new(cloud_center.x - 15.0, cloud_center.y, 30.0, 8.0),
        4.0,
        cloud,
    );

    match code {
        45 | 48 => {
            for row in 0..2 {
                let y = cloud_center.y + 12.0 + row as f32 * 5.0;
                c.line(
                    Point::new(center.x - 12.0 + drift, y),
                    Point::new(center.x + 12.0 + drift, y),
                    blue.with_alpha(0.8),
                    1.6,
                );
            }
        }
        51..=67 | 80..=82 => {
            let fall = 3.0_f32;
            for drop in 0..3 {
                let x = center.x - 9.0 + drop as f32 * 9.0;
                let y = cloud_center.y + 8.0 + (fall + drop as f32 * 2.5) % 8.0;
                c.line(Point::new(x, y), Point::new(x - 2.0, y + 4.0), blue, 2.0);
            }
        }
        71..=77 | 85 | 86 => {
            let fall = 3.0_f32;
            for flake in 0..3 {
                let x = center.x - 9.0 + flake as f32 * 9.0;
                let y = cloud_center.y + 10.0 + (fall + flake as f32 * 2.5) % 8.0;
                c.fill_circle(Point::new(x, y), 1.8, blue);
            }
        }
        95..=99 => {
            let flash = 0.7 + 0.3 * phase.sin().abs();
            c.line(
                Point::new(center.x + 2.0, cloud_center.y + 7.0),
                Point::new(center.x - 3.0, cloud_center.y + 16.0),
                yellow.with_alpha(flash),
                3.0,
            );
            c.line(
                Point::new(center.x - 3.0, cloud_center.y + 16.0),
                Point::new(center.x + 3.0, cloud_center.y + 15.0),
                yellow.with_alpha(flash),
                3.0,
            );
            c.line(
                Point::new(center.x + 3.0, cloud_center.y + 15.0),
                Point::new(center.x - 3.0, cloud_center.y + 24.0),
                yellow.with_alpha(flash),
                3.0,
            );
        }
        _ => {}
    }
}

/// 切月时保留日号；目标月较短时落到月底。唯一日期状态同时供日历和待办读取。
fn select_month(st: &mut AppState, year: i32, month: u8, today: i64) {
    let (_, _, date) = civil_from_days(st.ui.today_pick.unwrap_or(today));
    let selected = days_from_civil(year, month, date.min(days_in_month(year, month)));
    st.ui.today_month = Some((year, month));
    st.ui.today_pick = (selected != today).then_some(selected);
}

fn draw_calendar(
    ui: &mut Ui,
    st: &mut AppState,
    r: Rect,
    today: i64,
    days: Option<(&Weeks, Sources)>,
) {
    // 借鉴迷你月历的独立卡片、左对齐月份标题与轻量色标，保留农历双行日期。
    ui.c.fill_rrect(r, radius::PANEL, Color::white(0.025));
    let r = Rect::new(r.x + CALENDAR_PAD, r.y, r.w - 2.0 * CALENDAR_PAD, r.h);
    let selected = st.ui.today_pick.unwrap_or(today);
    let (selected_year, selected_month, _) = civil_from_days(selected);
    let (mut year, mut month) = st.ui.today_month.unwrap_or((selected_year, selected_month));
    let picker = st.ui.today_month_picker;
    let mut step = 0;
    for (index, direction, label) in [(0, -1, "‹"), (1, 1, "›")] {
        let enabled = if direction < 0 {
            year > lunar::FIRST_SOLAR_YEAR || (!picker && month > 1)
        } else {
            year < lunar::LAST_SOLAR_YEAR || (!picker && month < 12)
        };
        let x = if direction < 0 {
            r.right() - 54.0
        } else {
            r.right() - 26.0
        };
        let button = Rect::new(x, r.y + 8.0, 26.0, 26.0);
        let mut color = palette::TEXT_FAINT;
        if enabled {
            let response = ui.button(wid_n("today.month.step", index), button);
            color = palette::TEXT_DIM;
            ui.c.fill_rrect(button, 8.0, Color::white(0.035));
            if response.hovered || response.pressed {
                ui.c.fill_rrect(button, 7.0, palette::SURFACE_HOVER);
                color = palette::TEXT;
            }
            if response.clicked {
                step = direction;
            }
        }
        ui.c.text(label, button, &TextStyle::new(23.0).center().color(color));
    }
    if step != 0 {
        if picker {
            year += step;
        } else {
            let serial = year * 12 + i32::from(month) - 1 + step;
            year = serial.div_euclid(12);
            month = (serial.rem_euclid(12) + 1) as u8;
        }
        select_month(st, year, month, today);
        ui.request_anim();
    }
    let heading = if picker {
        format!("{year}年")
    } else {
        format!("{year}年{month}月")
    };
    let title_r = Rect::new(r.x, r.y + 7.0, r.w - 60.0, 28.0);
    let response = ui.button(wid("today.month.title"), title_r);
    if response.hovered {
        ui.c.fill_rrect(title_r, 7.0, palette::SURFACE_HOVER);
    }
    ui.c.text(
        &heading,
        title_r,
        &TextStyle::new(17.0).weight(Weight::Bold),
    );
    if response.clicked {
        st.ui.today_month_picker = !picker;
        ui.request_anim();
    }
    let summary = Rect::new(r.x, r.y + 41.0, r.w, 33.0);
    ui.c.fill_rrect(summary, radius::CONTROL, palette::SURFACE);
    if ui
        .pill(
            wid("today.return"),
            Rect::new(r.right() - 44.0, r.y + 46.0, 44.0, 22.0),
            "今天",
            None,
            PillStyle::UtilityAccent(st.accent),
        )
        .clicked
    {
        st.ui.today_pick = None;
        st.ui.today_month = None;
        st.ui.today_month_picker = false;
        ui.request_anim();
    }

    let selected = st.ui.today_pick.unwrap_or(today);
    let (_, selected_month, selected_date) = civil_from_days(selected);
    let weekday = WEEKDAY_LONG[weekday_from_days(selected) as usize];
    let label = format!("{selected_month}月{selected_date}日 · {weekday}");
    ui.c.text(
        &label,
        Rect::new(r.x + 6.0, r.y + 42.0, r.w - 54.0, 16.0),
        &TextStyle::caption()
            .weight(Weight::Semibold)
            .color(palette::TEXT),
    );
    if let Some(date) = lunar::from_day(selected) {
        let label = format!("农历{}{}", date.month_name(), date.day_name());
        ui.c.text(
            &label,
            Rect::new(r.x + 6.0, r.y + 58.0, r.w - 54.0, 14.0),
            &TextStyle::caption().color(st.accent),
        );
    }
    if let Some((_, apps)) = days {
        let mut x = r.right() - LOGO;
        for (mark, source) in [
            (Mark::Gmail, apps.gmail),
            (Mark::Calendar, apps.googlecalendar),
        ] {
            if let Some(image) = logo(mark) {
                ui.c.with_alpha(if source == Source::Off { 0.35 } else { 1.0 }, |c| {
                    c.image(&image, Rect::new(x, r.y + LEGEND_Y, LOGO, LOGO), 3.5)
                });
            }
            x -= LOGO + 5.0;
        }
    }

    if st.ui.today_month_picker {
        for index in 0..12 {
            let value = index + 1;
            let cell = Rect::new(
                r.x + (index % 3) as f32 * (r.w / 3.0),
                r.y + 88.0 + (index / 3) as f32 * 46.0,
                r.w / 3.0 - 4.0,
                38.0,
            );
            let response = ui.button(wid_n("today.month.pick", value), cell);
            ui.c.fill_rrect(cell, 9.0, Color::white(0.035));
            if value == u64::from(month) {
                ui.c.fill_rrect(cell, 9.0, st.accent.with_alpha(0.18));
            } else if response.hovered {
                ui.c.fill_rrect(cell, 9.0, palette::SURFACE_HOVER);
            }
            let color = if value == u64::from(month) {
                st.accent
            } else {
                palette::TEXT_DIM
            };
            ui.c.text(
                &format!("{value}月"),
                cell,
                &TextStyle::body()
                    .center()
                    .weight(Weight::Semibold)
                    .color(color),
            );
            if response.clicked {
                select_month(st, year, value as u8, today);
                st.ui.today_month_picker = false;
                ui.request_anim();
            }
        }
        ui.c.text(
            "左右切换年份，点击月份查看",
            Rect::new(r.x, r.y + FOOTER_Y, r.w, 18.0),
            &TextStyle::caption().color(palette::TEXT_FAINT),
        );
        return;
    }

    let initials_st = TextStyle::caption()
        .weight(Weight::Semibold)
        .color(palette::TEXT_FAINT)
        .center();
    ui.c.fill_rrect(
        Rect::new(
            r.x + 5.0 * CELL_W,
            r.y + GRID_Y,
            2.0 * CELL_W,
            INITIALS_H + 6.0 * CELL_H,
        ),
        8.0,
        Color::white(0.025),
    );
    for (column, initial) in INITIALS.iter().enumerate() {
        ui.c.text(
            initial,
            Rect::new(
                r.x + column as f32 * CELL_W,
                r.y + GRID_Y,
                CELL_W,
                INITIALS_H,
            ),
            &initials_st,
        );
    }
    // 日期标记与右侧列表共享可见性规则，今天的旧待办结转也由领域层处理。
    let first_day = days_from_civil(year, month, 1);
    let task_days =
        st.store
            .tasks
            .day_marks(first_day, usize::from(days_in_month(year, month)), today);
    let top = r.y + GRID_Y + INITIALS_H;
    for (row, dates) in month_grid(year, month).iter().enumerate() {
        for (column, date) in dates.iter().enumerate() {
            let Some(date) = date else { continue };
            let day = days_from_civil(year, month, *date);
            let cell = Rect::new(
                r.x + column as f32 * CELL_W,
                top + row as f32 * CELL_H,
                CELL_W,
                CELL_H,
            );
            let response = ui.button(wid_n("today.date", day as u64), cell);
            let poni_core::todo::DayTasks {
                has_tasks,
                has_open,
            } = task_days[usize::from(*date - 1)];
            let task_color = if has_open {
                palette::ORANGE
            } else {
                palette::GREEN
            };
            let tile = cell.inset(1.0, 0.5);
            if day == selected {
                ui.c.fill_rrect(tile, 8.0, st.accent.with_alpha(0.15));
                if day != today {
                    ui.c.stroke_rrect(tile, 8.0, st.accent.with_alpha(0.45), 1.0);
                }
            } else if response.hovered {
                ui.c.fill_rrect(tile, 8.0, palette::SURFACE_HOVER);
            } else if has_tasks {
                ui.c.fill_rrect(tile, 8.0, task_color.with_alpha(0.085));
            }
            if day == today {
                let badge = Point::new(cell.center().x, cell.y + 9.0);
                if day == selected {
                    ui.c.fill_circle(badge, 10.0, st.accent);
                } else {
                    ui.c.stroke_rrect(
                        Rect::from_center(badge, 21.0, 21.0),
                        10.5,
                        st.accent.with_alpha(0.6),
                        1.0,
                    );
                }
            }
            let number = Rect::new(cell.x, cell.y, cell.w, 18.0);
            let color = if day == today && day == selected {
                palette::ON_ACCENT
            } else if day == today {
                st.accent
            } else if has_tasks && day != selected {
                task_color
            } else if column >= 5 {
                palette::TEXT_DIM
            } else {
                palette::TEXT
            };
            ui.c.text(
                &date.to_string(),
                number,
                &TextStyle::footnote()
                    .tabular()
                    .center()
                    .weight(if day == today {
                        Weight::Bold
                    } else {
                        Weight::Regular
                    })
                    .color(color),
            );
            if let Some(date) = lunar::from_day(day) {
                let label = if date.day == 1 {
                    date.month_name()
                } else {
                    date.day_name().to_string()
                };
                let color = if day == selected {
                    st.accent
                } else {
                    palette::TEXT_FAINT
                };
                ui.c.text(
                    &label,
                    Rect::new(cell.x, cell.y + 18.0, cell.w, 11.0),
                    &TextStyle::new(9.0).center().color(color),
                );
            }
            if has_tasks {
                ui.c.fill_circle(
                    Point::new(cell.right() - 3.5, cell.y + 3.5),
                    1.8,
                    task_color,
                );
            }
            if let Some(kind) = days.and_then(|(weeks, _)| weeks.kinds.get(&day)) {
                let dots: Vec<Color> = [
                    (kind.calendar, CALENDAR_BLUE),
                    (kind.flight, GMAIL_RED),
                    (kind.bill, palette::ORANGE),
                ]
                .into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, color)| color)
                .collect();
                for (index, color) in dots.iter().enumerate() {
                    let x = cell.center().x - (dots.len() as f32 - 1.0) * 2.5 + index as f32 * 5.0;
                    ui.c.fill_circle(Point::new(x, cell.y + 28.5), 1.1, *color);
                }
            }
            if response.clicked {
                st.ui.today_pick = (day != today).then_some(day);
                ui.request_anim();
            }
        }
    }
    let footer = if let Some((weeks, _)) =
        days.filter(|(weeks, _)| selected >= weeks.today && selected <= weeks.last)
    {
        let count = weeks
            .days
            .iter()
            .find(|date| date.day == selected)
            .map_or(0, |date| date.rows.len());
        format!("已同步 {count} 项日程 · 点击日期查看待办")
    } else {
        "点击年月切换，点击日期查看待办".to_string()
    };
    ui.c.line(
        Point::new(r.x, r.y + FOOTER_Y - 3.0),
        Point::new(r.right(), r.y + FOOTER_Y - 3.0),
        Color::white(0.055),
        1.0,
    );
    ui.c.text(
        &footer,
        Rect::new(r.x, r.y + FOOTER_Y, r.w, 16.0),
        &TextStyle::new(10.0).color(palette::TEXT_FAINT),
    );
    for (x, label, color) in [
        (r.x + 4.0, "有待办", palette::ORANGE),
        (r.x + 78.0, "已完成", palette::GREEN),
    ] {
        ui.c.fill_circle(Point::new(x, r.y + LEGEND_Y + 8.0), 2.2, color);
        ui.c.text(
            label,
            Rect::new(x + 7.0, r.y + LEGEND_Y, 54.0, 16.0),
            &TextStyle::new(10.0).color(palette::TEXT_DIM),
        );
    }
}

const CALENDAR_PNG: &[u8] = include_bytes!("../../res/apps/googlecalendar.png");
const GMAIL_PNG: &[u8] = include_bytes!("../../res/apps/gmail.png");

thread_local! {
    static LOGOS: std::cell::RefCell<Option<[Option<Image>; 2]>> = const { std::cell::RefCell::new(None) };
}

fn logo(mark: Mark) -> Option<Image> {
    let index = match mark {
        Mark::Calendar => 0,
        Mark::Gmail => 1,
        Mark::Bill => return None,
    };
    LOGOS.with(|logos| {
        logos
            .borrow_mut()
            .get_or_insert_with(|| [decode(CALENDAR_PNG), decode(GMAIL_PNG)])[index]
            .clone()
    })
}

#[cfg(windows)]
fn decode(png: &[u8]) -> Option<Image> {
    crate::services::imaging::decode_bytes(png, 64).ok()
}

#[cfg(target_os = "macos")]
fn decode(png: &[u8]) -> Option<Image> {
    crate::services::thumbs::decode(png, 64)
}
