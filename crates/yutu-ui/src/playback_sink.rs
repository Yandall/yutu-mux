//! yutu-ui playback_sink: split from lib.rs (no behavior change).

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use pointing_slider::pointing_slider;
#[allow(unused_imports)]
use row_button::row_button;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use yutu_core::{
    forgotten_favorites, history_section, should_scrobble, weekly_mix, Album, Artist, HistoryEntry,
    HistorySection, LibraryStore, LyricsProvider, LyricsResult, Playlist, PlaylistHit,
    QueueSnapshot, SearchAll, SearchResults, TopKind, Track,
};
#[cfg(feature = "mpv")]
#[allow(unused_imports)]
use yutu_player::MpvBackend;
#[allow(unused_imports)]
use yutu_player::{EqPreset, Queue, RodioBackend, BAND_COUNT, MAX_GAIN_DB, MIN_GAIN_DB};

/// Playback sink: mpv when the `mpv` feature is on and libmpv initializes
/// (real seek + position + audible EQ), otherwise rodio (pure Rust).
/// The UI only talks to this enum: never to a concrete backend.
pub(crate) enum AudioOut {
    Rodio(RodioBackend),
    #[cfg(feature = "mpv")]
    Mpv(MpvBackend),
}

impl AudioOut {
    pub(crate) fn try_new() -> Option<Self> {
        #[cfg(feature = "mpv")]
        if let Ok(mpv) = MpvBackend::new() {
            return Some(Self::Mpv(mpv));
        }
        RodioBackend::try_new().ok().map(Self::Rodio)
    }

    /// True when this backend plays a URL progressively (mpv). Rodio needs
    /// full bytes, so the UI fetches first on that path.
    pub(crate) fn wants_stream(&self) -> bool {
        match self {
            Self::Rodio(_) => false,
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => true,
        }
    }

    /// Hand mpv a stream URL (no fetch). Rodio cannot do this.
    pub(crate) fn play_stream_url(&self, url: &str) -> Result<(), String> {
        let _ = url;
        match self {
            Self::Rodio(_) => Err("progressive streaming needs the mpv backend".into()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a
                .load_url(url)
                .and_then(|()| a.play())
                .map_err(|e| e.to_string()),
        }
    }

    pub(crate) fn backend_name(&self) -> &'static str {
        match self {
            Self::Rodio(_) => "rodio",
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => "mpv",
        }
    }

    /// Play fetched bytes (resolve + fetch run in Iced tasks).
    pub(crate) fn play_track_bytes(&self, bytes: Vec<u8>) -> Result<(), String> {
        match self {
            Self::Rodio(a) => a.play_bytes(bytes).map_err(|e| e.to_string()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a
                .load_bytes(&bytes)
                .and_then(|()| a.play())
                .map_err(|e| e.to_string()),
        }
    }

    pub(crate) fn pause(&self) {
        match self {
            Self::Rodio(a) => a.pause(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.pause();
            }
        }
    }

    pub(crate) fn resume(&self) {
        match self {
            Self::Rodio(a) => a.resume(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.play();
            }
        }
    }

    pub(crate) fn stop(&self) {
        match self {
            Self::Rodio(a) => a.stop(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.stop();
            }
        }
    }

    pub(crate) fn set_volume(&self, volume: f32) {
        match self {
            Self::Rodio(a) => a.set_volume(volume),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.set_volume(volume);
            }
        }
    }

    pub(crate) fn position_secs(&self) -> f64 {
        match self {
            Self::Rodio(a) => a.position_secs(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.position_secs(),
        }
    }

    /// Backend duration when it knows better (mpv), else `fallback`.
    pub(crate) fn duration_secs(&self, fallback: f64) -> f64 {
        #[cfg(feature = "mpv")]
        if let Self::Mpv(a) = self {
            let d = a.duration_secs();
            if d > 1.0 {
                return d;
            }
        }
        fallback
    }

    /// True when the backend holds no file (mpv idle after EOF/stop).
    /// Rodio has no idle signal (wall-clock only), so it never reports
    /// idle and keeps the clock-only EOF behavior exactly as before.
    pub(crate) fn is_idle(&self) -> bool {
        match self {
            Self::Rodio(_) => false,
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.is_idle(),
        }
    }

    /// Draggable seek: real on mpv, unsupported on rodio.
    pub(crate) fn seek_secs(&self, secs: f64) -> Result<(), String> {
        match self {
            Self::Rodio(a) => a.seek_secs(secs).map_err(|e| e.to_string()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.seek_secs(secs).map_err(|e| e.to_string()),
        }
    }

    /// Rodio decodes to raw PCM with no filter stage: EQ is mpv-only.
    /// Callers report this honestly instead of pretending it applied.
    pub(crate) fn supports_eq(&self) -> bool {
        match self {
            Self::Rodio(_) => false,
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => true,
        }
    }

    pub(crate) fn apply_eq(&self, preset: &EqPreset) -> Result<(), String> {
        #[cfg(not(feature = "mpv"))]
        let _ = preset;
        match self {
            Self::Rodio(_) => Err("EQ needs the mpv backend (libmpv)".into()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.apply_eq_preset(preset).map_err(|e| e.to_string()),
        }
    }
}
