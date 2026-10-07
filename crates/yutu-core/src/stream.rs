//! Stream resolution: client fallback chain + PO-token errors + failure reports.
//! Phase 4 wires this to live Innertube `player` calls; the policy lives here
//! so behavior is unit-tested without network.

use std::path::Path;

/// Innertube clients we can request streams from, in fallback order.
/// Rationale (yt-dlp PO-Token guide, July 2026): `android_vr` needs no PO
/// token; `web_music` (GVS) does; `ios` needs one for player in some cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamClient {
    AndroidVr,
    WebMusic,
    Ios,
}

impl StreamClient {
    pub fn fallback_chain() -> &'static [StreamClient] {
        &[Self::AndroidVr, Self::WebMusic, Self::Ios]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AndroidVr => "android_vr",
            Self::WebMusic => "web_music",
            Self::Ios => "ios",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamRequest {
    pub video_id: String,
    pub tried: Vec<StreamClient>,
}

impl StreamRequest {
    pub fn new(video_id: &str) -> Self {
        Self {
            video_id: video_id.into(),
            tried: Vec::new(),
        }
    }

    /// Next client to try, or None when the chain is exhausted.
    pub fn next_client(&mut self) -> Option<StreamClient> {
        let next = StreamClient::fallback_chain()
            .iter()
            .find(|c| !self.tried.contains(c))
            .copied()?;
        self.tried.push(next);
        Some(next)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StreamError {
    #[error("PO token required for {client} (HTTP 403 from GVS)")]
    PoTokenRequired { client: &'static str },
    #[error("no playable stream for {video_id}")]
    NotFound { video_id: String },
    #[error("network error: {0}")]
    Network(String),
}

impl StreamError {
    /// User-facing remediation (shown in UI status, never a silent failure).
    pub fn hint(&self) -> &'static str {
        match self {
            Self::PoTokenRequired { .. } => {
                "Install rustypipe-botguard for PO tokens, or play via android_vr client. Premium accounts are exempt."
            }
            Self::NotFound { .. } => "Track unavailable (region/age/private). Try radio or another client.",
            Self::Network(_) => "Check connection, then retry. Repeated 403s mean PO-token enforcement changed.",
        }
    }
}

/// Save a failing Innertube payload for the breakage harness (like rustypipe
/// `reports/`). Never include cookies — callers must redact first.
pub fn save_report(dir: &Path, name: &str, redacted_payload: &str) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{safe}.json"));
    std::fs::write(&path, redacted_payload).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_chain_order() {
        let mut r = StreamRequest::new("abc");
        assert_eq!(r.next_client(), Some(StreamClient::AndroidVr));
        assert_eq!(r.next_client(), Some(StreamClient::WebMusic));
        assert_eq!(r.next_client(), Some(StreamClient::Ios));
        assert_eq!(r.next_client(), None);
    }

    #[test]
    fn po_token_hint_mentions_botguard() {
        let e = StreamError::PoTokenRequired {
            client: "web_music",
        };
        assert!(e.hint().contains("botguard"));
    }

    #[test]
    fn reports_round_trip() {
        let dir = std::env::temp_dir().join("yutu-report-test");
        let p = save_report(&dir, "player/abc", "{\"a\":1}").unwrap();
        assert!(std::fs::read_to_string(p).unwrap().contains("\"a\""));
    }
}
