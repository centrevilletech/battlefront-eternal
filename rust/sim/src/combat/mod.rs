//! A soldier's side of the fight (src/combat.js, src/weapons.js,
//! and src/heroes.js's swing and
//! regeneration):

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
pub mod page_targeting;
// Native construction and service entry points for the owning unit phase.
#[allow(clippy::too_many_arguments)]
pub mod module;
pub(crate) mod live;
#[allow(clippy::neg_cmp_op_on_partial_ord, clippy::too_many_arguments)]
pub mod weapons;
pub mod world;
