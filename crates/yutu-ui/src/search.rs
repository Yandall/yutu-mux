//! yutu-ui search: split from lib.rs (no behavior change).

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

/// One ranked search hit across result types. The unified search
/// list mixes songs, albums, artists and playlists, so each row carries
/// its own value (no per-type sections anymore).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchHit<'a> {
    Song(&'a Track),
    Album(&'a Album),
    Artist(&'a Artist),
    Playlist(&'a PlaylistHit),
}

/// One search round trip: the server-ordered All response plus the
/// filtered per-type depth behind the Songs/Albums/Artists/Playlists
/// tabs. The cache stores the bundle; keystroke narrowing filters both
/// halves (order-preserving, so server rank survives typing). Boxed at
/// the `SearchCompleted` edge: nine vecs would bloat the `Message` enum.
#[derive(Debug, Clone, Default)]
pub struct SearchBundle {
    pub all: SearchAll,
    pub filtered: SearchResults,
}

/// True when the unfiltered response holds nothing renderable.
pub(crate) fn all_empty(all: &SearchAll) -> bool {
    all.tracks.is_empty()
        && all.artists.is_empty()
        && all.albums.is_empty()
        && all.playlists.is_empty()
}

/// Lowercase with edge punctuation trimmed, so "Chop Suey!" matches
/// a "Chop Suey" top hit when resolving the hero play target.
/// Diacritics fold too ("Héroes" matches "heroes"), like the server.
pub(crate) fn norm_title(s: &str) -> String {
    fold_diacritics(&s.to_lowercase())
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

/// Accent-insensitive fold for search matching: the server ranks
/// "Héroes del Silencio" first for "heroes", so the client narrow
/// must too (a raw `contains` drops every accented hit). Latin
/// diacritics map to their base letter; anything else passes through.
pub(crate) fn fold_diacritics(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' | 'ȁ' | 'ȃ' => {
                out.push('a')
            }
            'æ' => out.push_str("ae"),
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => out.push('c'),
            'ď' | 'đ' | 'ð' => out.push('d'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'ȅ' | 'ȇ' => {
                out.push('e')
            }
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
            'ĥ' | 'ħ' => out.push('h'),
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' | 'ǐ' | 'ȉ' | 'ȋ' => {
                out.push('i')
            }
            'ĵ' => out.push('j'),
            'ķ' => out.push('k'),
            'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => out.push('l'),
            'ñ' | 'ń' | 'ņ' | 'ň' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'ǒ' | 'ȍ' | 'ȏ' => {
                out.push('o')
            }
            'œ' => out.push_str("oe"),
            'ŕ' | 'ŗ' | 'ř' => out.push('r'),
            'ś' | 'ŝ' | 'ş' | 'š' => out.push('s'),
            'ß' => out.push_str("ss"),
            'ţ' | 'ť' | 'ŧ' => out.push('t'),
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ǔ' | 'ȕ' | 'ȗ' => {
                out.push('u')
            }
            'ŵ' => out.push('w'),
            'ý' | 'ÿ' | 'ŷ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            _ => out.push(c),
        }
    }
    out
}

/// Hero for the unified search list: the server's top hit resolved by
/// id into the shelves (the card tap target is directly actionable),
/// falling back to a title match and then the first row. The shelves
/// below supply the play button.
pub(crate) fn top_hero<'a>(all: &'a SearchAll) -> Option<SearchHit<'a>> {
    let by_id = all.top.as_ref().and_then(|top| match top.kind {
        TopKind::Song | TopKind::Video => all
            .tracks
            .iter()
            .find(|t| t.id == top.id)
            .map(SearchHit::Song),
        TopKind::Artist => all
            .artists
            .iter()
            .find(|a| a.id == top.id)
            .map(SearchHit::Artist),
        TopKind::Album => all
            .albums
            .iter()
            .find(|a| a.id == top.id)
            .map(SearchHit::Album),
        TopKind::Playlist => all
            .playlists
            .iter()
            .find(|p| p.id == top.id)
            .map(SearchHit::Playlist),
        TopKind::Other => None,
    });
    if by_id.is_some() {
        return by_id;
    }
    let matched = all.top.as_ref().and_then(|top| {
        let title = norm_title(&top.title);
        let artist = norm_title(&top.artist);
        match top.kind {
            TopKind::Song | TopKind::Video => all
                .tracks
                .iter()
                .find(|t| {
                    norm_title(&t.title) == title
                        && (artist.is_empty() || norm_title(&t.artist) == artist)
                })
                .map(SearchHit::Song),
            TopKind::Artist => all
                .artists
                .iter()
                .find(|a| norm_title(&a.name) == title || norm_title(&a.name) == artist)
                .map(SearchHit::Artist),
            TopKind::Album => all
                .albums
                .iter()
                .find(|a| norm_title(&a.title) == title)
                .map(SearchHit::Album),
            TopKind::Playlist => all
                .playlists
                .iter()
                .find(|p| norm_title(&p.title) == title)
                .map(SearchHit::Playlist),
            TopKind::Other => None,
        }
    });
    matched.or_else(|| {
        all.tracks
            .first()
            .map(SearchHit::Song)
            .or_else(|| all.artists.first().map(SearchHit::Artist))
            .or_else(|| all.albums.first().map(SearchHit::Album))
            .or_else(|| all.playlists.first().map(SearchHit::Playlist))
    })
}

/// Row order for the unified search list: the exact web mixed order
/// (`mixed` refs into the typed vecs), hero excluded and capped for
/// page speed. Narrowed-away refs are skipped. No client re-sort, so
/// official hits stay where the server put them.
pub(crate) fn order_all_hits<'a>(all: &'a SearchAll) -> Vec<SearchHit<'a>> {
    let hero_id: Option<&str> = match top_hero(all) {
        Some(SearchHit::Song(t)) => Some(t.id.as_str()),
        Some(SearchHit::Album(a)) => Some(a.id.as_str()),
        Some(SearchHit::Artist(a)) => Some(a.id.as_str()),
        Some(SearchHit::Playlist(p)) => Some(p.id.as_str()),
        None => None,
    };
    let mut rows: Vec<SearchHit<'a>> = Vec::new();
    for m in &all.mixed {
        let hit = match m.kind {
            0 => all
                .tracks
                .iter()
                .find(|t| t.id == m.id)
                .map(SearchHit::Song),
            1 => all
                .artists
                .iter()
                .find(|a| a.id == m.id)
                .map(SearchHit::Artist),
            2 => all
                .albums
                .iter()
                .find(|a| a.id == m.id)
                .map(SearchHit::Album),
            3 => all
                .playlists
                .iter()
                .find(|p| p.id == m.id)
                .map(SearchHit::Playlist),
            _ => None,
        };
        if let Some(hit) = hit {
            let id = match hit {
                SearchHit::Song(t) => t.id.as_str(),
                SearchHit::Album(a) => a.id.as_str(),
                SearchHit::Artist(a) => a.id.as_str(),
                SearchHit::Playlist(p) => p.id.as_str(),
            };
            if hero_id != Some(id) {
                rows.push(hit);
            }
        }
        if rows.len() >= MIXED_SEARCH_CAP {
            break;
        }
    }
    rows
}

/// True when every result list is empty (drives the search page
/// loading/empty states, never per-type placeholders).
pub(crate) fn results_empty(results: &SearchResults) -> bool {
    results.tracks.is_empty()
        && results.albums.is_empty()
        && results.artists.is_empty()
        && results.playlists.is_empty()
}

/// Accent-insensitive haystack match for a narrowed query `q`
/// (already folded): "Héroes" matches a "heroes" query.
pub(crate) fn hit_matches(text: &str, q: &str) -> bool {
    fold_diacritics(&text.to_lowercase()).contains(q)
}

/// Order-preserving substring narrow of an unfiltered response (the
/// keystroke path for the All tab): same predicates as
/// `filter_results`, server order untouched. The top hit survives only
/// while it still matches the narrower query.
pub(crate) fn filter_all(base: &SearchAll, query: &str) -> SearchAll {
    let q = fold_diacritics(&query.to_lowercase());
    if q.trim().is_empty() {
        return SearchAll::default();
    }
    SearchAll {
        top: base
            .top
            .clone()
            .filter(|t| hit_matches(&t.title, &q) || hit_matches(&t.artist, &q)),
        mixed: base.mixed.clone(),
        tracks: base
            .tracks
            .iter()
            .filter(|t| hit_matches(&t.title, &q) || hit_matches(&t.artist, &q))
            .cloned()
            .collect(),
        artists: base
            .artists
            .iter()
            .filter(|a| hit_matches(&a.name, &q))
            .cloned()
            .collect(),
        albums: base
            .albums
            .iter()
            .filter(|a| hit_matches(&a.title, &q) || hit_matches(&a.artist, &q))
            .cloned()
            .collect(),
        playlists: base
            .playlists
            .iter()
            .filter(|p| hit_matches(&p.title, &q) || hit_matches(&p.author, &q))
            .cloned()
            .collect(),
    }
}

/// Case-insensitive substring filter over a full search result set.
pub(crate) fn filter_results(base: &SearchResults, query: &str) -> SearchResults {
    let q = fold_diacritics(&query.to_lowercase());
    if q.trim().is_empty() {
        return SearchResults::default();
    }
    SearchResults {
        tracks: base
            .tracks
            .iter()
            .filter(|t| hit_matches(&t.title, &q) || hit_matches(&t.artist, &q))
            .cloned()
            .collect(),
        albums: base
            .albums
            .iter()
            .filter(|a| hit_matches(&a.title, &q) || hit_matches(&a.artist, &q))
            .cloned()
            .collect(),
        artists: base
            .artists
            .iter()
            .filter(|a| hit_matches(&a.name, &q))
            .cloned()
            .collect(),
        playlists: base
            .playlists
            .iter()
            .filter(|p| hit_matches(&p.title, &q) || hit_matches(&p.author, &q))
            .cloned()
            .collect(),
    }
}

/// Offline builds: only the yt-dlp path exists; without the binary installed
/// there is nothing else to try.
#[cfg(not(feature = "innertube"))]
pub(crate) async fn resolve_via_raw(video_id: &str) -> Result<Vec<u8>, String> {
    Err(format!(
        "yt-dlp not found and this is an offline build ({video_id}). Install yt-dlp or rebuild with `--features innertube`."
    ))
}

/// yt-dlp `--get-url` only (no fetch): the fast path behind instant
/// playback. Failures name the track; the caller decides bytes-vs-stream.
/// The sidecar runs on the blocking pool (it takes seconds, and on an
/// async worker it would also starve the caller's timeout). The URL is
/// then warmed: some clients' fresh URLs answer 403 for a few seconds,
/// and mpv opening one at once fails the load.
pub(crate) async fn resolve_stream_url(video_id: &str) -> Result<String, String> {
    let bin = yutu_core::find_yt_dlp()
        .ok_or_else(|| "yt-dlp not installed: install it to play music.".to_string())?;
    let url = ytdlp_resolve(&bin, video_id).await?;
    yutu_player::warm_stream_url(&url, std::time::Duration::from_secs(STREAM_WARM_SECS))
        .await
        .map_err(|e| e.to_string())?;
    Ok(url)
}

/// `yutu_core::resolve_audio_url` on the blocking pool: the sidecar
/// takes seconds (two runs when the pinned client fails), which on an
/// async worker would stall it and starve the caller's timeout.
pub(crate) async fn ytdlp_resolve(bin: &str, video_id: &str) -> Result<String, String> {
    let (bin, id) = (bin.to_string(), video_id.to_string());
    tokio::task::spawn_blocking(move || yutu_core::resolve_audio_url(&bin, &id))
        .await
        .map_err(|e| format!("stream resolver stopped: {e}"))?
        .map_err(|e| e.to_string())
}

/// Best-effort warm-up on the bytes path: a still-refused URL falls
/// through to the fetch, whose 403 handling owns the fallback chain.
pub(crate) async fn warm_for_fetch(url: &str) {
    let _ =
        yutu_player::warm_stream_url(url, std::time::Duration::from_secs(STREAM_WARM_SECS)).await;
}

/// Real search over the live Innertube API: the web-style unfiltered
/// search (server relevance order for the All tab) plus one raw
/// filtered search per type (depth for the filter tabs, same `params`
/// the web client sends). Five round trips, each failing open; only a
/// total failure errors. Uses the signed-in session when one exists
/// (the server then ranks like the logged-in web client, from the
/// account's own activity), else anonymous.
#[cfg(feature = "innertube")]
pub(crate) async fn live_search(query: &str) -> Result<Box<SearchBundle>, String> {
    use yutu_core::{raw_search_all, raw_search_filtered, SearchFilterKind};
    let auth = AppState::session_search_auth();
    let (all, songs, albums, artists, playlists) = tokio::join!(
        raw_search_all(query, &auth),
        raw_search_filtered(query, &auth, SearchFilterKind::Songs),
        raw_search_filtered(query, &auth, SearchFilterKind::Albums),
        raw_search_filtered(query, &auth, SearchFilterKind::Artists),
        raw_search_filtered(query, &auth, SearchFilterKind::Playlists),
    );
    if let (Err(e), Err(_), Err(_), Err(_), Err(_)) = (&all, &songs, &albums, &artists, &playlists)
    {
        return Err(e.clone());
    }
    let all = all.unwrap_or_default();
    let filtered = SearchResults {
        tracks: songs.map(|s| s.tracks).unwrap_or_default(),
        albums: albums.map(|s| s.albums).unwrap_or_default(),
        artists: artists.map(|s| s.artists).unwrap_or_default(),
        playlists: playlists.map(|s| s.playlists).unwrap_or_default(),
    };
    // Either half backs the other: a failed raw response still leaves
    // server-ordered filtered shelves for All, and failed filtered
    // queries fall back to the raw shelves for the tabs.
    let all = if all_empty(&all) {
        all_from_filtered(&filtered)
    } else {
        all
    };
    let filtered = SearchResults {
        tracks: if filtered.tracks.is_empty() {
            all.tracks.clone()
        } else {
            filtered.tracks
        },
        albums: if filtered.albums.is_empty() {
            all.albums.clone()
        } else {
            filtered.albums
        },
        artists: if filtered.artists.is_empty() {
            all.artists.clone()
        } else {
            filtered.artists
        },
        playlists: if filtered.playlists.is_empty() {
            all.playlists.clone()
        } else {
            filtered.playlists
        },
    };
    Ok(Box::new(SearchBundle { all, filtered }))
}

/// All response derived from filtered shelves (raw failure fallback):
/// per-type server order concatenated, no top hit.
#[cfg(feature = "innertube")]
pub(crate) fn all_from_filtered(filtered: &SearchResults) -> SearchAll {
    use yutu_core::MixedRef;
    let mut mixed = Vec::new();
    for t in &filtered.tracks {
        mixed.push(MixedRef {
            kind: 0,
            id: t.id.clone(),
        });
    }
    for a in &filtered.artists {
        mixed.push(MixedRef {
            kind: 1,
            id: a.id.clone(),
        });
    }
    for a in &filtered.albums {
        mixed.push(MixedRef {
            kind: 2,
            id: a.id.clone(),
        });
    }
    for p in &filtered.playlists {
        mixed.push(MixedRef {
            kind: 3,
            id: p.id.clone(),
        });
    }
    SearchAll {
        top: None,
        tracks: filtered.tracks.clone(),
        artists: filtered.artists.clone(),
        albums: filtered.albums.clone(),
        playlists: filtered.playlists.clone(),
        mixed,
    }
}

/// Fetch one search-result playlist (community or featured) into playable
/// tracks: the click path behind playlist heroes and rows.
#[cfg(feature = "innertube")]
pub(crate) async fn live_playlist_tracks(id: &str) -> Result<Vec<Track>, String> {
    use yutu_core::InnertubeBackend;
    let backend = InnertubeBackend::connect_anonymous().await?;
    backend.playlist_tracks(id).await
}

/// Radio sequence for a seed: the seed first, same-artist tracks next,
/// everything else after.
#[cfg(feature = "innertube")]
pub(crate) async fn live_radio(seed: &str, artist: &str) -> Result<Vec<Track>, String> {
    use yutu_core::InnertubeBackend;
    let backend = InnertubeBackend::connect_anonymous().await?;
    let found = backend.search_tracks(artist).await?;
    let mut seq: Vec<Track> = Vec::with_capacity(found.len());
    if let Some(t) = found.iter().find(|t| t.id == seed).cloned() {
        seq.push(t);
    }
    for t in &found {
        if t.id != seed && t.artist == artist && !seq.iter().any(|x| x.id == t.id) {
            seq.push(t.clone());
        }
    }
    for t in &found {
        if t.id != seed && !seq.iter().any(|x| x.id == t.id) {
            seq.push(t.clone());
        }
    }
    Ok(seq)
}

/// Hydrate one bare id (liked/history rows from installs that only
/// stored ids, or catalog rows missing art) into a full Track: a
/// watch-playlist probe whose panel contains the seed, keeping only the
/// exact-id match (neighbours stay out so a small lazy batch never
/// explodes into 100+ rows of DB writes + art).
#[cfg(feature = "innertube")]
pub(crate) async fn hydrate_one(id: String) -> Vec<Track> {
    use yutu_core::InnertubeBackend;
    let Ok(backend) = InnertubeBackend::connect_anonymous().await else {
        return Vec::new();
    };
    match backend.recommendations_for(&id).await {
        Ok(tracks) => tracks.into_iter().filter(|t| t.id == id).collect(),
        Err(e) => {
            yutu_core::app_log::warn("yutu-hydrate", format_args!("{id}: {e}"));
            Vec::new()
        }
    }
}

/// Offline builds cannot hydrate (no Innertube): the SQLite track cache
/// is the only source, so callers just get nothing new.
#[cfg(not(feature = "innertube"))]
pub(crate) async fn hydrate_one(_id: String) -> Vec<Track> {
    Vec::new()
}

/// Resolve a video ID to bytes. yt-dlp goes first (maintained decipher, no
/// feature gate, license-clean); then the raw WEB_REMIX probes with sidecar
/// mint + retry on PO-gating. A bare-URL 403 retries once with a freshly
/// resolved URL (Google edges transiently reject single URL instances),
/// then falls back to a direct sidecar fetch (memory-only) (`-o -`, whose own HTTP
/// stack handles the headers/retries a bare GET lacks).
pub(crate) async fn resolve_and_fetch(video_id: &str) -> Result<Vec<u8>, String> {
    if let Some(bin) = yutu_core::find_yt_dlp() {
        let resolved = ytdlp_resolve(&bin, video_id).await;
        if let Ok(url) = &resolved {
            warm_for_fetch(url).await;
        }
        match resolved {
            Ok(url) => match yutu_player::fetch_bytes(&url).await {
                Ok(bytes) => return Ok(bytes),
                Err(first) => {
                    let msg = first.to_string();
                    if msg.contains("403") {
                        yutu_core::app_log::warn(
                            "yutu-stream",
                            format_args!("chunked fetch 403 ({msg}); re-resolving once"),
                        );
                        if let Ok(fresh) = ytdlp_resolve(&bin, video_id).await {
                            warm_for_fetch(&fresh).await;
                            if let Ok(bytes) = yutu_player::fetch_bytes(&fresh).await {
                                return Ok(bytes);
                            }
                        }
                        yutu_core::app_log::warn(
                            "yutu-stream",
                            "fresh URL also failed; yt-dlp direct fetch (memory-only)",
                        );
                        let (b, id) = (bin.clone(), video_id.to_string());
                        let direct = tokio::task::spawn_blocking(move || {
                            yutu_core::download_audio_bytes(&b, &id).map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("direct fetch stopped: {e}")));
                        match direct {
                            Ok(bytes) => return Ok(bytes),
                            Err(e) => {
                                yutu_core::app_log::warn(
                                    "yutu-stream",
                                    format_args!("yt-dlp fetch failed ({e}); trying raw probes"),
                                );
                            }
                        }
                    } else {
                        yutu_core::app_log::warn(
                            "yutu-stream",
                            format_args!("yt-dlp path failed ({msg}); trying raw probes"),
                        );
                    }
                }
            },
            Err(e) => {
                yutu_core::app_log::warn(
                    "yutu-stream",
                    format_args!("yt-dlp path failed ({e}); trying raw probes"),
                );
            }
        }
    }
    resolve_via_raw(video_id).await
}

/// Raw-probe path (needs the `innertube` feature): anonymous connect,
/// fallback chain, sidecar mint + `poToken` retry on PO-gating.
#[cfg(feature = "innertube")]
pub(crate) async fn resolve_via_raw(video_id: &str) -> Result<Vec<u8>, String> {
    use yutu_core::{InnertubeBackend, StreamError, StreamRequest};
    let backend = InnertubeBackend::connect_anonymous().await?;
    let mut req = StreamRequest::new(video_id);
    match backend.resolve_stream(&mut req).await {
        Ok(url) => yutu_player::fetch_bytes(&url)
            .await
            .map_err(|e| e.to_string()),
        Err(StreamError::PoTokenRequired { .. }) => {
            let Some(bin) = yutu_core::find_botguard_bin() else {
                return Err("PO token required and rustypipe-botguard is not installed.".into());
            };
            let mut retry = StreamRequest::new(video_id);
            let resolved = backend
                .resolve_stream_with_sidecar(&mut retry, &bin)
                .await
                .map_err(|e| format!("{e}: {}", e.hint()))?;
            match yutu_player::fetch_bytes(&resolved.url).await {
                Ok(bytes) => Ok(bytes),
                Err(first) => match resolved.pot_token {
                    Some(pot) => {
                        let sep = if resolved.url.contains('?') { '&' } else { '?' };
                        let url = format!("{}{}pot={}", resolved.url, sep, pot);
                        yutu_player::fetch_bytes(&url)
                            .await
                            .map_err(|e| e.to_string())
                    }
                    None => Err(first.to_string()),
                },
            }
        }
        Err(e) => Err(format!("{e}: {}", e.hint())),
    }
}
