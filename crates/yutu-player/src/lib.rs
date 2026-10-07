//! yutu-player: queue state machine + audio backends.
//! v1: rodio (pure Rust). v2 (Phase 5): mpv backend behind the same trait.

pub mod backend;
pub mod queue;

pub use backend::{AudioBackend, PlayerError, RodioBackend, StubBackend};
pub use queue::{Queue, RepeatMode};
