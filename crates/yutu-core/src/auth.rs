//! Cookie auth store. Never logs secret material.

use std::path::Path;

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
