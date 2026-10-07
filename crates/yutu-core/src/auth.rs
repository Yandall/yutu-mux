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
#[derive(Debug, Clone)]
pub struct CookieAuth {
    raw: String,
}

impl CookieAuth {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("read cookie file: {e}"))?;
        let raw = raw.trim().to_string();
        if raw.is_empty() {
            return Err("cookie file is empty".into());
        }
        Ok(Self { raw })
    }

    pub fn state(&self) -> AuthState {
        AuthState::Cookie
    }

    /// Cookie header value for Innertube requests (Phase 4).
    pub fn header_value(&self) -> &str {
        &self.raw
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
/// contains exactly what `OAuthToken` needs to rebuild — no parallel schema to
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
/// repeatedly (waiting `interval` seconds between attempts — Google asks for
/// ~5s) until it returns `Ok(Some(_))` or a hard error.
#[cfg(feature = "innertube")]
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
/// `ytmapi-rs` embeds no client id/secret — bring your own Google OAuth
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
/// - `Ok(Some(persist))` — user approved; save `persist` via
///   [`OAuthPersist::save_to_file`] and connect with
///   [`InnertubeBackend::connect_with_oauth`](crate::innertube::InnertubeBackend::connect_with_oauth).
/// - `Ok(None)` — still pending (`authorization_pending` / `slow_down` from
///   Google); wait and retry.
/// - `Err(_)` — hard failure (`access_denied`, `expired_token`, network…);
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
}
