//! Song matching for playlist imports: a music video row ("Artist - Song
//! (Official Video)" on an `ArtistVEVO` channel) is swapped for the
//! matching song (audio track) when a confident match exists, and rows
//! from other platforms (Spotify, Apple Music, Deezer, CSV) are found on
//! YouTube Music the same way, song versions first.
//!
//! Pure string logic, so the rules stay unit-testable; the network side
//! (one filtered song search per video) lives in `innertube.rs`.
//! Scoring reuses the lyrics matcher ([`score_candidate`]): title match
//! dominates, artist confirms, duration distance penalizes.

use crate::lyrics_providers::{score_candidate, MatchCandidate, MatchQuery};
use crate::models::Track;
use crate::playlist_sources::ForeignTrack;

/// Minimum [`score_candidate`] score for a swap: an exact title needs at
/// least a partial artist match, a contained title needs the exact
/// artist. Anything weaker keeps the video (a wrong song is worse than
/// the right video).
const SWAP_MIN_SCORE: i64 = 110;

/// Version markers that make a different recording: a candidate carrying
/// one the video title does not is never a swap (normalization strips
/// "(Live)" style asides, so the scorer alone would call them exact).
const VERSION_MARKERS: [&str; 12] = [
    "live",
    "remix",
    "cover",
    "karaoke",
    "instrumental",
    "acoustic",
    "sped up",
    "slowed",
    "nightcore",
    "8d",
    "demo",
    "reverb",
];

/// Clean artist from a video's channel name: drops the auto-generated
/// `- Topic` suffix and the `VEVO` / `Official` / `Music` channel tags.
pub fn clean_channel(channel: &str) -> String {
    let mut s = channel.trim();
    if let Some(head) = s.strip_suffix(" - Topic") {
        s = head;
    }
    let mut out = s.to_string();
    for tag in ["VEVO", "Vevo", "vevo"] {
        if let Some(head) = out.strip_suffix(tag) {
            out = head.trim_end().to_string();
        }
    }
    for tag in [" Official", " official", " Music", " music"] {
        if let Some(head) = out.strip_suffix(tag) {
            out = head.trim_end().to_string();
        }
    }
    if out.is_empty() {
        channel.trim().to_string()
    } else {
        out
    }
}

/// Artist + title a video most likely stands for. Video titles commonly
/// read `Artist - Song (Official Video)`: the left side wins as artist
/// then (see [`split_artist_title`] for the accepted separators);
/// otherwise the cleaned channel name is the artist.
pub fn video_artist_title(video: &Track) -> (String, String) {
    split_artist_title(&video.title)
        .unwrap_or_else(|| (clean_channel(&video.artist), video.title.trim().to_string()))
}

/// `Artist <sep> Song` split of a video title, first separator that
/// leaves both sides non-empty. Spaced dashes and `~`, `|`, `:` first;
/// tight dashes (`Iron Maiden-The Trooper`, `System Of A Down- X`) only
/// when one side has a space, so `Spider-Man` stays one word. Asides on
/// the artist side (`[HQ] Powerwolf`) are dropped.
fn split_artist_title(title: &str) -> Option<(String, String)> {
    const SPACED: [&str; 7] = [" - ", " – ", " — ", " ~ ", " | ", ": ", " -- "];
    const TIGHT: [&str; 3] = ["- ", " -", "-"];
    let pick = |sep: &str, tight: bool| {
        let (left, right) = title.split_once(sep)?;
        let left = strip_asides(left);
        let right = right.trim();
        if left.is_empty() || right.is_empty() {
            return None;
        }
        if tight && sep == "-" && !left.contains(' ') && !right.contains(' ') {
            return None;
        }
        Some((left, right.to_string()))
    };
    SPACED
        .iter()
        .find_map(|sep| pick(sep, false))
        .or_else(|| TIGHT.iter().find_map(|sep| pick(sep, true)))
}

/// Matching form of a title: accents folded and `&` read as `and`
/// (`Girls & Boys` is the song of the `Girls and Boys` video). Titles
/// only: in artist lists `&` joins names, as `,` does.
fn match_form(s: &str) -> String {
    fold_accents(s).replace('&', " and ")
}

/// Song search text for a video (`artist title`, asides kept out so the
/// search ranks the plain recording first).
pub fn song_query(video: &Track) -> String {
    let (artist, title) = video_artist_title(video);
    let title = strip_asides(&title);
    format!("{artist} {title}").trim().to_string()
}

/// Best song among `candidates` for `video`, or `None` when nothing is
/// confident enough. Only songs (never other videos) qualify; versions
/// the video is not (live, remix, cover, ...) never qualify.
pub fn best_song_match<'a>(video: &Track, candidates: &'a [Track]) -> Option<&'a Track> {
    let (artist, title) = video_artist_title(video);
    // Both readings: split title (`Artist - Song`) and channel artist,
    // so a "Song - Artist" title or a featured-artist channel still hits.
    let channel = clean_channel(&video.artist);
    best_match(
        &[
            (artist.as_str(), title.as_str()),
            (channel.as_str(), video.title.as_str()),
        ],
        video.duration_secs,
        &video.title,
        candidates.iter().filter(|c| c.id != video.id),
        false,
    )
}

/// Best YouTube Music match for a row from another platform (or a CSV
/// line). Songs only unless `allow_videos` (the fallback when a title
/// has no song release); videos are scored on their cleaned
/// `Artist - Song` reading, not the raw video title.
pub fn best_foreign_match<'a>(
    row: &ForeignTrack,
    candidates: &'a [Track],
    allow_videos: bool,
) -> Option<&'a Track> {
    let primary = primary_artist(&row.artist);
    best_match(
        &[
            (primary.as_str(), row.title.as_str()),
            (row.artist.as_str(), row.title.as_str()),
        ],
        row.duration_secs,
        &row.title,
        candidates.iter(),
        allow_videos,
    )
}

/// Search text for a foreign row: primary artist + title, asides out.
pub fn foreign_query(row: &ForeignTrack) -> String {
    format!(
        "{} {}",
        primary_artist(&row.artist),
        strip_asides(&row.title)
    )
    .trim()
    .to_string()
}

/// First credited artist of a joined list (`A, B`, `A & B`, `A feat. B`).
pub fn primary_artist(artists: &str) -> String {
    let lower = artists.to_lowercase();
    let cut = [",", " & ", " feat", " ft.", ";", " / "]
        .iter()
        .filter_map(|m| lower.find(m))
        .min()
        .unwrap_or(artists.len());
    artists[..cut].trim().to_string()
}

/// Shared scorer: every query reading against every candidate, best
/// score wins (ties keep the server's order), under the swap threshold
/// nothing wins. With no artist known, an exact title is enough.
fn best_match<'a>(
    queries: &[(&str, &str)],
    duration: u32,
    want_title: &str,
    candidates: impl Iterator<Item = &'a Track>,
    allow_videos: bool,
) -> Option<&'a Track> {
    let want_lower = match_form(&want_title.to_lowercase());
    let no_artist = queries.iter().all(|(a, _)| a.trim().is_empty());
    // Accent-blind on both sides: catalogs disagree on diacritics
    // ("Olvidarás" vs "Olvidaras", "Colón" vs "Colon"), and the scorer
    // would read those as different words.
    let queries: Vec<(String, String)> = queries
        .iter()
        .map(|(a, t)| (fold_accents(a), match_form(t)))
        .collect();
    let min = if no_artist { 100 } else { SWAP_MIN_SCORE };
    candidates
        .filter(|c| !c.id.is_empty() && (allow_videos || !c.is_video))
        .filter(|c| {
            // Markers count on the artist too: karaoke and cover
            // catalogs tag the artist ("The Rolling Stones (Karaoke)").
            let got = match_form(&format!("{} {}", c.title, c.artist).to_lowercase());
            !VERSION_MARKERS
                .iter()
                .any(|m| has_word(&got, m) && !has_word(&want_lower, m))
        })
        .filter_map(|c| {
            // Videos read as `Artist - Song` with a cleaned channel.
            let (artist, title) = if c.is_video {
                video_artist_title(c)
            } else {
                (c.artist.clone(), c.title.clone())
            };
            let (artist, title) = (fold_accents(&artist), match_form(&title));
            let candidate = MatchCandidate {
                track_name: &title,
                artist_name: &artist,
                album_name: c.album.as_deref().unwrap_or(""),
                duration: c.duration_secs,
            };
            queries
                .iter()
                .map(|(a, t)| {
                    score_candidate(
                        &candidate,
                        &MatchQuery {
                            artist: a.as_str(),
                            title: t.as_str(),
                            album: None,
                            duration,
                        },
                    )
                })
                .max()
                .filter(|s| *s >= min)
                .map(|s| (s, c))
        })
        .fold(None::<(i64, &Track)>, |best, (s, c)| match best {
            Some((bs, _)) if bs >= s => best,
            _ => Some((s, c)),
        })
        .map(|(_, c)| c)
}

/// Drop repeated rows (same normalized title + primary artist), keeping
/// the first of each in order. Returns the unique rows and how many
/// repeats were dropped: playlists often hold a song twice, and every
/// repeat would cost a search and land on the same YouTube Music song.
pub fn dedupe_foreign(rows: Vec<ForeignTrack>) -> (Vec<ForeignTrack>, usize) {
    use crate::lyrics_providers::normalize_for_match;
    let total = rows.len();
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<ForeignTrack> = rows
        .into_iter()
        .filter(|r| {
            seen.insert((
                normalize_for_match(&fold_accents(&r.title)),
                normalize_for_match(&fold_accents(&primary_artist(&r.artist))),
            ))
        })
        .collect();
    let repeats = total - unique.len();
    (unique, repeats)
}

/// Find every foreign row on YouTube Music: a song search first, then
/// (no confident song) a general search where videos may stand in.
/// Order is kept; rows with no confident match come back separately.
/// `on_done` runs once per row (progress).
#[cfg(feature = "innertube")]
pub async fn resolve_foreign<F>(
    rows: Vec<ForeignTrack>,
    auth: &crate::raw_search::SearchAuth,
    mut on_done: F,
) -> (Vec<Track>, Vec<ForeignTrack>)
where
    F: FnMut(usize, usize),
{
    use futures::stream::{self, StreamExt};
    const IN_FLIGHT: usize = 6;
    let total = rows.len();
    let mut found = Vec::with_capacity(total);
    let mut missing = Vec::new();
    let mut results = stream::iter(rows.into_iter().map(|row| async move {
        let query = foreign_query(&row);
        let song = crate::raw_search::raw_search_filtered(
            &query,
            auth,
            crate::raw_search::SearchFilterKind::Songs,
        )
        .await
        .ok()
        .and_then(|all| best_foreign_match(&row, &all.tracks, false).cloned());
        let hit = match song {
            Some(song) => Some(song),
            None => crate::raw_search::raw_search_all(&query, auth)
                .await
                .ok()
                .and_then(|all| best_foreign_match(&row, &all.tracks, true).cloned()),
        };
        (row, hit)
    }))
    .buffered(IN_FLIGHT);
    let mut done = 0;
    while let Some((row, hit)) = results.next().await {
        done += 1;
        on_done(done, total);
        match hit {
            Some(t) => found.push(t),
            None => missing.push(row),
        }
    }
    (found, missing)
}

/// Swap every music video in `tracks` for its song version when a
/// confident match exists (one filtered song search per video, a few in
/// flight at once, order kept). Songs pass through untouched, and a
/// failed or weak search keeps the video. Returns the new list plus the
/// number of swaps. `on_done` runs once per video searched (progress).
#[cfg(feature = "innertube")]
pub async fn resolve_song_versions<F>(
    tracks: Vec<Track>,
    auth: &crate::raw_search::SearchAuth,
    mut on_done: F,
) -> (Vec<Track>, usize)
where
    F: FnMut(usize, usize),
{
    use futures::stream::{self, StreamExt};
    /// Parallel searches: quick on 100+ video lists without hammering.
    const IN_FLIGHT: usize = 6;
    let total = tracks.iter().filter(|t| t.is_video).count();
    if total == 0 {
        return (tracks, 0);
    }
    let mut done = 0;
    let mut swapped = 0;
    let mut out = Vec::with_capacity(tracks.len());
    let mut results = stream::iter(tracks.into_iter().map(|t| async move {
        if !t.is_video {
            return (t, None);
        }
        let query = song_query(&t);
        let found = crate::raw_search::raw_search_filtered(
            &query,
            auth,
            crate::raw_search::SearchFilterKind::Songs,
        )
        .await
        .ok()
        .and_then(|all| best_song_match(&t, &all.tracks).cloned());
        (t, Some(found))
    }))
    .buffered(IN_FLIGHT);
    while let Some((original, searched)) = results.next().await {
        match searched {
            None => out.push(original),
            Some(found) => {
                done += 1;
                on_done(done, total);
                match found {
                    Some(song) => {
                        swapped += 1;
                        out.push(song);
                    }
                    None => out.push(original),
                }
            }
        }
    }
    (out, swapped)
}

/// Latin letters without their diacritics (`á` -> `a`, `ñ` -> `n`,
/// `ç` -> `c`, `ø` -> `o`, `ß` -> `ss`, ...), case kept; anything else
/// passes through. Matching only: titles shown to the user keep theirs.
///
/// Both Unicode spellings fold: precomposed letters (`ú`, one char) and
/// decomposed ones (`u` + combining U+0301, as Apple Music sends some
/// titles), whose combining marks are dropped. Left in, a mark reads as
/// a word break to the scorer ("sedu ceme" vs "seduceme").
pub fn fold_accents(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_combining_mark(c) {
            continue;
        }
        let base = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "A",
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ė' | 'Ę' | 'Ě' => "E",
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
            'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' | 'Į' | 'İ' => "I",
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => "o",
            'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ø' | 'Ō' | 'Ő' => "O",
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
            'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ū' | 'Ů' | 'Ű' | 'Ų' => "U",
            'ñ' | 'ń' | 'ň' => "n",
            'Ñ' | 'Ń' | 'Ň' => "N",
            'ç' | 'ć' | 'č' => "c",
            'Ç' | 'Ć' | 'Č' => "C",
            'ý' | 'ÿ' => "y",
            'Ý' | 'Ÿ' => "Y",
            'š' | 'ś' | 'ş' => "s",
            'Š' | 'Ś' | 'Ş' => "S",
            'ž' | 'ź' | 'ż' => "z",
            'Ž' | 'Ź' | 'Ż' => "Z",
            'ł' => "l",
            'Ł' => "L",
            'ř' => "r",
            'Ř' => "R",
            'ğ' => "g",
            'Ğ' => "G",
            'đ' => "d",
            'Đ' => "D",
            'ß' => "ss",
            'æ' => "ae",
            'Æ' => "AE",
            'œ' => "oe",
            'Œ' => "OE",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(base);
    }
    out
}

/// Combining diacritical marks (the accents of decomposed letters).
fn is_combining_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

/// `needle` as a whole word (or phrase) inside `hay` (both lowercase).
fn has_word(hay: &str, needle: &str) -> bool {
    hay.match_indices(needle).any(|(i, _)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Title without `(...)` / `[...]` asides, whitespace collapsed.
fn strip_asides(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut depth: u32 = 0;
    for ch in title.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, title: &str, artist: &str, secs: u32, video: bool) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration_secs: secs,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
            is_video: video,
        }
    }

    #[test]
    fn channel_tags_are_cleaned() {
        assert_eq!(clean_channel("RickAstleyVEVO"), "RickAstley");
        assert_eq!(clean_channel("Daft Punk - Topic"), "Daft Punk");
        assert_eq!(clean_channel("Coldplay Official"), "Coldplay");
        assert_eq!(clean_channel("VEVO"), "VEVO");
    }

    #[test]
    fn video_titles_split_into_artist_and_song() {
        let v = t(
            "v1",
            "Rick Astley - Never Gonna Give You Up (Official Video)",
            "RickAstleyVEVO",
            213,
            true,
        );
        let (a, s) = video_artist_title(&v);
        assert_eq!(a, "Rick Astley");
        assert_eq!(s, "Never Gonna Give You Up (Official Video)");
        assert_eq!(song_query(&v), "Rick Astley Never Gonna Give You Up");
        let plain = t("v2", "Yellow", "Coldplay", 270, true);
        assert_eq!(song_query(&plain), "Coldplay Yellow");
    }

    #[test]
    fn the_studio_song_beats_live_and_other_videos() {
        let v = t(
            "v1",
            "Rick Astley - Never Gonna Give You Up (Official Video)",
            "RickAstleyVEVO",
            213,
            true,
        );
        let candidates = [
            t("x0", "Never Gonna Give You Up", "Rick Astley", 213, true),
            t(
                "x1",
                "Never Gonna Give You Up (Live)",
                "Rick Astley",
                230,
                false,
            ),
            t("x2", "Never Gonna Give You Up", "Rick Astley", 214, false),
        ];
        assert_eq!(best_song_match(&v, &candidates).unwrap().id, "x2");
    }

    #[test]
    fn weak_matches_keep_the_video() {
        let v = t("v1", "Yellow", "Coldplay", 270, true);
        // Same title, wrong artist: not a swap.
        let wrong_artist = [t("x1", "Yellow", "Some Cover Band", 268, false)];
        assert!(best_song_match(&v, &wrong_artist).is_none());
        // Different song by the same artist: not a swap.
        let wrong_song = [t("x2", "Clocks", "Coldplay", 307, false)];
        assert!(best_song_match(&v, &wrong_song).is_none());
        // Remix the video is not: not a swap.
        let remix = [t("x3", "Yellow (Remix)", "Coldplay", 270, false)];
        assert!(best_song_match(&v, &remix).is_none());
        assert!(best_song_match(&v, &[]).is_none());
    }

    #[test]
    fn a_live_video_may_match_a_live_song() {
        let v = t("v1", "Coldplay - Yellow (Live)", "Coldplay", 300, true);
        let live = [t("x1", "Yellow (Live)", "Coldplay", 301, false)];
        assert_eq!(best_song_match(&v, &live).unwrap().id, "x1");
    }

    fn row(title: &str, artist: &str, secs: u32) -> ForeignTrack {
        ForeignTrack {
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration_secs: secs,
        }
    }

    #[test]
    fn foreign_rows_prefer_songs_and_use_the_primary_artist() {
        let r = row("BbY WOW", "KAROL G, Judeline, rusowsky", 226);
        assert_eq!(primary_artist(&r.artist), "KAROL G");
        assert_eq!(foreign_query(&r), "KAROL G BbY WOW");
        let c = [
            t(
                "v",
                "KAROL G - BbY WOW (Official Video)",
                "KarolGVEVO",
                240,
                true,
            ),
            t("s", "BbY WOW", "KAROL G, Judeline & rusowsky", 226, false),
        ];
        assert_eq!(best_foreign_match(&r, &c, true).unwrap().id, "s");
        // Songs-only pass ignores the video entirely.
        assert!(best_foreign_match(&r, &c[..1], false).is_none());
    }

    #[test]
    fn repeated_rows_are_dropped_before_searching() {
        let rows = vec![
            row("Mal Herido", "Gilberto Santa Rosa", 300),
            row("Fuego", "Isla Bonita", 280),
            row("Mal Herido", "Gilberto Santa Rosa", 300),
            row("mal herido ", "Gilberto Santa Rosa, Someone", 301),
            row("Mal Herido (Live)", "Gilberto Santa Rosa", 320),
        ];
        let (unique, repeats) = dedupe_foreign(rows);
        // Case, spacing and featured artists do not make a new song;
        // a "(Live)" aside is normalized away too (same search anyway).
        assert_eq!(repeats, 3);
        let titles: Vec<&str> = unique.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Mal Herido", "Fuego"]);
    }

    #[test]
    fn accents_never_block_a_match() {
        // Live misses from an Apple Music salsa playlist (2026-11): the
        // right song was the top hit, but accents split the words.
        let r = row("No Me Olvidarás", "Orquesta Concepto Latino", 344);
        let c = [t(
            "s",
            "No Me Olvidaras",
            "Orquesta Concepto Latino",
            345,
            false,
        )];
        assert_eq!(best_foreign_match(&r, &c, false).unwrap().id, "s");
        let r = row("Mi Sueño", "Willie Colón", 396);
        let c = [t("s", "Mi Sueno", "Willie Colon", 397, false)];
        assert_eq!(best_foreign_match(&r, &c, false).unwrap().id, "s");
        // The other way round too (the source lacks the accent).
        let r = row("El Adios", "Orquesta Zodiac", 224);
        let c = [t("s", "El Adiós", "Orquesta Zodiac", 232, false)];
        assert_eq!(best_foreign_match(&r, &c, false).unwrap().id, "s");
        // A different song is still a different song.
        let r = row("Mi Sueño", "Willie Colón", 396);
        let c = [t("x", "Un Sueño", "Fernando Burbano", 193, false)];
        assert!(best_foreign_match(&r, &c, false).is_none());
        assert_eq!(fold_accents("Ñandú Çà Øl straße"), "Nandu Ca Ol strasse");
        // Decomposed accents (Apple Music sends some titles this way).
        let r = row("Sedu\u{301}ceme", "Viti Ruiz", 363);
        let c = [t("s", "Sedúceme", "Viti Ruiz", 363, false)];
        assert_eq!(best_foreign_match(&r, &c, false).unwrap().id, "s");
        assert_eq!(fold_accents("Sedu\u{301}ceme"), "Seduceme");
    }

    #[test]
    fn accent_variants_count_as_repeats() {
        let (unique, repeats) = dedupe_foreign(vec![
            row("Siempre Seré", "Tito Rojas", 324),
            row("Siempre Sere", "Tito Rojas", 323),
        ]);
        assert_eq!((unique.len(), repeats), (1, 1));
    }

    #[test]
    fn a_video_stands_in_when_no_song_exists() {
        let r = row("Rare Session", "Some Band", 300);
        let c = [t(
            "v",
            "Some Band - Rare Session (Official Video)",
            "SomeBandVEVO",
            305,
            true,
        )];
        assert_eq!(best_foreign_match(&r, &c, true).unwrap().id, "v");
    }

    #[test]
    fn foreign_rows_reject_wrong_songs_and_versions() {
        let r = row("Yellow", "Coldplay", 266);
        let c = [
            t("a", "Yellow", "Some Cover Band", 266, false),
            t("b", "Yellow (Live in Buenos Aires)", "Coldplay", 280, false),
        ];
        assert!(best_foreign_match(&r, &c, true).is_none());
        // Title-only rows (plain text lists) accept an exact title.
        let bare = row("Bohemian Rhapsody", "", 0);
        let c = [t("q", "Bohemian Rhapsody", "Queen", 355, false)];
        assert_eq!(best_foreign_match(&bare, &c, false).unwrap().id, "q");
    }

    #[test]
    fn loose_separators_and_ampersands_still_match() {
        let song = |id, title: &str, artist: &str, secs| t(id, title, artist, secs, false);
        let cases = [
            (
                t("v1", "Iron Maiden-The Trooper", "Imperio Metal", 253, true),
                song("x1", "The Trooper", "Iron Maiden", 253),
                "Iron Maiden The Trooper",
            ),
            (
                t("v2", "System Of A Down- X", "2azaz", 119, true),
                song("x2", "X", "System Of A Down", 119),
                "System Of A Down X",
            ),
            (
                t("v3", "Black Sabbath ~ War Pigs", "Someone", 475, true),
                song("x3", "War Pigs", "Black Sabbath", 470),
                "Black Sabbath War Pigs",
            ),
            (
                t(
                    "v4",
                    "Metallica: One (Official Music Video)",
                    "Metallica",
                    465,
                    true,
                ),
                song("x4", "One", "Metallica", 448),
                "Metallica One",
            ),
            (
                t(
                    "v5",
                    "[HQ] Powerwolf - Christ and Combat [Lyrics]",
                    "Natchan",
                    220,
                    true,
                ),
                song("x5", "Christ & Combat", "Powerwolf", 220),
                "Powerwolf Christ and Combat",
            ),
            (
                t("v6", "Girls and Boys", "Blur", 254, true),
                song("x6", "Girls & Boys", "Blur", 260),
                "Blur Girls and Boys",
            ),
        ];
        for (video, want, query) in cases {
            assert_eq!(song_query(&video), query);
            let got = best_song_match(&video, std::slice::from_ref(&want));
            assert_eq!(
                got.map(|t| t.id.as_str()),
                Some(want.id.as_str()),
                "{}",
                video.title
            );
        }
        // A karaoke catalog tags the artist, not the title.
        let v = t("v9", "Paint It, Black", "The Rolling Stones", 227, true);
        let karaoke = [song(
            "x9",
            "Paint It Black",
            "The Rolling Stones (Karaoke)",
            227,
        )];
        assert!(best_song_match(&v, &karaoke).is_none());
        // A hyphenated word is not an artist split.
        let v = t("v7", "Spider-Man", "Danny Elfman", 200, true);
        assert_eq!(song_query(&v), "Danny Elfman Spider-Man");
        // A different song sharing a word is still rejected now that
        // durations are known ("I Can Talk" is not "Talk").
        let v = t("v8", "Two Door Cinema Club - I Can Talk", "A M", 178, true);
        let other = [song("x8", "Talk", "Two Door Cinema Club", 265)];
        assert!(best_song_match(&v, &other).is_none());
    }

    #[test]
    fn channel_artist_reading_covers_plain_titles() {
        let v = t("v1", "Bohemian Rhapsody", "Queen Official", 355, true);
        let c = [t("x1", "Bohemian Rhapsody", "Queen", 354, false)];
        assert_eq!(best_song_match(&v, &c).unwrap().id, "x1");
    }

    /// Live: the official music video resolves to a song. Needs network:
    /// `cargo test -p yutu-core --features innertube -- --ignored live_video`.
    #[cfg(feature = "innertube")]
    #[test]
    #[ignore]
    fn live_video_resolves_to_its_song() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let video = t(
            "dQw4w9WgXcQ",
            "Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster)",
            "Rick Astley",
            213,
            true,
        );
        let song = t("keep", "Some Song", "Someone", 200, false);
        let (out, swapped) = rt.block_on(resolve_song_versions(
            vec![song.clone(), video],
            &crate::raw_search::SearchAuth::anonymous(),
            |_, _| {},
        ));
        eprintln!("{out:#?}");
        assert_eq!(swapped, 1);
        assert_eq!(out[0], song);
        assert!(!out[1].is_video);
        assert_ne!(out[1].id, "dQw4w9WgXcQ");
        assert!(out[1]
            .title
            .to_lowercase()
            .contains("never gonna give you up"));
    }
}
