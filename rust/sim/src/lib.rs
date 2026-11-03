//! The world simulation's WebAssembly module, `bfe_sim`: one module, loaded
//! once a thread and instantiated once a battle (src/sim/module.js), so the
//! world worker, a page and the dedicated server each hold one copy of what
//! the simulation's Rust needs, in one memory.
//!
//! - `collision`: the collision world's queries (casts, gathers, overlaps,
//!   ground, terrainNear, a hull's wall probes), src/ray-kernel.js's module;
//! - `movement`: the soldiers' movement (`Soldier.tick`), src/movement-kernel.js's;
//! - `pose`: the pose sampler of src/anim.js, src/anim-kernel.js's;
//! - `rig`: the skeletons and rigs the world tick reads (a rider's hit
//!   shapes, a droideka's muzzles, a hull's fire and seat nodes);
//! - `store` and `gen`: the entity store, its types and the code
//!   tools/sim-schema.mjs makes from the schemas in rust/sim/schema/;
//! - `abi`: the store's exports and the memory policy.
//!
//! Every function answers as the JavaScript it stands for does, to the last
//! bit; each module says how. The exports are plain `extern "C"` functions
//! (no wasm-bindgen), named as each bridge calls them.

pub mod abi;
pub mod collision;
pub mod gen;
// The generator's test schemas' code (rust/sim/schema-test), for its tests.
#[cfg(test)]
mod gen_test;
#[cfg(test)]
mod gen_tests;
pub mod movement;
pub mod pose;
pub mod rig;
pub mod store;
