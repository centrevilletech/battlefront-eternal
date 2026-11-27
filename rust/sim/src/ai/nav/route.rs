//! Navigation assets held for the admitted battle.
use super::{
    swbf2::{self, Swbf2Nav},
    Nav, PlannerOptions,
};
use bfe_content::Value;
use std::ops::{Deref, DerefMut};

/// What the navigation service established for this body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteStatus {
    Reachable,
    Unreachable,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteQuery {
    pub status: RouteStatus,
    /// Convenience for callers that use only positive answers.
    pub reachable: bool,
    /// Walkable distance in metres for a positive answer.
    pub distance: f64,
    pub hops: u32,
}

impl RouteQuery {
    pub const UNKNOWN: Self = Self {
        status: RouteStatus::Unknown,
        reachable: false,
        distance: f64::INFINITY,
        hops: 0,
    };
    pub const UNREACHABLE: Self = Self {
        status: RouteStatus::Unreachable,
        ..Self::UNKNOWN
    };

    pub(super) fn found(distance: f64, hops: u32) -> Self {
        Self {
            status: RouteStatus::Reachable,
            reachable: true,
            distance,
            hops,
        }
    }
}

pub struct MapNavigation {
    pub common: Nav,
    pub swbf2: Option<Box<Swbf2Nav>>,
}

impl From<Nav> for MapNavigation {
    fn from(common: Nav) -> Self {
        Self {
            common,
            swbf2: None,
        }
    }
}
impl Deref for MapNavigation {
    type Target = Nav;
    fn deref(&self) -> &Nav {
        &self.common
    }
}
impl DerefMut for MapNavigation {
    fn deref_mut(&mut self) -> &mut Nav {
        &mut self.common
    }
}

impl MapNavigation {
    pub fn new_swbf2(
        json: &Value,
        bin: Option<&[u8]>,
        capacity: Option<f64>,
        pools: PlannerOptions,
    ) -> Result<Self, String> {
        let mut nav = Self::load_swbf2(json, bin, capacity, pools)?;
        let state = nav.swbf2.as_mut().expect("Battlefront II navigation");
        if let Some(graph) = &state.graph {
            state.floors = swbf2::walk::Floors::build(
                graph,
                &mut nav.common.barriers,
                &mut swbf2::walk::Authored,
            )?;
        }
        Ok(nav)
    }

    pub(crate) fn load_swbf2(
        json: &Value,
        bin: Option<&[u8]>,
        capacity: Option<f64>,
        pools: PlannerOptions,
    ) -> Result<Self, String> {
        let data = swbf2::load_swbf2_nav(json, bin, capacity)?;
        let mut common = Nav::empty(pools);
        common.map = data.map;
        common.source = json.clone();
        common.barriers = data.barriers;
        common.hints = data.hints;
        common.paths = data.paths;
        common.boundary = data.boundary;
        common.regions = data.regions;
        let state = Swbf2Nav::new(
            data.graph,
            data.barrier_toggles,
            data.boundary_polygon_count,
        );
        Ok(Self {
            common,
            swbf2: Some(Box::new(state)),
        })
    }
    // ----- 2005 navigation controls -----

    pub fn new_control(&mut self) -> usize {
        let control = self.common.new_control();
        if let Some(s) = &mut self.swbf2 {
            s.reserve_controls(control + 1);
        }
        control
    }

    pub(super) fn clear_order_path(&mut self, control: usize) {
        if let Some(state) = &mut self.swbf2 {
            if let Some(slot) = state.orders.get_mut(control) {
                if let Some((path, _)) = slot.take() {
                    self.common.store.release(path);
                }
                state.held[control] = false;
                let stamp = &mut state.order_stamps[control];
                *stamp = stamp.wrapping_add(1);
            }
        }
    }
    pub(super) fn drop_order(&mut self, control: usize) {
        self.clear_order_path(control);
        if let Some(leg) = self.swbf2.as_mut().and_then(|s| s.legs.get_mut(control)) {
            *leg = None;
        }
    }

    pub fn control_dispose(&mut self, control: usize) {
        self.drop_order(control);
        self.common.control_dispose(control);
    }
    pub(crate) fn observe_collision(&mut self, epoch: (u64, u64), flags: &[u8]) -> bool {
        self.observe_collision_with(epoch, flags, |_| None)
    }
    pub(crate) fn observe_collision_with(
        &mut self,
        epoch: (u64, u64),
        flags: &[u8],
        bounds: impl Fn(usize) -> Option<[f64; 6]>,
    ) -> bool {
        let Some(state) = self.swbf2.as_mut() else {
            return false;
        };
        let previous = state.clearance.revision;
        let mut local = !state.clearance.has_order
            && state.clearance.epoch == Some(epoch)
            && state.clearance.flags.len() == flags.len();
        let mut changes = [[0.; 6]; 16];
        let mut n = 0;
        let mut doors_only = true;
        if local && state.clearance.flags != flags {
            for (i, (old, new)) in state.clearance.flags.iter().zip(flags).enumerate() {
                if old == new {
                    continue;
                }
                doors_only &= state.floors.is_surveyed_door(i);
                let Some(bb) = bounds(i) else {
                    local = false;
                    break;
                };
                if n < changes.len() {
                    changes[n] = bb;
                    n += 1;
                } else {
                    let last = &mut changes[n - 1];
                    for j in 0..3 {
                        last[j] = v8math::min(last[j], bb[j]);
                        last[j + 3] = v8math::max(last[j + 3], bb[j + 3]);
                    }
                }
            }
        }
        let changed = &changes[..n];
        let cached = state.clearance.observe(epoch, flags);
        if previous != 0 && previous != state.clearance.revision {
            if local {
                state.floors.changed_bodies(changed);
                if doors_only {
                    state.floors.restore_open_passages(flags);
                } else {
                    state.floors.forget_open_passages();
                }
            } else {
                state.floors.collision_changed();
            }
            for leg in state.legs.iter_mut().flatten() {
                leg.check_body |= !local
                    || changed
                        .iter()
                        .any(|bb| swbf2::walk::meets(leg.start, leg.end, bb));
            }
            state.revision = state.revision.wrapping_add(1);
        }
        cached
    }
    /// Observe collision-world order and enabled flags, keeping surveyed static
    /// corridors, door passages, and warmed destination rows intact when only
    /// moving bodies join, leave, swap, or toggle.
    pub fn observe_collision_classified(
        &mut self,
        epoch: (u64, u64),
        order: &[u32],
        flags: &[u8],
        is_body_moving: impl Fn(u32) -> bool,
        bounds: impl Fn(usize) -> Option<[f64; 6]>,
    ) -> bool {
        let Some(state) = self.swbf2.as_mut() else {
            return false;
        };
        let previous = state.clearance.revision;
        let same_epoch = state.clearance.has_order
            && state.clearance.epoch == Some(epoch)
            && state.clearance.order == order;
        let fixed_same = state.clearance.has_order
            && state
                .clearance
                .epoch
                .is_some_and(|(_, fixed)| fixed == epoch.1);
        let static_order_same = fixed_same
            && state
                .clearance
                .order
                .iter()
                .enumerate()
                .all(|(i, &b)| is_body_moving(b) || order.get(i) == Some(&b))
            && order
                .iter()
                .enumerate()
                .all(|(i, &b)| is_body_moving(b) || state.clearance.order.get(i) == Some(&b));
        let mut local = same_epoch || static_order_same;
        let mut local_legs = local;
        let mut door_changes = [[0.; 6]; 16];
        let mut door_n = 0usize;
        let mut other_changes = [[0.; 6]; 16];
        let mut other_n = 0usize;
        let mut leg_changes = [[0.; 6]; 16];
        let mut leg_n = 0usize;
        let push_box = |boxes: &mut [[f64; 6]; 16], count: &mut usize, bb: [f64; 6]| {
            if *count < boxes.len() {
                boxes[*count] = bb;
                *count += 1;
            } else {
                let last = &mut boxes[*count - 1];
                for j in 0..3 {
                    last[j] = v8math::min(last[j], bb[j]);
                    last[j + 3] = v8math::max(last[j + 3], bb[j + 3]);
                }
            }
        };
        if local && (state.clearance.order != order || state.clearance.flags != flags) {
            let len = state.clearance.order.len().max(order.len());
            for i in 0..len {
                let old_body = state.clearance.order.get(i).copied();
                let new_body = order.get(i).copied();
                let old_flag = state.clearance.flags.get(i).copied().unwrap_or(0);
                let new_flag = flags.get(i).copied().unwrap_or(0);
                if old_body == new_body && old_flag == new_flag {
                    continue;
                }
                let moving = old_body.is_none_or(&is_body_moving)
                    && new_body.is_none_or(&is_body_moving);
                if moving {
                    if old_body != new_body && old_flag != 0 {
                        local_legs = false;
                    }
                    if new_body.is_some() && (old_flag != 0 || new_flag != 0) {
                        if let Some(bb) = bounds(i) {
                            push_box(&mut leg_changes, &mut leg_n, bb);
                        } else {
                            local_legs = false;
                        }
                    }
                } else {
                    let Some(bb) = bounds(i) else {
                        local = false;
                        local_legs = false;
                        break;
                    };
                    if state.floors.is_surveyed_door(i) {
                        push_box(&mut door_changes, &mut door_n, bb);
                    } else {
                        push_box(&mut other_changes, &mut other_n, bb);
                    }
                    push_box(&mut leg_changes, &mut leg_n, bb);
                }
            }
        }
        let cached = state.clearance.observe_order(epoch, order, flags);
        if previous != 0 && previous != state.clearance.revision {
            if !local {
                state.floors.collision_changed();
                state.revision = state.revision.wrapping_add(1);
            } else if (door_n > 0 || other_n > 0)
                && state
                    .floors
                    .apply_static_changes(&door_changes[..door_n], &other_changes[..other_n])
            {
                state.revision = state.revision.wrapping_add(1);
            }
            if !local_legs || leg_n > 0 {
                let changed = &leg_changes[..leg_n];
                for leg in state.legs.iter_mut().flatten() {
                    leg.check_body |= !local_legs
                        || changed
                            .iter()
                            .any(|bb| swbf2::walk::meets(leg.start, leg.end, bb));
                }
            }
        }
        cached
    }
    fn invalidate_walks(&mut self, matched: bool) -> bool {
        if !matched {
            return false;
        }
        let state = self.swbf2.as_mut().unwrap();
        state.floors.invalidate();
        state.revision = state.revision.wrapping_add(1);
        for (i, slot) in state.legs.iter_mut().enumerate() {
            let Some(leg) = slot else { continue };
            let blocked = leg
                .arc
                .is_some_and(|i| state.graph.as_ref().unwrap().arcs[i].is_blocked());
            if blocked
                || !self
                    .common
                    .barriers
                    .point_clear(leg.destination, leg.radius, leg.mask)
                    .0
                || !swbf2::walk::clear(
                    &mut self.common.barriers,
                    leg.start,
                    leg.end,
                    leg.radius,
                    leg.mask,
                )
            {
                *slot = None;
                state.jump_pending[i] = false;
                self.common.control_stop(i, None);
                self.common.controls[i].request.valid = false;
                self.common.controls[i].jet.pending = false;
            }
        }
        true
    }
    /// Whether this service carries a loaded 2005 planning graph.
    pub fn has_swbf2_graph(&self) -> bool {
        self.swbf2
            .as_ref()
            .and_then(|s| s.graph.as_ref())
            .is_some_and(|g| !g.hubs.is_empty())
    }

    /// Block 2005 dynamic planning arcs matching `name` (`BlockPlanningGraphArcs`).
    pub fn block_planning_arcs(&mut self, name: &str) -> bool {
        let matched = self.swbf2.as_mut().is_some_and(|s| s.block_arcs(name));
        self.invalidate_walks(matched)
    }

    /// Unblock 2005 dynamic planning arcs matching `name` (`UnblockPlanningGraphArcs`).
    pub fn unblock_planning_arcs(&mut self, name: &str) -> bool {
        let matched = self.swbf2.as_mut().is_some_and(|s| s.unblock_arcs(name));
        self.invalidate_walks(matched)
    }

    /// Toggle 2005 dynamic planning arcs matching `name`.
    pub fn toggle_planning_arcs(&mut self, name: &str) -> bool {
        let matched = self.swbf2.as_mut().is_some_and(|s| s.toggle_arcs(name));
        self.invalidate_walks(matched)
    }

    /// Enable or disable 2005 authored barriers matching `name`
    /// (`EnableBarriers` / `DisableBarriers`).
    pub fn set_barriers_enabled(&mut self, name: &str, enabled: bool) -> bool {
        let Self { common, swbf2, .. } = self;
        let barriers = &mut common.barriers;
        let matched = swbf2
            .as_mut()
            .is_some_and(|s| s.barrier_toggles.set_enabled(barriers, name, enabled));
        self.invalidate_walks(matched)
    }

    /// Toggle 2005 authored barriers matching `name` (`ToggleBarriers`).
    pub fn toggle_barriers(&mut self, name: &str) -> bool {
        let Self { common, swbf2, .. } = self;
        let barriers = &mut common.barriers;
        let matched = swbf2
            .as_mut()
            .is_some_and(|s| s.barrier_toggles.toggle(barriers, name));
        self.invalidate_walks(matched)
    }
}
