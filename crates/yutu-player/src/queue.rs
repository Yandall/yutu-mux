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

    pub fn move_up(&mut self, i: usize) -> bool {
        if i == 0 || i >= self.items.len() {
            return false;
        }
        self.items.swap(i - 1, i);
        self.fix_index_after_swap(i - 1, i);
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
        let next = match self.index {
            None => 0,
            Some(i) if i + 1 < self.items.len() => i + 1,
            Some(_) if self.repeat == RepeatMode::All => 0,
            Some(i) if self.repeat == RepeatMode::One => i,
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
