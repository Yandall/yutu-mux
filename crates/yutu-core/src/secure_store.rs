//! Secure cookie session store: OS keychain primary, 0600 file fallback.
//!
//! Rules:
//! - `SERVICE` + `ACCOUNT` are stable across releases, so app updates keep
//!   the session (the keychain lives outside the install dir).
//! - Opening the app never touches the keychain: [`cookie_session_exists`]
//!   checks marker/legacy files only, so a locked keychain cannot prompt
//!   on every launch. The keychain is read only when connecting, and the
//!   header is cached in memory afterwards so searches and background
//!   syncs do not hit D-Bus on every call.
//! - The marker (non-secret, empty file) records that the keychain holds
//!   the session. A present legacy file with no marker means the last
//!   save used the file fallback (keychain was unreachable), so loads
//!   prefer the file there: otherwise a stale keychain value would
//!   shadow the newer file.
//! - Errors never echo cookie material.

use std::path::Path;

/// Stable keychain identity (do not rename: releases share the session).
pub const COOKIE_SERVICE: &str = "yutu-mux";
/// Stable keychain identity (do not rename: releases share the session).
pub const COOKIE_ACCOUNT: &str = "cookie-session";

/// Where the session landed (honest status, never the secret).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureBackend {
    Keyring,
    File,
}

/// In-memory header cache: one keychain read per process, then memory.
/// Same lifetime as the backend clients built from it; cleared on logout.
static SESSION_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
    std::sync::OnceLock::new();

fn session_cache() -> &'static std::sync::Mutex<Option<String>> {
    SESSION_CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

fn cache_get() -> Option<String> {
    session_cache().lock().ok().and_then(|c| c.clone())
}

fn cache_set(header: &str) {
    if let Ok(mut c) = session_cache().lock() {
        *c = Some(header.to_string());
    }
}

fn cache_clear() {
    if let Ok(mut c) = session_cache().lock() {
        *c = None;
    }
}

/// Test-only force of the 0600-file path (idempotent, never unset, so
/// parallel tests cannot race). Release builds have no override: headless
/// machines use the file only via the keychain-error fallback below.
#[cfg(test)]
static TEST_FALLBACK_ONLY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Hermetic unit tests only: force the 0600-file path.
#[cfg(test)]
pub fn __test_force_fallback() {
    TEST_FALLBACK_ONLY.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn fallback_only() -> bool {
    #[cfg(test)]
    {
        if TEST_FALLBACK_ONLY.load(std::sync::atomic::Ordering::SeqCst) {
            return true;
        }
    }
    false
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(COOKIE_SERVICE, COOKIE_ACCOUNT)
        .map_err(|e| format!("secure store unavailable: {e}"))
}

/// Save the normalised header in the OS keychain.
pub fn save_to_keyring(header: &str) -> Result<(), String> {
    if header.trim().is_empty() {
        return Err("secure store save failed: empty session".into());
    }
    if fallback_only() {
        return Err("secure store disabled for tests".into());
    }
    let entry = keyring_entry()?;
    entry
        .set_password(header)
        .map_err(|e| format!("secure store save failed: {e}"))
}

/// Load the header from the OS keychain. The secret is returned, never logged.
pub fn load_from_keyring() -> Result<String, String> {
    if fallback_only() {
        return Err("secure store disabled for tests".into());
    }
    let entry = keyring_entry()?;
    entry
        .get_password()
        .map_err(|e| format!("secure store read failed: {e}"))
}

/// Delete the keychain entry. Missing entries count as deleted.
pub fn delete_from_keyring() -> Result<(), String> {
    if fallback_only() {
        return Ok(());
    }
    let entry = keyring_entry()?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("secure store delete failed: {e}")),
    }
}

fn ensure_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

fn write_marker(marker: &Path) -> Result<(), String> {
    ensure_parent(marker);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(marker)
            .and_then(|f| {
                use std::io::Write as _;
                let mut f = f;
                f.write_all(b"keyring")
            })
            .map_err(|e| format!("write session marker {}: {e}", marker.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(marker, b"keyring")
            .map_err(|e| format!("write session marker {}: {e}", marker.display()))
    }
}

fn write_fallback_file(header: &str, fallback: &Path) -> Result<(), String> {
    ensure_parent(fallback);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(fallback)
            .map_err(|e| format!("write cookie session {}: {e}", fallback.display()))?;
        std::fs::set_permissions(fallback, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod cookie session {}: {e}", fallback.display()))?;
        {
            use std::io::Write as _;
            let mut f = f;
            f.write_all(header.as_bytes())
        }
        .map_err(|e| format!("write cookie session {}: {e}", fallback.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(fallback, header)
            .map_err(|e| format!("write cookie session {}: {e}", fallback.display()))
    }
}

/// Remove a secret file, surfacing real failures (a swallowed
/// permission-denied would leave plaintext behind while the caller
/// reports success). Missing files are fine.
fn remove_secret_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove cookie session {}: {e}", path.display())),
    }
}

/// Remove the non-secret marker; failures are benign (existence still
/// reads true via the session file either way).
fn remove_marker(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Read the legacy file, migrating it into the keychain best-effort.
/// Returns the header either way; a failed migration keeps the file.
fn load_from_fallback_file(fallback: &Path, marker: &Path) -> Result<String, String> {
    let auth = crate::auth::CookieAuth::from_file(fallback).map_err(|e| {
        format!("saved cookie session failed ({e}): import your cookies file again")
    })?;
    let header = auth.header_value().to_string();
    cache_set(&header);
    // One-time migration: legacy file into the keychain, then drop the
    // plaintext. Best-effort: a failed migration keeps the file.
    if save_to_keyring(&header).is_ok() && write_marker(marker).is_ok() {
        let _ = remove_secret_file(fallback).map_err(|e| {
            crate::app_log::warn("yutu-login", format!("stale session file kept: {e}"));
        });
    }
    Ok(header)
}

/// Save the session: keychain first, 0600 file fallback when the keychain
/// is unreachable (headless, no D-Bus). On keychain success the legacy
/// file is removed so no plaintext lingers; on fallback the marker is
/// removed so existence still reads true via the file.
pub fn save_cookie_session(
    header: &str,
    fallback: &Path,
    marker: &Path,
) -> Result<SecureBackend, String> {
    if header.trim().is_empty() {
        return Err("cookie session is empty".into());
    }
    match save_to_keyring(header) {
        Ok(()) => {
            cache_set(header);
            // The marker is the restart signal: without it the app reads
            // signed-out while the keychain holds a live session. Keep a
            // file copy and fail honestly when it cannot be written.
            if let Err(m) = write_marker(marker) {
                let _ = write_fallback_file(header, fallback);
                return Err(m);
            }
            remove_secret_file(fallback)?;
            Ok(SecureBackend::Keyring)
        }
        Err(keyring_err) => {
            crate::app_log::warn(
                "yutu-login",
                format!("keychain save failed ({keyring_err}): session saved to file"),
            );
            write_fallback_file(header, fallback)?;
            cache_set(header);
            remove_marker(marker);
            Ok(SecureBackend::File)
        }
    }
}

/// Load the session, preferring the file when it is newer than the
/// keychain (legacy file present, marker absent: the last save used the
/// fallback). Otherwise the keychain first, migrating the marker and
/// dropping a stale legacy file. Errors name the problem, never the secret.
pub fn load_cookie_session(fallback: &Path, marker: &Path) -> Result<String, String> {
    if let Some(cached) = cache_get() {
        if !cached.trim().is_empty() {
            return Ok(cached);
        }
    }
    // File is newer than the keychain: the last save fell back to it.
    if fallback.exists() && !marker.exists() {
        return load_from_fallback_file(fallback, marker);
    }
    if let Ok(header) = load_from_keyring() {
        if !header.trim().is_empty() {
            cache_set(&header);
            // Only drop the legacy file once the marker is durable:
            // otherwise a restart would read signed-out with the secret
            // live in the keychain.
            if write_marker(marker).is_ok() {
                if let Err(e) = remove_secret_file(fallback) {
                    crate::app_log::warn("yutu-login", format!("stale session file kept: {e}"));
                }
            } else {
                crate::app_log::warn("yutu-login", "session marker unwritable");
            }
            return Ok(header);
        }
    }
    load_from_fallback_file(fallback, marker)
}

/// Delete the session everywhere. The keychain goes first: on a hard
/// keychain failure nothing else is touched, so the state stays
/// consistent (still signed in) and the user can retry instead of
/// stranding a live keychain entry behind a signed-out UI.
pub fn delete_cookie_session(fallback: &Path, marker: &Path) -> Result<(), String> {
    delete_from_keyring()?;
    remove_marker(marker);
    remove_secret_file(fallback)?;
    cache_clear();
    Ok(())
}

/// True when a session exists. File checks only (marker or legacy file),
/// so startup never unlocks the keychain and never prompts.
pub fn cookie_session_exists(fallback: &Path, marker: &Path) -> bool {
    marker.exists() || fallback.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn temp_paths(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let n = TEST_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("yutu-secure-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        (
            dir.join("cookie_session.txt"),
            dir.join("cookie_session.keyring"),
            dir,
        )
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fallback_save_load_round_trips_without_leaking() {
        super::__test_force_fallback();
        cache_clear();
        let (fallback, marker, dir) = temp_paths("roundtrip");
        let header = "SID=abc; SAPISID=def";
        let backend = save_cookie_session(header, &fallback, &marker).expect("fallback saves");
        assert_eq!(backend, SecureBackend::File);
        assert!(fallback.exists());
        assert!(super::cookie_session_exists(&fallback, &marker));
        cache_clear();
        let loaded = load_cookie_session(&fallback, &marker).expect("fallback loads");
        assert_eq!(loaded, header);
        delete_cookie_session(&fallback, &marker).expect("delete clears");
        assert!(!super::cookie_session_exists(&fallback, &marker));
        cleanup(&dir);
    }

    #[test]
    fn file_newer_than_keychain_wins_load() {
        super::__test_force_fallback();
        cache_clear();
        let (fallback, marker, dir) = temp_paths("precedence");
        // A legacy file with no marker means the last save used the file.
        std::fs::write(&fallback, "SID=abc; SAPISID=def").unwrap();
        let _ = std::fs::remove_file(&marker);
        let loaded = load_cookie_session(&fallback, &marker).expect("file loads");
        assert_eq!(loaded, "SID=abc; SAPISID=def");
        delete_cookie_session(&fallback, &marker).expect("delete clears");
        cleanup(&dir);
    }

    #[test]
    fn exists_never_touches_keychain_result() {
        super::__test_force_fallback();
        let (fallback, marker, dir) = temp_paths("missing");
        let _ = std::fs::remove_file(&fallback);
        let _ = std::fs::remove_file(&marker);
        assert!(!super::cookie_session_exists(&fallback, &marker));
        cleanup(&dir);
    }

    #[test]
    fn rejects_empty_header_without_leaking() {
        let (fallback, marker, dir) = temp_paths("empty");
        let err = save_cookie_session("   ", &fallback, &marker).unwrap_err();
        assert!(!err.contains("SID"));
        cleanup(&dir);
    }
}
