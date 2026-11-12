//! Pure version-compare tests (no network, every build config).

use super::is_newer_version;

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
