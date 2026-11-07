//! The Classic AI in vehicles (src/ai/classic/vehicles/*): what a bot does
//! in a seat and about vehicles and turrets, to the last bit of the
//! JavaScript.
//!
//! - `service`: the VehicleAI service (index.js): the seats' queries, the
//!   boarding rules and reservations, mounted reach, the patrol's mounted
//!   branch and fire support, decay of unattended hulls, and the explicit
//!   entry points of what adaptClassicAI patches into a ClassicAI;
//! - `steering`: a seat's input step per family (steering.js), with the
//!   tow-cable run's request (`orbit`, orbit.js);
//! - `patterns`: the dispatch and attack patterns only vehicles run
//!   (patterns.js);
//! - `agents`: boarding, leaving and the flyer pilot's objective
//!   (agents.js), on the AI control core's agent machine; `fly`: the flyer
//!   movement helper (fly.js) on navigation's;
//! - `controllers`: the mounted controllers' policies (controllers.js);
//! - `reservations`, `notify`, `support`: seat reservations, the vehicle
//!   notifier, fire support for a post out of a hull's reach.
//!
//! Everything the vehicle AI reads of the battle and asks of it goes
//! through [`world::World`] (and the agents' and flyer helper's
//! [`agents::AgentHost`] and [`fly::FlyHost`]): views of the objects it
//! reads, its own state kept on them, and every call across its boundary
//! in the JavaScript's order, which src/ai/classic/vehicles/world.js and
//! the live objects' methods name. Generator A's draws come through
//! [`random::Draws`]. Math is V8's (rust/v8math) and three.js r170's
//! (rust/three), operation by operation.

// `!(a < b)` is JavaScript's own spelling here (NaN fails both), kept as
// written; kinds and states are numbers matched by guards, as float
// patterns cannot be.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::redundant_guards)]

pub mod agents;
pub mod controllers;
pub mod fly;
pub mod notify;
pub mod orbit;
pub mod patterns;
pub mod random;
#[cfg(feature = "replay")]
pub mod replay;
pub mod reservations;
pub mod service;
pub mod steering;
pub mod support;
pub mod world;

pub use service::VehicleAI;
pub use world::World;

pub mod support_key;
