//! EXIF 拍摄日期提取（best-effort，失败返回 None 由调用方回退文件 mtime）。

use std::path::Path;

use exif::{In, Reader, Tag, Value};

/// 解析 EXIF DateTimeOriginal（格式 `YYYY:MM:DD HH:MM:SS`）为 Unix 秒。
/// 支持 JPEG/TIFF；PNG/WebP 等若无 EXIF 返回 None。
pub fn exif_datetime(path: &Path) -> Option<i64> {
    let data = std::fs::read(path).ok()?;
    let reader = Reader::new();
    let exif = reader.read_raw(data).ok()?;
    let entry = exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)?;
    let Value::Ascii(parts) = &entry.value else {
        return None;
    };
    // ASCII 值可能分段（Vec<Vec<u8>>），拼接后解析
    let bytes: Vec<u8> = parts.iter().flat_map(|p| p.iter()).copied().collect();
    let s = String::from_utf8(bytes).ok()?;
    parse_datetime(&s)
}

/// 解析 `YYYY:MM:DD HH:MM:SS` 为 Unix 秒（忽略时区，视为本地时间）。
/// 手写实现，避免引入 chrono 依赖。
fn parse_datetime(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = s.split_once(' ')?;
    let mut dp = date.split(':');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let mut tp = time.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next()?.parse().ok()?;
    let second: i64 = tp.next()?.parse().ok()?;
    civil_to_unix(year, month, day, hour, minute, second)
}

/// 公历日期（proleptic Gregorian）转 Unix 秒。
/// 使用 Howard Hinnant 的 civil_from_days 逆算法（days_from_civil）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

fn civil_to_unix(y: i64, m: i64, d: i64, hh: i64, mm: i64, ss: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(0..=23).contains(&hh) || !(0..=59).contains(&mm) || !(0..=60).contains(&ss) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_datetime() {
        let ts = parse_datetime("2024:06:12 10:30:00").unwrap();
        // 2024-06-12 是 UTC epoch 下的具体值，用已知锚点校验
        // 2024-01-01 00:00:00 UTC = 1704067200
        let jan1 = civil_to_unix(2024, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(jan1, 1704067200);
        // 6月12日 = 1月1日 + 163 天
        let jun12 = civil_to_unix(2024, 6, 12, 10, 30, 0).unwrap();
        assert_eq!(jun12, jan1 + 163 * 86400 + 10 * 3600 + 30 * 60);
        assert!(ts > 0);
    }

    #[test]
    fn invalid_inputs_return_none() {
        assert!(parse_datetime("").is_none());
        assert!(parse_datetime("2024-06-12").is_none());
        assert!(parse_datetime("bad:06:12 10:30:00").is_none());
        assert!(civil_to_unix(2024, 13, 1, 0, 0, 0).is_none());
    }
}
