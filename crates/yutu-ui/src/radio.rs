//! yutu-ui radio: split from lib.rs (no behavior change).

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

/// Deterministic radio ordering: seed first, then same-artist tracks, then
/// the rest. Pure helper over caller-supplied (real) tracks.
pub fn order_radio(seed_id: &str, tracks: &[Track]) -> Vec<Track> {
    order_radio_impl(seed_id, tracks)
}

pub(crate) fn order_radio_impl(seed_id: &str, tracks: &[Track]) -> Vec<Track> {
    let start = tracks.iter().position(|t| t.id == seed_id).unwrap_or(0);
    let seed_artist = tracks
        .get(start)
        .map(|t| t.artist.clone())
        .unwrap_or_default();
    let mut seq = Vec::with_capacity(tracks.len());
    let seed_actual = tracks.get(start).map(|t| t.id.clone()).unwrap_or_default();
    if let Some(t) = tracks.get(start) {
        seq.push(t.clone());
    }
    for t in tracks {
        if t.id != seed_actual && t.artist == seed_artist {
            seq.push(t.clone());
        }
    }
    for t in tracks {
        if t.id != seed_actual && t.artist != seed_artist {
            seq.push(t.clone());
        }
    }
    seq
}

/// Fixture-backed radio for unit tests (never user-visible).
#[cfg(test)]
pub(crate) fn radio_for(seed_id: &str) -> Vec<Track> {
    order_radio(seed_id, &yutu_core::MockBackend::fixture_tracks())
}
