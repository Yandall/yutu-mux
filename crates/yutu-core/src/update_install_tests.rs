//! Install-path tests (need the `update` feature): checksum round
//! trips plus tarball/app swaps against scratch dirs. No network, no
//! touching the running binary (the Linux helper takes an explicit
//! exe path; the macOS helper takes explicit mount/app dirs).

use super::update::{
    install_linux_tarball_to, install_macos_app_from_mount, verify_sha256, Relaunch,
};
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yutu-update-test-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha256_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn checksum_round_trip_and_tamper_rejected() {
    let dir = scratch("sha");
    let asset = dir.join("pkg.tar.gz");
    std::fs::write(&asset, b"fake-bytes").unwrap();
    let sha = dir.join("pkg.tar.gz.sha256");
    std::fs::write(&sha, format!("{}  pkg.tar.gz\n", sha256_of(b"fake-bytes"))).unwrap();
    assert!(verify_sha256(&asset, &sha).is_ok());
    // Tampered payload fails closed.
    std::fs::write(&asset, b"fake-bytes!").unwrap();
    assert!(verify_sha256(&asset, &sha).is_err());
    // Missing and malformed sidecars fail closed too.
    assert!(verify_sha256(&asset, &dir.join("absent.sha256")).is_err());
    std::fs::write(&sha, "not-hex\n").unwrap();
    assert!(verify_sha256(&asset, &sha).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn linux_tarball_install_atomically_replaces_the_exe() {
    let dir = scratch("tarball");
    // Minimal release-shaped tarball: folder with the binary inside.
    let archive = dir.join("yutu-mux-9.9.9-linux-x86_64.tar.gz");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(
            &mut header,
            "yutu-mux-9.9.9-linux-x86_64/yutu-mux",
            b"new".as_slice(),
        )
        .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    let exe = dir.join("live-yutu-mux");
    std::fs::write(&exe, b"old").unwrap();
    let relaunch = install_linux_tarball_to(&archive, &exe).expect("install");
    assert!(matches!(relaunch, Relaunch::Exec(ref p) if p == &exe));
    assert_eq!(std::fs::read(&exe).unwrap(), b"new");
    // No staging droppings beside the exe.
    assert!(std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .all(|e| e.file_name() != "live-yutu-mux.update-new"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn linux_tarball_without_a_binary_is_honest() {
    let dir = scratch("tarball-empty");
    let archive = dir.join("pkg.tar.gz");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "notes.txt", b"new".as_slice())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    let exe = dir.join("live-yutu-mux");
    std::fs::write(&exe, b"old").unwrap();
    assert!(install_linux_tarball_to(&archive, &exe).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn macos_app_swap_replaces_the_bundle() {
    // Fake mount with a new bundle, fake installed old bundle.
    let dir = scratch("app");
    let mnt = dir.join("mnt");
    let new_app = mnt.join("YutuMux.app");
    std::fs::create_dir_all(new_app.join("Contents").join("MacOS")).unwrap();
    std::fs::write(
        new_app.join("Contents").join("MacOS").join("YutuMux"),
        b"new",
    )
    .unwrap();
    let parent = dir.join("Applications");
    let app = parent.join("YutuMux.app");
    std::fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
    std::fs::write(app.join("Contents").join("MacOS").join("YutuMux"), b"old").unwrap();
    let relaunch = install_macos_app_from_mount(&mnt, &app, &parent).expect("swap");
    assert!(matches!(relaunch, Relaunch::OpenApp(ref p) if p == &app));
    assert_eq!(
        std::fs::read(app.join("Contents").join("MacOS").join("YutuMux")).unwrap(),
        b"new"
    );
    // No staging/retired droppings beside the app.
    assert!(std::fs::read_dir(&parent)
        .unwrap()
        .flatten()
        .all(|e| e.file_name() == "YutuMux.app"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn macos_mount_without_a_bundle_is_honest() {
    let dir = scratch("app-empty");
    let mnt = dir.join("mnt");
    std::fs::create_dir_all(&mnt).unwrap();
    let parent = dir.join("Applications");
    let app = parent.join("YutuMux.app");
    assert!(install_macos_app_from_mount(&mnt, &app, &parent).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
