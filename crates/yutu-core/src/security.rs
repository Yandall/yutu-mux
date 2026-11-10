//! Least-privilege barrier: the app only does what it is supposed to do.
//!
//! Rules:
//! - All untrusted ids and URLs (server rows, sidecar stdout, D-Bus,
//!   file picker) are validated before use, so a compromised third party
//!   (sidecar, Innertube row, artwork host, decoder) cannot escalate to
//!   RCE or exfiltration by smuggling a `file://` URL, an attacker host,
//!   or a shell metacharacter through a string the app then fetches or
//!   hands to libmpv.
//! - Std-only, no feature gate, so every build config gets the same
//!   policy. Callers fail closed (reject), never fail open.

/// YouTube video ids are always 11 chars of base64url.
pub const VIDEO_ID_LEN: usize = 11;

/// Longest URL the app accepts (sidecar stdout, server rows).
pub const MAX_URL_LEN: usize = 4096;

/// Longest search query forwarded to Innertube (user typed).
pub const MAX_SEARCH_QUERY_LEN: usize = 500;

/// Largest cookie file accepted (normal files are a few KB).
pub const MAX_COOKIE_FILE_BYTES: u64 = 1_000_000;

/// Largest seek or position jump accepted from D-Bus (24h, far past any song).
pub const MAX_SEEK_SECS: f64 = 86_400.0;

/// True for a strict YouTube video id (`^[A-Za-z0-9_-]{11}$`).
#[must_use]
pub fn is_valid_video_id(id: &str) -> bool {
    if id.len() != VIDEO_ID_LEN {
        return false;
    }
    id.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// True for server ids (playlists, channels, browse ids): short, flat
/// charset, no slashes, no whitespace, no scheme. Length 2 to 128 covers
/// `LM`, `PL...`, `VLPL...`, `UC...`, `MPRE...` without allowing blobs.
#[must_use]
pub fn is_safe_server_id(id: &str) -> bool {
    if id.len() < 2 || id.len() > 128 {
        return false;
    }
    id.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Authority (host) part of an `https://` URL, lowercased. `None` when the
/// shape is wrong, when userinfo (`@`) or an explicit port (`:`) is
/// present, or when whitespace sneaks in. Googlevideo and artwork hosts
/// never use ports or userinfo, so rejecting them costs nothing.
fn https_authority(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    if rest.is_empty() || url.len() > MAX_URL_LEN {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains(':')
        || authority.bytes().any(|b| b.is_ascii_whitespace())
    {
        return None;
    }
    let host = authority.to_ascii_lowercase();
    if host.contains("..") || host.starts_with('.') || host.starts_with('-') {
        return None;
    }
    Some(host)
}

/// True for playable stream URLs: `https` plus a googlevideo host with a
/// path. Everything else (including `http`, `file`, attacker hosts, and
/// loopback) is rejected, so a compromised sidecar cannot redirect mpv
/// or the fetcher off-domain.
#[must_use]
pub fn is_allowed_stream_url(url: &str) -> bool {
    let Some(host) = https_authority(url) else {
        return false;
    };
    let ok_host = host == "googlevideo.com" || host.ends_with(".googlevideo.com");
    if !ok_host {
        return false;
    }
    let rest = &url["https://".len()..];
    rest.contains('/')
}

/// True for artwork URLs: `https` plus a YouTube image host. Covers the
/// thumbnail shapes Innertube serves (`i.ytimg.com`,
/// `yt3/lh3.googleusercontent.com`) and nothing else.
#[must_use]
pub fn is_allowed_artwork_url(url: &str) -> bool {
    let Some(host) = https_authority(url) else {
        return false;
    };
    host == "i.ytimg.com"
        || host == "ytimg.com"
        || host.ends_with(".ytimg.com")
        || host == "googleusercontent.com"
        || host.ends_with(".googleusercontent.com")
        || host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtu.be"
        || host == "music.youtube.com"
}

/// True for any URL the generic fetcher may pull: streams plus artwork.
/// Anything else (attacker hosts, `http`, `file`, loopback) is rejected.
#[must_use]
pub fn is_allowed_fetch_url(url: &str) -> bool {
    is_allowed_stream_url(url) || is_allowed_artwork_url(url)
}

/// Strip query and fragment for logs and status text: googlevideo URLs
/// carry short-lived `expire` and signature params that must never land
/// in the log file or on screen. Keeps scheme plus host plus path,
/// truncated to 200 chars.
#[must_use]
pub fn redact_url_for_log(url: &str) -> String {
    let base = url.split(['?', '#']).next().unwrap_or(url);
    let mut out: String = base.chars().take(200).collect();
    if base.len() > out.len() || url.len() != base.len() {
        out.push_str("?...redacted");
    }
    out
}

/// Truncate untrusted text (sidecar stderr, server bodies) for error
/// strings, so a compromised helper cannot OOM the app by dumping
/// megabytes into an error that then lives in status history.
#[must_use]
pub fn truncate_for_log(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push_str("...truncated");
    }
    out
}

/// Clamp a D-Bus relative seek to finite plus or minus 24h. Non-finite
/// becomes 0 (no-op) so a malicious client cannot smuggle NaN into the
/// playback controller.
#[must_use]
pub fn clamp_seek_offset_secs(offset: f64) -> f64 {
    if !offset.is_finite() {
        return 0.0;
    }
    offset.clamp(-MAX_SEEK_SECS, MAX_SEEK_SECS)
}

/// Clamp a D-Bus absolute position to `0.0..=24h`. Non-finite or
/// negative becomes 0.
#[must_use]
pub fn clamp_position_secs(pos: f64) -> f64 {
    if !pos.is_finite() {
        return 0.0;
    }
    pos.clamp(0.0, MAX_SEEK_SECS)
}

/// True when `query` is short enough to forward to Innertube.
#[must_use]
pub fn is_safe_search_query(query: &str) -> bool {
    !query.is_empty() && query.len() <= MAX_SEARCH_QUERY_LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_id_is_strict_11() {
        assert!(is_valid_video_id("dQw4w9WgXcQ"));
        assert!(is_valid_video_id("JWW8KK8kd1I"));
        assert!(is_valid_video_id("F8qqWkmtCG0"));
        assert!(!is_valid_video_id("m1"));
        assert!(!is_valid_video_id("vid"));
        assert!(!is_valid_video_id("dQw4w9WgXcQ;rm -rf"));
        assert!(!is_valid_video_id("dQw4w9WgXcQ "));
        assert!(!is_valid_video_id(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(!is_valid_video_id(""));
        assert!(!is_valid_video_id("dQw4w9WgXcQextra"));
        assert!(!is_valid_video_id("-abc;curl evil"));
    }

    #[test]
    fn server_id_allows_known_shapes() {
        assert!(is_safe_server_id("LM"));
        assert!(is_safe_server_id("PL123abc_-"));
        assert!(is_safe_server_id("VLPL123"));
        assert!(is_safe_server_id("UCabc123_-"));
        assert!(!is_safe_server_id("a"));
        assert!(!is_safe_server_id(""));
        assert!(!is_safe_server_id("PL/evil"));
        assert!(!is_safe_server_id("PL evil"));
        assert!(!is_safe_server_id("https://evil"));
        assert!(!is_safe_server_id(&"a".repeat(129)));
    }

    #[test]
    fn stream_url_allows_only_googlevideo_https() {
        assert!(is_allowed_stream_url(
            "https://r1---sn-abc.googlevideo.com/videoplayback?expire=1&sig=x"
        ));
        assert!(is_allowed_stream_url(
            "https://googlevideo.com/videoplayback"
        ));
        assert!(!is_allowed_stream_url(
            "http://r1.googlevideo.com/videoplayback"
        ));
        assert!(!is_allowed_stream_url("file:///etc/passwd"));
        assert!(!is_allowed_stream_url("https://evil.com/videoplayback"));
        assert!(!is_allowed_stream_url("https://googlevideo.com.evil.com/x"));
        assert!(!is_allowed_stream_url("https://127.0.0.1/videoplayback"));
        assert!(!is_allowed_stream_url("https://evil@r1.googlevideo.com/x"));
        assert!(!is_allowed_stream_url(
            "https://r1.googlevideo.com:443/videoplayback"
        ));
        assert!(!is_allowed_stream_url("https://r1.googlevideo.com"));
        assert!(!is_allowed_stream_url(""));
    }

    #[test]
    fn artwork_url_allows_only_image_hosts() {
        assert!(is_allowed_artwork_url(
            "https://i.ytimg.com/vi/abc/hqdefault.jpg"
        ));
        assert!(is_allowed_artwork_url(
            "https://yt3.googleusercontent.com/abc/photo.jpg"
        ));
        assert!(is_allowed_artwork_url(
            "https://lh3.googleusercontent.com/abc=w60-h60"
        ));
        assert!(!is_allowed_artwork_url("https://evil.com/cover.jpg"));
        assert!(!is_allowed_artwork_url(
            "http://i.ytimg.com/vi/abc/hqdefault.jpg"
        ));
        assert!(!is_allowed_artwork_url("file:///tmp/x.jpg"));
        assert!(!is_allowed_artwork_url("https://127.0.0.1/cover.jpg"));
    }

    #[test]
    fn fetch_url_is_union() {
        assert!(is_allowed_fetch_url(
            "https://r1.googlevideo.com/videoplayback?x=1"
        ));
        assert!(is_allowed_fetch_url(
            "https://i.ytimg.com/vi/abc/hqdefault.jpg"
        ));
        assert!(!is_allowed_fetch_url("https://evil.com/x"));
        assert!(!is_allowed_fetch_url("https://example.invalid/audio.webm"));
    }

    #[test]
    fn redact_strips_query() {
        let redacted =
            redact_url_for_log("https://r1.googlevideo.com/videoplayback?expire=1&sig=secret");
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("expire"));
        assert!(redacted.contains("googlevideo.com"));
        assert!(redacted.contains("redacted"));
    }

    #[test]
    fn truncate_caps() {
        assert_eq!(truncate_for_log("abc", 10), "abc");
        let long = "a".repeat(100);
        let out = truncate_for_log(&long, 10);
        assert!(out.len() < long.len());
        assert!(out.contains("truncated"));
    }

    #[test]
    fn seek_clamps() {
        assert_eq!(clamp_seek_offset_secs(f64::NAN), 0.0);
        assert_eq!(clamp_seek_offset_secs(f64::INFINITY), 0.0);
        assert_eq!(clamp_seek_offset_secs(f64::NEG_INFINITY), 0.0);
        assert_eq!(clamp_seek_offset_secs(1e12), MAX_SEEK_SECS);
        assert_eq!(clamp_seek_offset_secs(-1e12), -MAX_SEEK_SECS);
        assert_eq!(clamp_seek_offset_secs(5.0), 5.0);
        assert_eq!(clamp_position_secs(f64::NAN), 0.0);
        assert_eq!(clamp_position_secs(-5.0), 0.0);
        assert_eq!(clamp_position_secs(1e12), MAX_SEEK_SECS);
    }

    #[test]
    fn search_query_length() {
        assert!(!is_safe_search_query(""));
        assert!(is_safe_search_query("beatles"));
        assert!(!is_safe_search_query(&"a".repeat(MAX_SEARCH_QUERY_LEN + 1)));
    }
}
