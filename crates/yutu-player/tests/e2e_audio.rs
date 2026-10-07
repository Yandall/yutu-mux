//! End-to-end: yt-dlp resolve -> fetch -> decode real audio bytes.
//!
//! Ignored by default (needs the `yt-dlp` binary + network). Run with:
//! `cargo test -p yutu-player --test e2e_audio -- --ignored`.
//! Override the binary via `YUTU_YTDLP_BIN=/path/to/yt-dlp`.

use rodio::Source;

#[tokio::test]
#[ignore = "live: needs yt-dlp + network; proves audible bytes end-to-end"]
async fn stream_resolves_fetches_and_decodes() {
    let Some(bin) = yutu_core::find_yt_dlp() else {
        eprintln!("SKIP: yt-dlp not installed (or set YUTU_YTDLP_BIN)");
        return;
    };
    let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "JWW8KK8kd1I".into());

    let url = yutu_core::resolve_audio_url(&bin, &video).expect("resolve audio URL");
    assert!(url.starts_with("https://"), "not a stream URL: {url}");

    let bytes = yutu_player::fetch_bytes(&url).await.expect("fetch bytes");
    assert!(bytes.len() > 100_000, "suspiciously small: {}", bytes.len());

    let source = yutu_player::decode_bytes(bytes).expect("decode audio");
    let rate = source.sample_rate().get();
    assert!(
        rate == 44_100 || rate == 48_000,
        "unexpected sample rate: {rate}"
    );
    assert!(source.count() > 48_000, "less than a second of audio");
}
