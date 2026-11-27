//! What a ground check costs. A unit is about a fifth of a microsecond on the
//! reference x86-64 machine. Each piece of a check pays for the collision it
//! meets, so a crowded deck is charged what it takes, and the charge is the
//! same on every machine.

/// What one check may spend.
pub(super) struct Work {
    limit: usize,
    used: usize,
    cast: usize,
    step: usize,
}

impl Work {
    /// The admission survey finishes every check.
    pub fn unlimited() -> Self {
        Self {
            limit: usize::MAX,
            used: 0,
            cast: 0,
            step: 0,
        }
    }
    /// `enabled` bodies in the world are tried by a ray, and every one that
    /// `moves` by each part of a mover.
    pub fn new(limit: usize, enabled: usize, moves: usize) -> Self {
        Self {
            limit,
            used: 4,
            cast: 3 + enabled / 40,
            step: 3 + moves / 3,
        }
    }
    pub fn spent(&self) -> bool {
        self.used >= self.limit
    }
    pub fn used(&self) -> usize {
        self.used
    }
    /// One ray against the static world.
    pub fn cast(&mut self) {
        self.used += self.cast;
    }
    /// One mover step among `triangles`. Trying the space over a step ahead
    /// asks for every part of the body again and again.
    pub fn step(&mut self, triangles: usize, climbs: bool) {
        self.used += self.step + 2 * triangles;
        if climbs {
            self.used += 200;
        }
    }
}
