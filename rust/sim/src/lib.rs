//! The world simulation's WebAssembly module, `bfe_sim`: one module, loaded
//! once a thread and instantiated once a battle (src/sim/module.js), so the
//! world worker, a page and the dedicated server each hold one copy of what
//! the simulation's Rust needs, in one memory.
//!
//! - `collision`: the collision world's queries (casts, gathers, overlaps,
//!   ground, terrainNear, a hull's wall probes), src/ray-kernel.js's module;
//! - `movement`: the soldiers' movement (`Soldier.tick`), src/movement-kernel.js's;
//! - `pose`: the pose sampler of src/anim.js, src/anim-kernel.js's.
//!
//! Every function answers as the JavaScript it stands for does, to the last
//! bit; each module says how. The exports are plain `extern "C"` functions
//! (no wasm-bindgen), named as each bridge calls them.

pub mod collision;
pub mod movement;
pub mod pose;
