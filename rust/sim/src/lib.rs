//! The world simulation (`bfe_sim`), linked into the client's world thread and
//! the dedicated server: collision queries, soldier movement, pose sampling,
//! vehicles and flight, combat, ordnance, items, AI, the battle rules, the
//! entity store and its generated code (`gen`, from rust/sim/schema via
//! `cargo xtask schema`), and the wire publication. Arithmetic matches the
//! Electron build's JavaScript to the last bit (rust/v8math).

pub mod abi;
mod audio_control;
pub mod ai;
pub mod battle;
pub mod collision;
mod cameras;
pub mod client;
pub mod combat;
pub mod flight;
pub mod gen;
// The authority host's per-tick duties around the native step (native
// only: the WebAssembly host keeps them in JavaScript, and the Context it
// runs on exists only in unwinding native builds).
pub mod host;
// The generator's test schemas' code (rust/sim/schema-test), for its tests.
#[cfg(test)]
mod gen_test;
#[cfg(test)]
mod gen_tests;
pub mod items;
pub mod input;
pub mod movement;
pub mod ordnance;
pub mod players;
pub mod pose;
pub mod prediction;
pub mod recon;
pub mod rig;
pub mod seats;
pub mod store;
pub mod units;
pub mod vehicles;

pub mod wire;
pub mod world;

#[cfg(test)]
extern crate self as bfe_sim;
