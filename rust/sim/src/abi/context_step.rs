//! The native world API: one battle's authoritative tick, its publication
//! and its post-step reads through a thread-affine `Context`, for a host
//! that embeds the simulation instead of instantiating the WebAssembly
//! module (the dedicated server's Node addon, the whole-world replay
//! driver). These entries are the native twins of the `world_step_init`,
//! `world_step_run`, `wire_publish` and bank-reading exports JavaScript
//! calls on the module; they share the exports' bodies and never call back
//! into a host.
//!
//! Frozen signatures (hosts bind against this table; a change is announced
//! before it lands):
//!
//! | Entry | Signature | Notes |
//! | --- | --- | --- |
//! | step setup | `world_step_init(&self, WorldTickSetup) -> Result<(), Error>` | `World::step_init`: binds the water clock once a battle |
//! | step | `world_step(&self, WorldTickInput) -> Result<WorldTickOutput, Error>` | `World::step` under `catch_unwind`; a panic poisons the context |
//! | publish | `world_publish_into(&self, session: u32, facts: &[f64], out: &mut Publication) -> Result<(), Error>` | `facts` is a `WireWorldFacts` row; the shared `wire_publish_in` body; spans in the block are offsets into `Publication::bytes`; `out` keeps its capacity |
//! | publish, owned | `world_publish(&self, session: u32, facts: &[f64]) -> Result<Publication, Error>` | the same into a new publication |
//! | read | `read_bank(&self, bank: u32, out: &mut Vec<f64>) -> Result<usize, Error>` | copies one `bank::*` block into `out`, returns its word count |
//! | operate | `run_operation_into(&self, op: u32, args: &[f64], out: &mut Block) -> Result<(), Fault>` | a body from the generated operation table (`OPERATION_*`); `args` is its scalars then its call record; the result replaces `out`'s words |
//! | operate, owned | `run_operation(&self, op: u32, args: &[f64]) -> Result<Block, Fault>` | the same into a new block |
//! | replay admit | `world_replay_admit(&self, &Value) -> Result<WorldTickSetup, Error>` | harness only: recorded options and cold assets |
//! | replay prepare | `world_replay_prepare(&self, &Value) -> Result<(), Error>` | harness only: one frame's input, fire and mount commands |
//! | replay publish | `world_replay_publish(&self, &Value, &[WorldTickOutput]) -> Result<Value, Error>` | harness only: ACKs, then the observation |
//! | live handles | `world_replay_live_handles(&self) -> usize` | diagnostic; readable after disposal |
//! | panic | `world_replay_panic(&self) -> Result<(), Error>` | diagnostic; poisons the context and fails Poisoned |
//! | provided | `provided() -> Provided` | which entries have bodies, for a host's handshake flags |
//!
//! Errors: `Disposed` after `dispose`, `Poisoned` after an escaped panic
//! (disposal still succeeds), `Busy` while another entry is active,
//! `Unavailable` while an entry has no native body yet (its message names
//! the entry). Nothing here answers with a made-up value: an entry without
//! a body fails.
#[path = "context_publish.rs"]
mod publish;
pub use publish::{bank, Fault, Publication};
#[path = "context_replay.rs"]
mod replay;

use super::context::{Context, Error, ErrorKind};
use crate::gen::{WorldTickInput, WorldTickOutput, WorldTickSetup};
use crate::world::owner::World;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The stable refusal of an entry whose body has not landed. Its message
/// is the entry's name, so a host can tell which call it reached.
pub(crate) fn unavailable(entry: &'static str) -> Error {
    Error::new(ErrorKind::Unavailable, entry)
}

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
    pub(crate) fn owned<T>(&self, run: impl FnOnce(&mut World) -> T) -> Result<T, Error> {
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
}
