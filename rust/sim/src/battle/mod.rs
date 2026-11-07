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
pub(crate) mod f4;
pub(crate) mod f4_live;
pub(crate) mod f4_core;
pub mod f4_factory;
pub(crate) mod f4_tail;
pub(crate) mod f4_roster;
pub(crate) mod f4_connections;
pub(crate) mod native_lifecycle;
pub(crate) mod native_human;
pub mod commands;
pub mod core;
pub mod animated;
pub mod debris;
pub mod debris_commands;
pub mod fleet_posts;
pub mod held_bodies;
pub mod human;
pub mod human_owned;
pub mod human_posts;
pub mod human_world;
pub mod live;
pub mod live_phase;
pub mod mobile;
pub mod names;
pub mod objectives;
pub mod observation;
pub mod ownership;
pub mod owned_world;
pub mod native_actions;
pub mod publication;
pub mod prepared;
pub mod phase;
pub mod posts;
pub mod props;
pub mod regions;
pub mod registry;
pub mod roster;
pub mod rules;
pub mod spawning;
pub mod stat_records;
pub mod stats;
pub mod startup;
pub mod supply;
pub mod tail;
pub mod traps;
pub mod turrets;
pub mod world;

pub(crate) mod owner;
