//! The developer switch for online play of the 2005 game, for testing it
//! before it is switched on. This file is compiled only with
//! `--cfg bfe_dev_online` (`RUSTFLAGS`), which no feature, manifest or build
//! script here sets, so an ordinary build has no switch to read: not a
//! release build, and not one with every feature on.
//!
//! A program built with it still holds [`Gate::BUILT`] until
//! `BFE_DEV_SWBF2_ONLINE=1` is in its environment. The release checks
//! (`build/ci/check-release-gate.sh`, `build/native/stage.py`) refuse any
//! binary that contains [`MARKER`].

use crate::{Availability, GameFamily, Gate};

/// The gate's name. Any binary that contains the switch contains it; the
/// release checks look for it in every client and server they build.
pub const MARKER: &str = "developer switch BFE_DEV_SWBF2_ONLINE";

struct Switched;

impl Availability for Switched {
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

/// The gate the switch opens, when it is on.
pub(crate) fn gate() -> Option<Gate> {
    let on = std::env::var_os("BFE_DEV_SWBF2_ONLINE").is_some_and(|value| value == "1");
    on.then_some(Gate::new(&Switched))
}
