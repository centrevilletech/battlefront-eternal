//! The 2005 game's availability gate opened, for a test that hosts or joins
//! a 2005 battle. It is a value the test hands to what it starts (a server's
//! configuration, a client's join), never a switch for the process: tests in
//! the same binary that check the closed gate keep [`bfe_family::Gate::BUILT`].
//!
//! No crate that ships depends on this one; only `[dev-dependencies]` name it.
//! The release checks (`build/ci/check-release-gate.sh`) refuse a dependency
//! on it from a shipped crate under any features, and a release client or
//! server binary that contains [`MARKER`].

use bfe_family::{Availability, GameFamily, Gate};

/// The name of the open gate. Any binary that can use [`OPEN`] contains it;
/// the release checks look for it in every client and server they build.
pub const MARKER: &str = "bfe-test-gate: every game opened for a test";

struct Open;

impl Availability for Open {
    #[inline(never)]
    fn plays(&self, _family: GameFamily) -> bool {
        // Every binary that can ask this gate anything keeps the marker, even
        // when the optimizer calls this function without the gate's table.
        std::hint::black_box(MARKER);
        true
    }
    fn name(&self) -> &'static str {
        MARKER
    }
}

/// A gate that plays every game.
pub const OPEN: Gate = Gate::new(&Open);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_gate_plays_both_games_and_leaves_the_built_gate_closed() {
        assert!(OPEN.plays(GameFamily::Swbf1) && OPEN.plays(GameFamily::Swbf2));
        assert!(!Gate::BUILT.plays(GameFamily::Swbf2));
        assert!(!GameFamily::Swbf2.is_available());
        assert_eq!(format!("{OPEN:?}"), format!("Gate({MARKER:?})"));
    }
}
