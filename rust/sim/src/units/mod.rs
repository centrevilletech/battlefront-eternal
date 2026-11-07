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
//! - `soldier`: a store soldier's movement by the module's own (no exchange
//!   through JavaScript);
//! - `unit`: a unit (its life, intent, controls and hit shapes), its tick
//!   and its spawn;
//! - `body`: what the battle asks of a body between ticks (a player's look,
//!   a droideka's shield and guns);
//! - `droideka`: a droideka's body, its modes, walk, ball and collision;
//! - `class`: a body's numbers from its class (the Soldier and Droideka
//!   constructors);
//! - `damage`: what hitting the ground or a wall too hard does
//!   (collision damage) and the world's bounds;
//! - `factory`: each class's free bodies, taken back last in, first out;
//! - `separate`: the soldiers pushed apart, every pair;
//! - `water`: the map's water and what it does to a soldier each tick;
//! - `module` (the module only): canonical body and combat services for
//!   the required owning phase and generation-qualified lifecycle commands.
//!
//! Units reach the rest of the battle (the collision world, combat, the AI,
//! seats and props, the Characters) through `world::UnitsWorld`, the
//! boundary the replay harness serves from captures until those subsystems
//! are Rust's. Every function is its JavaScript's operation by operation,
//! with V8's Math (`v8math`) and three.js r170's vector arithmetic
//! (rust/three); each cites the JavaScript it ports.

pub mod body;
pub mod class;
pub mod construction;
pub mod damage;
pub mod droideka;
pub mod factory;
#[cfg(target_arch = "wasm32")]
pub mod f3;
#[cfg(target_arch = "wasm32")]
pub(crate) mod f3_construction;
#[cfg(target_arch = "wasm32")]
pub(crate) mod f3_items;
#[cfg(target_arch = "wasm32")]
pub(crate) mod f3_props;
#[cfg(target_arch = "wasm32")]
pub(crate) mod f3_hulls;
#[cfg(target_arch = "wasm32")]
pub(crate) mod f3_fleet;
#[cfg(target_arch = "wasm32")]
pub(crate) mod f3_sensors;
pub mod ownership;
pub mod poses;
#[cfg(target_arch = "wasm32")]
pub mod module;
#[cfg(target_arch = "wasm32")]
pub mod muzzles;
#[cfg(target_arch = "wasm32")]
pub(crate) mod live_pose;
#[cfg(target_arch = "wasm32")]
pub(crate) mod native;
pub mod registry;
pub mod separate;
pub mod shapes;
pub mod soldier;
pub mod unit;
pub mod water;
pub mod world;

pub mod page_body;
#[cfg(target_arch = "wasm32")]
pub(crate) mod owner;
