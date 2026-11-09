//! yutu-ui helpers: split from lib.rs (no behavior change).

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

/// Parse a googlevideo `expire` query param (`…&expire=1791411186&…`) into a
/// unix timestamp. Unknown shapes → None (caller re-resolves every time).
pub(crate) fn url_expiry(url: &str) -> Option<u64> {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire=").and_then(|v| v.parse().ok()))
}

/// Scheduled sync cadence + startup grace (see [`sync_due`]): pulls
/// are cheap (staged, fail-open) but still network, so 15 minutes with
/// a 90s grace after launch.
#[cfg(feature = "innertube")]
pub(crate) const AUTO_SYNC_INTERVAL_SECS: u64 = 15 * 60;
#[cfg(feature = "innertube")]
pub(crate) const AUTO_SYNC_STARTUP_GRACE_SECS: u64 = 90;

/// True when a scheduled pull is due (pure time math, unit-tested; the
/// caller additionally requires a session and an idle pipeline).
#[cfg(feature = "innertube")]
pub(crate) fn sync_due(
    auto_sync: bool,
    last_sync: Option<std::time::Instant>,
    started_at: std::time::Instant,
    now: std::time::Instant,
) -> bool {
    if !auto_sync {
        return false;
    }
    match last_sync {
        Some(t) => now.duration_since(t).as_secs() >= AUTO_SYNC_INTERVAL_SECS,
        None => now.duration_since(started_at).as_secs() >= AUTO_SYNC_STARTUP_GRACE_SECS,
    }
}

/// Context-menu origin from the last cursor position, clamped so the
/// 300px menu never runs off screen. Falls back to a sane default when
/// the cursor/window are still unknown (e.g. keyboard-opened).
pub(crate) fn menu_pos(cursor: Option<iced::Point>, window: Option<iced::Size>) -> (f32, f32) {
    pub(crate) const MENU_W: f32 = 300.0;
    pub(crate) const MENU_H: f32 = 510.0;
    let (cx, cy) = cursor.map(|p| (p.x, p.y)).unwrap_or((200.0, 150.0));
    let (ww, hh) = window
        .map(|s| (s.width, s.height))
        .unwrap_or((1280.0, 800.0));
    (
        cx.min((ww - MENU_W).max(0.0)).max(0.0),
        cy.min((hh - MENU_H).max(0.0)).max(0.0),
    )
}

/// Commit mapping for an insertion slot: `slot == from` and `slot ==
/// from + 1` are the item's own spot (no-op); anything else maps to a
/// `move_to` index. `None` on no-op or out-of-range.
/// Insertion slots live between rows plus both edges (0 before the
/// first row, len after the last) so every song, including the middle
/// of 3, always has a committable move.
pub(crate) fn slot_commit(len: usize, from: usize, slot: usize) -> Option<usize> {
    if from >= len || slot > len {
        return None;
    }
    let to = if slot <= from { slot } else { slot - 1 };
    if to == from {
        None
    } else {
        Some(to)
    }
}

/// Preview order for an insertion slot (test helper for the commit map).
#[cfg(test)]
pub(crate) fn drag_slot_preview(len: usize, from: usize, slot: Option<usize>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    if let Some(slot) = slot {
        if let Some(to) = slot_commit(len, from, slot) {
            let item = order.remove(from);
            order.insert(to, item);
        }
    }
    order
}

/// Display assembly for a live YouTube-style drag: the origin row is held
/// by the floating ghost, so the list shows every other row plus one gap
/// at the commit target `to` (resolved with [`slot_commit`], own spot
/// falls back to `from`). Returns the display sequence (`Some(row)` or
/// `None` for the gap) plus the slot each lane arms: the lane before
/// display position `p` (and the trailing lane) always commits to the
/// post-removal index at that visual spot, so hovering a lane opens the
/// gap exactly where the item would land.
pub(crate) fn drag_display_order(
    len: usize,
    from: usize,
    to: usize,
) -> (Vec<Option<usize>>, Vec<usize>) {
    let to = to.min(len.saturating_sub(1));
    let mut seq: Vec<Option<usize>> = (0..len).filter(|&k| k != from).map(Some).collect();
    seq.insert(to.min(seq.len()), None);
    let mut lanes = Vec::with_capacity(len + 1);
    for p in 0..=seq.len() {
        let q = if p <= to { p } else { p.saturating_sub(1) };
        lanes.push(if q <= from { q } else { q + 1 });
    }
    (seq, lanes)
}

/// Grab offset of the floating ghost from the press point: the held row
/// keeps the exact grab offset it was picked up with, so it never jumps
/// on grab and then glides per-pixel with the cursor.
pub(crate) const GHOST_GRAB_DX_FALLBACK: f32 = 28.0;

/// Floating ghost origin from the live cursor, clamped to the window
/// (same Space-offset idiom as [`menu_pos`]): the held row glides
/// per-pixel under the pointer while the list snaps discretely beneath
/// it (Iced never reports widget geometry, so the slot itself stays
/// lane-discrete). `None` while the cursor is still unknown (no ghost
/// until it can be placed).
pub(crate) fn drag_ghost_pos(
    cursor: Option<iced::Point>,
    window: Option<iced::Size>,
    w: f32,
    h: f32,
    press_x: Option<f32>,
    list_left: f32,
) -> Option<(f32, f32)> {
    let c = cursor?;
    let (ww, hh) = window
        .map(|s| (s.width, s.height))
        .unwrap_or((1280.0, 800.0));
    let grab = press_x
        .map(|px| px - list_left)
        .unwrap_or(GHOST_GRAB_DX_FALLBACK)
        .clamp(0.0, w);
    Some((
        (c.x - grab).clamp(0.0, (ww - w).max(0.0)),
        (c.y - h / 2.0).clamp(0.0, (hh - h).max(0.0)),
    ))
}

/// Search cache TTL (s) and cap (queries, evict-all when full).
pub(crate) const SEARCH_CACHE_TTL_SECS: u64 = 300;
pub(crate) const SEARCH_CACHE_CAP: usize = 30;

/// Retry cooldown (s) for failed artwork/metadata fetches: completion
/// chaining re-scans the visible screenful on every arrival, so without
/// this a permanently failing URL or video would refetch in a loop.
pub(crate) const FAILED_COOLDOWN_SECS: u64 = 120;

/// Normalize a query for cache keys (case, edges, inner runs).
pub(crate) fn search_cache_key(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Truncate a title to `max` chars with an ellipsis (menu rows).
pub(crate) fn truncate_title(title: &str, max: usize) -> String {
    if title.chars().count() <= max {
        title.to_owned()
    } else {
        format!("{}…", title.chars().take(max).collect::<String>())
    }
}
