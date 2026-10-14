//! Cookie auth store. Never logs secret material.

use std::path::Path;

#[cfg(feature = "innertube")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "innertube")]
use ytmapi_rs::auth::oauth::{OAuthDeviceCode, OAuthToken, OAuthTokenGenerator};
#[cfg(feature = "innertube")]
use ytmapi_rs::Client;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthState {
    #[default]
    Anonymous,
    Cookie,
}

/// Opaque cookie header value loaded from a `cookies.txt`-style file.
#[derive(Clone)]
pub struct CookieAuth {
    raw: String,
}

impl CookieAuth {
    /// Load a cookie file in either of the two real-world shapes:
    /// - Netscape `cookies.txt` (what the "Get cookies.txt" browser
    ///   extension exports: tab-separated `domain, flag, path, secure,
    ///   expiry, name, value` rows plus `#` comments), converted here to a
    ///   `name=value; ...` header string.
    /// - A raw `Cookie` header value (`SID=...; SAPISID=...; ...`), used as is.
    ///
    /// Upstream `BrowserToken` needs the header shape (it scans for
    /// `SAPISID=`), so storing the raw Netscape text would fail later with
    /// a generic "header" error. Normalising here keeps the failure local
    /// and actionable. Errors never echo cookie material.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("read cookie file: {e}"))?;
        Self::from_header_str(&raw)
    }

    pub fn state(&self) -> AuthState {
        AuthState::Cookie
    }

    /// Cookie header value for Innertube requests (Phase 4).
    pub fn header_value(&self) -> &str {
        &self.raw
    }

    /// Build directly from an already-normalised header value (used by
    /// session restore: the saved file holds exactly what [`from_file`]
    /// would have produced, so validation is the same).
    pub fn from_header_str(header: &str) -> Result<Self, String> {
        let raw = header.trim().to_string();
        if raw.is_empty() {
            return Err("cookie file is empty".into());
        }
        if looks_like_netscape(&raw) {
            let header = netscape_to_header(&raw).map_err(|e| format!("cookie file: {e}"))?;
            return Ok(Self { raw: header });
        }
        if !raw.contains('=') {
            return Err(
                "cookie file: no cookies found (export Netscape cookies.txt or a Cookie header)"
                    .into(),
            );
        }
        if !raw.contains("SAPISID=") {
            return Err(
                "cookie file: no SAPISID (log into music.youtube.com, then export while logged in)"
                    .into(),
            );
        }
        Ok(Self { raw })
    }

    /// Persist the normalised header to `path` with `0600` permissions
    /// (unix). This is the stay-signed-in session: [`from_file`] reads it
    /// back (header shape passes through). Errors name the path, never
    /// the cookie material.
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            // Lock permissions down before the secret lands: a
            // pre-existing file keeps no world-readable window.
            let f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)
                .map_err(|e| format!("write cookie session {}: {e}", path.display()))?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod cookie session {}: {e}", path.display()))?;
            {
                use std::io::Write as _;
                let mut f = f;
                f.write_all(self.raw.as_bytes())
            }
            .map_err(|e| format!("write cookie session {}: {e}", path.display()))?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(path, &self.raw)
                .map_err(|e| format!("write cookie session {}: {e}", path.display()))
        }
    }

    /// Redacted for logs.
    pub fn redacted(&self) -> &str {
        "[cookie-redacted]"
    }

    #[cfg(test)]
    pub fn test_value() -> Self {
        Self { raw: "test".into() }
    }
}

impl std::fmt::Debug for CookieAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieAuth")
            .field("raw", &"[cookie-redacted]")
            .finish()
    }
}

/// True when `raw` looks like a Netscape `cookies.txt` export rather than a
/// ready-made `Cookie` header (tab-separated rows or the generator banner).
fn looks_like_netscape(raw: &str) -> bool {
    raw.contains('\t') || raw.contains("# Netscape") || raw.contains("# HTTP Cookie")
}

/// Convert Netscape `cookies.txt` text to a `Cookie` header value
/// (`name=value; ...`). Skips `#` comments (except `#HttpOnly` rows, which
/// carry a real cookie after the marker). Errors name the problem, never the
/// cookie material.
fn netscape_to_header(text: &str) -> Result<String, String> {
    let mut pairs = Vec::new();
    for line in text.lines() {
        let mut line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#HttpOnly") {
            line = rest.trim();
            if line.is_empty() {
                continue;
            }
        } else if line.starts_with('#') {
            continue;
        }
        // Standard rows are tab-separated with 7 fields; fall back to
        // whitespace splitting for exporters that use spaces.
        let fields: Vec<&str> = if line.contains('\t') {
            line.split('\t').collect()
        } else {
            line.split_whitespace().collect()
        };
        if fields.len() < 7 {
            continue;
        }
        let name = fields[5].trim();
        // Values never contain tabs; if a row has extras, keep the first
        // value field so a stray column cannot corrupt the header.
        let value = fields[6].trim();
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        pairs.push(format!("{name}={value}"));
    }
    if pairs.is_empty() {
        return Err("no cookies found (empty Netscape file?)".into());
    }
    let header = pairs.join("; ");
    if !header.contains("SAPISID=") {
        return Err("no SAPISID (log into music.youtube.com, then export while logged in)".into());
    }
    Ok(header)
}

// ---- Google OAuth (device flow, `innertube` only) ---------------------------
// Secrecy discipline: `OAuthToken`'s own `Debug` prints only
// `"Private OAuthToken"`; the wrappers below redact everything and file
// errors never echo file contents.

/// Env vars holding the Google OAuth client credentials, in lookup order.
///
/// `ytmapi-rs` embeds no client id/secret (its docs defer to the youtui
/// README: register your own OAuth client). `YUTU_*` is checked first so yutu
/// users don't silently inherit youtui's credentials.
#[cfg(feature = "innertube")]
const OAUTH_CLIENT_ID_VARS: [&str; 2] = ["YUTU_OAUTH_CLIENT_ID", "YOUTUI_OAUTH_CLIENT_ID"];
#[cfg(feature = "innertube")]
const OAUTH_CLIENT_SECRET_VARS: [&str; 2] =
    ["YUTU_OAUTH_CLIENT_SECRET", "YOUTUI_OAUTH_CLIENT_SECRET"];

/// Serializable Google OAuth token bundle.
///
/// The file format is the wrapped [`OAuthToken`]'s own serde JSON (access +
/// refresh tokens, client id/secret, `expires_in`, request time), so it always
/// contains exactly what `OAuthToken` needs to rebuild: no parallel schema to
/// drift. Tokens auto-refresh client-side shortly before expiry (see
/// `REFRESH_S_BEFORE_EXPIRING` upstream); use
/// [`InnertubeBackend::refresh_oauth`](crate::innertube::InnertubeBackend::refresh_oauth)
/// to mint a fresh bundle and re-save it.
#[cfg(feature = "innertube")]
#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthPersist {
    token: OAuthToken,
}

#[cfg(feature = "innertube")]
impl OAuthPersist {
    /// Wrap a freshly polled token (see [`poll_google_login`]).
    pub fn from_token(token: OAuthToken) -> Self {
        Self { token }
    }

    /// Borrow the inner token for client construction.
    pub fn token(&self) -> &OAuthToken {
        &self.token
    }

    /// Clone the inner token out (e.g. after [`InnertubeBackend::refresh_oauth`](crate::innertube::InnertubeBackend::refresh_oauth)).
    pub fn to_token(&self) -> OAuthToken {
        self.token.clone()
    }

    /// Persist to `path` with `0600` permissions (unix). Errors name the path,
    /// never the token material.
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string(&self.token)
            .map_err(|e| format!("serialize oauth token: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)
                .and_then(|f| {
                    use std::io::Write as _;
                    let mut f = f;
                    f.write_all(json.as_bytes())
                })
                .map_err(|e| format!("write oauth file {}: {e}", path.display()))?;
            // Tighten pre-existing files too: `OpenOptionsExt::mode` only
            // applies at creation time.
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod oauth file {}: {e}", path.display()))?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(path, json)
                .map_err(|e| format!("write oauth file {}: {e}", path.display()))
        }
    }

    /// Load a bundle written by [`save_to_file`](Self::save_to_file). The file
    /// content is never echoed, not even on parse failure.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("read oauth file {}: {e}", path.display()))?;
        let token: OAuthToken = serde_json::from_str(&raw).map_err(|_| {
            format!(
                "parse oauth file {}: not a valid token file",
                path.display()
            )
        })?;
        Ok(Self { token })
    }

    /// Redacted for logs.
    pub fn redacted(&self) -> &str {
        "[oauth-redacted]"
    }
}

#[cfg(feature = "innertube")]
impl std::fmt::Debug for OAuthPersist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthPersist")
            .field("token", &"[oauth-redacted]")
            .finish()
    }
}

/// In-progress Google device-flow login.
///
/// Show [`verification_url`](Self::verification_url) to the user and ask them
/// to enter [`user_code`](Self::user_code); then call [`poll_google_login`]
/// repeatedly (waiting `interval` seconds between attempts: Google asks for
/// ~5s) until it returns `Ok(Some(_))` or a hard error.
#[cfg(feature = "innertube")]
#[derive(Clone)]
pub struct GoogleLogin {
    /// URL the user must visit to approve the device request.
    pub verification_url: String,
    /// Code the user must type at `verification_url`.
    pub user_code: String,
    device_code: OAuthDeviceCode,
    client: Client,
    client_id: String,
    client_secret: String,
}

#[cfg(feature = "innertube")]
impl std::fmt::Debug for GoogleLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleLogin")
            .field("verification_url", &self.verification_url)
            .field("user_code", &"[user-code-redacted]")
            .field("device_code", &"[device-code-redacted]")
            .finish()
    }
}

/// Start a Google device-flow login: requests a device/user code pair. No user
/// interaction happens here; the caller displays the returned URL + code.
///
/// Client credentials come from `YUTU_OAUTH_CLIENT_ID` /
/// `YUTU_OAUTH_CLIENT_SECRET` (falling back to the `YOUTUI_*` names
/// `ytmapi-rs` documents). They are read from the environment because
/// `ytmapi-rs` embeds no client id/secret: bring your own Google OAuth
/// client. Performs network I/O (device-code request).
#[cfg(feature = "innertube")]
pub async fn begin_google_login() -> Result<GoogleLogin, String> {
    let client_id = OAUTH_CLIENT_ID_VARS
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .ok_or_else(|| {
            "google login needs a client id: set YUTU_OAUTH_CLIENT_ID (or YOUTUI_OAUTH_CLIENT_ID)"
                .to_string()
        })?;
    let client_secret = OAUTH_CLIENT_SECRET_VARS
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .ok_or_else(|| {
            "google login needs a client secret: set YUTU_OAUTH_CLIENT_SECRET (or YOUTUI_OAUTH_CLIENT_SECRET)"
                .to_string()
        })?;
    let client = Client::new().map_err(|e| format!("oauth client setup failed: {e}"))?;
    let generator = OAuthTokenGenerator::new(&client, &client_id)
        .await
        .map_err(|e| format!("oauth device code request failed: {e}"))?;
    // Same URL shape as `ytmapi_rs::generate_oauth_code_and_url`.
    let verification_url = format!(
        "{}?user_code={}",
        generator.verification_url, generator.user_code
    );
    Ok(GoogleLogin {
        verification_url,
        user_code: generator.user_code,
        device_code: generator.device_code,
        client,
        client_id,
        client_secret,
    })
}

/// One poll attempt for [`GoogleLogin`]: exchanges the device code for tokens.
///
/// - `Ok(Some(persist))`: user approved; save `persist` via
///   [`OAuthPersist::save_to_file`] and connect with
///   [`InnertubeBackend::connect_with_oauth`](crate::innertube::InnertubeBackend::connect_with_oauth).
/// - `Ok(None)`: still pending (`authorization_pending` / `slow_down` from
///   Google); wait and retry.
/// - `Err(_)`: hard failure (`access_denied`, `expired_token`, network…);
///   the message never contains token material.
#[cfg(feature = "innertube")]
pub async fn poll_google_login(login: &GoogleLogin) -> Result<Option<OAuthPersist>, String> {
    match OAuthToken::from_code(
        &login.client,
        login.device_code.clone(),
        &login.client_id,
        &login.client_secret,
    )
    .await
    {
        Ok(token) => Ok(Some(OAuthPersist::from_token(token))),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("authorization_pending") || msg.contains("slow_down") {
                Ok(None)
            } else {
                Err(format!("google login failed: {msg}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts() {
        let c = CookieAuth::test_value();
        assert_eq!(c.redacted(), "[cookie-redacted]");
        assert_eq!(c.state(), AuthState::Cookie);
    }

    #[test]
    fn rejects_empty_file() {
        let dir = std::env::temp_dir();
        let p = dir.join("yutu-empty-cookie.txt");
        std::fs::write(&p, "   ").unwrap();
        assert!(CookieAuth::from_file(&p).is_err());
    }

    #[test]
    fn netscape_file_converts_to_header() {
        let dir = std::env::temp_dir();
        let p = dir.join("yutu-netscape-cookie.txt");
        std::fs::write(
            &p,
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tabc123\n.youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tsecret456\n",
        )
        .unwrap();
        let auth = CookieAuth::from_file(&p).expect("netscape converts");
        assert_eq!(auth.header_value(), "SID=abc123; SAPISID=secret456");
    }

    #[test]
    fn netscape_httponly_rows_convert() {
        let dir = std::env::temp_dir();
        let p = dir.join("yutu-httponly-cookie.txt");
        std::fs::write(
            &p,
            "#HttpOnly.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tabc\n#HttpOnly.youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tdef\n",
        )
        .unwrap();
        let auth = CookieAuth::from_file(&p).expect("httponly converts");
        assert!(auth.header_value().contains("SAPISID=def"));
    }

    #[test]
    fn header_file_passes_through() {
        let dir = std::env::temp_dir();
        let p = dir.join("yutu-header-cookie.txt");
        std::fs::write(&p, "SID=abc; SAPISID=def").unwrap();
        let auth = CookieAuth::from_file(&p).expect("header passes");
        assert_eq!(auth.header_value(), "SID=abc; SAPISID=def");
    }

    #[test]
    fn missing_sapisid_fails_fast_without_leaking() {
        let dir = std::env::temp_dir();
        let p = dir.join("yutu-no-sapisid-cookie.txt");
        std::fs::write(
            &p,
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tabc\n",
        )
        .unwrap();
        let err = CookieAuth::from_file(&p).unwrap_err();
        assert!(err.contains("SAPISID"), "unexpected error: {err}");
        assert!(!err.contains("abc"), "cookie material leaked: {err}");
    }

    #[test]
    fn session_round_trips_without_leaking() {
        let dir = std::env::temp_dir();
        let src = dir.join("yutu-session-src-cookie.txt");
        std::fs::write(
            &src,
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tabc\n.youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tdef\n",
        )
        .unwrap();
        let auth = CookieAuth::from_file(&src).expect("netscape loads");
        let dst = dir.join("yutu-session-saved-cookie.txt");
        let _ = std::fs::remove_file(&dst);
        auth.save_to_file(&dst).expect("session saves");
        let loaded = CookieAuth::from_file(&dst).expect("session reloads");
        assert_eq!(loaded.header_value(), "SID=abc; SAPISID=def");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "session file must be owner-only, got {mode:o}");
        }
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn debug_never_leaks_cookie_material() {
        let auth = CookieAuth::from_header_str("SID=abc; SAPISID=def").unwrap();
        let debug = format!("{auth:?}");
        assert!(!debug.contains("abc"), "SID leaked: {debug}");
        assert!(!debug.contains("def"), "SAPISID leaked: {debug}");
    }
}
