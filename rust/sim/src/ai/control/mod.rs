//! The Classic AI's control core (src/ai/classic/: index.js's entry
//! point, controller.js's controllers, scheduler.js, interrupts.js,
//! agents.js, patrol.js, squads.js, goals.js's objectives,
//! classes.js's behavior helpers, difficulty.js, and the Enhanced
//! objective agents and objectives). Everything outside it crosses
//! `world::AiWorld`. The JavaScript's classes
//! become index-driven dispatch here: every state machine's state is
//! plain data (controller.rs, agents.rs, patrol.rs), and this file
//! drives them all in the JavaScript's order, quirks included.

pub mod agents;
pub mod classes;
pub mod controller;
pub mod difficulty;
pub mod fsm;
pub mod goals;
pub mod patrol;
pub mod policy;
#[cfg(feature = "replay")]
pub mod replay;
pub mod scheduler;
pub mod squads;
pub mod vec;
pub mod views;
pub mod world;

use crate::ai::owned::intent::Command as IntentCommand;
use agents::{Agent, AgentKind, Helper, ObjectiveAgent, ObjectiveFlavor};
use controller::{goal, goal_state, role, role_of, Controller, SourceGoal};
use fsm::Fsm;
use std::collections::HashMap;
use vec as v;
use world::{AiWorld, Bind, Draws, EntRef, ForeignKind, HelperKind, Policy, TeamTable, UKey};
use super::nav::math::Vec3;

/// Options of createClassicAI (classic/index.js:222) that reach the
/// arithmetic: the bind, the fallback sensor range, the mission
/// controls (absent means controls() is null), the formation option
/// and the hard-tier override.
#[derive(Clone, Debug)]
pub struct Options {
    pub bind: Bind,
    pub fallback_sensor_range: f64,
    pub controls: Option<goals::MissionControls>,
    pub follow_near: Option<f64>,
    pub follow_far: Option<f64>,
    pub hard: Option<bool>,
    pub lod_disabled: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            bind: Bind::default(),
            fallback_sensor_range: 75.0,
            controls: None,
            follow_near: None,
            follow_far: None,
            hard: None,
            lod_disabled: false,
        }
    }
}

/// The Classic AI (classic/index.js ClassicAI). Characters are the
/// roster (index order); controllers are kept in attach order, the
/// live ones also in spawn order. The caller keeps each character's
/// `unit` current (a spawn or death is then noticed by sync(), or
/// reported by spawned()/died()).
pub struct Core {
    pub options: Options,
    pub teams: TeamTable,
    pub difficulty: difficulty::Difficulty,
    pub character_count: usize,
    pub controllers: Vec<Controller>,
    /// Live controllers' indices, spawn order (ai.controllers).
    pub live: Vec<usize>,
    /// Character index -> controller index (byCharacter); its order
    /// is attach order in `roster`.
    pub by_character: Vec<Option<usize>>,
    pub roster: Vec<usize>,
    /// Unit index -> controller index (unit.controller).
    pub unit_controller: HashMap<UKey, usize>,
    pub scheduler: scheduler::Scheduler,
    pub objectives: goals::Objectives,
    pub repair: classes::RepairServices,
    // Scheduler snapshots retain spawn order while service callbacks retire lives.
    decision_order: Vec<usize>,
    input_order: Vec<usize>,
    roster_order: Vec<usize>,
    human_order: Vec<UKey>,
    objective_scratch: goals::ObjectiveScratch,
}

// Keep the concrete native world through the scheduler and its helpers so
// scalar observations inline without materializing unused snapshot fields.
// The unsized bound also retains the same calls for trait-based replay hosts.
impl Core {
    pub fn new(options: Options, teams: TeamTable, difficulty: difficulty::Difficulty, character_count: usize) -> Self {
        let n = character_count;
        let objectives = if options.bind.enhanced { goals::Objectives::enhanced() } else { goals::Objectives::classic() };
        Core {
            options,
            teams,
            difficulty,
            character_count,
            controllers: Vec::with_capacity(n),
            live: Vec::with_capacity(n),
            by_character: vec![None; n],
            roster: Vec::with_capacity(n),
            unit_controller: HashMap::with_capacity(n),
            scheduler: scheduler::Scheduler::default(),
            objectives,
            repair: classes::RepairServices::default(),
            decision_order: Vec::with_capacity(n),
            input_order: Vec::with_capacity(n),
            roster_order: Vec::with_capacity(n),
            human_order: Vec::with_capacity(n),
            objective_scratch: goals::ObjectiveScratch::default(),
        }
    }

    /// Reserve world-sized caches before controllers can execute. Repair jobs
    /// distinguish health/ammo, and Enhanced recruitment tracks seven teams.
    pub fn prepare(&mut self,posts:usize,targets:usize,repair_jobs:usize) {
        self.objective_scratch.reserve(posts);
        self.objectives.threat_at.reserve(posts);
        self.objectives.recruit_at.reserve(posts.saturating_mul(7));
        self.repair.reservations.reserve(self.character_count);
        self.repair.futile.reserve(repair_jobs);
        self.human_order.reserve(targets.saturating_sub(self.human_order.len()));
    }

    pub fn now<W:AiWorld+?Sized>(&self, w: &W) -> f64 {
        w.time()
    }

    pub fn relation(&self, a: f64, b: f64) -> f64 {
        self.teams.relation(a, b)
    }

    pub fn faction(&self, team: f64) -> f64 {
        self.teams.faction(team)
    }

    pub fn is_hard(&self) -> bool {
        if let Some(h) = self.options.hard {
            return h;
        }
        self.difficulty.hard()
    }

    /// schedulerNumbers (classic/index.js:332).
    pub fn scheduler_numbers(&self) -> (f64, f64) {
        if self.options.hard.is_none() {
            return (self.difficulty.cycle(), self.difficulty.los_budget());
        }
        if self.is_hard() { (3.0, 12.0) } else { (15.0, 6.0) }
    }

    pub fn curves_for(&self, team: f64) -> Option<difficulty::Curves> {
        if self.options.bind.population {
            Some(self.difficulty.curves(team))
        } else {
            Some(self.difficulty.curves(team))
        }
    }

    pub fn controls(&self) -> Option<&goals::MissionControls> {
        self.options.controls.as_ref()
    }

    pub fn follow_radii(&self) -> (f64, f64) {
        let mut near = 0.0;
        let mut far = 3.0;
        if let Some(c) = &self.options.controls {
            near = c.formation_near;
            far = c.formation_far;
        }
        if let Some(n) = self.options.follow_near {
            near = n;
        }
        if let Some(f) = self.options.follow_far {
            far = f;
        }
        (near, far)
    }

    pub fn stay_in_turrets(&self) -> bool {
        self.controls().map(|c| c.stay_in_turrets).unwrap_or(false)
    }

    pub fn stay_delay(&self) -> f64 {
        self.controls().map(|c| c.stationary_wait).unwrap_or(10.0)
    }

    pub fn lod_disabled<W:AiWorld+?Sized>(&self, w: &W) -> bool {
        self.options.lod_disabled || w.lod_disabled_world()
    }

    pub fn controller_of_character(&self, character: usize) -> Option<usize> {
        self.by_character.get(character).copied().flatten()
    }

    pub fn controller_of_unit(&self, unit: UKey) -> Option<usize> {
        self.unit_controller.get(&unit).copied()
    }

    // -- lifetime (classic/index.js attach/spawned/died/sync) ----------

    pub fn attach<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize) -> Option<usize> {
        if character >= self.character_count || w.character(character).human {
            return None;
        }
        let ci = match self.by_character[character] {
            Some(ci) => ci,
            None => {
                let ci = self.controllers.len();
                self.controllers.push(Controller::new(character));
                self.by_character[character] = Some(ci);
                self.roster.push(character);
                w.link_character(character,ci,true);
                // adaptClassicAI: a fresh controller takes its policy
                // at its first bind (spawned below), not here.
                ci
            }
        };
        let unit = w.character(character).unit;
        if let Some(unit) = unit {
            if w.unit_scalars(unit).alive && self.controllers[ci].unit != Some(unit) {
                self.spawned(w, d, character);
            }
        }
        Some(ci)
    }

    pub fn detach<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize) {
        if character >= self.character_count {
            return;
        }
        let Some(controller)=self.by_character[character] else {return};
        self.died(w, d, character);
        self.by_character[character] = None;
        w.link_character(character,controller,false);
        self.roster.retain(|&c| c != character);
        // The controller stays in the roster's storage (the map entry
        // is what the JavaScript drops); a later attach makes a new
        // one. Mark it unbound so it is never serviced.
        let _ = self;
    }

    pub fn spawned<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize) -> Option<usize> {
        if character >= self.character_count {
            return None;
        }
        let ci = match self.by_character[character] {
            Some(ci) => ci,
            None => self.attach(w, d, character)?,
        };
        // adaptClassicAI resolves the current control for a seated
        // character; without it the body itself.
        let unit = if self.options.bind.vehicle_adapter && self.options.bind.vehicles {
            w.vehicles_current_controllable(character).or(w.character(character).unit)
        } else {
            w.character(character).unit
        };
        let unit = unit?;
        if !w.unit_scalars(unit).alive {
            return None;
        }
        if self.controllers[ci].unit == Some(unit) && self.controllers[ci].generation == w.unit_scalars(unit).generation {
            return Some(ci);
        }
        if self.controllers[ci].unit.is_some() {
            // spawned rebinds without unbinding (index.js spawned):
            // bind itself disposes the old agent.
            self.remove_live(ci);
        }
        self.bind(w, d, ci, unit);
        // The vehicle adapter runs after bind has republished the
        // character's objective, and replaces the previous life's policy.
        if self.options.bind.vehicle_adapter && self.options.bind.vehicles {
            let policy = w.vehicles_policy_of(unit);
            self.controllers[ci].policy = policy;
            if let Some(facing) = w.vehicles_policy_home_facing(policy, unit) {
                self.controllers[ci].home_facing = facing;
            }
        }
        self.live.push(ci);
        Some(ci)
    }

    pub fn died<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize) {
        let Some(ci) = self.controller_of_character(character) else { return };
        if self.controllers[ci].unit.is_none() {
            return;
        }
        self.remove_live(ci);
        self.unbind(w, d, ci);
    }

    fn remove_live(&mut self, ci: usize) {
        self.live.retain(|&x| x != ci);
    }

    pub fn sync<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws) {
        self.roster_order.clear();
        self.roster_order.extend_from_slice(&self.roster);
        for i in 0..self.roster_order.len() {
            let character = self.roster_order[i];
            let Some(ci) = self.by_character[character] else { continue };
            let unit = if self.options.bind.vehicle_adapter && self.options.bind.vehicles {
                w.vehicles_current_controllable(character).or(w.character_unit(character))
            } else {
                w.character_unit(character)
            };
            let bound = self.controllers[ci].unit;
            if bound.is_some() {
                let stale = match unit {
                    None => true,
                    Some(u) => Some(u) != bound || w.unit_scalars(u).generation != self.controllers[ci].generation || !w.unit_scalars(u).alive,
                };
                if stale {
                    self.died(w, d, character);
                }
            }
            if self.controllers[ci].unit.is_none() {
                if let Some(u) = unit {
                    if w.unit_scalars(u).alive {
                        self.spawned(w, d, character);
                    }
                }
            }
        }
    }

    /// UnitController.bind (controller.js:444), with the vehicle
    /// adapter's policy bind (vehicles/index.js adaptController).
    fn bind<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, unit: UKey) {
        if self.controllers[ci].agent.is_some() {
            self.dispose_agent(w, ci);
        }
        let now = self.now(w);
        let (generation, kind, team, aim_yaw) = {
            let u = w.unit(unit);
            (u.generation, u.kind, u.team, u.soldier.and_then(|s| s.aim_yaw).unwrap_or(0.0))
        };
        let curves = self.curves_for(team);
        self.controllers[ci].unit=Some(unit);
        self.controllers[ci].generation=generation;
        w.link_controller(unit,ci,true);
        {
            let c = &mut self.controllers[ci];
            c.fsm = Fsm::new();
            c.commands = controller::CommandQueue::new();
            c.phase = 0.0;
            c.clear_requests();
            c.aim_state = 2.0;
            c.aim_target = None;
            c.stance = 0.0;
            c.wanted_mode = 0.0;
            c.exit_request = false;
            c.leader = None;
            c.danger_pending = false;
            c.shot_pending = false;
            c.shot_known = false;
            c.shot_source = None;
            c.acoustic.clear();
            c.vehicle_notes.clear();
            c.command_pending = false;
            c.command_record = None;
            c.last_command = None;
            c.processed = false;
            c.lod_skipped = false;
            c.input_skip = false;
            c.input_carry = 0.0;
            c.secondary_wait_until = if kind == 2.0 || kind == 5.0 || kind == 6.0 { now + 4.0 } else { 0.0 };
            c.home_facing = false;
            c.roll = d.a_int(0.0, 100.0);
            c.home_direction = Vec3::new(v8math::sin(aim_yaw), 0.0, v8math::cos(aim_yaw));
            c.difficulty = curves;
            c.published = false;
        }
        w.controller_state_changed(ci,0.);
        self.unit_controller.insert(unit, ci);
        if self.options.bind.combat {
            self.combat_call(w, ci, |w| w.combat_create_perception(ci));
            self.combat_call(w, ci, |w| w.combat_attach_controller(ci));
        }
        let character = self.controllers[ci].character;
        if let Some(post) = w.character(character).objective {
            self.publish_objective(ci, post);
        }
    }

    fn unbind<W:AiWorld+?Sized>(&mut self, w: &mut W, _d: &mut dyn Draws, ci: usize) {
        if self.controllers[ci].agent.is_some() {
            self.dispose_agent(w, ci);
        }
        if self.controllers[ci].nav_control.is_some() {
            w.nav_control_reset(ci);
            self.sync_control(w, ci);
        }
        if let Some(unit) = self.controllers[ci].unit {
            if self.unit_controller.get(&unit) == Some(&ci) {
                self.unit_controller.remove(&unit);
                w.link_controller(unit,ci,false);
            }
        }
        if self.options.bind.combat {
            self.combat_call(w, ci, |w| w.combat_detach_controller(ci));
        }
        let c = &mut self.controllers[ci];
        c.unit = None;
        c.generation = -1.0;
        c.fsm = Fsm::new();
        c.leader = None;
        w.controller_state_changed(ci,0.);
    }

    pub fn publish_objective(&mut self, ci: usize, post: usize) {
        let c = &mut self.controllers[ci];
        c.commands.overwrite(SourceGoal::objective(Some(post)));
        c.published = true;
    }

    pub fn request_objective<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, exclude_previous: bool) -> Option<usize> {
        if !self.options.bind.population {
            return None;
        }
        let character = self.controllers[ci].character;
        let previous = w.character(character).objective;
        self.objectives_request(w, d, character, exclude_previous, previous)
    }

    /// requestExit (controller.js:664).
    pub fn request_exit<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        self.controllers[ci].exit_request = true;
        if self.options.bind.vehicles {
            w.vehicles_request_exit(self,ci);
        }
    }

    pub fn low_ticket_exit<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        if !self.options.bind.vehicles {
            return;
        }
        if !w.vehicles_boarded(ci) {
            return;
        }
        let team = match self.controllers[ci].unit {
            Some(u) => w.unit_scalars(u).team,
            None => return,
        };
        if w.vehicles_low_tickets(team) && !w.vehicles_pilot_keeps_seat(ci) {
            self.request_exit(w, ci);
        }
    }
}

// --------------------------------------------------------------------------
// The generic state-machine drive (controller.js Fsm), dispatched over
// every owner: a controller, its agent, and an objective agent's three
// embedded helpers.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Controller(usize),
    Agent(usize),
    Patrol(usize),
    Snipe(usize),
    Mine(usize),
}

impl Core {
    fn fsm_of(&self, owner: Owner) -> Fsm {
        match owner {
            Owner::Controller(ci) => self.controllers[ci].fsm,
            Owner::Agent(ci) => self.controllers[ci].agent.as_ref().map(|a| a.fsm).unwrap_or_default(),
            Owner::Patrol(ci) => self.objective(ci).map(|o| o.patrol.fsm).unwrap_or_default(),
            Owner::Snipe(ci) => self.objective(ci).map(|o| o.snipe.fsm).unwrap_or_default(),
            Owner::Mine(ci) => self.objective(ci).map(|o| o.mine.fsm).unwrap_or_default(),
        }
    }

    fn fsm_set(&mut self, owner: Owner, fsm: Fsm) {
        match owner {
            Owner::Controller(ci) => self.controllers[ci].fsm = fsm,
            Owner::Agent(ci) => {
                if let Some(a) = &mut self.controllers[ci].agent {
                    a.fsm = fsm;
                }
            }
            Owner::Patrol(ci) => {
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.fsm = fsm;
                }
            }
            Owner::Snipe(ci) => {
                if let Some(o) = self.objective_mut(ci) {
                    o.snipe.fsm = fsm;
                }
            }
            Owner::Mine(ci) => {
                if let Some(o) = self.objective_mut(ci) {
                    o.mine.fsm = fsm;
                }
            }
        }
    }

    pub fn objective(&self, ci: usize) -> Option<&ObjectiveAgent> {
        match &self.controllers[ci].agent.as_ref()?.kind {
            AgentKind::Objective(o) => Some(o),
            _ => None,
        }
    }

    pub fn objective_mut(&mut self, ci: usize) -> Option<&mut ObjectiveAgent> {
        match &mut self.controllers[ci].agent.as_mut()?.kind {
            AgentKind::Objective(o) => Some(o),
            _ => None,
        }
    }

    /// requestState. A controller's goes through the mounted policy's
    /// translation when the vehicle adapter adapted it
    /// (vehicles/index.js adaptController).
    pub fn request_state(&mut self, owner: Owner, state: f64) {
        let state = match owner {
            Owner::Controller(ci) => {
                let policy = self.controllers[ci].policy;
                if self.options.bind.vehicle_adapter && policy != Policy::None && state != fsm::DONE && state != 0.0 {
                    match controller::mobile_goal_of_state(state) {
                        Some(g) => controller::policy_goal_state(policy, g),
                        None => state,
                    }
                } else {
                    state
                }
            }
            _ => state,
        };
        let mut f = self.fsm_of(owner);
        f.pending = state;
        self.fsm_set(owner, f);
    }

    /// Fsm.update (controller.js:293).
    pub fn fsm_update<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, owner: Owner, dt: f64, alternate: bool) {
        if alternate {
            if self.fsm_of(owner).state != 0.0 {
                self.on_tick(w, d, owner, dt);
            }
            return;
        }
        let mut rounds = 0u32;
        let mut next = self.fsm_of(owner).pending;
        loop {
            while next != 0.0 {
                rounds += 1;
                if rounds > fsm::DRAIN_LIMIT {
                    let mut f = self.fsm_of(owner);
                    f.pending = 0.0;
                    self.fsm_set(owner, f);
                    return;
                }
                self.on_exit(w, d, owner);
                {
                    let mut f = self.fsm_of(owner);
                    f.state = f.pending;
                    f.pending = 0.0;
                    self.fsm_set(owner, f);
                    if let Owner::Controller(ci)=owner {w.controller_state_changed(ci,f.state);}
                }
                self.on_enter(w, d, owner);
                next = self.fsm_of(owner).pending;
            }
            if self.fsm_of(owner).state == 0.0 {
                return;
            }
            self.on_tick(w, d, owner, dt);
            next = self.fsm_of(owner).pending;
            if next == 0.0 {
                return;
            }
        }
    }

    /// Fsm.flush (controller.js:317).
    pub fn fsm_flush<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, owner: Owner) {
        let mut rounds = 0u32;
        while self.fsm_of(owner).pending != 0.0 {
            rounds += 1;
            if rounds > fsm::DRAIN_LIMIT {
                let mut f = self.fsm_of(owner);
                f.pending = 0.0;
                self.fsm_set(owner, f);
                return;
            }
            self.on_exit(w, d, owner);
            let mut f = self.fsm_of(owner);
            f.state = f.pending;
            f.pending = 0.0;
            self.fsm_set(owner, f);
            if let Owner::Controller(ci)=owner {w.controller_state_changed(ci,f.state);}
            self.on_enter(w, d, owner);
        }
    }

    /// Fsm.terminate (controller.js:329).
    pub fn fsm_terminate<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, owner: Owner) {
        self.on_exit(w, d, owner);
        let mut f = self.fsm_of(owner);
        f.state = fsm::DONE;
        self.fsm_set(owner, f);
        if let Owner::Controller(ci)=owner {w.controller_state_changed(ci,f.state);}
    }

    // -- the controller's own update (controller.js:518) -------------------

    pub fn controller_update<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64, phase: f64) {
        self.controllers[ci].phase = phase;
        let restricted = phase == 0.0;
        if restricted {
            self.controllers[ci].clear_requests();
        }
        self.drive_commands(w, d, ci, dt, restricted);
        if restricted {
            let c = &mut self.controllers[ci];
            c.shot_pending = false;
            c.shot_known = false;
            c.danger_pending = false;
            c.vehicle_notes.clear();
            c.command_pending = false;
            c.acoustic.clear();
        }
        self.low_ticket_exit(w, ci);
    }

    fn drive_commands<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64, restricted: bool) {
        if restricted {
            if !self.controllers[ci].commands.needs_next_goal() {
                self.fsm_update(w, d, Owner::Controller(ci), dt, true);
            }
            return;
        }
        let mut rounds = 0u32;
        self.controllers[ci].published = false;
        loop {
            if self.controllers[ci].commands.needs_next_goal() {
                if self.controllers[ci].commands.count() > 0 {
                    let goal = self.controllers[ci].commands.pop_front();
                    self.controllers[ci].commands.latch = false;
                    let state = goal_state(goal.kind);
                    if state != 0.0 {
                        self.request_state(Owner::Controller(ci), state);
                    }
                } else {
                    self.controllers[ci].commands.latch = false;
                }
            }
            self.fsm_update(w, d, Owner::Controller(ci), dt, false);
            rounds += 1;
            if rounds > fsm::DRAIN_LIMIT {
                let mut f = self.controllers[ci].fsm;
                f.pending = 0.0;
                self.controllers[ci].fsm = f;
                return;
            }
            if !(self.controllers[ci].commands.needs_next_goal() && !self.controllers[ci].published) {
                break;
            }
        }
    }

    // -- agents: creation, exit, disposal ------------------------------------

    /// createHelper (classic/index.js:447).
    pub(crate) fn create_helper<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> Helper {
        if self.options.bind.vehicles || self.options.bind.nav {
            let (kind, obs, control) = w.helper_create(ci);
            if let Some(control) = control {
                self.adopt_control(ci, control);
            }
            return Helper { kind: Some(kind), obs, direct: Default::default() };
        }
        Helper { kind: Some(HelperKind::Direct), obs: Default::default(), direct: Default::default() }
    }

    /// A navigation control appearing adopts the controller's request
    /// when it is already a MovementRequest, else replaces it
    /// (nav/control.js:67): the controller's first request is a plain
    /// object, so the first control always replaces it.
    fn adopt_control(&mut self, ci: usize, control: world::ControlObs) {
        let c = &mut self.controllers[ci];
        c.request = control.request;
        c.nav_control = Some(control);
    }

    /// After a call that can move the executor: the control's request
    /// is the controller's (nav/control.js aliases them).
    fn sync_control<W:helpers::World+?Sized>(&mut self, w: &mut W, ci: usize) {
        if self.controllers[ci].nav_control.is_some() {
            let obs = w.nav_control_obs(ci);
            self.adopt_control(ci, obs);
        }
    }

    pub(crate) fn sync_combat_writes<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        self.apply_combat_writes(ci,w.combat_writes(ci));
    }

    /// Native nested services share the controller object with combat. Return
    /// its current writes before reentering a Core operation that feeds it.
    pub(crate) fn apply_combat_writes(&mut self,ci:usize,writes:world::CombatWrites) {
        let c = &mut self.controllers[ci];
        c.fire_a = writes.fire_a;
        c.fire_b = writes.fire_b;
        c.target = writes.target;
        c.aim_state = writes.aim_state;
        c.aim_point = writes.aim_point;
        c.stance = writes.stance;
        c.facing = writes.facing;
        c.wanted_mode = writes.wanted_mode;
        if let Some(dodge) = writes.dodge {
            c.dodge = Some(dodge);
        }
        if let Some(alert) = writes.alert_anim {
            c.alert_anim = Some(alert);
        }
    }

    pub(crate) fn feed_combat<W:AiWorld+?Sized>(&self, w: &mut W, ci: usize) {
        let c = &self.controllers[ci];
        let objective = w.combat_objective(c.character);
        w.combat_feed(ci, &world::CombatInput::read(c, objective));
    }

    fn feed_all_combat<W:AiWorld+?Sized>(&self, w: &mut W) {
        for ci in 0..self.controllers.len() { self.feed_combat(w, ci); }
    }

    fn combat_call<W:AiWorld+?Sized,T>(&self, w: &mut W, ci: usize, call: impl FnOnce(&mut W) -> T) -> T {
        self.feed_combat(w, ci);
        call(w)
    }

    // Native pure queries prepare only the fields they inspect. Retained
    // trackers use their existing queues. Captured hosts keep the complete
    // preparation contract for both service kinds.
    fn combat_query<W:AiWorld+?Sized,T>(&self,w:&mut W,ci:usize,call:impl FnOnce(&mut W)->T)->T {
        w.combat_prepare_query(self,ci);call(w)
    }

    fn combat_retained<W:AiWorld+?Sized,T>(&self,w:&mut W,ci:usize,call:impl FnOnce(&mut W)->T)->T {
        w.combat_prepare_retained(self,ci);call(w)
    }

    // Acoustic broadcasts and protected-human rebuilding observe unit rows
    // directly. Their native owner keeps perception between calls; fixtures
    // can still prepare the captured controller views before the same pass.
    fn combat_sensors<W:AiWorld+?Sized,T>(&self, w: &mut W, call: impl FnOnce(&mut W) -> T) -> T {
        w.combat_prepare_sensors(self);
        call(w)
    }

    fn make_agent<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let state = self.controllers[ci].fsm.state;
        let goal = self.controllers[ci].commands.current;
        // agentFor (classic/index.js:404), with the mounted policy's
        // first refusal (vehicles/index.js agentFor).
        let policy = self.controllers[ci].policy;
        if self.options.bind.vehicle_adapter && self.options.bind.vehicles && policy != Policy::None {
            if w.vehicles_policy_agent(policy, state) {
                self.feed_combat(w,ci);let shared=world::ForeignShared::read(&self.controllers[ci]);
                let (tick,control)=w.foreign_agent_create(self,ci,ForeignKind::Fly,&shared);
                self.apply_service_effects(w, tick.effects);
                if let Some(shared) = tick.shared { shared.apply(&mut self.controllers[ci]); }
                if let Some(control) = control {
                    self.adopt_control(ci, control);
                }
                self.controllers[ci].agent = Some(Agent {
                    fsm: Fsm { state: tick.state, pending: tick.pending },
                    unit: self.controllers[ci].unit,
                    goal: Some(goal),
                    interrupts: tick.interrupts,
                    helper: tick.helper,
                    kind: AgentKind::Foreign(ForeignKind::Fly),
                });
                w.agent_created(ci,tick.helper);
                return;
            }
        }
        enum Made {
            Kind(AgentKind),
            Foreign(ForeignKind),
            None,
        }
        let unit_key = self.controllers[ci].unit;
        let enhanced = self.options.bind.enhanced;
        let classes = self.options.bind.classes;
        let made = match state {
            1.0 => Made::Kind(AgentKind::Wait { since: 0.0, look_point: Vec3::default() }),
            2.0 => {
                if self.options.bind.combat_agent {
                    Made::Foreign(ForeignKind::Combat)
                } else {
                    Made::Kind(AgentKind::MissingCombat)
                }
            }
            3.0 => {
                let r = match unit_key {
                    Some(k) => {
                        let u = w.unit(k);
                        u.class_def.as_ref().and_then(|c|c.prepared_role).unwrap_or_else(||role_of(
                            u.class_def.as_ref().and_then(|c| c.role),
                            u.class_def.as_ref().and_then(|c| c.unit_type.as_deref()),
                        ))
                    }
                    None => role::OTHER,
                };
                let flavor = if r == role::SCOUT {
                    ObjectiveFlavor::Scout
                } else if r == role::SUPPORT || r == role::PILOT {
                    ObjectiveFlavor::Repair
                } else {
                    ObjectiveFlavor::Trooper
                };
                let post = goal.post.or_else(|| {
                    let character = self.controllers[ci].character;
                    w.character(character).objective
                });
                Made::Kind(AgentKind::Objective(ObjectiveAgent::new(flavor, enhanced, post, classes)))
            }
            4.0 => Made::Kind(AgentKind::WaitSecondary),
            5.0 => {
                if self.options.bind.vehicles {
                    Made::Foreign(ForeignKind::Board)
                } else {
                    Made::Kind(AgentKind::Board { vehicle: goal.ref_a })
                }
            }
            6.0 => {
                if self.options.bind.vehicles {
                    Made::Foreign(ForeignKind::Unboard)
                } else {
                    Made::Kind(AgentKind::Unboard)
                }
            }
            9.0 => {
                let (start, retried) = match unit_key {
                    Some(k) => (w.unit_scalars(k).position, w.unit_scalars(k).position),
                    None => (Vec3::default(), Vec3::default()),
                };
                Made::Kind(AgentKind::Random { start, last_retry: 0.0, retried_at: retried })
            }
            10.0 => Made::Kind(AgentKind::Follow { leader: goal.ref_a, leader_point: Vec3::default(), countdown: 1.0 }),
            11.0 => Made::Kind(AgentKind::Hold { countdown: 1.0, look_point: Vec3::default() }),
            _ => Made::None,
        };
        let _ = d;
        match made {
            Made::None => {
                self.controllers[ci].agent = None;
            }
            Made::Foreign(kind) => {
                self.feed_combat(w,ci);let shared=world::ForeignShared::read(&self.controllers[ci]);
                let (tick,control)=w.foreign_agent_create(self,ci,kind,&shared);
                self.apply_service_effects(w, tick.effects);
                if let Some(shared) = tick.shared { shared.apply(&mut self.controllers[ci]); }
                if let Some(control) = control {
                    self.adopt_control(ci, control);
                }
                self.controllers[ci].agent = Some(Agent {
                    fsm: Fsm { state: tick.state, pending: tick.pending },
                    unit: unit_key,
                    goal: Some(goal),
                    interrupts: tick.interrupts,
                    helper: tick.helper,
                    kind: AgentKind::Foreign(kind),
                });
                w.agent_created(ci,tick.helper);
            }
            Made::Kind(kind) => {
                // Agent's constructor: interrupts, helper, state 1,
                // initAgent; then the subclass's request.
                let helper = self.create_helper(w, ci);
                let request = match &kind {
                    AgentKind::Wait { .. } => Some(2.0),
                    AgentKind::WaitSecondary => Some(2.0),
                    AgentKind::Random { .. } => Some(2.0),
                    AgentKind::Follow { .. } => Some(2.0),
                    AgentKind::Hold { .. } => Some(2.0),
                    AgentKind::Board { .. } => Some(2.0),
                    AgentKind::Unboard => Some(2.0),
                    AgentKind::Objective(_) => Some(1.0),
                    AgentKind::MissingCombat => Some(fsm::DONE),
                    AgentKind::Foreign(_) => None,
                };
                self.controllers[ci].agent = Some(Agent {
                    fsm: Fsm { state: 1.0, pending: 0.0 },
                    unit: unit_key,
                    goal: Some(goal),
                    interrupts: Default::default(),
                    helper,
                    kind,
                });
                w.agent_created(ci,helper);
                if self.options.bind.combat {
                    self.combat_call(w, ci, |w| w.combat_init_agent(ci));
                }
                if let Some(state) = request {
                    self.request_state(Owner::Agent(ci), state);
                }
            }
        }
    }

    /// Agent.exit + disposeAgent (agents.js:334, controller.js:593).
    fn exit_agent<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let Some(agent) = &self.controllers[ci].agent else { return };
        let foreign = matches!(agent.kind, AgentKind::Foreign(_));
        let objective = matches!(agent.kind, AgentKind::Objective(_));
        if objective {
            self.fsm_terminate(w, d, Owner::Patrol(ci));
            self.fsm_terminate(w, d, Owner::Snipe(ci));
            self.fsm_terminate(w, d, Owner::Mine(ci));
        }
        if foreign {
            self.feed_combat(w,ci);let input=world::ForeignShared::read(&self.controllers[ci]);
            let (shared,effects)=w.foreign_agent_exit(self,ci,&input);
            self.apply_service_effects(w, effects);
            if let Some(shared) = shared {
                shared.apply(&mut self.controllers[ci]);
            }
            self.sync_control(w, ci);
            self.sync_combat_writes(w, ci);
        } else {
            self.helper_terminate(w, ci);
            self.fsm_terminate(w, d, Owner::Agent(ci));
        }
        self.dispose_agent(w, ci);
    }

    fn dispose_agent<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        let agent = self.controllers[ci].agent.take();
        if let Some(agent) = agent {
            if matches!(agent.helper.kind, Some(HelperKind::Ground) | Some(HelperKind::Fly) | Some(HelperKind::Vehicle)) {
                w.helper_dispose(ci);
                self.sync_control(w, ci);
            }
            w.agent_disposed(ci,agent.helper);
        }
    }

    // -- helper dispatch (the agent's movement helper) ------------------------

    fn helper_kind(&self, ci: usize) -> Option<HelperKind> {
        self.controllers[ci].agent.as_ref().and_then(|a| a.helper.kind)
    }

    fn set_obs(&mut self, ci: usize, obs: world::HelperObs) {
        if let Some(a) = &mut self.controllers[ci].agent {
            a.helper.obs = obs;
        }
    }

    fn with_helper<R>(&mut self,ci:usize,missing:R,run:impl FnOnce(&mut helpers::Controller<'_>,&mut Helper)->R)->R {
        let controller=&mut self.controllers[ci];
        let Some(agent)=controller.agent.as_mut() else{return missing};
        let mut view=helpers::Controller {index:ci,unit:controller.unit,request:&mut controller.request,control:&mut controller.nav_control,leader:&mut controller.leader};
        run(&mut view,&mut agent.helper)
    }
    pub fn helper_goto<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize,point:Vec3,mode:world::GotoMode)->() {
        self.with_helper(ci,(),|controller,helper|helpers::goto(w,controller,helper,point,mode))
    }
    pub fn helper_stop<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize)->() {
        self.with_helper(ci,(),|controller,helper|helpers::stop(w,controller,helper))
    }
    pub fn helper_terminate<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize)->() {
        self.with_helper(ci,(),|controller,helper|helpers::terminate(w,controller,helper))
    }
    pub fn helper_tick<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize,dt:f64)->() {
        self.with_helper(ci,(),|controller,helper|helpers::tick(w,controller,helper,dt))
    }
    pub fn helper_destination<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize)->Option<Vec3> {
        self.with_helper(ci,None,|controller,helper|helpers::destination(w,controller,helper))
    }
    pub fn helper_install_path<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize,points:&[Vec3])->bool {
        self.with_helper(ci,false,|controller,helper|helpers::install(w,controller,helper,points))
    }
    pub fn helper_follow<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize,target:EntRef)->() {
        self.with_helper(ci,(),|controller,helper|helpers::follow(w,controller,helper,target))
    }
    pub fn helper_follow_trail<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize,on:bool)->() {
        self.with_helper(ci,(),|controller,helper|helpers::trail(w,controller,helper,on))
    }
    pub fn helper_hold_path<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize,on:bool)->() {
        self.with_helper(ci,(),|controller,helper|helpers::hold(w,controller,helper,on))
    }
    pub fn helper_set_leader<W:helpers::World+?Sized>(&mut self,w:&mut W,ci:usize,leader:Option<EntRef>)->() {
        self.controllers[ci].leader=leader;self.with_helper(ci,(),|controller,helper|helpers::leader(w,controller,helper,leader))
    }

    // -- shared agent steps -----------------------------------------------------

    fn resolve<W:AiWorld+?Sized>(&self, w: &W, r: Option<EntRef>) -> Option<UKey> {
        controller::resolve_ref(w, r)
    }

    fn ref_of<W:AiWorld+?Sized>(&self, w: &W, unit: Option<UKey>) -> Option<EntRef> {
        controller::ref_of(w, unit)
    }

    pub(crate) fn can_engage<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, target: UKey) -> bool {
        if self.options.bind.combat {
            self.combat_call(w, ci, |w| w.combat_can_engage(ci, target))
        } else {
            true
        }
    }

    fn board_policy<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, candidate: UKey) -> bool {
        if self.options.bind.vehicles {
            if let Some(answer) = w.vehicles_board_policy(ci, candidate) {
                return answer;
            }
        }
        agents::nominal_speed(Some(&w.unit(candidate))) > 0.0
    }

    pub(crate) fn set_stance<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize,stance:f64) {
        self.controllers[ci].set_stance(stance);
        if self.controllers[ci].nav_control.is_some() {w.nav_stance_changed(ci,stance);}
    }
    pub(crate) fn request_jet<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize,vector:Vec3) {
        self.controllers[ci].request_jet(vector);
        if self.controllers[ci].nav_control.is_some() {w.nav_jet_requested(ci,vector);}
    }
    /// Agent.aim (agents.js:352), with the Enhanced facing override.
    pub(crate) fn aim<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, stance: f64, look: Option<Vec3>) {
        let mut look = look;
        let enhanced = matches!(&self.controllers[ci].agent.as_ref().map(|a| &a.kind), Some(AgentKind::Objective(o)) if o.enhanced);
        if enhanced {
            if let Some(mem) = &self.controllers[ci].enhanced {
                if mem.look_until > self.now(w) {
                    look = Some(mem.look_point);
                }
            }
        }
        self.set_stance(w,ci,stance);
        if !self.options.bind.combat {
            match look {
                Some(point) => self.controllers[ci].look_at(point),
                None => self.controllers[ci].look_along_path(),
            }
            return;
        }
        if let Some(target) = self.combat_retained(w, ci, |w| w.combat_current_target(ci)) {
            let r = self.ref_of(w, Some(target));
            self.controllers[ci].aim_at(r);
            return;
        }
        if let Some(memory) = self.combat_retained(w, ci, |w| w.combat_memory_point(ci)) {
            self.controllers[ci].look_at(memory);
        } else if let Some(point) = look {
            self.controllers[ci].look_at(point);
        } else {
            self.controllers[ci].look_along_path();
        }
    }

    pub(crate) fn update_combat_target<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        if self.options.bind.combat {
            self.combat_call(w, ci, |w| w.combat_update_target(ci));
            self.sync_combat_writes(w, ci);
        }
    }

    // -- enter / exit / tick dispatch -------------------------------------------

    fn on_enter<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, owner: Owner) {
        match owner {
            Owner::Controller(ci) => self.make_agent(w, d, ci),
            Owner::Agent(ci) => self.agent_on_enter(w, d, ci),
            Owner::Patrol(ci) => self.patrol_on_enter(w, d, ci),
            Owner::Snipe(ci) => self.snipe_on_enter(w, d, ci),
            Owner::Mine(ci) => self.mine_on_enter(w, d, ci),
        }
    }

    fn on_exit<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, owner: Owner) {
        match owner {
            Owner::Controller(ci) => {
                if self.controllers[ci].agent.is_some() {
                    self.exit_agent(w, d, ci);
                }
            }
            Owner::Agent(ci) => self.agent_on_exit(w, d, ci),
            Owner::Patrol(ci) => {
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.look = false;
                    o.patrol.stance = 0.0;
                }
            }
            Owner::Snipe(ci) => {
                // The base helper's exit is empty; the behavior
                // helper's exitState clears its aim hold (exitState is
                // not onExit: Fsm.terminate and the embedding agent's
                // exit call onExit, which classes' SnipeHelper does
                // not override).
                let _ = ci;
            }
            Owner::Mine(_) => {}
        }
    }

    fn on_tick<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, owner: Owner, dt: f64) {
        match owner {
            Owner::Controller(ci) => {
                // UnitController.onTick: agent.tick(dt, phase).
                let Some(agent) = &self.controllers[ci].agent else { return };
                let phase = self.controllers[ci].phase;
                let done = if matches!(agent.kind, AgentKind::Foreign(_)) {
                    self.feed_combat(w,ci);let shared=world::ForeignShared::read(&self.controllers[ci]);
                    let tick=w.foreign_agent_tick(self,ci,dt,phase,&shared);
                    self.apply_service_effects(w, tick.effects);
                    if let Some(shared) = tick.shared { shared.apply(&mut self.controllers[ci]); }
                    self.sync_control(w, ci);
                    self.sync_combat_writes(w, ci);
                    // A synchronous native callback may have installed a
                    // successor while the old agent's stack was still active.
                    if !tick.replaced {
                        if let Some(a) = &mut self.controllers[ci].agent {
                            a.fsm.state = tick.state;
                            a.fsm.pending = tick.pending;
                            a.helper = tick.helper;
                            a.interrupts = tick.interrupts;
                        }
                    }
                    tick.state == fsm::DONE
                } else {
                    self.fsm_update(w, d, Owner::Agent(ci), dt, phase == 0.0);
                    self.fsm_of(Owner::Agent(ci)).state == fsm::DONE
                };
                self.controllers[ci].commands.current_done = done;
            }
            Owner::Agent(ci) => {
                // Agent.onTick (agents.js:322).
                if self.controllers[ci].phase != 0.0 {
                    if self.options.bind.combat {
                        self.combat_call(w, ci, |w| w.combat_rank_targets(ci));
                    }
                    return;
                }
                self.agent_behave(w, d, ci, dt);
            }
            Owner::Patrol(ci) => self.patrol_on_tick(w, d, ci),
            Owner::Snipe(ci) => self.snipe_on_tick(w, d, ci, dt),
            Owner::Mine(ci) => self.mine_on_tick(w, d, ci, dt),
        }
    }

    // -- agent entries, exits, behaviors (agents.js) ---------------------------

    fn agent_state(&self, ci: usize) -> f64 {
        self.fsm_of(Owner::Agent(ci)).state
    }

    fn agent_unit(&self, ci: usize) -> Option<UKey> {
        self.controllers[ci].agent.as_ref().and_then(|a| a.unit)
    }

    fn agent_on_enter<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let state = self.agent_state(ci);
        let kind_tag = match &self.controllers[ci].agent.as_ref().map(|a| &a.kind) {
            Some(AgentKind::Wait { .. }) => 1,
            Some(AgentKind::WaitSecondary) => 2,
            Some(AgentKind::Random { .. }) => 3,
            Some(AgentKind::Follow { .. }) => 4,
            Some(AgentKind::Hold { .. }) => 5,
            Some(AgentKind::Board { .. }) => 6,
            Some(AgentKind::Unboard) => 7,
            Some(AgentKind::Objective(o)) => match o.flavor {
                ObjectiveFlavor::Trooper => 8,
                ObjectiveFlavor::Scout => 9,
                ObjectiveFlavor::Repair => 10,
            },
            _ => 0,
        };
        match kind_tag {
            1 => {
                // UnitWaitAgent.onEnter.
                if state != 2.0 {
                    return;
                }
                let now = self.now(w);
                if let AgentKind::Wait { since, .. } = &mut self.controllers[ci].agent.as_mut().unwrap().kind {
                    *since = now;
                }
                self.wait_pick_look(w, d, ci);
            }
            2 => {
                // UnitWaitSecondaryAgent.onEnter.
                if state != 2.0 {
                    return;
                }
                self.helper_stop(w, ci);
                if self.options.bind.vehicles {
                    if let Some(vehicle) = w.vehicles_boarded_vehicle(ci) {
                        let character = self.controllers[ci].character;
                        let target = self.ref_of(w, Some(vehicle));
                        let now = self.now(w);
                        let cmd = controller::SquadCommand { source: Some(character), kind: squads::command::BOARD, time: now, target };
                        self.send_command(w, d, cmd, 0.3);
                    }
                }
            }
            3 => {
                if state == 2.0 {
                    self.random_retry(w, d, ci, false);
                }
            }
            4 => {
                // UnitFollowAgent.onEnter.
                let leader_ref = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                    AgentKind::Follow { leader, .. } => *leader,
                    _ => None,
                };
                let leader = self.resolve(&*w, leader_ref);
                match state {
                    2.0 => {
                        let Some(leader) = leader else { return };
                        let position = w.unit_scalars(leader).position;
                        if let AgentKind::Follow { leader_point, .. } = &mut self.controllers[ci].agent.as_mut().unwrap().kind {
                            *leader_point = position;
                        }
                        self.helper_goto(w, ci, position, world::GotoMode::Route);
                        self.helper_set_leader(w, ci, leader_ref);
                        self.helper_follow_trail(w, ci, true);
                    }
                    4.0 => {
                        let Some(leader) = leader else { return };
                        let (_, far) = self.follow_radii();
                        let mut off = agents::random_cube(&mut || d.b_uniform());
                        off = v::scale(off, far);
                        off.y = 0.0;
                        if v::length_sq(off) < 1.0 {
                            off = v::normalize(off);
                        }
                        let leader_pos = w.unit_scalars(leader).position;
                        let mut point = v::add(leader_pos, off);
                        if self.options.bind.nav {
                            if let Some(unit) = self.agent_unit(ci) {
                                let (radius, mask) = self.traversal(w, unit);
                                let (_, clipped) = w.nav_clip_segment(leader_pos, point, radius, mask);
                                point = clipped;
                            }
                        }
                        self.install_hold(w, ci, point);
                        self.helper_hold_path(w, ci, true);
                    }
                    _ if state == fsm::DONE => {
                        self.helper_set_leader(w, ci, None);
                    }
                    _ => {}
                }
            }
            5 => {
                if state == 2.0 {
                    self.hold_pick_look(w, d, ci);
                }
            }
            6 => {
                // UnitBoardAgent (stub).onEnter.
                if state != 2.0 {
                    return;
                }
                let vref = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                    AgentKind::Board { vehicle } => *vehicle,
                    _ => None,
                };
                let vehicle = self.resolve(&*w, vref);
                let ok = match vehicle {
                    Some(vehicle) if self.options.bind.vehicles => w.vehicles_reserve_seat(ci, vehicle),
                    _ => false,
                };
                if !ok {
                    self.request_state(Owner::Agent(ci), fsm::DONE);
                    return;
                }
                let position = w.unit_scalars(vehicle.unwrap()).position;
                self.helper_goto(w, ci, position, world::GotoMode::Route);
            }
            7 => {
                if state == 1.0 {
                    self.request_exit(w, ci);
                }
            }
            8 | 9 | 10 => self.objective_on_enter(w, d, ci, kind_tag),
            _ => {}
        }
    }

    fn agent_on_exit<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let state = self.agent_state(ci);
        match &self.controllers[ci].agent.as_ref().map(|a| &a.kind) {
            Some(AgentKind::Random { .. }) => {
                self.controllers[ci].wanted_mode = 2.0;
            }
            Some(AgentKind::Follow { .. }) => match state {
                1.0 => {
                    let leader_ref = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                        AgentKind::Follow { leader, .. } => *leader,
                        _ => None,
                    };
                    if self.resolve(&*w, leader_ref).is_none() {
                        self.request_state(Owner::Agent(ci), fsm::DONE);
                    }
                }
                2.0 => self.helper_follow_trail(w, ci, false),
                4.0 => self.helper_hold_path(w, ci, false),
                _ => {}
            },
            Some(AgentKind::Objective(o)) => {
                let (flavor_patrol, flavor_snipe, flavor_mine) = match o.flavor {
                    ObjectiveFlavor::Repair => (4.0, 5.0, 6.0),
                    _ => (2.0, 4.0, 5.0),
                };
                if state == flavor_patrol {
                    self.patrol_exit_state(ci);
                } else if state == flavor_snipe {
                    self.snipe_exit_state(ci);
                } else if state == flavor_mine {
                    self.mine_exit_state(ci);
                }
            }
            _ => {}
        }
        let _ = d;
    }

    fn patrol_exit_state(&mut self, ci: usize) {
        if let Some(o) = self.objective_mut(ci) {
            o.patrol.look = false;
            o.patrol.stance = 0.0;
        }
    }

    fn snipe_exit_state(&mut self, ci: usize) {
        if let Some(o) = self.objective_mut(ci) {
            if o.snipe.full {
                o.snipe.block_aim = false;
                o.snipe.look = false;
            }
        }
    }

    fn mine_exit_state(&mut self, _ci: usize) {}

    fn traversal<W:AiWorld+?Sized>(&self, w: &W, unit: UKey) -> (f64, f64) {
        let nav_unit = w.unit(unit).nav_unit();
        (super::nav::units::traversal_radius(&nav_unit), super::nav::units::traversal_mask(&nav_unit) as f64)
    }

    fn install_hold<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, point: Vec3) {
        if !self.helper_install_path(w, ci, &[point, point]) {
            self.helper_goto(w, ci, point, world::GotoMode::Route);
        }
    }

    fn wait_pick_look<W:AiWorld+?Sized>(&mut self, w: &W, d: &mut dyn Draws, ci: usize) {
        let range = if self.controllers[ci].home_facing { 0.5235988 } else { 1.3089969 };
        let angle = d.a_range(-range, range);
        let dir = agents::rotate_y(self.controllers[ci].home_direction, angle);
        if let Some(unit) = self.agent_unit(ci) {
            let position = w.unit_scalars(unit).position;
            if let AgentKind::Wait { look_point, .. } = &mut self.controllers[ci].agent.as_mut().unwrap().kind {
                *look_point = v::add_scaled(position, dir, 1000.0);
            }
        }
    }

    fn hold_pick_look<W:AiWorld+?Sized>(&mut self, w: &W, d: &mut dyn Draws, ci: usize) {
        let Some(unit) = self.agent_unit(ci) else { return };
        let position = w.unit_scalars(unit).position;
        let point = if self.controllers[ci].home_facing {
            let angle = d.a_range(-0.5235988, 0.5235988);
            let dir = agents::rotate_y(self.controllers[ci].home_direction, angle);
            v::add_scaled(position, dir, 10.0)
        } else {
            let mut cube = agents::random_cube(&mut || d.a_uniform());
            cube = v::scale(cube, 10.0);
            cube.y = 0.0;
            v::add(position, cube)
        };
        if let AgentKind::Hold { look_point, .. } = &mut self.controllers[ci].agent.as_mut().unwrap().kind {
            *look_point = point;
        }
    }

    fn random_retry<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, timed: bool) {
        let Some(unit) = self.agent_unit(ci) else { return };
        let position = w.unit_scalars(unit).position;
        let kind = w.unit_scalars(unit).kind;
        if timed {
            let retried = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                AgentKind::Random { retried_at, .. } => *retried_at,
                _ => position,
            };
            if kind == 0.0 && v::distance_sq(position, retried) < 1.0 {
                self.request_jet(w,ci,Vec3::default());
            }
            if let AgentKind::Random { retried_at, .. } = &mut self.controllers[ci].agent.as_mut().unwrap().kind {
                *retried_at = position;
            }
        }
        let cube = v::scale(agents::random_cube(&mut || d.b_uniform()), 10.0);
        let point = v::add(position, cube);
        self.helper_goto(w, ci, point, world::GotoMode::Straight);
        let now = self.now(w);
        if let AgentKind::Random { last_retry, .. } = &mut self.controllers[ci].agent.as_mut().unwrap().kind {
            *last_retry = now;
        }
    }

    // -- objective agents -------------------------------------------------

    fn arrived<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, offset: Option<Vec3>) -> bool {
        let (Some(unit), Some(post)) = (self.agent_unit(ci), self.objective(ci).and_then(|o| o.post)) else { return false };
        let u = w.unit(unit);
        let forward_speed = u.soldier.and_then(|s| s.max_speed).or_else(|| u.class_def.as_ref().and_then(|c| c.max_speed)).unwrap_or(0.0);
        let radius = w.unit_sphere_radius(unit).unwrap_or(0.5);
        patrol::arrived_at_objective(&w.post(post), u.position, forward_speed, radius, offset, self.options.bind.population)
    }

    fn approach<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let unit = self.agent_unit(ci);
        let post_index = self.objective(ci).and_then(|o| o.post);
        let (Some(post_i), post) = (post_index, post_index.map(|i| w.post(i).clone())) else {
            // cpCenter with no post is (0,0,0); cpOffset would throw on
            // Population's (postAI of null): the JS reaches here only
            // with a post in practice; keep the guard narrow.
            return;
        };
        let _ = post_i;
        let post = post.unwrap();
        let population = self.options.bind.population;
        let center = patrol::cp_center(&post, population);
        let start = unit.map(|u| w.unit_scalars(u).position);
        let support_point = if self.options.bind.vehicles {
            match unit {
                Some(u) => w.vehicles_support_point(u, post_i),
                None => None,
            }
        } else {
            None
        };
        let now = self.now(w);
        let offset = if let Some(point) = support_point {
            v::sub(point, center)
        } else {
            patrol::cp_offset(&post, population, d)
        };
        let point = v::add(center, offset);
        if let Some(o) = self.objective_mut(ci) {
            if let Some(start) = start {
                o.start_position = start;
            }
            o.center = center;
            o.support = support_point.is_some();
            o.offset = offset;
            o.entry_time = now;
        }
        self.helper_goto(w, ci, point, world::GotoMode::Route);
    }

    fn objective_moved<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize) -> bool {
        let (Some(unit), Some(o)) = (self.agent_unit(ci), self.objective(ci)) else { return false };
        let Some(post) = o.post else { return false };
        let population = self.options.bind.population;
        let center_now = patrol::cp_center(&w.post(post), population);
        if v::distance_sq(center_now, o.center) <= 400.0 {
            return false;
        }
        if !(o.entry_time + 15.0 < self.now(w)) {
            return false;
        }
        let position = w.unit_scalars(unit).position;
        let to_post = v::sub(center_now, position);
        let traveled = v::sub(position, o.start_position);
        if v::dot(to_post, traveled) < 0.0 {
            return true;
        }
        let arrived = self.controllers[ci].agent.as_ref().map(|a| a.helper_arrived()).unwrap_or(false);
        arrived && self.controllers[ci].executor_status() == 0.0
    }

    fn can_snipe<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> bool {
        let flavor = self.objective(ci).map(|o| o.flavor);
        match flavor {
            Some(ObjectiveFlavor::Scout) => self.snipe_select(w, d, ci),
            _ => {
                let unit = self.agent_unit(ci);
                let ok = match unit {
                    Some(u) => {
                        let u = w.unit(u);
                        u.kind == 0.0 && !u.hero
                    }
                    None => false,
                };
                ok && self.snipe_select(w, d, ci)
            }
        }
    }

    fn can_mine<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> bool {
        self.mine_eligible(w, ci) && self.mine_select(w, d, ci)
    }

    fn objective_on_enter<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, tag: u8) {
        let state = self.agent_state(ci);
        match tag {
            8 => match state {
                1.0 => {
                    if !self.arrived(w, ci, None) {
                        self.request_state(Owner::Agent(ci), 3.0);
                        let character = self.controllers[ci].character;
                        let target = self.ref_of(w, self.agent_unit(ci));
                        let now = self.now(w);
                        let cmd = controller::SquadCommand { source: Some(character), kind: squads::command::FOLLOW, time: now, target };
                        self.send_command(w, d, cmd, 0.3);
                    } else {
                        self.request_state(Owner::Agent(ci), 2.0);
                    }
                }
                2.0 => self.patrol_start(w, d, ci),
                3.0 => {
                    if let Some(o) = self.objective_mut(ci) {
                        o.mine_tries = 0.0;
                    }
                    self.approach(w, d, ci);
                }
                4.0 => self.snipe_start(w, d, ci),
                5.0 => self.mine_start(w, d, ci),
                _ => {}
            },
            9 => match state {
                1.0 => {
                    let arrived = self.arrived(w, ci, None);
                    self.request_state(Owner::Agent(ci), if arrived { 2.0 } else { 3.0 });
                }
                4.0 => {
                    self.snipe_start(w, d, ci);
                    let deadline = self.now(w) + d.a_range(60.0, 100.0);
                    if let Some(o) = self.objective_mut(ci) {
                        o.deadline = deadline;
                    }
                }
                2.0 => self.patrol_start(w, d, ci),
                3.0 => {
                    if let Some(o) = self.objective_mut(ci) {
                        o.mine_tries = 0.0;
                    }
                    self.approach(w, d, ci);
                }
                5.0 => self.mine_start(w, d, ci),
                _ => {}
            },
            10 => match state {
                1.0 => {
                    let arrived = self.arrived(w, ci, None);
                    self.request_state(Owner::Agent(ci), if arrived { 4.0 } else { 2.0 });
                }
                2.0 => {
                    if self.job_check(w, d, ci) {
                        self.request_state(Owner::Agent(ci), 3.0);
                        return;
                    }
                    if let Some(o) = self.objective_mut(ci) {
                        o.mine_tries = 0.0;
                    }
                    self.approach(w, d, ci);
                }
                3.0 => self.start_repair_service(w, ci),
                4.0 => {
                    if self.job_check(w, d, ci) {
                        self.request_state(Owner::Agent(ci), 3.0);
                        return;
                    }
                    self.patrol_start(w, d, ci);
                }
                5.0 => self.snipe_start(w, d, ci),
                6.0 => self.mine_start(w, d, ci),
                _ => {}
            },
            _ => {}
        }
    }

    fn patrol_start<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let post = self.objective(ci).and_then(|o| o.post);
        let now = self.now(w);
        if let Some(o) = self.objective_mut(ci) {
            o.patrol.post = post;
            o.patrol.look = false;
            o.patrol.stance = 0.0;
            o.patrol.hint = None;
            o.patrol.start_time = now;
        }
        self.request_state(Owner::Patrol(ci), 1.0);
        self.fsm_flush(w, d, Owner::Patrol(ci));
    }

    fn snipe_start<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        self.request_state(Owner::Snipe(ci), 2.0);
        self.fsm_flush(w, d, Owner::Snipe(ci));
    }

    fn mine_start<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        self.request_state(Owner::Mine(ci), 2.0);
        self.fsm_flush(w, d, Owner::Mine(ci));
    }

    // -- behaviors ---------------------------------------------------------

    fn agent_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        let tag = match &self.controllers[ci].agent.as_ref().map(|a| &a.kind) {
            Some(AgentKind::Wait { .. }) => 1,
            Some(AgentKind::WaitSecondary) => 2,
            Some(AgentKind::Random { .. }) => 3,
            Some(AgentKind::Follow { .. }) => 4,
            Some(AgentKind::Hold { .. }) => 5,
            Some(AgentKind::Board { .. }) => 6,
            Some(AgentKind::Unboard) => 7,
            Some(AgentKind::Objective(o)) => match o.flavor {
                ObjectiveFlavor::Trooper => 8,
                ObjectiveFlavor::Scout => 9,
                ObjectiveFlavor::Repair => 10,
            },
            _ => 0,
        };
        match tag {
            1 => self.wait_behave(w, d, ci, dt),
            2 => self.wait_secondary_behave(w, d, ci, dt),
            3 => self.random_behave(w, d, ci, dt),
            4 => self.follow_behave(w, d, ci, dt),
            5 => self.hold_behave(w, d, ci, dt),
            6 => {
                if self.interrupts_tick(w, d, ci) {
                    return;
                }
                self.update_combat_target(w, ci);
                if self.options.bind.vehicles {
                    w.vehicles_board_tick(self,ci, dt);
                }
                self.aim(w, ci, 0.0, None);
                self.helper_tick(w, ci, dt);
            }
            7 => {
                if self.interrupts_tick(w, d, ci) {
                    return;
                }
                self.update_combat_target(w, ci);
                let boarded = self.options.bind.vehicles && w.vehicles_boarded(ci);
                if !boarded {
                    self.request_state(Owner::Agent(ci), fsm::DONE);
                }
                self.helper_tick(w, ci, dt);
            }
            8 => self.trooper_behave(w, d, ci, dt, ObjectiveFlavor::Trooper),
            9 => self.scout_behave(w, d, ci, dt),
            10 => self.repair_behave(w, d, ci, dt),
            _ => {}
        }
    }

    fn wait_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        self.update_combat_target(w, ci);
        if self.agent_state(ci) == 2.0 {
            if d.a_int(1.0, 9.0) == 2.0 {
                self.wait_pick_look(w, d, ci);
            }
            let unit = self.agent_unit(ci);
            if let Some(unit) = unit {
                if agents::nominal_speed(Some(&w.unit(unit))) == 0.0 {
                    let memory = if self.options.bind.combat { self.combat_retained(w, ci, |w| w.combat_memory_point(ci)) } else { None };
                    if memory.is_none() {
                        let since = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                            AgentKind::Wait { since, .. } => *since,
                            _ => 0.0,
                        };
                        if self.now(w) > since + self.stay_delay() {
                            self.request_exit(w, ci);
                            return;
                        }
                    } else {
                        let now = self.now(w);
                        if let AgentKind::Wait { since, .. } = &mut self.controllers[ci].agent.as_mut().unwrap().kind {
                            *since = now;
                        }
                    }
                }
            }
            let target = if self.options.bind.combat { self.combat_retained(w, ci, |w| w.combat_current_target(ci)) } else { None };
            if let Some(target) = target {
                let r = self.ref_of(w, Some(target));
                self.controllers[ci].aim_at(r);
            } else {
                let memory = if self.options.bind.combat { self.combat_retained(w, ci, |w| w.combat_memory_point(ci)) } else { None };
                let point = memory.unwrap_or_else(|| match &self.controllers[ci].agent.as_ref().unwrap().kind {
                    AgentKind::Wait { look_point, .. } => *look_point,
                    _ => Vec3::default(),
                });
                self.controllers[ci].look_at(point);
            }
        }
        self.helper_tick(w, ci, dt);
    }

    fn wait_secondary_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        self.update_combat_target(w, ci);
        if self.agent_state(ci) == 2.0 && self.options.bind.vehicles {
            if let Some(vehicle) = w.vehicles_boarded_vehicle(ci) {
                if w.vehicles_seats_full(vehicle) {
                    let now = self.now(w);
                    self.controllers[ci].secondary_wait_until = now;
                    self.request_state(Owner::Agent(ci), fsm::DONE);
                } else {
                    let team = self.controllers[ci].unit.map(|u| w.unit_scalars(u).team);
                    let now = self.now(w);
                    let until = self.controllers[ci].secondary_wait_until;
                    if now > until {
                        let pending = match team {
                            Some(team) => w.vehicles_pending_reservations(vehicle, team),
                            None => false,
                        };
                        if !pending {
                            self.request_state(Owner::Agent(ci), fsm::DONE);
                        }
                    }
                }
            }
        }
        self.helper_tick(w, ci, dt);
    }

    fn random_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        self.update_combat_target(w, ci);
        if self.agent_state(ci) == 2.0 {
            let unit = self.agent_unit(ci);
            self.controllers[ci].wanted_mode = 1.0;
            let (kind, moved) = match unit {
                Some(u) => {
                    let start = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                        AgentKind::Random { start, .. } => *start,
                        _ => Vec3::default(),
                    };
                    (w.unit_scalars(u).kind, v::distance_sq(w.unit_scalars(u).position, start))
                }
                None => (f64::NAN, 0.0),
            };
            let threshold = if kind == 3.0 { 2.0 } else { 10.0 };
            if moved > threshold * threshold && self.clear_of_barriers(w, ci) {
                self.request_state(Owner::Agent(ci), fsm::DONE);
            } else {
                let side = self.controllers[ci].avoid_side();
                let no_path = !self.controllers[ci].has_executor();
                let last_retry = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                    AgentKind::Random { last_retry, .. } => *last_retry,
                    _ => 0.0,
                };
                let timed = self.now(w) > last_retry + 3.0;
                if side != 0.0 || timed || no_path {
                    self.random_retry(w, d, ci, timed);
                }
            }
        }
        self.aim(w, ci, 0.0, None);
        self.helper_tick(w, ci, dt);
    }

    fn clear_of_barriers<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> bool {
        let Some(unit) = self.agent_unit(ci) else { return false };
        if !self.options.bind.nav {
            return true;
        }
        let position = w.unit_scalars(unit).position;
        let (radius, mask) = self.traversal(&*w, unit);
        w.nav_point_clear(position, radius, mask)
    }

    fn follow_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        self.update_combat_target(w, ci);
        let unit = self.agent_unit(ci);
        let state = self.agent_state(ci);
        let leader_ref = match &self.controllers[ci].agent.as_ref().unwrap().kind {
            AgentKind::Follow { leader, .. } => *leader,
            _ => None,
        };
        if state != fsm::DONE {
            let leader = self.resolve(&*w, leader_ref);
            let Some(leader) = leader else {
                self.request_state(Owner::Agent(ci), fsm::DONE);
                return;
            };
            if self.count_followers(w, Some(leader)) > 3.0 {
                self.request_state(Owner::Agent(ci), fsm::DONE);
            }
            let vehicle = if self.options.bind.vehicles { w.vehicles_boarded_vehicle_of(leader) } else { None };
            if let Some(vehicle) = vehicle {
                let kind = unit.map(|u| w.unit_scalars(u).kind);
                if kind != Some(3.0) {
                    let r = self.ref_of(w, Some(vehicle));
                    self.controllers[ci].commands.prepend_one(SourceGoal::board(r));
                }
                self.request_state(Owner::Agent(ci), fsm::DONE);
                return;
            }
            if let Some(unit) = unit {
                if v::distance_sq(w.unit_scalars(leader).position, w.unit_scalars(unit).position) > 400.0 {
                    let countdown = match &mut self.controllers[ci].agent.as_mut().unwrap().kind {
                        AgentKind::Follow { countdown, .. } => {
                            *countdown = v8math::fround(*countdown - 0.05);
                            *countdown
                        }
                        _ => 1.0,
                    };
                    if countdown <= 0.0 {
                        self.request_state(Owner::Agent(ci), fsm::DONE);
                    }
                }
            }
        }
        let (near_r, far_r) = self.follow_radii();
        let leader = self.resolve(&*w, leader_ref);
        match state {
            2.0 => {
                let leader_point = match &self.controllers[ci].agent.as_ref().unwrap().kind {
                    AgentKind::Follow { leader_point, .. } => *leader_point,
                    _ => Vec3::default(),
                };
                if let Some(unit) = unit {
                    if agents::horizontal_distance_sq(w.unit_scalars(unit).position, leader_point) < 0.2 {
                        self.request_state(Owner::Agent(ci), 3.0);
                        self.aim(w, ci, 0.0, None);
                        return;
                    }
                }
                self.helper_tick(w, ci, dt);
            }
            3.0 => {
                if self.controllers[ci].executor_status() != 0.0 {
                    self.request_state(Owner::Agent(ci), 2.0);
                    self.aim(w, ci, 0.0, None);
                    return;
                }
                if let (Some(leader), Some(unit)) = (leader, unit) {
                    let near = near_r + 5.0;
                    if v::distance_sq(w.unit_scalars(leader).position, w.unit_scalars(unit).position) < near * near {
                        self.request_state(Owner::Agent(ci), 4.0);
                        self.aim(w, ci, 0.0, None);
                        return;
                    }
                }
            }
            4.0 => {
                if self.controllers[ci].executor_status() != 0.0 {
                    if let Some(unit) = unit {
                        let position = w.unit_scalars(unit).position;
                        self.install_hold(w, ci, position);
                    }
                    self.aim(w, ci, 0.0, None);
                    return;
                }
                if let (Some(leader), Some(unit)) = (leader, unit) {
                    let far = far_r + 6.0;
                    if far * far < v::distance_sq(w.unit_scalars(leader).position, w.unit_scalars(unit).position) {
                        self.request_state(Owner::Agent(ci), 3.0);
                        self.aim(w, ci, 0.0, None);
                        return;
                    }
                }
            }
            _ => {}
        }
        self.aim(w, ci, 0.0, None);
    }

    fn hold_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.agent_state(ci) == fsm::DONE {
            return;
        }
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        self.update_combat_target(w, ci);
        let source = self.controllers[ci].last_command.and_then(|c| c.source);
        let source_unit = match source {
            Some(ch) => self.current_controllable(w, ch),
            None => None,
        };
        let unit = self.agent_unit(ci);
        let (Some(source_unit), Some(unit)) = (source_unit, unit) else {
            self.request_state(Owner::Agent(ci), fsm::DONE);
            return;
        };
        if v::distance_sq(w.unit_scalars(source_unit).position, w.unit_scalars(unit).position) > 400.0 {
            let countdown = match &mut self.controllers[ci].agent.as_mut().unwrap().kind {
                AgentKind::Hold { countdown, .. } => {
                    *countdown = v8math::fround(*countdown - 0.05);
                    *countdown
                }
                _ => 1.0,
            };
            if countdown <= 0.0 {
                self.request_state(Owner::Agent(ci), fsm::DONE);
                return;
            }
        }
        if self.agent_state(ci) == 2.0 {
            if d.a_int(1.0, 9.0) == 2.0 {
                self.hold_pick_look(w, d, ci);
            }
            let target = if self.options.bind.combat { self.combat_retained(w, ci, |w| w.combat_current_target(ci)) } else { None };
            if let Some(target) = target {
                let r = self.ref_of(w, Some(target));
                self.controllers[ci].aim_at(r);
            } else {
                let memory = if self.options.bind.combat { self.combat_retained(w, ci, |w| w.combat_memory_point(ci)) } else { None };
                let point = memory.unwrap_or_else(|| match &self.controllers[ci].agent.as_ref().unwrap().kind {
                    AgentKind::Hold { look_point, .. } => *look_point,
                    _ => Vec3::default(),
                });
                self.controllers[ci].look_at(point);
            }
        }
        self.helper_tick(w, ci, dt);
    }

    fn trooper_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64, _flavor: ObjectiveFlavor) {
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        let state = self.agent_state(ci);
        if (state > 0.0 && state < 5.0) || state == fsm::DONE {
            self.update_combat_target(w, ci);
        }
        match state {
            2.0 => {
                let unit = self.agent_unit(ci);
                let walker_at = match unit {
                    Some(u) => w.unit_scalars(u).kind == 2.0 && self.options.bind.vehicles && w.vehicles_is_at_at(u),
                    None => false,
                };
                if walker_at {
                    self.helper_stop(w, ci);
                } else if self.can_snipe(w, d, ci) {
                    self.request_state(Owner::Agent(ci), 4.0);
                } else {
                    self.fsm_update(w, d, Owner::Patrol(ci), dt, false);
                    let (look, stance, look_point) = match self.objective(ci) {
                        Some(o) => (o.patrol.look, o.patrol.stance, o.patrol.look_point),
                        None => (false, 0.0, Vec3::default()),
                    };
                    self.aim(w, ci, stance, if look { Some(look_point) } else { None });
                }
            }
            3.0 => {
                let offset = self.objective(ci).map(|o| o.offset);
                let arrived = self.arrived(w, ci, offset);
                let helper_arrived = self.controllers[ci].agent.as_ref().map(|a| a.helper_arrived()).unwrap_or(false);
                if !arrived && !helper_arrived {
                    if self.objective_moved(&*w, ci) {
                        self.request_state(Owner::Agent(ci), 3.0);
                    } else if self.can_snipe(w, d, ci) {
                        self.request_state(Owner::Agent(ci), 4.0);
                    } else if self.can_mine(w, d, ci) {
                        self.request_state(Owner::Agent(ci), 5.0);
                    } else {
                        self.aim(w, ci, 0.0, None);
                    }
                } else {
                    self.request_state(Owner::Agent(ci), 2.0);
                    let character = self.controllers[ci].character;
                    let now = self.now(w);
                    let cmd = controller::SquadCommand { source: Some(character), kind: squads::command::MOVE_OUT, time: now, target: None };
                    self.send_command(w, d, cmd, 1.0);
                }
            }
            4.0 => {
                self.fsm_update(w, d, Owner::Snipe(ci), dt, false);
                let snipe = self.objective(ci).map(|o| (o.snipe.fsm.done(), o.snipe.look, o.snipe.stance, o.snipe.look_point, o.snipe.full && o.snipe.fsm.state == 3.0 && o.snipe.block_aim));
                if let Some((done, look, stance, look_point, busy)) = snipe {
                    if done {
                        self.request_state(Owner::Agent(ci), 3.0);
                    } else if !busy {
                        self.aim(w, ci, stance, if look { Some(look_point) } else { None });
                    }
                }
            }
            5.0 => {
                self.fsm_update(w, d, Owner::Mine(ci), dt, false);
                let mine = self.objective(ci).map(|o| (o.mine.fsm.done(), o.mine.fsm.state, o.mine.aim_point));
                if let Some((done, mstate, aim_point)) = mine {
                    if !done {
                        if mstate == 3.0 {
                            if let Some(point) = aim_point {
                                self.controllers[ci].look_at(point);
                            } else {
                                self.controllers[ci].look_along_path();
                            }
                        } else {
                            self.controllers[ci].look_along_path();
                        }
                        self.set_stance(w,ci,0.0);
                    } else {
                        self.request_state(Owner::Agent(ci), 3.0);
                    }
                }
            }
            _ => {}
        }
        self.helper_tick(w, ci, dt);
    }

    fn scout_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        let state = self.agent_state(ci);
        if state == 1.0 || state == 2.0 || state == 3.0 || state == fsm::DONE {
            self.update_combat_target(w, ci);
        } else if state == 4.0 && self.options.bind.combat {
            self.combat_call(w, ci, |w| w.combat_update_target_selection(ci));
            self.sync_combat_writes(w, ci);
        }
        match state {
            2.0 => {
                self.fsm_update(w, d, Owner::Patrol(ci), dt, false);
                let (look, stance, look_point) = match self.objective(ci) {
                    Some(o) => (o.patrol.look, o.patrol.stance, o.patrol.look_point),
                    None => (false, 0.0, Vec3::default()),
                };
                self.aim(w, ci, stance, if look { Some(look_point) } else { None });
            }
            3.0 => {
                if self.can_snipe(w, d, ci) {
                    self.request_state(Owner::Agent(ci), 4.0);
                } else {
                    let offset = self.objective(ci).map(|o| o.offset);
                    if self.arrived(w, ci, offset) {
                        self.request_state(Owner::Agent(ci), 2.0);
                    } else if self.objective_moved(&*w, ci) {
                        self.request_state(Owner::Agent(ci), 3.0);
                    } else if self.can_mine(w, d, ci) {
                        self.request_state(Owner::Agent(ci), 5.0);
                    } else {
                        self.aim(w, ci, 0.0, None);
                    }
                }
            }
            4.0 => {
                self.fsm_update(w, d, Owner::Snipe(ci), dt, false);
                let info = self.objective(ci).map(|o| (o.snipe.fsm.done(), o.deadline, o.snipe.look, o.snipe.stance, o.snipe.look_point, o.snipe.full && o.snipe.fsm.state == 3.0 && o.snipe.block_aim));
                if let Some((done, deadline, look, stance, look_point, busy)) = info {
                    if !done && self.now(w) <= deadline {
                        if !busy {
                            self.aim(w, ci, stance, if look { Some(look_point) } else { None });
                        }
                        if self.options.bind.combat {
                            self.combat_call(w, ci, |w| w.combat_gate_fire(ci));
                            self.sync_combat_writes(w, ci);
                        }
                    } else {
                        self.request_state(Owner::Agent(ci), 3.0);
                    }
                }
            }
            5.0 => {
                self.fsm_update(w, d, Owner::Mine(ci), dt, false);
                let mine = self.objective(ci).map(|o| (o.mine.fsm.done(), o.mine.fsm.state, o.mine.aim_point));
                if let Some((done, mstate, aim_point)) = mine {
                    if !done {
                        if mstate == 3.0 {
                            if let Some(point) = aim_point {
                                self.controllers[ci].look_at(point);
                            } else {
                                self.controllers[ci].look_along_path();
                            }
                        } else {
                            self.controllers[ci].look_along_path();
                        }
                        self.set_stance(w,ci,0.0);
                    } else {
                        self.request_state(Owner::Agent(ci), 3.0);
                    }
                }
            }
            _ => {}
        }
        self.helper_tick(w, ci, dt);
    }

    fn repair_behave<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.interrupts_tick(w, d, ci) {
            return;
        }
        let state = self.agent_state(ci);
        if state == 1.0 || state == 2.0 || state == 4.0 || state == 5.0 || state == fsm::DONE {
            self.update_combat_target(w, ci);
        }
        match state {
            2.0 => {
                if self.job_check(w, d, ci) {
                    self.request_state(Owner::Agent(ci), 3.0);
                } else {
                    let offset = self.objective(ci).map(|o| o.offset);
                    if self.arrived(w, ci, offset) {
                        self.request_state(Owner::Agent(ci), 4.0);
                    } else if self.objective_moved(&*w, ci) {
                        self.request_state(Owner::Agent(ci), 2.0);
                    } else {
                        let soldier = self.agent_unit(ci).map(|u| w.unit_scalars(u).kind == 0.0).unwrap_or(false);
                        if soldier && self.snipe_select(w, d, ci) {
                            self.request_state(Owner::Agent(ci), 5.0);
                        } else if self.can_mine(w, d, ci) {
                            self.request_state(Owner::Agent(ci), 6.0);
                        } else {
                            self.aim(w, ci, 0.0, None);
                        }
                    }
                }
            }
            3.0 => {
                let result = if self.options.bind.classes { self.repair_service(w, d, ci, dt) } else { None };
                match result {
                    None => self.request_state(Owner::Agent(ci), 2.0),
                    Some((target, near)) => {
                        if near {
                            if w.unit_scalars(target).alive {
                                let r = self.ref_of(w, Some(target));
                                self.controllers[ci].aim_at(r);
                            } else {
                                let point = w.unit_aim_point(target).unwrap_or_else(|| w.unit_scalars(target).position);
                                self.controllers[ci].look_at(point);
                            }
                            self.set_stance(w,ci,0.0);
                        } else {
                            self.aim(w, ci, 0.0, None);
                        }
                    }
                }
            }
            4.0 => {
                let soldier = self.agent_unit(ci).map(|u| w.unit_scalars(u).kind == 0.0).unwrap_or(false);
                if soldier && self.snipe_select(w, d, ci) {
                    self.request_state(Owner::Agent(ci), 5.0);
                } else {
                    self.fsm_update(w, d, Owner::Patrol(ci), dt, false);
                    if self.job_check(w, d, ci) {
                        self.request_state(Owner::Agent(ci), 3.0);
                    } else {
                        let (look, stance, look_point) = match self.objective(ci) {
                            Some(o) => (o.patrol.look, o.patrol.stance, o.patrol.look_point),
                            None => (false, 0.0, Vec3::default()),
                        };
                        self.aim(w, ci, stance, if look { Some(look_point) } else { None });
                    }
                }
            }
            5.0 => {
                self.fsm_update(w, d, Owner::Snipe(ci), dt, false);
                let info = self.objective(ci).map(|o| (o.snipe.fsm.done(), o.snipe.look, o.snipe.stance, o.snipe.look_point, o.snipe.full && o.snipe.fsm.state == 3.0 && o.snipe.block_aim));
                if let Some((done, look, stance, look_point, busy)) = info {
                    if done {
                        self.request_state(Owner::Agent(ci), 2.0);
                    } else if !busy {
                        self.aim(w, ci, stance, if look { Some(look_point) } else { None });
                    }
                }
            }
            6.0 => {
                self.fsm_update(w, d, Owner::Mine(ci), dt, false);
                if self.job_check(w, d, ci) {
                    self.request_state(Owner::Agent(ci), 3.0);
                } else {
                    let mine = self.objective(ci).map(|o| (o.mine.fsm.done(), o.mine.fsm.state, o.mine.aim_point));
                    if let Some((done, mstate, aim_point)) = mine {
                        if done {
                            self.request_state(Owner::Agent(ci), 2.0);
                        } else {
                            if mstate == 3.0 {
                                if let Some(point) = aim_point {
                                    self.controllers[ci].look_at(point);
                                } else {
                                    self.controllers[ci].look_along_path();
                                }
                            } else {
                                self.controllers[ci].look_along_path();
                            }
                            self.set_stance(w,ci,0.0);
                        }
                    }
                }
            }
            _ => {}
        }
        self.helper_tick(w, ci, dt);
    }

    fn job_check<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> bool {
        let now = self.now(w);
        let job_at = self.objective(ci).map(|o| o.job_at).unwrap_or(0.0);
        if !(job_at <= now && now != job_at) {
            return false;
        }
        if let Some(o) = self.objective_mut(ci) {
            o.job_at = now + 5.0;
        }
        let job = if self.options.bind.classes { self.find_repair_job(w, d, ci) } else { None };
        if let Some(o) = self.objective_mut(ci) {
            o.job = job;
        }
        job.is_some()
    }

    // -- patrol machine (patrol.js PatrolHelper) --------------------------------

    fn patrol_friendly_post<W:AiWorld+?Sized>(&self, w: &W, ci: usize) -> bool {
        let (Some(unit), Some(o)) = (self.agent_unit(ci), self.objective(ci)) else { return false };
        let Some(post) = o.patrol.post else { return false };
        self.relation(w.unit_scalars(unit).team, w.post(post).team) > 0.0
    }

    fn patrol_on_enter<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let state = self.fsm_of(Owner::Patrol(ci)).state;
        let unit = self.agent_unit(ci);
        let population = self.options.bind.population;
        match state {
            1.0 => {
                if unit.is_some() && self.options.bind.vehicles && w.vehicles_boarded(ci) {
                    let patrol = self.objective(ci).map(|o| o.patrol.clone()).unwrap_or_else(patrol::Patrol::new);
                    let mut shared = world::ForeignShared::read(&self.controllers[ci]);
                    let mut objectives=w.objective_buffer();
                    for i in 0..self.character_count {objectives.push(w.character(i).objective);}
                    let support_holds = self.objective(ci).and_then(|o| o.support_holds);
                    let after = w.vehicles_patrol_mounted(self,ci, patrol, &mut shared, &objectives, support_holds);
                    shared.apply(&mut self.controllers[ci]);
                    match after {
                        Some(after) => {
                            for (character, post) in after.publications {
                                self.objectives_publish(w, character, post);
                            }
                            if let Some(o) = self.objective_mut(ci) {
                                o.patrol = after.patrol;
                                o.support_holds = after.support_holds;
                            }
                            self.sync_control(w, ci);
                        }
                        None => {
                            self.request_exit(w, ci);
                            let now = self.now(w);
                            self.controllers[ci].reboard_at = now + 5.0;
                            self.request_state(Owner::Patrol(ci), fsm::DONE);
                        }
                    }
                    return;
                }
                if !self.patrol_friendly_post(&*w, ci) {
                    self.request_state(Owner::Patrol(ci), 2.0);
                    return;
                }
                let draw = d.a_int(1.0, 4.0);
                let kind = unit.map(|u| w.unit_scalars(u).kind);
                if !(draw != 1.0 && kind == Some(0.0)) {
                    self.request_state(Owner::Patrol(ci), 4.0);
                    return;
                }
                let post = self.objective(ci).and_then(|o| o.patrol.post);
                let Some(post) = post else {
                    self.request_state(Owner::Patrol(ci), 4.0);
                    return;
                };
                let post_snap = w.post(post).clone();
                let candidates = patrol::query_patrol_hints(w, self.options.bind.nav, population, &post_snap, unit);
                let previous = self.objective(ci).and_then(|o| o.patrol.hint);
                let now = self.now(w);
                let hint = patrol::choose_hint(w, self.options.bind.nav, &candidates, previous, now, d);
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.candidates = candidates;
                    o.patrol.hint = hint;
                }
                self.request_state(Owner::Patrol(ci), if hint.is_some() { 3.0 } else { 4.0 });
            }
            2.0 => {
                let post = self.objective(ci).and_then(|o| o.patrol.post);
                let Some(post) = post else { return };
                let post_snap = w.post(post).clone();
                let center = patrol::cp_center(&post_snap, population);
                let offset = patrol::cp_offset(&post_snap, population, d);
                self.helper_goto(w, ci, v::add(center, offset), world::GotoMode::Route);
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.centered = false;
                    o.patrol.expires = 0.0;
                }
            }
            3.0 => {
                let hint = self.objective(ci).and_then(|o| o.patrol.hint);
                if let Some(hint) = hint {
                    w.nav_mark_hint(hint, unit, false);
                    let position = w.nav_hint(hint).position;
                    self.helper_goto(w, ci, position, world::GotoMode::Route);
                }
                let expires = self.now(w) + d.a_int(30.0, 45.0);
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.expires = expires;
                }
            }
            4.0 => {
                let expires = self.now(w) + d.a_int(20.0, 30.0);
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.expires = expires;
                }
                let kind = unit.map(|u| w.unit_scalars(u).kind);
                let post = self.objective(ci).and_then(|o| o.patrol.post);
                if kind != Some(0.0) && kind != Some(1.0) && kind.is_some() {
                    let arrived = match (unit, post) {
                        (Some(u), Some(p)) => {
                            let snap = w.unit(u);
                            let forward_speed = snap.soldier.and_then(|s| s.max_speed).or_else(|| snap.class_def.as_ref().and_then(|c| c.max_speed)).unwrap_or(0.0);
                            let radius = w.unit_sphere_radius(u).unwrap_or(0.5);
                            patrol::arrived_at_objective(&w.post(p), snap.position, forward_speed, radius, None, population)
                        }
                        _ => false,
                    };
                    if arrived {
                        self.helper_stop(w, ci);
                        self.request_state(Owner::Patrol(ci), 6.0);
                        return;
                    }
                }
                let Some(post) = post else { return };
                let post_snap = w.post(post).clone();
                let center = patrol::cp_center(&post_snap, population);
                let z = v8math::fround(d.a_uniform() - 0.5);
                let y = v8math::fround(d.a_uniform() - 0.5);
                let x = v8math::fround(d.a_uniform() - 0.5);
                let mut off = Vec3::new(x * 2.0, y * 2.0, z * 2.0);
                off.y = 0.0;
                off = v::scale(off, d.a_range(8.0, 14.0));
                self.helper_goto(w, ci, v::add(center, off), world::GotoMode::Direct);
            }
            5.0 => {
                let hint = self.objective(ci).and_then(|o| o.patrol.hint);
                let (forward, hint_pos, mask) = match hint {
                    Some(h) => {
                        let snap = w.nav_hint(h);
                        (snap.forward, snap.position, snap.primary_stance)
                    }
                    None => (Vec3::new(0.0, 0.0, 1.0), Vec3::default(), 1.0),
                };
                let base = self.helper_destination(w, ci).unwrap_or(hint_pos);
                let stance = patrol::pick_hint_stance(mask, d);
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.look = true;
                    o.patrol.look_point = v::add(base, v::scale(forward, 10.0));
                    o.patrol.stance = stance;
                }
            }
            6.0 => {
                if let Some(o) = self.objective_mut(ci) {
                    o.patrol.stance = 0.0;
                }
            }
            _ => {}
        }
    }

    fn patrol_on_tick<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let state = self.fsm_of(Owner::Patrol(ci)).state;
        if state == fsm::DONE {
            return;
        }
        let now = self.now(w);
        let population = self.options.bind.population;
        let start_time = self.objective(ci).map(|o| o.patrol.start_time).unwrap_or(0.0);
        if start_time + 5.0 < now {
            if let Some(o) = self.objective_mut(ci) {
                o.patrol.start_time = patrol::REVALIDATE_SENTINEL;
            }
            let post = self.objective(ci).and_then(|o| o.patrol.post);
            let character = self.controllers[ci].character;
            let team = w.character(character).team;
            let stale = match post {
                None => true,
                Some(p) => {
                    let snap = w.post(p);
                    !patrol::has_capture_region_pop(&snap, population) || patrol::friendly_spawn_eligible(&snap, team, population, &|a, b| self.relation(a, b))
                }
            };
            if stale {
                self.request_objective(w, d, ci, true);
                self.request_state(Owner::Patrol(ci), fsm::DONE);
                return;
            }
        }
        match state {
            2.0 => {
                if self.patrol_friendly_post(&*w, ci) {
                    self.request_state(Owner::Patrol(ci), 1.0);
                    return;
                }
                let helper = self.controllers[ci].agent.as_ref().map(|a| (a.helper_arrived(), a.helper_failed()));
                let unit = self.agent_unit(ci);
                let post = self.objective(ci).and_then(|o| o.patrol.post);
                let inside = match (unit, post) {
                    (Some(u), Some(p)) => goals::post_contains(&w.post(p), w.unit_scalars(u).position),
                    _ => false,
                };
                let has_region = post.map(|p| patrol::has_capture_region_pop(&w.post(p), population)).unwrap_or(false);
                if let (Some((true, false)), Some(_)) = (helper, unit) {
                    if has_region && !inside {
                        let (centered, expires) = match self.objective(ci) {
                            Some(o) => (o.patrol.centered, o.patrol.expires),
                            None => (false, 0.0),
                        };
                        if !centered {
                            if let Some(o) = self.objective_mut(ci) {
                                o.patrol.centered = true;
                            }
                            let p = post.unwrap();
                            let center = patrol::cp_center(&w.post(p), population);
                            self.helper_goto(w, ci, center, world::GotoMode::Route);
                        } else if expires == 0.0 {
                            if let Some(o) = self.objective_mut(ci) {
                                o.patrol.expires = now + 20.0;
                            }
                        } else if expires < now {
                            if self.request_objective(w, d, ci, true).is_some() {
                                self.request_state(Owner::Patrol(ci), fsm::DONE);
                            } else {
                                self.request_state(Owner::Patrol(ci), 1.0);
                            }
                        }
                    }
                }
                return;
            }
            3.0 => {
                let arrived = self.controllers[ci].agent.as_ref().map(|a| a.helper_arrived()).unwrap_or(false);
                if arrived {
                    self.request_state(Owner::Patrol(ci), 5.0);
                    return;
                }
                if let Some(hint) = self.objective(ci).and_then(|o| o.patrol.hint) {
                    w.nav_mark_hint(hint, self.agent_unit(ci), false);
                }
            }
            4.0 => {
                let arrived = self.controllers[ci].agent.as_ref().map(|a| a.helper_arrived()).unwrap_or(false);
                if arrived {
                    self.request_state(Owner::Patrol(ci), 6.0);
                    return;
                }
            }
            5.0 => {
                let expires = self.objective(ci).map(|o| o.patrol.expires).unwrap_or(0.0);
                if expires < now {
                    self.request_state(Owner::Patrol(ci), 1.0);
                    return;
                }
                if let Some(hint) = self.objective(ci).and_then(|o| o.patrol.hint) {
                    w.nav_mark_hint(hint, self.agent_unit(ci), false);
                }
                return;
            }
            6.0 => {}
            _ => return,
        }
        let expires = self.objective(ci).map(|o| o.patrol.expires).unwrap_or(0.0);
        if expires < now {
            self.request_state(Owner::Patrol(ci), 1.0);
        }
        let _ = d;
    }

    // -- snipe and mine machines (classes.js over agents.js bases) -------------

    fn mark_hint<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, hint: Option<usize>) {
        if let Some(hint) = hint {
            w.nav_mark_hint(hint, self.agent_unit(ci), true);
        }
    }

    fn hint_available_at<W:AiWorld+?Sized>(&self, w: &W, hint: usize) -> bool {
        // classes.js available(): the raw reservedAt rule, not the
        // bag's hintAvailable.
        !(w.time() < w.nav_hint(hint).reserved_at.unwrap_or(-10.0) + 4.0)
    }

    fn low_tickets<W:AiWorld+?Sized>(&self, w: &W, team: f64) -> bool {
        let count = w.tickets(team).or_else(|| w.reinforcements(team));
        (team == 1.0 || team == 2.0) && count.map(|c| c <= 25.0).unwrap_or(false)
    }

    /// classes.js query(): candidates within range (+ radius),
    /// predicate applied, capped. `include_dead` uses the service
    /// candidates (buildings included).
    fn classes_query<W:AiWorld+?Sized>(&mut self, w: &mut W, center: Vec3, range: f64, cap: usize, include_dead: bool, pred: &dyn Fn(&Core, &W, UKey) -> bool) -> crate::ai::owned::scratch::List<UKey> {
        let candidates = if include_dead {
            w.service_candidates(center, range)
        } else {
            // The predicate runs inside the registry query too; the
            // outer filter re-applies it (classes.js:26). Snapshot the
            // pure answers so the query can borrow the world mutably.
            let mut accepted=w.predicate_buffer();
            for index in 0..w.units_len() {let key=w.unit_key(index);accepted.push((key,pred(self,&*w,key)));}
            w.units_query(center, range, &mut |unit| accepted.iter().find(|(key,_)|*key==unit.key).is_some_and(|(_,accepted)|*accepted))
        };
        let mut out = w.unit_buffer();
        for key in candidates {
            let u = w.unit(key);
            let r = range + w.unit_sphere_radius(key).or(u.radius).unwrap_or(0.);
            if v::distance_sq(u.position, center) <= r * r && pred(self, &*w, key) {
                out.push(key);
                if out.len() >= cap {
                    break;
                }
            }
        }
        out
    }

    fn snipe_target_ok<W:AiWorld+?Sized>(&self, w: &W, owner: UKey, candidate: UKey) -> bool {
        let o = w.unit(owner);
        let c = w.unit(candidate);
        if candidate == owner || !c.alive || c.targetable == Some(false) || c.kind == 10.0 || c.mine {
            return false;
        }
        if let Some(cd) = &c.class_def {
            if let Some(base) = &cd.base {
                if base == "mine" || base == "item" || base == "powerupitem" {
                    return false;
                }
            }
        }
        let mh = c.class_def.as_ref().and_then(|cd| cd.max_health).or(c.max_health).or(c.combat_max_health).unwrap_or(1.0);
        if !(mh < classes::FLT_MAX) {
            return false;
        }
        self.relation(o.team, c.team) < 0.0
    }

    fn snipe_suitable<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, hint: usize) -> bool {
        let Some(unit) = self.agent_unit(ci) else { return false };
        let team = w.unit_scalars(unit).team;
        if self.low_tickets(&*w, team) {
            return false;
        }
        let attacking_team = self.controls().map(|c| c.attacking_team).unwrap_or(1.0);
        let attacker = self.relation(team, attacking_team) > 0.0;
        let hint_snap = w.nav_hint(hint);
        let r = if hint_snap.mode != 0.0 {
            let range = self.controls().map(|c| if attacker { c.snipe_attacker } else { c.snipe_defender }).unwrap_or(140.0);
            range / 2.0
        } else {
            70.0
        };
        let center = v::add_scaled(hint_snap.position, hint_snap.forward, r);
        let found = self.classes_query(w, center, r, 175, false, &|core, ww, key| core.relation(team, ww.unit_scalars(key).team) < 0.0);
        found.iter().any(|&key| self.snipe_target_ok(w, unit, key))
    }

    fn snipe_corridor_clear<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, hint: usize) -> bool {
        let Some(unit) = self.agent_unit(ci) else { return true };
        if w.network_secondary_active() {
            return true;
        }
        let hint_snap = w.nav_hint(hint);
        let position = w.unit_scalars(unit).position;
        let team = w.unit_scalars(unit).team;
        let mut delta = v::sub(hint_snap.position, position);
        let length = v::length(delta);
        let center = v::add_scaled(position, delta, 0.5);
        delta = v::normalize(delta);
        let found = self.classes_query(w, center, length / 2.0, 175, false, &|core, ww, key| core.relation(team, ww.unit_scalars(key).team) < 0.0);
        !found.iter().any(|&key| {
            let c = v::cross(v::sub(w.unit_scalars(key).position, position), delta);
            v::length_sq(c) < 144.0 && self.snipe_target_ok(w, unit, key)
        })
    }

    /// selectSnipe (classes.js:74). Returns true and sets the hint.
    fn select_snipe_inner<W:AiWorld+?Sized>(&mut self, w: &mut W, _d: &mut dyn Draws, ci: usize) -> bool {
        let Some(unit) = self.agent_unit(ci) else { return false };
        if !self.options.bind.nav {
            return false;
        }
        let position = w.unit_scalars(unit).position;
        let mut list = w.nav_query_hints(patrol::HINT_SNIPE, position, 100.0, 20.0, None);
        let team = w.unit_scalars(unit).team;
        let attacking_team = self.controls().map(|c| c.attacking_team).unwrap_or(1.0);
        let attacker = self.relation(team, attacking_team) > 0.0;
        let scout = self.objective(ci).map(|o| o.snipe.scout).unwrap_or(false);
        let mut i = 0;
        while i < list.len() {
            let mode = w.nav_hint(list[i]).mode;
            let drop = if mode == 0.0 {
                !scout
            } else if mode == 1.0 {
                !attacker
            } else if mode == 2.0 {
                attacker
            } else {
                false
            };
            if drop {
                let last = list.len() - 1;
                list[i] = list[last];
                list.pop();
            } else {
                i += 1;
            }
        }
        let mut heap = classes::FloatHeap::new(20);
        for &hint in &list {
            let dist = v::distance_sq(w.nav_hint(hint).position, position);
            heap.insert(-dist, hint);
        }
        let mut attempts = 0;
        while heap.size() > 0 {
            let Some(hint) = heap.pop() else { break };
            if !self.hint_available_at(w, hint) {
                continue;
            }
            if attempts > 10 {
                break;
            }
            attempts += 1;
            if self.snipe_suitable(w, ci, hint) && self.snipe_corridor_clear(w, ci, hint) {
                if let Some(o) = self.objective_mut(ci) {
                    o.snipe.hint = Some(hint);
                }
                return true;
            }
        }
        false
    }

    /// SnipeHelper.select: the base delegates; classes' gates on the
    /// retry time and marks a found hint (classes.js:99).
    fn snipe_select<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> bool {
        if !self.options.bind.classes {
            return false;
        }
        let full = self.objective(ci).map(|o| o.snipe.full).unwrap_or(false);
        if full {
            let now = self.now(w);
            let retry_at = self.objective(ci).and_then(|o| o.snipe.retry_at).unwrap_or(-1.0);
            if !(now > retry_at) {
                return false;
            }
            let next = now + 8.0 + d.a_range(0.0, 4.0);
            if let Some(o) = self.objective_mut(ci) {
                o.snipe.retry_at = Some(next);
            }
            let found = self.select_snipe_inner(w, d, ci);
            if found {
                let hint = self.objective(ci).and_then(|o| o.snipe.hint);
                self.mark_hint(w, ci, hint);
            }
            return found;
        }
        self.select_snipe_inner(w, d, ci)
    }

    fn snipe_in_forward_cone<W:AiWorld+?Sized>(&self, w: &W, ci: usize, point: Option<Vec3>) -> bool {
        let Some(o) = self.objective(ci) else { return false };
        if !o.snipe.full {
            return false;
        }
        let (Some(point), Some(hint), Some(unit)) = (point, o.snipe.hint, self.agent_unit(ci)) else { return false };
        let to = v::normalize(v::sub(point, w.unit_scalars(unit).position));
        let forward = v::normalize(w.nav_hint(hint).forward);
        v::dot(to, forward) > 0.21643958985805511
    }

    fn snipe_on_enter<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let full = self.objective(ci).map(|o| o.snipe.full).unwrap_or(false);
        if !full {
            return;
        }
        let state = self.fsm_of(Owner::Snipe(ci)).state;
        if state == 2.0 {
            if let Some(hint) = self.objective(ci).and_then(|o| o.snipe.hint) {
                let position = w.nav_hint(hint).position;
                self.helper_goto(w, ci, position, world::GotoMode::Route);
            }
        } else if state == 3.0 {
            let mask = self.objective(ci).and_then(|o| o.snipe.hint).map(|h| w.nav_hint(h).primary_stance).unwrap_or(1.0);
            let stance = pick_stance(mask, d);
            if let Some(o) = self.objective_mut(ci) {
                o.snipe.stance = stance;
                o.snipe.revalidate_at = 0.0;
            }
        }
    }

    fn snipe_retry<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) {
        let found = self.select_snipe_inner(w, d, ci);
        self.request_state(Owner::Snipe(ci), if found { 2.0 } else { fsm::DONE });
    }

    fn snipe_on_tick<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, _dt: f64) {
        let full = self.objective(ci).map(|o| o.snipe.full).unwrap_or(false);
        if !full {
            return;
        }
        let state = self.fsm_of(Owner::Snipe(ci)).state;
        if state == 2.0 {
            let hint = self.objective(ci).and_then(|o| o.snipe.hint);
            self.mark_hint(w, ci, hint);
            let (arrived, status, failed) = match &self.controllers[ci].agent {
                Some(a) => (a.helper_arrived(), a.helper_status(), a.helper_failed()),
                None => (false, 0.0, false),
            };
            if arrived {
                if status != 0.0 || failed {
                    self.snipe_retry(w, d, ci);
                } else {
                    self.request_state(Owner::Snipe(ci), 3.0);
                }
            }
        } else if state == 3.0 {
            let now = self.now(w);
            let revalidate_at = self.objective(ci).map(|o| o.snipe.revalidate_at).unwrap_or(0.0);
            if now > revalidate_at {
                if let Some(o) = self.objective_mut(ci) {
                    o.snipe.revalidate_at = now + 15.0;
                }
                let hint = self.objective(ci).and_then(|o| o.snipe.hint);
                let suitable = match hint {
                    Some(h) => self.snipe_suitable(w, ci, h),
                    None => false,
                };
                if !suitable {
                    self.snipe_retry(w, d, ci);
                    return;
                }
            }
            let hint = self.objective(ci).and_then(|o| o.snipe.hint);
            self.mark_hint(w, ci, hint);
            let (Some(hint), Some(unit)) = (hint, self.agent_unit(ci)) else { return };
            let forward = v::normalize(w.nav_hint(hint).forward);
            let look_point = v::add_scaled(w.unit_scalars(unit).position, forward, 10.0);
            if let Some(o) = self.objective_mut(ci) {
                o.snipe.look_point = look_point;
            }
            let point = if self.options.bind.combat {
                match self.combat_retained(w, ci, |w| w.combat_current_target(ci)) {
                    Some(t) => Some(w.unit_scalars(t).position),
                    None => self.combat_retained(w, ci, |w| w.combat_memory_point(ci)).or(Some(look_point)),
                }
            } else {
                Some(look_point)
            };
            let block = !self.snipe_in_forward_cone(&*w, ci, point);
            let stance = self.objective(ci).map(|o| o.snipe.stance).unwrap_or(0.0);
            if let Some(o) = self.objective_mut(ci) {
                o.snipe.block_aim = block;
                o.snipe.look = !block;
            }
            if block {
                self.controllers[ci].look_at(look_point);
                self.set_stance(w,ci,stance);
            }
            self.controllers[ci].range_scale = 2.0;
        }
    }
}

impl Core {
    // -- mine machine and repair services (classes.js) ---------------------

    fn mine_weapon<W:AiWorld+?Sized>(&self, w: &W, ci: usize) -> Option<usize> {
        w.classes_mine_weapon(self.agent_unit(ci)?)
    }

    fn mine_eligible<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize) -> bool {
        if !self.options.bind.classes {
            return false;
        }
        let full = self.objective(ci).map(|o| o.mine.full).unwrap_or(false);
        if full {
            self.mine_weapon(w, ci).is_some()
        } else {
            // Base MineHelper.eligible delegates to classes.canMine,
            // which is mineWeapon's presence.
            self.mine_weapon(w, ci).is_some()
        }
    }

    fn mine_placement<W:AiWorld+?Sized>(&self, w: &W, ci: usize, hint: usize) -> Option<Vec3> {
        let unit = self.agent_unit(ci)?;
        let team = w.unit_scalars(unit).team;
        let snap = w.nav_hint(hint);
        let r = snap.radius;
        if !(r > 0.0) {
            return None;
        }
        let p = snap.position;
        // queryItems caps its spatial results before filtering mines.
        let mut nearby=[Vec3::default();30];let mut mines=0;let mut visited=0;
        for index in 0..w.items_len() {
            let item=w.item(index);
            if item.gone || !(self.relation(team,item.team)>0.) {continue;}
            let rr=r+6.+item.radius.unwrap_or(0.);
            if v::distance_sq(item.position,p)<=rr*rr {
                visited+=1;
                if item.mine||item.def_base.as_deref()==Some("mine") {nearby[mines]=item.position;mines+=1;}
                if visited==30 {break;}
            }
        }
        let h2 = |a: Vec3, b: Vec3| {
            let dx = a.x - b.x;
            let dz = a.z - b.z;
            dx * dx + dz * dz
        };
        let mut x = p.x - r;
        while x < p.x + r {
            let mut z = p.z - r;
            while z < p.z + r {
                let candidate = Vec3::new(x, p.y, z);
                if h2(candidate, p) < r * r && nearby[..mines].iter().all(|&m| h2(candidate, m) >= 5.0625) {
                    return Some(candidate);
                }
                z += 3.0;
            }
            x += 3.0;
        }
        None
    }

    fn mine_select<W:AiWorld+?Sized>(&mut self, w: &mut W, _d: &mut dyn Draws, ci: usize) -> bool {
        if !self.options.bind.classes {
            return false;
        }
        // selectMine (classes.js:184) runs for both helper flavors
        // (the base delegates to it).
        let now = self.now(w);
        let retry_at = self.objective(ci).and_then(|o| o.mine.retry_at).unwrap_or(-1.0);
        if !(now > retry_at) {
            return false;
        }
        if let Some(o) = self.objective_mut(ci) {
            o.mine.retry_at = Some(now + 10.0);
        }
        let Some(unit) = self.agent_unit(ci) else { return false };
        if !self.options.bind.nav {
            return false;
        }
        let position = w.unit_scalars(unit).position;
        let team = w.unit_scalars(unit).team;
        let mut candidates = w.nav_query_hints(patrol::HINT_MINEFIELD, position, 100.0, 3.0, Some(unit));
        let population = self.options.bind.population;
        let mut i = 0;
        while i < candidates.len() {
            let post = w.nav_hint(candidates[i]).post;
            let drop = match post {
                Some(p) => !patrol::friendly_spawn_eligible(&w.post(p), team, population, &|a, b| self.relation(a, b)),
                None => false,
            };
            if drop {
                let last = candidates.len() - 1;
                candidates[i] = candidates[last];
                candidates.pop();
            } else {
                i += 1;
            }
        }
        for hint in candidates {
            if !self.hint_available_at(w, hint) {
                continue;
            }
            let snap = w.nav_hint(hint);
            let mut center = snap.position;
            if self.options.bind.mine_ground {
                center.y = w.mine_ground_height(center);
            }
            let radius = snap.radius + 6.0;
            let mut hostile=None;
            let found = self.classes_query(w, center, radius, 1, false, &|core, ww, key| core.relation(team, ww.unit_scalars(key).team) < 0.0);
            for &key in &found {
                hostile=Some(w.unit(key).mine);
            }
            if hostile.is_none() {
                for index in 0..w.items_len() {
                    let item=w.item(index);
                    if item.gone || !(self.relation(team, item.team) < 0.0) {
                        continue;
                    }
                    let rr = radius + item.radius.unwrap_or(0.0);
                    if v::distance_sq(item.position, center) <= rr * rr {
                        hostile=Some(item.mine || item.def_base.as_deref() == Some("mine"));
                        break;
                    }
                }
            }
            if hostile == Some(true) {
                continue;
            }
            if let Some(placement) = self.mine_placement(&*w, ci, hint) {
                if let Some(o) = self.objective_mut(ci) {
                    o.mine.hint = Some(hint);
                    o.mine.placement = Some(placement);
                }
                self.mark_hint(w, ci, Some(hint));
                return true;
            }
        }
        false
    }

    fn mine_on_enter<W:AiWorld+?Sized>(&mut self, w: &mut W, _d: &mut dyn Draws, ci: usize) {
        let full = self.objective(ci).map(|o| o.mine.full).unwrap_or(false);
        if !full {
            return;
        }
        let state = self.fsm_of(Owner::Mine(ci)).state;
        if state == 2.0 {
            let placement = self.objective(ci).and_then(|o| o.mine.placement);
            let Some(placement) = placement else { return };
            let mut aim_point = placement;
            if self.options.bind.mine_ground {
                aim_point.y = w.mine_ground_height(placement) + 1.5;
            }
            if let Some(o) = self.objective_mut(ci) {
                o.mine.aim_point = Some(aim_point);
            }
            self.helper_goto(w, ci, placement, world::GotoMode::Route);
        } else if state == 3.0 {
            self.helper_stop(w, ci);
            if let Some(o) = self.objective_mut(ci) {
                o.mine.fired_at = None;
            }
        }
    }

    fn choose_weapon<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, weapon: usize) {
        let (has_fire, _) = w.perception_flags(ci);
        if has_fire {
            self.combat_call(w, ci, |w| w.weapons_switch(ci, weapon));
        } else {
            self.combat_call(w, ci, |w| w.weapons_select(ci, weapon));
        }
    }

    fn mine_on_tick<W:AiWorld+?Sized>(&mut self, w: &mut W, _d: &mut dyn Draws, ci: usize, _dt: f64) {
        let full = self.objective(ci).map(|o| o.mine.full).unwrap_or(false);
        if !full {
            return;
        }
        let state = self.fsm_of(Owner::Mine(ci)).state;
        if state != 2.0 && state != 3.0 {
            return;
        }
        let hint = self.objective(ci).and_then(|o| o.mine.hint);
        self.mark_hint(w, ci, hint);
        if state == 2.0 {
            let (arrived, status, failed) = match &self.controllers[ci].agent {
                Some(a) => (a.helper_arrived(), a.helper_status(), a.helper_failed()),
                None => (false, 0.0, false),
            };
            if arrived {
                if status != 0.0 || failed {
                    if let Some(h) = hint {
                        w.nav_hint_set_reserved(h, self.now(w) + 30.0);
                    }
                    self.request_state(Owner::Mine(ci), fsm::DONE);
                } else {
                    self.request_state(Owner::Mine(ci), 3.0);
                }
            }
            return;
        }
        let Some(weapon) = self.mine_weapon(&*w, ci) else {
            self.request_state(Owner::Mine(ci), fsm::DONE);
            return;
        };
        // selectedWeapon / chooseWeapon cross to the combat package.
        let selected = self.combat_query(w, ci, |w| w.weapons_selected(ci));
        if selected != Some(weapon) {
            self.choose_weapon(w, ci, weapon);
            return;
        }
        if let Some(point) = self.objective(ci).and_then(|o| o.mine.aim_point) {
            self.controllers[ci].look_at(point);
        }
        let fired_at = self.objective(ci).and_then(|o| o.mine.fired_at);
        let now = self.now(w);
        match fired_at {
            None => {
                self.controllers[ci].fire_a = true;
                if let Some(o) = self.objective_mut(ci) {
                    o.mine.fired_at = Some(now);
                }
            }
            Some(fired) => {
                if now > fired + 3.0 {
                    let placement = hint.and_then(|h| self.mine_placement(&*w, ci, h));
                    match placement {
                        Some(p) => {
                            if let Some(o) = self.objective_mut(ci) {
                                o.mine.placement = Some(p);
                            }
                            self.request_state(Owner::Mine(ci), 2.0);
                        }
                        None => self.request_state(Owner::Mine(ci), fsm::DONE),
                    }
                }
            }
        }
    }

    // -- repair services -------------------------------------------------------

    fn resolve_job<W:AiWorld+?Sized>(&self, w: &W, job: Option<world::JobRef>) -> Option<UKey> {
        let job = job?;
        let target = job.target;
        let u = w.unit(target.unit);
        if u.generation != target.generation {
            return None;
        }
        if classes::service_alive(&u) {
            Some(target.unit)
        } else {
            None
        }
    }

    fn service_weapon<W:AiWorld+?Sized>(&self, w: &W, ci: usize, target: UKey, ammo: bool) -> Option<usize> {
        w.classes_service_weapon(self.agent_unit(ci)?,classes::health_type(&w.unit(target)),ammo)
    }

    fn repair_reserved<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize, target: UKey, ammo: bool) -> bool {
        let now = self.now(w);
        let unit = self.agent_unit(ci);
        self.repair.reservations.retain(|r| {
            if !(now < r.time + 3.0) {
                return false;
            }
            let target_ok = {
                let u = w.unit(r.target.unit);
                u.generation == r.target.generation && classes::service_alive(&u)
            };
            let owner_ok = {
                let u = w.unit(r.owner.unit);
                u.alive && u.generation == r.owner.generation
            };
            target_ok && owner_ok
        });
        self.repair
            .reservations
            .iter()
            .any(|r| r.target.unit == target && r.ammo == ammo && Some(r.owner.unit) != unit)
    }

    fn futile_for<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize, target: UKey, ammo: bool) -> bool {
        let now = self.now(w);
        self.repair.futile.retain(|r| now < r.until);
        let unit = self.agent_unit(ci);
        match unit {
            Some(u) => {
                let generation = w.unit_scalars(u).generation;
                self.repair.futile.iter().any(|r| r.owner == u && r.generation == generation && r.target == target && r.ammo == ammo)
            }
            None => false,
        }
    }

    fn useful_kinds<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize, target: UKey) -> crate::ai::owned::scratch::Inline<bool,2> {
        let mut out = crate::ai::owned::scratch::Inline::default();
        for ammo in [false, true] {
            if w.classes_needs(target,ammo)
                && self.service_weapon(w, ci, target, ammo).is_some()
                && !self.repair_reserved(w, ci, target, ammo)
                && !self.futile_for(w, ci, target, ammo)
            {
                out.push(ammo);
            }
        }
        out
    }

    fn find_repair_job<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> Option<world::JobRef> {
        let unit = self.agent_unit(ci)?;
        let position = w.unit_scalars(unit).position;
        let team = w.unit_scalars(unit).team;
        if self.low_tickets(&*w, team) {
            return None;
        }
        let candidates = self.classes_query(w, position, 75.0, 50, true, &|core, ww, key| {
            key != unit && classes::service_alive(&ww.unit(key)) && core.relation(team, ww.unit_scalars(key).team) >= 0.0
        });
        let mut best: Option<world::JobRef> = None;
        let mut best_d = f64::INFINITY;
        for target in candidates {
            let useful = self.useful_kinds(&*w, ci, target);
            if useful.is_empty() {
                continue;
            }
            let ammo = if useful.len() == 2 {
                useful[d.a_int(0.0, 1.0) as usize]
            } else {
                useful[0]
            };
            let t = w.unit(target);
            let distance = v::distance_sq(t.position, position);
            let speed = t.velocity.map(v::length_sq).unwrap_or(0.0);
            let human = t.human || t.character.map(|ch| ch<self.character_count && w.character(ch).human).unwrap_or(false) || t.human_piloted;
            let forward = w.unit_forward(target).unwrap_or(Vec3::new(0.0, 0.0, 1.0));
            let facing = v::dot(forward, v::normalize(v::sub(position, t.position))) > v8math::cos(15.0);
            let job = world::JobRef { target: EntRef { unit: target, generation: t.generation }, ammo };
            if human && distance < 900.0 && (speed < 0.25 || facing) {
                return Some(job);
            }
            if speed < 0.25 && (human || t.kind != 0.0) && distance < best_d {
                best = Some(job);
                best_d = distance;
            }
        }
        best
    }

    fn start_repair_service<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        if !self.options.bind.classes {
            return;
        }
        let now = self.now(w);
        if let Some(o) = self.objective_mut(ci) {
            o.service = Some(agents::ServiceState {
                approach_at: now,
                progress_at: f64::INFINITY,
                saved_health: 0.0,
                toward: None,
                closing: false,
                workable: None,
            });
        }
        self.helper_stop(w, ci);
    }

    fn give_up<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize, target: UKey, ammo: bool) -> Option<(UKey, bool)> {
        if let Some(unit) = self.agent_unit(ci) {
            self.repair.futile.push(classes::Futile {
                owner: unit,
                generation: w.unit_scalars(unit).generation,
                target,
                ammo,
                until: self.now(w) + 30.0,
            });
        }
        None
    }

    fn repair_approach<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, target: UKey) {
        let alive = w.unit_scalars(target).alive;
        let toward = self.objective(ci).and_then(|o| o.service.as_ref().and_then(|s| s.toward));
        let has_follow = matches!(self.helper_kind(ci), Some(HelperKind::Ground) | Some(HelperKind::Vehicle) | Some(HelperKind::Stationary));
        if has_follow && alive {
            let generation = w.unit_scalars(target).generation;
            self.helper_follow(w, ci, EntRef { unit: target, generation });
            return;
        }
        let position = w.unit_scalars(target).position;
        let stale = match toward {
            None => true,
            Some(t) => agents::horizontal_distance_sq(t, position) > 1e-4,
        };
        if stale {
            if let Some(o) = self.objective_mut(ci) {
                if let Some(s) = &mut o.service {
                    s.toward = Some(position);
                }
            }
            self.helper_goto(w, ci, position, world::GotoMode::Route);
        }
    }

    fn repair_close_in<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, target: UKey) {
        let closing = self.objective(ci).and_then(|o| o.service.as_ref().map(|s| s.closing)).unwrap_or(false);
        if closing {
            return;
        }
        if let Some(o) = self.objective_mut(ci) {
            if let Some(s) = &mut o.service {
                s.closing = true;
                s.toward = None;
            }
        }
        let Some(unit) = self.agent_unit(ci) else { return };
        let p = w.unit_scalars(unit).position;
        let c = w.unit_scalars(target).position;
        let stop = w.unit_sphere_radius(target).unwrap_or(0.0) + 1.5;
        let dx = p.x - c.x;
        let dz = p.z - c.z;
        let dist = v8math::hypot(dx, dz);
        if dist <= stop {
            self.helper_stop(w, ci);
        } else {
            let point = Vec3::new(c.x + (dx * stop) / dist, p.y, c.z + (dz * stop) / dist);
            self.helper_goto(w, ci, point, world::GotoMode::Straight);
        }
    }

    fn nearby_service<W:AiWorld+?Sized>(&self, w: &W, target: UKey, ammo: bool) -> bool {
        let ty = classes::health_type(&w.unit(target)) as usize;
        if ty >= 7 {
            return false;
        }
        let position = w.unit_scalars(target).position;
        (0..w.items_len())
            .map(|index|w.item(index))
            .filter(|item| {
                if item.gone {
                    return false;
                }
                let rr = 15.0 + item.radius.unwrap_or(0.0);
                v::distance_sq(item.position, position) <= rr * rr
            })
            .take(10)
            .any(|item| {
                item.def_base.as_deref() == Some("powerupitem") && classes::quantity(&item.qty, ty as f64, ammo) > 0.0
            })
    }

    /// repairService (classes.js:355): (target, near) or None.
    fn repair_service<W:AiWorld+?Sized>(&mut self, w: &mut W, _d: &mut dyn Draws, ci: usize, _dt: f64) -> Option<(UKey, bool)> {
        let job = self.objective(ci).and_then(|o| o.job)?;
        let target = self.resolve_job(&*w, Some(job))?;
        let ammo = job.ammo;
        if !w.classes_needs(target,ammo) || self.nearby_service(&*w, target, ammo) {
            return None;
        }
        let weapon = self.service_weapon(&*w, ci, target, ammo)?;
        let unit = self.agent_unit(ci)?;
        let now = self.now(w);
        let distance = agents::horizontal_distance_sq(w.unit_scalars(unit).position, w.unit_scalars(target).position);
        let current = w.combat_repair_target(unit).or_else(|| self.controllers[ci].target.map(|t| t.unit));
        let selected = self.combat_query(w, ci, |w| w.weapons_selected(ci));
        let range_weapon = selected.unwrap_or(weapon);
        let range = self.combat_query(w, ci, |w| w.weapons_effective_range(ci, range_weapon));
        let locked = current.is_some_and(|current| w.repair_matches(target,current));
        let close = distance < range * range && (locked || distance < 25.0);
        // Reservation touch.
        let owner_ref = EntRef { unit, generation: w.unit_scalars(unit).generation };
        if let Some(r) = self.repair.reservations.iter_mut().find(|r| r.owner.unit == unit && r.target.unit == target && r.ammo == ammo) {
            r.time = now;
        } else {
            self.repair.reservations.push(classes::Reservation { target: job.target, ammo, owner: owner_ref, time: now });
        }
        self.controllers[ci].target = Some(job.target);
        if w.unit_scalars(target).alive {
            let r = Some(job.target);
            self.controllers[ci].aim_at(r);
        } else {
            let point = w.unit_aim_point(target).unwrap_or_else(|| w.unit_scalars(target).position);
            self.controllers[ci].look_at(point);
        }
        if !close {
            let (approach_at, has_path, status) = {
                let s = self.objective(ci).and_then(|o| o.service.as_ref());
                (s.map(|s| s.approach_at).unwrap_or(0.0), self.controllers[ci].has_executor(), self.controllers[ci].agent.as_ref().map(|a| a.helper_status()).unwrap_or(0.0))
            };
            if now > approach_at + 20.0 && (!has_path || status != 0.0) {
                return self.give_up(&*w, ci, target, ammo);
            }
            self.repair_approach(w, ci, target);
            if let Some(o) = self.objective_mut(ci) {
                if let Some(s) = &mut o.service {
                    s.closing = false;
                    s.workable = None;
                    s.progress_at = f64::INFINITY;
                }
            }
            return Some((target, false));
        }
        let weapon_is_repair = w.classes_repair_weapon(unit,weapon);
        if weapon_is_repair && !locked {
            self.repair_close_in(w, ci, target);
        } else {
            self.helper_stop(w, ci);
            if let Some(o) = self.objective_mut(ci) {
                if let Some(s) = &mut o.service {
                    s.toward = None;
                    s.closing = false;
                }
            }
        }
        if let Some(o) = self.objective_mut(ci) {
            if let Some(s) = &mut o.service {
                s.approach_at = now;
                if s.workable.is_none() {
                    s.workable = Some(now);
                }
            }
        }
        if self.combat_query(w, ci, |w| w.weapons_selected(ci)) != Some(weapon) {
            self.choose_weapon(w, ci, weapon);
            if let Some(o) = self.objective_mut(ci) {
                if let Some(s) = &mut o.service {
                    s.progress_at = f64::INFINITY;
                }
            }
            return Some((target, true));
        }
        let (_, has_aimer) = w.perception_flags(ci);
        let has_origin_forward = w.unit_has_origin_forward(unit);
        if has_aimer && has_origin_forward {
            let aim = w.unit_aim_point(target).unwrap_or_else(|| w.unit_scalars(target).position);
            if !self.combat_query(w, ci, |w| w.weapons_firing_cone(ci, aim)) {
                let workable = self.objective(ci).and_then(|o| o.service.as_ref().and_then(|s| s.workable)).unwrap_or(now);
                if let Some(o) = self.objective_mut(ci) {
                    if let Some(s) = &mut o.service {
                        s.progress_at = f64::INFINITY;
                    }
                }
                if now > workable + 8.0 {
                    return self.give_up(&*w, ci, target, ammo);
                }
                return Some((target, true));
            }
        }
        let (progress_at, saved) = match self.objective(ci).and_then(|o| o.service.as_ref()) {
            Some(s) => (s.progress_at, s.saved_health),
            None => (f64::INFINITY, 0.0),
        };
        if let Some(o) = self.objective_mut(ci) {
            if let Some(s) = &mut o.service {
                s.workable = Some(now);
            }
        }
        let first_action = progress_at > now;
        let mut progress_at = progress_at;
        let mut saved = saved;
        if progress_at > now {
            progress_at = now;
            saved = classes::unit_health(&w.unit(target));
        }
        if now > progress_at + 8.0 {
            if classes::unit_health(&w.unit(target)) <= saved {
                if let Some(o) = self.objective_mut(ci) {
                    if let Some(s) = &mut o.service {
                        s.progress_at = progress_at;
                        s.saved_health = saved;
                    }
                }
                return self.give_up(&*w, ci, target, ammo);
            }
            saved = classes::unit_health(&w.unit(target));
            progress_at = now;
        }
        if let Some(o) = self.objective_mut(ci) {
            if let Some(s) = &mut o.service {
                s.progress_at = progress_at;
                s.saved_health = saved;
            }
        }
        if weapon_is_repair || first_action {
            self.controllers[ci].fire_a = true;
        }
        Some((target, true))
    }
}

impl Core {
    // -- alert policies (agents.js virtuals, patrol/snipe/mine policies,
    // enhanced/agents.js focused) -----------------------------------------

    fn active_helper(&self, ci: usize) -> u8 {
        // 1 patrol, 2 snipe, 3 mine, 0 none.
        let Some(o) = self.objective(ci) else { return 0 };
        let state = self.agent_state(ci);
        match o.flavor {
            ObjectiveFlavor::Repair => {
                if state == 4.0 { 1 } else if state == 5.0 { 2 } else if state == 6.0 { 3 } else { 0 }
            }
            _ => {
                if state == 2.0 { 1 } else if state == 4.0 { 2 } else if state == 5.0 { 3 } else { 0 }
            }
        }
    }

    pub fn accepts_hazard<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize)->bool {
        let input=policy::Input::read(&self.controllers[ci]);let mut state=world::ResponseState::read(&self.controllers[ci]);
        let result=policy::accepts_hazard(input,&mut state,&mut PolicyScene {core:self,world:w,controller:ci});state.apply(&mut self.controllers[ci]);result
    }
    pub fn accepts_alert<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize,known:bool,point:Vec3)->bool {
        let input=policy::Input::read(&self.controllers[ci]);let mut state=world::ResponseState::read(&self.controllers[ci]);
        let result=policy::accepts_alert(input,&mut state,&mut PolicyScene {core:self,world:w,controller:ci},known,point);state.apply(&mut self.controllers[ci]);result
    }
    pub fn accepts_target<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize,target:UKey)->bool {
        if let Some(result)=w.agent_target_override(ci,target) {return result;}
        let input=policy::Input::read(&self.controllers[ci]);let mut state=world::ResponseState::read(&self.controllers[ci]);
        let result=policy::accepts_target(input,&mut state,&mut PolicyScene {core:self,world:w,controller:ci},target);state.apply(&mut self.controllers[ci]);result
    }
    pub fn accepts_acoustic<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize,source:UKey)->bool {
        let input=policy::Input::read(&self.controllers[ci]);let mut state=world::ResponseState::read(&self.controllers[ci]);
        let result=policy::accepts_acoustic(input,&mut state,&mut PolicyScene {core:self,world:w,controller:ci},source);state.apply(&mut self.controllers[ci]);result
    }
    pub fn accepts_board<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize,candidate:UKey)->bool {
        if let Some(result)=w.agent_board_override(ci,candidate) {return result;}
        let input=policy::Input::read(&self.controllers[ci]);policy::accepts_board(input,&mut PolicyScene {core:self,world:w,controller:ci},candidate)
    }

    pub fn accepts_secondary_wait(&self, ci: usize) -> bool {
        match &self.controllers[ci].agent.as_ref().map(|a| &a.kind) {
            Some(AgentKind::Wait { .. }) => true,
            Some(AgentKind::Random { .. }) => true,
            Some(AgentKind::Foreign(world::ForeignKind::Fly))=>true,
            Some(AgentKind::Objective(o)) => match o.flavor {
                ObjectiveFlavor::Scout | ObjectiveFlavor::Repair => true,
                ObjectiveFlavor::Trooper => match self.active_helper(ci) {
                    1 => false,
                    2 => {
                        let state3 = o.snipe.fsm.state == 3.0;
                        !state3
                    }
                    3 => false,
                    _ => true,
                },
            },
            _ => false,
        }
    }

    pub fn accepts_recovery(&self, ci: usize) -> bool {
        !matches!(&self.controllers[ci].agent.as_ref().map(|a| &a.kind), Some(AgentKind::Random { .. }))
    }

    pub fn accepts_command(&self, ci: usize, kind: f64) -> bool {
        match &self.controllers[ci].agent.as_ref().map(|a| &a.kind) {
            Some(AgentKind::Follow { .. }) => kind != squads::command::FOLLOW,
            _ => true,
        }
    }

    pub fn saves_on_interrupt(&self, ci: usize, goal_kind: f64) -> bool {
        match &self.controllers[ci].agent.as_ref().map(|a| &a.kind) {
            Some(AgentKind::WaitSecondary) => false,
            Some(AgentKind::Random { .. }) => false,
            Some(AgentKind::Board { .. }) => false,
            Some(AgentKind::Unboard) => false,
            Some(AgentKind::MissingCombat) => false,
            Some(AgentKind::Foreign(world::ForeignKind::Combat|world::ForeignKind::Board|world::ForeignKind::Unboard))=>false,
            Some(AgentKind::Follow { .. }) | Some(AgentKind::Hold { .. }) => !(goal_kind == goal::FOLLOW || goal_kind == goal::HOLD),
            _ => true,
        }
    }

    fn on_move_out<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize) {
        if matches!(&self.controllers[ci].agent.as_ref().map(|a| &a.kind), Some(AgentKind::WaitSecondary)) {
            self.controllers[ci].secondary_wait_until = self.now(w) - 2.0;
        }
    }

    // -- the interrupt pass (interrupts.js) --------------------------------------

    fn alerts_open<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> bool {
        if self.options.bind.vehicles && w.vehicles_alerts_blocked(ci) {
            return false;
        }
        let unit = self.controllers[ci].unit;
        if let Some(unit) = unit {
            if w.unit_scalars(unit).kind == 5.0 && self.options.bind.nav {
                let (radius, _) = self.traversal(&*w, unit);
                let padding = v8math::max(15.0, radius);
                let position = w.unit_scalars(unit).position;
                if w.nav_in_danger_region(position, padding) {
                    return false;
                }
            }
        }
        true
    }

    fn armed<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> bool {
        if self.options.bind.combat {
            self.combat_query(w, ci, |w| w.combat_has_weapon(ci))
        } else {
            true
        }
    }

    fn turret_cannot_reach<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, point: Vec3) -> bool {
        let Some(unit) = self.controllers[ci].unit else { return false };
        // interrupts.js nominalSpeed: soldier?.maxSpeed ?? MaxSpeed (no
        // ForwardSpeed fallback there).
        let u = w.unit(unit);
        let speed = u.soldier.and_then(|s| s.max_speed).or_else(|| u.class_def.as_ref().and_then(|c| c.max_speed)).unwrap_or(0.0);
        if speed != 0.0 || self.stay_in_turrets() {
            return false;
        }
        if self.options.bind.vehicles {
            !w.vehicles_mount_reach(unit, point)
        } else {
            false
        }
    }

    fn adopt_response<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, result: Option<world::Response>) -> Option<SourceGoal> {
        let result = result?;
        let goal = SourceGoal::combat(result.mode, result.point, result.target);
        let unit = self.controllers[ci].unit;
        if result.alert {
            if let Some(unit) = unit {
                if w.unit_scalars(unit).kind == 0.0 {
                    w.emit_vo(unit, "heardEnemy", "HeardEnemySound", -1.0, None, None, None);
                }
            }
        }
        if result.exit {
            self.request_exit(w, ci);
            return None;
        }
        Some(goal)
    }

    /// InterruptHelper.tick: true when a check fired.
    pub fn interrupts_tick<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> bool {
        let mut fired: Option<(SourceGoal, f64)> = None;
        let checks: [f64; 8] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        for priority in checks {
            let goal = match priority {
                1.0 => self.int_danger(w, ci),
                2.0 => self.int_shot(w, d, ci),
                3.0 => self.int_target(w, ci),
                4.0 => self.int_acoustic(w, ci),
                5.0 => self.int_command(w, ci),
                6.0 => self.int_boarding(w, d, ci),
                7.0 => self.int_secondary_wait(w, ci),
                _ => self.int_recovery(w, ci),
            };
            // Danger/shot/acoustic with a bound response return
            // Some(goal) only when adopted (adopt handles exit).
            if let Some(goal) = goal {
                fired = Some((goal, priority));
                break;
            }
            // The command check may end a follow/hold without a goal
            // (agent requested DONE inside); it still counts as not
            // fired, as the JavaScript returns `accepted`.
        }
        let Some((goal, priority)) = fired else { return false };
        if let Some(a) = &mut self.controllers[ci].agent {
            a.interrupts.last = priority;
        }
        // push (interrupts.js:103).
        if self.saves_on_interrupt(ci, goal.kind) {
            let saved = self.controllers[ci].commands.current_copy();
            self.controllers[ci].commands.prepend_two(saved, goal);
        } else {
            self.controllers[ci].commands.prepend_one(goal);
        }
        true
    }

    fn int_danger<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> Option<SourceGoal> {
        if !self.alerts_open(w, ci) {
            return None;
        }
        if self.options.bind.combat {
            let mut state = world::ResponseState::read(&self.controllers[ci]);
            let result = self.combat_call(w, ci, |w| w.combat_hazard_response(ci, &mut state));
            state.apply(&mut self.controllers[ci]);
            return self.adopt_response(w, ci, result);
        }
        let c = &self.controllers[ci];
        if !c.danger_pending {
            return None;
        }
        if !self.armed(w, ci) || !self.accepts_hazard(w, ci) {
            return None;
        }
        let unit = self.controllers[ci].unit?;
        if w.unit_scalars(unit).kind != 0.0 {
            return None;
        }
        let point = self.controllers[ci].danger_point;
        Some(SourceGoal::combat(controller::combat_mode::EVADE, Some(point), None))
    }

    fn int_shot<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> Option<SourceGoal> {
        if !self.alerts_open(w, ci) {
            return None;
        }
        if self.options.bind.combat {
            let mut state = world::ResponseState::read(&self.controllers[ci]);
            let result = self.combat_call(w, ci, |w| w.combat_alert_response(ci, &mut state));
            state.apply(&mut self.controllers[ci]);
            return self.adopt_response(w, ci, result);
        }
        // The local shotResponse (interrupts.js:187).
        let (pending, source_ch, known) = {
            let c = &self.controllers[ci];
            (c.shot_pending, c.shot_source, c.shot_known)
        };
        let unit = self.controllers[ci].unit?;
        if !pending {
            return None;
        }
        // This unbound fallback asks for a Character's current control;
        // direct unit sources are handled by the combat binding.
        let world::AlertSource::Character(source_ch) = source_ch? else { return None };
        let source = self.current_controllable(w, source_ch)?;
        let self_pos = w.unit_scalars(unit).position;
        let point = w.unit_scalars(source).position;
        if !self.armed(w, ci) || !self.accepts_alert(w, ci, known, point) {
            return None;
        }
        let range = if self.options.bind.combat {
            self.combat_query(w, ci, |w| w.combat_sensor_range(ci, source))
        } else {
            self.options.fallback_sensor_range
        };
        if v::distance_sq(point, self_pos) < range * range {
            let engage = !self.options.bind.combat || self.can_engage(w, ci, source);
            if engage {
                if self.turret_cannot_reach(w, ci, point) {
                    self.request_exit(w, ci);
                    return None;
                }
                return Some(SourceGoal::combat(controller::combat_mode::LOCATION, Some(point), None));
            }
        }
        if w.unit_scalars(unit).kind == 0.0 {
            let cube = agents::random_cube(&mut || d.b_uniform());
            return Some(SourceGoal::combat(controller::combat_mode::EVADE, Some(v::add(cube, self_pos)), None));
        }
        None
    }

    fn int_target<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> Option<SourceGoal> {
        if !self.alerts_open(w, ci) {
            return None;
        }
        let unit = self.controllers[ci].unit;
        let target = if self.options.bind.combat { self.combat_retained(w, ci, |w| w.combat_acquired_target(ci)) } else { None }?;
        if !self.accepts_target(w, ci, target) {
            return None;
        }
        if let Some(unit) = unit {
            if w.unit_scalars(unit).kind == 5.0 && self.options.bind.vehicles {
                let team = w.unit_scalars(unit).team;
                if w.vehicles_target_reserved(target, team) {
                    return None;
                }
                w.vehicles_reserve_target(target, unit);
            }
            if w.unit_scalars(unit).kind == 3.0 && v::distance_sq(w.unit_scalars(target).position, w.unit_scalars(unit).position) > 100.0 {
                return None;
            }
        }
        // combat.setTarget is not bound (a silent gap, measured).
        let r = self.ref_of(w, Some(target));
        Some(SourceGoal::combat(controller::combat_mode::TARGET, None, r))
    }

    fn int_acoustic<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> Option<SourceGoal> {
        if !self.alerts_open(w, ci) {
            return None;
        }
        if self.options.bind.combat {
            let mut state = world::ResponseState::read(&self.controllers[ci]);
            let result = self.combat_call(w, ci, |w| w.combat_acoustic_response(ci, &mut state));
            state.apply(&mut self.controllers[ci]);
            return self.adopt_response(w, ci, result);
        }
        let unit = self.controllers[ci].unit?;
        let self_pos = w.unit_scalars(unit).position;
        let list:crate::ai::owned::scratch::Inline<EntRef,20>=self.controllers[ci].acoustic.iter().copied().collect();
        for r in list {
            let Some(source) = self.resolve(&*w, Some(r)) else { continue };
            if !self.armed(w, ci) || !self.accepts_acoustic(w, ci, source) {
                continue;
            }
            let point = w.unit_scalars(source).position;
            let mut range = if self.options.bind.combat {
                self.combat_query(w, ci, |w| w.combat_acoustic_range(ci))
            } else {
                self.options.fallback_sensor_range
            };
            if w.unit_scalars(unit).kind == 3.0 {
                range *= 0.5;
            }
            if agents::horizontal_distance_sq(self_pos, point) < range * range {
                if w.unit_scalars(unit).kind == 0.0 {
                    w.emit_vo(unit, "heardEnemy", "HeardEnemySound", -1.0, None, None, None);
                }
                if self.turret_cannot_reach(w, ci, point) {
                    self.request_exit(w, ci);
                    return None;
                }
                return Some(SourceGoal::combat(controller::combat_mode::LOCATION, Some(point), None));
            }
        }
        None
    }

    fn int_command<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) -> Option<SourceGoal> {
        let (pending, record) = {
            let c = &self.controllers[ci];
            (c.command_pending, c.command_record)
        };
        if !pending {
            return None;
        }
        let command = record?;
        if !self.accepts_command(ci, command.kind) {
            return None;
        }
        self.controllers[ci].commands.remove_kind(goal::FOLLOW);
        self.controllers[ci].commands.remove_kind(goal::HOLD);
        if let Some(unit) = self.controllers[ci].unit {
            w.emit_vo(unit, "tac_yes", "AISCResponseYessirSound", command.kind, None, None, None);
        }
        let mut accepted: Option<SourceGoal> = None;
        match command.kind {
            k if k == squads::command::MOVE_OUT || k == squads::command::AT_EASE => {
                self.on_move_out(&*w, ci);
                let current_kind = self.controllers[ci].commands.current.kind;
                let last_source = self.controllers[ci].last_command.and_then(|c| c.source);
                if (current_kind == goal::FOLLOW || current_kind == goal::HOLD) && last_source == command.source && command.source.is_some() {
                    self.request_state(Owner::Agent(ci), fsm::DONE);
                }
            }
            k if k == squads::command::HOLD => {
                let refused = self.options.bind.vehicles && w.vehicles_refuses_hold(ci);
                if !refused {
                    accepted = Some(SourceGoal::new(goal::HOLD));
                }
            }
            k if k == squads::command::FOLLOW => {
                if let Some(target) = command.target {
                    if let Some(source) = command.source {
                        if let Some(leader) = self.current_controllable(w, source) {
                            let mine = self.count_followers(w, self.controllers[ci].unit);
                            let target_unit = self.resolve(&*w, Some(target));
                            let theirs = self.count_followers(w, target_unit);
                            if theirs < 3.0 && mine < 2.0 {
                                let leader_ref = self.ref_of(w, Some(leader));
                                accepted = Some(SourceGoal::follow(Some(target), leader_ref));
                            }
                        }
                    }
                }
            }
            k if k == squads::command::BOARD => {
                accepted = Some(SourceGoal::board(command.target));
            }
            k if k == squads::command::UNBOARD => {
                self.controllers[ci].reboard_at = self.now(w) + 20.0;
                accepted = Some(SourceGoal::new(goal::UNBOARD));
            }
            _ => {}
        }
        self.controllers[ci].last_command = Some(command);
        accepted
    }

    fn int_boarding<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize) -> Option<SourceGoal> {
        let unit = self.controllers[ci].unit;
        if let Some(unit) = unit {
            if w.unit_scalars(unit).kind == 5.0 && self.options.bind.nav {
                let (radius, _) = self.traversal(&*w, unit);
                let padding = v8math::max(15.0, radius);
                let position = w.unit_scalars(unit).position;
                if w.nav_in_danger_region(position, padding) {
                    return None;
                }
            }
            if w.unit_scalars(unit).kind != 0.0 {
                return None;
            }
        } else {
            return None;
        }
        let count = self.controllers[ci].vehicle_notes.len();
        if count == 0 { return None; }
        let unit = unit.unwrap();
        let pick = d.a_int(0.0, count as f64 - 1.0) as usize;
        let candidate = self.resolve(&*w, self.controllers[ci].vehicle_notes.get(pick).copied())?;
        if !self.options.bind.vehicles {
            return None;
        }
        if let Some(can) = w.vehicles_can_enter(ci, candidate) {
            if !can {
                return None;
            }
        }
        let team = w.unit_scalars(unit).team;
        if !(w.vehicles_free_seats(candidate, team, 1.0) > 0.0) {
            return None;
        }
        if !self.accepts_board(w, ci, candidate) {
            return None;
        }
        if !(self.controllers[ci].reboard_at < self.now(w)) {
            return None;
        }
        let goal = SourceGoal::board(self.ref_of(w, Some(candidate)));
        w.vehicles_reserve(candidate, unit, 1.0);
        Some(goal)
    }

    fn int_secondary_wait<W:AiWorld+?Sized>(&self, w: &W, ci: usize) -> Option<SourceGoal> {
        if !(self.now(w) < self.controllers[ci].secondary_wait_until) {
            return None;
        }
        if !self.accepts_secondary_wait(ci) {
            return None;
        }
        Some(SourceGoal::new(goal::WAIT_SECONDARY))
    }

    fn int_recovery<W:AiWorld+?Sized>(&self, w: &W, ci: usize) -> Option<SourceGoal> {
        if !self.accepts_recovery(ci) {
            return None;
        }
        let kind = self.controllers[ci].unit.map(|u| w.unit_scalars(u).kind);
        if kind != Some(0.0) && kind != Some(3.0) {
            return None;
        }
        if self.controllers[ci].executor_status() == 0.0 {
            return None;
        }
        Some(SourceGoal::new(goal::RECOVERY))
    }

    /// The charge test (interrupts.js:379).
    pub fn interrupts_charging<W:AiWorld+?Sized>(&mut self,w:&mut W,ci:usize)->bool {
        let input=policy::Input::read(&self.controllers[ci]);let mut state=world::ResponseState::read(&self.controllers[ci]);
        let result=policy::charging(input,&mut state.interrupts,&mut PolicyScene {core:self,world:w,controller:ci});state.apply(&mut self.controllers[ci]);result
    }

    pub fn query_allies<W:AiWorld+?Sized>(&mut self, w: &mut W, unit: UKey, radius: f64, cap: f64) -> crate::ai::owned::scratch::List<UKey> {
        let team = w.unit_scalars(unit).team;
        let position = w.unit_scalars(unit).position;
        let teams = &self.teams;
        let mut found = w.units_query(position, radius, &mut |other| other.key != unit && teams.relation(team, other.team) > 0.0);
        if (found.len() as f64) > cap {
            found.truncate(cap as usize);
        }
        found
    }
}

impl Core {
    // -- squads (squads.js) -----------------------------------------------------

    pub fn current_controllable<W:AiWorld+?Sized>(&mut self, w: &mut W, character: usize) -> Option<UKey> {
        if character >= self.character_count {
            return None;
        }
        if self.options.bind.vehicles {
            if let Some(u) = w.vehicles_current_controllable(character) {
                return Some(u);
            }
        }
        w.character(character).unit
    }

    pub fn count_followers<W:AiWorld+?Sized>(&mut self, w: &mut W, leader: Option<UKey>) -> f64 {
        let mut count = 0.0;
        for ch in 0..self.character_count {
            if self.current_controllable(w, ch).is_none() {
                continue;
            }
            let following = match self.controller_of_character(ch) {
                Some(ci) => {
                    let r = self.controllers[ci].leader;
                    if r.is_some() {
                        self.resolve(&*w, r)
                    } else {
                        None
                    }
                }
                None => None,
            };
            if following == leader {
                count += 1.0;
            }
        }
        count + 1.0
    }

    fn command_vo<W:AiWorld+?Sized>(&mut self, w: &mut W, command: controller::SquadCommand, context: f64) {
        let Some(source) = command.source else { return };
        let Some(sender) = self.current_controllable(w, source) else { return };
        let human = w.character(source).human;
        let Some(entry) = squads::vo_entry(command.kind, context, human) else { return };
        w.emit_vo(sender, entry.suffix, &entry.property, command.kind, Some(context), Some(!human), Some(human));
    }

    /// sendCommand (squads.js:132).
    pub fn send_command<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, mut command: controller::SquadCommand, probability: f64) -> bool {
        if command.kind == squads::command::FOLLOW && self.options.bind.vehicles {
            if let Some(source) = command.source {
                if let Some(vehicle) = w.vehicles_boarded_vehicle_of_character(source) {
                    command.kind = squads::command::BOARD;
                    command.target = Some(EntRef { unit: vehicle, generation: w.unit_scalars(vehicle).generation });
                }
            }
        }
        let roll = d.b_uniform();
        if roll <= probability {
            self.distribute_command(w, command);
            return true;
        }
        false
    }

    /// distributeCommand (squads.js:151). Returns delivered controller
    /// indices.
    pub fn distribute_command<W:AiWorld+?Sized>(&mut self, w: &mut W, command: controller::SquadCommand) -> crate::ai::owned::scratch::List<usize> {
        if w.paused() {
            return w.index_buffer();
        }
        let Some(source) = command.source else { return w.index_buffer() };
        let Some(sender) = self.current_controllable(w, source) else { return w.index_buffer() };
        if command.kind == squads::command::BOARD && self.options.bind.vehicles {
            let empty = w.vehicles_empty_seats(sender);
            if empty == 0.0 {
                return w.index_buffer();
            }
        }
        let boarded = if self.options.bind.vehicles {
            w.vehicles_boarded_vehicle_of_character(source)
        } else {
            None
        };
        let mut context = 0.0;
        if boarded.is_some() {
            context = if self.options.bind.vehicles && w.vehicles_is_mounted_turret(sender) { 3.0 } else { 1.0 };
        }
        self.command_vo(w, command, context);
        // secondaryNetworkWorld() is false (classic/index.js:381).
        let human = w.character(source).human;
        if !human && (command.kind < squads::command::BOARD || command.kind > squads::command::UNBOARD) {
            return w.index_buffer();
        }
        let mut delivered = w.index_buffer();
        if context == 0.0 || command.kind == squads::command::BOARD {
            let radius = if command.kind < 2.0 { 45.0 } else { 30.0 };
            let from = w.unit_scalars(sender).position;
            let sender_team = w.unit_scalars(sender).team;
            for ch in 0..self.character_count {
                if ch == source || w.character(ch).human {
                    continue;
                }
                let Some(control) = self.current_controllable(w, ch) else { continue };
                if !(self.relation(sender_team, w.unit_scalars(control).team) > 0.0) {
                    continue;
                }
                let char_hero = w.character(ch).unit.map(|u| w.unit_scalars(u).hero).unwrap_or(false);
                if char_hero || w.unit_scalars(control).hero {
                    continue;
                }
                let seat = if self.options.bind.vehicles {
                    w.vehicles_boarded_vehicle_of_character(ch)
                } else {
                    None
                };
                if command.kind == squads::command::BOARD {
                    if seat.is_none() && w.unit_scalars(control).kind == 3.0 {
                        continue;
                    }
                } else if command.kind == squads::command::FOLLOW && seat.is_some() {
                    continue;
                }
                let reach = if seat.is_some() { radius * 3.0 } else { radius };
                if !(v::distance_sq(from, w.unit_scalars(control).position) <= reach * reach) {
                    continue;
                }
                if context == 0.0 {
                    if let Some(seat) = seat {
                        if w.unit_scalars(seat).kind == 5.0 {
                            continue;
                        }
                    }
                }
                if let Some(ci) = self.controller_of_character(ch) {
                    self.controllers[ci].receive_command(command);
                    delivered.push(ci);
                }
            }
            return delivered;
        }
        if let (Some(boarded), true) = (boarded, self.options.bind.vehicles) {
            if let Some(except) = self.controller_of_character(source) {
                for ci in w.vehicles_crew_controllers(boarded, except) {
                    self.controllers[ci].receive_command(command);
                    delivered.push(ci);
                }
            }
        }
        delivered
    }

    /// playerCommand (squads.js:208).
    pub fn player_command<W:AiWorld+?Sized>(&mut self, w: &mut W, character: usize, kind: f64) -> crate::ai::owned::scratch::List<usize> {
        if character >= self.character_count {
            return w.index_buffer();
        }
        let Some(unit) = w.character(character).unit else { return w.index_buffer() };
        let target = if kind == squads::command::FOLLOW {
            Some(EntRef { unit, generation: w.unit_scalars(unit).generation })
        } else {
            None
        };
        let now = self.now(w);
        let command = controller::SquadCommand { source: Some(character), kind, time: now, target };
        self.distribute_command(w, command)
    }

    // -- the scheduler (scheduler.js) --------------------------------------------

    pub fn update<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, dt: f64) {
        self.decide(w, d, dt);
        self.execute(w, dt);
    }

    pub fn decide<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, dt: f64) {
        if self.options.bind.combat&&w.combat_prepare_ingest(self) {
            for state in w.combat_ingest() {
                let c = &mut self.controllers[state.controller];
                c.shot_pending = state.shot_pending;
                c.shot_known = state.shot_known;
                c.shot_point = state.shot_point;
                c.shot_source = state.shot_source;
                c.danger_pending = state.danger_pending;
                c.danger_point = state.danger_point;
                c.acoustic.clear();c.acoustic.extend_from_slice(&state.acoustic);
                c.dodge = state.dodge;
                c.alert_anim = state.alert_anim;
            }
        }
        self.sync(w, d);
        self.scheduler_update(w, d, dt);
    }

    pub fn execute<W:AiWorld+?Sized>(&mut self, w: &mut W, dt: f64) {
        self.execute_inputs(w, dt);
    }

    fn scheduler_update<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, dt: f64) {
        let (cycle, los_budget) = self.scheduler_numbers();
        self.scheduler.counter += 1.0;
        self.scheduler.parity = if self.scheduler.parity == 0.0 { 1.0 } else { 0.0 };
        if self.scheduler.counter > cycle {
            self.scheduler.counter = 1.0;
            if self.options.bind.vehicles {
                for (ci2, r) in w.vehicles_notify() {
                    self.controllers[ci2].add_vehicle_note(r);
                }
            }
            self.scheduler.skips = 0.0;
            if self.options.bind.combat {
                self.human_order.clear();
                for character_index in 0..self.character_count {
                    let character=w.character(character_index);
                    if character.human {
                        if let Some(unit) = character.unit {
                            if w.unit_scalars(unit).alive { self.human_order.push(unit); }
                        }
                    }
                }
                for i in 0..self.human_order.len() {
                    let unit = self.human_order[i];
                    for (ci2, r) in self.combat_sensors(w, |w| w.combat_broadcast_acoustic(unit)) {
                        self.controllers[ci2].add_acoustic(r);
                    }
                }
            }
            for &ci in &self.live {
                self.controllers[ci].processed = false;
            }
            if self.options.bind.combat {
                self.combat_sensors(w, |w| w.combat_rebuild_protected());
            }
        }
        let budget = scheduler::candidate_budget(self.live.len() as f64, cycle, self.scheduler.counter);
        self.scheduler.stats.budget = budget;
        self.scheduler.stats.considered = 0.0;
        self.scheduler.stats.serviced = 0.0;
        self.scheduler.stats.skipped = 0.0;
        self.decision_order.clear();
        self.decision_order.extend_from_slice(&self.live);
        for i in 0..self.decision_order.len() {
            let ci = self.decision_order[i];
            if self.scheduler.stats.considered >= budget {
                break;
            }
            if self.controllers[ci].processed {
                continue;
            }
            if self.lod_skip(w, ci) {
                self.scheduler.stats.skipped += 1.0;
            } else {
                self.service(w, d, ci, dt);
                self.scheduler.stats.serviced += 1.0;
            }
            self.controllers[ci].processed = true;
            self.scheduler.stats.considered += 1.0;
        }
        self.service_budgets(w, los_budget);
    }

    fn service<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, ci: usize, dt: f64) {
        if self.controller_live(w, ci) {
            self.controller_update(w, d, ci, dt, 0.0);
        }
        if self.options.bind.combat {
            self.combat_retained(w, ci, |w| w.combat_drop_los_requests(ci));
            self.combat_call(w, ci, |w| w.combat_scan(ci));
            if self.controllers[ci].unit.is_some() {
                let unit = self.controllers[ci].unit.unwrap();
                for (ci2, r) in self.combat_sensors(w, |w| w.combat_broadcast_acoustic(unit)) {
                    self.controllers[ci2].add_acoustic(r);
                }
            }
        }
        if self.controller_live(w, ci) {
            self.controller_update(w, d, ci, dt, 1.0);
        }
        if self.options.bind.combat {
            self.combat_retained(w, ci, |w| w.combat_clear_candidate(ci));
        }
    }

    fn controller_live<W:AiWorld+?Sized>(&self, w: &W, ci: usize) -> bool {
        self.controllers[ci].live(w)
    }

    fn service_budgets<W:AiWorld+?Sized>(&mut self, w: &mut W, los_budget: f64) {
        let mut scale = 1.0;
        if self.options.bind.nav {
            if w.nav_planner_idle() {
                scale = 2.0;
            } else {
                w.nav_service(1.0);
            }
        }
        let exhausted = if self.options.bind.combat {
            w.combat_prepare_los(self);
            w.combat_service_los(los_budget * scale)
        } else {
            false
        };
        if !exhausted && self.options.bind.nav {
            w.nav_service(1.0);
        }
    }

    fn lod_skip<W:AiWorld+?Sized>(&mut self, w: &W, ci: usize) -> bool {
        if !self.controllers[ci].lod_skipped {
            if let Some(unit) = self.controllers[ci].unit {
                let id = w.unit_motion(unit).id;
                let parity = ((!(v8math::to_int32(id))) & 1) as f64;
                if parity == self.scheduler.parity && !self.visible_or_near(w, unit) {
                    self.controllers[ci].lod_skipped = true;
                    self.scheduler.skips += 1.0;
                    return true;
                }
            }
        }
        self.controllers[ci].lod_skipped = false;
        false
    }

    fn visible_or_near<W:AiWorld+?Sized>(&self, w: &W, unit: UKey) -> bool {
        let motion=w.unit_motion(unit);
        scheduler::position_visible_or_near(motion.position,motion.velocity,w.cameras(),w.network_views(),self.lod_disabled(w))
    }

    fn execute_inputs<W:AiWorld+?Sized>(&mut self, w: &mut W, dt: f64) {
        self.scheduler.input_parity = if self.scheduler.input_parity == 0.0 { 1.0 } else { 0.0 };
        self.input_order.clear();
        self.input_order.extend_from_slice(&self.live);
        for i in 0..self.input_order.len() {
            let ci = self.input_order[i];
            if self.controller_live(w, ci) {
                self.execute_input(w, ci, dt);
            }
        }
    }

    fn execute_input<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, dt: f64) -> bool {
        let unit = self.controllers[ci].unit;
        let Some(unit) = unit else { return false };
        if !self.controllers[ci].input_skip {
            let id = w.unit_motion(unit).id;
            let parity = ((!(v8math::to_int32(id))) & 1) as f64;
            if parity == self.scheduler.input_parity && !self.visible_or_near(w, unit) {
                self.controllers[ci].input_skip = true;
                self.controllers[ci].input_carry = dt;
                if self.options.bind.combat {
                    self.combat_retained(w, ci, |w| w.combat_mark_skip(ci, true));
                }
                return false;
            }
        }
        let step = self.controllers[ci].input_carry + dt;
        self.controllers[ci].input_skip = false;
        self.controllers[ci].input_carry = 0.0;
        if self.options.bind.combat {
            self.combat_retained(w, ci, |w| w.combat_mark_skip(ci, false));
            let writes=w.combat_prepare_fire(self,ci);
            w.combat_consume_fire(ci,step);
            if writes {self.sync_combat_writes(w,ci);}else{
                let (a,b)=w.combat_fire_requests(ci);
                self.controllers[ci].fire_a=a;self.controllers[ci].fire_b=b;
            }
        }
        if self.controllers[ci].exit_request {
            w.intent_command(unit,IntentCommand::Exit);
            self.controllers[ci].exit_request = false;
        }
        let aim_point = self.scheduler_aim_point(w, ci, step);
        let has_control = self.controllers[ci].nav_control.is_some();
        if self.options.bind.nav && !has_control {
            let obs = w.nav_control_ensure(ci);
            self.adopt_control(ci, obs);
        }
        let has_control = self.controllers[ci].nav_control.is_some();
        if self.options.bind.nav && has_control {
            // Seat controllers go through the adapter's wrapped
            // navigationInput (vehicles.analogs path inside it);
            // the world implements the whole step.
            let _seat = w.unit_is_seat_unit(unit);
            let facing=self.controllers[ci].facing;
            let (obs, effects) = w.nav_navigation_input(self, ci, step, aim_point,facing);
            self.apply_service_effects(w, effects);
            if let Some(obs) = obs {
                self.set_obs(ci, obs);
            }
            self.sync_control(w, ci);
        } else {
            self.apply_posture(w, ci);
            let (pending, vector) = (self.controllers[ci].jet_pending, self.controllers[ci].jet_vector);
            if pending {
                w.intent_command(unit,IntentCommand::Jet(if v::length_sq(vector)>0. {
                    Some([vector.x,vector.y,vector.z])
                }else{None}));
                self.controllers[ci].jet_pending = false;
            }
            let out = if self.options.bind.nav {
                let facing=self.controllers[ci].facing;
                let (out, effects) = w.nav_analogs(self, ci, aim_point, step,facing);
                self.apply_service_effects(w, effects);
                out
            } else {
                let request = self.controllers[ci].request;
                let facing = self.controllers[ci].facing;
                scheduler::fallback_analogs(&w.unit(unit), &request, aim_point, step, facing)
            };
            w.intent_command(unit,IntentCommand::Analogs(out));
        }
        if w.unit_scalars(unit).kind == 3.0 {
            self.apply_droideka_mode(w, ci);
        }
        let dodge = self.controllers[ci].dodge;
        if let Some(dv) = dodge {
            if dv != 0.0 && !dv.is_nan() {
                w.intent_command(unit,IntentCommand::Dodge(dv));
                self.controllers[ci].dodge = Some(0.0);
            }
        }
        true
    }

    fn scheduler_aim_point<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize, dt: f64) -> Option<Vec3> {
        if self.options.bind.combat {
            let writes=w.combat_prepare_aim_query(self,ci);
            let point=w.combat_aim_point(ci,dt);
            if writes {self.sync_combat_writes(w,ci);}
            return point;
        }
        let unit = self.controllers[ci].unit?;
        match self.controllers[ci].aim_state {
            0.0 => {
                let target_ref = self.controllers[ci].aim_target;
                let live = self.resolve(&*w, target_ref);
                if let Some(target) = live {
                    return Some(w.unit_aim_point(target).unwrap_or_else(|| w.unit_scalars(target).position));
                }
                let forward = w.unit_forward(unit).unwrap_or(Vec3::new(0.0, 0.0, 1.0));
                Some(v::add_scaled(w.unit_scalars(unit).position, forward, 30.0))
            }
            1.0 => Some(self.controllers[ci].aim_point),
            _ => None,
        }
    }

    fn apply_droideka_mode<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        let Some(unit) = self.controllers[ci].unit else { return };
        let mode = self.controllers[ci].wanted_mode;
        let s = w.unit(unit).soldier;
        let Some(s) = s else { return };
        if mode == 1.0 && s.mode == Some(12.0) {
            w.intent_command(unit,IntentCommand::Use);
        } else if mode == 2.0 && s.rolled == Some(false) {
            w.intent_command(unit,IntentCommand::Use);
        }
    }

    fn apply_posture<W:AiWorld+?Sized>(&mut self, w: &mut W, ci: usize) {
        let Some(unit) = self.controllers[ci].unit else { return };
        let u = w.unit(unit);
        let Some(s) = &u.soldier else { return };
        if u.kind == 3.0 {
            return;
        }
        let stance = self.controllers[ci].nav_control.map(|c| c.stance).unwrap_or(self.controllers[ci].stance);
        let want = if stance == 1.0 {
            crate::gen::Posture::Crouch
        } else if stance == 2.0 {
            crate::gen::Posture::Prone
        } else {
            crate::gen::Posture::Stand
        };
        let Some(have) = s.posture else { return };
        if have == want {
            return;
        }
        let command=if want==crate::gen::Posture::Crouch {IntentCommand::Crouch}
            else if want==crate::gen::Posture::Prone {IntentCommand::Prone}
            else if have==crate::gen::Posture::Crouch {IntentCommand::Crouch}
            else {IntentCommand::Prone};
        w.intent_command(unit,command);
    }
}

impl Core {
    // -- objectives (goals.js ClassicObjectives, enhanced/objectives.js) -------

    /// Return effects from a foreign service at its call site. Preconditions
    /// fail closed so a returned write cannot conceal a native state error.
    pub fn apply_service_effects<C:world::Characters+?Sized>(&mut self, w:&mut C, effects: Vec<world::CoreEffect>) {
        for effect in effects {
            match effect {
                world::CoreEffect::Publish { character, before, after } => {
                    assert_eq!(w.character(character).objective, before, "service publication input for character {character}");
                    self.objectives_publish(w, character, after);
                }
                world::CoreEffect::Objective { character, before, after } => {
                    assert_eq!(w.character(character).objective, before, "service saved objective input for character {character}");
                    w.character_set_objective(character,after);
                }
                world::CoreEffect::Reboard { controller, before, after } => {
                    assert_eq!(self.controllers[controller].reboard_at.to_bits(), before.to_bits(), "service reboard input for controller {controller}");
                    self.controllers[controller].reboard_at = after;
                }
            }
        }
    }

    fn objectives_publish<C:world::Characters+?Sized>(&mut self, w:&mut C, character: usize, post: Option<usize>) {
        let has_unit = w.character(character).unit.is_some();
        if has_unit {
            if let (Some(ci), Some(post)) = (self.controller_of_character(character), post) {
                self.publish_objective(ci, post);
            }
        }
        w.character_set_objective(character,post);
    }

    pub fn objectives_select<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize, exclude_previous: bool, previous: Option<usize>) -> Option<usize> {
        if self.objectives.enhanced {
            if let Some(post) = self.enhanced_select(w, d, character, exclude_previous, previous) {
                return Some(post);
            }
            // (enhanced_select already falls back when its list is
            // empty, so reaching here means its Classic fallback
            // returned nothing.)
            return None;
        }
        self.classic_select(w, d, character, exclude_previous, previous)
    }

    fn classic_select<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize, exclude_previous: bool, previous: Option<usize>) -> Option<usize> {
        let team = w.character(character).team;
        let faction = self.faction(team);
        let seat = w.crew(character);
        let driver = seat.as_ref().map(|s| s.is_driver).unwrap_or(false);
        let vehicle_mask = if driver {
            match seat.as_ref() {
                Some(s) => w.objective_vehicle_mask(s.vehicle),
                None => 0.0,
            }
        } else {
            0.0
        };
        let teams = &self.teams;
        goals::select_objective_rows(w.posts_len(), &|i| w.objective_post(i), &mut self.objective_scratch,
            team, faction, d, &|a, b| teams.relation(a, b), driver, vehicle_mask, exclude_previous, previous)
    }

    pub fn objectives_request<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize, exclude_previous: bool, previous: Option<usize>) -> Option<usize> {
        let seat = w.crew(character);
        if let Some(seat) = &seat {
            if seat.is_driver {
                let post = self.objectives_select(w, d, character, exclude_previous, previous);
                self.objectives_publish(w, character, post);
                for other in w.crew_occupants(character) {
                    if other != character && !w.character(other).human {
                        self.objectives_publish(w, other, post);
                    }
                }
                return post;
            }
        }
        if let Some(seat) = &seat {
            if let Some(driver) = seat.driver {
                if !w.character(driver).human {
                    let post = w.character(driver).objective;
                    self.objectives_publish(w, character, post);
                    return post;
                }
            }
        }
        let post = self.objectives_select(w, d, character, exclude_previous, previous);
        self.objectives_publish(w, character, post);
        post
    }

    pub fn objectives_republish<C:world::Characters+?Sized>(&mut self, w:&mut C, character: usize) {
        let post = w.character(character).objective;
        self.objectives_publish(w, character, post);
    }

    pub fn objectives_assign<C:world::Characters+?Sized>(&mut self, w:&mut C, character: usize, post: Option<usize>) {
        self.objectives_publish(w, character, post);
    }

    pub fn objectives_on_boarded<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize, previous: Option<UKey>, vehicle: Option<UKey>) {
        if previous != vehicle && !w.character(character).human {
            self.objectives_request(w, d, character, false, None);
        }
    }

    fn objectives_reassign<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, post: usize, team: f64) {
        for ch in 0..self.character_count {
            if w.character(ch).team == team && !w.character(ch).human && w.character(ch).objective == Some(post) {
                self.objectives_request(w, d, ch, false, None);
            }
        }
    }

    fn objectives_recruit<W:AiWorld+?Sized>(&mut self, w: &mut W, post: usize, team: f64) -> f64 {
        let position = w.post(post).position;
        let teams = &self.teams;
        let found = w.units_query(position, 60.0, &mut |u| teams.relation(team, u.team) > 0.0);
        let bans = w.post(post).ai.bans;
        let mut sent = 0.0;
        for unit in found.into_iter().take(10) {
            if sent >= 5.0 {
                break;
            }
            let snap = w.unit(unit).clone();
            if self.unit_controller.get(&unit).is_none() || snap.human {
                continue;
            }
            if snap.kind == 5.0 {
                continue;
            }
            let mask = goals::class_travel_mask(snap.class_def.as_deref());
            if v8math::to_int32(bans) & v8math::to_int32(mask) != 0 {
                continue;
            }
            let Some(character) = snap.character else { continue };
            self.objectives_publish(w, character, Some(post));
            sent += 1.0;
        }
        sent
    }

    pub fn objectives_on_owner_changed<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, post: usize, old_owner: f64, new_owner: f64) {
        if old_owner == new_owner {
            return;
        }
        for team in 1..8 {
            let team = team as f64;
            let before = self.relation(team, old_owner);
            let after = self.relation(team, new_owner);
            if (before < 1.0 && after > 0.0) || (before >= 1.0 && after < 1.0) {
                self.objectives_reassign(w, d, post, team);
                if before > 0.0 && after == 0.0 {
                    self.objectives_recruit(w, post, team);
                }
            }
        }
    }

    pub fn objectives_on_deactivated<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, post: usize) {
        for team in 1..8 {
            self.objectives_reassign(w, d, post, team as f64);
        }
    }

    pub fn objectives_stale<W:AiWorld+?Sized>(&self, w: &W, character: usize) -> bool {
        match w.character(character).objective {
            None => true,
            Some(post) => {
                let snap = w.post(post);
                !snap.active || goals::friendly_spawn(&snap, w.character(character).team, &|a, b| self.relation(a, b))
            }
        }
    }

    pub fn objectives_revalidate<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize) -> bool {
        if !self.objectives_stale(w, character) {
            return false;
        }
        let previous = w.character(character).objective;
        self.objectives_request(w, d, character, true, previous);
        true
    }

    // -- Enhanced objectives -----------------------------------------------------

    fn enhanced_threatened<W:AiWorld+?Sized>(&self, w: &W, post: usize, team: f64) -> bool {
        let snap = w.post(post);
        if !snap.active {
            return false;
        }
        let owner = goals::owner_of(&snap);
        if owner == 0.0 || !(self.relation(team, owner) > 0.0) {
            return false;
        }
        if snap.neutralize > 0.0 {
            return true;
        }
        match self.objectives.threat_at.iter().find(|(p, _)| *p == post) {
            Some((_, at)) => w.time() - at < 8.0,
            None => false,
        }
    }

    fn enhanced_crowding<W:AiWorld+?Sized>(&self, w: &W, post: usize, team: f64, except: usize) -> f64 {
        let mut n = 0.0;
        for i in 0..self.character_count {
            let c=w.character(i);
            if i != except && c.team == team && c.objective == Some(post) {
                if let Some(u) = c.unit {
                    if w.unit_scalars(u).alive {
                        n += 1.0;
                    }
                }
            }
        }
        n
    }

    fn enhanced_select<W:AiWorld+?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize, exclude_previous: bool, previous: Option<usize>) -> Option<usize> {
        let team = w.character(character).team;
        let faction = self.faction(team);
        let seat = w.crew(character);
        let driver = seat.as_ref().map(|s| s.is_driver).unwrap_or(false);
        let mask = if driver {
            seat.as_ref().map(|s| w.objective_vehicle_mask(s.vehicle)).unwrap_or(0.0)
        } else {
            0.0
        };
        let mut list=w.index_buffer();
        let mut weights=w.weight_buffer();
        for i in 0..w.posts_len() {
            let snap = w.post(i).clone();
            if !snap.active {
                continue;
            }
            let defend = self.enhanced_threatened(w, i, team);
            if goals::friendly_spawn(&snap, team, &|a, b| self.relation(a, b)) && !defend {
                continue;
            }
            if driver && (v8math::to_int32(snap.ai.bans) & v8math::to_int32(mask)) != 0 {
                continue;
            }
            if exclude_previous && previous == Some(i) && !defend {
                continue;
            }
            let mut weight = goals::post_weight(&snap, team, faction);
            if defend {
                weight = (if weight > 0.0 { weight } else { 100.0 }) * 2.0;
            }
            if !(weight > 0.0) {
                continue;
            }
            if !defend && goals::owner_of(&snap) == 0.0 {
                weight *= 1.5;
            }
            if snap.building_alive == Some(true) && self.relation(team, goals::owner_of(&snap)) < 0.0 {
                weight *= 2.0;
            }
            let crowd = self.enhanced_crowding(w, i, team, character);
            if crowd > 6.0 {
                weight /= 1.0 + (crowd - 6.0) / 6.0;
            }
            list.push(i);
            weights.push(weight);
        }
        if list.is_empty() {
            return self.classic_select(w, d, character, exclude_previous, previous);
        }
        // distances
        let body_pos: Option<Vec3> = match w.character(character).unit {
            Some(u) if w.unit_scalars(u).alive => Some(w.unit(u).seat_vehicle_pos.unwrap_or_else(|| w.unit_scalars(u).position)),
            _ => None,
        };
        let mut spawns=w.index_buffer();
        if body_pos.is_none() {
            for i in 0..w.posts_len() {
                let snap=w.post(i);
                if snap.active&&goals::friendly_spawn(&snap,team,&|a,b|self.relation(a,b)) {spawns.push(i);}
            }
        }
        let flat = |a: Vec3, b: Vec3| v8math::hypot(a.x - b.x, a.z - b.z);
        let mut dists=w.weight_buffer();
        for &i in &list {
            let pos = w.post(i).position;
            let dist = match body_pos {
                Some(body) => flat(body, pos),
                None => {
                    let mut best = f64::INFINITY;
                    for &s in &spawns {
                        if s != i {
                            best = v8math::min(best, flat(w.post(s).position, pos));
                        }
                    }
                    if best.is_finite() { best } else { 0.0 }
                }
            };
            dists.push(v8math::max(dist, 20.0));
        }
        let nearest = dists.iter().copied().fold(f64::INFINITY, v8math::min);
        let mut total = 0.0;
        for i in 0..list.len() {
            let ratio = nearest / dists[i];
            weights[i] *= ratio * ratio;
            total += weights[i];
        }
        let mut draw = d.a_uniform() * total;
        for i in 0..list.len() {
            draw -= weights[i];
            if draw < 0.0 {
                return Some(list[i]);
            }
        }
        list.last().copied()
    }

    /// EnhancedObjectives.tick (called by the AI wrapper each step).
    pub fn enhanced_objectives_tick<W:AiWorld+?Sized>(&mut self, w: &mut W, _d: &mut dyn Draws) {
        if !self.objectives.enhanced {
            return;
        }
        let now = self.now(w);
        if now < self.objectives.poll_at {
            return;
        }
        self.objectives.poll_at = now + 1.0;
        for post in 0..w.posts_len() {
            let (active, neutral, team) = {
                let snap = w.post(post);
                (snap.active, snap.neutralize, snap.team)
            };
            if !active {
                continue;
            }
            let owner = v8math::to_int32(team) as f64;
            if owner == 0.0 || !(neutral > 0.0) {
                continue;
            }
            if let Some(e) = self.objectives.threat_at.iter_mut().find(|(p, _)| *p == post) {
                e.1 = now;
            } else {
                self.objectives.threat_at.push((post, now));
            }
            for team in 1..8 {
                let team = team as f64;
                if self.relation(team, owner) > 0.0 {
                    self.enhanced_defend(w, post, team, now);
                }
            }
        }
    }

    fn enhanced_defend<W:AiWorld+?Sized>(&mut self, w: &mut W, post: usize, team: f64, now: f64) -> f64 {
        let key=goals::RecruitKey::of(team,w.post_name_id(post));
        let last = self.objectives.recruit_at.iter().find(|(k, _)| *k == key).map(|e| e.1).unwrap_or(f64::NEG_INFINITY);
        if now - last < 10.0 {
            return 0.0;
        }
        let position = w.post(post).position;
        let bans = w.post(post).ai.bans;
        let found_units = w.units_query(position, 80.0, &mut |u| u.team == team && !u.human);
        let mut already = 0.0;
        let mut found=w.recruit_buffer();
        for unit in found_units {
            let snap = w.unit(unit).clone();
            let Some(character) = snap.character else { continue };
            if self.unit_controller.get(&unit).is_none() || snap.seat {
                continue;
            }
            if snap.kind != 0.0 && snap.kind != 3.0 {
                continue;
            }
            if w.character(character).objective == Some(post) {
                already += 1.0;
                continue;
            }
            let ci = self.unit_controller[&unit];
            let state = self.controllers[ci].fsm.state;
            if state != 3.0 && state != 1.0 {
                continue;
            }
            let mask = goals::class_travel_mask(snap.class_def.as_deref());
            if v8math::to_int32(bans) & v8math::to_int32(mask) != 0 {
                continue;
            }
            if let Some(taking) = w.character(character).objective {
                if taking != post {
                    if goals::post_contains(&w.post(taking), snap.position) {
                        continue;
                    }
                    if self.enhanced_threatened(w, taking, team) {
                        continue;
                    }
                }
            }
            found.push((v8math::hypot(snap.position.x - position.x, snap.position.z - position.z), character));
        }
        w.sort_recruits(&mut found);
        let mut sent = 0.0;
        for (_, character) in found {
            if already + sent >= 4.0 {
                break;
            }
            self.objectives_publish(w, character, Some(post));
            sent += 1.0;
        }
        if sent != 0.0 {
            if let Some(e) = self.objectives.recruit_at.iter_mut().find(|(k, _)| *k == key) {
                e.1 = now;
            } else {
                self.objectives.recruit_at.push((key, now));
            }
        }
        sent
    }
}

/// pickStance (nav/hints.js:179): at most 64 draws, then stand.
pub fn pick_stance(mask: f64, d: &mut dyn Draws) -> f64 {
    let mask = v8math::to_int32(mask);
    if mask & 7 == 0 {
        return 0.0;
    }
    for _ in 0..64 {
        let s = d.a_int(0.0, 2.0);
        if (mask >> v8math::to_int32(s)) & 1 != 0 {
            return s;
        }
    }
    0.0
}

/// Replay and the core use the same acceptance functions as native combat.
/// This adapter borrows core state only while no service owns a core borrow.
struct PolicyScene<'a,W:AiWorld+?Sized> {core:&'a mut Core,world:&'a mut W,controller:usize}
impl<W:AiWorld+?Sized> policy::Scene for PolicyScene<'_,W> {
    fn time(&self)->f64 {self.core.now(self.world)}
    fn kind(&self,unit:UKey)->f64 {self.world.unit_scalars(unit).kind}
    fn position(&self,unit:UKey)->Vec3 {self.world.unit_scalars(unit).position}
    fn identity(&self,unit:UKey)->(u64,u64) {let u=self.world.unit(unit);(u.id.to_bits(),u.generation.to_bits())}
    fn post_contains(&self,post:usize,position:Vec3)->bool {goals::post_contains(&self.world.post(post),position)}
    fn hint_forward(&self,hint:usize)->Vec3 {self.world.nav_hint(hint).forward}
    fn can_engage(&mut self,target:UKey)->bool {self.core.can_engage(self.world,self.controller,target)}
    fn board_policy(&mut self,candidate:UKey)->bool {self.core.board_policy(self.world,self.controller,candidate)}
    fn near_fighting(&mut self,unit:UKey)->f64 {
        let near=self.core.query_allies(self.world,unit,7.,30.);let mut fighting=0.;
        for other in near {
            if fighting>2. {break;}
            if self.world.unit_scalars(other).kind!=0. {continue;}
            let Some(ci)=self.core.unit_controller.get(&other).copied() else {continue};
            if ci!=self.controller&&self.core.controllers[ci].fsm.state==2. {fighting+=1.;}
        }fighting
    }
    fn emit_charge(&mut self,unit:UKey) {self.world.emit_vo(unit,"charge","ChargeSound",-1.,None,None,None);}
    fn new_memory(&mut self)->controller::EnhancedMemory {controller::EnhancedMemory::new()}
}

pub mod helpers;
