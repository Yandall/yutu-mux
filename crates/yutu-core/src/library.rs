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
                "CREATE TABLE IF NOT EXISTS likes(track_id TEXT PRIMARY KEY);
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
                     PRIMARY KEY (playlist_id, position));
                 CREATE INDEX IF NOT EXISTS idx_ptracks_pid ON playlist_tracks(playlist_id);",
            )
            .map_err(|e| e.to_string())
    }

    pub fn like(&self, track_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO likes(track_id) VALUES (?1)",
                params![track_id],
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

    /// Every liked track id (no catalog probe needed — no mock data).
    pub fn liked_ids(&self) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id FROM likes ORDER BY track_id")
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
                "SELECT track_id, title, artist, album, duration_secs
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
                 (playlist_id, position, track_id, title, artist, album, duration_secs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    playlist_id,
                    pos,
                    track.id,
                    track.title,
                    track.artist,
                    track.album,
                    track.duration_secs as i64
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
        assert_eq!(s.liked_ids().unwrap(), vec!["vid-a", "vid-b"]);
        s.unlike("vid-a").unwrap();
        assert_eq!(s.liked_ids().unwrap(), vec!["vid-b"]);
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
}
