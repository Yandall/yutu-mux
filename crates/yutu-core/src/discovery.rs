//! Local-history discovery mixes (pure functions, no I/O).
//!
//! Both helpers are deterministic: ties break by track id ascending so
//! repeated calls over the same inputs return the same order.

use std::collections::{HashMap, HashSet};

use crate::models::{HistoryEntry, Track};

const DAY_SECS: i64 = 86_400;
const RECENT_SKIP_SECS: i64 = DAY_SECS;
const FORGOTTEN_SECS: i64 = 30 * DAY_SECS;
const LIKED_TRACK_BONUS: f64 = 1.5;
const LIKED_ARTIST_BONUS: f64 = 1.0;

/// Weekly mix from local history.
///
/// Scoring per catalog track:
/// - each past play contributes `1 / (1 + age_days)` where
///   `age_days = max(0, now - played_at) / 86400`, so recent plays weigh more;
/// - `+1.5` when the track id is in `liked`;
/// - `+1.0` when the track's artist appears on any liked catalog track
///   (liked-artist boost).
///
/// Tracks played within the last 24h (`now - played_at < 86400`, including
/// timestamps in the future) are skipped. Only tracks with a score above
/// zero are returned (i.e. played at least once or liked / liked-artist),
/// ordered by score descending with track-id ascending tie-breaks,
/// truncated to `n` (`n == 0` yields an empty mix).
pub fn weekly_mix(
    history: &[HistoryEntry],
    liked: &HashSet<String>,
    catalog: &[Track],
    n: usize,
    now_unix: i64,
) -> Vec<Track> {
    if n == 0 || catalog.is_empty() {
        return Vec::new();
    }

    let liked_artists: HashSet<&str> = catalog
        .iter()
        .filter(|t| liked.contains(&t.id))
        .map(|t| t.artist.as_str())
        .collect();

    let mut last_played: HashMap<&str, i64> = HashMap::new();
    let mut recency_sum: HashMap<&str, f64> = HashMap::new();
    for entry in history {
        let age = (now_unix - entry.played_at_unix).max(0) as f64 / DAY_SECS as f64;
        let weight = 1.0 / (1.0 + age);
        *recency_sum.entry(entry.track_id.as_str()).or_insert(0.0) += weight;
        last_played
            .entry(entry.track_id.as_str())
            .and_modify(|last| *last = (*last).max(entry.played_at_unix))
            .or_insert(entry.played_at_unix);
    }

    let mut scored: Vec<(&Track, f64)> = Vec::new();
    for track in catalog {
        if let Some(last) = last_played.get(track.id.as_str()) {
            if now_unix - *last < RECENT_SKIP_SECS {
                continue;
            }
        }
        let mut score = recency_sum.get(track.id.as_str()).copied().unwrap_or(0.0);
        if liked.contains(&track.id) {
            score += LIKED_TRACK_BONUS;
        }
        if liked_artists.contains(track.artist.as_str()) {
            score += LIKED_ARTIST_BONUS;
        }
        if score > 0.0 {
            scored.push((track, score));
        }
    }

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    scored.into_iter().take(n).map(|(t, _)| t.clone()).collect()
}

/// Liked tracks unplayed for 30+ days (or never played).
///
/// `liked` holds track ids; `catalog` resolves them to [`Track`]s (unknown
/// ids are skipped). A track is forgotten when it has no history entry or
/// when `now - last_played >= 30 * 86400`. Results are ordered with
/// never-played first, then by last-played ascending (longest without a
/// play first), with track-id ascending tie-breaks.
pub fn forgotten_favorites(
    liked: &HashSet<String>,
    history: &[HistoryEntry],
    catalog: &[Track],
    now_unix: i64,
) -> Vec<Track> {
    let mut last_played: HashMap<&str, i64> = HashMap::new();
    for entry in history {
        last_played
            .entry(entry.track_id.as_str())
            .and_modify(|last| *last = (*last).max(entry.played_at_unix))
            .or_insert(entry.played_at_unix);
    }

    let mut out: Vec<(&Track, Option<i64>)> = Vec::new();
    for track in catalog {
        if !liked.contains(&track.id) {
            continue;
        }
        match last_played.get(track.id.as_str()) {
            None => out.push((track, None)),
            Some(last) => {
                if now_unix - *last >= FORGOTTEN_SECS {
                    out.push((track, Some(*last)));
                }
            }
        }
    }

    out.sort_by(|a, b| match (a.1, b.1) {
        (None, None) => a.0.id.cmp(&b.0.id),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.0.id.cmp(&b.0.id)),
    });
    out.into_iter().map(|(t, _)| t.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, artist: &str) -> Track {
        Track {
            id: id.to_owned(),
            title: format!("title-{id}"),
            artist: artist.to_owned(),
            album: None,
            duration_secs: 200,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
        }
    }

    #[test]
    fn mix_skips_played_in_last_24h() {
        let now = 1_700_000_000;
        let catalog = vec![track("a", "X"), track("b", "Y")];
        let history = vec![
            HistoryEntry {
                track_id: "a".into(),
                played_at_unix: now - 3_600, // 1h ago -> skip
            },
            HistoryEntry {
                track_id: "b".into(),
                played_at_unix: now - 2 * DAY_SECS,
            },
        ];
        let liked = HashSet::new();
        let mix = weekly_mix(&history, &liked, &catalog, 10, now);
        assert_eq!(mix.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn mix_prefers_recent_plays() {
        let now = 1_700_000_000;
        let catalog = vec![track("old", "X"), track("new", "X")];
        let history = vec![
            HistoryEntry {
                track_id: "old".into(),
                played_at_unix: now - 10 * DAY_SECS,
            },
            HistoryEntry {
                track_id: "new".into(),
                played_at_unix: now - 2 * DAY_SECS,
            },
        ];
        let liked = HashSet::new();
        let mix = weekly_mix(&history, &liked, &catalog, 10, now);
        assert_eq!(mix[0].id, "new");
        assert_eq!(mix[1].id, "old");
    }

    #[test]
    fn mix_liked_artist_boost_orders_deterministically() {
        let now = 1_700_000_000;
        // No history: only liked-artist tracks score; tie broken by id.
        let catalog = vec![
            track("b2", "Loved"),
            track("b1", "Loved"),
            track("c1", "Other"),
        ];
        let liked: HashSet<String> = ["b1".into()].iter().cloned().collect();
        let mix = weekly_mix(&[], &liked, &catalog, 10, now);
        // b1 (track + artist bonus) first, then b2 (artist bonus), c1 unscored.
        assert_eq!(
            mix.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["b1", "b2"]
        );
    }

    #[test]
    fn mix_accumulates_repeat_plays_and_caps_at_n() {
        let now = 1_700_000_000;
        let catalog = vec![track("a", "X"), track("b", "X"), track("c", "X")];
        let history = vec![
            HistoryEntry {
                track_id: "a".into(),
                played_at_unix: now - 5 * DAY_SECS,
            },
            HistoryEntry {
                track_id: "a".into(),
                played_at_unix: now - 6 * DAY_SECS,
            },
            HistoryEntry {
                track_id: "b".into(),
                played_at_unix: now - 5 * DAY_SECS,
            },
            HistoryEntry {
                track_id: "c".into(),
                played_at_unix: now - 5 * DAY_SECS,
            },
        ];
        let liked = HashSet::new();
        let mix = weekly_mix(&history, &liked, &catalog, 2, now);
        // "a" has two plays so it outranks the tied b/c; n caps length at 2.
        assert_eq!(
            mix.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn mix_empty_inputs() {
        let liked = HashSet::new();
        let catalog = vec![track("a", "X")];
        assert!(weekly_mix(&[], &liked, &[], 5, 0).is_empty());
        assert!(weekly_mix(&[], &liked, &catalog, 0, 0).is_empty());
        // No history and no likes -> no scores -> empty mix.
        assert!(weekly_mix(&[], &liked, &catalog, 5, 0).is_empty());
    }

    #[test]
    fn forgotten_finds_stale_and_never_played() {
        let now = 1_700_000_000;
        let catalog = vec![
            track("stale", "X"),
            track("fresh", "X"),
            track("never", "X"),
            track("unliked", "X"),
        ];
        let liked: HashSet<String> = ["stale", "fresh", "never"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let history = vec![
            HistoryEntry {
                track_id: "stale".into(),
                played_at_unix: now - 40 * DAY_SECS,
            },
            HistoryEntry {
                track_id: "fresh".into(),
                played_at_unix: now - 5 * DAY_SECS,
            },
        ];
        let out = forgotten_favorites(&liked, &history, &catalog, now);
        // Never-played first, then oldest play; fresh + unliked excluded.
        assert_eq!(
            out.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["never", "stale"]
        );
    }

    #[test]
    fn forgotten_boundary_is_30_days() {
        let now = 1_700_000_000;
        let catalog = vec![track("edge", "X"), track("young", "X")];
        let liked: HashSet<String> = ["edge", "young"].iter().map(|s| (*s).to_owned()).collect();
        let history = vec![
            HistoryEntry {
                track_id: "edge".into(),
                played_at_unix: now - 30 * DAY_SECS,
            },
            HistoryEntry {
                track_id: "young".into(),
                played_at_unix: now - 30 * DAY_SECS + 1,
            },
        ];
        let out = forgotten_favorites(&liked, &history, &catalog, now);
        assert_eq!(
            out.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["edge"]
        );
    }
}
