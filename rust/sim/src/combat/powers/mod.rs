//! Battlefront II (2005) hero powers in the fixed 60 Hz step: Force push and
//! pull, choke, lightning, the aura abilities and the on-foot target lock.
//!
//! Every value comes from the player's prepared weapon records (`def`). A
//! life's offhand trigger runs a small state machine (`cast`) that spends
//! stamina through the same latch as rolls and combo transitions. The world
//! side (`world`) finds targets among live soldiers, tests sight against
//! static geometry, sends damage through the battle's per-target damage
//! service and replaces a pushed soldier's velocity. Received modifiers and
//! choke holds live per life (`effects`). Only a 2005 battle installs any of
//! this; a 2004 battle never reaches it.

#![deny(clippy::disallowed_methods)]

pub mod cast;
pub mod def;
pub mod effects;
pub mod geometry;
pub mod key;
pub mod lock;
pub mod world;

#[cfg(test)]
mod tests;

pub use cast::{PowerInput, PowerKit, PowerState, PowerStep, Request, MAX_POWERS};
pub use def::{from_weapon, Area, AreaEffect, Aura, Blast, Buff, Cone, Cost, Effect, PowerDef, PowerDefError, Relations, Trigger};
pub use effects::{EffectTable, Held, Received, Timed};
pub use geometry::{Footing, Reaction};
pub use key::LifeKey;
pub use lock::LockState;
pub use world::{drain_published_in, effects_in, finish_tick_in, forward_in, held_in, install_in, play_explosion_in, powers_in, reserve_in, Forward, NoHost, PowerEvent, PowerEventKind, PowerFrame, PowerHit, PowerHost, PowerLog, Powers, PublishedPower, UnitPowerPose};
