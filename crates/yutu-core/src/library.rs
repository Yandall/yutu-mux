//! Local library: likes + play history, persisted in SQLite.
//! Schema v1: kv-free, two tables. Rusqlite (system sqlite, no bundled build).

use crate::models::HistoryEntry;
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
                 CREATE TABLE IF NOT EXISTS history(track_id TEXT NOT NULL, played_at_unix INTEGER NOT NULL);",
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
    fn history_order() {
        let s = LibraryStore::in_memory().unwrap();
        s.record_play("m1", 1).unwrap();
        s.record_play("m2", 2).unwrap();
        let h = s.history(10).unwrap();
        assert_eq!(h[0].track_id, "m2");
        assert_eq!(h.len(), 2);
    }
}
