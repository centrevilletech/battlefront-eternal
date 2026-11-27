//! Battle's live, rules, tail and connection services share a lifetime while
//! retaining separate publication banks, shells, clocks and identity tables.
//! Live/tail/roster take their existing frames before synchronous gameplay;
//! short projections leave the committed rows visible to recursive damage.
use super::{f4_connections, f4_core, f4_live, f4_roster, f4_tail, live_phase, rules::events::EventQueue};

pub(crate) struct BattleOwner {
    pub(super) live: Option<live_phase::Live>,
    pub(super) core: Option<f4_core::State>,
    pub(super) io: Option<f4_live::Io>,
    pub(super) metadata: Option<f4_live::Metadata>,
    pub(super) live_phase: Option<f4_live::State>,
    pub(super) tail: Option<f4_tail::State>,
    // Recursive deaths can enqueue here while the tail owner is lent out.
    pub(crate) rule_events: Option<EventQueue>,
    pub(super) death_tickets: super::rules::DeathTickets,
    pub(super) rule_factory: Option<Box<super::rules::Factory>>,
    /// Named centres recorded from the composed map at event-mode admission.
    pub(crate) map_goal_regions: Option<crate::ai::goals::GoalRegions>,
    pub(super) prepared_mission: Option<Box<super::rules::RuleSet>>,
    pub(crate) rule_request: Option<super::rules::Selection>,
    pub(super) roster: Option<f4_roster::State>,
    pub(super) connections: Option<f4_connections::State>,
    /// The next goal handle any installation of this world may issue.
    pub(crate) goal_handles: Option<crate::ai::goals::GoalHandles>,
    pub(crate) melee_driver: Option<Box<crate::ai::goals::melee::SoldierDriver>>,
    pub(crate) soldier_phase: crate::units::f3::SoldierPhase,
    pub(crate) bot_phase: fn(&mut crate::world::owner::World) -> u32,
    /// Goal bots whose rules were dropped before they could hand back their bots.
    pub(super) parked_goal_bots: Vec<crate::ai::goals::GoalController>,
    /// Goal-bot installations so far: each one's number stamps its samples.
    pub(super) goal_installations: u32,
}
impl BattleOwner {
    pub(crate) const fn new() -> Self {
        Self { live: None, core: None, io: None, metadata: None, live_phase: None,
            tail: None, rule_events: None, death_tickets: super::rules::DeathTickets::Conquest,
            rule_factory: None, prepared_mission: None, rule_request: None, map_goal_regions: None, roster: None, connections: None,
            goal_handles: None, melee_driver: None, soldier_phase: crate::units::f3::released_step_in, bot_phase: crate::ai::owned::entry::native_phase_in::<false>, parked_goal_bots: Vec::new(), goal_installations: 0 }
    }
}

impl BattleOwner {
    pub(crate) fn swap_rule_factory(&mut self, destination: &mut Self) {
        std::mem::swap(&mut self.rule_factory, &mut destination.rule_factory);
    }
    pub(crate) fn has_rule_events(&self) -> bool { self.rule_events.is_some() }
}
