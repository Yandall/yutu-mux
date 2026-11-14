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

/// App version from the package manifest (About section, one place).
pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Project home (About section link, one place).
pub(crate) const GITHUB_URL: &str = "https://github.com/Yandall/yutu-mux";

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
/// Sort selector width on playlist detail: fits the longest pick
/// ("Newest first" at button weight) so the `...` trigger never shifts
/// when the pick changes.
pub(crate) const SORT_BUTTON_W: f32 = 144.0;
/// Context menu item height (icon 14 + 14px text at 1.3 line height +
/// 8px padding each side, rounded up): fixed so the playlist flyout
/// anchor math can never drift from the laid-out rows.
pub(crate) const MENU_ROW_H: f32 = 36.0;
/// Selector menu cap (total card height incl. padding): long pickers
/// (system fonts) stop here and scroll instead of stretching the
/// overlay past the viewport.
pub(crate) const SELECT_MENU_MAX_H: f32 = 320.0;
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

/// Window layout breakpoints (window width): side panels drop to
/// drawers below `LAYOUT_NARROW_BELOW`, to overlay rails below
/// `LAYOUT_MEDIUM_BELOW`, and grow from `LAYOUT_WIDE_FROM` up.
pub(crate) const LAYOUT_NARROW_BELOW: f32 = 800.0;
pub(crate) const LAYOUT_MEDIUM_BELOW: f32 = 1100.0;
pub(crate) const LAYOUT_WIDE_FROM: f32 = 1600.0;
/// Side panel widths: library rail / full / wide, queue full / wide
/// (the queue rail is `RIGHT_RAIL_W`).
pub(crate) const SIDEBAR_RAIL_W: f32 = 56.0;
pub(crate) const SIDEBAR_FULL_W: f32 = 235.0;
pub(crate) const SIDEBAR_WIDE_W: f32 = 260.0;
pub(crate) const RIGHT_FULL_W: f32 = 285.0;
pub(crate) const RIGHT_WIDE_W: f32 = 320.0;
/// Smallest window the app accepts: below this nothing has room.
pub(crate) const MIN_WINDOW_W: f32 = 360.0;
pub(crate) const MIN_WINDOW_H: f32 = 480.0;
/// Base window padding (left + right) and the row gap beside a side
/// panel slot. Shared by every center-width formula.
pub(crate) const WINDOW_PAD_X: f32 = 16.0;
pub(crate) const PANEL_GAP: f32 = 8.0;
/// Center width below which the header drops its back/forward arrows.
pub(crate) const HEADER_ARROWS_MIN_CENTER: f32 = 480.0;
/// Wide-mode body cap (Normal keeps `CENTER_MAX_W`).
pub(crate) const CENTER_MAX_W_WIDE: f32 = 1400.0;
/// Now Playing two-column layout: minimum content width, hero column
/// width (300px art plus breathing room), and the gap to the lyrics.
pub(crate) const NP_TWO_COL_MIN_CONTENT: f32 = 900.0;
pub(crate) const NP_HERO_COL_W: f32 = 320.0;
pub(crate) const NP_COL_GAP: f32 = 40.0;
/// Provider stepper height above the lyrics (two-column follow math).
pub(crate) const LYRIC_STEPPER_H: f32 = 30.0;
/// Now Playing backdrop: downscale edge (pixels) and cache size.
pub(crate) const ART_BLUR_PX: u32 = 8;
pub(crate) const ART_BLUR_CAP: usize = 64;
/// Collection page backdrop band height and cover strength.
pub(crate) const PAGE_BACKDROP_H: f32 = 460.0;
pub(crate) const PAGE_BACKDROP_OPACITY: f32 = 0.65;
/// How strongly the stretched cover shows through the sheet.
pub(crate) const NP_BACKDROP_OPACITY: f32 = 0.7;
/// Side slot width in the stacked player: holds the time labels on the
/// seek row and the mute button on the volume row, so both bars line up.
pub(crate) const MINI_SIDE_W: f32 = 40.0;
/// Content width below which album/artist headers stack, and the
/// stacked art size.
pub(crate) const HERO_STACK_BELOW: f32 = 560.0;
pub(crate) const HERO_STACKED_ART: f32 = 140.0;
/// Lyrics follow never resumes while the drawer sits within this many
/// pixels of the top (the user scrolled up on purpose).
pub(crate) const LYRIC_TOP_HOLD_PX: f32 = 8.0;
/// Header control height: pills, pill inputs and round buttons share
/// it (the 20px button radius makes a 40px button a true circle).
pub(crate) const PILL_H: f32 = 40.0;
/// Pill input vertical padding (line height fills the rest).
pub(crate) const PILL_INPUT_PAD_Y: f32 = 10.0;
/// Playlist header cover edge (side-by-side layout).
pub(crate) const PLAYLIST_COVER_PX: f32 = 190.0;
/// Content width below which song rows drop the heart button.
pub(crate) const ROW_HEART_MIN_CONTENT: f32 = 480.0;
/// Content width from which quick picks use two columns.
pub(crate) const QUICK_TWO_COL_MIN_CONTENT: f32 = 600.0;
/// Fixed chrome beside a song row's title (art, paddings, gaps,
/// duration, ellipsis and heart buttons), for the title budget.
pub(crate) const ROW_TITLE_CHROME: f32 = 200.0;

thread_local! {
    /// Per-frame title budget (chars) for the shared two-line row text:
    /// set from the content width at the top of `view`, read by the
    /// static text helpers, so narrow rows end in "…" instead of being
    /// cut mid-glyph. The UI is single-threaded, so one cell suffices.
    static ROW_TITLE_CHARS: std::cell::Cell<usize> = const { std::cell::Cell::new(45) };
}

/// Current row title budget in characters.
pub(crate) fn row_title_chars() -> usize {
    ROW_TITLE_CHARS.with(std::cell::Cell::get)
}

/// Title budget for a content width: the space left beside the row
/// chrome at ~7.3px per 14px glyph, never above the 45-char default
/// and never below a readable floor.
pub(crate) fn row_title_budget(content_w: f32) -> usize {
    let room = (content_w - ROW_TITLE_CHROME).max(0.0);
    ((room / 7.3) as usize).clamp(12, 45)
}

pub(crate) fn set_row_title_chars(n: usize) {
    ROW_TITLE_CHARS.with(|c| c.set(n));
}

/// Center body padding (each side), see `center_view`.
pub(crate) const BODY_PAD_X: f32 = 20.0;

/// Whole-window layout mode (responsive narrow/medium/normal/wide).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutMode {
    /// Side panels are drawers opened from header buttons.
    Narrow,
    /// Side panels are rails that expand over the content.
    Medium,
    /// Today's layout: pinned panels push the content.
    Normal,
    /// Wider panels and a wider content cap.
    Wide,
}

impl LayoutMode {
    /// Expanded side panels float over the content instead of
    /// taking space from it.
    pub(crate) fn panels_overlay(self) -> bool {
        matches!(self, LayoutMode::Narrow | LayoutMode::Medium)
    }
}

/// Window width to layout mode (pure helper, unit-tested).
pub(crate) fn layout_mode(window_width: f32) -> LayoutMode {
    if window_width < LAYOUT_NARROW_BELOW {
        LayoutMode::Narrow
    } else if window_width < LAYOUT_MEDIUM_BELOW {
        LayoutMode::Medium
    } else if window_width < LAYOUT_WIDE_FROM {
        LayoutMode::Normal
    } else {
        LayoutMode::Wide
    }
}

impl AppState {
    /// Live window width (1280 before the first resize event).
    pub(crate) fn window_w(&self) -> f32 {
        self.window.map(|s| s.width).unwrap_or(1280.0)
    }

    /// Current whole-window layout mode.
    pub(crate) fn layout(&self) -> LayoutMode {
        layout_mode(self.window_w())
    }

    /// Expanded library panel width for the current mode (the Narrow
    /// drawer leaves a strip of content visible beside it).
    pub(crate) fn sidebar_full_w(&self) -> f32 {
        match self.layout() {
            LayoutMode::Narrow => (self.window_w() - 56.0).min(300.0),
            LayoutMode::Wide => SIDEBAR_WIDE_W,
            _ => SIDEBAR_FULL_W,
        }
    }

    /// Expanded queue panel width for the current mode (the Narrow
    /// drawer goes full width on the smallest windows).
    pub(crate) fn right_full_w(&self) -> f32 {
        let ww = self.window_w();
        match self.layout() {
            LayoutMode::Narrow if ww < 500.0 => (ww - WINDOW_PAD_X).max(0.0),
            LayoutMode::Narrow => (ww - 56.0).min(360.0),
            LayoutMode::Wide => RIGHT_WIDE_W,
            _ => RIGHT_FULL_W,
        }
    }

    /// Width the left panel takes from the center row: the animated
    /// panel itself when it pushes the content, the fixed rail when it
    /// floats over it (Medium), nothing at all for a drawer (Narrow).
    pub(crate) fn sidebar_slot_w(&self) -> f32 {
        match self.layout() {
            LayoutMode::Narrow => 0.0,
            LayoutMode::Medium => SIDEBAR_RAIL_W,
            _ => self.sidebar_w,
        }
    }

    /// Width the right panel takes from the center row (same rules).
    pub(crate) fn right_slot_w(&self) -> f32 {
        match self.layout() {
            LayoutMode::Narrow => 0.0,
            LayoutMode::Medium => RIGHT_RAIL_W,
            _ => self.right_w,
        }
    }

    /// A panel floats open over the content (Medium hover expand or a
    /// Narrow drawer).
    pub(crate) fn drawers_open(&self) -> bool {
        self.layout().panels_overlay() && (self.sidebar_latched || self.right_latched)
    }

    /// Close floating panels (scrim click, Esc, navigation). Push
    /// modes keep their hover latch: this only touches overlay modes.
    pub(crate) fn close_drawers(&mut self) {
        if self.layout().panels_overlay() {
            self.sidebar_latched = false;
            self.right_latched = false;
        }
    }

    /// Animated left panel width drawn above the content (Medium hover
    /// expand, Narrow drawer); `None` while nothing floats.
    pub(crate) fn sidebar_overlay_w(&self) -> Option<f32> {
        (self.layout().panels_overlay() && self.sidebar_w > self.sidebar_slot_w() + 0.5)
            .then_some(self.sidebar_w)
    }

    /// Same for the queue panel.
    pub(crate) fn right_overlay_w(&self) -> Option<f32> {
        (self.layout().panels_overlay() && self.right_w > self.right_slot_w() + 0.5)
            .then_some(self.right_w)
    }

    /// Center column width: window minus padding, side panel slots
    /// and the gap beside each present slot. One formula for every
    /// caller, so geometry never drifts between views.
    pub(crate) fn center_width(&self) -> f32 {
        let gap = |w: f32| if w > 0.0 { w + PANEL_GAP } else { 0.0 };
        (self.window_w() - WINDOW_PAD_X - gap(self.sidebar_slot_w()) - gap(self.right_slot_w()))
            .max(0.0)
    }

    /// Body width cap (very wide windows center a capped body). Wide
    /// mode lets the body grow further before the margins start.
    pub(crate) fn content_cap(&self) -> f32 {
        if self.layout() == LayoutMode::Wide {
            CENTER_MAX_W_WIDE
        } else {
            CENTER_MAX_W
        }
    }

    /// Song rows keep their heart button only with room to spare.
    pub(crate) fn rows_show_heart(&self) -> bool {
        self.content_width() >= ROW_HEART_MIN_CONTENT
    }

    /// Album/artist headers stack art above the title when narrow.
    pub(crate) fn hero_stacked(&self) -> bool {
        self.content_width() < HERO_STACK_BELOW
    }

    /// Single-column quick picks on narrow content.
    pub(crate) fn quick_picks_cols(&self) -> usize {
        if self.content_width() < QUICK_TWO_COL_MIN_CONTENT {
            1
        } else {
            2
        }
    }

    /// Now Playing lyrics column: the center minus breathing room,
    /// held between a readable floor and a comfortable line length.
    pub(crate) fn lyrics_col_w(&self) -> f32 {
        if self.np_two_col() {
            (self.content_width() - NP_HERO_COL_W - NP_COL_GAP).clamp(280.0, 640.0)
        } else {
            (self.center_width() - 64.0).clamp(280.0, 560.0)
        }
    }

    /// Now Playing puts art and lyrics side by side on wide content.
    pub(crate) fn np_two_col(&self) -> bool {
        self.content_width() >= NP_TWO_COL_MIN_CONTENT
    }

    /// Usable body width inside the center padding, capped.
    pub(crate) fn content_width(&self) -> f32 {
        (self.center_width().min(self.content_cap()) - 2.0 * BODY_PAD_X).max(0.0)
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

/// Widget id of the header search input (focus probe + cursor ops).
pub(crate) const SEARCH_INPUT_ID: &str = "yutu-search";
/// Widget id of the playlist page search input (focus probe).
pub(crate) const PL_SEARCH_INPUT_ID: &str = "yutu-pl-search";
/// Header pill chrome around the placeholder text: pill padding
/// (8 + 4), magnifier (14), input padding (2 x 10), row gaps (3 x 4),
/// empty clear slot (1), submit button (14 + 2 x 6), plus breathing
/// room so the placeholder never touches the button.
pub(crate) const SEARCH_PILL_CHROME: f32 = 8.0 + 4.0 + 14.0 + 20.0 + 12.0 + 1.0 + 26.0 + 16.0;
/// Playlist input chrome: padding (2 x 10), border, breathing room.
pub(crate) const PL_SEARCH_CHROME: f32 = 20.0 + 2.0 + 16.0;

/// Search field grow/shrink duration: fixed (not the Settings speed,
/// which is tuned for sidebar travel and reads as a jump on a field),
/// still instant when animations are off.
pub(crate) const SEARCH_ANIM_MS: f32 = 120.0;

/// Cubic ease-in-out on 0..1 (search field widths): gentle start and
/// landing, symmetric so a mid-flight reversal never jumps.
pub(crate) fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

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

/// Total length for collection headers: "2 h 51 min", "51 min", or
/// "1 min" for anything shorter (never "0 min" for a non-empty list).
pub(crate) fn fmt_total(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{} min", m.max(u64::from(secs > 0)))
    }
}

/// Song length for list rows: blank when unknown (0), never a fake
/// "0:00" (several YouTube sources carry no duration).
pub(crate) fn fmt_len(secs: u32) -> String {
    if secs == 0 {
        String::new()
    } else {
        fmt_time(secs as f64)
    }
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
