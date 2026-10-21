//! Raw Innertube search: the web-style "All" in one round trip.
//!
//! `ytmapi-rs` 0.3 only speaks the old shelf layout and fails whole
//! queries on a single unparseable item; the live API now returns a
//! flat server-ranked item list, so this module POSTs
//! `youtubei/v1/search` directly (same endpoint + `WEB_REMIX` client
//! the web uses) and parses tolerantly: unknown kinds and malformed
//! items are skipped, never fatal. Order is the server's relevance
//! order (personalized when authed): render `mixed` as-is, no client
//! re-sort. Flat items carry no durations, so rows hide zero ones.

use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::innertube::{parse_duration_secs, USER_AGENT, WEB_REMIX_VERSION};
use crate::models::{Album, Artist, MixedRef, PlaylistHit, SearchAll, TopHit, TopKind, Track};

const SEARCH_URL: &str =
    "https://music.youtube.com/youtubei/v1/search?alt=json&prettyPrint=false&key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
pub(crate) const ORIGIN: &str = "https://music.youtube.com";

/// Auth headers for raw search. Anonymous needs none; OAuth sends a
/// bearer token and cookie sessions send the cookie + `SAPISIDHASH`
/// (same scheme browsers use). A 401 falls back to anonymous inside
/// [`raw_search_all`] (stale token must never blank the page).
#[derive(Debug, Clone, Default)]
pub struct SearchAuth {
    cookie: Option<String>,
    bearer: Option<String>,
}

impl SearchAuth {
    /// No login: generic server ranking.
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// OAuth session: the server ranks from the account's activity.
    pub fn bearer(token: String) -> Self {
        Self {
            cookie: None,
            bearer: (!token.is_empty()).then_some(token),
        }
    }

    /// Cookie session (`Cookie` header value): same ranking as the
    /// logged-in web client.
    pub fn cookie(header: String) -> Self {
        Self {
            cookie: (!header.is_empty()).then_some(header),
            bearer: None,
        }
    }

    /// OAuth token out of a persisted login (the token file stores the
    /// upstream JSON shape, so the access token reads back out of it).
    /// `None` when the file holds no usable token.
    pub fn from_oauth(persist: &crate::auth::OAuthPersist) -> Option<Self> {
        let value = serde_json::to_value(persist.token()).ok()?;
        let token = value.get("access_token")?.as_str()?;
        (!token.is_empty()).then(|| Self::bearer(token.to_string()))
    }

    /// Cookie session out of a validated login (the header shape always
    /// carries `SAPISID`, enforced at import).
    pub fn from_cookie(auth: &crate::auth::CookieAuth) -> Self {
        Self::cookie(auth.header_value().to_string())
    }

    fn authed(&self) -> bool {
        self.cookie.is_some() || self.bearer.is_some()
    }

    fn sapisid(&self) -> Option<&str> {
        let header = self.cookie.as_deref()?;
        header.split(';').find_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            (name.trim() == "SAPISID")
                .then(|| value.trim())
                .filter(|v| !v.is_empty())
        })
    }
}

/// `SAPISIDHASH {timestamp}_{sha1(timestamp + " " + SAPISID + " " +
/// origin)}`: the browser auth scheme for cookie Innertube calls.
fn sapisid_hash(timestamp_secs: u64, sapisid: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{timestamp_secs} {sapisid} {ORIGIN}").into_bytes());
    format!("{timestamp_secs}_{:x}", hasher.finalize())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Per-type filter for raw search (the filter-tab requests): same
/// `params` the web client sends for each tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFilterKind {
    Songs,
    Albums,
    Artists,
    Playlists,
}

impl SearchFilterKind {
    /// Filter `params` (mirrors the `ytmapi-rs` filtered queries, same
    /// `ExactMatch` spelling the app has always used).
    fn params(self) -> &'static str {
        match self {
            SearchFilterKind::Songs => "EgWKAQIIAWoMEA4QChADEAQQCRAF",
            SearchFilterKind::Albums => "EgWKAQIYAWoMEA4QChADEAQQCRAF",
            SearchFilterKind::Artists => "EgWKAQIgAWoMEA4QChADEAQQCRAF",
            SearchFilterKind::Playlists => "EgeKAQQoAEABagwQDhAKEAMQBBAJEAU%3D",
        }
    }
}

/// One filtered search round trip + the same tolerant parse (filtered
/// responses use the old shelf layout, handled by the same path).
/// Same 401/403 anonymous retry as [`raw_search_all`].
pub async fn raw_search_filtered(
    query: &str,
    auth: &SearchAuth,
    kind: SearchFilterKind,
) -> Result<SearchAll, String> {
    match post_search(query, auth, Some(kind.params())).await {
        Ok(text) => parse_search_all(&text),
        Err(SearchFail::Auth) if auth.authed() => {
            crate::app_log::warn(
                "yutu-search",
                "authed filtered search rejected: retrying anonymous",
            );
            let text = post_search(query, &SearchAuth::anonymous(), Some(kind.params()))
                .await
                .map_err(|e| e.message())?;
            parse_search_all(&text)
        }
        Err(e) => Err(e.message()),
    }
}

/// Raw search failure: auth rejections (retryable anonymously) vs
/// everything else (network, server, parse-adjacent).
enum SearchFail {
    Auth,
    Other(String),
}

impl SearchFail {
    fn message(self) -> String {
        match self {
            SearchFail::Auth => "search: stale auth".into(),
            SearchFail::Other(e) => e,
        }
    }
}

/// One unfiltered search round trip + tolerant parse. Stale auth
/// (401/403) retries once anonymously instead of failing the page;
/// any other failure passes through (no blind double traffic).
pub async fn raw_search_all(query: &str, auth: &SearchAuth) -> Result<SearchAll, String> {
    match post_search(query, auth, None).await {
        Ok(text) => parse_search_all(&text),
        Err(SearchFail::Auth) if auth.authed() => {
            crate::app_log::warn("yutu-search", "authed search rejected: retrying anonymous");
            let text = post_search(query, &SearchAuth::anonymous(), None)
                .await
                .map_err(|e| e.message())?;
            parse_search_all(&text)
        }
        Err(e) => Err(e.message()),
    }
}

async fn post_search(
    query: &str,
    auth: &SearchAuth,
    params: Option<&str>,
) -> Result<String, SearchFail> {
    let mut body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": WEB_REMIX_VERSION,
                "user": {},
            },
        },
        "query": query,
    });
    if let Some(params) = params {
        body["params"] = serde_json::Value::String(params.to_string());
    }
    let mut request = crate::shared_client()
        .post(SEARCH_URL)
        .header("User-Agent", USER_AGENT)
        .header("X-Origin", ORIGIN)
        .header("Origin", ORIGIN)
        .header("Content-Type", "application/json")
        .json(&body);
    if let Some(cookie) = &auth.cookie {
        request = request.header("Cookie", cookie.clone());
        if let Some(sapisid) = auth.sapisid() {
            let hash = sapisid_hash(now_secs(), sapisid);
            request = request.header("Authorization", format!("SAPISIDHASH {hash}"));
        }
    }
    if let Some(bearer) = &auth.bearer {
        request = request.header("Authorization", format!("Bearer {bearer}"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| SearchFail::Other(format!("search request: {e}")))?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(SearchFail::Auth);
    }
    if !status.is_success() {
        return Err(SearchFail::Other(format!("search: HTTP {status}")));
    }
    response
        .text()
        .await
        .map_err(|e| SearchFail::Other(format!("search body: {e}")))
}

/// Parse a raw unfiltered response into server order. Unknown sections
/// and items are skipped; only a missing section list errors. Video
/// rows duplicating a song row are dropped last (song versions win
/// for streams and art); survivors keep exact server order.
pub fn parse_search_all(text: &str) -> Result<SearchAll, String> {
    let root: Value = serde_json::from_str(text).map_err(|e| format!("search parse: {e}"))?;
    let sections = section_contents(&root).ok_or("search parse: no section list")?;
    let mut out = SearchAll::default();
    let mut rows: Vec<TrackRow> = Vec::new();
    for section in sections {
        if let Some(card) = section.get("musicCardShelfRenderer") {
            if let Some(top) = parse_top_card(card) {
                out.top = Some(top);
            }
        } else if let Some(item) = section.get("itemSectionRenderer") {
            for content in item
                .get("contents")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                push_content(&mut out, &mut rows, content);
            }
        } else if let Some(shelf) = section.get("musicShelfRenderer") {
            push_shelf(&mut out, &mut rows, shelf);
        }
        // Carousels, podcasts, episodes, profiles: skipped by design.
    }
    prefer_song_versions(&mut out, &rows);
    Ok(out)
}

/// First tab carrying a section list (the response nests it under
/// `tabbedSearchResultsRenderer`).
fn section_contents(root: &Value) -> Option<&Vec<Value>> {
    root.pointer("/contents/tabbedSearchResultsRenderer/tabs")?
        .as_array()?
        .iter()
        .find_map(|t| {
            t.pointer("/tabRenderer/content/sectionListRenderer/contents")?
                .as_array()
        })
}

/// One content entry: flat item or old-style shelf (both tolerated so
/// layout flip-flops never blank the page).
fn push_content(out: &mut SearchAll, rows: &mut Vec<TrackRow>, content: &Value) {
    if let Some(item) = content.get("musicResponsiveListItemRenderer") {
        push_item(out, rows, item, None);
    } else if let Some(shelf) = content.get("musicShelfRenderer") {
        push_shelf(out, rows, shelf);
    }
}

/// Old-style shelf fallback: items take the shelf's kind (the shelf
/// title names it in the response locale).
fn push_shelf(out: &mut SearchAll, rows: &mut Vec<TrackRow>, shelf: &Value) {
    let title: String = shelf
        .get("title")
        .map(|t| run_texts(t).concat())
        .unwrap_or_default()
        .to_lowercase();
    let force = if title.contains("song") || title.contains("canci") {
        Some(0u8)
    } else if title.contains("video") || title.contains("vídeo") {
        Some(1u8)
    } else if title.contains("artist") {
        Some(2u8)
    } else if title.contains("album")
        || title.contains("álbum")
        || title.contains("single")
        || title.contains("sencillo")
    {
        Some(3u8)
    } else if title.contains("playlist") || title.contains("lista") {
        Some(4u8)
    } else {
        None
    };
    let Some(force) = force else { return };
    for item in shelf
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(renderer) = item.get("musicResponsiveListItemRenderer") {
            push_item(out, rows, renderer, Some(force));
        }
    }
}

/// Item kind tokens: the first subtitle segment names the type in the
/// response locale (the request sets no `hl`, so English; other big
/// locales covered for signed-in sessions that localize anyway).
fn token_kind(token: &str) -> Option<u8> {
    let t = token.to_lowercase();
    match t.as_str() {
        "song" | "canción" | "cancion" | "chanson" | "lied" | "canzone" | "canção" | "cancao" => {
            Some(0)
        }
        "video" | "vídeo" | "vidéo" => Some(1),
        "artist" | "artista" | "artiste" | "künstler" | "kunstler" => Some(2),
        "album" | "álbum" | "single" | "sencillo" | "singolo" => Some(3),
        "playlist" | "lista" | "liste" => Some(4),
        _ => None,
    }
}

/// Split subtitle runs on the "•" separators: `[type, artists..,
/// extra]`. Multi-artist segments (`A`, `&`, `B`) rejoin with spaces,
/// so "A & B" survives as one artist string.
fn segments(runs: &[String]) -> Vec<String> {
    let mut segs: Vec<Vec<&str>> = vec![Vec::new()];
    for r in runs {
        let t = r.trim();
        if t == "•" {
            segs.push(Vec::new());
        } else if !t.is_empty() {
            segs.last_mut().expect("segments nonempty").push(t);
        }
    }
    segs.into_iter().map(|s| s.join(" ")).collect()
}

/// One parsed song/video row plus its position in `mixed` (the song
/// preference needs both to drop dup videos cleanly).
struct TrackRow {
    song: bool,
    track: Track,
    mixed_pos: usize,
}

/// Video-ness markers stripped from titles when matching songs to
/// videos ("Song (Official Video)" is the same track as "Song";
/// anything else in parens, like remasters, never merges). Lyric
/// videos and visualizers reuse the studio recording, so they merge;
/// live videos are different performances and must NOT (durations
/// guard them when known, so they stay out of this list).
const VIDEO_MARKERS: [&str; 13] = [
    "official music video",
    "official lyric video",
    "official video",
    "official visualizer",
    "music video",
    "lyrics video",
    "lyric video",
    "official audio",
    "visualizer",
    "video",
    "audio",
    "lyrics",
    "lyric",
];

/// Collapse ASCII whitespace runs to single spaces.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Title key for song/video matching: lowercase, video markers
/// stripped (trailing ` - X`, ` (X)`, `[X]`, or bare ` X`), spaces
/// collapsed. Conservative: unknown parentheticals stay.
fn norm_title(title: &str) -> String {
    let mut s = title.to_lowercase();
    loop {
        let before = s.len();
        s = s.trim_end().to_string();
        for m in VIDEO_MARKERS {
            for pat in [
                format!(" - {m}"),
                format!(" ({m})"),
                format!(" [{m}]"),
                format!(" {m}"),
            ] {
                if s.len() > pat.len() {
                    if let Some(stripped) = s.strip_suffix(&pat) {
                        s = stripped.to_string();
                    }
                }
            }
        }
        if s.len() == before {
            break;
        }
    }
    collapse_ws(&s)
}

/// Artist key: lowercase, spaceless (topic/VEVO channels drop their
/// suffixes and spacing: "X - Topic" is "X").
fn norm_artist(artist: &str) -> String {
    let s = artist.to_lowercase();
    let s = s
        .strip_suffix(" - topic")
        .or_else(|| s.strip_suffix(" - vevo"))
        .or_else(|| s.strip_suffix("vevo"))
        .unwrap_or(&s);
    s.split_whitespace().collect::<Vec<_>>().join("")
}

/// True when `video` is the video version of `song`: same normalized
/// title + artist, durations within 3s when both are known (flat All
/// items carry none, so title + artist decide there).
fn same_track(song: &Track, video: &Track) -> bool {
    norm_title(&song.title) == norm_title(&video.title)
        && norm_artist(&song.artist) == norm_artist(&video.artist)
        && (song.duration_secs == 0
            || video.duration_secs == 0
            || song.duration_secs.abs_diff(video.duration_secs) <= 3)
}

/// Song-version preference over a parsed response: video rows
/// duplicating a song row are dropped (streams + art resolve to the
/// song instead); survivors keep exact server order (nothing is
/// re-sorted, only hidden twins). Videos without a song twin,
/// libraries, playlists, and radio are untouched. The top hit is
/// deliberately NOT retargeted: it carries no duration, so a long
/// video could bind to a short song (wrong stream); the server's
/// top pick plays as ranked.
fn prefer_song_versions(out: &mut SearchAll, rows: &[TrackRow]) {
    let songs: Vec<&Track> = rows.iter().filter(|r| r.song).map(|r| &r.track).collect();
    let dead: std::collections::HashSet<usize> = rows
        .iter()
        .filter(|r| !r.song && songs.iter().any(|s| same_track(s, &r.track)))
        .map(|r| r.mixed_pos)
        .collect();
    out.tracks = rows
        .iter()
        .filter(|r| r.song || !dead.contains(&r.mixed_pos))
        .map(|r| r.track.clone())
        .collect();
    let mut mixed = std::mem::take(&mut out.mixed);
    mixed = mixed
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !dead.contains(i))
        .map(|(_, m)| m)
        .collect();
    out.mixed = mixed;
}

/// One flat item into the typed vecs + mixed order. Endpoint shape
/// decides artists/albums/playlists (browse id prefix); songs and
/// videos share the watch path and split on the player type, with the
/// kind token as backup. Anything unrecognized is skipped.
fn push_item(out: &mut SearchAll, rows: &mut Vec<TrackRow>, item: &Value, force: Option<u8>) {
    let title: String = flex_runs(item, 0).concat();
    if title.is_empty() {
        return;
    }
    let segs = segments(&flex_runs(item, 1));
    // Type-led segments start past the token; bare shelf segments
    // (filtered shelves omit it) start at the artist/author.
    let first = segs.first().cloned().unwrap_or_default();
    let b: &[String] = if token_kind(&first).is_some() {
        &segs[1..]
    } else {
        &segs[..]
    };
    let art = thumbs(item);
    if let Some(browse) = browse_id(item) {
        if browse.starts_with("UC") {
            out.artists.push(Artist {
                id: browse,
                name: title.clone(),
                thumbnail_url: art.0,
            });
            let id = out.artists.last().map(|a| a.id.clone()).unwrap_or_default();
            out.mixed.push(MixedRef { kind: 1, id });
        } else if browse.starts_with("MPRE") {
            out.albums.push(Album {
                id: browse,
                title: title.clone(),
                artist: b.first().cloned().unwrap_or_default(),
                year: b.last().and_then(|y| y.trim().parse().ok()),
                thumbnail_url: art.0,
            });
            let id = out.albums.last().map(|a| a.id.clone()).unwrap_or_default();
            out.mixed.push(MixedRef { kind: 2, id });
        } else if browse.starts_with("VL") || browse.starts_with("PL") {
            out.playlists.push(PlaylistHit {
                id: browse,
                title: title.clone(),
                author: b.first().cloned().unwrap_or_default(),
                detail: b.get(1).cloned().unwrap_or_default(),
                thumbnail_url: art.0,
            });
            let id = out
                .playlists
                .last()
                .map(|p| p.id.clone())
                .unwrap_or_default();
            out.mixed.push(MixedRef { kind: 3, id });
        }
        return;
    }
    // Songs + videos: id from the item data or the play overlay.
    let video_id = item
        .get("playlistItemData")
        .and_then(|p| p.get("videoId"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| play_video_id(item))
        .map(str::to_string);
    let Some(video_id) = video_id else { return };
    // Song middle segments are album-or-nothing: `[artist, album,
    // duration]`, or a bare `[artist, duration]` with no album.
    // Filtered shelves carry durations; flat items do not.
    let (album, duration) = match (b.get(1), b.get(2)) {
        (Some(a), Some(d)) => (
            Some(a.clone()).filter(|s| !s.is_empty()),
            parse_duration_secs(d),
        ),
        (Some(x), None) if x.contains(':') => (None, parse_duration_secs(x)),
        (Some(a), None) => (Some(a.clone()).filter(|s| !s.is_empty()), 0),
        _ => (None, 0),
    };
    let song = match (force.or_else(|| token_kind(&first)), music_video_type(item)) {
        (Some(1), _) => false,
        (Some(0), _) => true,
        (None, Some(t)) => t == "MUSIC_VIDEO_TYPE_ATV",
        // No signal at all: skip, never misfile.
        _ => return,
    };
    // An explicit video player type wins over a forced song shelf:
    // views must never land in the album slot.
    let video_row = music_video_type(item).is_some_and(|t| t != "MUSIC_VIDEO_TYPE_ATV");
    let is_song = song && !video_row;
    let track = if is_song {
        Track {
            id: video_id.clone(),
            title: title.clone(),
            artist: b.first().cloned().unwrap_or_default(),
            album,
            duration_secs: duration,
            thumbnail_url: art.0,
            thumbnail_large_url: art.1,
            plays: third_col_plays(item),
        }
    } else {
        Track {
            id: video_id.clone(),
            title: title.clone(),
            artist: b.first().cloned().unwrap_or_default(),
            album: None,
            duration_secs: b.get(2).map(|d| parse_duration_secs(d)).unwrap_or(0),
            thumbnail_url: art.0,
            thumbnail_large_url: art.1,
            plays: b.get(1).cloned().filter(|v| !v.is_empty()),
        }
    };
    rows.push(TrackRow {
        song: is_song,
        mixed_pos: out.mixed.len(),
        track,
    });
    out.mixed.push(MixedRef {
        kind: 0,
        id: video_id,
    });
}

/// "2.1B plays" style third column (songs only; videos carry views in
/// the subtitle instead).
fn third_col_plays(item: &Value) -> Option<String> {
    let text: String = flex_runs(item, 2).concat();
    (!text.is_empty()).then_some(text)
}

/// `watchEndpoint.videoId` behind the hover play overlay.
fn play_video_id(item: &Value) -> Option<&str> {
    item.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId")?
        .as_str()
        .filter(|s| !s.is_empty())
}

/// `MUSIC_VIDEO_TYPE_ATV` (official audio) vs `OMV`/`UGC` (videos).
fn music_video_type(item: &Value) -> Option<&str> {
    item.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType")?
        .as_str()
}

/// Top-level browse target (`UC*` artists, `MPRE*` albums, `VL*`/`RD*`
/// playlists). Songs and videos carry none.
fn browse_id(item: &Value) -> Option<String> {
    let id = item
        .get("navigationEndpoint")?
        .get("browseEndpoint")?
        .get("browseId")?
        .as_str()
        .filter(|s| !s.is_empty())?;
    Some(id.to_string())
}

/// Top card: display metadata plus the tap-target id (watch video or
/// browse id), so the hero plays without shelf matching.
fn parse_top_card(card: &Value) -> Option<TopHit> {
    let title = card
        .get("title")
        .map(|t| run_texts(t).concat())
        .unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let runs = card.get("subtitle").map(run_texts).unwrap_or_default();
    let token = runs.first().cloned().unwrap_or_default();
    let kind = match token_kind(&token) {
        Some(0) => TopKind::Song,
        Some(1) => TopKind::Video,
        Some(2) => TopKind::Artist,
        Some(3) => TopKind::Album,
        Some(4) => TopKind::Playlist,
        _ => TopKind::Other,
    };
    let id = card
        .get("onTap")
        .and_then(|t| t.get("watchEndpoint"))
        .and_then(|w| w.get("videoId"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            card.get("onTap")
                .and_then(|t| t.get("browseEndpoint"))
                .and_then(|b| b.get("browseId"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    let (small, _) = thumbs(card);
    Some(TopHit {
        kind,
        id,
        title,
        artist: runs.get(2).cloned().unwrap_or_default(),
        thumbnail_url: small,
    })
}

/// Text runs of flex column `i` (unwraps the per-column renderer).
fn flex_runs(item: &Value, i: usize) -> Vec<String> {
    item.get("flexColumns")
        .and_then(Value::as_array)
        .and_then(|cols| cols.get(i))
        .and_then(|col| col.get("musicResponsiveListItemFlexColumnRenderer"))
        .map(run_texts)
        .unwrap_or_default()
}

/// Each text run separately (separators stay addressable). Flex
/// columns wrap runs in `text`, card/shelf titles hold them directly.
fn run_texts(col: &Value) -> Vec<String> {
    let runs = col
        .get("runs")
        .or_else(|| col.get("text").and_then(|t| t.get("runs")))
        .and_then(Value::as_array);
    runs.into_iter()
        .flatten()
        .filter_map(|r| r.get("text"))
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Smallest + largest artwork URLs under a thumbnail renderer.
fn thumbs(item: &Value) -> (Option<String>, Option<String>) {
    let list = item
        .get("thumbnail")
        .and_then(|t| t.get("musicThumbnailRenderer"))
        .and_then(|r| r.get("thumbnail"))
        .and_then(|t| t.get("thumbnails"))
        .and_then(Value::as_array);
    let mut urls: Vec<(&str, u32)> = list
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let url = t.get("url")?.as_str()?;
            let width = t.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
            (!url.is_empty()).then_some((url, width.max(1)))
        })
        .collect();
    if urls.is_empty() {
        return (None, None);
    }
    urls.sort_by_key(|(_, w)| *w);
    (
        Some(urls.first().map(|(u, _)| u.to_string()).unwrap_or_default()),
        Some(urls.last().map(|(u, _)| u.to_string()).unwrap_or_default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal flat response: card + one of each playable kind plus
    /// skippable kinds (episode, podcast, profile) and a broken item.
    fn fixture() -> Value {
        serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {"musicCardShelfRenderer": {
                                            "title": {"runs": [{"text": "Chop Suey!"}]},
                                            "subtitle": {"runs": [
                                                {"text": "Video"}, {"text": " • "},
                                                {"text": "System Of A Down"}, {"text": " • "},
                                                {"text": "1.6B views"}, {"text": " • "},
                                                {"text": "3:29"},
                                            ]},
                                            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                                                {"url": "https://example.invalid/s", "width": 60, "height": 60},
                                                {"url": "https://example.invalid/l", "width": 544, "height": 544},
                                            ]}}},
                                            "onTap": {"watchEndpoint": {"videoId": "CSvFpBOe8eY"}},
                                        }},
                                        {"itemSectionRenderer": {"contents": [
                                            {"musicResponsiveListItemRenderer": {
                                                "flexColumns": [
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Chop Suey!"}]}}},
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                        {"text": "Song"}, {"text": " • "}, {"text": "System Of A Down"},
                                                    ]}}},
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "2.1B plays"}]}}},
                                                ],
                                                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                                                    {"url": "https://example.invalid/t", "width": 60, "height": 60},
                                                ]}}},
                                                "playlistItemData": {"videoId": "-cid1qHuy_U"},
                                            }},
                                            {"musicResponsiveListItemRenderer": {
                                                "flexColumns": [
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "System of A Down"}]}}},
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                        {"text": "Artist"}, {"text": " • "}, {"text": "29.3M monthly audience"},
                                                    ]}}},
                                                ],
                                                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                                                "navigationEndpoint": {"browseEndpoint": {"browseId": "UC123"}},
                                            }},
                                            {"musicResponsiveListItemRenderer": {
                                                "flexColumns": [
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Chop Suey"}]}}},
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                        {"text": "Single"}, {"text": " • "},
                                                        {"text": "Vesanic Rave"}, {"text": " & "}, {"text": "Vesanic"},
                                                        {"text": " • "}, {"text": "2025"},
                                                    ]}}},
                                                ],
                                                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                                                "navigationEndpoint": {"browseEndpoint": {"browseId": "MPRE456"}},
                                            }},
                                            {"musicResponsiveListItemRenderer": {
                                                "flexColumns": [
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Mix"}]}}},
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                        {"text": "Playlist"}, {"text": " • "},
                                                        {"text": "YouTube Music"}, {"text": " • "}, {"text": "31 songs"},
                                                    ]}}},
                                                ],
                                                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                                                "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPL789"}},
                                            }},
                                            {"musicResponsiveListItemRenderer": {
                                                "flexColumns": [
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Show"}]}}},
                                                    {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                        {"text": "Podcast"}, {"text": " • "}, {"text": "Pub"},
                                                    ]}}},
                                                ],
                                                "navigationEndpoint": {"browseEndpoint": {"browseId": "MDP123"}},
                                            }},
                                            {"musicResponsiveListItemRenderer": {
                                                "flexColumns": [],
                                            }},
                                        ]}},
                                    ],
                                },
                            },
                        },
                    }],
                },
            },
        })
    }

    #[test]
    fn flat_response_parses_in_server_order() {
        let out = parse_search_all(&fixture().to_string()).expect("parse");
        let top = out.top.expect("top card");
        // The top card is the server's pick verbatim (song preference
        // applies to rows, never to the ranked top hit: it carries no
        // duration to disambiguate long videos from songs).
        assert_eq!(top.kind, TopKind::Video);
        assert_eq!(top.id, "CSvFpBOe8eY");
        assert_eq!(top.artist, "System Of A Down");
        assert_eq!(
            top.thumbnail_url.as_deref(),
            Some("https://example.invalid/s")
        );
        assert_eq!(out.tracks.len(), 1);
        assert_eq!(out.tracks[0].id, "-cid1qHuy_U");
        assert_eq!(out.tracks[0].plays.as_deref(), Some("2.1B plays"));
        assert_eq!(out.artists.len(), 1);
        assert_eq!(out.artists[0].id, "UC123");
        assert_eq!(out.albums.len(), 1);
        assert_eq!(out.albums[0].id, "MPRE456");
        assert_eq!(out.albums[0].artist, "Vesanic Rave & Vesanic");
        assert_eq!(out.albums[0].year, Some(2025));
        assert_eq!(out.playlists.len(), 1);
        assert_eq!(out.playlists[0].author, "YouTube Music");
        assert_eq!(out.playlists[0].detail, "31 songs");
        // Podcast + broken item skipped; mixed is the web row order.
        assert_eq!(
            out.mixed
                .iter()
                .map(|m| (m.kind, m.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (0, "-cid1qHuy_U"),
                (1, "UC123"),
                (2, "MPRE456"),
                (3, "VLPL789"),
            ]
        );
    }

    /// One flat item with the given subtitle runs + endpoint bits.
    fn item(title: &str, sub: Vec<&str>, extra: serde_json::Value) -> Value {
        let runs: Vec<Value> = sub
            .into_iter()
            .map(|t| serde_json::json!({"text": t}))
            .collect();
        let mut item = serde_json::json!({
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": title}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs}}},
            ],
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
        });
        for (k, v) in extra.as_object().unwrap() {
            item[k] = v.clone();
        }
        item
    }

    fn wrap(items: Vec<Value>) -> String {
        let contents: Vec<Value> = items
            .into_iter()
            .map(|r| serde_json::json!({"musicResponsiveListItemRenderer": r}))
            .collect();
        serde_json::json!({
            "contents": {"tabbedSearchResultsRenderer": {"tabs": [{
                "tabRenderer": {"content": {"sectionListRenderer": {"contents": [
                    {"itemSectionRenderer": {"contents": contents}},
                ]}}},
            }]}},
        })
        .to_string()
    }

    #[test]
    fn multi_artist_segments_keep_album_position() {
        // "Song • A & B" (one separator): artists join, no album.
        let out = parse_search_all(&wrap(vec![item(
            "Duo",
            vec!["Song", " • ", "A", " & ", "B"],
            serde_json::json!({"playlistItemData": {"videoId": "v1"}}),
        )]))
        .expect("parse");
        assert_eq!(out.tracks.len(), 1);
        assert_eq!(out.tracks[0].artist, "A & B");
        assert!(out.tracks[0].album.is_none());
        // "Song • A • Album" (two separators): last segment is the album.
        let out = parse_search_all(&wrap(vec![item(
            "Solo",
            vec!["Song", " • ", "A", " • ", "Album"],
            serde_json::json!({"playlistItemData": {"videoId": "v2"}}),
        )]))
        .expect("parse");
        assert_eq!(out.tracks[0].artist, "A");
        assert_eq!(out.tracks[0].album.as_deref(), Some("Album"));
    }

    #[test]
    fn radio_mixes_skip_but_bare_playlists_map() {
        let out = parse_search_all(&wrap(vec![
            item(
                "Mix",
                vec!["Playlist", " • ", "YTM", " • ", "31 songs"],
                serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "RDCLAK123"}}}),
            ),
            item(
                "Fan",
                vec!["Playlist", " • ", "Fan", " • ", "5 views"],
                serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "PL456"}}}),
            ),
        ]))
        .expect("parse");
        assert_eq!(out.playlists.len(), 1);
        assert_eq!(out.playlists[0].id, "PL456");
        assert_eq!(out.mixed.len(), 1);
    }

    #[test]
    fn unknown_tokens_need_a_player_signal() {
        // Unknown token + no player type: skipped, never misfiled.
        let out = parse_search_all(&wrap(vec![item(
            "???",
            vec!["Karaoke", " • ", "Someone"],
            serde_json::json!({"playlistItemData": {"videoId": "v9"}}),
        )]))
        .expect("parse");
        assert!(out.tracks.is_empty() && out.mixed.is_empty());
        // Unknown token + ATV overlay: trusted as a song.
        let out = parse_search_all(&wrap(vec![item(
            "???",
            vec!["Karaoke", " • ", "Someone"],
            serde_json::json!({
                "playlistItemData": {"videoId": "v9"},
                "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {
                    "playNavigationEndpoint": {"watchEndpoint": {
                        "videoId": "v9",
                        "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}},
                    }},
                }}}},
            }),
        )]))
        .expect("parse");
        assert_eq!(out.tracks.len(), 1);
    }

    #[test]
    fn old_shelf_layout_maps_by_title() {
        let shelf = serde_json::json!({
            "musicShelfRenderer": {
                "title": {"runs": [{"text": "Songs"}]},
                "contents": [
                    {"musicResponsiveListItemRenderer": {
                        "flexColumns": [
                            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Hey"}]}}},
                            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                {"text": "Song"}, {"text": " • "}, {"text": "Band"},
                            ]}}},
                        ],
                        "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                        "playlistItemData": {"videoId": "vshelf"},
                    }},
                ],
            },
        });
        let text = serde_json::json!({
            "contents": {"tabbedSearchResultsRenderer": {"tabs": [{
                "tabRenderer": {"content": {"sectionListRenderer": {"contents": [shelf]}}},
            }]}},
        })
        .to_string();
        let out = parse_search_all(&text).expect("parse");
        assert_eq!(out.tracks.len(), 1);
        assert_eq!(out.tracks[0].id, "vshelf");
    }

    #[test]
    fn bare_shelf_songs_parse_artist_album_duration() {
        // Filtered songs omit the type token: artist, album, duration.
        // (The overlay player type is the classification signal.)
        let atv = |id: &str| {
            serde_json::json!({"overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {
                "playNavigationEndpoint": {"watchEndpoint": {
                    "videoId": id,
                    "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}},
                }},
            }}}}})
        };
        let with_overlay = |id: &str| {
            let mut extra = serde_json::json!({"playlistItemData": {"videoId": id}});
            extra["overlay"] = atv(id)["overlay"].clone();
            extra
        };
        let out = parse_search_all(&wrap(vec![item(
            "Chop Suey!",
            vec!["System Of A Down", " • ", "Toxicity", " • ", "3:31"],
            with_overlay("v1"),
        )]))
        .expect("parse");
        assert_eq!(out.tracks.len(), 1);
        assert_eq!(out.tracks[0].artist, "System Of A Down");
        assert_eq!(out.tracks[0].album.as_deref(), Some("Toxicity"));
        assert_eq!(out.tracks[0].duration_secs, 211);
        // No-album shape keeps the duration out of the album slot.
        let out = parse_search_all(&wrap(vec![item(
            "Intro",
            vec!["Band", " • ", "0:42"],
            with_overlay("v2"),
        )]))
        .expect("parse");
        assert_eq!(out.tracks[0].artist, "Band");
        assert!(out.tracks[0].album.is_none());
        assert_eq!(out.tracks[0].duration_secs, 42);
        // Bare shelf playlists map author + detail without a token.
        let out = parse_search_all(&wrap(vec![item(
            "Mix",
            vec!["Fan", " • ", "56 views"],
            serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "VL1"}}}),
        )]))
        .expect("parse");
        assert_eq!(out.playlists.len(), 1);
        assert_eq!(out.playlists[0].author, "Fan");
        assert_eq!(out.playlists[0].detail, "56 views");
    }

    #[test]
    fn missing_section_list_errors_but_bad_items_do_not() {
        assert!(parse_search_all("{}").is_err());
        assert!(parse_search_all("not json").is_err());
        let mut broken = fixture();
        broken["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]
            ["sectionListRenderer"]["contents"][1]["itemSectionRenderer"]["contents"]
            .as_array_mut()
            .unwrap()
            .clear();
        let out = parse_search_all(&broken.to_string()).expect("parse");
        assert!(out.tracks.is_empty() && out.mixed.is_empty());
        assert!(out.top.is_some());
    }

    #[test]
    fn filter_params_match_web_tabs() {
        // Same `params` the web client sends per tab (mirrors the
        // ytmapi-rs filtered queries with ExactMatch spelling).
        assert_eq!(
            SearchFilterKind::Songs.params(),
            "EgWKAQIIAWoMEA4QChADEAQQCRAF"
        );
        assert_eq!(
            SearchFilterKind::Albums.params(),
            "EgWKAQIYAWoMEA4QChADEAQQCRAF"
        );
        assert_eq!(
            SearchFilterKind::Artists.params(),
            "EgWKAQIgAWoMEA4QChADEAQQCRAF"
        );
        assert_eq!(
            SearchFilterKind::Playlists.params(),
            "EgeKAQQoAEABagwQDhAKEAMQBBAJEAU%3D"
        );
    }

    #[test]
    fn sapisid_hash_shape() {
        let hash = sapisid_hash(1234567890, "abcDEF123");
        assert!(hash.starts_with("1234567890_"));
        assert_eq!(hash.len(), 11 + 40);
        assert!(hash[11..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Extra payload for video rows: playlist id plus the OMV player
    /// type (the mirror of the ATV helper elsewhere).
    fn with_video(id: &str) -> Value {
        serde_json::json!({
            "playlistItemData": {"videoId": id},
            "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {
                "playNavigationEndpoint": {"watchEndpoint": {
                    "videoId": id,
                    "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": "MUSIC_VIDEO_TYPE_OMV"}},
                }},
            }}}},
        })
    }

    #[test]
    fn song_preference_drops_duplicate_videos() {
        let out = parse_search_all(&wrap(vec![
            item(
                "Chop Suey!",
                vec!["Song", " • ", "System Of A Down"],
                serde_json::json!({"playlistItemData": {"videoId": "song1"}}),
            ),
            item(
                "Chop Suey! (Official Video)",
                vec!["Video", " • ", "System Of A Down"],
                with_video("vid1"),
            ),
            // Interleaved kinds hold their mixed positions around drops.
            item(
                "System of A Down",
                vec!["Artist", " • ", "29.3M monthly audience"],
                serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "UC123"}}}),
            ),
            item(
                "Lonely Day",
                vec!["Video", " • ", "System Of A Down"],
                with_video("vid2"),
            ),
            item(
                "Toxicity",
                vec!["Song", " • ", "System Of A Down"],
                serde_json::json!({"playlistItemData": {"videoId": "song2"}}),
            ),
        ]))
        .expect("parse");
        // Dup video gone, everything else in walk order (mixed too).
        let ids: Vec<&str> = out.tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["song1", "vid2", "song2"]);
        let mixed: Vec<(u8, &str)> = out.mixed.iter().map(|m| (m.kind, m.id.as_str())).collect();
        assert_eq!(
            mixed,
            vec![(0, "song1"), (1, "UC123"), (0, "vid2"), (0, "song2")]
        );
    }

    #[test]
    fn song_preference_keeps_videos_without_a_song_twin() {
        // Different durations: different recordings, both stay.
        let out = parse_search_all(&wrap(vec![
            item(
                "Chop Suey!",
                vec!["Song", " • ", "Band", " • ", "Album", " • ", "3:31"],
                serde_json::json!({"playlistItemData": {"videoId": "song1"}}),
            ),
            item(
                "Chop Suey!",
                vec!["Video", " • ", "Band", " • ", "1M views", " • ", "9:99"],
                with_video("vid1"),
            ),
            // Same duration: the video is the same track, dropped.
            item(
                "Intro",
                vec!["Song", " • ", "Band", " • ", "0:42"],
                serde_json::json!({"playlistItemData": {"videoId": "song2"}}),
            ),
            item(
                "Intro",
                vec!["Video", " • ", "Band", " • ", "9 views", " • ", "0:42"],
                with_video("vid2"),
            ),
            // Remaster markers are not video markers: both stay.
            item(
                "Song",
                vec!["Song", " • ", "Band"],
                serde_json::json!({"playlistItemData": {"videoId": "song3"}}),
            ),
            item(
                "Song (Remastered)",
                vec!["Video", " • ", "Band"],
                with_video("vid3"),
            ),
            // Topic channels match the plain artist name.
            item(
                "Anthem",
                vec!["Song", " • ", "Band"],
                serde_json::json!({"playlistItemData": {"videoId": "song4"}}),
            ),
            item(
                "Anthem [Video]",
                vec!["Video", " • ", "Band - Topic"],
                with_video("vid4"),
            ),
        ]))
        .expect("parse");
        let ids: Vec<&str> = out.tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["song1", "vid1", "song2", "song3", "vid3", "song4"]
        );
    }

    #[test]
    fn norm_title_strips_video_markers_only() {
        assert_eq!(norm_title("Chop Suey! (Official Video)"), "chop suey!");
        assert_eq!(norm_title("Anthem [Official Music Video]"), "anthem");
        assert_eq!(norm_title("Song - Music Video"), "song");
        assert_eq!(norm_title("Anthem [Official Lyric Video]"), "anthem");
        assert_eq!(norm_title("Clip (Visualizer)"), "clip");
        // Non-video parentheticals survive (never merge remasters).
        assert_eq!(norm_title("Song (Remastered)"), "song (remastered)");
        assert_eq!(norm_title("Song (Live)"), "song (live)");
        assert_eq!(norm_artist("Band - Topic"), "band");
        assert_eq!(norm_artist("BandVEVO"), "band");
    }

    #[test]
    fn cookie_sapisid_extracts() {
        let auth = SearchAuth::cookie("SID=x; SAPISID=mysecret; HSID=y".into());
        assert_eq!(auth.sapisid(), Some("mysecret"));
        assert!(SearchAuth::anonymous().sapisid().is_none());
    }
}
