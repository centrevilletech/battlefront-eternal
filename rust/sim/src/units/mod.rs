//! The battle's units (src/units.js, src/registry.js, src/soldier.js,
//! src/droideka.js, src/collision-damage.js and src/game.js's
//! separateSoldiers): every soldier on the field, his tick, the registry
//! that answers who is near a point and what a segment passes through, and
//! the crowd's push.
//!
//! - `shapes`: the hit shapes a soldier or a droideka is struck through,
//!   their broadphase sphere and the segment tests against each kind;
//! - `registry`: the registry's members in its order, the teams' relations,
//!   `query` and `raycast`;
//! - `unit`: a unit (its life, intent, controls and hit shapes) and its tick;
//! - `droideka`: a droideka's body, its modes, walk, ball and collision;
//! - `damage`: what hitting the ground or a wall too hard does
//!   (collision damage) and the world's bounds;
//! - `separate`: the soldiers pushed apart, every pair.
//!
//! Units reach the rest of the battle (the collision world, combat, the AI,
//! seats and props, the Characters) through `world::UnitsWorld`, the
//! boundary the replay harness serves from captures until those subsystems
//! are Rust's. Every function is its JavaScript's operation by operation,
//! with V8's Math (`v8math`) and three.js r170's vector arithmetic
//! (`crate::rig::three`); each cites the JavaScript it ports.

pub mod damage;
pub mod droideka;
pub mod registry;
pub mod separate;
pub mod shapes;
pub mod unit;
pub mod world;
