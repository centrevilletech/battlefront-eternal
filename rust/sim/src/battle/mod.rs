//! The battle (plan P12): command posts and the command vehicles' posts,
//! destructible posts' kill regions, world regions, the battle's rules and
//! scores, leaving the battlefield, the Classic spawn manager, and log
//! trap timing and crushing.
//!
//! Each module transliterates its JavaScript file (cited by line): the same
//! operations in the same order, the quirks kept. Battle and spawn state
//! use the store (rust/sim/schema/battle.mjs, `crate::gen`); what the rules
//! ask of the rest of the world goes through [`world::BattleWorld`]. The
//! trap state machine has its own [`traps::World`] for hulls and effects.

pub mod battlefield;
#[cfg(target_arch = "wasm32")]
pub mod battlefield_module;
pub mod animated;
#[cfg(target_arch = "wasm32")]
pub mod animated_module;
pub mod human;
#[cfg(target_arch = "wasm32")]
pub mod crews_module;
pub mod live;
#[cfg(target_arch = "wasm32")]
pub mod live_module;
#[cfg(target_arch = "wasm32")]
pub mod human_module;
#[cfg(target_arch = "wasm32")]
pub mod rules_module;
#[cfg(target_arch = "wasm32")]
pub mod posts_module;
#[cfg(target_arch = "wasm32")]
pub mod spawning_module;
#[cfg(target_arch = "wasm32")]
pub mod traps_module;
pub mod mobile;
pub mod names;
pub mod objectives;
pub mod phase;
pub mod posts;
pub mod props;
#[cfg(target_arch = "wasm32")]
pub mod props_module;
pub mod regions;
pub mod roster;
#[cfg(target_arch = "wasm32")]
pub mod roster_module;
pub mod rules;
pub mod spawning;
pub mod stats;
pub mod traps;
pub mod turrets;
#[cfg(target_arch = "wasm32")]
pub mod turrets_module;
pub mod world;
