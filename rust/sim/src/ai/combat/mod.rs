//! The Classic AI's perception, combat and aim (src/ai/classic/perception.js,
//! combat.js, aim.js, weapons-ai.js) and the Enhanced aim
//! (src/ai/enhanced/aim.js): what a bot notices, which target and weapon it
//! takes, how it fights (the combat agent, its attack patterns and cover),
//! how its gun is aimed and its trigger pressed.
//!
//! The control core (the scheduler, the controllers, the other agents)
//! calls these through [`binding::CombatAi`], the services the JavaScript
//! binds into `ai.combat`, and runs the combat agent
//! ([`agent::CombatAgent`]) when a controller is in combat. Everything
//! these reach outside their own state is [`world::World`].

pub mod agent;
pub mod aim;
pub mod binding;
pub mod control;
pub mod enhanced;
pub mod math;
pub mod perception;
pub mod weapons_ai;
pub mod world;

#[cfg(feature = "replay")]
pub mod replay;
