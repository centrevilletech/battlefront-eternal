//! Stops a window from flipping between two sizes forever.
//!
//! A Wayland compositor that resizes a floating window to whatever size its
//! client last committed (sway and others do) and a client that redraws at
//! whatever size it was last configured can chase each other: one frame is in
//! flight at the old size when the compositor settles on the new one, the
//! compositor takes that frame as a request for the old size, and from then
//! on each side answers the other's previous size. Nothing ends it, and the
//! window is redrawn at alternating sizes every frame.
//!
//! The guard watches the sizes the window is told to take. Once two sizes
//! have alternated five times within two seconds it picks one and reports it
//! for the window to request each time the other comes back, until the other
//! has stayed away for half a second. A third size ends the hold at once, so
//! a tile or fullscreen change the compositor decides is never refused.

/// Alternating sizes kept, oldest first.
const HISTORY: usize = 5;
/// Seconds within which those sizes must have arrived.
const WINDOW: f64 = 2.0;
/// Seconds the refused size must stay away before the hold ends.
const QUIET: f64 = 0.5;

#[derive(Default)]
pub struct ResizeGuard {
    recent: [(f64, [u32; 2]); HISTORY],
    len: usize,
    /// The size kept, the size refused, and when the hold ends.
    hold: Option<([u32; 2], [u32; 2], f64)>,
}

impl ResizeGuard {
    /// Takes a size the window was told to take at `now` seconds. Returns the
    /// size to request instead when this one is the refused half of an
    /// oscillation. `prefer` is a size to keep when it is one of the two
    /// (the monitor's while fullscreen); otherwise the newest is kept.
    pub fn observe(&mut self, now: f64, size: [u32; 2], prefer: Option<[u32; 2]>) -> Option<[u32; 2]> {
        if let Some((keep, refuse, until)) = self.hold {
            if now < until && size == refuse {
                self.hold = Some((keep, refuse, now + QUIET));
                return Some(keep);
            }
            if now < until && size == keep {
                return None;
            }
            self.hold = None;
            self.len = 0;
        }
        if self.len != 0 && self.recent[self.len - 1].1 == size {
            return None;
        }
        if self.len == HISTORY {
            self.recent.copy_within(1.., 0);
            self.len -= 1;
        }
        self.recent[self.len] = (now, size);
        self.len += 1;
        let r = &self.recent;
        let alternating = self.len == HISTORY
            && r[4].1 == r[2].1
            && r[2].1 == r[0].1
            && r[3].1 == r[1].1
            && now - r[0].0 <= WINDOW;
        if !alternating {
            return None;
        }
        let other = r[3].1;
        let keep = if prefer == Some(other) { other } else { size };
        let refuse = if keep == size { other } else { size };
        self.hold = Some((keep, refuse, now + QUIET));
        self.len = 0;
        (keep != size).then_some(keep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: [u32; 2] = [1490, 1110];
    const B: [u32; 2] = [1584, 1162];
    const C: [u32; 2] = [1256, 1412];

    #[test]
    fn two_alternating_sizes_are_held_at_the_newest() {
        let mut guard = ResizeGuard::default();
        for (i, size) in [A, B, A, B].into_iter().enumerate() {
            assert_eq!(guard.observe(i as f64 * 0.1, size, None), None);
        }
        // The fifth alternation starts the hold on the size just taken.
        assert_eq!(guard.observe(0.4, A, None), None);
        assert_eq!(guard.observe(0.5, B, None), Some(A));
        assert_eq!(guard.observe(0.6, A, None), None);
        assert_eq!(guard.observe(0.7, B, None), Some(A));
        // Quiet for half a second: the hold is over and B is an ordinary size.
        assert_eq!(guard.observe(1.3, B, None), None);
    }

    #[test]
    fn a_preferred_size_wins_and_a_third_size_ends_the_hold() {
        let mut guard = ResizeGuard::default();
        for (i, size) in [B, A, B, A].into_iter().enumerate() {
            assert_eq!(guard.observe(i as f64 * 0.1, size, Some(A)), None);
        }
        // B arrives fifth, but A is the monitor's size: A is kept.
        assert_eq!(guard.observe(0.4, B, Some(A)), Some(A));
        assert_eq!(guard.observe(0.5, B, Some(A)), Some(A));
        // The compositor decides a different size: accepted, hold dropped.
        assert_eq!(guard.observe(0.6, C, Some(A)), None);
        assert_eq!(guard.observe(0.7, B, Some(A)), None);
    }

    #[test]
    fn a_slow_or_one_way_resize_is_left_alone() {
        let mut guard = ResizeGuard::default();
        for (i, size) in [A, B, A, B, A].into_iter().enumerate() {
            assert_eq!(guard.observe(i as f64, size, None), None);
        }
        let mut guard = ResizeGuard::default();
        for i in 0..40 {
            assert_eq!(guard.observe(i as f64 * 0.01, [900 + i, 600 + i], None), None);
        }
    }
}
