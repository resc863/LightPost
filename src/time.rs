//! Minimal date handling for the timestamps SQLite produces.
//!
//! The database stores `CURRENT_TIMESTAMP` values (`YYYY-MM-DD HH:MM:SS`, UTC).
//! Pages, RSS and the sitemap need them in the blog's local offset and in a few
//! fixed formats, which is all this module does.

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A wall-clock time at a fixed UTC offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Sunday.
    pub weekday: u32,
    pub offset_minutes: i32,
}

impl LocalTime {
    /// Converts a SQLite UTC timestamp to the given offset.
    pub fn from_sqlite(value: &str, offset_minutes: i32) -> Option<Self> {
        parse_sqlite_utc(value).map(|unix| Self::from_unix(unix, offset_minutes))
    }

    pub fn from_unix(unix: i64, offset_minutes: i32) -> Self {
        let local = unix + i64::from(offset_minutes) * 60;
        let days = local.div_euclid(86_400);
        let seconds = local.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (seconds / 3600) as u32,
            minute: (seconds % 3600 / 60) as u32,
            second: (seconds % 60) as u32,
            weekday: (days + 4).rem_euclid(7) as u32,
            offset_minutes,
        }
    }

    /// `2026.10.12`, used on pages.
    pub fn display_date(&self) -> String {
        format!("{:04}.{:02}.{:02}", self.year, self.month, self.day)
    }

    /// RFC 2822 (`Mon, 12 Oct 2026 09:30:00 +0900`), used by RSS.
    pub fn rfc2822(&self) -> String {
        format!(
            "{}, {:02} {} {:04} {:02}:{:02}:{:02} {}",
            WEEKDAYS[self.weekday as usize],
            self.day,
            MONTHS[self.month as usize - 1],
            self.year,
            self.hour,
            self.minute,
            self.second,
            self.offset(""),
        )
    }

    /// RFC 3339 (`2026-10-12T09:30:00+09:00`), used by the sitemap.
    pub fn rfc3339(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.offset(":"),
        )
    }

    fn offset(&self, separator: &str) -> String {
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let minutes = self.offset_minutes.unsigned_abs();
        format!("{sign}{:02}{separator}{:02}", minutes / 60, minutes % 60)
    }
}

/// Parses `YYYY-MM-DD HH:MM:SS` (or with a `T` separator) as UTC seconds since the epoch.
pub fn parse_sqlite_utc(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    if bytes[10] != b' ' && bytes[10] != b'T' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let text = value.get(range)?;
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// Parses `+09:00`, `-0530`, `+9`, `Z` or `UTC` into minutes east of UTC.
pub fn parse_utc_offset(value: &str) -> Option<i32> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("z") || value.eq_ignore_ascii_case("utc") {
        return Some(0);
    }
    let (sign, rest) = match value.as_bytes().first()? {
        b'+' => (1, &value[1..]),
        b'-' => (-1, &value[1..]),
        _ => return None,
    };
    let digits: String = rest.chars().filter(|character| *character != ':').collect();
    if digits.is_empty() || digits.len() > 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let (hours, minutes) = if digits.len() <= 2 {
        (digits.parse::<i32>().ok()?, 0)
    } else {
        let split = digits.len() - 2;
        (
            digits[..split].parse::<i32>().ok()?,
            digits[split..].parse::<i32>().ok()?,
        )
    };
    if hours > 14 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

// Howard Hinnant's civil calendar algorithms.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sqlite_timestamps() {
        assert_eq!(parse_sqlite_utc("1970-01-01 00:00:00"), Some(0));
        assert_eq!(parse_sqlite_utc("2026-10-12 00:30:00"), Some(1_791_765_000));
        assert_eq!(parse_sqlite_utc("2026-10-12T00:30:00"), Some(1_791_765_000));
        assert_eq!(parse_sqlite_utc("not a date"), None);
        assert_eq!(parse_sqlite_utc("2026-13-01 00:00:00"), None);
    }

    #[test]
    fn formats_feed_dates_in_local_offset() {
        let time = LocalTime::from_sqlite("2026-10-12 00:30:00", 9 * 60).unwrap();
        assert_eq!(time.rfc2822(), "Mon, 12 Oct 2026 09:30:00 +0900");
        assert_eq!(time.rfc3339(), "2026-10-12T09:30:00+09:00");
        let utc = LocalTime::from_sqlite("2026-10-12 00:30:00", 0).unwrap();
        assert_eq!(utc.rfc2822(), "Mon, 12 Oct 2026 00:30:00 +0000");
    }

    #[test]
    fn display_date_follows_local_day() {
        // 20:00 UTC on the 11th is already the 12th in Korea.
        let time = LocalTime::from_sqlite("2026-10-11 20:00:00", 9 * 60).unwrap();
        assert_eq!(time.display_date(), "2026.10.12");
        let behind = LocalTime::from_sqlite("2026-01-01 03:00:00", -5 * 60).unwrap();
        assert_eq!(behind.display_date(), "2025.12.31");
        assert_eq!(behind.rfc3339(), "2025-12-31T22:00:00-05:00");
    }

    #[test]
    fn round_trips_civil_dates() {
        for days in [-800_000, -1, 0, 1, 19_000, 20_738, 2_000_000] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
    }

    #[test]
    fn parses_utc_offsets() {
        assert_eq!(parse_utc_offset("+09:00"), Some(540));
        assert_eq!(parse_utc_offset("-0530"), Some(-330));
        assert_eq!(parse_utc_offset("+9"), Some(540));
        assert_eq!(parse_utc_offset("UTC"), Some(0));
        assert_eq!(parse_utc_offset("09:00"), None);
        assert_eq!(parse_utc_offset("+25:00"), None);
    }
}
