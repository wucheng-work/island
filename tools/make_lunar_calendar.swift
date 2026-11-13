// 用 macOS Foundation 的中国历生成年月数据；运行时不依赖 Foundation 或网络。
// 在仓库根目录运行：swift tools/make_lunar_calendar.swift
import Foundation

var solar = Calendar(identifier: .gregorian)
solar.timeZone = TimeZone(identifier: "Asia/Shanghai")!
var lunar = Calendar(identifier: .chinese)
lunar.timeZone = solar.timeZone
let start = solar.date(from: DateComponents(year: 1899, month: 1, day: 1, hour: 12))!
let end = solar.date(from: DateComponents(year: 2102, month: 1, day: 1, hour: 12))!
var date = start
var year = 0
var yearStart = 0
var monthStart = 0
var lengths: [Int] = []
var leap = 0
var records: [String] = []
var endDay = 0

while date < end {
    let part = lunar.dateComponents([.month, .day, .isLeapMonth], from: date)
    let day = Int(floor(date.timeIntervalSince1970 / 86_400))
    if part.day == 1 {
        if year > 0 { lengths.append(day - monthStart) }
        if part.month == 1 && part.isLeapMonth != true {
            if year > 0 {
                precondition(lengths.count == (leap == 0 ? 12 : 13))
                precondition(lengths.allSatisfy { $0 == 29 || $0 == 30 })
                var mask = 0
                for (index, length) in lengths.enumerated() where length == 30 { mask |= 1 << index }
                records.append(String(format: "    (%d, 0x%04x, %d), // %d", yearStart, mask, leap, year))
            }
            year = solar.component(.year, from: date)
            if year == 2101 { endDay = day; break }
            yearStart = day
            lengths = []
            leap = 0
        }
        if part.isLeapMonth == true { leap = part.month! }
        monthStart = day
    }
    date = solar.date(byAdding: .day, value: 1, to: date)!
}
precondition(records.count == 202 && endDay > 0)
let output = """
// 由 tools/make_lunar_calendar.swift 生成，请勿手工修改。
// Foundation 中国历，Asia/Shanghai；每项为正月初一天数、顺序月大月位、闰月。
pub(super) const FIRST_YEAR: i32 = 1899;
pub(super) const END_DAY: i64 = \(endDay);
pub(super) const YEARS: &[(i32, u16, u8)] = &[
\(records.joined(separator: "\n"))
];

"""
try output.write(toFile: "crates/poni-core/src/lunar_data.rs", atomically: true, encoding: .utf8)
print("已生成 \(records.count) 个农历年，覆盖公历 1900—2100 年")
