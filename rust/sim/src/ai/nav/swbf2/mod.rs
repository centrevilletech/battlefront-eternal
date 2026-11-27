//! Battlefront II (2005) navigation support: planning graph, dynamic arc-group
//! deltas, barrier toggles, 2005 hint types, collision-aware routing, and
//! deterministic reachability/distance queries.
//!
//! Battle admission selects this reader and its ground helpers for the 2005
//! game family. The portable route query also supports the released graph.

pub mod cursor;
pub mod goto;
pub mod graph;
pub mod load;
pub(crate) mod records;
#[cfg(test)]
mod tests;
pub(crate) mod walk;

pub use cursor::Swbf2Cursor;
pub use goto::{Swbf2GotoWorld, Swbf2GroundGoto};
pub use graph::{
    PlanningGroup, RouteQuery, Swbf2Arc, Swbf2Graph, Swbf2Hub, ValidationReport, ARC_JET_JUMP,
    ARC_JUMP, ARC_ONE_WAY,
};
pub use load::{load_swbf2_nav, BarrierToggles, Swbf2NavData, SCHEMA_SWBF2};
pub use walk::ProbeKind;

/// Status of an external routed ground walk owned by 2005 navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavWalkStatus {
    /// Route planning, a leg check, or a closed surveyed proximity door is
    /// waiting; movement intent is zeroed while holding position.
    Waiting,
    /// Actively steering along a certified route; movement intent was written
    /// for this tick.
    Moving,
    /// Reached the target on its supported floor; the walk slot is released.
    Arrived,
    /// Unreachable, timed out, stalled, cap-limited, invalid/dead/seated unit,
    /// or no active 2005 navigation; the walk slot is released.
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WalkLeg {
    pub start: super::math::Vec3,
    pub end: super::math::Vec3,
    pub destination: super::math::Vec3,
    pub radius: f64,
    pub mask: u32,
    pub arc: Option<usize>,
    pub movement: u8,
    pub parapet_stall: u8,
    pub check_body: bool,
    pub local: bool,
    pub final_leg: bool,
}

/// 2005 navigation state attached to `Nav` only for `GameFamily::Swbf2` maps.
/// Always `None` for 2004 battles.
#[derive(Clone, Debug)]
pub struct Swbf2Nav {
    pub graph: Option<Swbf2Graph>,
    pub(crate) clearance: walk::Clearance,
    pub(crate) floors: walk::Floors,
    pub(crate) revision: u64,
    query_tick: Option<u32>,
    queries_left: u8,
    trips_left: u8,
    external_starts_left: usize,
    execution_checks_left: usize,
    row_entries_left: usize,
    pub(crate) legs: Vec<Option<WalkLeg>>,
    /// Counts the orders given to a control. A waiting order ends with the next.
    pub(crate) order_stamps: Vec<u32>,
    pub barrier_toggles: BarrierToggles,
    pub boundary_polygon_count: usize,
    /// Per-control one-shot jump triggers for 2005 jump arcs. Pre-sized on
    /// admission so runtime updates perform zero heap allocations.
    pub(crate) jump_pending: Vec<bool>,
    pub(crate) held: Vec<bool>,
    pub(crate) orders: Vec<Option<(usize, usize)>>,
}

impl Swbf2Nav {
    pub fn new(
        graph: Option<Swbf2Graph>,
        barrier_toggles: BarrierToggles,
        boundary_polygon_count: usize,
    ) -> Self {
        Self {
            graph,
            clearance: Default::default(),
            floors: Default::default(),
            revision: 0,
            query_tick: None,
            queries_left: Self::QUERIES_PER_TICK,
            trips_left: Self::TRIPS_PER_TICK,
            external_starts_left: Self::EXTERNAL_STARTS_PER_TICK,
            execution_checks_left: Self::EXECUTION_CHECKS_PER_TICK,
            row_entries_left: walk::SearchBudget::ROW_ENTRIES,
            legs: Vec::new(),
            order_stamps: Vec::new(),
            barrier_toggles,
            boundary_polygon_count,
            jump_pending: Vec::new(),
            held: Vec::new(),
            orders: Vec::new(),
        }
    }

    const QUERIES_PER_TICK: u8 = 4;
    /// Searches from a point to a point (each finds the floors' attachments at
    /// both ends) one tick may run. An order that needs more goes on with the
    /// rest in a later tick.
    const TRIPS_PER_TICK: u8 = 6;
    /// Maximum new external ground walks started across all callers per tick.
    pub(crate) const EXTERNAL_STARTS_PER_TICK: usize = 2;
    // Renewing live legs has its own count so hunters cannot spend it.
    const EXECUTION_CHECKS_PER_TICK: usize = 512;
    /// An order that waits a second for work falls back to a refused one.
    const LONGEST_WAIT: u32 = 60;

    /// Begin the navigation work allowance for a simulation tick. Repeated
    /// calls with the same tick share the remaining credits.
    pub fn begin_tick(&mut self, tick: u32) {
        if self.query_tick != Some(tick) {
            self.query_tick = Some(tick);
            self.queries_left = Self::QUERIES_PER_TICK;
            self.trips_left = Self::TRIPS_PER_TICK;
            self.external_starts_left = Self::EXTERNAL_STARTS_PER_TICK;
            self.execution_checks_left = Self::EXECUTION_CHECKS_PER_TICK;
            self.row_entries_left = walk::SearchBudget::ROW_ENTRIES;
        }
    }

    pub(crate) fn query_tick(&self) -> Option<u32> {
        self.query_tick
    }

    pub(crate) fn take_external_start(&mut self) -> bool {
        match self.external_starts_left.checked_sub(1) {
            Some(left) => {
                self.external_starts_left = left;
                true
            }
            None => false,
        }
    }

    pub(crate) fn may_connect(
        &self,
        from: super::math::Vec3,
        to: super::math::Vec3,
        slack: f64,
        mask: u32,
    ) -> bool {
        let Some(graph) = self.graph.as_ref().filter(|g| !g.hubs.is_empty()) else {
            return true;
        };
        self.floors.may_connect(graph, from, to, slack, mask)
    }

    pub fn prepare_clearance(&mut self) {
        self.clearance.prepare();
    }

    pub(crate) fn prepare_isolated_walk(&mut self) {
        self.clearance.prepare();
        self.query_tick = None;
        self.begin_tick(0);
    }

    pub(crate) fn has_query_credit(&self) -> bool {
        self.queries_left != 0
    }

    fn take_execution_check(&mut self) -> bool {
        match self.execution_checks_left.checked_sub(1) {
            Some(left) => {
                self.execution_checks_left = left;
                true
            }
            None => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn point_trip(
        &mut self,
        barriers: &mut super::barriers::BarrierSet,
        from: super::math::Vec3,
        to: super::math::Vec3,
        radius: f64,
        mask: u32,
        jet: bool,
        cached: bool,
        budget: &mut walk::SearchBudget,
        initial: &mut Option<walk::Attachments>,
        target_slack: f64,
        probe: &mut impl walk::PointProbe,
    ) -> Option<walk::Trip> {
        struct CachedProbe<'a, P> {
            clearance: &'a mut walk::Clearance,
            drives: &'a mut usize,
            waiting: &'a mut bool,
            cached: bool,
            inner: &'a mut P,
        }
        impl<P: walk::PointProbe> walk::PointProbe for CachedProbe<'_, P> {
            fn foot(&mut self, point: super::math::Vec3) -> Option<super::math::Vec3> {
                self.inner.foot(point)
            }
            fn walk(
                &mut self,
                from: super::math::Vec3,
                to: super::math::Vec3,
                movement: u8,
            ) -> Option<bool> {
                let (drives, waiting, inner) = (&mut self.drives, &mut self.waiting, &mut self.inner);
                if self.cached && movement == 0 {
                    self.clearance.try_check(from, to, |p, q| {
                        walk::spend_check(drives, waiting, || inner.walk(p, q, 0))
                    })
                } else {
                    walk::spend_check(drives, waiting, || inner.walk(from, to, movement))
                }
            }
        }
        let graph = self.graph.as_ref()?;
        if budget.stopped()
            || graph.table(mask) < 0
            || !barriers.point_clear(from, radius, mask).0
            || !barriers.point_clear(to, radius, mask).0
            || !self.floors.may_connect(graph, from, to, 0., mask)
        {
            return None;
        }
        let mut scan =
            |nearby, initial: &mut Option<walk::Attachments>, budget: &mut walk::SearchBudget| {
                let Some(left) = self.trips_left.checked_sub(1) else {
                    budget.waiting = true;
                    return None;
                };
                self.trips_left = left;
                budget.share_rows(&mut self.row_entries_left, |budget| {
                    let mut wrapped = CachedProbe {
                        clearance: &mut self.clearance,
                        drives: &mut budget.drives,
                        waiting: &mut budget.waiting,
                        cached,
                        inner: probe,
                    };
                    self.floors.point_trip(
                        graph,
                        barriers,
                        from,
                        to,
                        radius,
                        mask,
                        jet,
                        &mut budget.entries,
                        initial,
                        nearby,
                        target_slack,
                        &mut wrapped,
                    )
                })
            };
        let trip = scan(false, initial, budget);
        // A clear attachment in the home disc can face a disconnected floor.
        // Spend remaining credits on other directed entrances to the same spot.
        if trip.is_none() && !budget.stopped() && initial.is_some_and(|xs| xs[0].is_some()) {
            scan(true, &mut None, budget)
        } else {
            trip
        }
    }

    /// Pre-allocate per-control jump trigger slots for `capacity` controls.
    pub fn reserve_controls(&mut self, capacity: usize) {
        if self.jump_pending.len() < capacity {
            self.jump_pending.resize(capacity, false);
            self.held.resize(capacity, false);
            self.legs.resize(capacity, None);
            self.orders.resize(capacity, None);
            self.order_stamps.resize(capacity, 0);
        }
    }

    pub(crate) fn set_jump_pending(&mut self, control: usize, value: bool) {
        if control < self.jump_pending.len() {
            self.jump_pending[control] = value;
        }
    }

    pub(crate) fn take_jump_pending(&mut self, control: usize) -> bool {
        if let Some(slot) = self.jump_pending.get_mut(control) {
            let was = *slot;
            *slot = false;
            was
        } else {
            false
        }
    }

    /// Block dynamic planning arcs matching `name` (either a `plnp` group arc
    /// hash or an individual arc name). Returns `true` if any group or arc
    /// matched.
    pub fn block_arcs(&mut self, name: &str) -> bool {
        self.graph
            .as_mut()
            .is_some_and(|g| g.set_arcs_blocked(name, true))
    }

    /// Unblock dynamic planning arcs matching `name`. Returns `true` if any
    /// group or arc matched.
    pub fn unblock_arcs(&mut self, name: &str) -> bool {
        self.graph
            .as_mut()
            .is_some_and(|g| g.set_arcs_blocked(name, false))
    }

    /// Toggle dynamic planning arcs matching `name`. Returns `true` if any
    /// group or arc matched.
    pub fn toggle_arcs(&mut self, name: &str) -> bool {
        self.graph.as_mut().is_some_and(|g| g.toggle_arcs(name))
    }
}
