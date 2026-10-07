//! yutu-desktop: OS integration (MPRIS on Linux, Now Playing notes for macOS).
//! Implemented in the desktop-integration track; this stub keeps the
//! workspace green until then.

/// MPRIS identity advertised on D-Bus.
pub const MPRIS_IDENTITY: &str = "yutu-mux";

/// Metadata mapped to MPRIS fields; pure data for unit tests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_id: String,
    pub length_secs: u32,
}

impl NowPlaying {
    pub fn mpris_track_id(&self) -> String {
        format!("/yutu_mux/{}", self.track_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_id_format() {
        let n = NowPlaying {
            track_id: "m1".into(),
            ..Default::default()
        };
        assert_eq!(n.mpris_track_id(), "/yutu_mux/m1");
    }
}
