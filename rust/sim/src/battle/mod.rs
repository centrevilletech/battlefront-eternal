//! The battle (plan P12): command posts and the command vehicles' posts,
//! destructible posts' kill regions, world regions, the battle's rules and
//! scores, leaving the battlefield, the Classic spawn manager. (The live
//! objects follow.)
//!
//! Each module transliterates its JavaScript file (cited by line): the same
//! operations in the same order, the quirks kept. The state is the store's
//! (rust/sim/schema/battle.mjs, `crate::gen`); what the rules ask of the
//! rest of the world goes through [`world::BattleWorld`].

pub mod battlefield;
pub mod mobile;
pub mod names;
pub mod objectives;
pub mod phase;
pub mod posts;
pub mod regions;
pub mod roster;
pub mod rules;
pub mod spawning;
pub mod stats;
pub mod world;
