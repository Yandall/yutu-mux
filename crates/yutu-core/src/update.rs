//! Self-update: compare the running build against the latest GitHub
//! release tag, then download, verify, install and restart.
//! Pure helpers (version compare, asset pick, checksum, layout) run on
//! every build config; network and install work lives behind `update`
//! so offline builds keep no HTTP client and no file-replace machinery.

use std::path::{Path, PathBuf};

/// Tag list endpoint (latest release only, no auth, no pagination).
pub const RELEASES_API_URL: &str = "https://api.github.com/repos/Yandall/yutu-mux/releases/latest";
/// Human landing page when a newer release exists.
pub const RELEASES_PAGE_URL: &str = "https://github.com/Yandall/yutu-mux/releases";
/// Cap on release-note characters kept for the update modal (bodies can
/// run long; the dialog scrolls, but never pins megabytes).
pub const NOTES_CAP: usize = 3000;

/// Latest release verdict for one check: the tag, where to get it, an
/// excerpt of its notes, and whether it is newer than this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub latest_tag: String,
    pub html_url: String,
    pub notes: String,
    pub newer: bool,
}

/// Which release artifact updates this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// Portable Linux tarball (binary replaced in place, then re-exec).
    LinuxTarball,
    /// macOS disk image (whole .app replaced, then reopened).
    MacosDmg,
    /// Windows installer (spawned, then this process exits: the
    /// installer relaunches the app post-install).
    WindowsSetup,
}

/// One downloadable release artifact plus its checksum sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub sha_url: String,
    pub kind: AssetKind,
}

/// After a verified download lands: everything the install step needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedAsset {
    pub dir: String,
    pub asset_path: String,
    pub kind: AssetKind,
}

/// How the app comes back after an install step succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relaunch {
    /// Re-exec this path with the current arguments (Linux/macOS).
    Exec(PathBuf),
    /// Open this .app bundle (macOS whole-app replace).
    OpenApp(PathBuf),
    /// The Windows installer was spawned and relaunches the app itself.
    SpawnedInstaller,
}

/// Dot-separated numeric core of a version (`v` prefix plus any
/// pre-release/build suffix stripped).
fn version_core(v: &str) -> &str {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("")
}

/// Numeric prefix of one dot-separated version part (`"0"`, `"10rc1"`
/// reads as 10, `""` reads as 0).
fn part_number(part: &str) -> u64 {
    part.chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// True when `latest_tag` is strictly newer than `current`. Both accept
/// an optional leading `v` plus pre-release/build suffixes (compared
/// numerically on the dot-separated core, missing parts read as 0, so
/// `0.1` equals `0.1.0` and suffixes never outrank a higher core).
/// Empty or unparsable tags never report newer (a broken API payload
/// must not nag).
pub fn is_newer_version(current: &str, latest_tag: &str) -> bool {
    let latest = version_core(latest_tag);
    if latest.is_empty() {
        return false;
    }
    let mut have: Vec<u64> = version_core(current).split('.').map(part_number).collect();
    let mut want: Vec<u64> = latest.split('.').map(part_number).collect();
    let n = have.len().max(want.len()).max(1);
    have.resize(n, 0);
    want.resize(n, 0);
    want > have
}

/// Release artifact for this machine, `None` when releases ship no
/// build for it (honest fallback, never a wrong download). Pure over
/// `(os, arch)` so tests pin every platform mapping.
pub fn release_asset_for_os(tag: &str, os: &str, arch: &str) -> Option<UpdateAsset> {
    let version = tag.trim().trim_start_matches(['v', 'V']);
    if version.is_empty() {
        return None;
    }
    let (name, kind) = match (os, arch) {
        ("linux", "x86_64") => (
            format!("yutu-mux-{version}-linux-x86_64.tar.gz"),
            AssetKind::LinuxTarball,
        ),
        ("macos", "aarch64") => (
            format!("YutuMux-{version}-macos-arm64.dmg"),
            AssetKind::MacosDmg,
        ),
        ("macos", "x86_64") => (
            format!("YutuMux-{version}-macos-x86_64.dmg"),
            AssetKind::MacosDmg,
        ),
        ("windows", "x86_64") => (
            format!("YutuMux-{version}-windows-x86_64-Setup.exe"),
            AssetKind::WindowsSetup,
        ),
        _ => return None,
    };
    let base = format!("https://github.com/Yandall/yutu-mux/releases/download/{tag}");
    Some(UpdateAsset {
        url: format!("{base}/{name}"),
        sha_url: format!("{base}/{name}.sha256"),
        name,
        kind,
    })
}

/// Release artifact for the running machine (see
/// [`release_asset_for_os`]).
pub fn release_asset_for(tag: &str) -> Option<UpdateAsset> {
    release_asset_for_os(tag, std::env::consts::OS, std::env::consts::ARCH)
}

/// Filesystem-safe scratch dir name for one tag (tags come from the
/// API; anything outside a small charset becomes `_`).
pub fn update_scratch_name(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    for c in tag.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unknown");
    }
    format!("yutu-update-{out}")
}

/// Nearest enclosing `*.app` bundle for a path inside one (macOS
/// whole-app replace target), `None` otherwise. Pure so tests pin the
/// walk without touching the real install.
pub fn find_parent_app(path: &Path) -> Option<PathBuf> {
    let mut current: &Path = path;
    loop {
        if current
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
            && current.join("Contents").join("MacOS").is_dir()
        {
            return Some(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent,
            _ => return None,
        }
    }
}

/// Fetch the latest release and compare it against `current`
/// (`CARGO_PKG_VERSION` at the call site). Auth-free: public repo
/// metadata only, 15s timeout, explicit User-Agent (the API 403s
/// anonymous clients without one). Notes are capped (see
/// [`NOTES_CAP`]).
#[cfg(feature = "update")]
pub async fn fetch_update_info(current: &str) -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("yutu-mux/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("update check failed: {e}"))?;
    let value: serde_json::Value = client
        .get(RELEASES_API_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("update check failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("update check failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("update check failed: {e}"))?;
    let latest_tag = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "update check failed: no tag_name in response".to_string())?;
    let html_url = value
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(RELEASES_PAGE_URL);
    let mut notes = value
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if notes.len() > NOTES_CAP {
        notes.truncate(NOTES_CAP);
    }
    Ok(UpdateInfo {
        latest_tag: latest_tag.to_string(),
        html_url: html_url.to_string(),
        notes,
        newer: is_newer_version(current, latest_tag),
    })
}

/// Download one release artifact plus its `.sha256` sidecar into `dir`,
/// reporting `(downloaded_bytes, total_bytes)` per chunk. A missing
/// total (no `Content-Length`) still downloads; the progress UI reads
/// it as indeterminate.
#[cfg(feature = "update")]
pub async fn download_release_asset(
    asset: &UpdateAsset,
    dir: &Path,
    on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<DownloadedAsset, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("update download failed: {e}"))?;
    let asset_path = dir.join(&asset.name);
    let sha_path = dir.join(format!("{}.sha256", asset.name));
    let client = reqwest::Client::builder()
        .user_agent(concat!("yutu-mux/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("update download failed: {e}"))?;
    let mut response = client
        .get(&asset.url)
        .send()
        .await
        .map_err(|e| format!("update download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("update download failed: {e}"))?;
    let total = response.content_length();
    let mut file =
        std::fs::File::create(&asset_path).map_err(|e| format!("update download failed: {e}"))?;
    use std::io::Write as _;
    let mut downloaded: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("update download failed: {e}"))?
    {
        file.write_all(&chunk)
            .map_err(|e| format!("update download failed: {e}"))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    let sha_body = client
        .get(&asset.sha_url)
        .send()
        .await
        .map_err(|e| format!("update checksum download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("update checksum download failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("update checksum download failed: {e}"))?;
    std::fs::write(&sha_path, sha_body).map_err(|e| format!("update checksum save failed: {e}"))?;
    Ok(DownloadedAsset {
        dir: dir.to_string_lossy().into_owned(),
        asset_path: asset_path.to_string_lossy().into_owned(),
        kind: asset.kind,
    })
}

/// Verify a downloaded artifact against its `.sha256` sidecar (first
/// hex field, like the installer). Fails closed: missing, malformed,
/// or mismatched checksums never install.
#[cfg(feature = "update")]
pub fn verify_sha256(asset_path: &Path, sha_path: &Path) -> Result<(), String> {
    use sha2::Digest as _;
    let sidecar =
        std::fs::read_to_string(sha_path).map_err(|e| format!("update checksum missing: {e}"))?;
    let expected = sidecar.split_whitespace().next().unwrap_or("");
    if expected.len() < 32 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("update checksum malformed: refusing to install".into());
    }
    let bytes = std::fs::read(asset_path).map_err(|e| format!("update file unreadable: {e}"))?;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&bytes);
    let actual = hex_of(&hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(
            "update checksum mismatch: download is corrupt or tampered with, refusing to install"
                .into(),
        )
    }
}

/// Lowercase hex for a digest (no extra dep for one formatting loop).
#[cfg(feature = "update")]
fn hex_of(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// First regular file named `file_name` under `dir` (tarball layout
/// search: the binary sits one folder down).
#[cfg(feature = "update")]
fn find_file_named(dir: &Path, file_name: &str) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(top) = stack.pop() {
        let entries = std::fs::read_dir(&top).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == file_name) {
                return Some(path);
            }
        }
    }
    None
}

/// Install one verified download and report how to come back. Blocking
/// file work (runs inside a background task, never on the event
/// thread): verify, unpack/replace per artifact kind, then the caller
/// relaunches and exits. Every failure is honest (nothing half
/// replaced: the live binary is swapped atomically last).
#[cfg(feature = "update")]
pub fn install_update_asset(downloaded: &DownloadedAsset) -> Result<Relaunch, String> {
    let asset_path = Path::new(&downloaded.asset_path);
    let dir = Path::new(&downloaded.dir);
    verify_sha256(
        asset_path,
        &dir.join(format!(
            "{}.sha256",
            asset_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("pkg")
        )),
    )?;
    match downloaded.kind {
        AssetKind::LinuxTarball => install_linux_tarball_to(
            asset_path,
            &std::env::current_exe()
                .map_err(|e| format!("update install failed: current exe unknown: {e}"))?,
        ),
        AssetKind::MacosDmg => install_macos_dmg(asset_path),
        AssetKind::WindowsSetup => {
            std::process::Command::new(asset_path)
                .spawn()
                .map_err(|e| format!("update installer failed to launch: {e}"))?;
            Ok(Relaunch::SpawnedInstaller)
        }
    }
}

/// Linux tarball install into `exe_path` (production: the running
/// binary): unpack beside the download, atomically swap the binary
/// (temp + rename, so a crash never leaves a half-written exe), and
/// report a re-exec. Separated from [`install_update_asset`] only so
/// tests can target a scratch path instead of the test binary.
#[cfg(feature = "update")]
pub fn install_linux_tarball_to(asset_path: &Path, exe_path: &Path) -> Result<Relaunch, String> {
    let dir = asset_path
        .parent()
        .ok_or_else(|| "update install failed: bad download path".to_string())?;
    let pkg_dir = dir.join("pkg");
    if pkg_dir.exists() {
        std::fs::remove_dir_all(&pkg_dir).map_err(|e| format!("update install failed: {e}"))?;
    }
    std::fs::create_dir_all(&pkg_dir).map_err(|e| format!("update install failed: {e}"))?;
    let file =
        std::fs::File::open(asset_path).map_err(|e| format!("update install failed: {e}"))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    archive
        .unpack(&pkg_dir)
        .map_err(|e| format!("update install failed: unpack: {e}"))?;
    let fresh = find_file_named(&pkg_dir, "yutu-mux").ok_or_else(|| {
        "update install failed: release archive did not contain a yutu-mux binary".to_string()
    })?;
    let staging = exe_path.with_extension(format!("update-new.{}", std::process::id()));
    std::fs::copy(&fresh, &staging)
        .map_err(|e| format!("update install failed: cannot write next to the app ({e}); install the release manually"))?;
    std::fs::rename(&staging, exe_path).map_err(|e| {
        let _ = std::fs::remove_file(&staging);
        format!("update install failed: cannot replace the app ({e}); install the release manually")
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = std::fs::metadata(exe_path)
            .map_err(|e| format!("update install failed: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(exe_path, perms)
            .map_err(|e| format!("update install failed: {e}"))?;
    }
    Ok(Relaunch::Exec(exe_path.to_path_buf()))
}

/// macOS disk image install: mount read-only, copy the bundled
/// `YutuMux.app` over the running one atomically (temp + renames on
/// the same volume), unmount, and report an `open`. Needs write
/// access to the .app parent (`/Applications` needs sudo): without it
/// the error says to install the dmg by hand instead of pretending.
#[cfg(feature = "update")]
pub fn install_macos_dmg(asset_path: &Path) -> Result<Relaunch, String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("update install failed: {e}"))?;
    let app = find_parent_app(&current_exe).ok_or_else(|| {
        "update install failed: not running from an installed YutuMux.app".to_string()
    })?;
    let parent = app
        .parent()
        .ok_or_else(|| "update install failed: bad app path".to_string())?
        .to_path_buf();
    let dir = asset_path
        .parent()
        .ok_or_else(|| "update install failed: bad download path".to_string())?;
    let mnt = dir.join("mnt");
    if mnt.exists() {
        let _ = std::fs::remove_dir_all(&mnt);
    }
    std::fs::create_dir_all(&mnt).map_err(|e| format!("update install failed: {e}"))?;
    let mount_status = std::process::Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-mountpoint",
            &mnt.to_string_lossy(),
            &asset_path.to_string_lossy(),
        ])
        .status()
        .map_err(|e| format!("update install failed: hdiutil missing ({e})"))?;
    if !mount_status.success() {
        return Err("update install failed: could not mount the disk image".into());
    }
    let result = install_macos_app_from_mount(&mnt, &app, &parent);
    let _ = std::process::Command::new("hdiutil")
        .args(["detach", &mnt.to_string_lossy()])
        .status();
    result
}

/// Copy the mounted `YutuMux.app` over `app` (temp + renames, same
/// volume so the swap is atomic), split out for unit tests that
/// bypass the mount.
#[cfg(feature = "update")]
pub(crate) fn install_macos_app_from_mount(
    mnt: &Path,
    app: &Path,
    parent: &Path,
) -> Result<Relaunch, String> {
    let src = mnt.join("YutuMux.app");
    if !src.is_dir() {
        return Err("update install failed: disk image did not contain YutuMux.app".into());
    }
    let staging = parent.join(format!(".YutuMux.app.update-new.{}", std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| format!("update install failed: {e}"))?;
    }
    let copy_status = std::process::Command::new("cp")
        .args(["-R", &src.to_string_lossy(), &staging.to_string_lossy()])
        .status()
        .map_err(|e| format!("update install failed: {e}"))?;
    if !copy_status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("update install failed: cannot stage the new app (needs write access to the Applications folder; install the dmg by hand)".into());
    }
    let retired = parent.join(format!(".YutuMux.app.retired.{}", std::process::id()));
    if let Err(e) = std::fs::rename(app, &retired) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!(
            "update install failed: cannot replace the app ({e}); install the dmg by hand"
        ));
    }
    if let Err(e) = std::fs::rename(&staging, app) {
        let _ = std::fs::rename(&retired, app);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!(
            "update install failed: cannot replace the app ({e}); install the dmg by hand"
        ));
    }
    let _ = std::fs::remove_dir_all(&retired);
    Ok(Relaunch::OpenApp(app.to_path_buf()))
}

/// Come back after an install: re-exec `exe` with the current
/// arguments (detached), then the caller exits. Windows never reaches
/// here (the installer relaunches the app itself).
#[cfg(feature = "update")]
pub fn relaunch_at(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("update restart failed: {e}"))
}

/// Open one path with the platform default handler (macOS `open` for
/// the replaced .app bundle).
#[cfg(feature = "update")]
pub fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("update restart failed: {e}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("update restart failed: unsupported platform".into())
    }
}

/// Live release lookup (needs network + a published release): verifies
/// the tag parses and the compare runs against this build. Ignored by
/// default (`-- --ignored` with `--features update`).
#[cfg(all(test, feature = "update"))]
#[tokio::test]
#[ignore]
async fn live_release_lookup_parses() {
    let info = fetch_update_info(env!("CARGO_PKG_VERSION"))
        .await
        .expect("release lookup");
    assert!(!info.latest_tag.is_empty());
    assert_eq!(
        info.newer,
        is_newer_version(env!("CARGO_PKG_VERSION"), &info.latest_tag)
    );
}
