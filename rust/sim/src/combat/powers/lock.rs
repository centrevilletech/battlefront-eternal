//! The on-foot target lock: one press picks the enemy nearest the aim within
//! the selected weapon's lock range, a second press lets it go.

use super::key::LifeKey;

/// Acquisition cone around the aim, as the cosine of its half angle.
/// Provisional until a capture measures the 2005 game's on-foot cone.
pub const PROVISIONAL_LOCK_ACQUIRE_COS: f64 = 0.866_025_403_784_438_6;
/// A held lock survives out to this multiple of the acquisition range.
/// Provisional until a capture measures the 2005 game's release distance.
pub const PROVISIONAL_LOCK_KEEP_RANGE: f64 = 1.25;
/// A lock lapses after its target has been out of sight this many ticks.
/// Provisional until a capture measures the 2005 game's release timing.
pub const PROVISIONAL_LOCK_UNSEEN_TICKS: u32 = 60;

/// Per-life lock state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LockState {
    pub target: Option<LifeKey>,
    /// Ticks since the target was last in sight.
    pub unseen: u32,
    /// Range the lock was taken with.
    pub range: f64,
}

/// One lock candidate as the world sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LockCandidate {
    pub life: LifeKey,
    pub distance: f64,
    /// Cosine of the angle between the aim and the direction to the target.
    pub alignment: f64,
}

impl LockState {
    /// Choose among candidates already known to be enemies in sight: the
    /// best aligned one inside the cone and the range, ties to the nearer.
    pub fn choose(range: f64, candidates: impl Iterator<Item = LockCandidate>) -> Option<LockCandidate> {
        let mut best: Option<LockCandidate> = None;
        for c in candidates {
            if !(c.distance <= range) || !(c.alignment >= PROVISIONAL_LOCK_ACQUIRE_COS) {
                continue;
            }
            let better = match best {
                None => true,
                Some(b) => c.alignment > b.alignment || (c.alignment == b.alignment && c.distance < b.distance),
            };
            if better {
                best = Some(c);
            }
        }
        best
    }

    pub fn take(&mut self, target: LifeKey, range: f64) {
        *self = LockState { target: Some(target), unseen: 0, range };
    }

    pub fn clear(&mut self) {
        *self = LockState::default();
    }

    /// Keep the lock for one more tick given the target's distance and
    /// whether it is in sight; a lock on a dead or replaced life is dropped
    /// by the caller.
    pub fn keep(&mut self, distance: f64, seen: bool) -> bool {
        if self.target.is_none() {
            return false;
        }
        self.unseen = if seen { 0 } else { self.unseen.saturating_add(1) };
        if !(distance <= self.range * PROVISIONAL_LOCK_KEEP_RANGE) || self.unseen > PROVISIONAL_LOCK_UNSEEN_TICKS {
            self.clear();
            return false;
        }
        true
    }
}
