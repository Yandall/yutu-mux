//! PO-token sidecar caller (`rustypipe-botguard`).
//!
//! YouTube requires Proof-of-Origin tokens for web-client streams (see
//! `docs/PO_TOKEN.md`). The BotGuard VM is too heavy to embed, so the
//! official pattern (same as RustyPipe itself) is a sidecar binary: we call
//! it with an identifier, it prints space-separated tokens plus
//! `key=value` metadata (`valid_until`, `from_snapshot`).
//!
//! Std-only, no async: callers run this in `spawn_blocking` (it solves a
//! challenge and can take seconds on a cold snapshot).

use std::path::{Path, PathBuf};

pub const BOTGUARD_BIN_NAME: &str = "rustypipe-botguard";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoToken {
    /// One content-bound token (player request) + one session-bound token
    /// (stream `pot` param), in sidecar output order.
    pub tokens: Vec<String>,
    pub valid_until: Option<i64>,
    pub from_snapshot: bool,
}

/// Locate the sidecar: `PATH` first, then the current working directory.
/// Returns `None` when the user has not installed it (callers then fall back
/// to token-free clients and surface [`crate::stream::StreamError::hint`]).
pub fn find_botguard_bin() -> Option<String> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(BOTGUARD_BIN_NAME);
            if is_executable_file(&candidate) {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    let cwd = std::env::current_dir()
        .map(|d| d.join(BOTGUARD_BIN_NAME))
        .unwrap_or_else(|_| PathBuf::from(BOTGUARD_BIN_NAME));
    if is_executable_file(&cwd) {
        return Some(cwd.to_string_lossy().into_owned());
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
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

/// Mint PO token(s) for `identifier` (video ID for content-bound tokens,
/// visitor-data for session-bound ones).
///
/// The `--` separator is mandatory: video IDs may start with `-` and would
/// otherwise parse as CLI flags.
pub fn fetch_po_token(bin: &str, identifier: &str) -> Result<PoToken, String> {
    let output = crate::ytdlp::sidecar_command(bin)
        .arg("--")
        .arg(identifier)
        .output()
        .map_err(|e| format!("run {bin}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{bin} failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_po_output(&stdout)
}

/// Parse sidecar stdout: space-separated tokens, then `key=value` metadata.
/// Unknown metadata keys are ignored (forward compatibility).
///
/// Tokens are base64(ish) and routinely *end* with `=` padding, so an item is
/// only treated as metadata when the part before the first `=` is a short
/// identifier (`valid_until`, `from_snapshot`, …). Anything else (including
/// strings containing `=`) is a token.
pub fn parse_po_output(out: &str) -> Result<PoToken, String> {
    let mut tokens = Vec::new();
    let mut valid_until = None;
    let mut from_snapshot = false;
    for item in out.split_whitespace() {
        match item.split_once('=') {
            Some((key, value)) if is_metadata_key(key) => match key {
                "valid_until" => {
                    valid_until = value.parse::<i64>().ok();
                }
                "from_snapshot" => {
                    from_snapshot = value == "true";
                }
                _ => {}
            },
            _ => tokens.push(item.to_string()),
        }
    }
    if tokens.is_empty() {
        return Err("sidecar returned no tokens".into());
    }
    Ok(PoToken {
        tokens,
        valid_until,
        from_snapshot,
    })
}

fn is_metadata_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape of a real sidecar response (tokens redacted, metadata kept).
    const SAMPLE: &str = "MnRUOKENT0KENt6x0HYKon9J8I-SCEWj3aOQKzLam6LXdLRXW3ypcXHdygoyg4slY9Z9kW0HXdWGKbBep7vWCgE2rjynS1YrhUJYiQfCZTS7FqeJz3MEFIg== MnR3PvWN5T_RRHcCYSMusEHb5MIJmbaZae9TQkssf4Zzht_2ooYgESn5F2OmBM33_hgi3597PoQMtwAofrz5HIyLICAfocSxBZCT1frOBEoXUIOMeXjtctrmfccXNJn9kFq0IvboUu2uKbAYQVVKjHa4gxmoSQ== valid_until=1791428867 from_snapshot=false";

    #[test]
    fn parses_real_sidecar_shape() {
        let tok = parse_po_output(SAMPLE).unwrap();
        assert_eq!(tok.tokens.len(), 2);
        assert_eq!(tok.valid_until, Some(1791428867));
        assert!(!tok.from_snapshot);
    }

    #[test]
    fn rejects_empty_and_stderr_only() {
        assert!(parse_po_output("").is_err());
        assert!(parse_po_output("valid_until=123 from_snapshot=true").is_err());
    }

    #[test]
    fn ignores_unknown_metadata() {
        let tok = parse_po_output("AAA future_key=zzz valid_until=7").unwrap();
        assert_eq!(tok.tokens, vec!["AAA"]);
        assert_eq!(tok.valid_until, Some(7));
    }

    #[cfg(unix)]
    fn write_fake_sidecar(body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "yutu-fake-botguard-{}-{}.sh",
            std::process::id(),
            body.len()
        ));
        std::fs::write(&path, format!("#!/bin/sh\necho '{body}'\n")).unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&path, perm).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn fetches_via_fake_binary() {
        let bin = write_fake_sidecar(SAMPLE);
        let tok = fetch_po_token(&bin.to_string_lossy(), "any-id").unwrap();
        assert_eq!(tok.tokens.len(), 2);
        assert_eq!(tok.valid_until, Some(1791428867));
    }

    #[test]
    fn missing_binary_is_a_clean_error() {
        let err = fetch_po_token("/nonexistent/rustypipe-botguard", "x").unwrap_err();
        assert!(err.contains("run /nonexistent"));
    }

    #[test]
    #[ignore = "live: requires installed rustypipe-botguard + network"]
    fn live_mints_token_for_known_video() {
        let Some(bin) = find_botguard_bin() else {
            eprintln!("SKIP: rustypipe-botguard not in PATH");
            return;
        };
        let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "dQw4w9WgXcQ".into());
        let tok = fetch_po_token(&bin, &video).expect("mint PO token");
        assert!(!tok.tokens.is_empty());
    }
}
