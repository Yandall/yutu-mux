//! Play-history date bucketing (UTC day math, std only, no chrono).
//!
//! The History page groups plays into Today / Yesterday / This week /
//! This month / older months. All day boundaries are UTC, matching the
//! log-pruner day math in [`crate::app_log`]: deterministic and
//! unit-testable without timezone data.

/// Whole days since the Unix epoch (UTC) for a timestamp.
#[must_use]
pub fn history_day(unix: i64) -> i64 {
    unix.div_euclid(86_400)
}

/// Monday-start week index for a day count (1970-01-01 was a Thursday,
/// so the +3 anchors weeks on Monday).
#[must_use]
pub fn history_week(day: i64) -> i64 {
    (day + 3).div_euclid(7)
}

/// Proleptic Gregorian (year, month, day) for days-since-epoch
/// (Hinnant's civil algorithm, std only — same math as the log
/// pruner in [`crate::app_log`]).
#[must_use]
pub fn history_ymd(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// English month name (1-based; unknown numbers render numbered).
#[must_use]
pub fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Month ?",
    }
}

/// `HH:MM` (UTC) for a timestamp, for today's entries.
#[must_use]
pub fn history_hhmm(unix: i64) -> String {
    let tod = unix.rem_euclid(86_400);
    format!("{:02}:{:02}", tod / 3_600, (tod % 3_600) / 60)
}

/// `YYYY-MM-DD` (UTC) for a timestamp, for older entries.
#[must_use]
pub fn history_date(unix: i64) -> String {
    let (y, m, d) = history_ymd(history_day(unix));
    format!("{y:04}-{m:02}-{d:02}")
}

/// One History page bucket. `Older` carries its calendar month so
/// distant plays group by month label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistorySection {
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
    Older { year: i64, month: u32 },
}

/// Bucket one play for `now_unix`: Today / Yesterday / the rest of
/// the current Monday-start week / the rest of the calendar month /
/// the entry's calendar month. Future timestamps (clock skew) read
/// as Today rather than vanishing.
#[must_use]
pub fn history_section(entry_unix: i64, now_unix: i64) -> HistorySection {
    if entry_unix > now_unix {
        return HistorySection::Today;
    }
    let day = history_day(entry_unix);
    let today = history_day(now_unix);
    match today - day {
        0 => HistorySection::Today,
        1 => HistorySection::Yesterday,
        _ if history_week(day) == history_week(today) => HistorySection::ThisWeek,
        _ => {
            let (y, m, _) = history_ymd(day);
            let (ty, tm, _) = history_ymd(today);
            if (y, m) == (ty, tm) {
                HistorySection::ThisMonth
            } else {
                HistorySection::Older { year: y, month: m }
            }
        }
    }
}

/// Display header for a section (`"February 2026"` for older months).
#[must_use]
pub fn history_section_label(section: &HistorySection) -> String {
    match *section {
        HistorySection::Today => "Today".to_string(),
        HistorySection::Yesterday => "Yesterday".to_string(),
        HistorySection::ThisWeek => "This week".to_string(),
        HistorySection::ThisMonth => "This month".to_string(),
        HistorySection::Older { year, month } => {
            format!("{} {year}", month_name(month))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-23 12:00 UTC (a Friday).
    const NOW: i64 = 1_792_756_800;

    fn days(n: i64) -> i64 {
        NOW - n * 86_400
    }

    #[test]
    fn day_math_matches_known_calendar() {
        // Sanity on the anchor math first.
        assert_eq!(history_ymd(history_day(NOW)), (2026, 10, 23));
        assert_eq!(history_hhmm(NOW), "12:00");
        assert_eq!(history_date(days(40)), "2026-09-13");
        assert_eq!(month_name(2), "February");
    }

    #[test]
    fn sections_bucket_by_recency() {
        use HistorySection as S;
        assert_eq!(history_section(NOW, NOW), S::Today);
        assert_eq!(history_section(NOW - 3_600, NOW), S::Today);
        assert_eq!(history_section(days(1), NOW), S::Yesterday);
        // Same Monday-start week (Mon 19th-Sun 25th), past yesterday.
        assert_eq!(history_section(days(2), NOW), S::ThisWeek);
        assert_eq!(history_section(days(4), NOW), S::ThisWeek);
        // Previous week, same calendar month.
        assert_eq!(history_section(days(7), NOW), S::ThisMonth);
        // September: older month group.
        assert_eq!(
            history_section(days(40), NOW),
            S::Older {
                year: 2026,
                month: 9
            }
        );
        // Future timestamps (clock skew) read as today.
        assert_eq!(history_section(NOW + 3_600, NOW), S::Today);
    }

    #[test]
    fn section_labels_read_naturally() {
        use HistorySection as S;
        assert_eq!(history_section_label(&S::Today), "Today");
        assert_eq!(history_section_label(&S::Yesterday), "Yesterday");
        assert_eq!(history_section_label(&S::ThisWeek), "This week");
        assert_eq!(history_section_label(&S::ThisMonth), "This month");
        assert_eq!(
            history_section_label(&S::Older {
                year: 2026,
                month: 2
            }),
            "February 2026"
        );
    }
}
