//! The whole-world replay driver's entries (docs/world-replay.md, "Native
//! driver contract"): the harness feeds a recorded corpus through the same
//! owners the page uses, and these adapters admit, prepare and observe one
//! frame at a time. Harness only: a game host never calls them.
use super::super::context::{Context, Error};
use super::unavailable;
use crate::gen::{WorldTickOutput, WorldTickSetup};
use serde_json::Value;

impl Context {
    /// Admits the recorded options, the decoded cold assets, the local
    /// player and the publication host, and answers with the tick setup the
    /// recording made for the battle.
    pub fn world_replay_admit(&self, admission: &Value) -> Result<WorldTickSetup, Error> {
        self.check()?;
        if !crate::world::admit::provided() { return Err(unavailable("world_replay_admit")); }
        let (files, options) = super::admit::recorded(admission)?;
        Ok(self.world_admit(&files, &options)?.setup)
    }

    /// Applies one frame's recorded input, fire and mount commands and
    /// prepares the native owners' input banks for its ticks.
    pub fn world_replay_prepare(&self, frame: &Value) -> Result<(), Error> {
        self.check()?;
        let _ = frame;
        Err(unavailable("world_replay_prepare"))
    }

    /// Applies the frame's recorded ACKs at their post-step slot and answers
    /// with the observation `{prefix, tick, stateJson, digestJson, packets}`
    /// in the observers' field order.
    pub fn world_replay_publish(&self, frame: &Value, outputs: &[WorldTickOutput]) -> Result<Value, Error> {
        self.check()?;
        let _ = (frame, outputs);
        Err(unavailable("world_replay_publish"))
    }

    /// How many native resources the context still holds: the owning world
    /// until disposal (the pools it holds are counted once their admission
    /// lands). Readable after disposal, when it must be zero.
    pub fn world_replay_live_handles(&self) -> usize {
        usize::from(self.live().is_ok())
    }

    /// Panics inside an owning entry on purpose, proving the poison path:
    /// the panic is caught here, the context is poisoned and the answer is
    /// Poisoned, as every later entry's will be.
    pub fn world_replay_panic(&self) -> Result<(), Error> {
        self.owned(|_| panic!("replay harness panic"))
    }
}
