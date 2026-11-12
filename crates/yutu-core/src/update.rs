//! Self-update check: compare the running build against the latest
//! GitHub release tag (read-only API call, no download or install).
//! The pure version compare runs on every build config; the network
//! fetch lives behind the `update` feature so offline builds keep no
//! HTTP client at all.

/// Tag list endpoint (latest release only, no auth, no pagination).
pub const RELEASES_API_URL: &str = "https://api.github.com/repos/Yandall/yutu-mux/releases/latest";
/// Human landing page when a newer release exists.
pub const RELEASES_PAGE_URL: &str = "https://github.com/Yandall/yutu-mux/releases";

/// Latest release verdict for one check: the tag, where to get it,
/// and whether it is newer than the running build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub latest_tag: String,
    pub html_url: String,
    pub newer: bool,
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

/// Numeric prefix of one dot-separated version part (`"0"`, `"10rc1"`
/// reads as 10, `""` reads as 0).
fn part_number(part: &str) -> u64 {
    part.chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Fetch the latest release and compare it against `current`
/// (`CARGO_PKG_VERSION` at the call site). Auth-free: public repo
/// metadata only, 15s timeout, explicit User-Agent (the API 403s
/// anonymous clients without one).
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
    Ok(UpdateInfo {
        latest_tag: latest_tag.to_string(),
        html_url: html_url.to_string(),
        newer: is_newer_version(current, latest_tag),
    })
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
