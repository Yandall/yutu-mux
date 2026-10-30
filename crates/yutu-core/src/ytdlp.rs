//! Audio-URL resolution via the `yt-dlp` sidecar.
//!
//! Rationale: signature-cipher decipher + PO-token policy is a treadmill
//! best maintained by yt-dlp (updated within hours of YouTube changes;
//! verified working Oct 2026, when `rustypipe` 0.11.4's deobfuscator could no
//! longer parse the player JS). Same sidecar pattern as `pot.rs`: external
//! binary, auto-detected, graceful degradation when absent.
//!
//! Std-only, no cargo deps, no feature gate: and license-clean (yt-dlp is
//! public domain / Unlicense), unlike the removed GPL `rustypipe` path.

use std::path::PathBuf;
use std::process::Command;

use crate::stream::StreamError;

pub const YTDLP_BIN_NAME: &str = "yt-dlp";

/// Locate the sidecar: `YUTU_YTDLP_BIN` env override first (tests, custom
/// installs), then `PATH`, then the current working directory.
pub fn find_yt_dlp() -> Option<String> {
    if let Some(path) = std::env::var_os("YUTU_YTDLP_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in [YTDLP_BIN_NAME, &format!("{YTDLP_BIN_NAME}.exe")] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }
    let cwd = std::env::current_dir()
        .map(|d| d.join(YTDLP_BIN_NAME))
        .unwrap_or_else(|_| PathBuf::from(YTDLP_BIN_NAME));
    if cwd.is_file() {
        return Some(cwd.to_string_lossy().into_owned());
    }
    None
}

/// Resolve `video_id` to a direct audio URL: `yt-dlp --get-url -f
/// bestaudio/best`. Prefers AAC-in-M4A (itag 140 class): our pure-Rust
/// decoder (symphonia) cannot do Opus-in-WebM, so Opus is last resort.
/// Returns the first `https://` line of stdout.
pub fn resolve_audio_url(bin: &str, video_id: &str) -> Result<String, StreamError> {
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = Command::new(bin)
        .args([
            "--get-url",
            "--quiet",
            "--no-warnings",
            "-f",
            "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio",
            "--no-playlist",
            "--no-progress",
            &url,
        ])
        .output()
        .map_err(|e| StreamError::Network(format!("run {bin}: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(line) = stdout.lines().find_map(|l| {
        let l = l.trim();
        l.starts_with("https://").then_some(l.to_string())
    }) {
        return Ok(line);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(classify_ytdlp_error(&stderr, video_id))
}

/// Fetch `video_id` audio straight to memory via the sidecar itself
/// (`yt-dlp -o -`): the sidecar's own HTTP stack sends the headers,
/// retries, and throttling handling that a bare `GET` of a `--get-url`
/// URL lacks, so this is the fallback when fetching a resolved URL
/// 403s (transient edge rejection) or when re-resolving also fails.
/// Memory-only for playback; never written to disk. Same format
/// preference as [`resolve_audio_url`]. Capped at 200 MiB.
pub fn download_audio_bytes(bin: &str, video_id: &str) -> Result<Vec<u8>, StreamError> {
    const MAX_BYTES: u64 = 200 * 1024 * 1024;
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = Command::new(bin)
        .args([
            "-f",
            "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio",
            "--no-playlist",
            "--quiet",
            "--no-warnings",
            "--no-progress",
            "-o",
            "-",
            &url,
        ])
        .output()
        .map_err(|e| StreamError::Network(format!("run {bin}: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_ytdlp_error(&stderr, video_id));
    }
    if output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_ytdlp_error(&stderr, video_id));
    }
    if output.stdout.len() as u64 > MAX_BYTES {
        return Err(StreamError::Network(format!(
            "yt-dlp direct fetch of {video_id}: over size cap"
        )));
    }
    Ok(output.stdout)
}

fn classify_ytdlp_error(stderr: &str, video_id: &str) -> StreamError {
    let lowered = stderr.to_lowercase();
    if crate::stream::looks_po_gated(&lowered) {
        return StreamError::PoTokenRequired {
            client: "web_music",
        };
    }
    if lowered.contains("private")
        || lowered.contains("unavailable")
        || lowered.contains("age")
        || lowered.contains("login")
        || lowered.contains("not found")
        || lowered.contains("removed")
    {
        StreamError::NotFound {
            video_id: video_id.to_string(),
        }
    } else {
        StreamError::Network(format!("yt-dlp: {}", stderr.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_fake_ytdlp(stdout_body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "yutu-fake-ytdlp-{}-{}.sh",
            std::process::id(),
            stdout_body.len()
        ));
        std::fs::write(&path, format!("#!/bin/sh\necho '{stdout_body}'\n")).unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&path, perm).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn resolves_first_https_line() {
        let bin = write_fake_ytdlp("https://example.invalid/audio.webm");
        let url = resolve_audio_url(&bin.to_string_lossy(), "vid").unwrap();
        assert_eq!(url, "https://example.invalid/audio.webm");
    }

    #[test]
    #[cfg(unix)]
    fn stderr_without_url_classifies() {
        let bin = write_fake_ytdlp("");
        let err = resolve_audio_url(&bin.to_string_lossy(), "vid").unwrap_err();
        assert!(matches!(err, StreamError::Network(_)));
    }

    #[test]
    fn missing_binary_is_a_clean_error() {
        let err = resolve_audio_url("/nonexistent/yt-dlp", "x").unwrap_err();
        assert!(matches!(err, StreamError::Network(_)));
    }

    #[test]
    #[cfg(unix)]
    fn direct_download_returns_stdout_bytes() {
        // Fake sidecar: ignores args, prints media bytes to stdout.
        let bin = write_fake_ytdlp("fake-audio-bytes");
        let bytes = download_audio_bytes(&bin.to_string_lossy(), "vid").unwrap();
        // `echo` appends a newline; the point is stdout passes through.
        assert_eq!(bytes, b"fake-audio-bytes\n");
    }

    #[test]
    fn direct_download_missing_binary_is_a_clean_error() {
        let err = download_audio_bytes("/nonexistent/yt-dlp", "x").unwrap_err();
        assert!(matches!(err, StreamError::Network(_)));
    }

    #[test]
    fn env_override_wins() {
        let key = "YUTU_YTDLP_BIN";
        let old = std::env::var_os(key);
        std::env::set_var(key, "/nonexistent/custom-yt-dlp");
        let found = find_yt_dlp();
        // Nonexistent override falls through to PATH/cwd lookup.
        let _ = found;
        match old {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
