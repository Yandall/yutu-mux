//! Lyrics provider layer: LRC parsing + LRCLIB fetch (no credentials)
//! plus YouTube Music plain lyrics (no extra keys, same Innertube
//! session as search).
//!
//! LRCLIB (`https://lrclib.net`) is a free public lyrics API that needs no
//! keys. `GET /api/get?artist_name=X&track_name=Y&album_name=Z&duration=S`
//! returns JSON with `plainLyrics`, `syncedLyrics` (LRC format or null),
//! `instrumental`, and `duration` fields. Exact `/api/get` misses fall
//! back to `/api/search` with fuzzy title/artist matching so alternate
//! spellings, featured artists, and remaster suffixes still resolve.
//!
//! YouTube Music lyrics come from the Innertube `get_lyrics_id` + `get_lyrics`
//! flow (plain lines, no timing). They are the fallback when LRCLIB has
//! no record, and a selectable provider in the UI.

#[cfg(feature = "lyrics")]
use serde::Deserialize;

/// Lyrics source selectable in Settings and the Now Playing drawer.
/// LRCLIB carries synced timing; YouTube Music is plain text only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LyricsProvider {
    Lrclib,
    YouTubeMusic,
}

impl LyricsProvider {
    /// All providers in cycle order (the drawer `< >` stepper walks this).
    pub fn all() -> [Self; 2] {
        [Self::Lrclib, Self::YouTubeMusic]
    }

    /// Human label for pick lists and the drawer stepper.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Lrclib => "LRCLIB",
            Self::YouTubeMusic => "YouTube Music",
        }
    }

    /// Parse a persisted pref value (unknown or empty falls back to LRCLIB).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "youtubemusic" | "youtube_music" | "youtube music" | "ytm" => Self::YouTubeMusic,
            _ => Self::Lrclib,
        }
    }

    /// Persisted pref value.
    pub fn as_pref(&self) -> &'static str {
        match self {
            Self::Lrclib => "lrclib",
            Self::YouTubeMusic => "youtubemusic",
        }
    }

    /// Next provider in cycle order (drawer `>` button).
    pub fn next(self) -> Self {
        match self {
            Self::Lrclib => Self::YouTubeMusic,
            Self::YouTubeMusic => Self::Lrclib,
        }
    }

    /// Previous provider in cycle order (drawer `<` button).
    pub fn prev(self) -> Self {
        self.next()
    }
}

/// Normalize a track title or artist name for fuzzy matching: lowercase,
/// strip parenthetical/bracketed asides (`(Remastered)`, `[Live]`), drop
/// `feat.`/`ft.`/`with` credits, drop version suffixes after ` - `
/// (`- Remastered`, `- Live`, `- Official Video`, ...), remove
/// punctuation, and collapse whitespace. Never panics; empty in gives
/// empty out.
pub fn normalize_for_match(input: &str) -> String {
    let lower = input.to_lowercase();
    // Strip parenthetical and bracketed asides first.
    let mut stripped = String::with_capacity(lower.len());
    let mut paren_depth: u32 = 0;
    let mut bracket_depth: u32 = 0;
    for ch in lower.chars() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {
                if paren_depth == 0 && bracket_depth == 0 {
                    stripped.push(ch);
                }
            }
        }
    }
    // Drop " - suffix" version tags (remaster, live, acoustic, remix,
    // official video/audio, lyric video, sped up, etc.). A bare hyphen
    // inside the title (no spaces) is kept.
    let mut base = stripped.as_str();
    if let Some((head, tail)) = base.rsplit_once(" - ") {
        let suffix = tail.trim();
        const VERSION_HINTS: [&str; 16] = [
            "remaster", "remix", "live", "acoustic", "version", "edit", "mix", "cover", "official",
            "lyric", "audio", "video", "sped up", "slowed", "single", "hq",
        ];
        if VERSION_HINTS.iter().any(|h| suffix.contains(h)) {
            base = head;
        }
    }
    // Drop feat./ft. credits: truncate at the first credit marker.
    // Bare "with" is kept: titles like "Dance With Somebody" would
    // otherwise truncate to just "Dance".
    let mut base = base.to_string();
    for marker in ["feat.", "ft.", "featuring", "feat "] {
        if let Some(idx) = base.find(marker) {
            base.truncate(idx);
        }
    }
    let no_feat = base;
    // Keep alphanumerics and spaces only, collapse whitespace.
    let mut out = String::with_capacity(no_feat.len());
    let mut last_space = true;
    for ch in no_feat.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_owned()
}

/// Fuzzy-match inputs for [`score_candidate`]: the candidate record on
/// the left, the requested track on the right. Plain string views so the
/// scorer stays unit-testable without network types.
pub struct MatchCandidate<'a> {
    pub track_name: &'a str,
    pub artist_name: &'a str,
    pub album_name: &'a str,
    pub duration: u32,
}

/// Requested track for [`score_candidate`].
pub struct MatchQuery<'a> {
    pub artist: &'a str,
    pub title: &'a str,
    pub album: Option<&'a str>,
    pub duration: u32,
}

/// Score an LRCLIB search candidate against the requested track (higher
/// is better). Title match dominates, artist confirms, album is a small
/// bonus, and duration distance penalizes. Exact normalized matches beat
/// substring beats token overlap; unknown durations (0) read as neutral.
pub fn score_candidate(candidate: &MatchCandidate<'_>, query: &MatchQuery<'_>) -> i64 {
    let norm_title = normalize_for_match(candidate.track_name);
    let norm_want_title = normalize_for_match(query.title);
    let norm_artist = normalize_for_match(candidate.artist_name);
    let norm_want_artist = normalize_for_match(query.artist);
    let mut score: i64 = 0;
    if norm_title.is_empty() || norm_want_title.is_empty() {
        return i64::MIN / 2;
    }
    if norm_title == norm_want_title {
        score += 100;
    } else if norm_title.contains(norm_want_title.as_str())
        || norm_want_title.contains(norm_title.as_str())
    {
        score += 50;
    } else {
        let want_tokens: Vec<&str> = norm_want_title.split_whitespace().collect();
        let got_tokens: std::collections::HashSet<&str> = norm_title.split_whitespace().collect();
        let overlap = want_tokens
            .iter()
            .filter(|t| got_tokens.contains(**t))
            .count();
        if overlap == 0 {
            return i64::MIN / 2;
        }
        score += (overlap as i64) * 10;
        // Heavy token mismatch (e.g. a cover with the same one-word
        // chorus) must not outrank duration: cap partial title credit.
        score = score.min(30);
    }
    if !norm_artist.is_empty() && !norm_want_artist.is_empty() {
        if norm_artist == norm_want_artist {
            score += 60;
        } else if norm_artist.contains(norm_want_artist.as_str())
            || norm_want_artist.contains(norm_artist.as_str())
        {
            score += 30;
        } else {
            let want_tokens: Vec<&str> = norm_want_artist.split_whitespace().collect();
            let got_tokens: std::collections::HashSet<&str> =
                norm_artist.split_whitespace().collect();
            let overlap = want_tokens
                .iter()
                .filter(|t| got_tokens.contains(**t))
                .count();
            if overlap == 0 {
                score -= 40;
            } else {
                score += (overlap as i64) * 8;
            }
        }
    }
    if let Some(want_album) = query.album {
        let norm_album = normalize_for_match(candidate.album_name);
        let norm_want_album = normalize_for_match(want_album);
        if !norm_album.is_empty() && !norm_want_album.is_empty() && norm_album == norm_want_album {
            score += 10;
        }
    }
    if query.duration > 0 && candidate.duration > 0 {
        let diff = candidate.duration.abs_diff(query.duration) as i64;
        score += match diff {
            0 => 20,
            1..=2 => 15,
            3..=5 => 10,
            6..=10 => 5,
            _ => -(diff / 2).min(30),
        };
    }
    score
}

/// Convert YouTube Music plain lyric lines into a [`LyricsResult`]
/// (plain text only, no timing). Empty input reads as no lyrics.
pub fn ytm_lyrics_to_result(lines: Vec<String>) -> Result<LyricsResult, String> {
    let kept: Vec<String> = lines
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if kept.is_empty() {
        return Err("youtube music: no lyrics found".to_string());
    }
    Ok(LyricsResult {
        plain: kept.join("\n"),
        synced: Vec::new(),
        instrumental: false,
    })
}

/// Convert YouTube Music timed lyric lines into a [`LyricsResult`] with
/// synced timing (highlight + follow work like LRCLIB synced lyrics).
/// Empty input reads as no lyrics.
pub fn ytm_timed_to_result(timed: Vec<LyricLine>) -> Result<LyricsResult, String> {
    let mut kept: Vec<LyricLine> = timed
        .into_iter()
        .filter(|l| !l.text.trim().is_empty())
        .collect();
    if kept.is_empty() {
        return Err("youtube music: no lyrics found".to_string());
    }
    kept.sort_by_key(|l| l.start_ms);
    let plain = kept
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(LyricsResult {
        plain,
        synced: kept,
        instrumental: false,
    })
}

/// LRCLIB `/api/get` endpoint.
pub const LRCLIB_API_URL: &str = "https://lrclib.net/api/get";

/// User-Agent sent on every LRCLIB request.
pub const LYRICS_USER_AGENT: &str = "yutu-mux/0.2.0 (https://github.com/Yandall/yutu-mux)";

/// One synced lyric line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
}

/// Lyrics for one track: plain text plus optional synced lines.
///
/// `synced` is empty when the provider has no timing data (plain-only
/// lyrics) or for instrumentals.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
pub(crate) fn parse_timestamp(tag: &str) -> Option<u64> {
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
/// - Blank lines are preserved as empty-text stanza markers sharing the
///   next line's timestamp, so renderers can separate strophes; markers
///   never win [`line_at`].
pub fn parse_lrc(input: &str) -> Vec<LyricLine> {
    let mut parsed: Vec<(u64, String, bool)> = Vec::new();
    let mut stanza_break = false;
    for raw_line in input.lines() {
        if raw_line.trim().is_empty() {
            if !parsed.is_empty() {
                stanza_break = true;
            }
            continue;
        }
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
            parsed.push((start_ms, text.clone(), stanza_break));
            stanza_break = false;
        }
    }
    parsed.sort_by_key(|(start_ms, _, _)| *start_ms);
    let mut out = Vec::with_capacity(parsed.len());
    for (start_ms, text, stanza_break) in parsed {
        if stanza_break {
            out.push(LyricLine {
                start_ms,
                text: String::new(),
            });
        }
        out.push(LyricLine { start_ms, text });
    }
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
pub(crate) struct LrclibResponse {
    #[serde(default, rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default, rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

/// One `/api/search` record (full lyrics payload included).
#[cfg(feature = "lyrics")]
#[derive(Debug, Deserialize)]
pub(crate) struct LrclibSearchRecord {
    #[serde(default)]
    id: u64,
    #[serde(default, rename = "trackName")]
    track_name: String,
    #[serde(default, rename = "artistName")]
    artist_name: String,
    #[serde(default, rename = "albumName")]
    album_name: String,
    #[serde(default)]
    duration: u32,
    #[serde(default)]
    instrumental: bool,
    #[serde(default, rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default, rename = "syncedLyrics")]
    synced: Option<String>,
}

/// Maximum LRCLIB attempts per request (initial + 2 retries).
#[cfg(feature = "lyrics")]
const LRCLIB_MAX_ATTEMPTS: u32 = 3;

/// Backoff between LRCLIB retries when the server gives no `Retry-After`:
/// 1 s after the first failure, 2 s after the second.
#[cfg(feature = "lyrics")]
pub(crate) fn lrclib_backoff_secs(failed_attempt: u32) -> u64 {
    match failed_attempt {
        0 => 1,
        _ => 2,
    }
}

/// `Retry-After` delay in seconds, capped at 5 s (a misbehaving edge must
/// not stall the lyrics fetch). Non-numeric values (HTTP dates) fall back
/// to `None` so the caller uses the regular backoff.
#[cfg(feature = "lyrics")]
pub(crate) fn retry_after_secs(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|s| s.min(5))
}

/// GET with retries: transport errors, HTTP 429, and HTTP 5xx retry up to
/// [`LRCLIB_MAX_ATTEMPTS`] times, honoring `Retry-After` on 429/503.
/// Other statuses return immediately for the caller to interpret.
#[cfg(feature = "lyrics")]
async fn lrclib_get(client: &reqwest::Client, url: String) -> Result<reqwest::Response, String> {
    let mut attempt: u32 = 0;
    loop {
        match client.get(url.clone()).send().await {
            Ok(resp) => {
                let status = resp.status();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if retryable && attempt + 1 < LRCLIB_MAX_ATTEMPTS {
                    let wait =
                        retry_after_secs(&resp).unwrap_or_else(|| lrclib_backoff_secs(attempt));
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => {
                let retryable = e.is_timeout() || e.is_connect() || e.status().is_none();
                if retryable && attempt + 1 < LRCLIB_MAX_ATTEMPTS {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_secs(lrclib_backoff_secs(
                        attempt - 1,
                    )))
                    .await;
                    continue;
                }
                if e.is_timeout() {
                    return Err("lrclib request failed: timeout".to_string());
                }
                return Err("lrclib request failed: transport error".to_string());
            }
        }
    }
}

/// Turn a parsed LRCLIB body into a [`LyricsResult`], synthesizing plain
/// text from synced lines when only synced lyrics exist.
#[cfg(feature = "lyrics")]
pub(crate) fn lrclib_body_to_result(
    plain_opt: Option<String>,
    synced_opt: Option<String>,
    instrumental: bool,
) -> Result<LyricsResult, String> {
    if instrumental {
        return Ok(LyricsResult {
            plain: "[Instrumental]".to_string(),
            synced: Vec::new(),
            instrumental: true,
        });
    }
    let plain_raw = plain_opt.unwrap_or_default();
    let synced_raw = synced_opt.unwrap_or_default();
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
    Ok(LyricsResult {
        plain,
        synced,
        instrumental: false,
    })
}

/// Build the LRCLIB `/api/search` URL. At least one of title/artist is
/// sent; album narrows when non-empty.
#[cfg(feature = "lyrics")]
pub(crate) fn lrclib_search_url(artist: &str, title: &str, album: Option<&str>) -> String {
    const SEARCH_URL: &str = "https://lrclib.net/api/search";
    let mut parts = Vec::new();
    if !title.is_empty() {
        parts.push(format!("track_name={}", percent_encode(title)));
    }
    if !artist.is_empty() {
        parts.push(format!("artist_name={}", percent_encode(artist)));
    }
    if let Some(album_name) = album {
        if !album_name.is_empty() {
            parts.push(format!("album_name={}", percent_encode(album_name)));
        }
    }
    if parts.is_empty() {
        // The API ignores a bare query with no fields; fall back to the
        // combined `q` search so something is always sent.
        return format!("{SEARCH_URL}?q={}", percent_encode(title));
    }
    format!("{SEARCH_URL}?{}", parts.join("&"))
}

/// Search LRCLIB for candidates and pick the best fuzzy match. Returns the
/// record id plus its score. `None` when nothing scores above the noise
/// floor (title must at least share a token with the request).
#[cfg(feature = "lyrics")]
pub(crate) fn pick_search_best(
    records: &[LrclibSearchRecord],
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_secs: u32,
) -> Option<(u64, i64)> {
    let mut best: Option<(u64, i64, bool, u32)> = None;
    let query = MatchQuery {
        artist,
        title,
        album,
        duration: duration_secs,
    };
    for r in records {
        // Instrumental records are valid hits; they just have no text.
        let candidate = MatchCandidate {
            track_name: &r.track_name,
            artist_name: &r.artist_name,
            album_name: &r.album_name,
            duration: r.duration,
        };
        let score = score_candidate(&candidate, &query);
        if score <= i64::MIN / 4 {
            continue;
        }
        let has_synced = r.synced.as_deref().is_some_and(|s| !s.trim().is_empty());
        let duration_diff = r.duration.abs_diff(duration_secs);
        let better = match best {
            None => true,
            Some((_, best_score, best_synced, best_diff)) => {
                score > best_score
                    || (score == best_score
                        && (has_synced && !best_synced
                            || (has_synced == best_synced && duration_diff < best_diff)))
            }
        };
        if better {
            best = Some((r.id, score, has_synced, duration_diff));
        }
    }
    best.map(|(id, score, _, _)| (id, score))
}

/// Percent-encode a query parameter value (RFC 3986 unreserved set left as-is).
///
/// `reqwest`'s `query` helper needs the optional `query` feature, which this
/// crate deliberately does not enable (shared `reqwest` feature set with
/// `innertube`/`submit`); encoding here keeps the dependency surface stable.
#[cfg(feature = "lyrics")]
pub(crate) fn percent_encode(value: &str) -> String {
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
pub(crate) fn lrclib_url(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_secs: u32,
) -> String {
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
/// User-Agent. Each request retries up to 3 attempts on transport errors,
/// HTTP 429, or HTTP 5xx (LRCLIB occasionally returns 503 under load),
/// honoring `Retry-After` on 429/503; other HTTP statuses fail that
/// request immediately. When the exact `/api/get` match misses (404 or
/// empty), the fetch falls back to `/api/search` with fuzzy title/artist
/// matching (alternate spellings, featured artists, remaster suffixes)
/// and duration tolerance, then loads the best hit by id. Returns `Err`
/// with a short status/reason string on final failure or when the
/// response carries neither plain nor synced lyrics. When `instrumental`
/// is `true`, returns `Ok` with `plain == "[Instrumental]"` and empty
/// `synced`.
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

    // Exact match first: correct spellings resolve in one round trip.
    let url = lrclib_url(artist, title, album, duration_secs);
    let response = lrclib_get(&client, url).await?;
    let status = response.status();
    if status.is_success() {
        let body: LrclibResponse = response
            .json()
            .await
            .map_err(|_| "lrclib response parse failed".to_string())?;
        match lrclib_body_to_result(body.plain, body.synced, body.instrumental) {
            Ok(result) => return Ok(result),
            Err(_) => {
                // Exact endpoint answered but carries no usable lyrics:
                // fall through to the fuzzy search below.
            }
        }
    } else if status.as_u16() != 404 {
        return Err(format!("lrclib request failed: http {}", status.as_u16()));
    }

    // Fuzzy fallback: alternate titles, featured artists, and remaster
    // suffixes miss the exact endpoint but match `/api/search`.
    let search_url = lrclib_search_url(artist, title, album);
    let search_resp = lrclib_get(&client, search_url).await?;
    if !search_resp.status().is_success() {
        return Err(format!(
            "lrclib request failed: http {}",
            search_resp.status().as_u16()
        ));
    }
    let records: Vec<LrclibSearchRecord> = search_resp
        .json()
        .await
        .map_err(|_| "lrclib response parse failed".to_string())?;
    let Some((best_id, _)) = pick_search_best(&records, artist, title, album, duration_secs) else {
        return Err("lrclib: no lyrics found".to_string());
    };
    // Prefer the in-hand record when it already carries lyrics (saves a
    // round trip); otherwise load the canonical record by id.
    if let Some(hit) = records.iter().find(|r| r.id == best_id) {
        let direct = lrclib_body_to_result(hit.plain.clone(), hit.synced.clone(), hit.instrumental);
        if direct.is_ok() {
            return direct;
        }
    }
    let by_id = lrclib_get(&client, format!("{LRCLIB_API_URL}/{best_id}")).await?;
    if !by_id.status().is_success() {
        return Err(format!(
            "lrclib request failed: http {}",
            by_id.status().as_u16()
        ));
    }
    let body: LrclibResponse = by_id
        .json()
        .await
        .map_err(|_| "lrclib response parse failed".to_string())?;
    lrclib_body_to_result(body.plain, body.synced, body.instrumental)
}
