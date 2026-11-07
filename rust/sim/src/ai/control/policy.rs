//! Agent acceptance policies shared by the control core and native combat
//! callbacks. Inputs contain AI state only; world facts are observed on demand.
use super::{
    agents::{AgentKind, Interrupts, ObjectiveFlavor},
    controller::{Controller, EnhancedMemory},
    vec as v,
    world::{ResponseState, UKey},
};
use crate::ai::nav::math::Vec3;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Other,
    WaitSecondary,
    Combat,
    Follow,
    Hold,
    Board,
    Unboard,
    Objective,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub kind: Kind,
    pub unit: Option<UKey>,
    pub controller_unit: Option<UKey>,
    pub has_agent: bool,
    pub state: f64,
    pub repair: bool,
    pub enhanced: bool,
    pub helper: u8,
    pub patrol_moving: bool,
    pub patrol_post: Option<usize>,
    pub snipe_full: bool,
    pub snipe_hint: Option<usize>,
    pub snipe_ready: bool,
}
impl Input {
    pub fn read(c: &Controller) -> Self {
        let mut out = Self {
            controller_unit: c.unit,
            ..Self::default()
        };
        let Some(agent) = &c.agent else { return out };
        out.has_agent = true;
        out.unit = agent.unit;
        out.state = agent.fsm.state;
        out.kind = match &agent.kind {
            AgentKind::WaitSecondary => Kind::WaitSecondary,
            AgentKind::Follow { .. } => Kind::Follow,
            AgentKind::Hold { .. } => Kind::Hold,
            AgentKind::Board { .. } => Kind::Board,
            AgentKind::Unboard => Kind::Unboard,
            // CombatAgent.acceptsTarget always refuses: its own tracker
            // already selects candidates. Replacing it on every sighting
            // would restart its pattern before it can consume fire.
            AgentKind::Foreign(super::world::ForeignKind::Combat)=>Kind::Combat,
            AgentKind::Foreign(super::world::ForeignKind::Board)=>Kind::Board,
            AgentKind::Foreign(super::world::ForeignKind::Unboard)=>Kind::Unboard,
            AgentKind::Objective(o) => {
                out.repair = o.flavor == ObjectiveFlavor::Repair;
                out.enhanced = o.enhanced;
                out.helper = if out.repair {
                    if out.state == 4. {
                        1
                    } else if out.state == 5. {
                        2
                    } else if out.state == 6. {
                        3
                    } else {
                        0
                    }
                } else {
                    if out.state == 2. {
                        1
                    } else if out.state == 4. {
                        2
                    } else if out.state == 5. {
                        3
                    } else {
                        0
                    }
                };
                out.patrol_moving = o.patrol.fsm.state == 2.;
                out.patrol_post = o.patrol.post;
                out.snipe_full = o.snipe.full;
                out.snipe_hint = o.snipe.hint;
                out.snipe_ready = o.snipe.fsm.state == 3.;
                Kind::Objective
            }
            _ => Kind::Other,
        };
        out
    }
    fn approach(self) -> f64 {
        if self.repair {
            2.
        } else {
            3.
        }
    }
    fn on_run(self) -> bool {
        self.state == self.approach() || (self.helper == 1 && self.patrol_moving)
    }
    fn capturing(self, w: &dyn Scene) -> bool {
        self.helper == 1
            && self.patrol_moving
            && self
                .unit
                .zip(self.patrol_post)
                .is_some_and(|(u, p)| w.post_contains(p, w.position(u)))
    }
    fn forward_cone(self, w: &dyn Scene, point: Vec3) -> bool {
        if !self.snipe_full {
            return false;
        }
        let (Some(unit), Some(hint)) = (self.unit, self.snipe_hint) else {
            return false;
        };
        v::dot(
            v::normalize(v::sub(point, w.position(unit))),
            v::normalize(w.hint_forward(hint)),
        ) > 0.21643958985805511
    }
}
pub trait Scene {
    fn time(&self) -> f64;
    fn kind(&self, unit: UKey) -> f64;
    fn position(&self, unit: UKey) -> Vec3;
    fn identity(&self, unit: UKey) -> (u64, u64);
    fn post_contains(&self, post: usize, position: Vec3) -> bool;
    fn hint_forward(&self, hint: usize) -> Vec3;
    fn can_engage(&mut self, target: UKey) -> bool;
    fn board_policy(&mut self, candidate: UKey) -> bool;
    fn near_fighting(&mut self, unit: UKey) -> f64;
    fn emit_charge(&mut self, unit: UKey);
    fn new_memory(&mut self) -> EnhancedMemory;
}
/// interrupts.js charging: its time cache and the first three fighting allies.
pub fn charging(input: Input, state: &mut Interrupts, w: &mut dyn Scene) -> bool {
    if !input.has_agent {
        return false;
    }
    let now = w.time();
    if now <= state.checked_at {
        return state.charge;
    }
    state.charge = false;
    state.checked_at = now;
    let Some(unit) = input.controller_unit else {
        return false;
    };
    if w.kind(unit) != 0. {
        return false;
    }
    if w.near_fighting(unit) == 3. {
        w.emit_charge(unit);
        state.charge = true;
        return true;
    }
    false
}
fn charging_now(input: Input, state: &mut ResponseState, w: &mut dyn Scene) -> bool {
    input.state == input.approach() && charging(input, &mut state.interrupts, w)
}
pub fn accepts_hazard(input: Input, state: &mut ResponseState, w: &mut dyn Scene) -> bool {
    match input.kind {
        Kind::WaitSecondary => {
            if input.unit.is_some_and(|u| w.kind(u) == 5.) {
                state.secondary_wait_until = 0.;
                false
            } else {
                true
            }
        }
        Kind::Follow => false,
        Kind::Objective => {
            if matches!(input.helper, 1 | 2 | 3) {
                true
            } else {
                !charging_now(input, state, w)
            }
        }
        _ => true,
    }
}
pub fn accepts_alert(
    input: Input,
    state: &mut ResponseState,
    w: &mut dyn Scene,
    known: bool,
    point: Vec3,
) -> bool {
    match input.kind {
        Kind::WaitSecondary => {
            if input.unit.is_some_and(|u| w.kind(u) == 5.) {
                state.secondary_wait_until = 0.;
                false
            } else {
                true
            }
        }
        Kind::Follow => false,
        Kind::Objective => {
            if input.enhanced {
                if let Some(unit) = input.unit {
                    if input.on_run() {
                        let near = super::agents::horizontal_distance_sq(point, w.position(unit))
                            < 25. * 25.;
                        if input.capturing(w) || !near {
                            let until = w.time() + 2.5;
                            let memory = state.enhanced.get_or_insert_with(|| w.new_memory());
                            memory.look_point = point;
                            memory.look_until = until;
                            return false;
                        }
                    }
                }
            }
            match input.helper {
                1 | 3 => true,
                2 => !(input.snipe_ready && known && input.forward_cone(w, point)),
                _ => !charging_now(input, state, w),
            }
        }
        _ => true,
    }
}
pub fn accepts_target(
    input: Input,
    state: &mut ResponseState,
    w: &mut dyn Scene,
    target: UKey,
) -> bool {
    match input.kind {
        Kind::WaitSecondary => {
            if input.unit.is_some_and(|u| w.kind(u) == 5.) {
                false
            } else {
                w.can_engage(target)
            }
        }
        Kind::Follow|Kind::Combat => false,
        Kind::Objective => {
            if input.enhanced {
                if let Some(unit) = input.unit {
                    if input.on_run()
                        && (input.capturing(w)
                            || !(super::agents::horizontal_distance_sq(
                                w.position(target),
                                w.position(unit),
                            ) < 20. * 20.))
                    {
                        return false;
                    }
                }
            }
            match input.helper {
                1 | 3 => w.can_engage(target),
                2 => {
                    if !input.snipe_ready {
                        return w.can_engage(target);
                    }
                    if input.forward_cone(w, w.position(target)) {
                        return false;
                    }
                    let Some(unit) = input.unit else { return false };
                    if v::distance_sq(w.position(unit), w.position(target)) >= 1600. {
                        return false;
                    }
                    w.can_engage(target)
                }
                _ => {
                    if input.repair {
                        let Some(unit) = input.unit else { return false };
                        if super::agents::horizontal_distance_sq(
                            w.position(unit),
                            w.position(target),
                        ) >= 625.
                        {
                            return false;
                        }
                    }
                    if charging_now(input, state, w) {
                        return false;
                    }
                    w.can_engage(target)
                }
            }
        }
        _ => w.can_engage(target),
    }
}
fn objective_acoustic(
    input: Input,
    state: &mut ResponseState,
    w: &mut dyn Scene,
    source: UKey,
) -> bool {
    match input.helper {
        1 | 3 => w.can_engage(source),
        2 => {
            if !input.snipe_ready {
                w.can_engage(source)
            } else {
                !input.forward_cone(w, w.position(source)) && w.can_engage(source)
            }
        }
        _ => {
            if charging_now(input, state, w) {
                false
            } else {
                w.can_engage(source)
            }
        }
    }
}
pub fn accepts_acoustic(
    input: Input,
    state: &mut ResponseState,
    w: &mut dyn Scene,
    source: UKey,
) -> bool {
    match input.kind {
        Kind::WaitSecondary => {
            if input.unit.is_some_and(|u| w.kind(u) == 5.) {
                state.secondary_wait_until = 0.;
                false
            } else {
                w.can_engage(source)
            }
        }
        Kind::Follow => false,
        Kind::Objective => {
            if input.enhanced {
                let Some(unit) = input.unit else { return false };
                if input.on_run()
                    && (input.capturing(w)
                        || !(super::agents::horizontal_distance_sq(
                            w.position(source),
                            w.position(unit),
                        ) < 12. * 12.))
                {
                    return false;
                }
                let key = w.identity(source);
                let now = w.time();
                let recent = state.enhanced.as_ref().and_then(|m| m.heard_get(key));
                if now - recent.unwrap_or(f64::NEG_INFINITY) < 10. {
                    return false;
                }
                let accepted = objective_acoustic(input, state, w, source);
                if accepted {
                    let memory = state.enhanced.get_or_insert_with(|| w.new_memory());
                    memory.heard_set(key, now);
                    memory.heard_prune(now, 10.);
                }
                accepted
            } else {
                objective_acoustic(input, state, w, source)
            }
        }
        _ => w.can_engage(source),
    }
}
pub fn accepts_board(input: Input, w: &mut dyn Scene, candidate: UKey) -> bool {
    match input.kind {
        Kind::WaitSecondary | Kind::Follow | Kind::Hold | Kind::Board | Kind::Unboard => false,
        Kind::Objective => match input.helper {
            1 => false,
            2 => {
                if input.snipe_ready {
                    false
                } else {
                    w.board_policy(candidate)
                }
            }
            3 => true,
            _ => w.board_policy(candidate),
        },
        _ => w.board_policy(candidate),
    }
}
