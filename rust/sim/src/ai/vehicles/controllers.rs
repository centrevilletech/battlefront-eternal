//! The mounted controllers (src/ai/classic/vehicles/controllers.js): the
//! 2004 game gives every occupied control its own controller, and they
//! differ only in which source goals they turn into which states, and in
//! the flyer's objective agent.
//!
//! - UnitMobileController (hover and walker pilots): 1..6 to the same
//!   states, 8 to 9, 9 to 10, 10 to 11;
//! - UnitStationaryController (gunner and passenger seats, turrets): 1 and
//!   3 to Wait, 2 to Combat, 6 to Unboard; its wait looks round +-30
//!   degrees when the control belongs to a building (a turret);
//! - UnitFlyController (flyer pilots): 2 to Combat, 3 to the flyer agent's
//!   state 7, 4 to the crew wait, 10 to Hold (11).
//!
//! The Classic AI's controller consults the policy when its unit is a
//! control: the explicit form of what index.js adaptClassicAI patches in
//! (the service's adaptController, mountedState and agentFor).

use super::world::UnitView;

/// The terminal state every machine uses (controller.js DONE).
pub const DONE: f64 = 100.0;

/// controller.js ROLE: the support and pilot roles the boarding policy
/// keeps out of turrets, and the role of a class with none.
pub const ROLE_SUPPORT: f64 = 3.0;
pub const ROLE_PILOT: f64 = 4.0;
pub const ROLE_OTHER: f64 = 6.0;

/// roleOf(classDef) (controller.js:104): the class's `role` when it is an
/// integer, else its UnitType (or unitType) by name.
pub fn role_of(role: Option<f64>, unit_type: Option<&str>) -> f64 {
    if let Some(r) = role {
        if r.is_finite() && v8math::trunc(r) == r {
            return r;
        }
    }
    let Some(t) = unit_type else { return ROLE_OTHER };
    match t.trim_matches(bfe_content::js::is_js_space).to_lowercase().as_str() {
        "scout" => 0.0,
        "anti-armor" => 1.0,
        "assault" => 2.0,
        "support" | "repair" | "special" => 3.0,
        "pilot" => 4.0,
        "trooper" => 5.0,
        _ => ROLE_OTHER,
    }
}

/// A mounted policy (controllers.js:191-213).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Mobile,
    Stationary,
    Fly,
}

/// The agent a policy has for its own states (`agentFor`): only the
/// flyer's objective agent, for state 7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountedAgent {
    Fly,
}

impl Policy {
    /// The controller class the 2004 game names (`policy.name`).
    pub fn name(self) -> &'static str {
        match self {
            Policy::Mobile => "UnitMobileController",
            Policy::Stationary => "UnitStationaryController",
            Policy::Fly => "UnitFlyController",
        }
    }

    /// goalState(kind) (controllers.js:187-189): the state a source goal
    /// starts, 0 for none.
    pub fn goal_state(self, goal: f64) -> f64 {
        let table: &[(f64, f64)] = match self {
            Policy::Mobile => &[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0), (5.0, 5.0), (6.0, 6.0), (8.0, 9.0), (9.0, 10.0), (10.0, 11.0)],
            Policy::Stationary => &[(1.0, 1.0), (3.0, 1.0), (2.0, 2.0), (6.0, 6.0)],
            Policy::Fly => &[(2.0, 2.0), (3.0, 7.0), (4.0, 4.0), (10.0, 11.0)],
        };
        // Map.get (SameValueZero over numbers).
        table.iter().find(|(k, _)| *k == goal).map_or(0.0, |&(_, s)| s)
    }

    /// agentFor(state, controller) (controllers.js:211).
    pub fn agent_for(self, state: f64) -> Option<MountedAgent> {
        if self == Policy::Fly && state == 7.0 {
            Some(MountedAgent::Fly)
        } else {
            None
        }
    }

    /// bind(controller, unit): the home-facing flag a stationary policy
    /// sets (`controller.homeFacing = Boolean(unit.turret)`); None for the
    /// others, which set nothing.
    pub fn home_facing(self, unit: &UnitView) -> Option<bool> {
        match self {
            Policy::Stationary => Some(unit.turret.is_some()),
            _ => None,
        }
    }
}

/// controllerPolicy(unit) (controllers.js:217): none for a soldier on foot.
pub fn controller_policy(unit: Option<&UnitView>) -> Option<Policy> {
    let unit = unit?;
    if unit.seat.is_none() || unit.human || unit.lease.is_none() {
        return None;
    }
    if unit.kind == 5.0 {
        return Some(Policy::Fly);
    }
    if unit.kind == 7.0 {
        return Some(Policy::Stationary);
    }
    Some(Policy::Mobile)
}

/// index.js MOBILE_GOAL: the goal kind a mobile controller's state came from.
fn mobile_goal(state: f64) -> Option<f64> {
    const TABLE: [(f64, f64); 9] = [(1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0), (5.0, 5.0), (6.0, 6.0), (9.0, 8.0), (10.0, 9.0), (11.0, 10.0)];
    TABLE.iter().find(|(k, _)| *k == state).map(|&(_, g)| g)
}

/// mountedState(controller, state) (index.js): the state a controller's
/// request becomes under its policy.
pub fn mounted_state(policy: Option<Policy>, state: f64) -> f64 {
    let Some(p) = policy else { return state };
    if state == DONE || state == 0.0 {
        return state;
    }
    match mobile_goal(state) {
        None => state,
        Some(goal) => p.goal_state(goal),
    }
}
