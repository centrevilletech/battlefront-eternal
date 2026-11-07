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

// The JavaScript's own spellings are kept (`!(a < b)` where NaN must take
// the other branch) and its functions' arguments as they are.
#[allow(clippy::module_inception, clippy::neg_cmp_op_on_partial_ord, clippy::too_many_arguments, clippy::needless_late_init)]
pub mod combat;
pub mod defs;
pub mod services;
pub mod impacts;
pub mod history;
pub mod owners;
pub mod targeting;
// Native construction and service entry points for the owning unit phase.
#[allow(clippy::too_many_arguments)]
pub mod module;
pub(crate) mod live;
#[allow(clippy::neg_cmp_op_on_partial_ord, clippy::too_many_arguments)]
pub mod weapons;
pub mod world;
