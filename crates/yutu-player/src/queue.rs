//! Queue state machine (no audio here).

use yutu_core::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Default)]
pub struct Queue {
    items: Vec<Track>,
    index: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

impl Queue {
    pub fn push(&mut self, track: Track) {
        self.items.push(track);
        if self.index.is_none() {
            self.index = Some(0);
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current(&self) -> Option<&Track> {
        self.index.and_then(|i| self.items.get(i))
    }

    pub fn items(&self) -> &[Track] {
        &self.items
    }

    pub fn remove_at(&mut self, i: usize) -> bool {
        if i >= self.items.len() {
            return false;
        }
        self.items.remove(i);
        self.index = match self.index {
            None => None,
            Some(_) if self.items.is_empty() => None,
            Some(cur) if i < cur => Some(cur - 1),
            Some(cur) if i == cur => Some(cur.min(self.items.len().saturating_sub(1))),
            Some(cur) => Some(cur),
        };
        true
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.index = None;
    }

    /// Remove one occurrence of `track_id`, preferring the instance at or
    /// after the current position (the one the user sees ahead), falling
    /// back to the nearest one before it. Returns false when absent.
    /// Index bookkeeping mirrors [`Queue::remove_at`].
    pub fn remove_id(&mut self, track_id: &str) -> bool {
        let at = self
            .index
            .and_then(|cur| {
                self.items
                    .iter()
                    .enumerate()
                    .skip(cur)
                    .find(|(_, t)| t.id == track_id)
                    .map(|(i, _)| i)
            })
            .or_else(|| self.items.iter().position(|t| t.id == track_id));
        match at {
            Some(i) => self.remove_at(i),
            None => false,
        }
    }

    /// Insert `track` right after the current item (play-next). Empty queue:
    /// behaves like [`Queue::push`]. Returns the inserted position.
    pub fn insert_next(&mut self, track: Track) -> usize {
        let at = match self.index {
            Some(i) => (i + 1).min(self.items.len()),
            None => {
                self.push(track);
                return 0;
            }
        };
        self.items.insert(at, track);
        at
    }

    pub fn move_up(&mut self, i: usize) -> bool {
        if i == 0 || i >= self.items.len() {
            return false;
        }
        self.items.swap(i - 1, i);
        self.fix_index_after_swap(i - 1, i);
        true
    }

    /// Move item `from` to position `to` (drag reorder). Out of range
    /// or no-op moves return false without touching anything. The current
    /// index tracks its item across the move.
    pub fn move_to(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return from != to && (from < self.items.len() && to < self.items.len());
        }
        let track = self.items.remove(from);
        self.items.insert(to, track);
        self.index = self.index.map(|cur| {
            if cur == from {
                to
            } else if from < cur && cur <= to {
                cur - 1
            } else if to <= cur && cur < from {
                cur + 1
            } else {
                cur
            }
        });
        true
    }

    pub fn move_down(&mut self, i: usize) -> bool {
        if i + 1 >= self.items.len() {
            return false;
        }
        self.items.swap(i, i + 1);
        self.fix_index_after_swap(i, i + 1);
        true
    }

    fn fix_index_after_swap(&mut self, a: usize, b: usize) {
        self.index = self.index.map(|cur| {
            if cur == a {
                b
            } else if cur == b {
                a
            } else {
                cur
            }
        });
    }

    pub fn next_track(&mut self) -> Option<&Track> {
        if self.items.is_empty() {
            return None;
        }
        // Repeat-One replays the current song wherever it sits; it must
        // win over the plain advance below it.
        let next = match self.index {
            None => 0,
            Some(i) if self.repeat == RepeatMode::One => i,
            Some(i) if i + 1 < self.items.len() => i + 1,
            Some(_) if self.repeat == RepeatMode::All => 0,
            _ => return None,
        };
        self.index = Some(next);
        self.current()
    }

    pub fn prev_track(&mut self) -> Option<&Track> {
        if self.items.is_empty() {
            return None;
        }
        let prev = match self.index {
            None => 0,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.index = Some(prev);
        self.current()
    }

    /// Current position in the queue (index of [`Queue::current`]).
    #[must_use]
    pub fn position(&self) -> Option<usize> {
        self.index
    }

    /// Jump playback to item `i`. Returns false (no move) when out of range.
    pub fn jump_to(&mut self, i: usize) -> bool {
        if i < self.items.len() {
            self.index = Some(i);
            true
        } else {
            false
        }
    }

    /// Cycle repeat mode Off → All → One → Off (player-controls button).
    pub fn cycle_repeat(&mut self) -> RepeatMode {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        self.repeat
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yutu_core::MockBackend;

    fn queue_of(n: usize) -> Queue {
        let mut q = Queue::default();
        for t in MockBackend::fixture_tracks().into_iter().take(n) {
            q.push(t);
        }
        q
    }

    #[test]
    fn jump_to_moves_within_range_only() {
        let mut q = queue_of(3);
        assert_eq!(q.position(), Some(0));
        assert!(q.jump_to(2));
        assert_eq!(q.position(), Some(2));
        assert_eq!(q.current().unwrap().id, "m3");
        assert!(!q.jump_to(3));
        assert!(!q.jump_to(99));
        assert_eq!(q.position(), Some(2));
        assert!(Queue::default().position().is_none());
        assert!(!Queue::default().jump_to(0));
    }

    #[test]
    fn remove_id_prefers_upcoming_occurrence() {
        let mut q = queue_of(3);
        q.push(MockBackend::fixture_tracks()[0].clone()); // m1 twice
        assert!(q.remove_id("m1"));
        // Current is m1 itself (index 0) → that instance goes.
        assert_eq!(q.items().len(), 3);
        assert_eq!(q.items()[0].id, "m2");
        assert!(!q.remove_id("zzz"));
        // Removing the current item keeps a valid index.
        assert!(q.jump_to(2));
        assert!(q.remove_id(q.current().unwrap().id.clone().as_str()));
        assert!(q.position().unwrap() < q.len());
    }

    #[test]
    fn repeat_one_replays_mid_queue() {
        let mut q = queue_of(3);
        q.repeat = RepeatMode::One;
        assert!(q.jump_to(0));
        // Mid-queue: One replays the same song (used to skip ahead).
        assert_eq!(q.next_track().unwrap().id, "m1");
        assert_eq!(q.position(), Some(0));
        assert!(q.jump_to(1));
        assert_eq!(q.next_track().unwrap().id, "m2");
    }

    #[test]
    fn move_to_reorders_and_tracks_current() {
        let mut q = queue_of(4); // m1 m2 m3 m4, current m1
        assert!(q.move_to(0, 2)); // m2 m3 m1 m4
        assert_eq!(q.items()[2].id, "m1");
        assert_eq!(q.position(), Some(2));
        assert!(q.move_to(3, 0)); // m4 m2 m3 m1
        assert_eq!(q.items()[0].id, "m4");
        assert_eq!(q.position(), Some(3));
        assert!(!q.move_to(1, 1));
        assert!(!q.move_to(0, 9));
        assert!(!q.move_to(9, 0));
        assert_eq!(q.items().len(), 4);
    }

    #[test]
    fn insert_next_lands_after_current() {
        let mut q = queue_of(3);
        assert_eq!(q.position(), Some(0));
        let t = MockBackend::fixture_tracks()[4].clone();
        assert_eq!(q.insert_next(t), 1);
        assert_eq!(q.items()[1].id, "m5");
        assert_eq!(q.position(), Some(0));
        // Empty queue: insert acts as push.
        let mut q = Queue::default();
        assert_eq!(q.insert_next(MockBackend::fixture_tracks()[0].clone()), 0);
        assert_eq!(q.position(), Some(0));
    }

    #[test]
    fn repeat_cycles_off_all_one_off() {
        let mut q = Queue::default();
        assert_eq!(q.repeat, RepeatMode::Off);
        assert_eq!(q.cycle_repeat(), RepeatMode::All);
        assert_eq!(q.cycle_repeat(), RepeatMode::One);
        assert_eq!(q.cycle_repeat(), RepeatMode::Off);
    }

    #[test]
    fn queue_next_prev() {
        let mut q = queue_of(3);
        assert_eq!(q.current().unwrap().id, "m1");
        assert_eq!(q.next_track().unwrap().id, "m2");
        assert_eq!(q.next_track().unwrap().id, "m3");
        assert!(q.next_track().is_none());
        assert_eq!(q.prev_track().unwrap().id, "m2");
    }

    #[test]
    fn queue_repeat_all_wraps() {
        let mut q = queue_of(2);
        q.repeat = RepeatMode::All;
        q.next_track();
        q.next_track();
        assert_eq!(q.current().unwrap().id, "m1");
    }

    #[test]
    fn queue_remove_and_reorder() {
        let mut q = queue_of(3);
        assert!(q.move_down(0));
        assert_eq!(q.items()[0].id, "m2");
        assert!(q.move_up(1));
        assert_eq!(q.items()[0].id, "m1");
        assert!(q.remove_at(0));
        assert_eq!(q.len(), 2);
        assert!(!q.remove_at(9));
        q.clear();
        assert!(q.is_empty());
        assert!(q.current().is_none());
    }
}
