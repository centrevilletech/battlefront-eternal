//! Admission selects the battle's rules. Characters and their statistics
//! stay in the canonical battle store.
//!
//! The state is the store's (rust/sim/schema/battle.json): the `battle`
//! singleton and its Characters, by reference.

//! A native host supplies the factory before admitting an event-driven mode.
//! Replay invokes the factory with the recorded parameters.
//!
//! ```
//! use bfe_family::GameFamily;
//! use bfe_sim::battle::rules::{ModeParameters, events::*};
//! use bfe_sim::world::owner::World;
//!
//! struct ScoreLimit;
//!
//! impl EventRules for ScoreLimit {
//!     fn tick(&mut self, _: f64, batch: &EventQueue, actions: &mut dyn RuleActions) {
//!         for event in batch.events() {
//!             if let Event::TeamPointsChanged { team, points } = event {
//!                 if actions.score_limit().is_some_and(|limit| *points >= limit) {
//!                     actions.finish(Outcome::Winner(*team)).expect("admitted scoring team");
//!                     break;
//!                 }
//!             }
//!         }
//!     }
//! }
//!
//! let mut world = World::new();
//! world.register_rule_factory(|family, mode, _params| match (family, mode) {
//!     (GameFamily::Swbf2, "example") => Ok(Box::new(ScoreLimit)),
//!     _ => Err(RuleError::NotAvailable),
//! });
//! let parameters = ModeParameters { command_limit: 8, score_limit: Some(4), ..Default::default() };
//! world.request_battle_rules(GameFamily::Swbf2, "example", parameters)?;
//! # Ok::<(), RuleError>(())
//! ```

use super::posts;
use super::stats::{self, Seen};
use super::world::BattleWorld;
use crate::gen::{Battle, BattleStats, Character, KillRing, Store, TeamDesc};
use crate::store::{IntObj, Nullable, Ref, Strings, TypedArray};
use bfe_family::GameFamily;

pub mod awards;
pub(crate) mod hero_health;
mod conquest;
pub mod events;
pub mod objects;

pub use conquest::{living, score, update, update_locals, update_team};
use events::{EventDriven, EventQueue, EventRules, MissionObjects, RuleError, RuleResult};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModeParameters {
    pub command_limit: u32,
    pub timer_limit: u32,
    pub score_limit: Option<u32>,
    pub time_limit: Option<f64>,
}

#[derive(Clone)]
pub(crate) struct Selection {
    pub family: GameFamily,
    pub mode: String,
    pub parameters: ModeParameters,
}

pub(crate) type Factory = dyn Fn(GameFamily, &str, &ModeParameters) -> RuleResult<Box<dyn EventRules>>;

impl crate::world::owner::World {
    /// Whether the battle runs a scripted mission, installed or about to be.
    pub(crate) fn has_mission(&self) -> bool {
        self.battle.prepared_mission.is_some() || self.mission_state().is_some()
    }

    pub(crate) fn can_admit_mission(&self) -> bool {
        self.battle.tail.is_none() && self.battle.core.is_none() && self.battle.rule_request.is_none() && !self.tick.running
    }

    /// Allocate mission goal handles from this world, including during script load.
    pub fn goal_handles(&mut self) -> crate::ai::goals::GoalHandles {
        self.battle.goal_handles.get_or_insert_with(Default::default).clone()
    }

    pub(crate) fn prepare_script_rules(&mut self, mode: &str, parameters: ModeParameters, rules: super::script::ScriptRules) -> RuleResult<()> {
        if self.battle.rule_request.is_some() || self.battle.prepared_mission.is_some()
            || !self.goal_handles().same_world(&rules.host().goal_handles) {
            return Err(RuleError::InvalidValue);
        }
        let mode_rules = EventDriven::new(rules, parameters.command_limit as usize, parameters.timer_limit as usize);
        self.battle.prepared_mission = Some(Box::new(RuleSet::EventDriven(mode_rules)));
        self.battle.rule_request = Some(Selection { family: GameFamily::Swbf2, mode: mode.to_string(), parameters });
        Ok(())
    }

    /// Supply a fresh battle-local handler for a supported family and mode.
    /// A replay supplies its recorded parameters to this same factory.
    pub fn register_rule_factory(&mut self, factory: impl Fn(GameFamily, &str, &ModeParameters) -> RuleResult<Box<dyn EventRules>> + 'static) {
        self.battle.rule_factory = Some(Box::new(factory));
    }

    /// Supply the world allocator to a native goal writer before its factory runs.
    pub fn register_goal_rule_factory(&mut self, factory: impl Fn(GameFamily, &str, &ModeParameters, crate::ai::goals::GoalHandles) -> RuleResult<Box<dyn EventRules>> + 'static) {
        let handles = self.goal_handles();
        self.register_rule_factory(move |family, mode, params| factory(family, mode, params, handles.clone()));
    }

    /// Cold host entry. The player availability gate does not request event modes.
    pub fn request_battle_rules(&mut self, family: GameFamily, mode: &str, parameters: ModeParameters) -> RuleResult<()> {
        if self.battle.tail.is_some() || mode.is_empty() || parameters.time_limit.is_some_and(|t| !t.is_finite() || t < 0.0) {
            return Err(RuleError::InvalidValue);
        }
        match (family, mode) {
            (GameFamily::Swbf1, "conquest") => self.battle.rule_request = None,
            (GameFamily::Swbf2, _) if self.battle.rule_factory.is_some() => {
                self.battle.rule_request = Some(Selection { family, mode: mode.to_owned(), parameters });
            }
            _ => return Err(RuleError::NotAvailable),
        }
        Ok(())
    }
}

/// The native death path still runs while the tail and handler are on loan.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DeathTickets { Conquest, Handler }


pub enum RuleSet {
    Conquest,
    EventDriven(EventDriven),
}

impl RuleSet {
    pub(crate) fn mission_waves(&mut self) -> Option<&mut super::spawning::mission::Waves> {
        match self { Self::EventDriven(mode) => mode.waves.as_mut(), Self::Conquest => None }
    }

    pub(super) fn select(family: GameFamily, mode: &str, parameters: ModeParameters,
        factory: Option<&Factory>, objects: MissionObjects) -> RuleResult<Self> {
        match (family, mode) {
            (GameFamily::Swbf1, "conquest") => Ok(Self::Conquest),
            (GameFamily::Swbf2, _) => {
                let handler = factory.ok_or(RuleError::NotAvailable)?(family, mode, &parameters)?;
                let rules = EventDriven::from_handler(handler, parameters.command_limit as usize, parameters.timer_limit as usize);
                Ok(Self::EventDriven(rules.with_mission(objects, parameters.score_limit)))
            }
            _ => Err(RuleError::NotAvailable),
        }
    }

    pub(super) fn tick<W: RulesWorld + ?Sized>(
        &mut self,
        s: &mut Store,
        dt: f64,
        strings: &dyn stats::ContextText<W>,
        stats_active: impl Fn(Ref<Character>, &Character) -> bool,
        world: &mut W,
    ) {
        match self {
            Self::Conquest => conquest::update(s, dt, strings, stats_active, world),
            Self::EventDriven(mode) => {
                if !mode.prepared || world.rule_events().is_none() { return; }
                let active = advance_lifecycle(s, dt, strings, stats_active, world);
                mode.tick(s, dt, active, world);
            }
        }
    }
}

/// GAME_OVER_DELAY: s from a team's last reinforcement to the game-over flag.
pub const GAME_OVER_DELAY: f64 = 2.0;
/// END_SEQUENCE: s from the game-over flag to the end-of-match screens.
pub const END_SEQUENCE: f64 = 7.0;
/// The no-post countdown, s.
pub const NO_POST: f64 = 20.0;

/// The services used by the rules and tallies. Native whole-battle hosts
/// use BattleWorld below; the live module can also call these rules while
/// the command posts and bodies still belong to JavaScript.
pub trait RulesWorld {
    fn ai_goal_pass(&mut self, _: &mut Store, _: &mut crate::ai::goals::GoalController, _: &dyn crate::ai::goals::GoalTable, _: f64) {}
    fn ai_soldier_inputs(&mut self, _: &mut Store, _: &mut crate::ai::goals::SoldierInputs, _: f64) {}

    fn stats_active(&mut self, _s: &Store, _r: Ref<Character>, _c: &Character, active: bool) -> bool { active }

    /// Owned worlds move prepared storage into a lazy record at this slot.
    /// Capture hosts retain recordOf's ordinary first-use construction.
    fn prepare_record(&mut self, _s: &mut Store, _character: Ref<Character>) {}
    fn relation(&mut self, a: f64, b: f64) -> f64;
    fn unit_alive(&mut self, s: &Store, unit: Ref<crate::gen::Unit>) -> bool;
    fn observe(&mut self, s: &Store, character: Ref<Character>, unit: Option<Ref<crate::gen::Unit>>) -> Option<Seen>;
    fn living(&mut self, s: &Store, team: f64) -> Option<f64>;
    fn owned_values(&mut self, s: &mut Store, team: f64) -> Vec<Option<f64>>;
    fn battle_over(&mut self, s: &mut Store, loser: f64);
    /// Exposes the native object service for queries and property writes.
    /// Building health uses the same qualified property interface.
    ///
    /// ```compile_fail,E0599
    /// # use bfe_sim::{battle::rules::RulesWorld, gen::{F4Handle, Store}};
    /// fn destroy(world: &mut dyn RulesWorld, store: &mut Store, building: &F4Handle) {
    ///     world.set_rule_building_health(store, building, 0.0);
    /// }
    /// ```
    fn rule_objects(&mut self) -> Option<&mut dyn objects::ObjectServices> { None }
    fn rule_events(&mut self) -> Option<&mut EventQueue> { None }
    /// Native owners can sum their placed/mobile posts directly. The legacy
    /// foreign host still supplies its value list while its callers migrate.
    fn owned_summary(&mut self, s: &mut Store, team: f64) -> (usize, f64) {
        let values = self.owned_values(s, team);
        let mut sum = 0.0;
        for value in &values { sum += value.unwrap_or(10.0); }
        (values.len(), sum)
    }
}

impl<T: BattleWorld + ?Sized> RulesWorld for T {
    fn relation(&mut self, a: f64, b: f64) -> f64 { BattleWorld::relation(self, a, b) }
    fn unit_alive(&mut self, _s: &Store, unit: Ref<crate::gen::Unit>) -> bool { BattleWorld::unit_alive(self, unit) }
    fn observe(&mut self, _s: &Store, character: Ref<Character>, unit: Option<Ref<crate::gen::Unit>>) -> Option<Seen> { BattleWorld::observe(self, character, unit) }
    fn living(&mut self, _: &Store, team: f64) -> Option<f64> { BattleWorld::living(self, team) }
    fn owned_values(&mut self, s: &mut Store, team: f64) -> Vec<Option<f64>> {
        posts::owned(s, team, self).into_iter().map(|post| posts::value_bleed(s, post)).collect()
    }
    fn battle_over(&mut self, _: &mut Store, loser: f64) { BattleWorld::battle_over(self, loser); }
}

/// new Character(index, team, human): his name a human's
/// ("Player 1"), else none until his roster names him.
pub fn new_character(index: f64, team: f64, human: bool, strings: &mut Strings) -> Character {
    Character {
        index,
        team,
        human,
        name: human.then(|| strings.intern_str(&super::names::human_name(0.0))),
        out_of_bounds: -1.0,
        ..Default::default()
    }
}

/// new BattleStats(): an empty ring.
pub fn new_stats() -> BattleStats {
    BattleStats { time: 0.0, ring: KillRing { killers: vec![None; super::stats::RING_SIZE], times: TypedArray::default(), slots: TypedArray::default(), head: 0.0, tail: 0.0 } }
}

/// new Battle({ teams, posts, own, foe }), its Characters
/// none yet.
pub fn new_battle(teams: IntObj<TeamDesc>, own: f64, foe: f64) -> Battle {
    let reinforcements: IntObj<f64> = teams.iter().map(|(t, v)| (t, v.reinforcements.unwrap_or(if t >= 3.0 { 2147483647.0 } else { 0.0 }))).collect();
    let team_ids: Vec<f64> = teams.keys().collect();
    let no_post = team_ids.iter().map(|t| (*t, NO_POST)).collect();
    let bleed_rules = teams
        .iter()
        .map(|(t, v)| {
            let mut rules = v.bleed.clone().unwrap_or_default();
            // [...rules].sort((a, b) => a[0] - b[0]): a stable sort.
            jsrt::sort::js_sort(&mut rules, &mut |a: &[f64; 2], b: &[f64; 2]| a[0] - b[0]);
            (t, rules)
        })
        .collect();
    Battle {
        own,
        foe,
        start: reinforcements.clone(),
        reinforcements,
        team_ids,
        no_post,
        victory_clock: Nullable::Null,
        countdown: Nullable::Null,
        loser: None,
        bleed_rules,
        teams,
        stats: new_stats(),
        ..Default::default()
    }
}

/// spend(team, count): a death costs the team a
/// reinforcement.
pub fn spend(battle: &mut Battle, team: f64, count: f64) {
    let left = battle.reinforcements.at(team).unwrap_or(count);
    battle.reinforcements.set(team, v8math::max(left - count, 0.0));
}

/// credit(victim, killer, relation).
pub fn credit(s: &mut Store, victim: Option<Ref<Character>>, killer: Option<Ref<Character>>, relation: f64) {
    if s.battle.game_over {
        return;
    }
    if let Some(k) = killer {
        if relation != 0.0 {
            let c = s.character.get_mut(k).expect("a killer");
            c.kills += 1.0;
            if relation > 0.0 {
                c.team_kills += 1.0;
            }
        }
    }
    stats::kill(&mut s.battle.stats, &mut s.character, killer, victim, relation);
    if let Some(v) = victim {
        s.character.get_mut(v).expect("a victim").deaths += 1.0;
    }
    stats::death(&mut s.character, victim);
}

/// headshot(credited).
pub fn headshot(s: &mut Store, credited: Option<Ref<Character>>) {
    if !s.battle.game_over {
        stats::headshot(&mut s.character, credited);
    }
}

/// vehicleDestroyed(destroyer).
pub fn vehicle_destroyed(s: &mut Store, destroyer: Option<Ref<Character>>) {
    if !s.battle.game_over {
        stats::vehicle_destroyed(&mut s.character, destroyer);
    }
}

/// point(character): a command post point.
pub fn point(s: &mut Store, character: Option<Ref<Character>>) {
    let Some(r) = character else { return };
    if s.battle.game_over {
        return;
    }
    let c = s.character.get_mut(r).expect("a Character");
    c.captures = ((jsrt::conv::to_int32(c.captures + 1.0)) & 0xffff) as f64;
}

/// postDestroyed(destroyer, relation).
pub fn post_destroyed(s: &mut Store, destroyer: Option<Ref<Character>>, relation: f64) {
    if relation < 1.0 {
        point(s, destroyer);
    }
}

/// Shared tallies and end sequence, advanced before a mode can declare a winner.
fn advance_lifecycle<W: RulesWorld + ?Sized>(
    s: &mut Store,
    dt: f64,
    strings: &dyn stats::ContextText<W>,
    stats_active: impl Fn(Ref<Character>, &Character) -> bool,
    world: &mut W,
) -> bool {
    if !s.battle.game_over {
        s.battle.stats.time += dt;
        for i in 0..s.battle.characters.len() {
            let r = s.battle.characters[i];
            let c = s.character.get(r).expect("a Character of the battle");
            if !world.stats_active(s, r, c, stats_active(r, c)) { continue; }
            world.prepare_record(s, r);
            let c = s.character.get_mut(r).expect("active statistics Character");
            stats::record_of(c);
            let unit = c.unit;
            let visible = stats::truthy(&stats::BoundText(strings,world), c.class_name) || unit.is_some();
            let seen = if visible { world.observe(s, r, unit) } else { None };
            stats::tick_character(dt, s.character.get_mut(r).unwrap(), &stats::BoundText(strings,world), seen);
        }
    }
    let b = &mut s.battle;
    if b.over {
        b.over_time += dt;
        if b.over_time >= GAME_OVER_DELAY {
            b.game_over = true;
        }
        if b.over_time >= GAME_OVER_DELAY + END_SEQUENCE {
            b.finished = true;
        }
        return false;
    }
    true
}

fn begin_end<W: RulesWorld + ?Sized>(s: &mut Store, loser: f64, world: &mut W) {
    let b = &mut s.battle;
    b.over = true;
    b.loser = Some(loser);
    b.countdown = Nullable::Null;
    world.battle_over(s, loser);
}
