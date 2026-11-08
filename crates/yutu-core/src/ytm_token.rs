//! Modern-User-Agent auth tokens for `ytmapi-rs`.
//!
//! `ytmapi-rs` 0.3 hardcodes a Firefox/88 `USER_AGENT` on the initial
//! HTML page fetch. YouTube's edge now resets the HTTP/2 stream for
//! that UA, so `YtMusic::from_cookie` and `new_unauthenticated` fail
//! with a transport error even for valid cookies (API POSTs still
//! accept the old UA, so only the page fetch needs replacing). These
//! tokens do that fetch with our own reqwest client and the current
//! Chromium UA (see [`USER_AGENT`](crate::innertube::USER_AGENT)) and
//! otherwise mirror upstream header/auth behavior exactly (same
//! `SAPISIDHASH` scheme as [`raw_search`](crate::raw_search), same
//! `ytcfg` visitor parsing). They implement the public
//! [`AuthToken`](ytmapi_rs::auth::AuthToken) trait (plus
//! [`LoggedIn`](ytmapi_rs::auth::LoggedIn) for cookies, which upstream
//! leaves implementable), so every generic query keeps working with no
//! per-query changes.
//!
//! One known deviation: upstream's `deserialize_response` also rejects
//! responses whose JSON body carries an `/error.code`, but the accessor
//! it uses is crate-private upstream, so these tokens return the
//! parsed body directly. A server-side error body therefore surfaces
//! later as a downstream parse failure instead of a discriminated
//! 401/403, and genuinely expired cookies fail at the first query
//! rather than at connect (same connect-then-fail shape as upstream,
//! with a less specific message).
//!
//! Secrecy: cookie material is never logged. `ModernBrowserToken`'s
//! `Debug` prints only a redacted marker (same discipline as
//! upstream's token).

use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

use sha1::{Digest, Sha1};
use ytmapi_rs::auth::{AuthToken, LoggedIn, RawResult};
use ytmapi_rs::parse::ProcessedResult;

use crate::innertube::USER_AGENT;

const YTM_URL: &str = "https://music.youtube.com";

/// Network timeout for the initial login-page fetch: connect must fail
/// fast and explicitly instead of hanging the import task.
const PAGE_TIMEOUT_SECS: u64 = 30;

/// Marker for a "browser not supported" page (parity with the upstream
/// check; unreachable with [`USER_AGENT`], kept so a future UA rot
/// fails with words instead of a missing-marker error).
const BLOCK_PAGE_MARKER: &str = "Sorry, YouTube Music is not optimised for your browser";

/// Cookie-authed token (mirrors upstream `BrowserToken`).
#[derive(Clone)]
pub struct ModernBrowserToken {
    sapisid: String,
    client_version: String,
    cookies: String,
}

/// Anonymous token (mirrors upstream `NoAuthToken`, with the real page
/// client version instead of a date-derived fallback).
#[derive(Clone, Debug)]
pub struct ModernAnonymousToken {
    visitor_id: String,
    client_version: String,
}

impl ModernBrowserToken {
    /// Same shape as upstream `BrowserToken::from_str`, but the login
    /// page fetch uses [`USER_AGENT`]. `header` is the normalised
    /// `Cookie` value from
    /// [`CookieAuth::header_value`](crate::auth::CookieAuth::header_value).
    /// The cheap local checks run before any network I/O so a header
    /// without `SAPISID` fails fast even offline.
    pub async fn from_cookie_header(header: &str) -> Result<Self, String> {
        let cookies = header.trim().to_string();
        let sapisid =
            parse_sapisid(&cookies).ok_or_else(|| "cookie header has no SAPISID".to_string())?;
        let page = fetch_login_page(Some(&cookies)).await?;
        reject_block_page(&page)?;
        let client_version = parse_client_version(&page)
            .ok_or_else(|| "login page changed (no client version found)".to_string())?;
        Ok(Self {
            sapisid,
            client_version,
            cookies,
        })
    }
}

impl ModernAnonymousToken {
    /// Same shape as upstream `NoAuthToken::new`, but the visitor-data
    /// fetch uses [`USER_AGENT`].
    pub async fn fetch() -> Result<Self, String> {
        let page = fetch_login_page(None).await?;
        reject_block_page(&page)?;
        let client_version = parse_client_version(&page)
            .ok_or_else(|| "login page changed (no client version found)".to_string())?;
        let visitor_id = parse_visitor_id(&page)
            .ok_or_else(|| "login page changed (no visitor data found)".to_string())?;
        Ok(Self {
            visitor_id,
            client_version,
        })
    }
}

impl std::fmt::Debug for ModernBrowserToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModernBrowserToken")
            .field("token", &"[cookie-redacted]")
            .finish()
    }
}

/// Fetch the YouTube Music homepage (the same document upstream parses
/// for client version / visitor data). Errors name the failure, never
/// cookie material.
async fn fetch_login_page(cookies: Option<&str>) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(PAGE_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("login http client failed: {e}"))?;
    let mut req = client.get(YTM_URL).header("User-Agent", USER_AGENT);
    if let Some(cookies) = cookies {
        req = req.header("Cookie", cookies);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("login page request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("login page returned HTTP {status}"));
    }
    resp.text()
        .await
        .map_err(|e| format!("login page read failed: {e}"))
}

fn reject_block_page(page: &str) -> Result<(), String> {
    if page.contains(BLOCK_PAGE_MARKER) {
        return Err("login page says the browser is not supported (user agent changed?)".into());
    }
    Ok(())
}

/// `INNERTUBE_CLIENT_VERSION":"..."` from the login page (same marker
/// upstream scans for).
fn parse_client_version(page: &str) -> Option<String> {
    page.split_once("INNERTUBE_CLIENT_VERSION\":\"")?
        .1
        .split_once('"')
        .map(|(v, _)| v.to_string())
        .filter(|v| !v.is_empty())
}

/// `SAPISID` value out of a normalised `Cookie` header (pair-based, so
/// a trailing `__Secure-*-PAPISID` or a last-position `SAPISID` with
/// no trailing `;` cannot confuse it).
fn parse_sapisid(header: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == "SAPISID")
            .then(|| value.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// `VISITOR_DATA` out of the login page `ytcfg` blob (same markers
/// upstream scans for).
fn parse_visitor_id(page: &str) -> Option<String> {
    let raw = page.split_once("ytcfg.set({")?.1.split_once("})")?.0.trim();
    let mut map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&format!("{{{raw}}}")).ok()?;
    map.remove("VISITOR_DATA")
        .and_then(|v| serde_json::from_value(v).ok())
        .filter(|v: &String| !v.is_empty())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `SAPISIDHASH {ts}_{sha1(ts + " " + SAPISID + " " + origin)}`: the
/// browser auth scheme for cookie Innertube calls (same formula as
/// [`raw_search`](crate::raw_search) and upstream).
fn sapisid_hash(timestamp_secs: u64, sapisid: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{timestamp_secs} {sapisid} {YTM_URL}").into_bytes());
    format!("{timestamp_secs}_{:x}", hasher.finalize())
}

impl AuthToken for ModernBrowserToken {
    fn headers(&self) -> ytmapi_rs::error::Result<impl IntoIterator<Item = (&str, Cow<'_, str>)>> {
        let hash = sapisid_hash(now_secs(), &self.sapisid);
        Ok([
            ("User-Agent", USER_AGENT.into()),
            ("X-Origin", YTM_URL.into()),
            ("Origin", YTM_URL.into()),
            ("Content-Type", "application/json".into()),
            ("Authorization", format!("SAPISIDHASH {hash}").into()),
            ("Cookie", self.cookies.as_str().into()),
            ("Accept", "*/*".into()),
            ("Accept-Encoding", "gzip, deflate".into()),
        ])
    }

    fn client_version(&self) -> Cow<'_, str> {
        (&self.client_version).into()
    }

    fn deserialize_response<Q>(
        raw: RawResult<Q, Self>,
    ) -> ytmapi_rs::error::Result<ProcessedResult<Q>> {
        ProcessedResult::try_from(raw)
    }
}

impl LoggedIn for ModernBrowserToken {}

impl AuthToken for ModernAnonymousToken {
    fn headers(&self) -> ytmapi_rs::error::Result<impl IntoIterator<Item = (&str, Cow<'_, str>)>> {
        Ok([
            ("User-Agent", USER_AGENT.into()),
            ("X-Origin", YTM_URL.into()),
            ("X-Goog-Visitor-Id", (&self.visitor_id).into()),
            ("Content-Type", "application/json".into()),
        ])
    }

    fn client_version(&self) -> Cow<'_, str> {
        (&self.client_version).into()
    }

    fn deserialize_response<Q>(
        raw: RawResult<Q, Self>,
    ) -> ytmapi_rs::error::Result<ProcessedResult<Q>> {
        ProcessedResult::try_from(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytmapi_rs::YtMusic;

    const SAMPLE_PAGE: &str = r#"<html><head><script>ytcfg.set({"VISITOR_DATA":"CgtqZ3h4eHl6eQ%3D%3D","OTHER":1})</script></head><body data="INNERTUBE_CLIENT_VERSION":"1.20261104.00.00",done></body></html>"#;

    #[test]
    fn client_version_parses() {
        assert_eq!(
            parse_client_version(SAMPLE_PAGE).as_deref(),
            Some("1.20261104.00.00")
        );
        assert_eq!(parse_client_version("<html>no marker</html>"), None);
    }

    #[test]
    fn sapisid_parses_middle_and_last() {
        assert_eq!(
            parse_sapisid("SID=abc; SAPISID=secret; HSID=x").as_deref(),
            Some("secret")
        );
        assert_eq!(
            parse_sapisid("SID=abc; SAPISID=last-one").as_deref(),
            Some("last-one")
        );
        // Lookalike names must not match.
        assert_eq!(
            parse_sapisid("SID=abc; __Secure-1PAPISID=nope; HSID=x"),
            None
        );
        assert_eq!(parse_sapisid("SID=abc; HSID=x"), None);
        assert_eq!(parse_sapisid("SID=abc; SAPISID=; HSID=x"), None);
    }

    #[test]
    fn visitor_id_parses() {
        assert_eq!(
            parse_visitor_id(SAMPLE_PAGE).as_deref(),
            Some("CgtqZ3h4eHl6eQ%3D%3D")
        );
        assert_eq!(parse_visitor_id("<html>no ytcfg</html>"), None);
    }

    #[test]
    fn block_page_rejects() {
        assert!(reject_block_page("all good").is_ok());
        assert!(reject_block_page(
            "Sorry, YouTube Music is not optimised for your browser. Try Chrome."
        )
        .is_err());
    }

    #[test]
    fn hash_shape_matches_browser_scheme() {
        // Precomputed: sha1("0 test https://music.youtube.com").
        assert_eq!(
            sapisid_hash(0, "test"),
            "0_4d0d6459fb1015b4986bdfabf76113f41741d918"
        );
    }

    #[test]
    fn cookie_headers_carry_modern_ua_and_sapisidhash() {
        let token = ModernBrowserToken {
            sapisid: "secret".into(),
            client_version: "1.20261104.00.00".into(),
            cookies: "SID=abc; SAPISID=secret".into(),
        };
        let headers: Vec<_> = token.headers().unwrap().into_iter().collect();
        let ua = headers
            .iter()
            .find(|(k, _)| *k == "User-Agent")
            .map(|(_, v)| v.clone());
        assert_eq!(ua.as_deref(), Some(USER_AGENT));
        assert!(!ua.unwrap().contains("Firefox/88"));
        let auth = headers
            .iter()
            .find(|(k, _)| *k == "Authorization")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(auth.starts_with("SAPISIDHASH "));
        assert!(auth.contains('_'));
        assert_eq!(token.client_version(), "1.20261104.00.00");
    }

    #[test]
    fn anon_headers_carry_visitor_id_and_modern_ua() {
        let token = ModernAnonymousToken {
            visitor_id: "visitor-123".into(),
            client_version: "1.20261104.00.00".into(),
        };
        let headers: Vec<_> = token.headers().unwrap().into_iter().collect();
        let ua = headers
            .iter()
            .find(|(k, _)| *k == "User-Agent")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(ua.as_ref(), USER_AGENT);
        let visitor = headers
            .iter()
            .find(|(k, _)| *k == "X-Goog-Visitor-Id")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(visitor.as_ref(), "visitor-123");
        assert_eq!(token.client_version(), "1.20261104.00.00");
    }

    #[test]
    fn debug_never_leaks_cookie_material() {
        let token = ModernBrowserToken {
            sapisid: "secret".into(),
            client_version: "v".into(),
            cookies: "SID=abc; SAPISID=secret".into(),
        };
        let debug = format!("{token:?}");
        assert!(!debug.contains("secret"), "SAPISID leaked: {debug}");
        assert!(!debug.contains("abc"), "SID leaked: {debug}");
    }

    #[test]
    fn cookie_token_counts_as_logged_in() {
        fn needs_logged_in<A: LoggedIn>(_: &YtMusic<A>) {}
        let token = ModernBrowserToken {
            sapisid: "s".into(),
            client_version: "v".into(),
            cookies: "SID=x; SAPISID=s".into(),
        };
        needs_logged_in(&YtMusic::from_auth_token(token));
    }
}
