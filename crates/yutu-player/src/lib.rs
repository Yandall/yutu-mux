//! yutu-player: queue state machine + audio backends.
//! v1: rodio (pure Rust). v2 (Phase 5): mpv backend behind the same trait.

pub mod backend;
pub mod controller;
pub mod eq;
pub mod local;
#[cfg(feature = "mpv")]
pub mod mpv;
pub mod queue;

pub use backend::{
    decode_bytes, fetch_bytes, AudioBackend, PlayerError, RodioBackend, StubBackend,
};
pub use controller::{PlaybackState, Player};
pub use eq::{EqBand, EqError, EqPreset, BAND_COUNT, EQ_FREQUENCIES, MAX_GAIN_DB, MIN_GAIN_DB};
pub use local::{scan_folder, AUDIO_EXTENSIONS};
#[cfg(feature = "mpv")]
pub use mpv::MpvBackend;
pub use queue::{Queue, RepeatMode};
