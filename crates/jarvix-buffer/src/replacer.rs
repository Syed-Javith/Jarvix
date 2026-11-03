pub type FrameId = usize;

/// ClockReplacer implements the second-chance Clock page eviction policy.
pub struct ClockReplacer {
    num_frames: usize,
    clock_hand: usize,
    referenced: Vec<bool>,
    in_replacer: Vec<bool>,
    size: usize,
}

impl ClockReplacer {
    pub fn new(num_frames: usize) -> Self {
        Self {
            num_frames,
            clock_hand: 0,
            referenced: vec![false; num_frames],
            in_replacer: vec![false; num_frames],
            size: 0,
        }
    }

    /// Returns the number of frames currently eligible for eviction.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Notify replacer that a frame has been unpinned and is now eligible for eviction.
    pub fn set_evictable(&mut self, frame_id: FrameId, evictable: bool) {
        if frame_id >= self.num_frames {
            return;
        }

        if evictable {
            if !self.in_replacer[frame_id] {
                self.in_replacer[frame_id] = true;
                self.referenced[frame_id] = true;
                self.size += 1;
            }
        } else if self.in_replacer[frame_id] {
            self.in_replacer[frame_id] = false;
            self.referenced[frame_id] = false;
            self.size -= 1;
        }
    }

    /// Record an access to a frame (giving it a second chance).
    pub fn record_access(&mut self, frame_id: FrameId) {
        if frame_id < self.num_frames && self.in_replacer[frame_id] {
            self.referenced[frame_id] = true;
        }
    }

    /// Evicts a frame using the Clock algorithm, returning its FrameId.
    pub fn victim(&mut self) -> Option<FrameId> {
        if self.size == 0 {
            return None;
        }

        let mut scans = 0;
        let max_scans = self.num_frames * 2;

        while scans < max_scans {
            let curr = self.clock_hand;
            self.clock_hand = (self.clock_hand + 1) % self.num_frames;

            if self.in_replacer[curr] {
                if self.referenced[curr] {
                    // Give second chance
                    self.referenced[curr] = false;
                } else {
                    // Evict this frame
                    self.in_replacer[curr] = false;
                    self.size -= 1;
                    return Some(curr);
                }
            }
            scans += 1;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_replacer() {
        let mut replacer = ClockReplacer::new(4);

        replacer.set_evictable(0, true);
        replacer.set_evictable(1, true);
        replacer.set_evictable(2, true);
        assert_eq!(replacer.size(), 3);

        // Frame 0, 1, 2 have reference bit set to true initially.
        // First scan clears ref bits, next scan evicts 0.
        let victim1 = replacer.victim();
        assert!(victim1.is_some());
        assert_eq!(replacer.size(), 2);
    }
}
