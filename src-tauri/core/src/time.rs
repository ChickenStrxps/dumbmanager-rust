//! UTC timestamps without a date/time dependency.
//!
//! All stored and returned dates use one fixed ISO 8601 shape,
//! `YYYY-MM-DDTHH:MM:SS.mmmZ` (the same as JavaScript's `toISOString`), so
//! they parse directly with `new Date(...)` and sort as plain strings.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch (negative if the clock is before 1970).
pub fn now_millis() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// Current time as an ISO 8601 UTC string.
pub fn now_iso() -> String {
    iso_from_millis(now_millis())
}

/// Formats Unix milliseconds as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn iso_from_millis(ms: i64) -> String {
    let (year, month, day, hour, minute, second, millis) = parts(ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Filesystem-safe compact stamp, `YYYYMMDD-HHMMSS-mmm` (UTC).
pub fn compact_stamp(ms: i64) -> String {
    let (year, month, day, hour, minute, second, millis) = parts(ms);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}-{millis:03}")
}

fn parts(ms: i64) -> (i64, u32, u32, i64, i64, i64, i64) {
    let seconds = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    (
        year,
        month,
        day,
        second_of_day / 3600,
        (second_of_day % 3600) / 60,
        second_of_day % 60,
        millis,
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097); // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // [0, 365]
    let month_index = (5 * day_of_year + 2) / 153; // [0, 11], March-based
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32; // [1, 31]
    let month = (if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    }) as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
