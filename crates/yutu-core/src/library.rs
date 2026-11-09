//! Local library: likes + play history + user playlists, persisted in SQLite.
//! Schema v2: playlists + playlist_tracks added (migrate is additive).
//! Rusqlite (system sqlite, no bundled build).

use crate::models::{HistoryEntry, Playlist, QueueSnapshot, Track};
use rusqlite::{params, Connection};

/// Canonical stored playlist id (edit form, no browse `VL` prefix).
///
/// Library browse returns `VL`-prefixed ids (`VLPL...`, `VLLM`) while the
/// `browse/edit_playlist`, `playlist/create|delete` and `like/*` endpoints
/// want the raw id (`PL...`, `LM`). Stored ids use this form so imports
/// (browse) and creates (edit) never duplicate the same list.
pub fn canonical_playlist_id(id: &str) -> String {
    id.strip_prefix("VL").unwrap_or(id).to_string()
}

/// Browse-endpoint id (`browse` with `browseId`): `VL`-prefixed.
/// Accepts either form, so freshly created (`PL...`) and imported
/// (`VLPL...`) ids both read.
pub fn browse_playlist_id(id: &str) -> String {
    if id.starts_with("VL") {
        id.to_string()
    } else {
        format!("VL{id}")
    }
}

/// Edit-endpoint id (`browse/edit_playlist`, `playlist/delete`, `like/*`
/// playlist target): raw id without the browse `VL` prefix. Accepts
/// either form, so sending a `VL...` browse id to an edit endpoint (HTTP
/// 400 `invalid argument`) cannot recur.
pub fn edit_playlist_id(id: &str) -> String {
    canonical_playlist_id(id)
}

/// One not-yet-confirmed server write, recorded at action time and
/// cleared when its push succeeds. Sync replays leftovers before
/// pulling, so web changes reconcile without dropping local intent.
/// Op names: `like`, `unlike`, `dislike`, `undislike` (track_id),
/// `add`, `remove` (playlist_id + track_id), `create` (playlist_id is
/// the local shell id, title set), `delete` (playlist_id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOp {
    pub op: String,
    pub playlist_id: Option<String>,
    pub track_id: Option<String>,
    pub title: Option<String>,
}

pub struct LibraryStore {
    pub(crate) conn: Connection,
}

impl LibraryStore {
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        // GUI-thread writes (likes, playlist edits) share the file with
        // background import tasks: wait briefly on a locked database
        // instead of failing the write silently.
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    /// Run `f` inside a single write transaction: one journal flush
    /// instead of one per statement. Multi-row playlist writes ran one
    /// transaction per row on the event thread, stalling it long enough
    /// on large lists that the app looked frozen. Rolls back on error.
    fn with_tx(&self, f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;
        match f() {
            Ok(()) => match self.conn.execute_batch("COMMIT") {
                Ok(()) => Ok(()),
                Err(e) => {
                    let _ = self.conn.execute_batch("ROLLBACK");
                    Err(e.to_string())
                }
            },
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    pub(crate) fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS likes(track_id TEXT PRIMARY KEY, liked_at INTEGER);
                 CREATE TABLE IF NOT EXISTS dislikes(track_id TEXT PRIMARY KEY, disliked_at INTEGER);
                 CREATE TABLE IF NOT EXISTS prefs(key TEXT PRIMARY KEY, value TEXT);
                 CREATE TABLE IF NOT EXISTS pending_ops(
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     op TEXT NOT NULL,
                     playlist_id TEXT,
                     track_id TEXT,
                     title TEXT,
                     created_at INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS history(track_id TEXT NOT NULL, played_at_unix INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS playlists(id TEXT PRIMARY KEY, title TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS tracks(
                     track_id TEXT PRIMARY KEY,
                     title TEXT NOT NULL,
                     artist TEXT NOT NULL,
                     album TEXT,
                     duration_secs INTEGER NOT NULL,
                     thumbnail_url TEXT,
                     thumbnail_large_url TEXT,
                     is_video INTEGER NOT NULL DEFAULT 0,
                     updated_at INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS playlist_tracks(
                     playlist_id TEXT NOT NULL,
                     position INTEGER NOT NULL,
                     track_id TEXT NOT NULL,
                     title TEXT NOT NULL,
                     artist TEXT NOT NULL,
                     album TEXT,
                     duration_secs INTEGER NOT NULL,
                     thumbnail_url TEXT,
                     thumbnail_large_url TEXT,
                     is_video INTEGER NOT NULL DEFAULT 0,
                     PRIMARY KEY (playlist_id, position));
                 CREATE INDEX IF NOT EXISTS idx_ptracks_pid ON playlist_tracks(playlist_id);",
            )
            .map_err(|e| e.to_string())?;
        // One-time cleanup: pre-release installs may hold a `downloads`
        // table from the removed offline-download surface.
        let _ = self.conn.execute_batch("DROP TABLE IF EXISTS downloads");
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS queue_items(
                     position INTEGER PRIMARY KEY,
                     track_id TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS queue_meta(
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL);",
            )
            .map_err(|e| e.to_string())?;
        // Additive migration for pre-art databases (no version table).
        for col in ["thumbnail_url", "thumbnail_large_url"] {
            let mut has = false;
            let mut pragma = self
                .conn
                .prepare("PRAGMA table_info(playlist_tracks)")
                .map_err(|e| e.to_string())?;
            let names = pragma
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| e.to_string())?;
            for name in names.flatten() {
                if name == col {
                    has = true;
                }
            }
            if !has {
                self.conn
                    .execute_batch(&format!(
                        "ALTER TABLE playlist_tracks ADD COLUMN {col} TEXT"
                    ))
                    .map_err(|e| e.to_string())?;
            }
        }
        // Song/video flag migration: old rows default to songs.
        for table in ["tracks", "playlist_tracks"] {
            let mut has = false;
            let mut pragma = self
                .conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .map_err(|e| e.to_string())?;
            let names = pragma
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| e.to_string())?;
            for name in names.flatten() {
                if name == "is_video" {
                    has = true;
                }
            }
            if !has {
                self.conn
                    .execute_batch(&format!(
                        "ALTER TABLE {table} ADD COLUMN is_video INTEGER NOT NULL DEFAULT 0"
                    ))
                    .map_err(|e| e.to_string())?;
            }
        }
        // Likes add-order migration: liked_at timestamps new likes; old
        // rows backfill from rowid order so existing installs keep a
        // stable order and new likes append after.
        {
            let mut has = false;
            let mut pragma = self
                .conn
                .prepare("PRAGMA table_info(likes)")
                .map_err(|e| e.to_string())?;
            let names = pragma
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| e.to_string())?;
            for name in names.flatten() {
                if name == "liked_at" {
                    has = true;
                }
            }
            if !has {
                self.conn
                    .execute_batch("ALTER TABLE likes ADD COLUMN liked_at INTEGER")
                    .map_err(|e| e.to_string())?;
            }
            self.conn
                .execute_batch("UPDATE likes SET liked_at = rowid WHERE liked_at IS NULL")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn like(&self, track_id: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT OR IGNORE INTO likes(track_id, liked_at) VALUES (?1, ?2)",
                params![track_id, now],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn unlike(&self, track_id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM likes WHERE track_id = ?1", params![track_id])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn is_liked(&self, track_id: &str) -> Result<bool, String> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM likes WHERE track_id = ?1",
                params![track_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    /// Every liked track id in add order (no catalog probe needed).
    pub fn liked_ids(&self) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id FROM likes ORDER BY liked_at, rowid")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Dislike ("don't recommend"): mirrors the likes table so the state
    /// survives restarts. Like and dislike are mutually exclusive
    /// server-side; callers clear the other side when setting one.
    pub fn dislike(&self, track_id: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT OR IGNORE INTO dislikes(track_id, disliked_at) VALUES (?1, ?2)",
                params![track_id, now],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn undislike(&self, track_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM dislikes WHERE track_id = ?1",
                params![track_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn is_disliked(&self, track_id: &str) -> Result<bool, String> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM dislikes WHERE track_id = ?1",
                params![track_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    /// Every disliked track id in add order (no catalog probe needed).
    pub fn disliked_ids(&self) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id FROM dislikes ORDER BY disliked_at, rowid")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Replace the whole liked set with the server set (sync pull):
    /// rows the server no longer likes disappear, new ones append.
    /// Runs in one transaction.
    pub fn sync_likes(&self, liked_ids: &[String]) -> Result<(), String> {
        self.with_tx(|| self.sync_likes_in_tx(liked_ids))
    }

    /// Replace the whole disliked set with the server set (sync pull).
    /// Same single-transaction contract as [`Self::sync_likes`].
    pub fn sync_dislikes(&self, disliked_ids: &[String]) -> Result<(), String> {
        self.with_tx(|| self.sync_dislikes_in_tx(disliked_ids))
    }

    /// Small string preference (sync settings, last-sync time).
    pub fn get_pref(&self, key: &str) -> Result<Option<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM prefs WHERE key = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query_map(params![key], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        match rows.next() {
            None => Ok(None),
            Some(r) => r.map(Some).map_err(|e| e.to_string()),
        }
    }

    pub fn set_pref(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO prefs(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Record a pending server write (see [`PendingOp`]). Latest intent
    /// wins: recording a track rate drops older rate rows for that
    /// track, recording add/remove drops the older row for that pair,
    /// and recording a delete drops add/remove rows for that playlist
    /// (the list is gone locally either way).
    pub fn record_pending(
        &self,
        op: &str,
        playlist_id: Option<&str>,
        track_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.with_tx(|| {
            match op {
                "like" | "unlike" | "dislike" | "undislike" => {
                    self.conn
                        .execute(
                            "DELETE FROM pending_ops WHERE op IN ('like','unlike','dislike','undislike')
                             AND IFNULL(track_id,'') = IFNULL(?1,'')",
                            params![track_id],
                        )
                        .map_err(|e| e.to_string())?;
                }
                "add" | "remove" => {
                    self.conn
                        .execute(
                            "DELETE FROM pending_ops WHERE op IN ('add','remove')
                             AND IFNULL(playlist_id,'') = IFNULL(?1,'')
                             AND IFNULL(track_id,'') = IFNULL(?2,'')",
                            params![playlist_id, track_id],
                        )
                        .map_err(|e| e.to_string())?;
                }
                "create" => {
                    self.conn
                        .execute(
                            "DELETE FROM pending_ops WHERE op = 'create'
                             AND IFNULL(playlist_id,'') = IFNULL(?1,'')",
                            params![playlist_id],
                        )
                        .map_err(|e| e.to_string())?;
                }
                "delete" => {
                    self.conn
                        .execute(
                            "DELETE FROM pending_ops WHERE op IN ('add','remove','delete')
                             AND IFNULL(playlist_id,'') = IFNULL(?1,'')",
                            params![playlist_id],
                        )
                        .map_err(|e| e.to_string())?;
                }
                _ => {}
            }
            self.conn
                .execute(
                    "INSERT INTO pending_ops(op, playlist_id, track_id, title, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![op, playlist_id, track_id, title, now],
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    /// Drop the pending row for one confirmed push (exact-op match, so a
    /// stale in-flight task cannot clear a newer intent).
    pub fn clear_pending(
        &self,
        op: &str,
        playlist_id: Option<&str>,
        track_id: Option<&str>,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM pending_ops WHERE op = ?1
                 AND IFNULL(playlist_id,'') = IFNULL(?2,'')
                 AND IFNULL(track_id,'') = IFNULL(?3,'')",
                params![op, playlist_id, track_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Every pending write in record order.
    pub fn pending_ops(&self) -> Result<Vec<PendingOp>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT op, playlist_id, track_id, title FROM pending_ops ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PendingOp {
                    op: r.get(0)?,
                    playlist_id: r.get(1).unwrap_or(None),
                    track_id: r.get(2).unwrap_or(None),
                    title: r.get(3).unwrap_or(None),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Move pending rows from a local shell id to its server id (after a
    /// playlist create migrates the shell).
    pub fn repoint_pending(
        &self,
        old_playlist_id: &str,
        new_playlist_id: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE pending_ops SET playlist_id = ?1 WHERE IFNULL(playlist_id,'') = ?2",
                params![new_playlist_id, old_playlist_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// True when a playlist has pending add/remove rows (sync skips
    /// replacing its contents until the pushes land).
    pub fn has_playlist_pending(&self, playlist_id: &str) -> Result<bool, String> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pending_ops WHERE op IN ('add','remove')
                 AND IFNULL(playlist_id,'') = ?1",
                params![playlist_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    /// Apply one sync pull to the stored sets (single transaction):
    /// replace likes/dislikes when `apply_ratings`, re-apply still
    /// pending rate rows so unpushed intent survives the replace, and
    /// delete `prune_ids` playlists outright. Callers compute
    /// `prune_ids` (server lists absent from clean headers); local
    /// `u{seq}` shells are never pruned here.
    pub fn apply_sync_pull(
        &self,
        liked_ids: &[String],
        disliked_ids: &[String],
        apply_ratings: bool,
        prune_ids: &[String],
    ) -> Result<(), String> {
        self.with_tx(|| {
            if apply_ratings {
                self.sync_likes_in_tx(liked_ids)?;
                self.sync_dislikes_in_tx(disliked_ids)?;
                // Still-pending rates ride over the server sets.
                let pendings = self.pending_ops_in_tx()?;
                for p in &pendings {
                    match p.op.as_str() {
                        "like" => {
                            if let Some(id) = p.track_id.as_deref() {
                                self.like_in_tx(id)?;
                            }
                        }
                        "unlike" => {
                            if let Some(id) = p.track_id.as_deref() {
                                self.unlike_in_tx(id)?;
                            }
                        }
                        "dislike" => {
                            if let Some(id) = p.track_id.as_deref() {
                                self.dislike_in_tx(id)?;
                            }
                        }
                        "undislike" => {
                            if let Some(id) = p.track_id.as_deref() {
                                self.undislike_in_tx(id)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            for id in prune_ids {
                self.delete_playlist_in_tx(id)?;
            }
            Ok(())
        })
    }

    /// Transaction-free variants for use inside [`Self::apply_sync_pull`]
    /// (a nested `BEGIN` would fail).
    fn pending_ops_in_tx(&self) -> Result<Vec<PendingOp>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT op, playlist_id, track_id, title FROM pending_ops ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PendingOp {
                    op: r.get(0)?,
                    playlist_id: r.get(1).unwrap_or(None),
                    track_id: r.get(2).unwrap_or(None),
                    title: r.get(3).unwrap_or(None),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    fn like_in_tx(&self, track_id: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT OR IGNORE INTO likes(track_id, liked_at) VALUES (?1, ?2)",
                params![track_id, now],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn unlike_in_tx(&self, track_id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM likes WHERE track_id = ?1", params![track_id])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn dislike_in_tx(&self, track_id: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT OR IGNORE INTO dislikes(track_id, disliked_at) VALUES (?1, ?2)",
                params![track_id, now],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn undislike_in_tx(&self, track_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM dislikes WHERE track_id = ?1",
                params![track_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn sync_likes_in_tx(&self, liked_ids: &[String]) -> Result<(), String> {
        if liked_ids.is_empty() {
            self.conn
                .execute("DELETE FROM likes", [])
                .map_err(|e| e.to_string())?;
        } else {
            let placeholders = liked_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            self.conn
                .execute(
                    &format!("DELETE FROM likes WHERE track_id NOT IN ({placeholders})"),
                    rusqlite::params_from_iter(liked_ids.iter()),
                )
                .map_err(|e| e.to_string())?;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        for id in liked_ids {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO likes(track_id, liked_at) VALUES (?1, ?2)",
                    params![id, now],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn sync_dislikes_in_tx(&self, disliked_ids: &[String]) -> Result<(), String> {
        if disliked_ids.is_empty() {
            self.conn
                .execute("DELETE FROM dislikes", [])
                .map_err(|e| e.to_string())?;
        } else {
            let placeholders = disliked_ids
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            self.conn
                .execute(
                    &format!("DELETE FROM dislikes WHERE track_id NOT IN ({placeholders})"),
                    rusqlite::params_from_iter(disliked_ids.iter()),
                )
                .map_err(|e| e.to_string())?;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        for id in disliked_ids {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO dislikes(track_id, disliked_at) VALUES (?1, ?2)",
                    params![id, now],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn delete_playlist_in_tx(&self, id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
                params![id],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM playlists WHERE id = ?1", params![id])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn record_play(&self, track_id: &str, played_at_unix: i64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO history(track_id, played_at_unix) VALUES (?1, ?2)",
                params![track_id, played_at_unix],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn history(&self, limit: usize) -> Result<Vec<HistoryEntry>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id, played_at_unix FROM history ORDER BY played_at_unix DESC LIMIT ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(HistoryEntry {
                    track_id: r.get(0)?,
                    played_at_unix: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn create_playlist(&self, id: &str, title: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO playlists(id, title) VALUES (?1, ?2)",
                params![id, title],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Mirror a server playlist exactly: upsert the title (re-imports
    /// refresh renames instead of keeping the stale one) and replace the
    /// track rows, so server-side removals disappear locally too. Order
    /// follows `tracks`.
    pub fn sync_server_playlist(
        &self,
        id: &str,
        title: &str,
        tracks: &[Track],
    ) -> Result<(), String> {
        self.with_tx(|| {
            self.conn
                .execute(
                    "INSERT INTO playlists(id, title) VALUES (?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET title = excluded.title",
                    params![id, title],
                )
                .map_err(|e| e.to_string())?;
            self.conn
                .execute(
                    "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
                    params![id],
                )
                .map_err(|e| e.to_string())?;
            for (i, t) in tracks.iter().enumerate() {
                self.conn
                    .execute(
                        "INSERT OR IGNORE INTO playlist_tracks
                         (playlist_id, position, track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url, is_video)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            id,
                            i as i64,
                            t.id,
                            t.title,
                            t.artist,
                            t.album,
                            t.duration_secs as i64,
                            t.thumbnail_url,
                            t.thumbnail_large_url,
                            i64::from(t.is_video)
                        ],
                    )
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }

    pub fn delete_playlist(&self, id: &str) -> Result<(), String> {
        self.with_tx(|| {
            self.conn
                .execute(
                    "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
                    params![id],
                )
                .map_err(|e| e.to_string())?;
            self.conn
                .execute("DELETE FROM playlists WHERE id = ?1", params![id])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    /// Persist the playback session: queue ids in order plus the current
    /// index, position, shuffle/repeat/volume. One shot replaces the
    /// previous snapshot (ids only: rows resolve via the track cache).
    pub fn save_queue(&self, snap: &QueueSnapshot) -> Result<(), String> {
        self.with_tx(|| {
            self.conn
                .execute("DELETE FROM queue_items", [])
                .map_err(|e| e.to_string())?;
            for (i, id) in snap.ids.iter().enumerate() {
                self.conn
                    .execute(
                        "INSERT INTO queue_items(position, track_id) VALUES (?1, ?2)",
                        params![i as i64, id],
                    )
                    .map_err(|e| e.to_string())?;
            }
            let meta = [
                (
                    "index",
                    snap.index.map(|i| i.to_string()).unwrap_or_default(),
                ),
                ("position_secs", snap.position_secs.to_string()),
                ("shuffle", (snap.shuffle as u8).to_string()),
                ("repeat", snap.repeat.clone()),
                ("volume", snap.volume.to_string()),
                ("muted", (snap.muted as u8).to_string()),
            ];
            for (k, v) in meta {
                self.conn
                    .execute(
                        "INSERT INTO queue_meta(key, value) VALUES (?1, ?2)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                        params![k, v],
                    )
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }

    /// Load the persisted session (`None` when nothing was ever saved).
    /// Parsing is lenient: a bad value falls back, never errors.
    pub fn load_queue(&self) -> Result<Option<QueueSnapshot>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id FROM queue_items ORDER BY position")
            .map_err(|e| e.to_string())?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        if ids.is_empty() {
            return Ok(None);
        }
        let meta = |key: &str| -> String {
            self.conn
                .query_row(
                    "SELECT value FROM queue_meta WHERE key = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .unwrap_or_default()
        };
        Ok(Some(QueueSnapshot {
            ids,
            index: meta("index").parse().ok(),
            position_secs: {
                let pos: f64 = meta("position_secs").parse().unwrap_or(0.0);
                if pos.is_finite() {
                    pos.max(0.0)
                } else {
                    0.0
                }
            },
            shuffle: meta("shuffle") == "1",
            repeat: {
                let r = meta("repeat");
                if ["off", "all", "one"].contains(&r.as_str()) {
                    r
                } else {
                    "off".into()
                }
            },
            volume: meta("volume").parse::<f32>().unwrap_or(0.8).clamp(0.0, 1.0),
            // Missing on pre-mute snapshots: restarts unmuted.
            muted: meta("muted") == "1",
        }))
    }

    /// Drop the persisted session (empty queue stays empty on restart).
    pub fn clear_queue(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM queue_items", [])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM queue_meta", [])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn user_playlists(&self) -> Result<Vec<Playlist>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title FROM playlists ORDER BY rowid")
            .map_err(|e| e.to_string())?;
        let metas = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        metas
            .into_iter()
            .map(|(id, title)| {
                Ok(Playlist {
                    id: id.clone(),
                    title,
                    tracks: self.playlist_tracks(&id)?,
                })
            })
            .collect()
    }

    fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url, is_video
                 FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![playlist_id], |r| {
                Ok(Track {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    duration_secs: r.get::<_, i64>(4)? as u32,
                    thumbnail_url: r.get(5).unwrap_or(None),
                    thumbnail_large_url: r.get(6).unwrap_or(None),
                    // Play counts are search-only metadata, never persisted.
                    plays: None,
                    is_video: r.get::<_, i64>(7).unwrap_or(0) != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn add_to_playlist(&self, playlist_id: &str, track: &Track) -> Result<(), String> {
        self.with_tx(|| {
            let existing: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                    params![playlist_id, track.id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if existing > 0 {
                return Ok(());
            }
            let pos: i64 = self
                .conn
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
                    params![playlist_id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO playlist_tracks
                     (playlist_id, position, track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url, is_video)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        playlist_id,
                        pos,
                        track.id,
                        track.title,
                        track.artist,
                        track.album,
                        track.duration_secs as i64,
                        track.thumbnail_url,
                        track.thumbnail_large_url,
                        i64::from(track.is_video)
                    ],
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    pub fn remove_from_playlist(&self, playlist_id: &str, track_id: &str) -> Result<(), String> {
        self.with_tx(|| {
            // Lowest position of a removed row, if any: rows after it
            // shift down by one in a single UPDATE instead of one per
            // track (the old per-row loop stalled the event thread on
            // large playlists, freezing the app).
            let pos: Option<i64> = self
                .conn
                .query_row(
                    "SELECT MIN(position) FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                    params![playlist_id, track_id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            self.conn
                .execute(
                    "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                    params![playlist_id, track_id],
                )
                .map_err(|e| e.to_string())?;
            if let Some(p) = pos {
                self.conn
                    .execute(
                        "UPDATE playlist_tracks SET position = position - 1 WHERE playlist_id = ?1 AND position > ?2",
                        params![playlist_id, p],
                    )
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }

    /// Upsert one track into the metadata cache, preserving existing art
    /// when the incoming row has none. Called whenever the app sees a
    /// full Track (search, radio, import, hydration, like, play) so a
    /// restart can rebuild the catalog without a network round trip.
    pub fn upsert_track(&self, track: &Track) -> Result<(), String> {
        self.upsert_tracks(std::slice::from_ref(track))
    }

    /// Batch version of [`Self::upsert_track`]: merges each incoming row
    /// with its cached row (incoming wins, but `None` art never clobbers
    /// cached art, and empty titles never clobber cached titles; the
    /// video flag is sticky once true, so a default-false source never
    /// clears a known video).
    pub fn upsert_tracks(&self, tracks: &[Track]) -> Result<(), String> {
        self.with_tx(|| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            for t in tracks {
            let cached = self.cached_track(&t.id).unwrap_or(None);
            let (title, artist, album, duration_secs, thumb, large, is_video) = match cached {
                Some(c) => {
                    let title = if t.title.is_empty() {
                        c.title
                    } else {
                        t.title.clone()
                    };
                    let artist = if t.artist.is_empty() {
                        c.artist
                    } else {
                        t.artist.clone()
                    };
                    let album = t.album.clone().or(c.album);
                    let duration_secs = if t.duration_secs == 0 {
                        c.duration_secs
                    } else {
                        t.duration_secs
                    };
                    let thumb = t.thumbnail_url.clone().or(c.thumbnail_url);
                    let large = t.thumbnail_large_url.clone().or(c.thumbnail_large_url);
                    let is_video = t.is_video || c.is_video;
                    (title, artist, album, duration_secs, thumb, large, is_video)
                }
                None => (
                    t.title.clone(),
                    t.artist.clone(),
                    t.album.clone(),
                    t.duration_secs,
                    t.thumbnail_url.clone(),
                    t.thumbnail_large_url.clone(),
                    t.is_video,
                ),
            };
            // Skip bare-id rows with no usable metadata: they would poison
            // the cache and hide later hydration (empty title + no art).
            if title.is_empty() && thumb.is_none() && large.is_none() {
                continue;
            }
            self.conn
                .execute(
                    "INSERT INTO tracks
                     (track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url, is_video, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(track_id) DO UPDATE SET
                       title = excluded.title,
                       artist = excluded.artist,
                       album = excluded.album,
                       duration_secs = excluded.duration_secs,
                       thumbnail_url = excluded.thumbnail_url,
                       thumbnail_large_url = excluded.thumbnail_large_url,
                       is_video = excluded.is_video,
                       updated_at = excluded.updated_at",
                    params![
                        t.id,
                        title,
                        artist,
                        album,
                        duration_secs as i64,
                        thumb,
                        large,
                        i64::from(is_video),
                        now
                    ],
                )
                .map_err(|e| e.to_string())?;
        }
            // Heal playlist rows that predate the art columns (or were stored
            // before art arrived): fill missing thumbs from the fresh rows so
            // old installs regain covers without re-adding songs.
            self.heal_playlist_art(tracks)?;
            Ok(())
        })
    }

    /// Like plus metadata cache in one call: the liked row stays an id,
    /// but the track cache keeps the title/art so Favorites survives a
    /// restart even before any search runs.
    pub fn like_track(&self, track: &Track) -> Result<(), String> {
        self.upsert_track(track)?;
        self.like(&track.id)
    }

    pub fn cached_track(&self, track_id: &str) -> Result<Option<Track>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url, is_video
                 FROM tracks WHERE track_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query_map(params![track_id], |r| {
                Ok(Track {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    duration_secs: r.get::<_, i64>(4)? as u32,
                    thumbnail_url: r.get(5).unwrap_or(None),
                    thumbnail_large_url: r.get(6).unwrap_or(None),
                    // Play counts are search-only metadata, never persisted.
                    plays: None,
                    is_video: r.get::<_, i64>(7).unwrap_or(0) != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        match rows.next() {
            None => Ok(None),
            Some(r) => r.map(Some).map_err(|e| e.to_string()),
        }
    }

    /// Cached rows for `ids` in request order (unknown ids skipped).
    pub fn cached_tracks(&self, ids: &[String]) -> Result<Vec<Track>, String> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(t) = self.cached_track(id)? {
                out.push(t);
            }
        }
        Ok(out)
    }

    /// Every cached track (catalog seed at startup).
    pub fn all_cached_tracks(&self) -> Result<Vec<Track>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url, is_video
                 FROM tracks",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Track {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    duration_secs: r.get::<_, i64>(4)? as u32,
                    thumbnail_url: r.get(5).unwrap_or(None),
                    thumbnail_large_url: r.get(6).unwrap_or(None),
                    // Play counts are search-only metadata, never persisted.
                    plays: None,
                    is_video: r.get::<_, i64>(7).unwrap_or(0) != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Fill `NULL` art columns on stored playlist rows from fresh tracks.
    /// Only touches rows that lack art the fresh row provides; titles and
    /// durations are left alone (playlist rows are already authoritative).
    fn heal_playlist_art(&self, tracks: &[Track]) -> Result<(), String> {
        for t in tracks {
            if t.thumbnail_url.is_none() && t.thumbnail_large_url.is_none() {
                continue;
            }
            if t.thumbnail_url.is_some() && t.thumbnail_large_url.is_some() {
                self.conn
                    .execute(
                        "UPDATE playlist_tracks SET thumbnail_url = COALESCE(thumbnail_url, ?1),
                         thumbnail_large_url = COALESCE(thumbnail_large_url, ?2)
                         WHERE track_id = ?3
                         AND (thumbnail_url IS NULL OR thumbnail_large_url IS NULL)",
                        params![t.thumbnail_url, t.thumbnail_large_url, t.id],
                    )
                    .map_err(|e| e.to_string())?;
            } else if let Some(u) = t.thumbnail_url.as_deref() {
                self.conn
                    .execute(
                        "UPDATE playlist_tracks SET thumbnail_url = COALESCE(thumbnail_url, ?1)
                         WHERE track_id = ?2 AND thumbnail_url IS NULL",
                        params![u, t.id],
                    )
                    .map_err(|e| e.to_string())?;
            } else if let Some(u) = t.thumbnail_large_url.as_deref() {
                self.conn
                    .execute(
                        "UPDATE playlist_tracks SET thumbnail_large_url = COALESCE(thumbnail_large_url, ?1)
                         WHERE track_id = ?2 AND thumbnail_large_url IS NULL",
                        params![u, t.id],
                    )
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
