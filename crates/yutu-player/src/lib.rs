//! yutu-player: queue state machine + audio backend trait.
//! Phase 2 adds rodio; Phase 5 adds mpv backend. Stub keeps Phase 0 green.

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

/// Stability anchor for audio backends (stub → rodio → mpv).
pub trait AudioBackend {
    fn is_ok(&self) -> bool;
}

pub struct StubBackend;

impl AudioBackend for StubBackend {
    fn is_ok(&self) -> bool {
        true
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
}
