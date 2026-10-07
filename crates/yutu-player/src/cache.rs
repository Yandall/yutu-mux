//! Offline smart-download tracker.
//!
//! Pure in-memory bookkeeping: which tracks are cached, how many bytes
//! they use, and LRU eviction when the capacity cap is exceeded.
//! No actual downloading happens here.

use std::collections::{HashMap, VecDeque};

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
}

impl OfflineCache {
    /// Create an empty cache capped at `capacity_bytes` total bytes.
    #[must_use]
    pub fn new(capacity_bytes: u64) -> Self {
        Self {
            capacity_bytes,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.capacity_bytes
    }

    /// Change the cap, evicting oldest entries if now over capacity.
    /// Returns the evicted entries (oldest first).
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
    pub fn remove(&mut self, track_id: &str) -> Option<CacheEntry> {
        let removed = self.entries.remove(track_id)?;
        self.remove_from_order(track_id);
        Some(removed)
    }

    /// Remove everything.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Track ids from oldest to newest (useful for UI / tests).
    #[must_use]
    pub fn order_oldest_first(&self) -> Vec<String> {
        self.order.iter().cloned().collect()
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
