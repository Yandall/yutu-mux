# Innertube (Phase 4)

Live YouTube Music access for `yutu-core`, behind the `innertube` cargo
feature so offline builds keep working with `MockBackend`.

```toml
# crates/yutu-core/Cargo.toml
[features]
innertube = ["dep:ytmapi-rs", "dep:tokio"]
```

## Client choice: `ytmapi-rs` 0.3

- Async (tokio) pure-Rust client for YouTube Music's internal API, MIT
  licensed, maintained alongside `youtui` (`nick42d`, `youtui` org).
- Type-safe queries (`Query` + `PostQuery` traits, `YtMusic::query`) with
  `simplified-queries` convenience methods enabled via default features.
- Auth generics: `YtMusic<A: AuthToken>` — `NoAuthToken` for
  `YtMusic::new_unauthenticated()`, `BrowserToken` for
  `YtMusic::from_cookie_file(path)` / `from_cookie(str)`. `InnertubeBackend`
  wraps both in one enum so the generic does not leak into our API.
- Verified against source `ytmapi-rs-0.3.3` (semver-compatible with our
  `ytmapi-rs = "0.3"` requirement): constructors, `get_search_suggestions`
  (generic over `A: AuthToken`, works anonymous), `SearchSuggestion::get_text`,
  `VideoID::from_raw` via the `YoutubeID` trait, and `raw_json_query`.

## PO-token fallback

Stream resolution (`InnertubeBackend::resolve_stream`) walks
`StreamClient::fallback_chain` from `crate::stream`:

1. `android_vr` first — needs no PO token (yt-dlp PO-Token guide, July 2026).
2. `web_music` (GVS) — 403s here mean PO-token enforcement.
3. `ios` — likewise may require a token for `player`.

Each attempted client is recorded in `StreamRequest::tried`. HTTP-403-like
failures (message contains `403`, `po token`/`potoken`, `botguard`, or
`STATUS_FAILED`) map to `StreamError::PoTokenRequired`, whose `hint()` tells
the user to install `rustypipe-botguard` or play via `android_vr`. An
exhausted chain returns `StreamError::NotFound`; a PO-token failure seen
anywhere in the chain takes precedence on exhaustion because it is the
actionable error. Premium accounts are exempt from PO-token gating.

## Cookie safety

- Cookie files are validated locally with `CookieAuth::from_file` before any
  network; the parsed value is dropped (the real client reads the file
  itself) and never appears in errors or logs.
- Connection errors echo server responses only — never request headers.
  (Upstream `BrowserToken::from_str` additionally requires a `SAPISID=`
  entry; a cookie without one fails with a static "header" error.)
- `BrowserToken`'s upstream `Debug` impl prints only `"Private BrowserToken"`,
  so `InnertubeBackend` can safely derive `Debug`/`Clone`. Still, never
  `println!`/`log` cookie contents; use `CookieAuth::redacted()`.
- Failing payloads can be saved with `crate::stream::save_report` — callers
  must redact first (strip `Cookie`/`Authorization`/`SAPISIDHASH`).

## What is live vs stubbed

| Piece | Status |
|---|---|
| `connect_anonymous` (`new_unauthenticated`) | LIVE (network: fetches visitor data) |
| `connect_with_cookie_file` (`from_cookie_file`) | LIVE (network: scrapes `ytcfg`) |
| `search_suggestions` (`get_search_suggestions`) | LIVE read query, proves wiring |
| `player`-endpoint probe (`GetSongTrackingUrlQuery`, path `"player"`) | LIVE per fallback client |
| Direct audio-stream URL extraction | STUBBED: `ytmapi-rs` 0.3 has no `streamingData` parser, so `extract_audio_url` best-effort scans raw `player` JSON for an `adaptiveFormats`/`formats` audio `url`. `signatureCipher`-only or missing formats fall through to the next client, then `NotFound`. Full cipher/throttle handling is future work. |
| Per-client GVS pinning (`android_vr`/`web_music`/`ios` Innertube contexts) | STUBBED: 0.3 pins one `WEB_REMIX` context per handle; the client label is used for error attribution + chain bookkeeping until a custom `Query` per context lands. |

## API gaps found (ytmapi-rs 0.3)

1. **No `player`/stream-URL query.** Closest available is
   `get_song_tracking_url` / `GetSongTrackingUrlQuery` (hits Innertube path
   `"player"` but returns a history-tracking URL, not `streamingData`). We use
   `raw_json_query` on that query and parse `streamingData` ourselves.
2. **No per-client context override.** Innertube `clientName`/`clientVersion`
   (e.g. `ANDROID_VR`) cannot be set per request; one context per handle.
3. **No PO-token field.** Nothing to attach a `poToken`/`contentPoToken` to;
   hence the fallback-and-hint policy instead of token injection.
4. `get_watch_playlist_from_video_id` notes tracks are unimplemented upstream
   (empty vec); we do not rely on it.

## Tests

- Offline unit tests (run normally): 403/botguard → `PoTokenRequired` mapping,
  unplayable → `NotFound`, other → `Network`; `hint()` actionability; audio-URL
  extraction against `crates/yutu-core/fixtures/player_*.json` (hand-made,
  redacted samples shaped like `player` responses); fallback-chain recording;
  empty-cookie rejection without network.
- Live tests (`connect_anonymous` + suggestions, `resolve_stream` chain
  bookkeeping) are `#[tokio::test]` + `#[ignore]`; run with
  `cargo test -p yutu-core --features innertube -- --ignored` on a networked
  machine.

## Stream resolution: yt-dlp first (current)

`crates/yutu-core/src/ytdlp.rs` (no feature gate, std-only, license-clean)
resolves video IDs via the `yt-dlp` sidecar (`--get-url`, format
`bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio` — AAC preferred
because symphonia cannot decode Opus-in-WebM). The UI tries it before the
raw probes below. Requires `yt-dlp` in `PATH` (`YUTU_YTDLP_BIN` override);
missing binary degrades to the raw path with a clear status.

Two hard-won transport facts (verified Oct 2026, permanent e2e test in
`crates/yutu-player/tests/e2e_audio.rs`, ignored without binary+network):
- GVS throttles full-body GETs to a trickle; 1 MiB `Range` chunks download
  a 4 MB song in ~2 s (`fetch_bytes` chunks unconditionally).
- `signatureCipher` URLs need a deobfuscator. `rustypipe` 0.11.4's can no
  longer parse current player JS (`could not extract sig fn name`, Oct
  2026), so the `rp` feature/module was removed rather than kept as a
  19-second dead end (history preserves it; revisit if upstream revives).
  yt-dlp's extractor is current and maintained within hours.
