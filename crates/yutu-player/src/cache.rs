//! Offline smart-download tracker + disk-backed cache.
//!
//! In-memory bookkeeping: which tracks are cached, how many bytes
//! they use, and LRU eviction when the capacity cap is exceeded.
//!
//! Disk layer (optional, enabled via [`OfflineCache::set_dir`]):
//! [`OfflineCache::store_bytes`] / [`OfflineCache::read_bytes`] persist
//! track bytes as `{sanitized}.m4a` files inside the cache dir, where
//! `sanitized` keeps ASCII alphanumerics and maps everything else to `_`
//! (empty input becomes `"unknown"`).

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One cached track: identity + where it lives on disk + size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub track_id: String,
    pub path: String,
    pub bytes: u64,
    pub downloaded_at_unix: u64,
}

impl CacheEntry {
    pub fn new(
        track_id: impl Into<String>,
        path: impl Into<String>,
        bytes: u64,
        downloaded_at_unix: u64,
    ) -> Self {
        Self {
            track_id: track_id.into(),
            path: path.into(),
            bytes,
            downloaded_at_unix,
        }
    }
}

/// In-memory LRU cache with a byte-capacity cap.
///
/// `order` holds track ids front (oldest / evict first) to back (newest).
/// [`OfflineCache::get`] promotes the entry to most-recently-used.
#[derive(Debug, Default)]
pub struct OfflineCache {
    capacity_bytes: u64,
    entries: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
    dir: Option<String>,
}

impl OfflineCache {
    /// Create an empty cache capped at `capacity_bytes` total bytes.
    #[must_use]
    pub fn new(capacity_bytes: u64) -> Self {
        Self {
            capacity_bytes,
            entries: HashMap::new(),
            order: VecDeque::new(),
            dir: None,
        }
    }

    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.capacity_bytes
    }

    /// Change the cap, evicting oldest entries if now over capacity.
    /// Returns the evicted entries (oldest first).
    ///
    /// NOTE: this is pure bookkeeping and does NOT delete any files from
    /// disk. Evicted entries' files (if any) are left in place. Use
    /// [`OfflineCache::set_capacity_on_disk`] or
    /// [`OfflineCache::evict_to_disk`] when a cache dir is set and file
    /// deletion is desired.
    pub fn set_capacity(&mut self, capacity_bytes: u64) -> Vec<CacheEntry> {
        self.capacity_bytes = capacity_bytes;
        self.evict_while_over_cap()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total bytes currently held.
    #[must_use]
    pub fn usage(&self) -> u64 {
        self.entries.values().map(|e| e.bytes).sum()
    }

    #[must_use]
    pub fn contains(&self, track_id: &str) -> bool {
        self.entries.contains_key(track_id)
    }

    /// Immutable peek (does NOT affect LRU order).
    #[must_use]
    pub fn peek(&self, track_id: &str) -> Option<&CacheEntry> {
        self.entries.get(track_id)
    }

    /// LRU `get`: returns the entry and promotes it to most-recently-used.
    pub fn get(&mut self, track_id: &str) -> Option<&CacheEntry> {
        if !self.entries.contains_key(track_id) {
            return None;
        }
        self.promote(track_id);
        self.entries.get(track_id)
    }

    /// Insert (or replace) an entry. Evicts oldest entries until
    /// [`OfflineCache::usage`] is under the cap. Returns evicted entries
    /// (oldest first).
    ///
    /// If the new entry alone exceeds the cap, it is still kept (the
    /// cache will hold just that one entry and report over-cap usage);
    /// callers that want strict rejection should check sizes first.
    ///
    /// NOTE: this is pure bookkeeping and does NOT delete any files from
    /// disk. Evicted entries' files (if any) are left in place and would
    /// leak unless the caller deletes them. Use
    /// [`OfflineCache::store_bytes`] (which deletes evicted files
    /// best-effort via [`OfflineCache::evict_to_disk`]) for the
    /// disk-backed path.
    pub fn insert(&mut self, entry: CacheEntry) -> Vec<CacheEntry> {
        let id = entry.track_id.clone();
        if self.entries.contains_key(&id) {
            self.remove_from_order(&id);
        }
        self.order.push_back(id.clone());
        self.entries.insert(id, entry);
        self.evict_while_over_cap()
    }

    /// Remove an entry by track id.
    ///
    /// NOTE: bookkeeping only; does NOT delete the entry's file from disk.
    /// Use [`OfflineCache::remove_file`] to also delete the file.
    pub fn remove(&mut self, track_id: &str) -> Option<CacheEntry> {
        let removed = self.entries.remove(track_id)?;
        self.remove_from_order(track_id);
        Some(removed)
    }

    /// Remove everything.
    ///
    /// NOTE: bookkeeping only; does NOT delete any files from disk.
    /// Use [`OfflineCache::clear_disk`] to also delete `*.m4a` files.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Track ids from oldest to newest (useful for UI / tests).
    #[must_use]
    pub fn order_oldest_first(&self) -> Vec<String> {
        self.order.iter().cloned().collect()
    }

    // ------------------------------------------------------------------
    // Disk layer
    // ------------------------------------------------------------------

    /// Set the cache directory for disk-backed operations.
    pub fn set_dir(&mut self, dir: impl Into<String>) {
        self.dir = Some(dir.into());
    }

    /// The configured cache directory, if any.
    #[must_use]
    pub fn dir(&self) -> Option<&str> {
        self.dir.as_deref()
    }

    /// Filesystem path for `track_id` inside the cache dir, if a dir is set.
    ///
    /// The filename is `{sanitized}.m4a` where `sanitized` keeps ASCII
    /// alphanumerics and maps every other char to `_`; empty input (or an
    /// empty sanitized result, which only happens for empty input) becomes
    /// `"unknown"`.
    #[must_use]
    pub fn file_path_for(&self, track_id: &str) -> Option<String> {
        let dir = self.dir.as_deref()?;
        let name = sanitize_track_id(track_id);
        let path: PathBuf = Path::new(dir).join(format!("{name}.m4a"));
        Some(path.to_string_lossy().into_owned())
    }

    /// Store `bytes` for `track_id` on disk and in the tracker.
    ///
    /// Creates the cache dir if needed, writes the file, inserts/refreshes
    /// the entry (`downloaded_at_unix` = now), then evicts over-cap entries
    /// and deletes their files best-effort (individual delete errors are
    /// ignored). Returns the stored entry.
    ///
    /// Returns `Err` if no cache dir is set (via [`OfflineCache::set_dir`])
    /// or if dir creation / file write fails.
    pub fn store_bytes(&mut self, track_id: &str, bytes: &[u8]) -> io::Result<CacheEntry> {
        let dir = self
            .dir
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cache dir not set"))?;
        fs::create_dir_all(&dir)?;
        let path = Path::new(&dir)
            .join(format!("{}.m4a", sanitize_track_id(track_id)))
            .to_string_lossy()
            .into_owned();
        fs::write(&path, bytes)?;
        let entry = CacheEntry {
            track_id: track_id.to_string(),
            path,
            bytes: bytes.len() as u64,
            downloaded_at_unix: now_unix(),
        };
        // Insert without evicting first so `evict_to_disk` handles both
        // eviction and file deletion in one place.
        let id = entry.track_id.clone();
        if self.entries.contains_key(&id) {
            self.remove_from_order(&id);
        }
        self.order.push_back(id.clone());
        self.entries.insert(id.clone(), entry);
        let _evicted = self.evict_to_disk();
        // The just-stored entry is newest so `evict_while_over_cap` (which
        // always keeps at least one entry) never evicts it.
        self.entries
            .get(&id)
            .cloned()
            .ok_or_else(|| io::Error::other("stored entry was evicted"))
    }

    /// Read cached bytes for `track_id`, promoting it to most-recently-used
    /// via [`OfflineCache::get`].
    ///
    /// Returns a `NotFound` io error if no dir is set, if there is no entry
    /// for `track_id`, or if the file is missing/unreadable as not-found.
    /// Self-healing: if the entry exists but its file is gone, the entry is
    /// dropped and `NotFound` is returned.
    pub fn read_bytes(&mut self, track_id: &str) -> io::Result<Vec<u8>> {
        if self.dir.is_none() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "cache dir not set"));
        }
        let path = match self.get(track_id) {
            None => {
                return Err(io::Error::new(io::ErrorKind::NotFound, "track not cached"));
            }
            Some(entry) => entry.path.clone(),
        };
        match fs::read(&path) {
            Ok(data) => Ok(data),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.remove(track_id);
                Err(e)
            }
            Err(e) => {
                if !Path::new(&path).exists() {
                    self.remove(track_id);
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "cached file missing",
                    ));
                }
                Err(e)
            }
        }
    }

    /// Remove the entry for `track_id` and delete its file best-effort
    /// (file-delete errors are ignored). Returns the removed entry, if any.
    pub fn remove_file(&mut self, track_id: &str) -> Option<CacheEntry> {
        let removed = self.remove(track_id)?;
        let _ = fs::remove_file(&removed.path);
        Some(removed)
    }

    /// Change the cap like [`OfflineCache::set_capacity`] but also delete
    /// evicted entries' files best-effort. Returns evicted entries (oldest
    /// first).
    pub fn set_capacity_on_disk(&mut self, capacity_bytes: u64) -> Vec<CacheEntry> {
        self.capacity_bytes = capacity_bytes;
        self.evict_to_disk()
    }

    /// Run eviction until under cap and delete evicted entries' files
    /// best-effort (individual delete errors are ignored). Returns evicted
    /// entries (oldest first). No-op when under cap.
    pub fn evict_to_disk(&mut self) -> Vec<CacheEntry> {
        let evicted = self.evict_while_over_cap();
        for entry in &evicted {
            let _ = fs::remove_file(&entry.path);
        }
        evicted
    }

    /// Drop all entries and delete cached audio files from disk.
    ///
    /// File-deletion policy: deletes every `*.m4a` file directly inside the
    /// cache dir (non-recursive), regardless of whether its stem currently
    /// maps to a tracked id. Rationale: this cache only ever creates
    /// `*.m4a` files in that dir, so any `*.m4a` there is ours: including
    /// orphans left by a previous crash, by [`OfflineCache::insert`] /
    /// [`OfflineCache::set_capacity`] / [`OfflineCache::remove`] /
    /// [`OfflineCache::clear`] (which never touch disk), or by filename
    /// collisions from sanitization. Non-`m4a` files and subdirectories are
    /// left alone. All deletions are best-effort; read/dir errors are
    /// ignored. No-op (besides clearing memory) when no dir is set.
    pub fn clear_disk(&mut self) {
        if let Some(dir) = self.dir.clone() {
            if let Ok(read_dir) = fs::read_dir(&dir) {
                for item in read_dir.flatten() {
                    let path = item.path();
                    if path.is_file() && path.extension().is_some_and(|ext| ext == "m4a") {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
        self.entries.clear();
        self.order.clear();
    }

    fn promote(&mut self, track_id: &str) {
        self.remove_from_order(track_id);
        self.order.push_back(track_id.to_string());
    }

    fn remove_from_order(&mut self, track_id: &str) {
        if let Some(pos) = self.order.iter().position(|id| id == track_id) {
            self.order.remove(pos);
        }
    }

    fn evict_while_over_cap(&mut self) -> Vec<CacheEntry> {
        let mut evicted = Vec::new();
        while self.usage() > self.capacity_bytes && self.order.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                evicted.push(entry);
            }
        }
        evicted
    }
}

/// Map a track id to a safe filename stem: ASCII alphanumerics are kept,
/// every other char becomes `_`. Empty input yields `"unknown"`.
fn sanitize_track_id(track_id: &str) -> String {
    if track_id.is_empty() {
        return "unknown".to_string();
    }
    track_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn entry(id: &str, bytes: u64) -> CacheEntry {
        CacheEntry::new(id, format!("/cache/{id}.mp3"), bytes, 1_700_000_000)
    }

    #[test]
    fn insert_get_remove() {
        let mut cache = OfflineCache::new(10_000);
        assert!(cache.is_empty());
        assert_eq!(cache.usage(), 0);

        assert!(cache.insert(entry("a", 100)).is_empty());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.usage(), 100);
        assert!(cache.contains("a"));

        assert_eq!(cache.get("a").unwrap().bytes, 100);
        assert!(cache.get("missing").is_none());

        let removed = cache.remove("a").unwrap();
        assert_eq!(removed.track_id, "a");
        assert!(cache.is_empty());
        assert_eq!(cache.usage(), 0);
        assert!(cache.remove("a").is_none());
    }

    #[test]
    fn evicts_oldest_first_under_cap() {
        let mut cache = OfflineCache::new(300);
        assert!(cache.insert(entry("a", 100)).is_empty());
        assert!(cache.insert(entry("b", 100)).is_empty());
        assert!(cache.insert(entry("c", 100)).is_empty());
        assert_eq!(cache.usage(), 300);

        // Inserting d (100) pushes usage to 400 -> evict oldest (a).
        let evicted = cache.insert(entry("d", 100));
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].track_id, "a");
        assert_eq!(cache.usage(), 300);
        assert!(!cache.contains("a"));
        assert!(cache.contains("d"));
        assert_eq!(
            cache.order_oldest_first(),
            vec!["b".to_string(), "c".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn get_refreshes_lru_so_hot_entry_survives() {
        let mut cache = OfflineCache::new(300);
        cache.insert(entry("a", 100));
        cache.insert(entry("b", 100));
        cache.insert(entry("c", 100));
        // Touch "a" -> order becomes b, c, a.
        cache.get("a");
        assert_eq!(
            cache.order_oldest_first(),
            vec!["b".to_string(), "c".to_string(), "a".to_string()]
        );
        // Next insert evicts "b", not the recently used "a".
        let evicted = cache.insert(entry("d", 100));
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].track_id, "b");
        assert!(cache.contains("a"));
    }

    #[test]
    fn cap_enforced_across_many_inserts() {
        let mut cache = OfflineCache::new(250);
        for i in 0..10 {
            cache.insert(entry(&format!("t{i}"), 100));
            assert!(
                cache.usage() <= 250,
                "usage {} exceeds cap after insert {i}",
                cache.usage()
            );
        }
        // Only the 2 newest survive (100 bytes each).
        assert_eq!(cache.len(), 2);
        assert!(cache.contains("t8"));
        assert!(cache.contains("t9"));
    }

    #[test]
    fn replacing_entry_updates_bytes_and_recency() {
        let mut cache = OfflineCache::new(500);
        cache.insert(entry("a", 100));
        cache.insert(entry("b", 100));
        // Re-insert "a" with bigger size: becomes newest.
        cache.insert(entry("a", 200));
        assert_eq!(cache.usage(), 300);
        assert_eq!(
            cache.order_oldest_first(),
            vec!["b".to_string(), "a".to_string()]
        );
        assert_eq!(cache.peek("a").unwrap().bytes, 200);
    }

    #[test]
    fn set_capacity_evicts_down() {
        let mut cache = OfflineCache::new(1000);
        cache.insert(entry("a", 200));
        cache.insert(entry("b", 200));
        cache.insert(entry("c", 200));
        let evicted = cache.set_capacity(400);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].track_id, "a");
        assert!(cache.usage() <= 400);
    }

    // ------------------------------------------------------------------
    // Disk-layer tests (offline; unique temp subdir per test, cleaned up).
    // ------------------------------------------------------------------

    static DISK_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_dir(name: &str) -> PathBuf {
        let n = DISK_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "yutu-mux-cache-{}-{}-{}",
            std::process::id(),
            name,
            n
        ))
    }

    fn disk_cache(name: &str, capacity: u64) -> (OfflineCache, PathBuf) {
        let dir = unique_dir(name);
        let _ = fs::remove_dir_all(&dir);
        let mut cache = OfflineCache::new(capacity);
        cache.set_dir(dir.to_string_lossy().into_owned());
        (cache, dir)
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn disk_ops_without_dir_return_err() {
        let mut cache = OfflineCache::new(1024);
        assert_eq!(cache.dir(), None);
        assert!(cache.file_path_for("a").is_none());
        assert!(cache.store_bytes("a", b"data").is_err());
        assert!(cache.read_bytes("a").is_err());
    }

    #[test]
    fn set_dir_and_file_path_for() {
        let mut cache = OfflineCache::new(1024);
        cache.set_dir("/tmp/yutu-test-dir");
        assert_eq!(cache.dir(), Some("/tmp/yutu-test-dir"));
        let p = cache.file_path_for("abc-123").unwrap();
        assert!(p.ends_with("abc_123.m4a"), "unexpected path: {p}");
    }

    #[test]
    fn store_read_roundtrip() {
        let (mut cache, dir) = disk_cache("roundtrip", 10_000);
        let stored = cache.store_bytes("track-1", b"hello audio").unwrap();
        assert_eq!(stored.track_id, "track-1");
        assert_eq!(stored.bytes, 11);
        assert!(Path::new(&stored.path).exists());
        let data = cache.read_bytes("track-1").unwrap();
        assert_eq!(data, b"hello audio");
        cleanup(&dir);
    }

    #[test]
    fn read_promotes_lru() {
        let (mut cache, dir) = disk_cache("promote", 30);
        cache.store_bytes("a", &[0u8; 10]).unwrap();
        cache.store_bytes("b", &[1u8; 10]).unwrap();
        cache.store_bytes("c", &[2u8; 10]).unwrap();
        assert_eq!(
            cache.order_oldest_first(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        // Reading "a" promotes it to newest.
        let data = cache.read_bytes("a").unwrap();
        assert_eq!(data, vec![0u8; 10]);
        assert_eq!(
            cache.order_oldest_first(),
            vec!["b".to_string(), "c".to_string(), "a".to_string()]
        );
        cleanup(&dir);
    }

    #[test]
    fn read_missing_entry_is_not_found() {
        let (mut cache, dir) = disk_cache("read-missing", 1024);
        let err = cache.read_bytes("nope").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        cleanup(&dir);
    }

    #[test]
    fn eviction_deletes_oldest_file() {
        let (mut cache, dir) = disk_cache("evict", 25);
        cache.store_bytes("a", &[0u8; 10]).unwrap();
        let path_a = cache.file_path_for("a").unwrap();
        assert!(Path::new(&path_a).exists());
        cache.store_bytes("b", &[1u8; 10]).unwrap();
        // usage 20 <= 25, both files present.
        assert!(Path::new(&path_a).exists());
        cache.store_bytes("c", &[2u8; 10]).unwrap();
        // usage would be 30 > 25 -> oldest ("a") evicted and file deleted.
        assert!(!cache.contains("a"));
        assert!(
            !Path::new(&path_a).exists(),
            "evicted file should be deleted"
        );
        assert!(cache.contains("b"));
        assert!(cache.contains("c"));
        assert!(Path::new(&cache.file_path_for("b").unwrap()).exists());
        assert!(Path::new(&cache.file_path_for("c").unwrap()).exists());
        cleanup(&dir);
    }

    #[test]
    fn read_after_hot_touch_evicts_cold_entry_file() {
        let (mut cache, dir) = disk_cache("hot", 25);
        cache.store_bytes("a", &[0u8; 10]).unwrap();
        cache.store_bytes("b", &[1u8; 10]).unwrap();
        // Touch "a" so "b" is oldest.
        cache.read_bytes("a").unwrap();
        cache.store_bytes("c", &[2u8; 10]).unwrap();
        assert!(cache.contains("a"));
        assert!(!cache.contains("b"));
        assert!(!Path::new(&cache.file_path_for("b").unwrap_or_default()).exists());
        cleanup(&dir);
    }

    #[test]
    fn self_heals_on_missing_file() {
        let (mut cache, dir) = disk_cache("selfheal", 1024);
        cache.store_bytes("a", b"data").unwrap();
        let path = cache.file_path_for("a").unwrap();
        fs::remove_file(&path).unwrap();
        let err = cache.read_bytes("a").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        // Entry was dropped.
        assert!(!cache.contains("a"));
        assert!(cache.peek("a").is_none());
        cleanup(&dir);
    }

    #[test]
    fn sanitize_edge_cases() {
        let (cache, dir) = disk_cache("sanitize", 1024);
        assert_eq!(
            cache.file_path_for("abcXYZ012").unwrap(),
            Path::new(&dir)
                .join("abcXYZ012.m4a")
                .to_string_lossy()
                .into_owned()
        );
        // Everything non-alphanumeric becomes `_`.
        let p = cache.file_path_for("a/b\\c:d e.f-g!").unwrap();
        assert!(p.ends_with("a_b_c_d_e_f_g_.m4a"), "unexpected: {p}");
        // Empty id -> "unknown".
        assert!(cache.file_path_for("").unwrap().ends_with("unknown.m4a"));
        // Unicode alphanumerics are NOT ASCII -> `_`.
        let uni = cache.file_path_for("héllo").unwrap();
        assert!(uni.ends_with("h_llo.m4a"), "unexpected: {uni}");
        cleanup(&dir);
    }

    #[test]
    fn remove_file_deletes_from_disk() {
        let (mut cache, dir) = disk_cache("remove", 1024);
        cache.store_bytes("a", b"data").unwrap();
        let path = cache.file_path_for("a").unwrap();
        assert!(Path::new(&path).exists());
        let removed = cache.remove_file("a").unwrap();
        assert_eq!(removed.track_id, "a");
        assert!(!Path::new(&path).exists());
        assert!(cache.remove_file("missing").is_none());
        cleanup(&dir);
    }

    #[test]
    fn clear_disk_removes_files() {
        let (mut cache, dir) = disk_cache("clear", 1024);
        cache.store_bytes("a", b"aaa").unwrap();
        cache.store_bytes("b", b"bbb").unwrap();
        // A non-m4a sidecar must survive (policy: only *.m4a deleted).
        let sidecar = dir.join("notes.txt");
        fs::write(&sidecar, b"keep").unwrap();
        // An orphan *.m4a (no tracker entry) is also removed: it can only
        // be ours since we solely create *.m4a in this dir.
        let orphan = dir.join("orphan.m4a");
        fs::write(&orphan, b"orphan").unwrap();
        cache.clear_disk();
        assert!(cache.is_empty());
        assert_eq!(cache.usage(), 0);
        assert!(!dir.join("a.m4a").exists());
        assert!(!dir.join("b.m4a").exists());
        assert!(!orphan.exists());
        assert!(sidecar.exists(), "non-m4a files must be left alone");
        cleanup(&dir);
    }

    #[test]
    fn set_capacity_on_disk_deletes_files() {
        let (mut cache, dir) = disk_cache("capdisk", 1000);
        cache.store_bytes("a", &[0u8; 200]).unwrap();
        cache.store_bytes("b", &[1u8; 200]).unwrap();
        cache.store_bytes("c", &[2u8; 200]).unwrap();
        let path_a = cache.file_path_for("a").unwrap();
        assert!(Path::new(&path_a).exists());
        let evicted = cache.set_capacity_on_disk(400);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].track_id, "a");
        assert!(!Path::new(&path_a).exists());
        assert!(cache.usage() <= 400);
        cleanup(&dir);
    }

    #[test]
    fn store_overwrites_same_track() {
        let (mut cache, dir) = disk_cache("overwrite", 1024);
        cache.store_bytes("a", b"v1").unwrap();
        let stored = cache.store_bytes("a", b"v2-longer").unwrap();
        assert_eq!(stored.bytes, 9);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.read_bytes("a").unwrap(), b"v2-longer");
        cleanup(&dir);
    }
}
