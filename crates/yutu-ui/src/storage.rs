//! yutu-ui storage: split from lib.rs (no behavior change).

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

pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// XDG data dir without extra deps: $XDG_DATA_HOME or ~/.local/share.
pub fn data_dir() -> String {
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.local/share")
    });
    format!("{base}/yutu-mux")
}

pub fn default_library_path() -> String {
    format!("{}/library.db", data_dir())
}

/// Disk dir for cached lyrics (`{sanitized-id}.json` per track).
pub fn lyrics_dir() -> String {
    format!("{}/lyrics", data_dir())
}

/// Disk dir for cached artwork (`{url-hash}.{ext}` per image URL).
/// Tests redirect it via `YUTU_TEST_ART_DIR` (same env-at-test-time
/// rule as the other `YUTU_TEST_*` vars) so completions never touch
/// the real cache.
pub fn artwork_dir() -> String {
    if let Ok(dir) = std::env::var("YUTU_TEST_ART_DIR") {
        if !dir.is_empty() {
            return dir;
        }
    }
    format!("{}/artwork", data_dir())
}

/// Filename-safe stem: ASCII alphanumerics kept, all else `_`.
/// Same rule as the lyrics files so ids map 1:1 with their `.json`.
pub(crate) fn sanitize_filename(s: &str) -> String {
    if s.is_empty() {
        return "unknown".to_string();
    }
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Lyrics file for `track_id` inside `dir` (caller passes [`lyrics_dir`]
/// in prod, a temp dir in tests).
pub(crate) fn lyrics_file_in(dir: &str, track_id: &str) -> String {
    format!("{}/{}.json", dir, sanitize_filename(track_id))
}

/// Persisted lyrics path for `track_id`.
pub fn lyrics_file_for(track_id: &str) -> String {
    lyrics_file_in(&lyrics_dir(), track_id)
}

/// Artwork stem for `url` inside `dir`: hex of the URL hash, so no URL
/// characters ever reach the filesystem. The extension is sniffed from
/// the bytes at save time (see [`artwork_ext`]).
pub(crate) fn artwork_stem_in(dir: &str, url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{}/{:016x}", dir, hasher.finish())
}

/// Extension sniffed from magic bytes. The image loader (`image::open`
/// behind iced file handles) keys the decoder off the file extension,
/// so extensionless files never render: every saved file carries one.
/// YTM thumbs are JPEG; unknown bytes default there too.
pub(crate) fn artwork_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.len() >= 12 && bytes[0..4] == *b"RIFF" && bytes[8..12] == *b"WEBP" {
        "webp"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else {
        "jpg"
    }
}

/// True for an observed cache age within the TTL (shared by the
/// memory and disk tiers; hermetic under test, unlike `Instant` or
/// file-mtime math).
pub(crate) fn art_age_fresh(age: std::time::Duration) -> bool {
    age.as_secs() <= ART_CACHE_TTL_SECS
}

/// True for a file mtime within the art TTL. Missing mtimes read as
/// fresh (absence is already handled by the caller), and future
/// mtimes are clock skew, never expiry.
pub(crate) fn art_mtime_fresh(modified: Option<std::time::SystemTime>) -> bool {
    match modified.and_then(|t| std::time::SystemTime::now().duration_since(t).ok()) {
        Some(age) => art_age_fresh(age),
        None => true,
    }
}

/// True when the file at `path` is older than the art cache TTL.
pub(crate) fn artwork_expired(path: &std::path::Path) -> bool {
    !art_mtime_fresh(std::fs::metadata(path).ok().and_then(|m| m.modified().ok()))
}

/// Candidate extensions for a saved artwork stem, in lookup order.
pub(crate) const ARTWORK_EXTS: [&str; 4] = ["jpg", "png", "webp", "gif"];

/// Saved artwork path for `url`, if any: tries the sniffed-extension
/// names, then migrates a legacy extensionless `.bin` file (sniff +
/// rename, best-effort) so early saves regain their covers.
/// Covers older than the TTL read as missing (never deleted here:
/// an offline viewer keeps its files until a successful fetch lets
/// the prune sweep them). One metadata call per candidate, so rows
/// stat once, not twice.
pub(crate) fn artwork_path_in(dir: &str, url: &str) -> Option<String> {
    let stem = artwork_stem_in(dir, url);
    for ext in ARTWORK_EXTS {
        let path = format!("{stem}.{ext}");
        match std::fs::metadata(&path).ok().filter(|m| m.is_file()) {
            None => continue,
            Some(m) if art_mtime_fresh(m.modified().ok()) => return Some(path),
            // Expired: a miss (the prune sweep deletes it).
            Some(_) => continue,
        }
    }
    let legacy = format!("{stem}.bin");
    if std::path::Path::new(&legacy).exists() {
        // Expired legacy files are a miss, never migrated (the prune
        // sweep deletes them like any other expired cover).
        if artwork_expired(std::path::Path::new(&legacy)) {
            return None;
        }
        if let Ok(bytes) = std::fs::read(&legacy) {
            let migrated = format!("{stem}.{}", artwork_ext(&bytes));
            if std::fs::rename(&legacy, &migrated).is_ok() {
                return Some(migrated);
            }
        }
        // Rename failed (or bytes unreadable): leave the legacy file;
        // nothing decodes it, so report absence.
    }
    None
}

/// Persisted artwork path for `url`, if cached.
pub fn artwork_path_for(url: &str) -> Option<String> {
    artwork_path_in(&artwork_dir(), url)
}

/// True when the artwork bytes for `url` are already on disk.
pub(crate) fn artwork_file_exists(url: &str) -> bool {
    artwork_path_for(url).is_some()
}

/// Lyrics previously cached on disk.
/// `None` when never fetched: the caller fires the network task.
#[cfg(feature = "lyrics")]
pub(crate) fn load_lyrics(track_id: &str) -> Option<yutu_core::LyricsResult> {
    load_lyrics_in(&lyrics_dir(), track_id)
}

/// [`load_lyrics`] from an explicit dir (tests pass a temp dir).
#[cfg(feature = "lyrics")]
pub(crate) fn load_lyrics_in(dir: &str, track_id: &str) -> Option<yutu_core::LyricsResult> {
    let bytes = std::fs::read(lyrics_file_in(dir, track_id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Best-effort persist of fetched artwork bytes (failures ignored).
pub(crate) fn save_artwork(url: &str, bytes: &[u8]) {
    save_artwork_in(&artwork_dir(), url, bytes);
}

/// Prune an artwork dir (best-effort): day-old covers are swept no
/// matter the size, then the survivors are capped to `max_bytes`
/// (LRU by mtime). The disk cache holds every fetched cover, so
/// browsing history cannot grow it without bound or pin it past
/// the TTL.
pub(crate) fn prune_artwork_dir_in(dir: &str, max_bytes: u64) {
    let entries: Vec<(u64, u64, std::path::PathBuf)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                // Unknown mtimes read as fresh (matching the lookup
                // path): uncounted files must never dodge the cap, so
                // they still sort oldest-first below.
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                Some((mtime, meta.len(), e.path()))
            })
            .collect(),
        Err(_) => return,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Expired covers go first, regardless of size (unknown mtimes
    // read as fresh, matching the lookup path).
    let mut fresh = Vec::with_capacity(entries.len());
    for (mtime, len, path) in entries {
        if mtime > 0 && now.saturating_sub(mtime) > ART_CACHE_TTL_SECS {
            let _ = std::fs::remove_file(&path);
        } else {
            fresh.push((mtime, len, path));
        }
    }
    let total: u64 = fresh.iter().map(|(_, len, _)| len).sum();
    if total <= max_bytes {
        return;
    }
    fresh.sort_by_key(|(mtime, _, _)| *mtime);
    let mut freed = 0u64;
    let need = total - max_bytes;
    for (_, len, path) in fresh {
        if freed >= need {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            freed += len;
        }
    }
}

/// [`save_artwork`] into an explicit dir (tests pass a temp dir).
/// Sibling extensions for the same stem are removed first: a changed
/// content type must not hide behind a stale file (lookup tries
/// extensions in order and would keep serving the old one).
/// Same-path rewrite note: iced keys decoded art by path hash, so a
/// rewritten file can paint stale pixels after an in-memory miss
/// (fresh fetches render from the memory handle; restarts decode the
/// file anew). Same-URL byte changes are rare upstream, so the stable
/// name stays: versioning per fetch would orphan a file per refresh.
pub(crate) fn save_artwork_in(dir: &str, url: &str, bytes: &[u8]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let stem = artwork_stem_in(dir, url);
    let ext = artwork_ext(bytes);
    for sibling in ARTWORK_EXTS {
        if sibling != ext {
            let _ = std::fs::remove_file(format!("{stem}.{sibling}"));
        }
    }
    let _ = std::fs::remove_file(format!("{stem}.bin"));
    let _ = std::fs::write(format!("{stem}.{ext}"), bytes);
}

/// Saved Google OAuth token bundle (0600, written by the login flow).
#[cfg(feature = "innertube")]
pub fn oauth_path() -> String {
    format!("{}/youtube_oauth.json", data_dir())
}

/// Saved cookie session fallback: the normalised `Cookie` header as a
/// 0600 file. Primary storage is the OS keychain (see
/// [`cookie_keyring_marker_path`]); this file holds the session only when
/// the keychain is unreachable, plus pre-keychain legacy sessions until
/// they migrate on next load.
#[cfg(feature = "innertube")]
pub fn cookie_session_path() -> String {
    format!("{}/cookie_session.txt", data_dir())
}

/// Non-secret marker: the session lives in the OS keychain
/// (`yutu-mux` / `cookie-session`, stable across releases). The marker
/// lets startup check existence without unlocking the keychain, so
/// opening the app never prompts for an OS password.
#[cfg(feature = "innertube")]
pub fn cookie_keyring_marker_path() -> String {
    format!("{}/cookie_session.keyring", data_dir())
}

#[cfg(feature = "innertube")]
pub(crate) fn cookie_session_exists() -> bool {
    yutu_core::secure_session_exists(
        std::path::Path::new(&cookie_session_path()),
        std::path::Path::new(&cookie_keyring_marker_path()),
    )
}

/// Load the saved header from the keychain (migrating the legacy file).
/// Hermetic under test (see [`load_scrobble_secret`]): unit builds fail
/// fast without touching the keychain, so logout/sync paths can never
/// delete or read a real session.
#[cfg(feature = "innertube")]
pub(crate) fn load_cookie_header() -> Result<String, String> {
    #[cfg(test)]
    {
        Err("secure store disabled for tests".into())
    }
    #[cfg(not(test))]
    {
        yutu_core::load_cookie_session(
            std::path::Path::new(&cookie_session_path()),
            std::path::Path::new(&cookie_keyring_marker_path()),
        )
    }
}

/// Delete the session everywhere (keychain, marker, legacy file).
/// Hermetic under test: unit builds report success without touching
/// anything (see [`load_cookie_header`]).
#[cfg(feature = "innertube")]
pub(crate) fn delete_cookie_session_secure() -> Result<(), String> {
    #[cfg(test)]
    {
        Ok(())
    }
    #[cfg(not(test))]
    {
        yutu_core::delete_cookie_session(
            std::path::Path::new(&cookie_session_path()),
            std::path::Path::new(&cookie_keyring_marker_path()),
        )
    }
}
#[cfg(feature = "innertube")]
pub(crate) fn oauth_path_exists() -> bool {
    std::path::Path::new(&oauth_path()).exists()
}

/// Sync prefs from SQLite: auto-sync toggle (default on) plus the last
/// completed pull, so the Settings label survives restarts.
#[cfg(feature = "innertube")]
pub(crate) fn load_sync_prefs(path: &str) -> (bool, Option<i64>) {
    let Ok(store) = LibraryStore::open(path) else {
        return (true, None);
    };
    let auto_sync = store.get_pref("auto_sync").unwrap_or(None).as_deref() != Some("0");
    let last_sync_unix = store
        .get_pref("last_sync_unix")
        .unwrap_or(None)
        .and_then(|v| v.parse().ok());
    (auto_sync, last_sync_unix)
}

#[allow(clippy::type_complexity)]
pub(crate) fn load_persisted(
    path: &str,
) -> (
    HashSet<String>,
    Vec<String>,
    HashSet<String>,
    Vec<String>,
    Vec<HistoryEntry>,
    Vec<Playlist>,
    u64,
    Vec<Track>,
    HashMap<String, HashMap<String, i64>>,
) {
    let empty = (
        HashSet::new(),
        Vec::new(),
        HashSet::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        0,
        Vec::new(),
        HashMap::new(),
    );
    let store = match LibraryStore::open(path) {
        Ok(s) => s,
        Err(_) => return empty,
    };
    // Likes: every id in SQLite in add order, no catalog probe.
    let order: Vec<String> = store.liked_ids().unwrap_or_default();
    let liked: HashSet<String> = order.iter().cloned().collect();
    // Dislikes ("don't recommend"): same contract as likes.
    let disliked_order: Vec<String> = store.disliked_ids().unwrap_or_default();
    let disliked: HashSet<String> = disliked_order.iter().cloned().collect();
    let history_full: Vec<HistoryEntry> = store.history(HISTORY_FULL_CAP).unwrap_or_default();
    // One-time migration: older imports stored browse (`VL...`) ids while
    // creates store edit (`PL...`) ids. Canonicalise to edit form so the
    // same list never appears twice and writes always send the edit form
    // (a `VL...` id to `browse/edit_playlist` 400s with invalid argument).
    // Runs only while stale browse-form rows exist.
    let mut user_playlists = store.user_playlists().unwrap_or_default();
    if user_playlists.iter().any(|p| p.id.starts_with("VL")) {
        let mut merged: Vec<Playlist> = Vec::new();
        let mut stale: Vec<String> = Vec::new();
        for p in user_playlists.drain(..) {
            if p.id.starts_with("VL") {
                stale.push(p.id.clone());
            }
            let id = yutu_core::canonical_playlist_id(&p.id);
            match merged.iter_mut().find(|m| m.id == id) {
                Some(m) => {
                    for t in p.tracks {
                        if !m.tracks.iter().any(|x| x.id == t.id) {
                            m.tracks.push(t);
                        }
                    }
                }
                None => merged.push(Playlist {
                    id,
                    title: p.title,
                    tracks: p.tracks,
                }),
            }
        }
        for m in &merged {
            let _ = store.sync_server_playlist(&m.id, &m.title, &m.tracks);
        }
        for old in stale {
            if old.starts_with("VL") {
                let _ = store.delete_playlist(&old);
            }
        }
        user_playlists = merged;
    }
    let seq = user_playlists
        .iter()
        .filter_map(|p| p.id.strip_prefix('u')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    // Track metadata cache: lets Favorites/history render on first
    // launch with no search (offline-friendly). Missing rows simply
    // mean an older install: the caller hydrates them from the network.
    let cached = store.all_cached_tracks().unwrap_or_default();
    let playlist_added = store.playlist_added_all().unwrap_or_default();
    // Legacy cleanup: pre-release installs may hold an offline audio
    // cache dir plus a `downloads` table; both are unused now.
    let _ = std::fs::remove_dir_all(format!("{}/offline_cache", data_dir()));
    (
        liked,
        order,
        disliked,
        disliked_order,
        history_full,
        user_playlists,
        seq,
        cached,
        playlist_added,
    )
}

/// Width for the Import button plus row spacing, in px (generous
/// estimate: the button must never be pushed out of view).
#[cfg(feature = "innertube")]
pub(crate) const COOKIE_IMPORT_ROOM: f32 = 120.0;

/// Chars of a picked path shown in the fixed-width picker field: `...`
/// plus the tail. Budget fits the 560px field at 12px (icon, spacing,
/// and padding discounted, margin to spare at every UI scale, since
/// field and glyphs scale together). Iced never clips overflowing text,
/// so an untrimmed label would paint past the field border.
#[cfg(feature = "innertube")]
pub(crate) const COOKIE_PATH_SHOWN: usize = 71;

/// Display form of a picked cookies.txt path: the full path when short,
/// else `...` plus the tail so the filename stays visible.
/// Char-based, so multi-byte names never split mid-codepoint.
#[cfg(feature = "innertube")]
pub(crate) fn trim_cookie_path(path: &str, max: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max || max <= 3 {
        return path.to_string();
    }
    format!(
        "...{}",
        chars[chars.len() - (max - 3)..].iter().collect::<String>()
    )
}

/// True when a center column of `center_w` px fits the full-width picker
/// field plus Import beside it. Otherwise the field goes fluid: it
/// shrinks and Import stays visible instead of being pushed out of view.
#[cfg(feature = "innertube")]
pub(crate) fn cookie_row_roomy(center_w: f32) -> bool {
    center_w >= SETTINGS_INPUT_W + COOKIE_IMPORT_ROOM
}

/// Label char budget for the picked path: full budget when the field is
/// full width, fitted to the shrunken field on narrow windows (icon,
/// spacing, and padding discounted, ~7px a char at 12px). The floor
/// keeps short names whole; the ceiling is the full-width budget.
#[cfg(feature = "innertube")]
pub(crate) fn cookie_path_budget(field_w: f32) -> usize {
    if field_w >= SETTINGS_INPUT_W {
        COOKIE_PATH_SHOWN
    } else {
        (((field_w - 42.0) / 7.0) as usize).clamp(24, COOKIE_PATH_SHOWN)
    }
}

/// True when the picked path is a plausible cookies.txt export: a `.txt`
/// file. The picker filters for it; the Import button gates on this, so a
/// stale non-txt pick never offers an import that cannot work.
#[cfg(feature = "innertube")]
pub(crate) fn cookie_pick_valid(path: &str) -> bool {
    let trimmed = path.trim();
    !trimmed.is_empty() && trimmed.to_lowercase().ends_with(".txt")
}

/// Load one scrobble secret, hermetic under test: unit builds read
/// nothing (pre-existing in-memory values stand), so tests never touch
/// the real keychain (`cfg(test)` does not propagate into dependency
/// crates, so core cannot tell a UI test run apart).
pub(crate) fn load_scrobble_secret(account: &str) -> Option<String> {
    #[cfg(test)]
    {
        let _ = account;
        None
    }
    #[cfg(not(test))]
    {
        yutu_core::load_secret(account).ok()
    }
}

/// Save one scrobble secret, hermetic under test (see
/// [`load_scrobble_secret`]): unit builds report success without writing.
pub(crate) fn save_scrobble_secret(account: &str, value: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        let _ = (account, value);
        Ok(())
    }
    #[cfg(not(test))]
    {
        yutu_core::save_secret(account, value)
    }
}
