//! Goal bots: bots for the 2005 game's modes, which steer bots by goals a
//! mission script gives each team (find and fight enemies, defend or destroy
//! an object, follow someone, capture a flag, take command posts).
//!
//! A battle gets the goal bots only when the rules installed for it supply a
//! goal table ([`GoalTable`]); a 2004 battle never constructs them, and the
//! released bot controller runs unchanged for every battle. The goal bots
//! live inside the installed rules; every disposal of those rules hands their
//! bots back to the released controller first. Once per rules tick they link
//! bots to goals ([`assign`]) and steer them through the controller's goal
//! operations ([`crate::ai::control::goal_orders`], [`controller`]). See
//! docs/ai-goals.md.

pub mod api;
pub mod assign;
pub mod census;
pub mod controller;
pub mod melee;
pub mod reach;
pub mod regions;
pub mod table;

pub use controller::{GoalController, Resolved, SoldierInputs, Subject};
pub use melee::{BlockMode, MeleeInput};
pub use regions::GoalRegions;
pub use table::{accepts_target, GoalArg, GoalHandles, GoalKind, GoalRow, GoalSlots, GoalTable, NoGoals, StoredArg, GOAL_SLOTS, GOAL_TEAMS, MAX_GOAL_HANDLE, NO_GOALS};

#[cfg(test)]
pub(crate) mod tests;
