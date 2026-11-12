//! Pure self-update tests (no network, every build config).

use super::{
    find_parent_app, is_newer_version, release_asset_for_os, update_scratch_name, AssetKind,
};

#[test]
fn newer_major_minor_patch_detected() {
    assert!(is_newer_version("0.1.0", "v0.1.1"));
    assert!(is_newer_version("0.1.0", "v0.2.0"));
    assert!(is_newer_version("0.1.0", "v1.0.0"));
    assert!(is_newer_version("0.9.9", "v0.10.0"));
}

#[test]
fn equal_or_older_never_reports_newer() {
    assert!(!is_newer_version("0.1.0", "v0.1.0"));
    assert!(!is_newer_version("0.2.0", "v0.1.9"));
    assert!(!is_newer_version("1.0.0", "v0.9.9"));
    // Missing parts read as zero, so short tags equal their padded core.
    assert!(!is_newer_version("0.1.0", "v0.1"));
    assert!(!is_newer_version("0.1", "v0.1.0"));
    // Case-insensitive `v` prefix, surrounding whitespace tolerated.
    assert!(!is_newer_version(" 0.1.0 ", "V0.1.0"));
}

#[test]
fn suffixes_never_outrank_a_higher_core() {
    // Higher core wins even with a pre-release suffix on the tag.
    assert!(is_newer_version("0.1.0", "v0.2.0-rc1"));
    // Equal cores with suffixes are not newer (never nag on rebuilds).
    assert!(!is_newer_version("0.1.0", "v0.1.0-rc1"));
    assert!(!is_newer_version("0.1.0-rc1", "v0.1.0"));
    // Build metadata ignored the same way.
    assert!(!is_newer_version("0.1.0", "v0.1.0+build5"));
}

#[test]
fn empty_or_garbage_tags_never_report_newer() {
    assert!(!is_newer_version("0.1.0", ""));
    assert!(!is_newer_version("0.1.0", "v"));
    assert!(!is_newer_version("0.1.0", "latest"));
    // Empty current reads as 0.0.0, so any real tag is newer; an empty
    // tag still never is (broken payloads must not nag).
    assert!(is_newer_version("", "v0.1.0"));
    assert!(!is_newer_version("", ""));
}

#[test]
fn asset_names_match_the_release_workflow() {
    // Names mirror packaging/install.sh + release.yml exactly.
    let linux = release_asset_for_os("v0.2.0", "linux", "x86_64").expect("linux");
    assert_eq!(linux.name, "yutu-mux-0.2.0-linux-x86_64.tar.gz");
    assert_eq!(linux.kind, AssetKind::LinuxTarball);
    assert_eq!(
        linux.url,
        "https://github.com/Yandall/yutu-mux/releases/download/v0.2.0/yutu-mux-0.2.0-linux-x86_64.tar.gz"
    );
    assert!(linux.sha_url.ends_with(".sha256"));
    let mac = release_asset_for_os("v0.2.0", "macos", "aarch64").expect("mac");
    assert_eq!(mac.name, "YutuMux-0.2.0-macos-arm64.dmg");
    assert_eq!(mac.kind, AssetKind::MacosDmg);
    let mac_intel = release_asset_for_os("v0.2.0", "macos", "x86_64").expect("intel");
    assert_eq!(mac_intel.name, "YutuMux-0.2.0-macos-x86_64.dmg");
    let win = release_asset_for_os("v0.2.0", "windows", "x86_64").expect("win");
    assert_eq!(win.name, "YutuMux-0.2.0-windows-x86_64-Setup.exe");
    assert_eq!(win.kind, AssetKind::WindowsSetup);
    // Tags without the `v` work the same (version embedded bare).
    let bare = release_asset_for_os("0.2.0", "linux", "x86_64").expect("bare");
    assert_eq!(bare.name, "yutu-mux-0.2.0-linux-x86_64.tar.gz");
}

#[test]
fn unsupported_platforms_and_tags_yield_no_asset() {
    assert!(release_asset_for_os("v0.2.0", "linux", "aarch64").is_none());
    assert!(release_asset_for_os("v0.2.0", "freebsd", "x86_64").is_none());
    assert!(release_asset_for_os("v0.2.0", "windows", "aarch64").is_none());
    assert!(release_asset_for_os("", "linux", "x86_64").is_none());
    assert!(release_asset_for_os("v", "linux", "x86_64").is_none());
}

#[test]
fn scratch_names_stay_path_safe() {
    assert_eq!(update_scratch_name("v0.2.0"), "yutu-update-v0.2.0");
    assert_eq!(update_scratch_name("../../etc"), "yutu-update-.._.._etc");
    assert_eq!(update_scratch_name(""), "yutu-update-unknown");
}

#[test]
fn parent_app_walk_finds_bundles_only() {
    // A real bundle shape resolves; plain paths never do.
    let dir = std::env::temp_dir().join(format!("yutu-app-walk-{}", std::process::id()));
    let macos = dir.join("YutuMux.app").join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    let exe = macos.join("YutuMux");
    assert_eq!(find_parent_app(&exe), Some(dir.join("YutuMux.app")));
    assert_eq!(find_parent_app(&dir.join("plain").join("bin")), None);
    // An `.app` suffix without bundle contents is not a bundle.
    let fake = dir.join("Fake.app").join("YutuMux");
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    assert_eq!(find_parent_app(&fake), None);
    let _ = std::fs::remove_dir_all(&dir);
}
