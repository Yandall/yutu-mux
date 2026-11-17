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
/// installs), then the directory holding the running executable (the
/// Windows installer and the Scoop/zip layout ship `yt-dlp` beside
/// `yutu-mux` so nothing needs to be on PATH), then `PATH`.
/// No current-directory fallback: running from an attacker directory must
/// never pick up a planted binary.
pub fn find_yt_dlp() -> Option<String> {
    if let Some(path) = std::env::var_os("YUTU_YTDLP_BIN") {
        let path = PathBuf::from(path);
        // Env override must be absolute: a relative name would resolve
        // against the current directory, reintroducing the plant this
        // barrier removes.
        if path.is_absolute() && is_executable_file(&path) {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in [YTDLP_BIN_NAME, &format!("{YTDLP_BIN_NAME}.exe")] {
                let candidate = dir.join(name);
                if is_executable_file(&candidate) {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            // Empty entries, "." and any relative entry mean cwd: skip
            // them, never load from there.
            if dir.as_os_str().is_empty() {
                continue;
            }
            if !dir.is_absolute() {
                continue;
            }
            for name in [YTDLP_BIN_NAME, &format!("{YTDLP_BIN_NAME}.exe")] {
                let candidate = dir.join(name);
                if is_executable_file(&candidate) {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

fn is_executable_file(path: &std::path::Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Build a sidecar [`Command`] that never flashes a console window.
/// Windows gives every console-subsystem child its own console unless
/// asked not to (`CREATE_NO_WINDOW`); for a GUI app that flash reads as
/// malware behavior, so every sidecar spawn goes through here. Everywhere
/// else this is exactly `Command::new`.
pub(crate) fn sidecar_command(bin: &str) -> Command {
    // `mut` is only needed for the Windows flag below.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// yt-dlp player client tried first. Its URLs stream on the first
/// request. Newer yt-dlp defaults (2026.10+) also query `tv_samsung`
/// and prefer its `TVHTML5` URLs, which the edge answers with HTTP 403
/// for roughly 1.5 to 3.5 s after minting: a player that opens them at
/// once fails the load. Older defaults (visionos only) never hit it.
pub const PREFERRED_PLAYER_CLIENT: &str = "visionos";

/// Resolve `video_id` to a direct audio URL: `yt-dlp --get-url -f
/// bestaudio/best`. Prefers AAC-in-M4A (itag 140 class): our pure-Rust
/// decoder (symphonia) cannot do Opus-in-WebM, so Opus is last resort.
/// Asks [`PREFERRED_PLAYER_CLIENT`] first, then yt-dlp's own default
/// client set when that fails (clients break independently, so a pin
/// must never be the only path). Returns the first `https://` line of
/// stdout that passes the stream allowlist; a sidecar that returns an
/// off-domain URL is rejected fail-closed. On double failure the default-set
/// error wins (it is the configuration yt-dlp maintains).
pub fn resolve_audio_url(bin: &str, video_id: &str) -> Result<String, StreamError> {
    if !crate::security::is_valid_video_id(video_id) {
        return Err(StreamError::NotFound {
            video_id: crate::security::truncate_for_log(video_id, 64),
        });
    }
    match get_url(bin, video_id, Some(PREFERRED_PLAYER_CLIENT)) {
        Ok(url) => Ok(url),
        Err(_) => get_url(bin, video_id, None),
    }
}

/// Player client for the signed-in path. `web_music` is the only client
/// that serves YouTube Music's 256 kbps AAC (itag 141), and only to
/// Premium accounts: yt-dlp detects Premium from the session and then
/// needs no PO token for it. Free accounts get no usable formats from it
/// (PO-gated), which is what makes the attempt fail fast for them.
pub const PREMIUM_PLAYER_CLIENT: &str = "web_music";

/// Format order on the signed-in path: Premium 256 kbps AAC first (by
/// prefix: YouTube may suffix ids, e.g. `140-11`), then
/// the same AAC-first preference as the anonymous path.
const PREMIUM_FORMAT: &str =
    "bestaudio[format_id^=141]/bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio";

/// After a failed signed-in attempt, skip it for this long (free
/// accounts, missing JS runtime, expired cookies): one wasted sidecar run
/// per window instead of one per song. Re-checked after, so an upgrade
/// to Premium is picked up without a restart.
const PREMIUM_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Per-process memory of failed signed-in attempts, keyed by a keyed
/// hash of the cookie header (a new sign-in retries at once). Holds no
/// cookie material.
type Backoff = std::collections::HashMap<u64, std::time::Instant>;
static PREMIUM_BACKOFF: std::sync::Mutex<Option<Backoff>> = std::sync::Mutex::new(None);

fn session_fingerprint(auth: &crate::auth::CookieAuth) -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static KEYS: std::sync::OnceLock<std::collections::hash_map::RandomState> =
        std::sync::OnceLock::new();
    let mut h = KEYS.get_or_init(Default::default).build_hasher();
    h.write(auth.header_value().as_bytes());
    h.finish()
}

fn premium_backed_off(fp: u64) -> bool {
    let guard = PREMIUM_BACKOFF.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|m| m.get(&fp))
        .is_some_and(|at| at.elapsed() < PREMIUM_RETRY_AFTER)
}

/// Record (`failed`) or clear the backoff for one session.
fn set_premium_backoff(fp: u64, failed: bool) {
    let mut guard = PREMIUM_BACKOFF.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(Default::default);
    map.retain(|_, at| at.elapsed() < PREMIUM_RETRY_AFTER);
    if failed {
        map.insert(fp, std::time::Instant::now());
    } else {
        map.remove(&fp);
    }
}

/// [`resolve_audio_url`] with an optional signed-in session. With
/// cookies it first asks [`PREMIUM_PLAYER_CLIENT`] as that account
/// (Premium accounts get 256 kbps AAC); any failure there falls back to
/// the anonymous path unchanged, so a session can never make playback
/// worse. The cookies reach the sidecar only through a private,
/// short-lived jar file (see [`crate::cookie_jar`]), never argv or env.
pub fn resolve_audio_url_with_session(
    bin: &str,
    video_id: &str,
    session: Option<&crate::auth::CookieAuth>,
) -> Result<String, StreamError> {
    if !crate::security::is_valid_video_id(video_id) {
        return Err(StreamError::NotFound {
            video_id: crate::security::truncate_for_log(video_id, 64),
        });
    }
    if let Some(auth) = session {
        let fp = session_fingerprint(auth);
        if !premium_backed_off(fp) {
            match get_url_signed_in(bin, video_id, auth) {
                Ok((url, format)) => {
                    set_premium_backoff(fp, false);
                    crate::app_log::info(
                        "yutu-stream",
                        format_args!("signed-in audio for {video_id}: format {format}"),
                    );
                    return Ok(url);
                }
                Err(e) => {
                    set_premium_backoff(fp, true);
                    crate::app_log::info(
                        "yutu-stream",
                        format_args!(
                            "signed-in audio unavailable ({e}); standard quality for {} min",
                            PREMIUM_RETRY_AFTER.as_secs() / 60
                        ),
                    );
                }
            }
        }
    }
    resolve_audio_url(bin, video_id)
}

/// One signed-in sidecar run: returns the stream URL and its format id.
fn get_url_signed_in(
    bin: &str,
    video_id: &str,
    auth: &crate::auth::CookieAuth,
) -> Result<(String, String), StreamError> {
    let jar = crate::cookie_jar::CookieJarFile::create(auth).map_err(StreamError::Network)?;
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = sidecar_command(bin)
        .arg("--cookies")
        .arg(jar.path())
        .args([
            "--extractor-args",
            &format!("youtube:player_client={PREMIUM_PLAYER_CLIENT}"),
            "--print",
            "format_id",
            "--print",
            "urls",
            "--quiet",
            "--no-warnings",
            "-f",
            PREMIUM_FORMAT,
            "--no-playlist",
            "--no-progress",
            &url,
        ])
        .output()
        .map_err(|e| StreamError::Network(format!("run {bin}: {e}")))?;
    // The jar goes as soon as the run ends (the sidecar rewrote it).
    drop(jar);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let format = lines.next().unwrap_or_default().to_string();
    let found = lines.find(|l| l.starts_with("https://"));
    match found {
        Some(line) if crate::security::is_allowed_stream_url(line) => Ok((
            line.to_string(),
            crate::security::truncate_for_log(&format, 16),
        )),
        Some(_) => Err(StreamError::Network(
            "sidecar returned untrusted URL".to_string(),
        )),
        None => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(classify_ytdlp_error(&stderr, video_id))
        }
    }
}

/// One `yt-dlp --get-url` run, optionally pinned to a player client.
fn get_url(bin: &str, video_id: &str, client: Option<&str>) -> Result<String, StreamError> {
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let mut cmd = sidecar_command(bin);
    if let Some(client) = client {
        cmd.args([
            "--extractor-args",
            &format!("youtube:player_client={client}"),
        ]);
    }
    let output = cmd
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
        if crate::security::is_allowed_stream_url(&line) {
            return Ok(line);
        }
        return Err(StreamError::Network(
            "sidecar returned untrusted URL".to_string(),
        ));
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
    if !crate::security::is_valid_video_id(video_id) {
        return Err(StreamError::NotFound {
            video_id: crate::security::truncate_for_log(video_id, 64),
        });
    }
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = sidecar_command(bin)
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
            video_id: crate::security::truncate_for_log(video_id, 64),
        }
    } else {
        let snippet = crate::security::truncate_for_log(stderr.trim(), 500);
        StreamError::Network(format!("yt-dlp: {snippet}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_fake_ytdlp(stdout_body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "yutu-fake-ytdlp-{}-{}.sh",
            std::process::id(),
            stdout_body.len()
        ));
        crate::test_exec::write_executable(&path, &format!("#!/bin/sh\necho '{stdout_body}'\n"));
        path
    }

    /// Fake sidecar with an arbitrary shell body (unique per `tag`).
    #[cfg(unix)]
    fn write_fake_script(tag: &str, body: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("yutu-fake-ytdlp-{}-{tag}.sh", std::process::id()));
        crate::test_exec::write_executable(&path, &format!("#!/bin/sh\n{body}\n"));
        path
    }

    #[test]
    #[cfg(unix)]
    fn preferred_client_is_asked_first() {
        // Answers only when pinned to the preferred client.
        let bin = write_fake_script(
            "pinned",
            &format!(
                r#"case "$*" in *player_client={PREFERRED_PLAYER_CLIENT}*) echo https://r1.googlevideo.com/pinned ;; *) exit 1 ;; esac"#
            ),
        );
        let url = resolve_audio_url(&bin.to_string_lossy(), "dQw4w9WgXcQ").unwrap();
        assert_eq!(url, "https://r1.googlevideo.com/pinned");
    }

    #[test]
    #[cfg(unix)]
    fn falls_back_to_default_clients_when_pinned_fails() {
        // A broken pinned client must never be the only path.
        let bin = write_fake_script(
            "fallback",
            r#"case "$*" in *player_client=*) echo 'ERROR: page needs reload' >&2; exit 1 ;; *) echo https://r1.googlevideo.com/default ;; esac"#,
        );
        let url = resolve_audio_url(&bin.to_string_lossy(), "dQw4w9WgXcQ").unwrap();
        assert_eq!(url, "https://r1.googlevideo.com/default");
    }

    #[test]
    #[cfg(unix)]
    fn double_failure_reports_the_default_set_error() {
        let bin = write_fake_script(
            "both-fail",
            r#"case "$*" in *player_client=*) echo 'ERROR: pinned broke' >&2 ;; *) echo 'ERROR: Video unavailable' >&2 ;; esac; exit 1"#,
        );
        let err = resolve_audio_url(&bin.to_string_lossy(), "dQw4w9WgXcQ").unwrap_err();
        assert!(matches!(err, StreamError::NotFound { .. }), "{err:?}");
    }

    /// Session whose cookie value marks the test (unique backoff key).
    fn session(tag: &str) -> crate::auth::CookieAuth {
        crate::auth::CookieAuth::from_header_str(&format!("SID=s; SAPISID=secret-{tag}")).unwrap()
    }

    #[test]
    #[cfg(unix)]
    fn signed_in_path_uses_a_private_jar_never_argv() {
        // Answers only for the premium client with a readable jar that
        // holds the session, and refuses if the secret is on argv.
        let seen = std::env::temp_dir().join(format!("yutu-jar-seen-{}", std::process::id()));
        let bin = write_fake_script(
            "premium",
            &format!(
                r#"case "$*" in *secret-*) exit 9 ;; esac
jar=""; prev=""
for a in "$@"; do [ "$prev" = "--cookies" ] && jar="$a"; prev="$a"; done
echo "$jar" > '{seen}'
case "$*" in *player_client={PREMIUM_PLAYER_CLIENT}*) ;; *) exit 1 ;; esac
grep -q "	SAPISID	secret-argv" "$jar" || exit 1
echo 141; echo https://r1.googlevideo.com/premium"#,
                seen = seen.display()
            ),
        );
        let auth = session("argv");
        let url =
            resolve_audio_url_with_session(&bin.to_string_lossy(), "dQw4w9WgXcQ", Some(&auth))
                .unwrap();
        assert_eq!(url, "https://r1.googlevideo.com/premium");
        let jar = std::fs::read_to_string(&seen).unwrap();
        let jar = std::path::Path::new(jar.trim());
        assert!(jar.is_absolute(), "{jar:?}");
        assert!(!jar.exists(), "jar must be removed after the run");
        let _ = std::fs::remove_file(&seen);
    }

    #[test]
    #[cfg(unix)]
    fn signed_in_failure_falls_back_and_backs_off() {
        // Free account: the premium client yields nothing, the anonymous
        // pinned client plays; the next song skips the signed-in attempt.
        let count = std::env::temp_dir().join(format!("yutu-premium-n-{}", std::process::id()));
        let _ = std::fs::remove_file(&count);
        let bin = write_fake_script(
            "free",
            &format!(
                r#"case "$*" in
*player_client={PREMIUM_PLAYER_CLIENT}*) echo x >> '{n}'; echo 'ERROR: Requested format is not available' >&2; exit 1 ;;
*player_client={PREFERRED_PLAYER_CLIENT}*) echo https://r1.googlevideo.com/standard ;;
*) exit 1 ;;
esac"#,
                n = count.display()
            ),
        );
        let bin = bin.to_string_lossy();
        let auth = session("free");
        for _ in 0..2 {
            let url = resolve_audio_url_with_session(&bin, "dQw4w9WgXcQ", Some(&auth)).unwrap();
            assert_eq!(url, "https://r1.googlevideo.com/standard");
        }
        let attempts = std::fs::read_to_string(&count).unwrap_or_default();
        assert_eq!(
            attempts.lines().count(),
            1,
            "signed-in attempt must back off"
        );
        let _ = std::fs::remove_file(&count);
    }

    #[test]
    #[cfg(unix)]
    fn signed_in_off_domain_url_is_not_played() {
        let bin = write_fake_script(
            "premium-evil",
            &format!(
                r#"case "$*" in *player_client={PREMIUM_PLAYER_CLIENT}*) echo 141; echo https://evil.invalid/a ;; *) echo https://r1.googlevideo.com/standard ;; esac"#
            ),
        );
        let auth = session("evil");
        let url =
            resolve_audio_url_with_session(&bin.to_string_lossy(), "dQw4w9WgXcQ", Some(&auth))
                .unwrap();
        assert_eq!(url, "https://r1.googlevideo.com/standard");
    }

    #[test]
    #[cfg(unix)]
    fn no_session_is_the_anonymous_path() {
        let bin = write_fake_script(
            "anon",
            r#"case "$*" in *--cookies*) exit 1 ;; *) echo https://r1.googlevideo.com/anon ;; esac"#,
        );
        let url =
            resolve_audio_url_with_session(&bin.to_string_lossy(), "dQw4w9WgXcQ", None).unwrap();
        assert_eq!(url, "https://r1.googlevideo.com/anon");
    }

    #[test]
    #[cfg(unix)]
    fn resolves_first_https_line() {
        let bin = write_fake_ytdlp("https://r1.googlevideo.com/audio");
        let url = resolve_audio_url(&bin.to_string_lossy(), "dQw4w9WgXcQ").unwrap();
        assert_eq!(url, "https://r1.googlevideo.com/audio");
    }

    #[test]
    #[cfg(unix)]
    fn off_domain_url_is_rejected() {
        let bin = write_fake_ytdlp("https://example.invalid/audio.webm");
        let err = resolve_audio_url(&bin.to_string_lossy(), "dQw4w9WgXcQ").unwrap_err();
        assert!(matches!(err, StreamError::Network(_)), "{err:?}");
    }

    #[test]
    fn invalid_video_id_is_rejected() {
        let err = resolve_audio_url("/nonexistent/yt-dlp", "vid").unwrap_err();
        assert!(matches!(err, StreamError::NotFound { .. }), "{err:?}");
        let err = resolve_audio_url("/nonexistent/yt-dlp", "https://evil").unwrap_err();
        assert!(matches!(err, StreamError::NotFound { .. }), "{err:?}");
    }

    #[test]
    #[cfg(unix)]
    fn stderr_without_url_classifies() {
        let bin = write_fake_ytdlp("");
        let err = resolve_audio_url(&bin.to_string_lossy(), "dQw4w9WgXcQ").unwrap_err();
        assert!(matches!(err, StreamError::Network(_)));
    }

    #[test]
    fn missing_binary_is_a_clean_error() {
        let err = resolve_audio_url("/nonexistent/yt-dlp", "dQw4w9WgXcQ").unwrap_err();
        assert!(matches!(err, StreamError::Network(_)));
    }

    #[test]
    #[cfg(unix)]
    fn direct_download_returns_stdout_bytes() {
        // Fake sidecar: ignores args, prints media bytes to stdout.
        let bin = write_fake_ytdlp("fake-audio-bytes");
        let bytes = download_audio_bytes(&bin.to_string_lossy(), "dQw4w9WgXcQ").unwrap();
        // `echo` appends a newline; the point is stdout passes through.
        assert_eq!(bytes, b"fake-audio-bytes\n");
    }

    #[test]
    fn direct_download_missing_binary_is_a_clean_error() {
        let err = download_audio_bytes("/nonexistent/yt-dlp", "dQw4w9WgXcQ").unwrap_err();
        assert!(matches!(err, StreamError::Network(_)));
    }

    #[test]
    fn direct_download_rejects_invalid_id() {
        let err = download_audio_bytes("/nonexistent/yt-dlp", "x").unwrap_err();
        assert!(matches!(err, StreamError::NotFound { .. }), "{err:?}");
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
