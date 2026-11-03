//! A soldier's side of the fight (src/combat.js, src/weapons.js,
//! src/weapon-targeting.js, src/combat-weapon-input.js, src/combat-input.js,
//! src/runtime/network-shot-aim.js and src/heroes.js's swing and
//! regeneration):
//!
//! - `defs`: the weapon and unit classes combat reads, with JavaScript's
//!   own rules for fields the content leaves out;
//! - `weapons`: a weapon's timers, salvos, throws, charges, spread and
//!   lock, and a soldier's two channels; shared with seats and turrets;
//! - `combat`: a world's combat (a bot's or a network player's): its aim,
//!   triggers, fire paths, the arc caster, a repair tool's lock, the kick,
//!   blasts, damage, death and a droid's blowing apart;
//! - `world`: what combat asks of the rest of the battle, the boundary of
//!   src/combat-world.js, which the replays serve from captures.
//!
//! Every function is its JavaScript's operation by operation, with V8's
//! Math (`v8math`), JavaScript's conversions and sort (`jsrt`) and three.js
//! r170's vector arithmetic (`three`); each cites the JavaScript it ports.

#[allow(clippy::module_inception)]
pub mod combat;
pub mod defs;
pub mod weapons;
pub mod world;
