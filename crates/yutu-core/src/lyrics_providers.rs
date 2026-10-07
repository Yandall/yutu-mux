//! Lyrics provider layer: LRC parsing + LRCLIB fetch (no credentials).
//!
//! LRCLIB (`https://lrclib.net`) is a free public lyrics API that needs no
//! keys. `GET /api/get?artist_name=X&track_name=Y&album_name=Z&duration=S`
//! returns JSON with `plainLyrics`, `syncedLyrics` (LRC format or null),
//! `instrumental`, and `duration` fields.

#[cfg(feature = "lyrics")]
use serde::Deserialize;

/// LRCLIB `/api/get` endpoint.
pub const LRCLIB_API_URL: &str = "https://lrclib.net/api/get";

/// User-Agent sent on every LRCLIB request.
pub const LYRICS_USER_AGENT: &str = "yutu-mux/0.2.0 (https://github.com/Yandall/yutu-mux)";

/// One synced lyric line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
}

/// Lyrics for one track: plain text plus optional synced lines.
///
/// `synced` is empty when the provider has no timing data (plain-only
/// lyrics) or for instrumentals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsResult {
    pub plain: String,
    pub synced: Vec<LyricLine>,
    pub instrumental: bool,
}

/// Parse an LRC timestamp tag body (without brackets) into milliseconds.
///
/// Accepts `mm:ss`, `mm:ss.xx` (centiseconds, 2 digits) and `mm:ss.xxx`
/// (milliseconds, 3 digits). The fraction separator may be `.` or `:`.
/// Returns `None` for metadata tags (`ar:`, `ti:`, `length:`, ...) and any
/// malformed input. Never panics.
fn parse_timestamp(tag: &str) -> Option<u64> {
    let tag = tag.trim();
    let (min_str, rest) = tag.split_once(':')?;
    if min_str.is_empty() || !min_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes: u64 = min_str.parse().ok()?;

    // Split seconds from the optional fraction (`.` or `:` separator).
    let (sec_str, frac_opt) = match rest.find(['.', ':']) {
        Some(idx) => {
            let (s, f) = rest.split_at(idx);
            // `f` starts with the separator; the fraction is the remainder.
            let frac = &f[1..];
            // Reject a second separator inside the fraction.
            if frac.contains('.') || frac.contains(':') {
                return None;
            }
            (s, Some(frac))
        }
        None => (rest, None),
    };
    if sec_str.is_empty() || !sec_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds: u64 = sec_str.parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let frac_ms: u64 = match frac_opt {
        None => 0,
        Some(frac) => {
            if frac.len() == 2 && frac.bytes().all(|b| b.is_ascii_digit()) {
                frac.parse::<u64>().ok()? * 10
            } else if frac.len() == 3 && frac.bytes().all(|b| b.is_ascii_digit()) {
                frac.parse::<u64>().ok()?
            } else {
                return None;
            }
        }
    };
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds.checked_mul(1000)?)?
        .checked_add(frac_ms)
}

/// Parse LRC text into timestamped lines.
///
/// Rules:
/// - `[mm:ss.xx]` tags (2- or 3-digit fraction) and bare `[mm:ss]`; multiple
///   tags on one line emit one [`LyricLine`] per tag with the same text.
/// - Metadata tags (`[ar:]`, `[ti:]`, `[length:]`, ...) carry no timestamp
///   and are ignored.
/// - Lines with no valid timestamp tag, or with empty text after stripping
///   all `[...]` tags, are skipped.
/// - Output is sorted by `start_ms`.
/// - Malformed lines are skipped; this function never panics.
pub fn parse_lrc(input: &str) -> Vec<LyricLine> {
    let mut out = Vec::new();
    for raw_line in input.lines() {
        let mut timestamps = Vec::new();
        let mut text_buf = String::with_capacity(raw_line.len());
        let mut rest = raw_line;
        while let Some(open) = rest.find('[') {
            text_buf.push_str(&rest[..open]);
            let after_open = &rest[open + 1..];
            match after_open.find(']') {
                Some(close) => {
                    let tag = &after_open[..close];
                    if let Some(ms) = parse_timestamp(tag) {
                        timestamps.push(ms);
                    }
                    rest = &after_open[close + 1..];
                }
                None => {
                    // Unclosed bracket: treat the remainder as text.
                    text_buf.push_str(&rest[open..]);
                    rest = "";
                    break;
                }
            }
        }
        text_buf.push_str(rest);
        let text = text_buf.trim().to_owned();
        if timestamps.is_empty() || text.is_empty() {
            continue;
        }
        for start_ms in timestamps {
            out.push(LyricLine {
                start_ms,
                text: text.clone(),
            });
        }
    }
    out.sort_by_key(|l| l.start_ms);
    out
}

/// Index of the last line with `start_ms <= position_ms`.
///
/// Uses binary search (`partition_point`), so complexity is O(log n) in the
/// number of lines. Returns `None` when `lines` is empty or `position_ms` is
/// before the first line.
pub fn line_at(lines: &[LyricLine], position_ms: u64) -> Option<usize> {
    let idx = lines.partition_point(|l| l.start_ms <= position_ms);
    if idx == 0 {
        None
    } else {
        Some(idx - 1)
    }
}

/// LRCLIB response shape (only the fields we need).
#[cfg(feature = "lyrics")]
#[derive(Debug, Deserialize)]
struct LrclibResponse {
    #[serde(default, rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default, rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

/// Percent-encode a query parameter value (RFC 3986 unreserved set left as-is).
///
/// `reqwest`'s `query` helper needs the optional `query` feature, which this
/// crate deliberately does not enable (shared `reqwest` feature set with
/// `innertube`/`submit`); encoding here keeps the dependency surface stable.
#[cfg(feature = "lyrics")]
fn percent_encode(value: &str) -> String {
    const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~";
    let mut out = String::with_capacity(value.len());
    for b in value.as_bytes() {
        if UNRESERVED.contains(b) {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Build the LRCLIB `/api/get` URL. `album_name` is omitted when `None` or
/// empty.
#[cfg(feature = "lyrics")]
fn lrclib_url(artist: &str, title: &str, album: Option<&str>, duration_secs: u32) -> String {
    let mut url = format!(
        "{LRCLIB_API_URL}?artist_name={}&track_name={}&duration={duration_secs}",
        percent_encode(artist),
        percent_encode(title),
    );
    if let Some(album_name) = album {
        if !album_name.is_empty() {
            url.push_str(&format!("&album_name={}", percent_encode(album_name)));
        }
    }
    url
}
///
/// Fetch lyrics from LRCLIB (no credentials required).
///
/// Sends `artist_name`, `track_name`, and `duration` plus `album_name` when
/// `album` is `Some` and non-empty, with a 15 s timeout and the crate
/// User-Agent. Retries ONCE after a 1 s delay on transport errors or HTTP
/// 5xx (LRCLIB occasionally returns 503 under load); other HTTP statuses
/// fail immediately. Returns `Err` with a short status/reason string on
/// final failure or when the response carries neither plain nor synced
/// lyrics. When `instrumental` is `true`, returns `Ok` with
/// `plain == "[Instrumental]"` and empty `synced`.
///
/// Error strings contain only the HTTP status and a short reason; they never
/// include the artist/title.
#[cfg(feature = "lyrics")]
pub async fn fetch_lrclib(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_secs: u32,
) -> Result<LyricsResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(LYRICS_USER_AGENT)
        .build()
        .map_err(|e| format!("lrclib client build failed: {e}"))?;

    let mut attempt: u32 = 0;
    loop {
        let url = lrclib_url(artist, title, album, duration_secs);
        let response = match client.get(url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                if attempt == 0 && (e.is_timeout() || e.is_connect() || e.status().is_none()) {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
                if e.is_timeout() {
                    return Err("lrclib request failed: timeout".to_string());
                }
                return Err("lrclib request failed: transport error".to_string());
            }
        };
        let status = response.status();
        if status.is_server_error() {
            if attempt == 0 {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
            return Err(format!("lrclib request failed: http {}", status.as_u16()));
        }
        if !status.is_success() {
            return Err(format!("lrclib request failed: http {}", status.as_u16()));
        }
        let body: LrclibResponse = response
            .json()
            .await
            .map_err(|_| "lrclib response parse failed".to_string())?;
        if body.instrumental {
            return Ok(LyricsResult {
                plain: "[Instrumental]".to_string(),
                synced: Vec::new(),
                instrumental: true,
            });
        }
        let plain_raw = body.plain.unwrap_or_default();
        let synced_raw = body.synced.unwrap_or_default();
        let has_plain = !plain_raw.trim().is_empty();
        let has_synced = !synced_raw.trim().is_empty();
        if !has_plain && !has_synced {
            return Err("lrclib: no lyrics found".to_string());
        }
        let synced = if has_synced {
            parse_lrc(&synced_raw)
        } else {
            Vec::new()
        };
        let plain = if has_plain {
            plain_raw
        } else {
            synced
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        if plain.trim().is_empty() && synced.is_empty() {
            return Err("lrclib: no lyrics found".to_string());
        }
        return Ok(LyricsResult {
            plain,
            synced,
            instrumental: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(start_ms: u64, text: &str) -> LyricLine {
        LyricLine {
            start_ms,
            text: text.to_owned(),
        }
    }

    #[test]
    fn lrc_single_line_basic() {
        let lines = parse_lrc("[00:12.34]Hello\n");
        assert_eq!(lines, vec![tagged(12_340, "Hello")]);
    }

    #[test]
    fn lrc_multi_tag_line_emits_one_per_timestamp() {
        let lines = parse_lrc("[00:01.00][00:02.00]Hello");
        assert_eq!(lines, vec![tagged(1_000, "Hello"), tagged(2_000, "Hello")]);
    }

    #[test]
    fn lrc_metadata_tags_skipped() {
        let input = "[ar:Artist]\n[ti:Title]\n[al:Album]\n[length: 03:45]\n[by:someone]\n[00:10.00]Real line\n";
        let lines = parse_lrc(input);
        assert_eq!(lines, vec![tagged(10_000, "Real line")]);
    }

    #[test]
    fn lrc_two_vs_three_digit_fractions() {
        let lines = parse_lrc("[00:01.23]two\n[00:02.456]three\n");
        assert_eq!(lines, vec![tagged(1_230, "two"), tagged(2_456, "three")]);
    }

    #[test]
    fn lrc_bare_mm_ss_without_fraction() {
        let lines = parse_lrc("[01:23]No fraction\n");
        assert_eq!(lines, vec![tagged(83_000, "No fraction")]);
    }

    #[test]
    fn lrc_unsorted_input_gets_sorted() {
        let lines = parse_lrc("[00:30.00]third\n[00:10.00]first\n[00:20.00]second\n");
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
        assert!(lines.windows(2).all(|w| w[0].start_ms <= w[1].start_ms));
    }

    #[test]
    fn lrc_garbage_lines_skipped() {
        let input = "no tags here\n[bad]nope\n[00:xx.yy]bad time\n[00:10.0]one digit frac\n[00:70.00]bad seconds\n[00:10.00]\n[00:11.00]   \n[00:12.00]Good\n";
        let lines = parse_lrc(input);
        assert_eq!(lines, vec![tagged(12_000, "Good")]);
    }

    #[test]
    fn lrc_empty_input_gives_empty_vec() {
        assert!(parse_lrc("").is_empty());
        assert!(parse_lrc("\n   \n").is_empty());
    }

    #[test]
    fn lrc_colon_fraction_separator_accepted() {
        let lines = parse_lrc("[00:01:23]colon frac\n");
        assert_eq!(lines, vec![tagged(1_230, "colon frac")]);
    }

    #[test]
    fn line_at_before_first_is_none() {
        let lines = vec![tagged(1_000, "a"), tagged(2_000, "b")];
        assert_eq!(line_at(&lines, 0), None);
        assert_eq!(line_at(&lines, 999), None);
    }

    #[test]
    fn line_at_exact_hit() {
        let lines = vec![tagged(1_000, "a"), tagged(2_000, "b")];
        assert_eq!(line_at(&lines, 1_000), Some(0));
        assert_eq!(line_at(&lines, 2_000), Some(1));
    }

    #[test]
    fn line_at_between_lines() {
        let lines = vec![tagged(1_000, "a"), tagged(2_000, "b"), tagged(3_000, "c")];
        assert_eq!(line_at(&lines, 1_500), Some(0));
        assert_eq!(line_at(&lines, 2_999), Some(1));
    }

    #[test]
    fn line_at_past_end_gives_last() {
        let lines = vec![tagged(1_000, "a"), tagged(2_000, "b")];
        assert_eq!(line_at(&lines, 2_001), Some(1));
        assert_eq!(line_at(&lines, u64::MAX), Some(1));
    }

    #[test]
    fn line_at_empty_slice_is_none() {
        assert_eq!(line_at(&[], 10_000), None);
    }

    #[test]
    fn lyrics_result_offline_shape() {
        let r = LyricsResult {
            plain: "line one\nline two".to_owned(),
            synced: vec![tagged(1_000, "line one"), tagged(2_000, "line two")],
            instrumental: false,
        };
        assert!(!r.plain.is_empty());
        assert_eq!(r.synced.len(), 2);
        assert!(!r.instrumental);

        let inst = LyricsResult {
            plain: "[Instrumental]".to_owned(),
            synced: Vec::new(),
            instrumental: true,
        };
        assert_eq!(inst.plain, "[Instrumental]");
        assert!(inst.synced.is_empty());
        assert!(inst.instrumental);
    }

    #[cfg(feature = "lyrics")]
    #[test]
    fn lrclib_url_omits_empty_album() {
        let with_album = lrclib_url("The Beatles", "Yesterday", Some("Help!"), 125);
        assert!(with_album.contains("album_name=Help%21"));
        assert!(with_album.contains("artist_name=The%20Beatles"));
        let no_album = lrclib_url("A", "T", None, 200);
        assert!(!no_album.contains("album_name"));
        let empty_album = lrclib_url("A", "T", Some(""), 200);
        assert!(!empty_album.contains("album_name"));
    }

    #[cfg(feature = "lyrics")]
    #[tokio::test]
    #[ignore]
    async fn live_lrclib_beatles_yesterday() {
        let res = fetch_lrclib("The Beatles", "Yesterday", Some("Help!"), 125).await;
        match res {
            Ok(lyrics) => assert!(
                !lyrics.plain.trim().is_empty(),
                "expected non-empty plain lyrics for LRCLIB lookup"
            ),
            Err(e) => panic!("live LRCLIB fetch failed: {e}"),
        }
    }
}
