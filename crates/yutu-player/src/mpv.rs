//! Gapless-capable mpv audio backend (opt-in `mpv` feature).
//!
//! License note: this module talks to libmpv, which is licensed LGPL-2.1+
//! and provided as a dynamically linked system library (`/usr/lib/libmpv.so`,
//! it is NOT vendored and no libmpv source is included here). All code in
//! this file remains MIT like the rest of the workspace, but a binary built
//! with `--features mpv` links a copyleft (LGPL) library at runtime — the
//! same opt-in pattern this project previously used for GPL-adjacent code:
//! default builds stay pure MIT with no libmpv dependency, and only an
//! explicit `mpv` feature flag pulls the LGPL link in.
//!
//! Backend notes (v1):
//! - Synchronous, small API surface; no background threads are spawned (mpv
//!   runs its own internal playback core, but we never poll events in v1).
//! - Gapless transitions: the handle is created with `gapless-audio=yes` so
//!   consecutive `loadfile ... append` playlist additions play gaplessly.
//!   v1 `load_url`/`load_file` use `replace` (single-track) semantics;
//!   true queued-gapless playback is future work.
//! - Headless: video output is disabled (`vo=null`, `audio-display=no`); mpv
//!   is used as an audio engine only.
//! - Volume mapping: caller range `0.0..=1.0` is clamped and scaled to mpv's
//!   `volume` property range `0..=100` (see [`volume_to_mpv`]).
//! - EQ mapping: the 10 bands of [`crate::eq::EqPreset`] map to one mpv `af`
//!   filter chain of 10 FFmpeg `equalizer` biquad sections, one per band:
//!   `equalizer=f={FREQ}:t=h:w=1:g={GAIN}` joined with commas and applied via
//!   the `af` property (see [`af_chain_for_gains`]). This reuses the exact
//!   format of [`crate::eq::EqPreset::to_mpv_af_chain`]. Approximation note:
//!   each section is a narrow peaking filter at the band center frequency, so
//!   the chain approximates — but does not exactly reproduce — an octave-wide
//!   graphic EQ; gains are defensively clamped to +/-12 dB because `EqPreset`
//!   fields are public and may bypass validation. Applying a preset replaces
//!   the whole `af` chain and may cause a brief audible re-init glitch.
//! - `load_bytes` spills audio to a temp file (`std::env::temp_dir`) because
//!   libmpv loads from paths/URLs, not memory; spilled files are removed
//!   best-effort on `stop`/`Drop`.

use crate::backend::{AudioBackend, PlayerError};
use crate::eq::{EqPreset, BAND_COUNT, EQ_FREQUENCIES, MAX_GAIN_DB, MIN_GAIN_DB};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Clamp a caller volume (`0.0..=1.0`) and scale it to mpv's `volume`
/// property range (`0..=100`). Non-finite input maps to silence (0.0).
#[must_use]
pub fn volume_to_mpv(volume: f32) -> f64 {
    let clamped = if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    };
    f64::from(clamped) * 100.0
}

/// Clamp one EQ gain to the valid +/-12 dB range. Non-finite input maps to
/// `0.0 dB` (no cut/boost) instead of propagating NaN into the filter chain.
#[must_use]
pub fn clamp_eq_gain(gain_db: f32) -> f32 {
    if !gain_db.is_finite() {
        return 0.0;
    }
    gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB)
}

/// Build the mpv `af` filter chain for 10 band gains (order matches
/// [`crate::eq::EQ_FREQUENCIES`]). Output equals
/// [`crate::eq::EqPreset::to_mpv_af_chain`] for validated presets; out-of-range
/// gains are clamped via [`clamp_eq_gain`].
#[must_use]
pub fn af_chain_for_gains(gains: &[f32; BAND_COUNT]) -> String {
    EQ_FREQUENCIES
        .iter()
        .zip(gains.iter())
        .map(|(freq, gain)| format!("equalizer=f={freq}:t=h:w=1:g={}", clamp_eq_gain(*gain)))
        .collect::<Vec<_>>()
        .join(",")
}

fn mpv_op_err(op: &str, err: libmpv2::Error) -> PlayerError {
    PlayerError::Stream(format!("mpv {op} failed: {err}"))
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Spill in-memory audio to a uniquely named temp file for `loadfile`.
fn spill_temp_bytes(bytes: &[u8]) -> Result<PathBuf, PlayerError> {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("yutu-mux-{}-{n}.media", std::process::id()));
    std::fs::write(&path, bytes)
        .map_err(|e| PlayerError::Stream(format!("mpv temp spill failed: {e}")))?;
    Ok(path)
}

/// Gapless-capable audio backend over libmpv (`--features mpv` only).
pub struct MpvBackend {
    mpv: libmpv2::Mpv,
    temp_files: Mutex<Vec<PathBuf>>,
}

impl MpvBackend {
    /// Create and initialize a headless mpv instance (`vo=null`,
    /// `gapless-audio=yes`). Returns `Err` (never panics) when mpv cannot be
    /// created/initialized — e.g. headless environments or a broken install.
    pub fn new() -> Result<Self, PlayerError> {
        let mpv = libmpv2::Mpv::with_initializer(|init| {
            init.set_option("vo", "null")?;
            init.set_option("audio-display", "no")?;
            init.set_option("gapless-audio", "yes")?;
            Ok(())
        })
        .map_err(|e| PlayerError::NoDevice(format!("mpv init failed: {e}")))?;
        Ok(Self {
            mpv,
            temp_files: Mutex::new(Vec::new()),
        })
    }

    /// Load a stream URL or local path, replacing the current playlist entry.
    /// Playback starts unless the player is paused (see [`MpvBackend::play`]).
    pub fn load_url(&self, url: &str) -> Result<(), PlayerError> {
        self.mpv
            .command("loadfile", &[url, "replace"])
            .map_err(|e| mpv_op_err("loadfile", e))
    }

    /// Load a local file path, replacing the current playlist entry.
    pub fn load_file(&self, path: &str) -> Result<(), PlayerError> {
        self.load_url(path)
    }

    /// Load in-memory audio by spilling it to a temp file and loading that.
    /// Spilled files are tracked and removed best-effort on `stop`/`Drop`.
    pub fn load_bytes(&self, bytes: &[u8]) -> Result<(), PlayerError> {
        let path = spill_temp_bytes(bytes)?;
        let path_str = path.to_string_lossy().into_owned();
        let result = self.load_url(&path_str);
        match result {
            Ok(()) => {
                if let Ok(mut guard) = self.temp_files.lock() {
                    guard.push(path);
                }
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        }
    }

    /// Resume playback.
    pub fn play(&self) -> Result<(), PlayerError> {
        self.mpv
            .set_property("pause", false)
            .map_err(|e| mpv_op_err("play", e))
    }

    /// Pause playback (position retained).
    pub fn pause(&self) -> Result<(), PlayerError> {
        self.mpv
            .set_property("pause", true)
            .map_err(|e| mpv_op_err("pause", e))
    }

    /// Stop playback, clear the playlist, and remove spilled temp files.
    pub fn stop(&self) -> Result<(), PlayerError> {
        let result = self
            .mpv
            .command("stop", &[])
            .map_err(|e| mpv_op_err("stop", e));
        self.cleanup_temp_files();
        result
    }

    /// Absolute seek in seconds. Negative values clamp to `0.0`; non-finite
    /// values are rejected.
    pub fn seek_secs(&self, secs: f64) -> Result<(), PlayerError> {
        if !secs.is_finite() {
            return Err(PlayerError::Stream(format!(
                "mpv seek failed: non-finite position {secs}"
            )));
        }
        let arg = secs.max(0.0).to_string();
        self.mpv
            .command("seek", &[arg.as_str(), "absolute"])
            .map_err(|e| mpv_op_err("seek", e))
    }

    /// Set volume (`0.0..=1.0`, clamped) mapped to mpv's `0..=100` range.
    pub fn set_volume(&self, volume: f32) -> Result<(), PlayerError> {
        self.mpv
            .set_property("volume", volume_to_mpv(volume))
            .map_err(|e| mpv_op_err("set volume", e))
    }

    /// Apply a 10-band EQ preset as the mpv `af` equalizer chain (replaces any
    /// existing audio filters). See the module docs for the mapping.
    pub fn apply_eq_preset(&self, preset: &EqPreset) -> Result<(), PlayerError> {
        let chain = af_chain_for_gains(&preset.bands);
        self.mpv
            .set_property("af", chain.as_str())
            .map_err(|e| mpv_op_err("apply EQ", e))
    }

    /// Current playback position in seconds (`time-pos` property).
    /// Returns `0.0` when mpv reports an error (idle, no file loaded),
    /// a negative value, or a non-finite value.
    #[must_use]
    pub fn position_secs(&self) -> f64 {
        self.mpv
            .get_property::<f64>("time-pos")
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(0.0)
    }

    /// Current track duration in seconds (`duration` property).
    /// Returns `0.0` when mpv reports an error (idle, no file loaded),
    /// a negative value, or a non-finite value.
    #[must_use]
    pub fn duration_secs(&self) -> f64 {
        self.mpv
            .get_property::<f64>("duration")
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(0.0)
    }

    /// `true` when audio is currently playing (not paused and not idle).
    /// Property read failures conservatively report `false`.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        let paused = self.mpv.get_property::<bool>("pause").unwrap_or(true);
        if paused {
            return false;
        }
        let idle = self.mpv.get_property::<bool>("idle-active").unwrap_or(true);
        !idle
    }

    fn cleanup_temp_files(&self) {
        if let Ok(mut guard) = self.temp_files.lock() {
            for path in guard.drain(..) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

impl Drop for MpvBackend {
    fn drop(&mut self) {
        self.cleanup_temp_files();
    }
}

impl AudioBackend for MpvBackend {
    fn is_ok(&self) -> bool {
        // The mpv handle is owned by `self`; it is alive until `Drop`.
        true
    }

    fn position_secs(&self) -> f64 {
        MpvBackend::position_secs(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eq::EqPreset;

    #[test]
    fn volume_mapping_clamps_and_scales() {
        assert_eq!(volume_to_mpv(0.0), 0.0);
        assert_eq!(volume_to_mpv(1.0), 100.0);
        assert_eq!(volume_to_mpv(0.5), 50.0);
        assert_eq!(volume_to_mpv(-0.25), 0.0);
        assert_eq!(volume_to_mpv(2.0), 100.0);
        assert_eq!(volume_to_mpv(f32::NAN), 0.0);
        assert_eq!(volume_to_mpv(f32::INFINITY), 0.0);
        // Spot-check linearity.
        assert!((volume_to_mpv(0.2) - 20.0).abs() < 1e-6);
    }

    #[test]
    fn eq_gain_clamping() {
        assert_eq!(clamp_eq_gain(0.0), 0.0);
        assert_eq!(clamp_eq_gain(12.0), 12.0);
        assert_eq!(clamp_eq_gain(-12.0), -12.0);
        assert_eq!(clamp_eq_gain(99.0), 12.0);
        assert_eq!(clamp_eq_gain(-99.0), -12.0);
        assert_eq!(clamp_eq_gain(f32::NAN), 0.0);
        assert_eq!(clamp_eq_gain(f32::INFINITY), 0.0);
        assert_eq!(clamp_eq_gain(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn af_chain_shape_matches_eq_module() {
        for preset in EqPreset::builtin_presets() {
            let chain = af_chain_for_gains(&preset.bands);
            // Same mapping as the canonical `EqPreset` helpers.
            assert_eq!(chain, preset.to_mpv_af_chain(), "preset {}", preset.name);
            assert_eq!(chain.split(',').count(), BAND_COUNT);
        }
        let flat = af_chain_for_gains(&EqPreset::flat().bands);
        assert!(flat.starts_with("equalizer=f=31:t=h:w=1:g=0"));
        for freq in EQ_FREQUENCIES {
            assert!(flat.contains(&format!("f={freq}:")), "missing band {freq}");
        }
    }

    #[test]
    fn af_chain_clamps_out_of_range_gains() {
        let mut bands = [0.0f32; BAND_COUNT];
        bands[0] = 99.0;
        bands[9] = -99.0;
        let chain = af_chain_for_gains(&bands);
        let segs: Vec<&str> = chain.split(',').collect();
        assert_eq!(segs.len(), BAND_COUNT);
        assert!(segs[0].ends_with("g=12"), "got {}", segs[0]);
        assert!(segs[9].ends_with("g=-12"), "got {}", segs[9]);
    }

    /// Minimal 0.1s mono 16-bit 44.1kHz WAV, built by hand (no deps).
    fn wav_bytes() -> Vec<u8> {
        let rate: u32 = 44_100;
        let n: usize = (rate as usize) / 10;
        let mut data = Vec::with_capacity(44 + n * 2);
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&((36 + n * 2) as u32).to_le_bytes());
        data.extend_from_slice(b"WAVEfmt ");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&rate.to_le_bytes());
        data.extend_from_slice(&(rate * 2).to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&16u16.to_le_bytes());
        data.extend_from_slice(b"data");
        data.extend_from_slice(&((n * 2) as u32).to_le_bytes());
        for i in 0..n {
            let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
            data.extend_from_slice(&((s * 0.2 * i16::MAX as f32) as i16).to_le_bytes());
        }
        data
    }

    #[test]
    #[ignore = "needs real libmpv + audio stack; run manually with --features mpv"]
    fn live_mpv_smoke() {
        let backend = match MpvBackend::new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: mpv unavailable in this environment: {e}");
                return;
            }
        };
        assert!(backend.is_ok());
        // Lenient by design: without an audio device, ops may fail; the point
        // is exercising the API without failing headless CI if ever un-ignored.
        if let Err(e) = backend.load_bytes(&wav_bytes()) {
            eprintln!("SKIP: mpv load failed headless: {e}");
            return;
        }
        if let Err(e) = backend.set_volume(0.5) {
            eprintln!("SKIP: mpv volume failed headless: {e}");
            return;
        }
        if let Err(e) = backend.apply_eq_preset(&EqPreset::rock()) {
            eprintln!("SKIP: mpv EQ failed headless: {e}");
            return;
        }
        if let Err(e) = backend.seek_secs(0.0) {
            eprintln!("SKIP: mpv seek failed headless: {e}");
            return;
        }
        let _ = backend.play();
        let _ = backend.pause();
        let _ = backend.play();
        let _ = backend.stop();
        assert!(backend.is_ok());
    }

    #[test]
    #[ignore = "needs real libmpv; run manually with --features mpv"]
    fn live_mpv_seek_rejects_non_finite() {
        let backend = match MpvBackend::new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: mpv unavailable in this environment: {e}");
                return;
            }
        };
        assert!(backend.seek_secs(f64::NAN).is_err());
        assert!(backend.seek_secs(f64::INFINITY).is_err());
    }

    #[test]
    #[ignore = "needs real libmpv; run manually with --features mpv"]
    fn live_mpv_position_and_duration() {
        let backend = match MpvBackend::new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: mpv unavailable in this environment: {e}");
                return;
            }
        };
        // Idle (nothing loaded): both fall back to 0.0, never negative/NaN.
        assert_eq!(backend.position_secs(), 0.0);
        assert_eq!(backend.duration_secs(), 0.0);
        // Trait view agrees with the inherent method.
        let via_trait: &dyn AudioBackend = &backend;
        assert_eq!(via_trait.position_secs(), backend.position_secs());
        if let Err(e) = backend.load_bytes(&wav_bytes()) {
            eprintln!("SKIP: mpv load failed headless: {e}");
            return;
        }
        if let Err(e) = backend.play() {
            eprintln!("SKIP: mpv play failed headless: {e}");
            return;
        }
        // Position is a real `time-pos` read: finite and non-negative.
        let pos = backend.position_secs();
        assert!(pos.is_finite() && pos >= 0.0, "got {pos}");
        let dur = backend.duration_secs();
        assert!(dur.is_finite() && dur >= 0.0, "got {dur}");
        let _ = backend.stop();
    }
}
