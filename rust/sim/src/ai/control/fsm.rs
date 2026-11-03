//! The generic state-machine driver every Classic controller, agent
//! and embedded helper shares (controller.js:274 Fsm). The drive
//! itself lives with the core (mod.rs `fsm_update`/`fsm_flush`),
//! dispatching each owner's enter/exit/tick; this is its state.

/// The terminal state every machine here uses (controller.js:27).
pub const DONE: f64 = 100.0;

/// Bound on transitions in one update (controller.js:272). The 2004
/// game has none; the JavaScript stops and warns, and so does this.
pub const DRAIN_LIMIT: u32 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fsm {
    pub state: f64,
    pub pending: f64,
}

impl Fsm {
    pub fn new() -> Self {
        Fsm { state: 0.0, pending: 0.0 }
    }
    pub fn done(&self) -> bool {
        self.state == DONE
    }
    pub fn request(&mut self, state: f64) {
        self.pending = state;
    }
}
