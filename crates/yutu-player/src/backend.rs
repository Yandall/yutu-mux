//! Audio backends. `RodioBackend` is real (pure Rust decode via symphonia).
//! Network streams download to memory (`fetch_bytes`) and decode from a
//! `Cursor` (`decode_bytes`); Innertube resolution + PO tokens live in
//! `yutu_core::{InnertubeBackend, fetch_po_token}`.

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

/// Shared HTTP client for stream/audio fetches: one connection pool
/// and TLS session cache per process instead of one per fetch (a fresh
/// `Client::new()` per 60px thumb burned handshakes plus pool memory
/// on every scroll sweep). Hardened: 15s timeout, at most 5 redirects.
fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// True for URLs the fetcher may pull. Prod: streams plus artwork only.
/// Tests additionally allow loopback stub servers (`127.0.0.1`, `localhost`)
/// so the offline harness keeps working without weakening prod.
fn is_fetchable(url: &str) -> bool {
    if yutu_core::security::is_allowed_fetch_url(url) {
        return true;
    }
    is_test_loopback(url)
}

/// True for the loopback stub servers used by tests. Never true in prod
/// builds: a compromised sidecar must not make the app hit local services.
fn is_test_loopback(url: &str) -> bool {
    #[cfg(test)]
    {
        url.starts_with("http://127.0.0.1/")
            || url.starts_with("http://localhost/")
            || url.starts_with("http://127.0.0.1:")
            || url.starts_with("http://localhost:")
    }
    #[cfg(not(test))]
    {
        let _ = url;
        false
    }
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
/// trickle, but answers `Range` requests at full speed: so songs are pulled
/// in 1 MiB chunks and concatenated. Servers without range support (plain
/// 200 + full body) are returned as-is. HTTP errors surface as
/// `PlayerError::Stream` with the status preserved for PO-token hints.
///
/// Barrier: only allowlisted hosts are fetched (streams plus artwork), so
/// a compromised sidecar or server row cannot redirect the fetcher to an
/// attacker host, `file://`, or loopback. URLs are redacted in errors so
/// short-lived signatures never land in logs.
///
/// Browser-like headers go on every request: bare HTTP clients (default
/// library user-agents, no referer) get transient 403s from some Google
/// edges that a normal browser header set sails through.
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, PlayerError> {
    const CHUNK: u64 = 1024 * 1024;
    const MAX_BYTES: u64 = 100 * 1024 * 1024;
    if !is_fetchable(url) {
        return Err(PlayerError::Stream("blocked untrusted URL".to_string()));
    }
    let redacted = yutu_core::security::redact_url_for_log(url);
    let client = http_client();
    let mut start: u64 = 0;
    let mut out = Vec::new();
    loop {
        let end = start + CHUNK - 1;
        let response = client
            .get(url)
            .header("Range", format!("bytes={start}-{end}"))
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Origin", "https://www.youtube.com")
            .header("Referer", "https://www.youtube.com/")
            .send()
            .await
            .map_err(|e| PlayerError::Stream(format!("GET {redacted}: {e}")))?;
        let status = response.status();
        if status == reqwest::StatusCode::OK {
            // No range support: whole body at once.
            let body = response
                .bytes()
                .await
                .map_err(|e| PlayerError::Stream(format!("read {redacted}: {e}")))?;
            out.extend_from_slice(&body);
            if out.len() as u64 > MAX_BYTES {
                return Err(PlayerError::Stream("over size cap".to_string()));
            }
            return Ok(out);
        }
        if !status.is_success() {
            return Err(PlayerError::Stream(format!(
                "GET {redacted}: HTTP {status}"
            )));
        }
        let total = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_total);
        let chunk = response
            .bytes()
            .await
            .map_err(|e| PlayerError::Stream(format!("read {redacted}: {e}")))?;
        if chunk.is_empty() {
            return Err(PlayerError::Stream(format!("empty chunk from {redacted}")));
        }
        out.extend_from_slice(&chunk);
        start += chunk.len() as u64;
        if out.len() as u64 > MAX_BYTES {
            return Err(PlayerError::Stream("over size cap".to_string()));
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

/// Pause between warm-up probes of a refused stream URL.
const WARM_RETRY: std::time::Duration = std::time::Duration::from_millis(300);
/// Cap per warm-up probe: a stalled edge or hung connect is a
/// transport error (fail open), never a wait past the caller's budget.
const WARM_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Wait until a freshly resolved stream URL actually serves bytes.
/// Some yt-dlp clients mint googlevideo URLs that answer HTTP 403 for
/// a few seconds before they go live; mpv opening one at once fails
/// the load. Probes with a 1-byte ranged GET: 2xx is ready, 403 is
/// retried until `max_wait`, then errors (the caller re-resolves).
/// Any other HTTP error fails at once. Transport errors (including a
/// probe past [`WARM_PROBE_TIMEOUT`]) fail open (`Ok`): the probe is advisory, so a local network quirk must never
/// block playback that the player itself might manage.
///
/// Barrier: untrusted URLs are rejected before any probe, so a compromised
/// sidecar cannot make the warmer hit an attacker host.
pub async fn warm_stream_url(url: &str, max_wait: std::time::Duration) -> Result<(), PlayerError> {
    // Streams only (no artwork): the warmer precedes mpv, which only
    // plays googlevideo.
    if !yutu_core::security::is_allowed_stream_url(url) && !is_test_loopback(url) {
        return Err(PlayerError::Stream("blocked untrusted URL".to_string()));
    }
    let client = http_client();
    let deadline = std::time::Instant::now() + max_wait;
    loop {
        let response = match client
            .get(url)
            .header("Range", "bytes=0-0")
            .header("Accept", "*/*")
            .timeout(WARM_PROBE_TIMEOUT)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        if status != reqwest::StatusCode::FORBIDDEN {
            return Err(PlayerError::Stream(format!(
                "stream URL refused: HTTP {status}"
            )));
        }
        if std::time::Instant::now() + WARM_RETRY > deadline {
            return Err(PlayerError::Stream(format!(
                "stream URL still refused (HTTP 403) after {}s",
                max_wait.as_secs_f32().round()
            )));
        }
        tokio::time::sleep(WARM_RETRY).await;
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
    use rodio::source::Source;

    /// Control ops never append a source, so this test is silent: no
    /// test tone, no audio out of the speakers during `cargo test`.
    #[test]
    fn rodio_controls_do_not_error_and_play_nothing() {
        let backend = match RodioBackend::try_new() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("SKIP: no audio device in CI/headless: {e}");
                return;
            }
        };
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

    /// Clock behavior without touching hardware: `ClockState` is pure
    /// wall-clock logic, so this test needs no device and emits no audio.
    #[test]
    fn clock_advances_freezes_resumes_and_stops() {
        let mut clock = ClockState::default();
        assert_eq!(clock.position_secs(), 0.0);
        clock.restart();
        // Monotonic non-decreasing across two reads.
        let first = clock.position_secs();
        assert!(first >= 0.0);
        std::thread::sleep(Duration::from_millis(30));
        let second = clock.position_secs();
        assert!(
            second >= first,
            "position should be monotonic: {first} -> {second}"
        );

        // Pause freezes: two reads must be exactly equal (no running part).
        clock.freeze();
        let frozen_a = clock.position_secs();
        let frozen_b = clock.position_secs();
        assert_eq!(frozen_a, frozen_b);

        // Resume advances again.
        clock.resume();
        std::thread::sleep(Duration::from_millis(30));
        let advanced = clock.position_secs();
        assert!(
            advanced >= frozen_b,
            "position should advance after resume: {frozen_b} -> {advanced}"
        );

        // Stop resets to zero.
        clock.reset();
        assert_eq!(clock.position_secs(), 0.0);
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
    async fn untrusted_hosts_are_blocked_without_network() {
        let err = fetch_bytes("https://example.com").await.unwrap_err();
        assert!(err.to_string().contains("blocked"), "{err}");
        let err = fetch_bytes("file:///etc/passwd").await.unwrap_err();
        assert!(err.to_string().contains("blocked"), "{err}");
        let err = warm_stream_url(
            "https://evil.com/videoplayback",
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("blocked"), "{err}");
    }

    /// Local HTTP stub: answers `refusals` requests with `refuse_status`,
    /// then 206 forever. Returns its URL and a served-request counter.
    fn stub_server(
        refusals: usize,
        refuse_status: &'static str,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        use std::sync::atomic::Ordering;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = served.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 4096];
                let mut req = Vec::new();
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let n = counter.fetch_add(1, Ordering::SeqCst);
                let reply = if n < refusals {
                    format!("HTTP/1.1 {refuse_status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                } else {
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/10\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx".to_string()
                };
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (format!("http://{addr}/videoplayback"), served)
    }

    #[tokio::test]
    async fn warm_waits_out_a_fresh_url_403_window() {
        let (url, served) = stub_server(2, "403 Forbidden");
        warm_stream_url(&url, std::time::Duration::from_secs(5))
            .await
            .expect("URL goes live after the 403 window");
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn warm_gives_up_on_a_permanent_403() {
        let (url, _) = stub_server(usize::MAX, "403 Forbidden");
        let err = warm_stream_url(&url, std::time::Duration::from_millis(700))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("403"), "{err}");
    }

    #[tokio::test]
    async fn warm_fails_fast_on_other_http_errors() {
        let (url, served) = stub_server(usize::MAX, "404 Not Found");
        let err = warm_stream_url(&url, std::time::Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn warm_fails_open_on_transport_errors() {
        // Bind then drop: the port refuses connections.
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let url = format!("http://{addr}/videoplayback");
        warm_stream_url(&url, std::time::Duration::from_secs(1))
            .await
            .expect("transport errors never block playback");
    }
}
