//! Playlist import sources besides YouTube Music: Deezer (official public
//! API), Spotify and Apple Music (the public playlist pages a browser gets
//! anonymously), and CSV / plain-text exports from any service.
//!
//! Every source boils down to [`ForeignTrack`] rows (title, artist, album,
//! duration); [`crate::song_match::resolve_foreign`] then finds each one on
//! YouTube Music. Parsing is pure and unit-tested; fetching is gated on
//! `innertube` (reqwest + tokio).
//!
//! Etiquette for page reads: only on a user-pasted link, one playlist per
//! import, no login, no token extraction, no cookies, a plain browser
//! request. A page format change surfaces as a read error, and the dialog
//! points users at the CSV route instead.

use std::path::PathBuf;

/// Where a pasted link (or picked file) points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    /// YouTube / YouTube Music playlist id (canonical edit form).
    YouTube(String),
    /// Deezer: numeric playlist id, or a short share link to resolve.
    Deezer(LinkId),
    /// Spotify: playlist id, or a short share link to resolve.
    Spotify(LinkId),
    /// Apple Music: the full playlist page URL (keeps the storefront).
    Apple(String),
    /// CSV or plain-text export on disk.
    File(PathBuf),
}

/// A platform playlist id, or a short share link (`link.deezer.com`,
/// `spotify.link`) whose redirect names the id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkId {
    Id(String),
    Short(String),
}

impl ImportSource {
    /// Platform name for messages ("Spotify", "CSV file", ...).
    pub fn platform(&self) -> &'static str {
        match self {
            ImportSource::YouTube(_) => "YouTube Music",
            ImportSource::Deezer(_) => "Deezer",
            ImportSource::Spotify(_) => "Spotify",
            ImportSource::Apple(_) => "Apple Music",
            ImportSource::File(_) => "file",
        }
    }
}

/// One playlist row from another platform, before matching.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignTrack {
    pub title: String,
    /// Primary artist first; several artists joined with `, `.
    pub artist: String,
    pub album: Option<String>,
    /// 0 when unknown.
    pub duration_secs: u32,
}

impl ForeignTrack {
    /// `Artist - Title` (or just the title) for "not found" lists.
    pub fn label(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artist, self.title)
        }
    }
}

/// A read playlist: its title, rows, and the total the source claims
/// when that is more than it handed over (a capped page).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignPlaylist {
    pub title: String,
    pub tracks: Vec<ForeignTrack>,
    pub limit: Option<ReadLimit>,
}

/// Why a source returned fewer rows than the playlist has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadLimit {
    /// The source said how many there are (`got` of `total`).
    Partial { got: usize, total: usize },
    /// Spotify's public page lists at most this many songs and never
    /// says how many more exist.
    SpotifyCap(usize),
}

/// Rows a Spotify public page lists at most.
pub const SPOTIFY_PAGE_CAP: usize = 100;

/// Detect what `input` (a pasted link, a bare id, or a file path) points
/// at. Platform hosts win over the YouTube `list=` rule, so a foreign
/// link carrying a `list` query never reads as YouTube.
pub fn detect_source(input: &str) -> Option<ImportSource> {
    let s = input.trim().trim_matches(|c| c == '"' || c == '\'');
    if s.is_empty() {
        return None;
    }
    let lower = s.to_lowercase();
    if let Some(path) = file_path(s, &lower) {
        return Some(ImportSource::File(path));
    }
    if lower.starts_with("spotify:playlist:") {
        let id = &s["spotify:playlist:".len()..];
        return id_ok(id).then(|| ImportSource::Spotify(LinkId::Id(id.to_string())));
    }
    if lower.contains("spotify.link/") || lower.contains("spotify.app.link/") {
        return Some(ImportSource::Spotify(LinkId::Short(with_scheme(s))));
    }
    if lower.contains("open.spotify.com/") || lower.contains("play.spotify.com/") {
        return segment_after(s, "/playlist/")
            .filter(|id| id_ok(id))
            .map(|id| ImportSource::Spotify(LinkId::Id(id)));
    }
    if lower.contains("link.deezer.com/")
        || lower.contains("deezer.page.link/")
        || lower.contains("dzr.page.link/")
    {
        return Some(ImportSource::Deezer(LinkId::Short(with_scheme(s))));
    }
    if lower.contains("deezer.com/") {
        return segment_after(s, "/playlist/")
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            .map(|id| ImportSource::Deezer(LinkId::Id(id)));
    }
    if lower.contains("music.apple.com/") || lower.contains("itunes.apple.com/") {
        let ok = lower.contains("/playlist/")
            && segment_after(s, "/playlist/").is_some()
            && s.split(['/', '?', '#']).any(|seg| seg.starts_with("pl."));
        return ok.then(|| ImportSource::Apple(with_scheme(s)));
    }
    crate::library::parse_playlist_link(s).map(ImportSource::YouTube)
}

/// A local export: an existing file, or anything that looks like a path
/// to a `.csv` / `.txt` file (the reader reports a missing one).
fn file_path(s: &str, lower: &str) -> Option<PathBuf> {
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    let raw = s.strip_prefix("file://").unwrap_or(s);
    let path = PathBuf::from(raw);
    let looks = lower.ends_with(".csv") || lower.ends_with(".txt") || lower.ends_with(".tsv");
    (looks || (path.is_absolute() && path.is_file())).then_some(path)
}

fn with_scheme(s: &str) -> String {
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}

/// Path segment right after `marker` (query/fragment cut).
fn segment_after(s: &str, marker: &str) -> Option<String> {
    let at = s.find(marker)?;
    let rest = &s[at + marker.len()..];
    let end = rest.find(['/', '?', '#', '&']).unwrap_or(rest.len());
    let seg = &rest[..end];
    (!seg.is_empty()).then(|| seg.to_string())
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
}

// ---------------------------------------------------------------- parsers

/// Spotify embed page (`open.spotify.com/embed/playlist/{id}`): the
/// `__NEXT_DATA__` JSON carries the playlist entity with up to
/// [`SPOTIFY_PAGE_CAP`] rows (`title`, `subtitle` = artists, `duration`
/// in ms). A full page marks the read as capped.
pub fn parse_spotify_embed(html: &str) -> Result<ForeignPlaylist, String> {
    let json = script_json(
        html,
        r#"<script id="__NEXT_DATA__" type="application/json">"#,
    )
    .ok_or("spotify page: no playlist data (page format changed?)")?;
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("spotify page json: {e}"))?;
    let entity = value
        .pointer("/props/pageProps/state/data/entity")
        .ok_or("spotify page: no playlist entity (private or removed?)")?;
    let title = str_at(entity, "/name")
        .or_else(|| str_at(entity, "/title"))
        .unwrap_or_default();
    let rows = entity
        .get("trackList")
        .and_then(|v| v.as_array())
        .ok_or("spotify page: no track list")?;
    let tracks: Vec<ForeignTrack> = rows
        .iter()
        .filter(|r| r.get("entityType").and_then(|v| v.as_str()) != Some("episode"))
        .filter_map(|r| {
            let title = clean(str_at(r, "/title")?.as_str());
            (!title.is_empty()).then(|| ForeignTrack {
                title,
                artist: clean(&str_at(r, "/subtitle").unwrap_or_default()),
                album: None,
                duration_secs: ms_to_secs(r.get("duration")),
            })
        })
        .collect();
    let limit = (rows.len() >= SPOTIFY_PAGE_CAP).then_some(ReadLimit::SpotifyCap(SPOTIFY_PAGE_CAP));
    Ok(ForeignPlaylist {
        title: clean(&title),
        tracks,
        limit,
    })
}

/// Apple Music playlist page: the `serialized-server-data` JSON holds
/// sections; the header lockup has the title and `trackCount`, and the
/// `trackLockup` section the rows (`title`, `artistName`, `duration` in
/// ms, album as the first tertiary link).
pub fn parse_apple_page(html: &str) -> Result<ForeignPlaylist, String> {
    let json = script_json(
        html,
        r#"<script type="application/json" id="serialized-server-data">"#,
    )
    .ok_or("apple music page: no playlist data (page format changed?)")?;
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("apple music page json: {e}"))?;
    let sections = value
        .pointer("/data/0/data/sections")
        .and_then(|v| v.as_array())
        .ok_or("apple music page: no sections")?;
    let kind = |s: &serde_json::Value| {
        s.get("itemKind")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let header = sections
        .iter()
        .find(|s| kind(s).as_deref() == Some("containerDetailHeaderLockup"))
        .and_then(|s| s.pointer("/items/0"));
    let title = header.and_then(|h| str_at(h, "/title")).unwrap_or_default();
    let total = header
        .and_then(|h| h.get("trackCount"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let rows: Vec<&serde_json::Value> = sections
        .iter()
        .filter(|s| kind(s).as_deref() == Some("trackLockup"))
        .filter_map(|s| s.get("items").and_then(|v| v.as_array()))
        .flatten()
        .collect();
    if rows.is_empty() && header.is_none() {
        return Err("apple music page: no tracks (private or removed?)".into());
    }
    let tracks: Vec<ForeignTrack> = rows
        .iter()
        .filter_map(|r| {
            let title = clean(&str_at(r, "/title")?);
            (!title.is_empty()).then(|| ForeignTrack {
                title,
                artist: clean(
                    &str_at(r, "/artistName")
                        .or_else(|| str_at(r, "/subtitleLinks/0/title"))
                        .unwrap_or_default(),
                ),
                album: str_at(r, "/tertiaryLinks/0/title")
                    .map(|a| clean(&a))
                    .filter(|a| !a.is_empty()),
                duration_secs: ms_to_secs(r.get("duration")),
            })
        })
        .collect();
    let limit = total
        .filter(|&t| t > tracks.len())
        .map(|total| ReadLimit::Partial {
            got: tracks.len(),
            total,
        });
    Ok(ForeignPlaylist {
        title: clean(&title),
        tracks,
        limit,
    })
}

/// Deezer API playlist header (`/playlist/{id}`): title + track total.
/// A Deezer error object (private or missing list) is an error.
pub fn parse_deezer_header(json: &str) -> Result<(String, usize), String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("deezer json: {e}"))?;
    if let Some(err) = value.get("error") {
        let msg = str_at(err, "/message").unwrap_or_else(|| "unknown error".into());
        return Err(format!("deezer: {msg}"));
    }
    let title = str_at(&value, "/title").unwrap_or_default();
    let total = value.get("nb_tracks").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    Ok((clean(&title), total))
}

/// One Deezer API tracks page (`/playlist/{id}/tracks`): rows plus the
/// `next` page URL when there is one.
pub fn parse_deezer_tracks(json: &str) -> Result<(Vec<ForeignTrack>, Option<String>), String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("deezer json: {e}"))?;
    if let Some(err) = value.get("error") {
        let msg = str_at(err, "/message").unwrap_or_else(|| "unknown error".into());
        return Err(format!("deezer: {msg}"));
    }
    let rows = value
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or("deezer: no track data")?;
    let tracks = rows
        .iter()
        .filter_map(|r| {
            let title = clean(&str_at(r, "/title")?);
            (!title.is_empty()).then(|| ForeignTrack {
                title,
                artist: clean(&str_at(r, "/artist/name").unwrap_or_default()),
                album: str_at(r, "/album/title")
                    .map(|a| clean(&a))
                    .filter(|a| !a.is_empty()),
                duration_secs: r.get("duration").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            })
        })
        .collect();
    let next = str_at(&value, "/next").filter(|n| n.starts_with("https://api.deezer.com/"));
    Ok((tracks, next))
}

/// CSV / TSV / plain-text export. Headed files map known column names
/// (Exportify, TuneMyMusic, Soundiiz and generic `title`/`artist`
/// headers); files without a recognizable header read one song per
/// line as `Artist - Title` (or a bare title). `fallback_title` names
/// the playlist unless a `Playlist name` column does.
pub fn parse_csv(text: &str, fallback_title: &str) -> Result<ForeignPlaylist, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let delim = [',', ';', '\t']
        .into_iter()
        .max_by_key(|d| count_outside_quotes(first, *d))
        .filter(|d| count_outside_quotes(first, *d) > 0);
    let mut out = ForeignPlaylist {
        title: fallback_title.to_string(),
        ..Default::default()
    };
    if let Some(delim) = delim {
        let records = csv_records(text, delim);
        if let Some((header, rows)) = records.split_first() {
            if let Some(cols) = Columns::find(header) {
                let mut playlist_name = None;
                for row in rows {
                    let get = |i: Option<usize>| {
                        i.and_then(|i| row.get(i))
                            .map(|v| clean(v))
                            .unwrap_or_default()
                    };
                    let title = get(Some(cols.title));
                    if title.is_empty() {
                        continue;
                    }
                    if playlist_name.is_none() {
                        playlist_name = Some(get(cols.playlist)).filter(|p| !p.is_empty());
                    }
                    let album = get(cols.album);
                    out.tracks.push(ForeignTrack {
                        title,
                        artist: get(cols.artist),
                        album: (!album.is_empty()).then_some(album),
                        duration_secs: cols
                            .duration
                            .and_then(|i| row.get(i))
                            .map(|v| parse_duration(v, cols.duration_ms))
                            .unwrap_or(0),
                    });
                }
                if let Some(name) = playlist_name {
                    out.title = name;
                }
                return if out.tracks.is_empty() {
                    Err("csv: no songs in the file".into())
                } else {
                    Ok(out)
                };
            }
        }
    }
    // No usable header: one song per line.
    for line in text.lines() {
        let line = clean(line);
        if line.is_empty() {
            continue;
        }
        let (artist, title) = match line.split_once(" - ") {
            Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
            None => (String::new(), line),
        };
        if !title.is_empty() {
            out.tracks.push(ForeignTrack {
                title,
                artist,
                ..Default::default()
            });
        }
    }
    if out.tracks.is_empty() {
        Err("csv: no songs in the file".into())
    } else {
        Ok(out)
    }
}

/// Known column positions of a headed export.
struct Columns {
    title: usize,
    artist: Option<usize>,
    album: Option<usize>,
    duration: Option<usize>,
    duration_ms: bool,
    playlist: Option<usize>,
}

impl Columns {
    fn find(header: &[String]) -> Option<Self> {
        let names: Vec<String> = header
            .iter()
            .map(|h| h.trim().trim_matches('"').to_lowercase())
            .collect();
        let pos = |cands: &[&str]| names.iter().position(|n| cands.contains(&n.as_str()));
        let title = pos(&[
            "track name",
            "track title",
            "song name",
            "song title",
            "title",
            "track",
            "song",
            "name",
        ])?;
        let duration = pos(&[
            "duration (ms)",
            "duration_ms",
            "duration ms",
            "track duration (ms)",
        ]);
        let (duration, duration_ms) = match duration {
            Some(i) => (Some(i), true),
            None => (pos(&["duration", "length", "time"]), false),
        };
        Some(Columns {
            title,
            artist: pos(&[
                "artist name(s)",
                "artist name",
                "artist names",
                "artist(s)",
                "artists",
                "artist",
                "creator",
                "performer",
            ]),
            album: pos(&["album name", "album title", "album"]),
            duration,
            duration_ms,
            playlist: pos(&["playlist name", "playlist"]),
        })
    }
}

/// `3:20`, `1:02:03`, plain seconds, or milliseconds (`ms` columns, or
/// bare numbers too large to be seconds).
fn parse_duration(v: &str, ms: bool) -> u32 {
    let v = v.trim();
    if v.contains(':') {
        return v
            .split(':')
            .map(|p| p.trim().parse::<u32>().ok())
            .try_fold(0u32, |acc, p| p.map(|p| acc * 60 + p))
            .unwrap_or(0);
    }
    match v.parse::<f64>() {
        Ok(n) if ms || n > 36_000.0 => (n / 1000.0).round() as u32,
        Ok(n) => n.round() as u32,
        Err(_) => 0,
    }
}

fn count_outside_quotes(line: &str, delim: char) -> usize {
    let mut quoted = false;
    line.chars()
        .filter(|&c| {
            if c == '"' {
                quoted = !quoted;
            }
            c == delim && !quoted
        })
        .count()
}

/// RFC 4180 records: quoted fields may hold the delimiter, newlines and
/// doubled quotes. Blank lines are skipped.
fn csv_records(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == delim => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.trim().is_empty()) {
                    records.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => field.push(c),
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.trim().is_empty()) {
        records.push(row);
    }
    records
}

/// Inner JSON of the `<script>` opened by `open_tag`.
fn script_json<'a>(html: &'a str, open_tag: &str) -> Option<&'a str> {
    let start = html.find(open_tag)? + open_tag.len();
    let end = html[start..].find("</script>")? + start;
    Some(html[start..end].trim())
}

fn str_at(v: &serde_json::Value, pointer: &str) -> Option<String> {
    v.pointer(pointer)
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

fn ms_to_secs(v: Option<&serde_json::Value>) -> u32 {
    v.and_then(|x| x.as_u64())
        .map(|ms| ((ms + 500) / 1000) as u32)
        .unwrap_or(0)
}

/// Trim, swap no-break spaces (Spotify artist lists) for plain ones and
/// collapse runs of whitespace.
fn clean(s: &str) -> String {
    s.replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------- fetching

/// Cap for one page/API read (connect + body).
#[cfg(feature = "innertube")]
const FETCH_TIMEOUT_SECS: u64 = 20;
/// Largest export file read (a 10k-song CSV is well under 2 MB).
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Deezer pages per import (100 rows each): 10k songs.
#[cfg(feature = "innertube")]
const DEEZER_MAX_PAGES: usize = 100;

/// Plain public-page client: browser user agent, no cookies, short
/// redirect chains allowed (share links), bounded time.
#[cfg(feature = "innertube")]
fn page_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(crate::innertube::USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// GET `url` as text, returning the final URL after redirects too.
#[cfg(feature = "innertube")]
async fn get_text(url: &str) -> Result<(String, String), String> {
    let response = page_client()
        .get(url)
        .header("Accept-Language", "en-US,en;q=0.8")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = response.status();
    let final_url = response.url().to_string();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body = response
        .text()
        .await
        .map_err(|e| format!("body read failed: {e}"))?;
    Ok((final_url, body))
}

/// Read a non-YouTube playlist (`YouTube` sources error: they import
/// through the Innertube path).
#[cfg(feature = "innertube")]
pub async fn fetch_foreign(source: &ImportSource) -> Result<ForeignPlaylist, String> {
    match source {
        ImportSource::YouTube(_) => Err("youtube playlists import through innertube".into()),
        ImportSource::File(path) => read_file(path),
        ImportSource::Spotify(link) => {
            let id = resolve_id(link, "/playlist/", id_ok).await?;
            let (_, html) = get_text(&format!("https://open.spotify.com/embed/playlist/{id}"))
                .await
                .map_err(|e| format!("spotify: {e}"))?;
            parse_spotify_embed(&html)
        }
        ImportSource::Apple(url) => {
            let (_, html) = get_text(url)
                .await
                .map_err(|e| format!("apple music: {e}"))?;
            parse_apple_page(&html)
        }
        ImportSource::Deezer(link) => {
            let id = resolve_id(link, "/playlist/", |s| {
                s.chars().all(|c| c.is_ascii_digit())
            })
            .await?;
            let (_, head) = get_text(&format!("https://api.deezer.com/playlist/{id}"))
                .await
                .map_err(|e| format!("deezer: {e}"))?;
            let (title, total) = parse_deezer_header(&head)?;
            let mut tracks = Vec::new();
            let mut next = Some(format!(
                "https://api.deezer.com/playlist/{id}/tracks?index=0&limit=100"
            ));
            let mut pages = 0;
            while let Some(url) = next.take() {
                pages += 1;
                if pages > DEEZER_MAX_PAGES {
                    break;
                }
                let (_, body) = get_text(&url).await.map_err(|e| format!("deezer: {e}"))?;
                let (rows, more) = parse_deezer_tracks(&body)?;
                if rows.is_empty() {
                    break;
                }
                tracks.extend(rows);
                next = more;
            }
            let limit = (total > tracks.len()).then_some(ReadLimit::Partial {
                got: tracks.len(),
                total,
            });
            Ok(ForeignPlaylist {
                title,
                tracks,
                limit,
            })
        }
    }
}

/// The playlist id of a link: as given, or found in a short link's
/// redirect target (final URL first, then the landing page body).
#[cfg(feature = "innertube")]
async fn resolve_id(
    link: &LinkId,
    marker: &str,
    valid: fn(&str) -> bool,
) -> Result<String, String> {
    match link {
        LinkId::Id(id) => Ok(id.clone()),
        LinkId::Short(url) => {
            let (final_url, body) = get_text(url)
                .await
                .map_err(|e| format!("share link: {e}"))?;
            segment_after(&final_url, marker)
                .filter(|id| valid(id))
                .or_else(|| {
                    body.match_indices(marker).find_map(|(i, _)| {
                        segment_after(&body[i..], marker)
                            .map(|id| {
                                id.chars()
                                    .take_while(|c| c.is_ascii_alphanumeric())
                                    .collect::<String>()
                            })
                            .filter(|id| !id.is_empty() && valid(id))
                    })
                })
                .ok_or_else(|| "share link: no playlist behind it".to_string())
        }
    }
}

/// Read and parse an export file (size-capped, lossy UTF-8).
pub fn read_file(path: &std::path::Path) -> Result<ForeignPlaylist, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("file: {e}"))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("file: too large for a playlist export".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("file: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().replace(['_', '-'], " "))
        .unwrap_or_default();
    parse_csv(&text, clean(&stem).as_str())
}

#[cfg(test)]
#[path = "playlist_sources_tests.rs"]
mod tests;
