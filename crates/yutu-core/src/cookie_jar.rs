//! Short-lived private cookie jar for the yt-dlp sidecar.
//!
//! yt-dlp only takes cookies as a Netscape `cookies.txt` file
//! (`--cookies`), and it rewrites that file when it exits. So the
//! signed-in session never reaches the sidecar through the places other
//! users or logs can see, it goes through one throwaway file:
//!
//! - Never argv (`ps` shows every user's command lines) and never env.
//! - Lives in a fresh directory created `0700` with an unguessable name
//!   (an existing path, planted or not, is never reused) and the file is
//!   created `0600` with `create_new` (`O_EXCL`: no symlink is followed).
//! - Prefers `$XDG_RUNTIME_DIR` (per-user tmpfs, never written to disk)
//!   when it is private and ours, else the OS temp dir (per-user on
//!   macOS and Windows; on Linux `/tmp` the `0700` directory guards it).
//! - Only `.youtube.com` rows, `secure`, one-hour expiry: the sidecar can
//!   never send the session to another host, even via a redirect.
//! - Removed on drop (including unwinds), so it exists only while the
//!   one yt-dlp run lasts; jars a crash left behind are swept at the
//!   next creation.
//!
//! Errors never echo cookie material.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::auth::CookieAuth;

/// Directory name prefix, also the stale-sweep match.
const DIR_PREFIX: &str = "yutu-mux-ck-";
/// Jar file name inside the private directory.
const JAR_NAME: &str = "cookies.txt";
/// Leftover jars (crashed runs) older than this are removed.
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);
/// Expiry stamped on every row: long enough for one sidecar run.
const ROW_LIFETIME_SECS: u64 = 60 * 60;

/// A private on-disk cookie jar; the directory is removed on drop.
pub(crate) struct CookieJarFile {
    dir: PathBuf,
    file: PathBuf,
}

impl CookieJarFile {
    /// Write `auth` as a private Netscape jar for one sidecar run.
    pub(crate) fn create(auth: &CookieAuth) -> Result<Self, String> {
        let body = netscape_jar(auth.header_value(), now_secs() + ROW_LIFETIME_SECS)?;
        let mut last_err = String::from("no private directory available");
        for base in candidate_bases() {
            sweep_stale(&base);
            match create_in(&base, &body) {
                Ok(jar) => return Ok(jar),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Path to hand to `yt-dlp --cookies`.
    pub(crate) fn path(&self) -> &Path {
        &self.file
    }
}

impl Drop for CookieJarFile {
    fn drop(&mut self) {
        // Best effort: the sweep removes anything this misses.
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl std::fmt::Debug for CookieJarFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieJarFile")
            .field("dir", &self.dir)
            .finish()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Bases to try, most private first: the per-user runtime dir (tmpfs)
/// when it is a real `0700` directory, then the OS temp dir.
fn candidate_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    #[cfg(unix)]
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        use std::os::unix::fs::PermissionsExt;
        let private = dir.is_absolute()
            && std::fs::symlink_metadata(&dir)
                .map(|m| m.is_dir() && m.permissions().mode() & 0o077 == 0)
                .unwrap_or(false);
        if private {
            bases.push(dir);
        }
    }
    bases.push(std::env::temp_dir());
    bases
}

/// Unguessable directory name: `RandomState` keys come from the OS RNG.
fn random_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.write_u32(std::process::id());
    let a = h.finish();
    let mut h2 = std::collections::hash_map::RandomState::new().build_hasher();
    h2.write_u64(a);
    format!("{DIR_PREFIX}{a:016x}{:016x}", h2.finish())
}

fn create_in(base: &Path, body: &str) -> Result<CookieJarFile, String> {
    // A colliding (or planted) name fails `create_dir`; retry fresh.
    for _ in 0..8 {
        let dir = base.join(random_name());
        match make_private_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cookie jar dir: {e}")),
        }
        // From here the guard owns cleanup on every error path.
        let jar = CookieJarFile {
            file: dir.join(JAR_NAME),
            dir,
        };
        verify_owned(base, &jar.dir)?;
        write_private_file(&jar.file, body).map_err(|e| format!("cookie jar file: {e}"))?;
        return Ok(jar);
    }
    Err("cookie jar dir: no free name".into())
}

fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// The new directory must be a real directory, owner-only, and (in the
/// runtime dir) owned by the same user as its base: a runtime dir that
/// belongs to someone else is not private to us.
fn verify_owned(base: &Path, dir: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("cookie jar dir: {e}"))?;
    if !meta.is_dir() {
        return Err("cookie jar dir: not a directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("cookie jar dir: not private".into());
        }
        if base != std::env::temp_dir() {
            let base_meta = std::fs::metadata(base).map_err(|e| format!("cookie jar base: {e}"))?;
            if base_meta.uid() != meta.uid() {
                return Err("cookie jar base: owned by another user".into());
            }
        }
    }
    #[cfg(not(unix))]
    let _ = base;
    Ok(())
}

fn write_private_file(path: &Path, body: &str) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(body.as_bytes())?;
    f.flush()
}

/// Remove our own jars that a crashed run left behind. Only real
/// directories with our prefix and older than [`STALE_AFTER`]; symlinks
/// are never followed, and another user's directories fail to delete.
fn sweep_stale(base: &Path) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(DIR_PREFIX) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > STALE_AFTER);
        if meta.is_dir() && old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Netscape rows for a `name=value; ...` header, scoped to
/// `.youtube.com`, `secure`, expiring at `expiry` (unix seconds).
/// Pairs with a tab, newline or other control character are dropped
/// (they would forge extra rows); no usable pair is an error.
pub(crate) fn netscape_jar(header: &str, expiry: u64) -> Result<String, String> {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    let mut rows = 0;
    for pair in header.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        let clean = |s: &str| !s.chars().any(|c| c.is_control());
        if name.is_empty() || name.starts_with('#') || !clean(name) || !clean(value) {
            continue;
        }
        if name.contains(char::is_whitespace) {
            continue;
        }
        out.push_str(&format!(
            ".youtube.com\tTRUE\t/\tTRUE\t{expiry}\t{name}\t{value}\n"
        ));
        rows += 1;
    }
    if rows == 0 {
        return Err("cookie session has no usable cookies".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_scoped_to_youtube_and_secure() {
        let jar = netscape_jar("SID=a; SAPISID=b", 42).unwrap();
        let rows: Vec<&str> = jar.lines().skip(1).collect();
        assert_eq!(
            rows,
            [
                ".youtube.com\tTRUE\t/\tTRUE\t42\tSID\ta",
                ".youtube.com\tTRUE\t/\tTRUE\t42\tSAPISID\tb",
            ]
        );
    }

    #[test]
    fn control_characters_cannot_forge_rows() {
        let jar = netscape_jar(
            "SAPISID=ok; EVIL=x\n.evil.com\tTRUE\t/\tFALSE\t0\tA\tB; T\tAB=1",
            1,
        )
        .unwrap();
        assert_eq!(jar.lines().count(), 2, "{jar:?}");
        assert!(!jar.contains("evil"));
    }

    #[test]
    fn empty_header_is_an_error_without_material() {
        let err = netscape_jar(" ; =x; #c=1", 1).unwrap_err();
        assert!(!err.contains('='));
    }

    #[test]
    fn values_keep_equals_signs() {
        let jar = netscape_jar("SAPISID=a=b==", 1).unwrap();
        assert!(jar.ends_with("\tSAPISID\ta=b==\n"));
    }

    #[test]
    fn jar_is_private_and_removed_on_drop() {
        let auth = CookieAuth::from_header_str("SID=a; SAPISID=b").unwrap();
        let jar = CookieJarFile::create(&auth).unwrap();
        let path = jar.path().to_path_buf();
        let dir = path.parent().unwrap().to_path_buf();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\tSAPISID\tb"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(&path).unwrap().permissions().mode();
            let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(file_mode & 0o777, 0o600);
            assert_eq!(dir_mode & 0o777, 0o700);
        }
        assert!(format!("{jar:?}").find("SAPISID").is_none());
        drop(jar);
        assert!(!path.exists());
        assert!(!dir.exists());
    }

    #[test]
    fn planted_name_is_never_reused() {
        // Every jar gets a fresh directory, so a pre-existing path with
        // the prefix (another user's plant) is simply not used.
        let auth = CookieAuth::from_header_str("SAPISID=b").unwrap();
        let a = CookieJarFile::create(&auth).unwrap();
        let b = CookieJarFile::create(&auth).unwrap();
        assert_ne!(a.path(), b.path());
    }
}
