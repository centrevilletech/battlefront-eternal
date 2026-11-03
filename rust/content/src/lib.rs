//! The game's content as the world simulation reads it, in Rust: every file
//! a battle loads parsed as the JavaScript parses it, and every number the
//! JavaScript derives from the files at load (an item's radius and pickup
//! centre, a weapon's muzzle, a droideka's barrel tips, a hull's muzzles and
//! wheels, clip lengths, the terrain grid, built regions, ODF numbers) made
//! the same way, to the last bit. The simulation's Rust reads its
//! definitions from here.
//!
//! The proof is the canonical dump (src/dump.rs): tools/content-dump.mjs
//! writes one from the JavaScript loaders themselves, the content-dump
//! binary writes one from this crate, and the two must be identical for
//! every map, battle and side.
//!
//! Plain Rust structures; nothing here knows the simulation's entity store.
//! A host hands files over by path (src/files.rs) and asks for a battle's
//! content (src/battle.rs).

pub mod actor;
pub mod battle;
pub mod battle_dump;
pub mod bin_arrays;
pub mod catalog;
pub mod clips;
pub mod collision;
pub mod content;
pub mod derived;
pub mod dump;
pub mod files;
pub mod js;
pub mod json;
pub mod mission;
pub mod models;
pub mod odf;
pub mod regions;
pub mod three;
pub mod value;

pub use value::{Object, Value};

/// src/soldier.js ENGINE.diveLength: the share of the dive clip a dive
/// lasts (units.js buildBody: diveTime).
pub const DIVE_LENGTH: f64 = 0.75;
