//! The native world API: one battle's authoritative tick, its publication
//! and its post-step reads through a thread-affine `Context`, for a host
//! that embeds the simulation instead of instantiating the WebAssembly
//! module (the dedicated server's Node addon, the whole-world replay
//! driver). These entries are the native twins of the `world_step_init`,
//! `world_step_run`, `wire_publish` and bank-reading exports JavaScript
//! calls on the module; they share the exports' bodies and never call back
//! into a host.
#[path = "context_publish.rs"]
mod publish;
pub use publish::{bank, Fault, Publication};
#[path = "context_admit.rs"]
mod admit;
#[path = "context_observe.rs"]
mod observe;
pub use observe::{ExplosionField, ExplosionOwner, OwnedExplosion, PropTargetFacts, WeaponLevelFacts};

use super::context::{Context, Error, ErrorKind};
use crate::gen::{WorldTickInput, WorldTickOutput, WorldTickSetup};
use crate::world::owner::World;
use std::panic::{catch_unwind, AssertUnwindSafe};


/// Which entries have bodies. A host reports these in its handshake instead
/// of hard-coding them, so a build with a refusing entry says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provided {
    /// `world_step_init` and `world_step` run the native tick.
    pub step: bool,
    /// `world_publish_into` and `world_publish` run the shared publisher.
    pub publication: bool,
    /// `read_bank` copies every `bank::*`.
    pub banks: bool,
    /// `run_operation_into` and `run_operation` run the operation table.
    pub operations: bool,
    /// The whole-world replay adapters admit, prepare and observe.
    pub replay: bool,
}
/// What this build provides: everything but the whole-world replay.
pub const fn provided() -> Provided {
    Provided { step: true, publication: true, banks: true, operations: true, replay: false }
}

impl Context {
    /// One owning entry: the world borrow under `catch_unwind`. A panic in a
    /// body leaves the world half changed, so it poisons the context instead
    /// of unwinding into a host: every later entry fails Poisoned and only
    /// `dispose` succeeds. Nothing here calls back into a host, so the only
    /// panics are Rust's own, and the hook has already reported them.
    pub fn owned<T>(&self, run: impl FnOnce(&mut World) -> T) -> Result<T, Error> {
        let _flight = self.enter()?;
        match catch_unwind(AssertUnwindSafe(|| self.world(run))) {
            Ok(result) => result,
            Err(_payload) => {
                self.poison();
                Err(Error::new(ErrorKind::Poisoned, "context poisoned"))
            }
        }
    }

    /// `World::step_init`: adopts the battle's water clock before the first
    /// tick. Ordinary ticks never reimport it.
    pub fn world_step_init(&self, setup: WorldTickSetup) -> Result<(), Error> {
        self.owned(|w| w.step_init(&setup))
    }

    /// `World::step`, the whole authoritative interval, inside the owning
    /// borrow. A panic inside the step leaves the world half stepped, so it
    /// poisons the context instead of propagating: every later entry fails
    /// Poisoned and only `dispose` succeeds. A world whose unit service is
    /// not prepared cannot tick; the step is refused the way `World::step`
    /// refuses a bad input, with `accepted` false and the clock unchanged.
    pub fn world_step(&self, input: WorldTickInput) -> Result<WorldTickOutput, Error> {
        self.owned(|w| {
            if crate::units::f3::output_bank_in(w).is_none() {
                return WorldTickOutput { clock: input.clock.clone(), ..Default::default() };
            }
            w.step(&input)
        })
    }

    /// The authority's tick, native only: what the JavaScript server did
    /// around `game.step` (the edges' advance, the owners' input banks, the
    /// notices' drain and capture, the players' mirrors, the edges'
    /// consumption; crate::host) with `world_step` between. A refused step
    /// answers `accepted` false and leaves the host's state as it was.
    pub fn world_tick(&self, input: WorldTickInput) -> Result<WorldTickOutput, Error> {
        self.host(|host| host.tick(self, input))?.map_err(Error::host)
    }
}
