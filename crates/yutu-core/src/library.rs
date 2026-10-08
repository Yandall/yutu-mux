//! Local library: likes + play history + user playlists, persisted in SQLite.
//! Schema v2: playlists + playlist_tracks added (migrate is additive).
//! Rusqlite (system sqlite, no bundled build).

use crate::models::{HistoryEntry, Playlist, Track};
use rusqlite::{params, Connection};

pub struct LibraryStore {
    conn: Connection,
}

impl LibraryStore {
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        let s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS likes(track_id TEXT PRIMARY KEY, liked_at INTEGER);
                 CREATE TABLE IF NOT EXISTS history(track_id TEXT NOT NULL, played_at_unix INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS playlists(id TEXT PRIMARY KEY, title TEXT NOT NULL);
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
                     PRIMARY KEY (playlist_id, position));
                 CREATE INDEX IF NOT EXISTS idx_ptracks_pid ON playlist_tracks(playlist_id);",
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

    pub fn delete_playlist(&self, id: &str) -> Result<(), String> {
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
                "SELECT track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url
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
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn add_to_playlist(&self, playlist_id: &str, track: &Track) -> Result<(), String> {
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
                 (playlist_id, position, track_id, title, artist, album, duration_secs, thumbnail_url, thumbnail_large_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    playlist_id,
                    pos,
                    track.id,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_secs as i64,
                    track.thumbnail_url,
                    track.thumbnail_large_url
                ],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn remove_from_playlist(&self, playlist_id: &str, track_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                params![playlist_id, track_id],
            )
            .map_err(|e| e.to_string())?;
        // Renumber positions to keep the (playlist_id, position) PK dense.
        let ids: Vec<String> = self
            .playlist_tracks(playlist_id)?
            .into_iter()
            .map(|t| t.id)
            .collect();
        for (i, tid) in ids.iter().enumerate() {
            self.conn
                .execute(
                    "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
                    params![i as i64, playlist_id, tid],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_round_trip() {
        let s = LibraryStore::in_memory().unwrap();
        assert!(!s.is_liked("m1").unwrap());
        s.like("m1").unwrap();
        assert!(s.is_liked("m1").unwrap());
        s.unlike("m1").unwrap();
        assert!(!s.is_liked("m1").unwrap());
    }

    #[test]
    fn liked_ids_lists_without_catalog_probe() {
        let s = LibraryStore::in_memory().unwrap();
        assert!(s.liked_ids().unwrap().is_empty());
        s.like("vid-b").unwrap();
        s.like("vid-a").unwrap();
        s.like("vid-a").unwrap();
        // Add order, not alphabetical: b first, re-like is a no-op.
        assert_eq!(s.liked_ids().unwrap(), vec!["vid-b", "vid-a"]);
        s.unlike("vid-a").unwrap();
        assert_eq!(s.liked_ids().unwrap(), vec!["vid-b"]);
    }

    #[test]
    fn liked_ids_preserves_insertion_order() {
        let s = LibraryStore::in_memory().unwrap();
        s.like("b").unwrap();
        s.like("a").unwrap();
        s.like("c").unwrap();
        assert_eq!(s.liked_ids().unwrap(), vec!["b", "a", "c"]);
        s.unlike("a").unwrap();
        s.like("a").unwrap();
        assert_eq!(s.liked_ids().unwrap(), vec!["b", "c", "a"]);
    }

    #[test]
    fn history_order() {
        let s = LibraryStore::in_memory().unwrap();
        s.record_play("m1", 1).unwrap();
        s.record_play("m2", 2).unwrap();
        let h = s.history(10).unwrap();
        assert_eq!(h[0].track_id, "m2");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn playlists_crud_persists() {
        use crate::models::Track;
        let s = LibraryStore::in_memory().unwrap();
        s.create_playlist("u1", "Gym").unwrap();
        let t = Track {
            id: "m1".into(),
            title: "Come Together".into(),
            artist: "The Beatles".into(),
            album: Some("Abbey Road".into()),
            duration_secs: 259,
            thumbnail_url: None,
            thumbnail_large_url: None,
        };
        s.add_to_playlist("u1", &t).unwrap();
        s.add_to_playlist("u1", &t).unwrap(); // deduped
        let lists = s.user_playlists().unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].tracks.len(), 1);
        assert_eq!(lists[0].tracks[0].album.as_deref(), Some("Abbey Road"));
        s.remove_from_playlist("u1", "m1").unwrap();
        assert!(s.user_playlists().unwrap()[0].tracks.is_empty());
        s.delete_playlist("u1").unwrap();
        assert!(s.user_playlists().unwrap().is_empty());
    }

    #[test]
    fn playlist_art_round_trips() {
        use crate::models::Track;
        let s = LibraryStore::in_memory().unwrap();
        s.create_playlist("u1", "Gym").unwrap();
        let t = Track {
            id: "m1".into(),
            title: "Come Together".into(),
            artist: "The Beatles".into(),
            album: Some("Abbey Road".into()),
            duration_secs: 259,
            thumbnail_url: Some("https://example.invalid/s".into()),
            thumbnail_large_url: Some("https://example.invalid/l".into()),
        };
        s.add_to_playlist("u1", &t).unwrap();
        let lists = s.user_playlists().unwrap();
        assert_eq!(
            lists[0].tracks[0].thumbnail_url.as_deref(),
            Some("https://example.invalid/s")
        );
        assert_eq!(
            lists[0].tracks[0].thumbnail_large_url.as_deref(),
            Some("https://example.invalid/l")
        );
    }

    #[test]
    fn migration_adds_art_columns_to_old_db() {
        let s = LibraryStore::in_memory().unwrap();
        s.create_playlist("u1", "Gym").unwrap();
        s.conn
            .execute(
                "INSERT INTO playlist_tracks
                 (playlist_id, position, track_id, title, artist, album, duration_secs)
                 VALUES ('u1', 0, 'm1', 'Come Together', 'The Beatles', 'Abbey Road', 259)",
                [],
            )
            .unwrap();
        // Simulate a pre-art database: drop the new columns, remigrate.
        s.conn
            .execute_batch(
                "CREATE TEMP TABLE backup AS SELECT playlist_id, position, track_id, title, artist, album, duration_secs FROM playlist_tracks;
                 DROP TABLE playlist_tracks;
                 CREATE TABLE playlist_tracks(
                     playlist_id TEXT NOT NULL,
                     position INTEGER NOT NULL,
                     track_id TEXT NOT NULL,
                     title TEXT NOT NULL,
                     artist TEXT NOT NULL,
                     album TEXT,
                     duration_secs INTEGER NOT NULL,
                     PRIMARY KEY (playlist_id, position));
                 INSERT INTO playlist_tracks SELECT * FROM backup;
                 DROP TABLE backup;",
            )
            .unwrap();
        s.migrate().unwrap();
        // Old rows survive with empty art; new writes carry art.
        let lists = s.user_playlists().unwrap();
        assert_eq!(lists[0].tracks.len(), 1);
        assert_eq!(lists[0].tracks[0].id, "m1");
        assert!(lists[0].tracks[0].thumbnail_url.is_none());
    }
}
