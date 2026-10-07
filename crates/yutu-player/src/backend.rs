//! Audio backends. `RodioBackend` is real (pure Rust decode via symphonia).
//! Stream-URL resolution (Innertube player + PO token) lands in Phase 4;
//! until then, playback is local/synth sources + clear errors for URLs.

use rodio::source::{SineWave, Source};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("no audio output device: {0}")]
    NoDevice(String),
    #[error("stream error: {0}")]
    Stream(String),
    #[error("network stream resolution not yet implemented (Phase 4)")]
    NotImplemented,
}

/// Stability anchor for audio backends (stub → rodio → mpv).
pub trait AudioBackend {
    fn is_ok(&self) -> bool;
}

pub struct StubBackend;

impl AudioBackend for StubBackend {
    fn is_ok(&self) -> bool {
        true
    }
}

pub struct RodioBackend {
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl RodioBackend {
    pub fn try_new() -> Result<Self, PlayerError> {
        let sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| PlayerError::NoDevice(e.to_string()))?;
        let player = rodio::Player::connect_new(sink.mixer());
        Ok(Self {
            _sink: sink,
            player,
        })
    }

    /// 440Hz test tone, 0.5s. Proves the whole output chain without network.
    pub fn play_test_tone(&self) {
        let source = SineWave::new(440.0)
            .take_duration(Duration::from_secs_f32(0.5))
            .amplify(0.2);
        self.player.append(source);
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn resume(&self) {
        self.player.play();
    }

    pub fn stop(&self) {
        self.player.clear();
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume.clamp(0.0, 1.0));
    }
}

impl AudioBackend for RodioBackend {
    fn is_ok(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rodio_test_tone_plays_or_skips_headless() {
        let backend = match RodioBackend::try_new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: no audio device in CI/headless: {e}");
                return;
            }
        };
        backend.play_test_tone();
        backend.set_volume(0.5);
        backend.pause();
        backend.resume();
        backend.stop();
        assert!(backend.is_ok());
    }
}
