//! Append-only file logging with 7-day retention.
//!
//! Everything the app reports (stream resolves, retries, skips, sync,
//! errors) funnels through [`log`], which mirrors to stderr and, once
//! [`init`] ran at startup, to a daily file under `<data>/logs/` named
//! `yutu-mux-YYYY-MM-DD.log`. Files older than [`LOG_KEEP_DAYS`] are
//! deleted on init, so a bug report is always "send the last few files
//! in the logs dir", never a stale megabyte archive.
//!
//! Rules: std only (this module compiles under every feature set,
//! including `--no-default-features`); lock-poison tolerant like the
//! rest of the codebase; every write is flushed (logs must survive a
//! crash, volume is lines-per-minute, not per-frame); day rollover
//! reopens the file on the next write; uninitialized use (tests) is
//! stderr-only.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// Daily log files older than this are deleted on [`init`].
pub const LOG_KEEP_DAYS: u64 = 7;
/// Log file shape: `yutu-mux-2026-10-19.log`.
const LOG_PREFIX: &str = "yutu-mux-";
const LOG_SUFFIX: &str = ".log";

/// Log severity (part of every line, for grep).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// The open log file plus the date it belongs to (rollover compares
/// on every write; `file` is `None` before [`init`] or when the dir is
/// unusable, in which case lines are stderr-only).
struct Sink {
    dir: String,
    date: String,
    file: Option<fs::File>,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

fn sink() -> &'static Mutex<Sink> {
    SINK.get_or_init(|| {
        Mutex::new(Sink {
            dir: String::new(),
            date: String::new(),
            file: None,
        })
    })
}

/// Initialize file logging into `dir` (created if missing): opens
/// today's file for append and deletes files older than
/// [`LOG_KEEP_DAYS`]. Call once at startup; later calls re-target
/// (handy for tests). Always ends with a marker line, so a missing
/// marker means the app died before logging started.
pub fn init(dir: &str) {
    let _ = fs::create_dir_all(dir);
    prune_old_logs_in(dir, LOG_KEEP_DAYS);
    let date = today_days().map(date_string).unwrap_or_default();
    let file = if date.is_empty() {
        None
    } else {
        open_log(dir, &date)
    };
    {
        let mut guard = sink().lock().unwrap_or_else(|poison| poison.into_inner());
        guard.dir = dir.to_string();
        if file.is_some() {
            guard.date = date;
        }
        guard.file = file;
    }
    info(
        "yutu-log",
        format!("file logging ready ({dir}, keeping {LOG_KEEP_DAYS}d)"),
    );
}

/// Write one line to stderr and, when initialized, to the log file.
/// `module` is the existing area tag (`yutu-play`, `yutu-stream`,
/// `yutu-import`, ...), kept stable so old notes still grep.
pub fn log(level: Level, module: &str, msg: &str) {
    let line = format_line(level, module, msg);
    eprintln!("{line}");
    append_blocking(&line);
}

/// Panic-hook entry: like [`error`] but never blocks on the sink lock
/// (a panic may fire while another thread holds it; blocking there
/// would hang the crash report). Under contention the line is
/// stderr-only; the next regular write heals any missed rollover.
pub fn log_panic(module: &str, msg: &str) {
    let line = format_line(Level::Error, module, msg);
    eprintln!("{line}");
    if let Ok(mut guard) = sink().try_lock() {
        if let Some(file) = guard.file.as_mut() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

fn format_line(level: Level, module: &str, msg: &str) -> String {
    format!(
        "[{}] [{}] [{module}] {msg}",
        timestamp_now(),
        level.as_str()
    )
}

/// Append one formatted line to the open file (no-op when
/// uninitialized): lazy-opens on first use (heals a failed [`init`]
/// or a clock that recovered since), reopens on day rollover.
fn append_blocking(line: &str) {
    let mut guard = sink().lock().unwrap_or_else(|poison| poison.into_inner());
    let today = today_days().map(date_string);
    let stale = guard.file.is_none() || today.as_deref().is_some_and(|t| *t != guard.date);
    if stale {
        if let (Some(today), false) = (today, guard.dir.is_empty()) {
            let dir = guard.dir.clone();
            // One file-create per day (plus retries while the dir is
            // unusable): log volume is failures-only, never per-frame.
            guard.file = open_log(&dir, &today);
            if guard.file.is_some() {
                guard.date = today;
            }
        }
    }
    if let Some(file) = guard.file.as_mut() {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

/// [`log`] at Info level.
pub fn info(module: &str, msg: impl std::fmt::Display) {
    log(Level::Info, module, &msg.to_string());
}

/// [`log`] at Warn level (degraded, retried, fell back).
pub fn warn(module: &str, msg: impl std::fmt::Display) {
    log(Level::Warn, module, &msg.to_string());
}

/// [`log`] at Error level (failed, skipped, gave up).
pub fn error(module: &str, msg: impl std::fmt::Display) {
    log(Level::Error, module, &msg.to_string());
}

fn open_log(dir: &str, date: &str) -> Option<fs::File> {
    let path = format!("{dir}/{LOG_PREFIX}{date}{LOG_SUFFIX}");
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Delete `yutu-mux-*.log` files in `dir` older than `keep_days`
/// (by filename date; unparseable names and future dates are left
/// alone; anything wearing our prefix with an old date is ours and
/// goes). Best-effort: read/dir errors yield 0. Returns the removed
/// count (useful for tests).
pub fn prune_old_logs_in(dir: &str, keep_days: u64) -> usize {
    let Some(today) = today_days() else {
        return 0;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name
            .strip_prefix(LOG_PREFIX)
            .and_then(|s| s.strip_suffix(LOG_SUFFIX))
        else {
            continue;
        };
        let Some(days) = parse_date(stem) else {
            continue;
        };
        if today - days > keep_days as i64 && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Whole days since the Unix epoch (UTC), or `None` when the clock is
/// unreadable (then logging stays stderr-only / nothing is pruned).
fn today_days() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| (d.as_secs() / 86_400) as i64)
}

/// `YYYY-MM-DD` for days-since-epoch (Hinnant's civil algorithm,
/// proleptic Gregorian; std only, no chrono).
fn date_string(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse `YYYY-MM-DD` back to days-since-epoch (`None` on any shape,
/// range, or calendar violation, so stray files are never pruned).
fn parse_date(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let digits = |lo: usize, hi: usize| {
        s[lo..hi]
            .parse::<i64>()
            .ok()
            .filter(|_| s[lo..hi].bytes().all(|b| b.is_ascii_digit()))
    };
    let (y, m, d) = (digits(0, 4)?, digits(5, 7)?, digits(8, 10)?);
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// Days in a month (proleptic Gregorian leap rule, matching the
/// civil algorithms above).
fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                29
            } else {
                28
            }
        }
    }
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DDTHH:MM:SSZ` (UTC) for the current instant; a fixed
/// placeholder when the clock is unreadable (never empty, always
/// greppable).
fn timestamp_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3_600,
        (tod % 3_600) / 60,
        tod % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static LOG_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_dir(name: &str) -> std::path::PathBuf {
        let n = LOG_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("yutu-mux-applog-{}-{name}-{n}", std::process::id()))
    }

    #[test]
    fn civil_roundtrips_known_dates() {
        // 1970-01-01 is day 0; spot-checks against known weekdays.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(date_string(0), "1970-01-01");
        assert_eq!(date_string(20_454), "2026-01-01");
        assert_eq!(parse_date("2026-10-19"), Some(20_745));
        assert_eq!(parse_date(date_string(20_745).as_str()), Some(20_745));
        // Leap day survives the round trip.
        assert_eq!(parse_date("2024-02-29"), Some(days_from_civil(2024, 2, 29)));
        assert_eq!(date_string(days_from_civil(2024, 2, 29)), "2024-02-29");
    }

    #[test]
    fn parse_date_rejects_garbage() {
        for bad in [
            "",
            "2026-1-1",
            "26-10-19",
            "2026/10/19",
            "2026-13-01",
            "2026-00-10",
            "2026-10-32",
            "2026-02-29",
            "2023-02-29",
            "2026-04-31",
            "not-a-date",
            "2026-10-19.log",
        ] {
            assert_eq!(parse_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn prune_keeps_a_week_and_spares_stray_files() {
        let dir = unique_dir("prune");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let today = today_days().expect("clock works in tests");
        let recent = date_string(today - 3);
        let edge = date_string(today - LOG_KEEP_DAYS as i64);
        let old = date_string(today - LOG_KEEP_DAYS as i64 - 1);
        for name in [
            format!("{LOG_PREFIX}{recent}{LOG_SUFFIX}"),
            format!("{LOG_PREFIX}{edge}{LOG_SUFFIX}"),
            format!("{LOG_PREFIX}{old}{LOG_SUFFIX}"),
            "notes.txt".to_string(),
            format!("{LOG_PREFIX}garbage{LOG_SUFFIX}"),
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(prune_old_logs_in(dir.to_str().unwrap(), LOG_KEEP_DAYS), 1);
        assert!(dir
            .join(format!("{LOG_PREFIX}{recent}{LOG_SUFFIX}"))
            .exists());
        assert!(dir.join(format!("{LOG_PREFIX}{edge}{LOG_SUFFIX}")).exists());
        assert!(!dir.join(format!("{LOG_PREFIX}{old}{LOG_SUFFIX}")).exists());
        assert!(dir.join("notes.txt").exists());
        assert!(dir
            .join(format!("{LOG_PREFIX}garbage{LOG_SUFFIX}"))
            .exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_missing_dir_is_zero() {
        assert_eq!(prune_old_logs_in("/nonexistent-dir-xyz", LOG_KEEP_DAYS), 0);
    }

    #[test]
    fn init_and_log_writes_daily_file() {
        // Only test in this binary touches the global sink, so no
        // cross-test serialization is needed.
        let dir = unique_dir("global");
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.to_str().unwrap());
        info("yutu-test", "hello-file");
        let today = today_days().map(date_string).expect("clock works");
        let content =
            std::fs::read_to_string(dir.join(format!("{LOG_PREFIX}{today}{LOG_SUFFIX}"))).unwrap();
        assert!(content.contains("file logging ready"), "{content}");
        assert!(
            content.contains("[INFO] [yutu-test] hello-file"),
            "{content}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timestamp_shape_is_fixed() {
        let ts = timestamp_now();
        assert_eq!(ts.len(), 20, "{ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[10..11], "T");
    }
}
