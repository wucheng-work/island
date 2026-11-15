//! 中国农历（公历 1900—2100 年），使用预生成年月数据，不联网。

// 保留农历数据生成器的排版。
#[rustfmt::skip]
#[path = "lunar_data.rs"]
mod data;

pub const FIRST_SOLAR_YEAR: i32 = 1900;
pub const LAST_SOLAR_YEAR: i32 = 2100;
const MONTHS: [&str; 12] = [
    "正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "冬月", "腊月",
];
const DAYS: [&str; 30] = [
    "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十", "十一", "十二",
    "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十", "廿一", "廿二", "廿三", "廿四",
    "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
];

#[derive(Clone, Copy, Debug)]
pub struct LunarDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub leap: bool,
}

impl LunarDate {
    pub fn month_name(&self) -> String {
        format!(
            "{}{}",
            if self.leap { "闰" } else { "" },
            MONTHS[self.month as usize - 1]
        )
    }

    pub fn day_name(&self) -> &'static str {
        DAYS[self.day as usize - 1]
    }
}

/// 输入为公历民用日期自 1970-01-01 起的天数，不受设备时区影响。
pub fn from_day(day: i64) -> Option<LunarDate> {
    let index = data::YEARS
        .partition_point(|&(start, _, _)| i64::from(start) <= day)
        .checked_sub(1)?;
    if day >= data::END_DAY {
        return None;
    }
    let (start, long_months, leap_month) = data::YEARS[index];
    let mut remaining = day - i64::from(start);
    let months = if leap_month == 0 { 12 } else { 13 };
    for slot in 0..months {
        let length = 29 + i64::from((long_months >> slot) & 1);
        if remaining < length {
            let leap = leap_month > 0 && slot == leap_month;
            let month = if leap_month > 0 && slot >= leap_month {
                slot
            } else {
                slot + 1
            };
            return Some(LunarDate {
                year: data::FIRST_YEAR + index as i32,
                month,
                day: (remaining + 1) as u8,
                leap,
            });
        }
        remaining -= length;
    }
    None
}
