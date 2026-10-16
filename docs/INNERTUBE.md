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
  network: Netscape `cookies.txt` exports (the "Get cookies.txt" shape:
  tab-separated rows) are normalised to a `name=value; ...` header, raw
  header files pass through, and files without `SAPISID` fail fast with an
  actionable message. Cookie material never appears in errors or logs.
- A successful cookie import stashes the normalised header at
  `{data_dir}/cookie_session.txt` (0600, same as the OAuth token file), so
  restarts stay signed in: Settings shows "Signed in with cookies" with
  one-click re-import plus sign-out (deletes the file). Startup itself
  still does zero network; the session only reconnects on user action.
- Connection errors echo server responses only — never request headers.
  (Upstream `BrowserToken::from_str` additionally requires a `SAPISID=`
  entry; a cookie without one fails with a static "header" error.)

## Two-way sync (while signed in)

- Import reads the whole account in stages so the window never
  freezes on one long silent future: the base stage (library songs +
  liked ids + playlist headers, each list failing open) merges first
  and shows at once, then one task per playlist streams its tracks in,
  each arrival updating the status line (`Playlist 'X' done
  (123 tracks) - 2/5`). Every list endpoint streams all
  continuation pages (about 100 items each): single-shot queries stop
  at the first page, which truncated big playlists and libraries.
  Server lists keep their real ids
  (`PL...`/`VL...`) in the local store; local `u{seq}` shells never
  collide. An empty account reports honestly ("library is empty: like
  songs ... then import again") instead of a generic failure.
- Playlist rows with shapes the typed parser rejects (seen live: no
  `fixedColumns`, duration in a fourth flex column) fall back to a
  tolerant raw-JSON pass that maps what it can per row and skips only
  the bad rows (deleted, greyed-out, episodes), so one odd row cannot
  sink a whole playlist. Such rows may carry duration 0 (no duration
  on the row at all).
- Local edits apply first, then mirror to YouTube Music in the
  background (OAuth token preferred, cookie session fallback): likes via
  `rate_song`, playlist creates (private) via `create_playlist` with the
  local shell migrated to the server id, adds via
  `add_video_items_to_playlist`, deletes via `delete_playlist`. Only
  failures surface in the status ("kept locally: ..."); success stays
  silent because the row already shows the state.
- Honest gap: single-track playlist removes stay local-only (upstream
  `remove_playlist_items` needs a per-membership `setVideoId` that
  playlist listings do not return). The status says "(local only)" on
  server lists; re-import will bring the track back.
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
