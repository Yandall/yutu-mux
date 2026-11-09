//! yutu-ui layout: split from lib.rs (no behavior change).

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

/// Fixed row/card heights: every card and row keeps one height and long
/// names truncate with an ellipsis instead of stretching the container.
/// Containers pair these with `.clip(true)` as a backstop, so even an
/// untruncated overflow can never move its neighbours.
pub(crate) const HOME_CARD_W: f32 = 156.0;
pub(crate) const HOME_PICK_ART: f32 = 140.0;
pub(crate) const HOME_CARD_H: f32 = 200.0;
/// Unified search list cap (songs + albums + artists mixed): the page
/// stays fast while every type keeps its head results visible.
pub(crate) const MIXED_SEARCH_CAP: usize = 30;
/// Breathing room below the home pick strip: the floating horizontal
/// scrollbar sits on the strip's bottom edge, clear of the cards above
/// and the next section below.
pub(crate) const HOME_STRIP_GAP: f32 = 14.0;
pub(crate) const TRACK_ROW_H: f32 = 56.0;
/// Context menu item height (icon 14 + 14px text at 1.3 line height +
/// 8px padding each side, rounded up): fixed so the playlist flyout
/// anchor math can never drift from the laid-out rows.
pub(crate) const MENU_ROW_H: f32 = 36.0;
pub(crate) const SUGGEST_ROW_H: f32 = 44.0;
/// Queue row height: fits a wrapped 2-line title plus the artist line
/// with room to spare (2 x 12px x 1.25 + 2px gap + 11px x 1.25 +
/// 16px row padding = 61.75px, rounded up). Anything shorter clips
/// the artist on long titles (the row clips instead of growing).
pub(crate) const QUEUE_ROW_H: f32 = 64.0;
/// Lazy art sweep: viewport overscan each side (absorbs header-height
/// estimates and fast scrolls), scroll buckets plus minimum gap (scroll
/// storms collapse), fetch cap per sweep.
pub(crate) const ART_OVERSCAN_PX: f32 = 480.0;
pub(crate) const ART_BUCKET_PX: f32 = 120.0;
pub(crate) const ART_SWEEP_MIN_MS: u128 = 250;
pub(crate) const ART_SWEEP_CAP: usize = 48;
/// In-flight fetch ceiling (sweeps pause past it; completions chain).
pub(crate) const ART_INFLIGHT_CAP: usize = 48;
/// In-memory handle cap (evicted entries re-resolve from disk, which
/// the cache keeps, so dropping here never blanks rows). Sized for one
/// viewport plus overscan: visible covers decode once, the rest reads
/// from disk on scroll (cheap) instead of pinning decoded textures.
pub(crate) const ART_MEM_CAP: usize = 128;
/// Playback history cap (drives Home picks, which read the last 8):
/// very long sessions must not grow it without bound.
pub(crate) const HISTORY_CAP: usize = 500;
/// Full timestamped history cap (drives the History page): bounds the
/// in-memory list while the database keeps every play ever.
/// Sized for years of listening; the page renders it grouped.
pub(crate) const HISTORY_FULL_CAP: usize = 5000;
/// History page render window (newest first; "Show more" pages in the
/// rest). Full rows cost ~7 widgets each on every view build, so an
/// uncapped multi-thousand log stalls frames (measured ~65ms/build
/// at 5000 rows against a 16ms frame budget).
pub(crate) const HISTORY_PAGE_SIZE: usize = 100;
/// Lyrics cache cap (entries are KBs of text; a dropped half refetches
/// on demand, never in a loop: plays fetch once per need).
pub(crate) const LYRICS_CACHE_CAP: usize = 100;
/// Manual lyrics-provider pins cap (track ids pinned to strict display
/// by the stepper; a dropped pin simply falls back to automatic).
pub(crate) const LYRICS_MANUAL_CAP: usize = 200;
/// Resolved stream-URL cap (entries expire in hours; the sweep below
/// drops the dead ones first).
pub(crate) const URL_CACHE_CAP: usize = 200;
/// On-disk art cache cap (LRU by mtime, pruned at most every interval).
pub(crate) const ART_CACHE_MAX_BYTES: u64 = 150 * 1024 * 1024;
pub(crate) const ART_PRUNE_MIN_SECS: u64 = 300;
/// Image cache time-to-live, both tiers: covers older than a day read
/// as missing and are refetched on demand, so browsing history never
/// pins disk (or memory) past a day. Absolute from fetch time: hits
/// do not extend it (wall-clock mtimes are unreliable for atime).
pub(crate) const ART_CACHE_TTL_SECS: u64 = 24 * 60 * 60;
/// Resolve attempts per load before skipping ahead (initial load
/// plus retries): transient yt-dlp/network flakes recover, dead
/// tracks fail over fast instead of hanging the queue.
pub(crate) const MAX_RESOLVE_ATTEMPTS: u8 = 3;
/// A stream-URL resolve that outlives this never returns: the timeout
/// error flows through the normal retry path instead of wedging the
/// loader on a hung yt-dlp. Generous: healthy resolves take seconds
/// (worst case: pinned client fails, default set, then URL warm-up).
pub(crate) const RESOLVE_TIMEOUT_SECS: u64 = 20;
/// Longest wait for a fresh stream URL to stop answering 403 (seen:
/// up to ~3.5s on yt-dlp's `tv_samsung` client URLs).
pub(crate) const STREAM_WARM_SECS: u64 = 6;
/// An mpv stream load that is idle this long after `loadfile` without
/// any audible playback never opened (`loadfile` is async, so an edge
/// refusal never reaches the call site): retry it like a failed resolve.
pub(crate) const STREAM_OPEN_GRACE: std::time::Duration = std::time::Duration::from_millis(2500);
/// Same backstop for full-fetch resolves (slow networks fetch
/// for minutes legitimately, so this only catches true stalls).
pub(crate) const RESOLVE_BYTES_TIMEOUT_SECS: u64 = 300;
/// Back-button threshold in seconds: past this the button restarts
/// the song from 00:00 (instant seek, no reload); inside it the
/// previous song plays instead.
pub(crate) const PREV_RESTART_SECS: f64 = 3.0;
/// Playlist flyout hover grace: crossing the gap between the toggle
/// row and the panel exits one layer before entering the other, so
/// the Tick only closes past this delay instead of flickering.
pub(crate) const SUBMENU_HOVER_GRACE: std::time::Duration = std::time::Duration::from_millis(300);
/// Divider pitch between uniform rows (1px hairline + 2px breathing
/// room each side, never trailing): row height + 5.
pub(crate) const DIVIDER_PX: f32 = 5.0;
/// Drawer lyrics follow geometry (estimates: verses wrap, so exact
/// per-row heights are unknowable; every constant mirrors its view
/// builder, and error only shifts the center, never loses the verse).
/// Hero block above the lyrics: 300 art + 24/15/12 titles at default
/// line height + like row + spacings.
/// Scrollable id for the drawer lyrics pane (follow snap target).
pub(crate) const LYRICS_SCROLL_ID: &str = "drawer-lyrics";
/// Provider stepper label box: fits "YouTube Music" at 12px with
/// headroom for wider display fonts, so the `< >` arrows never drift
/// when the provider changes. Text centers inside.
pub(crate) const LYRICS_PROVIDER_LABEL_W: f32 = 104.0;
/// Estimated hero block height (see sum above).
pub(crate) const LYRIC_HERO_H: f32 = 428.0;
/// Hero-to-lyrics column spacing.
pub(crate) const LYRIC_LIST_GAP: f32 = 16.0;
/// Spacing between lyric rows (and around stanza gaps).
pub(crate) const LYRIC_ROW_SPACING: f32 = 6.0;
/// Stanza separator height (plus the row spacing around it).
pub(crate) const LYRIC_GAP_H: f32 = 10.0;
/// Lyric button vertical padding (4 top + 4 bottom).
pub(crate) const LYRIC_BTN_PAD: f32 = 8.0;
/// Horizontal chrome beside lyric text (25px rail/icon + 8px button
/// padding; both row arms sum the same, so one constant serves).
pub(crate) const LYRIC_ROW_CHROME: f32 = 33.0;
/// Window chrome above the drawer viewport (header + player).
pub(crate) const LYRIC_VIEWPORT_PAD: f32 = 260.0;
/// Any user scroll disengages follow (glide echoes match the command
/// exactly, so they never trip it); scrolling back near the live
/// target re-engages.
pub(crate) const LYRIC_RESUME_PX: f32 = 120.0;
/// Sidebar sync spinner frames (verbatim loader glyph, one rotation
/// step each): 60 steps at the ~60fps `Animate` tick is one turn/sec.
pub(crate) const SPINNER_STEPS: usize = 60;
/// Shared lazy-art geometry: body padding, uniform row pitch (52px
/// rows + 2px spacing), card grid pitch (200px cards + 10px wrap
/// spacing). Declared once so slot math cannot drift per call site.
pub(crate) const BODY_PAD: f32 = 20.0;
pub(crate) const ROW_PITCH: f32 = TRACK_ROW_H + 2.0;
pub(crate) const GRID_PITCH: f32 = HOME_CARD_H + 10.0;
/// Scrollbar fade: idle delay before the fade starts plus the per-tick
/// step (~8 frames on the Animate tick, matching panel glides).
pub(crate) const SCROLL_FADE_DELAY_MS: u64 = 700;
pub(crate) const SCROLL_FADE_STEP: f32 = 0.12;

/// Hover hysteresis grace (px): how far outside the expanded panel the
/// cursor may roam before an unpinned sidebar collapses.
pub(crate) const SIDEBAR_GRACE: f32 = 16.0;
/// Same contract for the right panel (measured from the window edge).
pub(crate) const RIGHT_GRACE: f32 = 16.0;
/// Collapsed right rail width; the expand zone starts just outside it.
pub(crate) const RIGHT_RAIL_W: f32 = 56.0;
/// How far outside the collapsed rail the expand zone reaches.
pub(crate) const RIGHT_EXPAND_GRACE: f32 = 8.0;
/// Collapsed rail thumb size (matches the thumb_sized call below).
pub(crate) const RAIL_THUMB_PX: f32 = 32.0;
/// Collapsed rail pitch per thumb (thumb plus button padding plus wrapper
/// padding plus column spacing). Idle and live share the same outer box
/// (transparent vs red border) so the pitch never shifts on highlight.
pub(crate) const RAIL_ITEM_PITCH: f32 = RAIL_THUMB_PX + 4.0 + 2.0 + 6.0;
/// Collapsed rail vertical chrome (outer padding plus base spacing plus
/// rail padding). Subtracted with the player reserve to size the fill.
pub(crate) const RAIL_CHROME_PX: f32 = 48.0;
/// Collapsed rail capacity bounds: never fewer than the old fixed 6,
/// never more than a 4K-tall window could show.
pub(crate) const RAIL_MIN_CAP: usize = 6;
pub(crate) const RAIL_MAX_CAP: usize = 50;
/// Bottom strip reserved for the player bar: sidebar hover expand never
/// fires from inside this zone. Measured: status line (~16) + control
/// row (~50) + player padding (2x10) + spacings (~16) with margin.
/// The stacked minimal player is taller (see player_zone_h).
pub(crate) const PLAYER_ZONE_H: f32 = 105.0;
/// Minimal stacked player reserve (4 rows + status + padding).
pub(crate) const PLAYER_ZONE_MINIMAL_H: f32 = 210.0;
/// Player breakpoints on window width: single row down to 700,
/// tight single row down to 500, stacked below that.
pub(crate) const PLAYER_COMPACT_BELOW: f32 = 700.0;
pub(crate) const PLAYER_MINIMAL_BELOW: f32 = 500.0;

/// Player bar layout mode (responsive narrow/medium/wide).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlayerLayout {
    Wide,
    Compact,
    Minimal,
}

/// Window width to player layout (pure helper, unit-tested).
pub(crate) fn player_layout_mode(window_width: f32) -> PlayerLayout {
    if window_width < PLAYER_MINIMAL_BELOW {
        PlayerLayout::Minimal
    } else if window_width < PLAYER_COMPACT_BELOW {
        PlayerLayout::Compact
    } else {
        PlayerLayout::Wide
    }
}

/// Bottom-strip reserve for the current player height (the stacked
/// minimal player is taller than the single row).
impl AppState {
    pub(crate) fn player_zone_h(&self) -> f32 {
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        if player_layout_mode(ww) == PlayerLayout::Minimal {
            PLAYER_ZONE_MINIMAL_H
        } else {
            PLAYER_ZONE_H
        }
    }
}
/// Animation tick and duration slider bounds (Settings slider, ms).
pub(crate) const ANIM_TICK_MS: u64 = 16;
pub(crate) const ANIM_MIN_MS: u64 = 20;
pub(crate) const ANIM_MAX_MS: u64 = 400;
pub(crate) const ANIM_DEFAULT_MS: u64 = 130;
/// Total travels used to derive per-tick steps for a target duration.
pub(crate) const SIDEBAR_TRAVEL: f32 = 179.0;
/// Right worst case (285 to 0 when the rail is empty) so every right
/// glide finishes within the window; shorter glides snap early.
pub(crate) const RIGHT_TRAVEL: f32 = 285.0;
/// Per-tick steps for a target duration in ms at the 16ms tick:
/// (sidebar px, right px, unit 0..1). Widths floor at 1px.
pub(crate) fn anim_steps(anim_ms: u64) -> (f32, f32, f32) {
    let frames = ((anim_ms as f32 / ANIM_TICK_MS as f32).round() as u32).max(1) as f32;
    (
        (SIDEBAR_TRAVEL / frames).max(1.0),
        (RIGHT_TRAVEL / frames).max(1.0),
        1.0 / frames,
    )
}

/// Header-height reserve above the Now Playing sheet: measured header
/// row (22px icon + 2x10 padding = 42) plus the 4px column spacing.
/// Tied to the header control heights; the fully-open body layer drops
/// it (real header above) so both states meet with no snap.
pub(crate) const DRAWER_TOP_RESERVE: f32 = 46.0;
/// Center content cap on very wide windows (body only, header stays
/// full bleed so search math never drifts).
pub(crate) const CENTER_MAX_W: f32 = 1150.0;
/// Window width past which the center body centers with margins.
pub(crate) const CENTER_MARGIN_BREAK: f32 = 1600.0;

/// Ease `current` toward `target` by `step` (frame tick for drawer and
/// sidebar transitions). Snaps when closer than one step.
pub(crate) fn ease_toward(current: f32, target: f32, step: f32) -> f32 {
    if (target - current).abs() <= step {
        target
    } else {
        current + (target - current).signum() * step
    }
}

/// Format seconds as `m:ss` for the player bar.
pub(crate) fn fmt_time(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Play count plus duration for song rows ("213M plays · 3:45",
/// muted): raw All results carry no durations, so a zero one never
/// appends "· 0:00" (bare count instead). Blank count renders the
/// bare duration (blank when zero); blank both renders blank, so
/// count-less rows keep their exact current layout.
pub(crate) fn song_meta(plays: Option<&str>, duration_secs: u32) -> String {
    let duration = fmt_time(duration_secs as f64);
    match (
        plays.map(str::trim).filter(|s| !s.is_empty()),
        duration_secs,
    ) {
        (Some(p), 0) => p.to_string(),
        (Some(p), _) => format!("{p} · {duration}"),
        (None, 0) => String::new(),
        (None, _) => duration,
    }
}

/// Right-side meta cell for song rows (same "{plays} · {duration}"
/// idiom as the search popup): one widget in the duration slot, so
/// count-less rows render pixel-identical to before.
pub(crate) fn song_meta_text(
    plays: Option<&str>,
    duration_secs: u32,
) -> iced::Element<'static, Message> {
    use iced::widget::text;
    use iced::widget::text::Wrapping;
    text(song_meta(plays, duration_secs))
        .size(12)
        .color(MUTED)
        .wrapping(Wrapping::None)
        .into()
}
