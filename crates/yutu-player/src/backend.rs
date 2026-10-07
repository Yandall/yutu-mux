//! Audio backends. `RodioBackend` is real (pure Rust decode via symphonia).
//! Network streams download to memory (`fetch_bytes`) and decode from a
//! `Cursor` (`decode_bytes`); Innertube resolution + PO tokens live in
//! `yutu_core::{InnertubeBackend, fetch_po_token}`.

use rodio::source::{SineWave, Source};
use rodio::Decoder;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

    /// Current playback position in seconds (>= 0.0).
    ///
    /// Default is `0.0` so [`StubBackend`] and any external out-of-tree
    /// impls keep compiling without changes.
    fn position_secs(&self) -> f64 {
        0.0
    }
}

pub struct StubBackend;

impl AudioBackend for StubBackend {
    fn is_ok(&self) -> bool {
        true
    }
}

/// Wall-clock playback position tracker for [`RodioBackend`].
///
/// `rodio::Player` exposes no position API, so position is estimated as
/// wall-clock time since playback started. `elapsed` accumulates time across
/// pause/resume cycles; `running_since` is `Some` while audio is (assumed)
/// playing and `None` while paused or stopped.
#[derive(Debug, Default)]
struct ClockState {
    elapsed: Duration,
    running_since: Option<Instant>,
}

impl ClockState {
    /// Reset to zero and start counting from now (new track / restart).
    fn restart(&mut self) {
        self.elapsed = Duration::ZERO;
        self.running_since = Some(Instant::now());
    }

    /// Freeze accumulation at the current position (pause). No-op unless
    /// currently running.
    fn freeze(&mut self) {
        if let Some(since) = self.running_since.take() {
            self.elapsed += since.elapsed();
        }
    }

    /// Continue counting from the frozen position (resume). No-op while
    /// already running.
    fn resume(&mut self) {
        if self.running_since.is_none() {
            self.running_since = Some(Instant::now());
        }
    }

    /// Reset to zero and stop counting.
    fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.running_since = None;
    }

    fn position_secs(&self) -> f64 {
        let extra = self
            .running_since
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);
        (self.elapsed + extra).as_secs_f64().max(0.0)
    }
}

/// Lock the clock mutex without ever panicking on poisoning: a poisoned
/// mutex still holds usable state, so recover the inner guard instead.
fn lock_clock<'a>(mutex: &'a Mutex<ClockState>) -> std::sync::MutexGuard<'a, ClockState> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct RodioBackend {
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
    clock: Mutex<ClockState>,
}

impl RodioBackend {
    pub fn try_new() -> Result<Self, PlayerError> {
        let sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| PlayerError::NoDevice(e.to_string()))?;
        let player = rodio::Player::connect_new(sink.mixer());
        Ok(Self {
            _sink: sink,
            player,
            clock: Mutex::new(ClockState::default()),
        })
    }

    /// 440Hz test tone, 0.5s. Proves the whole output chain without network.
    pub fn play_test_tone(&self) {
        let source = SineWave::new(440.0)
            .take_duration(Duration::from_secs_f32(0.5))
            .amplify(0.2);
        self.player.append(source);
        lock_clock(&self.clock).restart();
    }

    /// Decode in-memory audio (WAV/MP3/Opus/AAC via symphonia) and play it.
    /// Runs on any async runtime (iced's tokio executor in the app).
    ///
    /// Resets the wall-clock position to zero and starts counting.
    ///
    /// Precision: wall-clock time is an approximation of the decoder
    /// position. Pause/resume boundaries are the only adjustments applied;
    /// output latency, buffering, and decode gaps are not accounted for, so
    /// expect tens-of-milliseconds of drift versus the true sample position.
    pub fn play_bytes(&self, bytes: Vec<u8>) -> Result<(), PlayerError> {
        let source = decode_bytes(bytes)?;
        self.player.append(source);
        lock_clock(&self.clock).restart();
        Ok(())
    }

    /// Download a stream URL fully, then play it. Whole-song buffering is
    /// deliberate v1 simplicity (songs are a few MB); range-streaming is a
    /// later optimization.
    ///
    /// On success the wall-clock position resets to zero and starts
    /// counting (same semantics as [`RodioBackend::play_bytes`]).
    pub async fn fetch_and_play(&self, url: &str) -> Result<(), PlayerError> {
        let bytes = fetch_bytes(url).await?;
        self.play_bytes(bytes)
    }

    pub fn pause(&self) {
        self.player.pause();
        lock_clock(&self.clock).freeze();
    }

    pub fn resume(&self) {
        self.player.play();
        lock_clock(&self.clock).resume();
    }

    pub fn stop(&self) {
        self.player.clear();
        lock_clock(&self.clock).reset();
    }

    /// Absolute seek in seconds. The rodio path has no seeking support
    /// (decoded sources are appended fire-and-forget), so this always
    /// fails. Provided for API symmetry with `MpvBackend::seek_secs`.
    pub fn seek_secs(&self, _secs: f64) -> Result<(), PlayerError> {
        Err(PlayerError::Stream(
            "seek unsupported on rodio backend".to_string(),
        ))
    }

    /// Wall-clock playback position in seconds since the current track
    /// started (>= 0.0). Frozen while paused, zero after [`RodioBackend::stop`]
    /// or before any playback. See [`RodioBackend::play_bytes`] for
    /// precision limits.
    #[must_use]
    pub fn position_secs(&self) -> f64 {
        lock_clock(&self.clock).position_secs()
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume.clamp(0.0, 1.0));
    }
}

impl AudioBackend for RodioBackend {
    fn is_ok(&self) -> bool {
        true
    }

    fn position_secs(&self) -> f64 {
        RodioBackend::position_secs(self)
    }
}

/// Download a URL into memory. GoogleVideo throttles full-body GETs to a
/// trickle, but answers `Range` requests at full speed — so songs are pulled
/// in 1 MiB chunks and concatenated. Servers without range support (plain
/// 200 + full body) are returned as-is. HTTP errors surface as
/// `PlayerError::Stream` with the status preserved for PO-token hints.
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, PlayerError> {
    const CHUNK: u64 = 1024 * 1024;
    const MAX_BYTES: u64 = 100 * 1024 * 1024;
    let client = reqwest::Client::new();
    let mut start: u64 = 0;
    let mut out = Vec::new();
    loop {
        let end = start + CHUNK - 1;
        let response = client
            .get(url)
            .header("Range", format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|e| PlayerError::Stream(format!("GET {url}: {e}")))?;
        let status = response.status();
        if status == reqwest::StatusCode::OK {
            // No range support: whole body at once.
            let body = response
                .bytes()
                .await
                .map_err(|e| PlayerError::Stream(format!("read {url}: {e}")))?;
            out.extend_from_slice(&body);
            return Ok(out);
        }
        if !status.is_success() {
            return Err(PlayerError::Stream(format!("GET {url}: HTTP {status}")));
        }
        let total = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_total);
        let chunk = response
            .bytes()
            .await
            .map_err(|e| PlayerError::Stream(format!("read {url}: {e}")))?;
        if chunk.is_empty() {
            return Err(PlayerError::Stream(format!("empty chunk from {url}")));
        }
        out.extend_from_slice(&chunk);
        start += chunk.len() as u64;
        if out.len() as u64 > MAX_BYTES {
            return Err(PlayerError::Stream(format!("{url}: over size cap")));
        }
        match total {
            Some(total) if start >= total => return Ok(out),
            Some(_) => {}
            None => {
                // No total advertised but chunked 206: stop on short chunk.
                if (chunk.len() as u64) < CHUNK {
                    return Ok(out);
                }
            }
        }
    }
}

/// Parse `bytes 0-1048575/4172542` into the total length.
fn parse_content_range_total(header: &str) -> Option<u64> {
    header.rsplit('/').next()?.trim().parse().ok()
}

/// Decode in-memory audio without touching hardware (headless-testable).
pub fn decode_bytes(bytes: Vec<u8>) -> Result<Decoder<Cursor<Vec<u8>>>, PlayerError> {
    Decoder::try_from(Cursor::new(bytes)).map_err(|e| PlayerError::Stream(format!("decode: {e:?}")))
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

    /// Minimal 0.1s mono 16-bit 44.1kHz WAV, built by hand (no deps).
    fn wav_bytes() -> Vec<u8> {
        let rate: u32 = 44_100;
        let n: usize = (rate as usize) / 10;
        let mut data = Vec::with_capacity(44 + n * 2);
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&((36 + n * 2) as u32).to_le_bytes());
        data.extend_from_slice(b"WAVEfmt ");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // PCM
        data.extend_from_slice(&1u16.to_le_bytes()); // mono
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
    fn decodes_in_memory_wav_without_hardware() {
        use std::num::NonZero;
        let source = decode_bytes(wav_bytes()).expect("decode wav");
        assert_eq!(source.channels(), NonZero::new(1).unwrap());
        assert_eq!(source.sample_rate(), NonZero::new(44_100).unwrap());
        assert!(source.count() > 0);
    }

    #[test]
    fn garbage_is_a_decode_error_not_a_panic() {
        assert!(decode_bytes(b"not audio at all".to_vec()).is_err());
    }

    #[test]
    fn content_range_total_parses() {
        assert_eq!(
            parse_content_range_total("bytes 0-1048575/4172542"),
            Some(4172542)
        );
        assert_eq!(parse_content_range_total("bytes */4172542"), Some(4172542));
        assert_eq!(parse_content_range_total("garbage"), None);
    }

    #[test]
    fn clock_starts_at_zero_before_playback() {
        let backend = match RodioBackend::try_new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: no audio device in CI/headless: {e}");
                return;
            }
        };
        assert_eq!(backend.position_secs(), 0.0);
    }

    #[test]
    fn clock_advances_freezes_resumes_and_stops() {
        let backend = match RodioBackend::try_new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: no audio device in CI/headless: {e}");
                return;
            }
        };
        assert_eq!(backend.position_secs(), 0.0);
        if let Err(e) = backend.play_bytes(wav_bytes()) {
            eprintln!("SKIP: play_bytes failed headless: {e}");
            return;
        }
        // Monotonic non-decreasing across two reads.
        let first = backend.position_secs();
        assert!(first >= 0.0);
        std::thread::sleep(Duration::from_millis(30));
        let second = backend.position_secs();
        assert!(
            second >= first,
            "position should be monotonic: {first} -> {second}"
        );

        // Pause freezes: two reads must be exactly equal (no running part).
        backend.pause();
        let frozen_a = backend.position_secs();
        let frozen_b = backend.position_secs();
        assert_eq!(frozen_a, frozen_b);

        // Resume advances again.
        backend.resume();
        std::thread::sleep(Duration::from_millis(30));
        let advanced = backend.position_secs();
        assert!(
            advanced >= frozen_b,
            "position should advance after resume: {frozen_b} -> {advanced}"
        );

        // Stop resets to zero.
        backend.stop();
        assert_eq!(backend.position_secs(), 0.0);
    }

    #[test]
    fn clock_seek_is_unsupported() {
        let backend = match RodioBackend::try_new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: no audio device in CI/headless: {e}");
                return;
            }
        };
        let err = backend.seek_secs(10.0).expect_err("rodio seek must fail");
        assert!(
            matches!(err, PlayerError::Stream(_)),
            "unexpected error kind: {err:?}"
        );
    }

    #[tokio::test]
    #[ignore = "live network: fetches example.com"]
    async fn live_fetch_bytes() {
        let bytes = fetch_bytes("https://example.com").await.expect("fetch");
        assert!(!bytes.is_empty());
    }
}
