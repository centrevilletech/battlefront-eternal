//! Events and world operations for an authority-side rules handler.

use super::RulesWorld;
use super::awards::{Credit, Destroyed, SystemTransition, PersonalScore};
pub use super::objects::{MissionObjects, NamedObject, ObjectProperty, ObjectState, RuleError, RuleResult};
use crate::battle::spawning::TEAM_COUNT;
use crate::gen::{Character, F3UnitKey, F4Handle, MobilePost, Post, SpawnPost, Store, Vehicle};
use crate::store::{Ref, StrId};

/// A placed live object or one life of a vehicle occupying a fleet slot.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectHandle {
    Prop(F4Handle),
    /// Indestructible door, addressed by placement within the current battle.
    Door(u32),
    Post(SpawnPost),
    Vehicle { vehicle: Ref<Vehicle>, life: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A dynamically spawned flyer has a fleet life rather than a named placement.
    MissionFlyerKilled { slot: usize, generation: u32, team: i32, kind: Destroyed, credit: Credit },
    MissionSoldierKilled { victim: Ref<Character>, victim_team: i32, human: bool, hero: bool, credit: Option<Credit>, death: crate::units::infantry::contact::DeathRecord },
    MissionObject { object: ObjectHandle, transition: SystemTransition, team: i32, kind: Destroyed, credit: Option<Credit>, absorbed: f64 },
    UnitKilled {
        unit: F3UnitKey,
        victim: Option<Ref<Character>>,
        victim_team: f64,
        killer: Option<Ref<Character>>,
        killer_team: Option<f64>,
        relation: f64,
    },
    ObjectKilled { object: ObjectHandle, killer: Option<Ref<Character>> },
    TicketsChanged { team: f64, count: u32 },
    TeamPointsChanged { team: f64, points: u32 },
    PostCaptured { post: Ref<Post>, team: f64 },
    PostNeutralized { post: Ref<Post>, from: f64 },
    PostLost { post: Ref<Post>, from: f64 },
    MobilePostOwner { post: Ref<MobilePost>, from: f64, to: f64 },
    MobilePostLost { post: Ref<MobilePost>, from: f64 },
    TimerElapsed { timer: StrId },
    RegionEntered { region: StrId, unit: F3UnitKey },
    RegionLeft { region: StrId, unit: F3UnitKey },
    FlagChanged { flag: StrId, change: FlagChange },
}

#[derive(Clone, Debug, PartialEq)]
pub enum FlagChange {
    PickedUp(F3UnitKey),
    Dropped(F3UnitKey),
    Returned,
    Captured { team: f64 },
}

/// Events in producer order. The producer and consumer each keep a buffer
/// reserved at admission, so events raised during a callback go to the next
/// batch even when the callback causes a synchronous death.
pub struct EventQueue {
    events: Vec<Event>,
    // Sorted placement keys point into this batch. Kill and respawn close a damage interval.
    damage: Vec<((u8, u32, u32), usize)>,
    limit: usize,
    dropped: u32,
    expired_without_delivery: Vec<StrId>,
    mission: bool,
    #[cfg(test)]
    tolerate_overflow: bool,
}

impl EventQueue {
    pub fn new(limit: usize) -> Self {
        Self { events: Vec::with_capacity(limit), damage: Vec::new(), limit, dropped: 0, expired_without_delivery: Vec::new(), mission: false, #[cfg(test)] tolerate_overflow: false }
    }

    /// Overflow keeps the prefix in order and is visible to the consumer.
    pub fn push(&mut self, event: Event) -> bool {
        if let Event::MissionObject { object, transition, absorbed, .. } = &event {
            let key = super::objects::key(object);
            let pos = self.damage.binary_search_by_key(&key, |&(key, _)| key);
            match (transition, pos) {
                (SystemTransition::Damage, Ok(i)) => {
                    let index = self.damage[i].1;
                    if let Event::MissionObject { object: previous, absorbed: total, .. } = &mut self.events[index] {
                        if previous == object {
                            *total += absorbed;
                            return true;
                        }
                    }
                    if self.events.len() < self.limit { self.damage[i].1 = self.events.len(); }
                }
                (SystemTransition::Damage, Err(i)) if self.events.len() < self.limit => {
                    self.damage.insert(i, (key, self.events.len()));
                }
                (_, Ok(i)) => { self.damage.remove(i); }
                _ => {}
            }
        }
        if self.events.len() < self.limit {
            self.events.push(event);
            true
        } else {
            self.dropped = self.dropped.saturating_add(1);
            #[cfg(any(debug_assertions, test))]
            {
                #[cfg(test)]
                let defect = !self.tolerate_overflow;
                #[cfg(not(test))]
                let defect = true;
                assert!(self.mission || !defect, "admitted battle rules event capacity exceeded");
            }
            false
        }
    }

    pub(crate) fn digest_into(&self,h:&mut super::super::script::state::MissionDigest) {
        h.field(&(self.limit,self.overflowed(),self.mission,&self.events));
        if self.overflowed() { h.field(&(self.dropped, &self.expired_without_delivery)); }
    }

    pub fn mission_events(&self) -> bool { self.mission }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn overflowed(&self) -> bool {
        self.dropped != 0
    }

    pub(crate) fn dropped(&self) -> u32 { self.dropped }

    pub(crate) fn expired_without_delivery(&self) -> &[StrId] { &self.expired_without_delivery }

    fn clear(&mut self) {
        self.events.clear();
        self.damage.clear();
        self.dropped = 0;
        self.expired_without_delivery.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reinforcements {
    Finite(u32),
    Unlimited,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Winner(f64),
    Loser(f64),
    Draw,
}

/// The operations a mode may perform during its rules slot.
pub trait RuleActions {
    /// Applied commands and refusals at the command limit in this rules tick.
    fn command_usage(&self) -> (usize, u32) { (0, 0) }
    fn award(&mut self, _credit: Credit, _delta: i32) -> RuleResult<u32> { Err(RuleError::NotAvailable) }
    fn score(&self, team: f64) -> Option<u32>;
    fn committed_outcome(&self) -> Option<Outcome> { None }
    fn target_winner(&self) -> Option<f64> { None }
    fn rearm_score_target(&mut self) {}
    fn set_score(&mut self, team: f64, score: u32) -> bool { self.set_team_points(team, score).is_ok() }
    fn team_points(&self, team: f64) -> RuleResult<u32> { self.score(team).ok_or(RuleError::InvalidValue) }
    fn set_team_points(&mut self, _team: f64, _points: u32) -> RuleResult<()> { Err(RuleError::NotAvailable) }
    fn add_team_points(&mut self, _team: f64, _delta: i32) -> RuleResult<u32> { Err(RuleError::NotAvailable) }
    /// Apply one scripted team change and credit its recipient in the same transaction.
    fn add_scored_team_points(&mut self, team: f64, delta: i32, _scorer: Ref<Character>) -> RuleResult<u32> {
        self.add_team_points(team, delta)
    }
    fn score_limit(&self) -> Option<u32> { None }
    fn battle_time(&self) -> RuleResult<f64> { Err(RuleError::NotAvailable) }
    fn reinforcements(&self, _team: f64) -> RuleResult<Reinforcements> { Err(RuleError::NotAvailable) }
    fn set_reinforcements(&mut self, _team: f64, _count: Reinforcements) -> RuleResult<()> { Err(RuleError::NotAvailable) }
    fn add_reinforcements(&mut self, _team: f64, _delta: i32) -> RuleResult<Reinforcements> { Err(RuleError::NotAvailable) }
    fn end_battle(&mut self, winner: f64) -> bool { self.finish(Outcome::Winner(winner)).is_ok() }
    fn finish(&mut self, _outcome: Outcome) -> RuleResult<()> { Err(RuleError::NotAvailable) }
    fn character_team(&self, _character: Ref<Character>) -> RuleResult<f64> { Err(RuleError::NotAvailable) }
    fn character_class(&self, _character: Ref<Character>) -> RuleResult<StrId> { Err(RuleError::NotAvailable) }
    fn character_is_human(&self, _character: Ref<Character>) -> RuleResult<bool> { Err(RuleError::NotAvailable) }
    fn text(&mut self, _id: StrId) -> RuleResult<&[u16]> { Err(RuleError::NotAvailable) }
    fn find_object(&mut self, _name: &str) -> RuleResult<ObjectHandle> { Err(RuleError::NotAvailable) }
    fn object_name(&self, _object: &ObjectHandle) -> RuleResult<&str> { Err(RuleError::NotAvailable) }
    fn object_class(&self, _object: &ObjectHandle) -> RuleResult<&str> { Err(RuleError::NotAvailable) }
    fn object_state(&mut self, _object: &ObjectHandle) -> RuleResult<ObjectState> { Err(RuleError::NotAvailable) }
    fn object_team(&mut self, object: &ObjectHandle) -> RuleResult<f64> {
        self.object_state(object)?.team.ok_or(RuleError::NotAvailable)
    }
    fn object_health(&mut self, object: &ObjectHandle) -> RuleResult<(f64, f64)> {
        self.object_state(object)?.health.ok_or(RuleError::NotAvailable)
    }
    fn object_alive(&mut self, object: &ObjectHandle) -> RuleResult<bool> { Ok(self.object_state(object)?.alive) }
    fn object_shields(&mut self, _object: &ObjectHandle) -> RuleResult<(f64, f64, f64)> { Err(RuleError::NotAvailable) }
    fn add_landing_region(&mut self, _name: &str) -> RuleResult<()> { Err(RuleError::NotAvailable) }
    fn set_passage_barrier(&mut self, _name: &str, _enabled: bool) -> RuleResult<()> { Err(RuleError::NotAvailable) }
    fn set_property(&mut self, _object: &ObjectHandle, _property: ObjectProperty) -> RuleResult<()> { Err(RuleError::NotAvailable) }
    fn set_health(&mut self, object: &ObjectHandle, value: f64) -> RuleResult<()> {
        self.set_property(object, ObjectProperty::Health(value))
    }
    fn kill_object(&mut self, object: &ObjectHandle) -> RuleResult<()> { self.set_health(object, 0.0) }
    fn set_building_health(&mut self, object: &F4Handle, health: f64) -> bool {
        self.set_health(&ObjectHandle::Prop(object.clone()), health).is_ok()
    }
    fn start_timer(&mut self, timer: StrId, seconds: f64) -> bool;
    fn stop_timer(&mut self, _timer: StrId) -> bool { false }
}

/// A mode receives one batch per active tick, even when it is empty.
pub trait EventRules {
    fn script(&self) -> Option<&crate::battle::script::ScriptRules> { None }
    #[cfg(test)]
    fn script_mut(&mut self) -> Option<&mut crate::battle::script::ScriptRules> { None }
    fn reserve_delivery(&mut self, _batch: usize, _commands: usize) -> Option<()> { Some(()) }
    fn tick(&mut self, dt: f64, events: &EventQueue, actions: &mut dyn RuleActions);
    /// The mission's goal table for the bots. Rules that return one when they
    /// are installed get the goal bots for their whole installation; the bots
    /// read it after each tick, and a later `None` reads as an empty table.
    fn ai_goals(&self) -> Option<&dyn crate::ai::goals::GoalTable> { None }

}

#[derive(Debug)]
struct Timer {
    name: StrId,
    remaining_ticks: f64,
}

/// Maximum native events between delivery slots. Commands run after a swap,
/// so the next batch also has room for their mutations and due timers.
#[derive(Clone, Copy, Default)]
pub struct BattleCapacity {
    pub registry: usize,
    pub victims: usize,
    pub buildings: usize,
    pub posts: usize,
}

impl BattleCapacity {
    fn event_limit(self, commands: usize, timers: usize) -> Option<usize> {
        // Reserve a death per body/hull, another life per spawn/repairer,
        // a building death and regeneration, and one change per post.
        self.victims.max(self.registry).checked_add(self.registry)?
            .checked_add(self.buildings.checked_mul(2)?)?.checked_add(self.posts)?
            .checked_add(commands)?.checked_add(timers)
    }
}

/// Retained, simulation-only mode state.
pub struct EventDriven {
    handler: Box<dyn EventRules>,
    batch: EventQueue,
    observed: Option<Vec<Event>>,
    pub(super) prepared: bool,
    scores: [u32; TEAM_COUNT],
    target_winner: Option<f64>,
    personal: Vec<PersonalScore>,
    pub(crate) waves: Option<crate::battle::spawning::mission::Waves>,
    timers: Vec<Timer>,
    timer_limit: usize,
    command_limit: usize,
    score_limit: Option<u32>,
    objects: MissionObjects,
    /// The goal bots of these rules, installed with them.
    bots: Option<Box<crate::ai::goals::GoalController>>,
    unassigned_inputs: Option<Box<crate::ai::goals::SoldierInputs>>,
}

impl EventDriven {
    pub fn new(handler: impl EventRules + 'static, command_limit: usize, timer_limit: usize) -> Self {
        Self::from_handler(Box::new(handler), command_limit, timer_limit)
    }

    pub(crate) fn from_handler(handler: Box<dyn EventRules>, command_limit: usize, timer_limit: usize) -> Self {
        Self {
            handler,
            batch: EventQueue::new(0),
            observed: None,
            prepared: false,
            scores: Default::default(),
            target_winner: None,
            personal: Vec::new(),
            waves: None,
            timers: Vec::new(),
            timer_limit,
            command_limit,
            score_limit: None,
            objects: MissionObjects::default(),
            bots: None,
            unassigned_inputs: None,
        }
    }

    pub(crate) fn digest_into(&self, h: &mut super::super::script::state::MissionDigest) {
        h.field(&(self.prepared,self.command_limit,self.timer_limit,self.score_limit));
        h.field(&(&self.scores,self.target_winner,&self.personal,&self.timers,&self.waves));
        self.batch.digest_into(h);
    }

    pub fn ai_goals(&self) -> Option<&dyn crate::ai::goals::GoalTable> { self.handler.ai_goals() }

    pub(crate) fn observe_transitions(&mut self) {
        if self.observed.is_none() {
            self.observed = Some(Vec::with_capacity(self.batch.limit));
        }
    }

    /// Mission transitions delivered in the last rules tick, in authority order.
    pub fn observed_transitions(&self) -> &[Event] {
        self.observed.as_deref().unwrap_or_default()
    }

    pub fn team_points(&self) -> &[u32; TEAM_COUNT] { &self.scores }
    pub fn score_limit(&self) -> Option<u32> { self.score_limit }
    pub fn personal_points(&self) -> &[PersonalScore] { &self.personal }
    /// Armed native deadlines in insertion order, with remaining time in 60 Hz ticks.
    pub fn running_timers(&self) -> impl Iterator<Item=(StrId,f64)> + '_ {
        self.timers.iter().map(|timer| (timer.name,timer.remaining_ticks))
    }
    pub fn named_objects(&self) -> &MissionObjects { &self.objects }


    pub fn with_mission(mut self, objects: MissionObjects, score_limit: Option<u32>) -> Self {
        self.admit_objects(objects, score_limit);
        self
    }

    pub(crate) fn admit_objects(&mut self, objects: MissionObjects, score_limit: Option<u32>) {
        self.objects = objects;
        self.score_limit = score_limit;
    }

    /// The ticks until the mission may give a Character who waits for one a
    /// body, zero once it may; none for one who waits for nothing.
    pub fn deploys_in(&self, s: &Store, c: &Character) -> Option<u32> {
        self.waves.as_ref()?.opens_in(s, c)
    }

    /// Whether a team may have one more alive of a class of its roster.
    pub fn has_room(&self, s: &Store, team: usize, class: StrId) -> bool {
        self.waves.as_ref().is_none_or(|waves| waves.has_room(s, team, class))
    }

    pub fn mission(&self) -> Option<(&crate::battle::script::ScriptRules, &[PersonalScore])> {
        Some((self.handler.script()?, &self.personal))
    }

    #[cfg(test)]
    pub(crate) fn mission_mut(&mut self) -> Option<&mut crate::battle::script::ScriptRules> {
        self.handler.script_mut()
    }

    /// Reserve both buffers at admission and give the producer to the world.
    pub fn prepare(&mut self, capacity: BattleCapacity) -> Option<EventQueue> {
        let mission = self.handler.script().is_some();
        let mut limit = capacity.event_limit(self.command_limit, self.timer_limit)?;
        if mission {
            // A damage interval in addition to each admitted object's death/repair events.
            limit = limit.checked_add(capacity.registry.max(capacity.victims))?
                .checked_add(capacity.buildings)?.checked_add(capacity.posts)?;
        }
        if let Some(rules) = self.handler.script() {
            let host = rules.host();
            self.waves = Some(crate::battle::spawning::mission::Waves::new(host.setup.spawn_wave_delay.unwrap_or(10.0),
                host.setup.spawn_slot_delay.unwrap_or(0.25), host.config.network, &host.setup.teams));
        }
        if mission {
            self.handler.reserve_delivery(limit, self.command_limit)?;
            self.personal.try_reserve_exact(capacity.victims.max(capacity.registry).max(256)).ok()?;
        }
        let reserve = || {
            let mut queue = EventQueue::new(0);
            queue.events.try_reserve_exact(limit).ok()?;
            if mission {
                queue.damage.try_reserve_exact(limit).ok()?;
                queue.expired_without_delivery.try_reserve_exact(self.timer_limit).ok()?;
            }
            queue.limit = limit;
            queue.mission = mission;
            Some(queue)
        };
        self.timers.try_reserve_exact(self.timer_limit).ok()?;
        let producer = reserve()?;
        self.batch = reserve()?;
        self.prepared = true;
        if self.handler.ai_goals().is_some() {
            self.bots = Some(Box::default());
        }
        Some(producer)
    }

    pub(crate) fn prepare_unassigned_inputs(&mut self, characters: usize, epoch: u32) -> Option<()> {
        let mut inputs = Box::<crate::ai::goals::SoldierInputs>::default();
        inputs.reserve(characters)?;
        inputs.epoch = epoch;
        self.unassigned_inputs = Some(inputs);
        Some(())
    }

    pub(crate) fn soldier_inputs(&mut self) -> Option<&mut crate::ai::goals::SoldierInputs> {
        match &mut self.bots {
            Some(bots) => Some(&mut bots.soldiers),
            None => self.unassigned_inputs.as_deref_mut(),
        }
    }


    pub(crate) fn has_goal_bots(&self) -> bool { self.bots.is_some() }

    /// The goal bots, when these rules have a goal table.
    pub(crate) fn goal_bots(&mut self) -> Option<&mut crate::ai::goals::GoalController> { self.bots.as_deref_mut() }
    pub(crate) fn take_goal_bots(&mut self) -> Option<Box<crate::ai::goals::GoalController>> { self.bots.take() }
    pub(crate) fn restore_goal_bots(&mut self, bots: Box<crate::ai::goals::GoalController>) { self.bots = Some(bots); }
    pub(crate) fn accepts_goal_handles(&self, handles: &crate::ai::goals::GoalHandles) -> bool {
        self.handler.ai_goals().is_none_or(|table| table.handles().is_some_and(|h| h.same_world(handles)))
    }

    #[cfg(test)]
    pub(crate) fn force_overflow(&mut self, producer: &mut EventQueue, limit: usize) {
        for queue in [&mut self.batch, producer] {
            queue.limit = limit;
            queue.tolerate_overflow = true;
        }
    }

    pub(super) fn tick<W: RulesWorld + ?Sized>(&mut self, s: &mut Store, dt: f64, active: bool, world: &mut W) {
        let Some(pending) = world.rule_events() else { return };
        if active {
            self.timers.retain_mut(|timer| {
                timer.remaining_ticks -= dt * 60.0;
                if timer.remaining_ticks <= 0.0 {
                    let delivered = pending.push(Event::TimerElapsed { timer: timer.name });
                    if !delivered && pending.mission {
                        // The native timer still expires. Keep its identity for the script timer mirror.
                        pending.expired_without_delivery.push(timer.name);
                    }
                    !delivered && !pending.mission
                } else {
                    true
                }
            });
        }
        std::mem::swap(&mut self.batch, pending);
        if active {
            let mut actions = Actions {
                s,
                world,
                scores: &mut self.scores,
                target_winner: &mut self.target_winner,
                personal: &mut self.personal,
                score_limit: self.score_limit,
                command_limit: self.command_limit,
                commands_used: 0,
                commands_refused: 0,
                objects: &self.objects,
                timers: &mut self.timers,
                timer_limit: self.timer_limit,
            };
            #[cfg(test)]
            crate::ai::owned::tests::rules_handler(||self.handler.tick(dt,&self.batch,&mut actions));
            #[cfg(not(test))]
            self.handler.tick(dt,&self.batch,&mut actions);
            if let Some(bots) = self.bots.as_deref_mut() {
                let table = self.handler.ai_goals().unwrap_or(&crate::ai::goals::NO_GOALS);
                if bots.resolve_due(table) {
                    if let Some(objects) = world.rule_objects() {
                        bots.resolve(table, &self.objects, objects, s);
                    }
                }
                world.ai_goal_pass(s, bots, table, dt);
            } else if let Some(inputs) = &mut self.unassigned_inputs {
                world.ai_soldier_inputs(s, inputs, dt);
            }
        }
        if let Some(observed) = &mut self.observed {
            observed.clear();
            observed.extend(self.batch.events().iter().filter(|event| matches!(event,
                Event::MissionObject { transition: SystemTransition::Kill | SystemTransition::Respawn, .. }
                    | Event::MissionSoldierKilled { .. } | Event::MissionFlyerKilled { .. }
                    | Event::TeamPointsChanged { .. }
            )).cloned());
        }
        self.batch.clear();
    }
}

struct Actions<'a, W: ?Sized> {
    s: &'a mut Store,
    world: &'a mut W,
    scores: &'a mut [u32],
    target_winner: &'a mut Option<f64>,
    personal: &'a mut Vec<PersonalScore>,
    objects: &'a MissionObjects,
    score_limit: Option<u32>,
    command_limit: usize,
    commands_used: usize,
    commands_refused: u32,
    timers: &'a mut Vec<Timer>,
    timer_limit: usize,
}

fn score_slot(team: f64) -> Option<usize> {
    ((1.0..TEAM_COUNT as f64).contains(&team) && team.fract() == 0.0).then_some(team as usize)
}

impl<W: RulesWorld + ?Sized> Actions<'_, W> {
    fn check_command(&mut self) -> RuleResult<()> {
        if self.commands_used == self.command_limit {
            self.commands_refused = self.commands_refused.saturating_add(1);
            return Err(RuleError::LimitReached);
        }
        Ok(())
    }

    fn command(&mut self) -> RuleResult<()> {
        self.check_command()?;
        self.commands_used += 1;
        Ok(())
    }

    fn apply_award(&mut self, team: f64, scorer: Option<Ref<Character>>, delta: i32, team_delta: i32) -> RuleResult<u32> {
        let current = self.team_points(team)?;
        let live = scorer.filter(|r| self.s.character.get(*r).is_some());
        let index = live.and_then(|r| self.personal.iter().position(|p| p.character.slot == r.slot));
        if live.is_some() && index.is_none() && self.personal.len() == self.personal.capacity() {
            return Err(RuleError::LimitReached);
        }
        let total = if team_delta == 0 { current } else { self.add_team_points(team, team_delta)? };
        if let Some(r) = live {
            if let Some(i) = index {
                let row = &mut self.personal[i];
                if row.character != r { *row = PersonalScore { character: r, points: 0 }; }
                row.points = row.points.saturating_add(delta);
            } else {
                self.personal.push(PersonalScore { character: r, points: delta });
            }
        }
        Ok(total)
    }
}

impl<W: RulesWorld + ?Sized> RuleActions for Actions<'_, W> {
    fn command_usage(&self) -> (usize, u32) { (self.commands_used, self.commands_refused) }

    fn award(&mut self, credit: Credit, delta: i32) -> RuleResult<u32> {
        let team_delta = if delta > 0 || credit.human { delta } else { 0 };
        self.apply_award(f64::from(credit.team), credit.character, delta, team_delta)
    }

    fn add_scored_team_points(&mut self, team: f64, delta: i32, scorer: Ref<Character>) -> RuleResult<u32> {
        self.apply_award(team, Some(scorer), delta, delta)
    }

    fn committed_outcome(&self) -> Option<Outcome> {
        let battle = &self.s.battle;
        battle.over.then(|| match battle.loser {
            Some(loser) if loser == battle.own => Outcome::Winner(battle.foe),
            Some(loser) if loser == battle.foe => Outcome::Winner(battle.own),
            _ => Outcome::Draw,
        })
    }

    fn target_winner(&self) -> Option<f64> { *self.target_winner }

    fn rearm_score_target(&mut self) { *self.target_winner = None; }

    fn score(&self, team: f64) -> Option<u32> {
        self.scores.get(score_slot(team)?).copied()
    }

    fn set_team_points(&mut self, team: f64, points: u32) -> RuleResult<()> {
        let i = score_slot(team).ok_or(RuleError::InvalidValue)?;
        if self.scores[i] != points {
            self.command()?;
            self.scores[i] = points;
            if self.target_winner.is_none() && self.score_limit.is_some_and(|limit| points >= limit)
                && (team == self.s.battle.own || team == self.s.battle.foe) {
                *self.target_winner = Some(team);
            }
            self.world.rule_events().ok_or(RuleError::NotAvailable)?.push(Event::TeamPointsChanged { team, points });
        }
        Ok(())
    }

    fn add_team_points(&mut self, team: f64, delta: i32) -> RuleResult<u32> {
        let points = self.score(team).ok_or(RuleError::InvalidValue)?.saturating_add_signed(delta);
        self.set_team_points(team, points)?;
        Ok(points)
    }

    fn score_limit(&self) -> Option<u32> { self.score_limit }
    fn battle_time(&self) -> RuleResult<f64> { Ok(self.s.battle.stats.time) }

    fn reinforcements(&self, team: f64) -> RuleResult<Reinforcements> {
        score_slot(team).ok_or(RuleError::InvalidValue)?;
        let count = self.s.battle.reinforcements.at(team).ok_or(RuleError::NotAvailable)?;
        if count == -1.0 { return Ok(Reinforcements::Unlimited); }
        if count != f64::from(count as u32) { return Err(RuleError::NotAvailable); }
        Ok(Reinforcements::Finite(count as u32))
    }

    fn set_reinforcements(&mut self, team: f64, count: Reinforcements) -> RuleResult<()> {
        score_slot(team).ok_or(RuleError::InvalidValue)?;
        let value = match count { Reinforcements::Finite(n) => f64::from(n), Reinforcements::Unlimited => -1.0 };
        if self.s.battle.reinforcements.at(team) != Some(value) {
            self.command()?;
            self.s.battle.reinforcements.set(team, value);
            if let Reinforcements::Finite(count) = count {
                self.world.rule_events().ok_or(RuleError::NotAvailable)?.push(Event::TicketsChanged { team, count });
            }
        }
        Ok(())
    }

    fn add_reinforcements(&mut self, team: f64, delta: i32) -> RuleResult<Reinforcements> {
        let Reinforcements::Finite(n) = self.reinforcements(team)? else { return Ok(Reinforcements::Unlimited) };
        let next = Reinforcements::Finite(n.saturating_add_signed(delta));
        self.set_reinforcements(team, next)?;
        Ok(next)
    }

    fn finish(&mut self, outcome: Outcome) -> RuleResult<()> {
        let b = &self.s.battle;
        if b.over || (outcome != Outcome::Draw && b.own == b.foe) { return Err(RuleError::InvalidValue); }
        let loser = match outcome {
            Outcome::Winner(t) if t == b.own => b.foe,
            Outcome::Winner(t) if t == b.foe => b.own,
            Outcome::Loser(t) if t == b.own || t == b.foe => t,
            Outcome::Draw => 0.0,
            _ => return Err(RuleError::InvalidValue),
        };
        if outcome != Outcome::Draw && score_slot(loser).is_none() { return Err(RuleError::InvalidValue); }
        let b = &mut self.s.battle;
        b.over = true;
        b.loser = (outcome != Outcome::Draw).then_some(loser);
        b.losing = b.loser == Some(b.own);
        b.victory_clock = b.loser.map_or(crate::store::Nullable::Null, |_| crate::store::Nullable::Value(0.0));
        b.countdown = crate::store::Nullable::Null;
        self.world.battle_over(self.s, loser);
        Ok(())
    }

    fn character_team(&self, character: Ref<Character>) -> RuleResult<f64> {
        Ok(self.s.character.get(character).ok_or(RuleError::StaleHandle)?.team)
    }
    fn character_class(&self, character: Ref<Character>) -> RuleResult<StrId> {
        let c = self.s.character.get(character).ok_or(RuleError::StaleHandle)?;
        c.class_name.or_else(|| self.s.unit.get(c.unit?).map(|u| u.class_name)).ok_or(RuleError::NotAvailable)
    }
    fn character_is_human(&self, character: Ref<Character>) -> RuleResult<bool> {
        Ok(self.s.character.get(character).ok_or(RuleError::StaleHandle)?.human)
    }
    fn text(&mut self, id: StrId) -> RuleResult<&[u16]> {
        self.world.rule_objects().ok_or(RuleError::NotAvailable)?.text(id)
    }
    fn find_object(&mut self, name: &str) -> RuleResult<ObjectHandle> {
        let object = self.objects.find(name)?;
        self.world.rule_objects().ok_or(RuleError::NotAvailable)?.current(self.s, object)
    }
    fn object_name(&self, object: &ObjectHandle) -> RuleResult<&str> {
        self.objects.row(object)?.name.as_deref().ok_or(RuleError::NotAvailable)
    }
    fn object_class(&self, object: &ObjectHandle) -> RuleResult<&str> {
        self.objects.row(object)?.class.as_deref().ok_or(RuleError::NotAvailable)
    }
    fn object_state(&mut self, object: &ObjectHandle) -> RuleResult<ObjectState> {
        self.world.rule_objects().ok_or(RuleError::NotAvailable)?.state(self.s, object)
    }
    fn object_shields(&mut self, object: &ObjectHandle) -> RuleResult<(f64, f64, f64)> {
        self.world.rule_objects().ok_or(RuleError::NotAvailable)?.shields(self.s, object)
    }
    fn add_landing_region(&mut self, name: &str) -> RuleResult<()> {
        self.check_command()?;
        self.world.rule_objects().ok_or(RuleError::NotAvailable)?.add_landing_region(name)?;
        self.commands_used += 1;
        Ok(())
    }
    fn set_passage_barrier(&mut self, name: &str, enabled: bool) -> RuleResult<()> {
        self.check_command()?;
        if !self.world.rule_objects().is_some_and(|port| port.passage_barrier(name, enabled)) {
            return Err(RuleError::NotAvailable);
        }
        self.commands_used += 1;
        Ok(())
    }
    fn set_property(&mut self, object: &ObjectHandle, property: ObjectProperty) -> RuleResult<()> {
        self.check_command()?;
        self.world.rule_objects().ok_or(RuleError::NotAvailable)?.change(self.s, object, property)?;
        self.commands_used += 1;
        Ok(())
    }

    fn start_timer(&mut self, timer: StrId, seconds: f64) -> bool {
        if !seconds.is_finite() || seconds < 0.0 {
            return false;
        }
        if let Some(entry) = self.timers.iter_mut().find(|entry| entry.name == timer) {
            entry.remaining_ticks = seconds * 60.0;
        } else if self.timers.len() < self.timer_limit {
            self.timers.push(Timer { name: timer, remaining_ticks: seconds * 60.0 });
        } else {
            return false;
        }
        true
    }

    fn stop_timer(&mut self, timer: StrId) -> bool {
        self.timers.retain(|entry| entry.name != timer);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "admitted battle rules event capacity exceeded")]
    fn undersized_queue_reports_an_admission_defect() {
        EventQueue::new(0).push(Event::TimerElapsed { timer: StrId(0) });
    }

    #[test]
    fn queue_keeps_the_ordered_prefix_and_reuses_its_allocation() {
        let mut queue = EventQueue::new(2);
        queue.tolerate_overflow = true;
        let allocation = queue.events.as_ptr();
        let event = |name| Event::TimerElapsed { timer: StrId(name) };
        for name in 1..=3 {
            queue.push(event(name));
        }
        assert_eq!(queue.events(), [event(1), event(2)]);
        assert!(queue.overflowed());

        queue.clear();
        queue.push(event(4));
        assert_eq!(queue.events(), [event(4)]);
        assert!(!queue.overflowed());
        assert_eq!(queue.events.as_ptr(), allocation);
    }
}
