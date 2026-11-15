//! The Classic AI in vehicles (src/ai/classic/vehicles/*): what a bot does
//! in a seat and about vehicles and turrets, to the last bit of the
//! JavaScript.

// `!(a < b)` is JavaScript's own spelling here (NaN fails both), kept as
// written; kinds and states are numbers matched by guards, as float
// patterns cannot be.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::redundant_guards)]

pub mod agents;
pub mod controllers;
pub mod notify;
pub mod orbit;
pub mod patterns;
pub mod random;
pub mod reservations;
pub mod service;
pub mod steering;
pub mod support;
pub mod world;

pub use service::VehicleAI;
pub use world::World;

pub mod support_key;
