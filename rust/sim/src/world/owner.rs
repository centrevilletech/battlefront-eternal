//! One battle lifetime for canonical rows, phase services and page resources.
//! Cold storage needs no allocation; warm admission reserves the exchange core
//! at the first Sim access.
use crate::abi::Sim;
pub use crate::ai::nav::swbf2::NavWalkStatus;

// One initializer list checks COLD's completeness and initializes its boxed
// twin field by field. Adding an owner cannot leave uninitialized heap data.
macro_rules! cold_fields {
    ($build:ident) => {
        $build! {
            network: None,
            sim: Sim::cold(), tick: super::Tick::cold(),
            input: crate::input::owner::Owner::new(),
            ai: crate::ai::owned::owner::Owner::new(),
            wire: None, page: crate::client::resources::Resources::new(),
            collision: crate::collision::CollisionOwner::new(),
            movement: crate::movement::MovementOwner::new(),
            pose: crate::pose::State::new(),
            units: crate::units::owner::Units::new(),
            fleet: crate::vehicles::module::Fleet::new(),
            battle: crate::battle::owner::BattleOwner::new(),
            combat: crate::combat::module::CombatOwner::new(),
            ordnance: crate::ordnance::module::OrdnanceOwnerState::new(),
            cameras: crate::cameras::Owner::new(),
            space: None,
            warmed: false,
        }
    };
}
macro_rules! cold_value {
    ($($(#[$attribute:meta])* $field:ident: $value:expr,)*) => {
        Self { $($(#[$attribute])* $field: $value,)* }
    };
}
macro_rules! cold_box {
    ($($(#[$attribute:meta])* $field:ident: $value:expr,)*) => {{
        let mut allocation = Box::<Self>::new_uninit();
        let pointer = allocation.as_mut_ptr();
        // SAFETY: the same exhaustive list builds COLD. Every field is written
        // once by const constructors that neither allocate nor panic.
        unsafe {
            $($(#[$attribute])* std::ptr::addr_of_mut!((*pointer).$field).write($value);)*
            allocation.assume_init()
        }
    }};
}

pub struct World {
    pub(crate) network: Option<Box<super::admit::network::Network>>,
    pub(crate) sim: Sim,
    pub(crate) tick: super::Tick,
    pub(crate) input: crate::input::owner::Owner,
    pub(crate) ai: crate::ai::owned::owner::Owner,
    // The cold root cannot contain a Box before admission. Publication retains
    // its existing take/return loan until its callers receive explicit borrows.
    pub(crate) wire: Option<Box<crate::wire::owner::Owner>>,
    pub(crate) page: crate::client::resources::Resources,
    pub(crate) collision: crate::collision::CollisionOwner,
    pub(crate) movement: crate::movement::MovementOwner,
    pub(crate) pose: crate::pose::State,
    // Every phase belongs to this battle on both host targets.
    pub(crate) units: crate::units::owner::Units,
    pub(crate) fleet: crate::vehicles::module::Fleet,
    pub(crate) battle: crate::battle::owner::BattleOwner,
    pub(crate) combat: crate::combat::module::CombatOwner,
    pub(crate) ordnance: crate::ordnance::module::OrdnanceOwnerState,
    pub(crate) cameras: crate::cameras::Owner,
    pub(crate) space: Option<Box<crate::space::SpaceBattlefield>>,
    pub(crate) warmed: bool,
}
impl World {
    pub const COLD: Self = cold_fields!(cold_value);
    pub fn space(&self) -> Option<&crate::space::SpaceBattlefield> { self.space.as_deref() }
    pub fn space_mut(&mut self) -> Option<&mut crate::space::SpaceBattlefield> { self.space.as_deref_mut() }
    /// Sim's old construction order: tick output, wire box, kind blocks, events.
    /// Repeated admission leaves every retained bank and logical identity intact.
    pub fn warm(&mut self) {
        if self.warmed { return; }
        self.tick.warm();
        self.wire = Some(Box::new(crate::wire::owner::Owner::new()));
        self.sim.warm();
        self.warmed = true;
    }
    pub fn new() -> Box<Self> {
        // The inline collision banks are large. Initialize their final heap
        // allocation rather than copying a World through native call stacks.
        let mut world = cold_fields!(cold_box);
        world.warm();
        world
    }

    /// Block 2005 dynamic planning graph arcs matching `name` (`BlockPlanningGraphArcs`).
    pub fn block_planning_graph_arcs(&mut self, name: &str) -> bool {
        self.ai
            .battle_mut()
            .is_some_and(|b| b.state.navigation.nav.block_planning_arcs(name))
    }

    /// Unblock 2005 dynamic planning graph arcs matching `name` (`UnblockPlanningGraphArcs`).
    pub fn unblock_planning_graph_arcs(&mut self, name: &str) -> bool {
        self.ai
            .battle_mut()
            .is_some_and(|b| b.state.navigation.nav.unblock_planning_arcs(name))
    }

    /// Toggle 2005 dynamic planning graph arcs matching `name`.
    pub fn toggle_planning_graph_arcs(&mut self, name: &str) -> bool {
        self.ai
            .battle_mut()
            .is_some_and(|b| b.state.navigation.nav.toggle_planning_arcs(name))
    }

    /// Enable 2005 authored navigation barriers matching `name` (`EnableBarriers`).
    pub fn enable_barriers(&mut self, name: &str) -> bool {
        self.ai
            .battle_mut()
            .is_some_and(|b| b.state.navigation.nav.set_barriers_enabled(name, true))
    }

    /// Disable 2005 authored navigation barriers matching `name` (`DisableBarriers`).
    pub fn disable_barriers(&mut self, name: &str) -> bool {
        self.ai
            .battle_mut()
            .is_some_and(|b| b.state.navigation.nav.set_barriers_enabled(name, false))
    }

    /// Toggle 2005 authored navigation barriers matching `name` (`ToggleBarriers`).
    pub fn toggle_barriers(&mut self, name: &str) -> bool {
        self.ai
            .battle_mut()
            .is_some_and(|b| b.state.navigation.nav.toggle_barriers(name))
    }

    /// Deterministic, zero-allocation reachability and walkable route distance
    /// between two world positions across 2005 graphs, 2004 graphs, and
    /// graphless barrier/collision arenas.
    pub fn nav_route_distance(
        &mut self,
        from: [f64; 3],
        to: [f64; 3],
        radius: f64,
        mask: u32,
        can_jet: bool,
    ) -> crate::ai::nav::route::RouteQuery {
        use crate::ai::nav::{math::Vec3, route::RouteQuery};
        let Some(mut battle) = self.ai.take_battle() else {
            return RouteQuery::UNKNOWN;
        };
        let nav=&mut battle.state.navigation.nav;
        if nav.swbf2.as_ref().is_some_and(|s| !s.has_query_credit() || !self.ai.has_ground_probe()) {
            self.ai.return_battle(battle);
            return RouteQuery::UNKNOWN;
        }
        let cached=crate::ai::owned::live::observe_navigation(self,nav);
        let result = nav.try_distance_with_clearance(
            Vec3::new(from[0], from[1], from[2]),
            Vec3::new(to[0], to[1], to[2]),
            radius,
            mask,
            can_jet,
            cached,
            |a, b, m| {
                crate::ai::owned::live::try_crosses(
                    self,
                    a,
                    b,
                    m,
                    crate::ai::nav::swbf2::ProbeKind::Question,
                )
            },
        );
        self.ai.return_battle(battle);
        result
    }

    /// Start (or keep, if already active for the same unit life and within
    /// 0.1 m of `target`) an external routed ground walk for `unit` on a 2005
    /// map. Uses a dedicated pre-reserved external control slot so Classic/Goal
    /// AI phases cannot clobber it. Returns `NavWalkStatus::Failed` with zero
    /// physical checks on disconnected floors, when the per-tick start cap is
    /// spent, or in a 2004 battle. Zero allocations.
    pub fn nav_walk_begin(
        &mut self,
        unit: crate::store::Ref<crate::gen::Unit>,
        target: [f64; 3],
    ) -> NavWalkStatus {
        self.nav_walk_begin_within(unit, target, 0.0)
    }

    /// Start (or keep) an external routed ground walk for `unit` to `target`,
    /// accepting a barrier-clear floor approach within `approach` metres when
    /// `approach > 0.0` (for example, a chair or console ringed by its own
    /// navigation barrier). Zero allocations.
    pub(crate) fn nav_walk_begin_within(
        &mut self,
        unit: crate::store::Ref<crate::gen::Unit>,
        target: [f64; 3],
        approach: f64,
    ) -> NavWalkStatus {
        let Some(mut battle) = self.ai.take_battle() else {
            return NavWalkStatus::Failed;
        };
        let state = &mut battle.state;
        let status = state
            .navigation
            .nav_walk_begin(self, &state.nav_classes, unit, target, approach);
        self.ai.return_battle(battle);
        status
    }

    /// Advance `unit`'s active external routed ground walk by `dt` and
    /// overwrite movement analogs, posture, jump, and jet on
    /// `store.unit[unit].intent`. Auto-releases on `Arrived`, `Failed`, death,
    /// generation change, or seating. Zero allocations; returns `Failed` in a
    /// 2004 battle.
    pub fn nav_walk_step(
        &mut self,
        unit: crate::store::Ref<crate::gen::Unit>,
        dt: f64,
    ) -> NavWalkStatus {
        let Some(mut battle) = self.ai.take_battle() else {
            return NavWalkStatus::Failed;
        };
        let state = &mut battle.state;
        let status = state
            .navigation
            .nav_walk_step(self, &state.nav_classes, unit, dt);
        self.ai.return_battle(battle);
        status
    }

    /// Cancel and release `unit`'s active external ground walk slot.
    /// Idempotent and zero allocations; no-op in a 2004 battle.
    pub fn nav_walk_stop(&mut self, unit: crate::store::Ref<crate::gen::Unit>) {
        let Some(mut battle) = self.ai.take_battle() else {
            return;
        };
        battle.state.navigation.nav_walk_stop(unit);
        self.ai.return_battle(battle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gen::{Store, KINDS}, store::block::Block};

    #[test]
    fn cold_store_matches_every_derived_singleton_default() {
        // Const construction must preserve optional tags and collection order,
        // including records added to the schema after this owner was introduced.
        let mut cold = Store::cold();
        let mut default = Store::default();
        for kind in 1..=KINDS {
            let (mut a, mut b) = (Block::new(), Block::new());
            assert_eq!(cold.export(kind as u16, &mut a, 1), default.export(kind as u16, &mut b, 1));
            assert_eq!(a.words.iter().map(|n| n.to_bits()).collect::<Vec<_>>(),
                b.words.iter().map(|n| n.to_bits()).collect::<Vec<_>>(), "kind {kind}");
        }
    }

    #[test]
    fn warming_retains_banks_and_worlds_keep_independent_rows() {
        let mut a = World::new();
        let mut b = World::new();
        let blocks = a.sim.blocks.as_ptr();
        let wire = a.wire.as_deref().unwrap() as *const _;
        a.sim.store.battle.stats.time = 23.0;
        a.warm();
        assert_eq!(a.sim.blocks.as_ptr(), blocks);
        assert_eq!(a.wire.as_deref().unwrap() as *const _, wire);
        assert_eq!(a.sim.events.len(), 0);
        assert_eq!(a.sim.blocks.len(), KINDS + 1);
        assert_eq!(a.sim.store.battle.stats.time, 23.0);
        assert_eq!(b.sim.store.battle.stats.time, 0.0);
        b.sim.store.battle.stats.time = 9.0;
        assert_eq!(a.sim.store.battle.stats.time, 23.0);
    }
}
