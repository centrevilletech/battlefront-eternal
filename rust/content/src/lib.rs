//! The game's content as the world simulation reads it, in Rust: every file
//! a battle loads parsed as the JavaScript parses it, and every number the
//! JavaScript derives from the files at load (an item's radius and pickup
//! centre, a weapon's muzzle, a droideka's barrel tips, a hull's muzzles and
//! wheels, clip lengths, the terrain grid, built regions, ODF numbers) made
//! the same way, to the last bit. The simulation's Rust reads its
//! definitions from here.

pub mod actor;
pub mod battle;
pub mod bin_arrays;
pub mod catalog;
pub mod clips;
pub mod collision;
pub mod content;
pub mod defs;
pub mod derived;
pub mod dump;
pub mod files;
pub mod gear;
pub mod intern;
pub mod bvh;
pub mod install;
pub mod js;
pub mod json;
pub mod mission;
pub mod models;
pub mod odf;
pub mod regions;
pub mod registry;
pub mod three;
pub mod value;

pub use value::{Object, Value};

/// ENGINE.diveLength: the share of the dive clip a dive
/// lasts (buildBody: diveTime).
pub const DIVE_LENGTH: f64 = 0.75;
