//! The per-bot controller's data (controller.js): source goals and
//! their queue, weak references, the role of a class, and the
//! controller's fields. The behavior (bind, the per-service update,
//! the state machine's entries) is the core's (mod.rs), where the
//! whole roster is in reach.

use super::fsm::Fsm;
use super::world::{AiWorld, ControlObs, EntRef, HelperObs, Policy, UKey};
use super::super::nav::math::Vec3;
use std::collections::HashMap;

/// Source-goal kinds (controller.js:30 GOAL).
pub mod goal {
    pub const NONE: f64 = 0.0;
    pub const WAIT: f64 = 1.0;
    pub const COMBAT: f64 = 2.0;
    pub const OBJECTIVE: f64 = 3.0;
    pub const WAIT_SECONDARY: f64 = 4.0;
    pub const BOARD: f64 = 5.0;
    pub const UNBOARD: f64 = 6.0;
    pub const DROID_REPAIR: f64 = 7.0;
    pub const RECOVERY: f64 = 8.0;
    pub const FOLLOW: f64 = 9.0;
    pub const HOLD: f64 = 10.0;
}

/// Controller states (controller.js:46 STATE; DONE is fsm::DONE).
pub mod state {
    pub const NONE: f64 = 0.0;
    pub const WAIT: f64 = 1.0;
    pub const COMBAT: f64 = 2.0;
    pub const OBJECTIVE: f64 = 3.0;
    pub const WAIT_SECONDARY: f64 = 4.0;
    pub const BOARD: f64 = 5.0;
    pub const UNBOARD: f64 = 6.0;
    pub const RANDOM: f64 = 9.0;
    pub const FOLLOW: f64 = 10.0;
    pub const HOLD: f64 = 11.0;
}

/// Combat source-goal sub-modes (controller.js:78).
pub mod combat_mode {
    pub const LOCATION: f64 = 0.0;
    pub const TARGET: f64 = 1.0;
    pub const EVADE: f64 = 2.0;
    pub const MODE3: f64 = 3.0;
}

/// UnitType roles (controller.js:88).
pub mod role {
    pub const SCOUT: f64 = 0.0;
    pub const ANTI_ARMOR: f64 = 1.0;
    pub const ASSAULT: f64 = 2.0;
    pub const SUPPORT: f64 = 3.0;
    pub const PILOT: f64 = 4.0;
    pub const TROOPER: f64 = 5.0;
    pub const OTHER: f64 = 6.0;
}

/// The mobile goal-to-state map (controller.js:60); goal 7 has no
/// branch, and an unmapped goal gives 0 (no state requested).
pub fn goal_state(kind: f64) -> f64 {
    if kind == goal::WAIT {
        state::WAIT
    } else if kind == goal::COMBAT {
        state::COMBAT
    } else if kind == goal::OBJECTIVE {
        state::OBJECTIVE
    } else if kind == goal::WAIT_SECONDARY {
        state::WAIT_SECONDARY
    } else if kind == goal::BOARD {
        state::BOARD
    } else if kind == goal::UNBOARD {
        state::UNBOARD
    } else if kind == goal::RECOVERY {
        state::RANDOM
    } else if kind == goal::FOLLOW {
        state::FOLLOW
    } else if kind == goal::HOLD {
        state::HOLD
    } else {
        0.0
    }
}

/// The mounted policies' maps (vehicles/controllers.js), as the
/// requestState translation applies them: a mobile state back to its
/// goal kind, then the policy's goal-to-state.
pub fn mobile_goal_of_state(state: f64) -> Option<f64> {
    if state == 1.0 { Some(1.0) }
    else if state == 2.0 { Some(2.0) }
    else if state == 3.0 { Some(3.0) }
    else if state == 4.0 { Some(4.0) }
    else if state == 5.0 { Some(5.0) }
    else if state == 6.0 { Some(6.0) }
    else if state == 9.0 { Some(8.0) }
    else if state == 10.0 { Some(9.0) }
    else if state == 11.0 { Some(10.0) }
    else { None }
}

pub fn policy_goal_state(policy: Policy, kind: f64) -> f64 {
    match policy {
        Policy::Stationary => {
            if kind == 1.0 || kind == 3.0 { 1.0 }
            else if kind == 2.0 { 2.0 }
            else if kind == 6.0 { 6.0 }
            else { 0.0 }
        }
        Policy::Fly => {
            if kind == 2.0 { 2.0 }
            else if kind == 3.0 { 7.0 }
            else if kind == 4.0 { 4.0 }
            else if kind == 10.0 { 11.0 }
            else { 0.0 }
        }
        _ => goal_state(kind),
    }
}

/// The role of a class (controller.js:104 roleOf): a numeric role
/// wins; else the UnitType string trimmed and lower-cased (ASCII in
/// content, as the nav size names are); anything else is OTHER.
pub fn role_of(role_num: Option<f64>, unit_type: Option<&str>) -> f64 {
    if let Some(r) = role_num {
        if r.is_finite() && v8math::trunc(r) == r {
            return r;
        }
    }
    let Some(t) = unit_type else { return role::OTHER };
    match t.trim().to_lowercase().as_str() {
        "scout" => role::SCOUT,
        "anti-armor" => role::ANTI_ARMOR,
        "assault" => role::ASSAULT,
        "support" | "repair" | "special" => role::SUPPORT,
        "pilot" => role::PILOT,
        "trooper" => role::TROOPER,
        _ => role::OTHER,
    }
}

/// A source goal (controller.js:117 SourceGoal).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceGoal {
    pub kind: f64,
    pub ref_a: Option<EntRef>,
    pub ref_b: Option<EntRef>,
    pub post: Option<usize>,
    pub point: Option<Vec3>,
    pub mode: f64,
}

impl SourceGoal {
    pub fn new(kind: f64) -> Self {
        SourceGoal { kind, ref_a: None, ref_b: None, post: None, point: None, mode: 0.0 }
    }
    pub fn objective(post: Option<usize>) -> Self {
        let mut g = SourceGoal::new(goal::OBJECTIVE);
        g.post = post;
        g
    }
    pub fn combat(mode: f64, point: Option<Vec3>, target: Option<EntRef>) -> Self {
        let mut g = SourceGoal::new(goal::COMBAT);
        g.mode = mode;
        g.point = point;
        g.ref_a = target;
        g
    }
    pub fn follow(leader: Option<EntRef>, source: Option<EntRef>) -> Self {
        let mut g = SourceGoal::new(goal::FOLLOW);
        g.ref_a = leader;
        g.ref_b = source;
        g
    }
    pub fn board(vehicle: Option<EntRef>) -> Self {
        let mut g = SourceGoal::new(goal::BOARD);
        g.ref_a = vehicle;
        g
    }
    pub fn droid_repair(target: Option<EntRef>, owner: Option<EntRef>) -> Self {
        let mut g = SourceGoal::new(goal::DROID_REPAIR);
        g.ref_a = target;
        g.ref_b = owner;
        g
    }
}

pub const COMMAND_CAPACITY: usize = 5;

/// The CommandFsm's storage (controller.js:187 CommandQueue).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandQueue {
    pub current: SourceGoal,
    pub queue: Vec<SourceGoal>,
    pub latch: bool,
    pub current_done: bool,
}

impl Default for CommandQueue {
    fn default() -> Self {
        CommandQueue::new()
    }
}

impl CommandQueue {
    pub fn new() -> Self {
        CommandQueue { current: SourceGoal::new(goal::NONE), queue: Vec::new(), latch: false, current_done: false }
    }
    pub fn count(&self) -> usize {
        self.queue.len()
    }
    pub fn needs_next_goal(&self) -> bool {
        self.latch || (self.current_done && !self.queue.is_empty())
    }
    pub fn overwrite(&mut self, goal: SourceGoal) {
        self.latch = true;
        self.queue.clear();
        self.queue.push(goal);
    }
    pub fn append(&mut self, goal: SourceGoal) -> bool {
        if self.queue.len() >= COMMAND_CAPACITY {
            return false;
        }
        self.queue.push(goal);
        true
    }
    pub fn prepend_one(&mut self, goal: SourceGoal) -> bool {
        if self.latch {
            return false;
        }
        self.queue.insert(0, goal);
        self.queue.truncate(COMMAND_CAPACITY);
        self.latch = true;
        true
    }
    pub fn prepend_two(&mut self, saved: SourceGoal, goal: SourceGoal) -> bool {
        if self.latch {
            return false;
        }
        self.queue.insert(0, saved);
        self.queue.insert(0, goal);
        self.queue.truncate(COMMAND_CAPACITY);
        self.latch = true;
        true
    }
    pub fn pop_front(&mut self) -> SourceGoal {
        if !self.queue.is_empty() {
            self.current = self.queue.remove(0);
        }
        self.current_done = false;
        self.current
    }
    /// controller.js:251: a record of kind 0 ends the scan and drops
    /// everything after it.
    pub fn remove_kind(&mut self, kind: f64) {
        let mut kept = Vec::new();
        for goal in self.queue.drain(..) {
            if goal.kind == goal::NONE {
                break;
            }
            if goal.kind != kind {
                kept.push(goal);
            }
        }
        self.queue = kept;
    }
    pub fn current_copy(&self) -> SourceGoal {
        let kind = self.current.kind;
        if (0.0..=10.0).contains(&kind) { self.current } else { SourceGoal::new(goal::NONE) }
    }
}

/// refOf (controller.js:343) over a snapshot: None without a unit.
pub fn ref_of(w: &dyn AiWorld, unit: Option<UKey>) -> Option<EntRef> {
    unit.map(|key| EntRef { unit: key, generation: w.unit(key).generation })
}

/// resolveRef (controller.js:347).
pub fn resolve_ref(w: &dyn AiWorld, r: Option<EntRef>) -> Option<UKey> {
    let r = r?;
    let u = w.unit(r.unit);
    if u.generation != r.generation || !u.alive {
        return None;
    }
    Some(r.unit)
}

/// sameRef (controller.js:353).
pub fn same_ref(w: &dyn AiWorld, a: Option<EntRef>, b: Option<EntRef>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.unit == b.unit && a.generation == b.generation,
        _ => a == b || (resolve_ref(w, a).is_none() && resolve_ref(w, b).is_none()),
    }
}

/// The movement request (controller.js:381 / nav MovementRequest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveRequest {
    pub valid: bool,
    pub steering_target: Vec3,
    pub waypoint: Vec3,
    pub segment_direction: Vec3,
    pub mode: f64,
    pub lower: f64,
    pub upper: f64,
}

impl Default for MoveRequest {
    fn default() -> Self {
        MoveRequest { valid: false, steering_target: Vec3::default(), waypoint: Vec3::default(), segment_direction: Vec3::default(), mode: 0.0, lower: -1.0, upper: 1.0 }
    }
}

impl MoveRequest {
    pub fn write(&mut self, target: Vec3, waypoint: Vec3, direction: Vec3, mode: f64, lower: f64, upper: f64) {
        self.valid = true;
        self.steering_target = target;
        self.waypoint = waypoint;
        self.segment_direction = direction;
        self.mode = mode;
        self.lower = lower;
        self.upper = upper;
    }
    pub fn clear(&mut self) {
        self.valid = false;
    }
}

/// A squad command record as controllers keep it (squads.js:20;
/// receiveCommand copies it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SquadCommand {
    pub source: Option<usize>,
    pub kind: f64,
    pub time: f64,
    pub target: Option<EntRef>,
}

/// What an Enhanced controller remembers across its agents
/// (enhanced/agents.js memoryOf). The heard map's iteration order is
/// insertion order (its cleanup walks it); keys are "id:generation"
/// as numbers here (unit id bits, generation).
#[derive(Clone, Debug, PartialEq)]
pub struct EnhancedMemory {
    pub look_until: f64,
    pub look_point: Vec3,
    pub heard: Vec<((u64, u64), f64)>,
    heard_at: HashMap<(u64, u64), usize>,
}

impl EnhancedMemory {
    pub fn new() -> Self {
        EnhancedMemory { look_until: f64::NEG_INFINITY, look_point: Vec3::default(), heard: Vec::new(), heard_at: HashMap::new() }
    }
    pub fn heard_get(&self, key: (u64, u64)) -> Option<f64> {
        self.heard_at.get(&key).map(|&i| self.heard[i].1)
    }
    pub fn heard_set(&mut self, key: (u64, u64), at: f64) {
        if let Some(&i) = self.heard_at.get(&key) {
            self.heard[i].1 = at;
        } else {
            self.heard_at.insert(key, self.heard.len());
            self.heard.push((key, at));
        }
    }
    /// The cleanup when the map passes 32 (enhanced/agents.js): delete
    /// the entries at least `recheck` old, in insertion order.
    pub fn heard_prune(&mut self, now: f64, recheck: f64) {
        if self.heard.len() <= 32 {
            return;
        }
        self.heard.retain(|(_, at)| now - at < recheck);
        self.heard_at = self.heard.iter().enumerate().map(|(i, (k, _))| (*k, i)).collect();
    }
}

/// One bot's controller (controller.js UnitController). Its agent and
/// the navigation control's observable state ride along; its Fsm is
/// driven by the core.
pub struct Controller {
    pub fsm: Fsm,
    pub character: usize,
    pub unit: Option<UKey>,
    pub generation: f64,
    pub agent: Option<super::agents::Agent>,
    pub commands: CommandQueue,
    pub phase: f64,
    pub published: bool,
    pub request: MoveRequest,
    pub aim_state: f64,
    pub aim_point: Vec3,
    pub aim_target: Option<EntRef>,
    pub target: Option<EntRef>,
    pub fire_a: bool,
    pub fire_b: bool,
    pub facing: bool,
    pub facing_aux: bool,
    pub stance: f64,
    pub range_scale: f64,
    pub jet_pending: bool,
    pub jet_vector: Vec3,
    pub wanted_mode: f64,
    pub exit_request: bool,
    pub leader: Option<EntRef>,
    pub danger_pending: bool,
    pub danger_point: Vec3,
    pub shot_pending: bool,
    pub shot_known: bool,
    pub shot_point: Vec3,
    pub shot_source: Option<usize>,
    pub acoustic: Vec<EntRef>,
    pub vehicle_notes: Vec<EntRef>,
    pub command_pending: bool,
    pub command_record: Option<SquadCommand>,
    pub last_command: Option<SquadCommand>,
    pub secondary_wait_until: f64,
    pub reboard_at: f64,
    pub roll: f64,
    pub home_facing: bool,
    pub home_direction: Vec3,
    pub processed: bool,
    pub lod_skipped: bool,
    pub input_skip: bool,
    pub input_carry: f64,
    pub nav_control: Option<ControlObs>,
    pub helper_obs: HelperObs,
    pub difficulty: Option<super::difficulty::Curves>,
    pub dodge: Option<f64>,
    pub alert_anim: Option<bool>,
    pub enhanced: Option<EnhancedMemory>,
    pub policy: Policy,
}

pub const ALERT_ACOUSTIC_CAP: usize = 20;
pub const VEHICLE_NOTE_CAP: usize = 10;

impl Controller {
    pub fn new(character: usize) -> Self {
        Controller {
            fsm: Fsm::new(),
            character,
            unit: None,
            generation: -1.0,
            agent: None,
            commands: CommandQueue::new(),
            phase: 0.0,
            published: false,
            request: MoveRequest::default(),
            aim_state: 2.0,
            aim_point: Vec3::default(),
            aim_target: None,
            target: None,
            fire_a: false,
            fire_b: false,
            facing: false,
            facing_aux: false,
            stance: 0.0,
            range_scale: 1.0,
            jet_pending: false,
            jet_vector: Vec3::default(),
            wanted_mode: 0.0,
            exit_request: false,
            leader: None,
            danger_pending: false,
            danger_point: Vec3::default(),
            shot_pending: false,
            shot_known: false,
            shot_point: Vec3::default(),
            shot_source: None,
            acoustic: Vec::new(),
            vehicle_notes: Vec::new(),
            command_pending: false,
            command_record: None,
            last_command: None,
            secondary_wait_until: 0.0,
            reboard_at: f64::NEG_INFINITY,
            roll: 0.0,
            home_facing: false,
            home_direction: Vec3::new(0.0, 0.0, 1.0),
            processed: false,
            lod_skipped: false,
            input_skip: false,
            input_carry: 0.0,
            nav_control: None,
            helper_obs: HelperObs::default(),
            difficulty: None,
            dodge: None,
            alert_anim: None,
            enhanced: None,
            policy: Policy::None,
        }
    }

    pub fn live(&self, w: &dyn AiWorld) -> bool {
        match self.unit {
            Some(key) => {
                let u = w.unit(key);
                u.alive && u.generation == self.generation
            }
            None => false,
        }
    }

    /// What phase 0 clears (controller.js:602).
    pub fn clear_requests(&mut self) {
        self.jet_pending = false;
        self.jet_vector = Vec3::default();
        // The navigation control's jet is a different slot; the
        // JavaScript's clearRequests clears only the controller's own
        // (its `jet` field), and requestJet prefers the control's.
        self.fire_a = false;
        self.fire_b = false;
        self.target = None;
        self.facing = false;
        self.facing_aux = false;
        self.range_scale = 1.0;
    }

    pub fn look_at(&mut self, point: Vec3) {
        self.aim_state = 1.0;
        self.aim_point = point;
    }

    pub fn aim_at(&mut self, r: Option<EntRef>) {
        self.aim_state = 0.0;
        self.aim_target = r;
    }

    pub fn look_along_path(&mut self) {
        self.aim_state = 2.0;
    }

    pub fn set_stance(&mut self, stance: f64) {
        self.stance = stance;
        if let Some(control) = &mut self.nav_control {
            control.stance = stance;
        }
    }

    pub fn request_facing(&mut self, w: &dyn AiWorld) {
        let kind = self.unit.map(|k| w.unit(k).kind);
        if kind == Some(0.0) || kind == Some(3.0) {
            self.facing = true;
        }
    }

    pub fn request_jet(&mut self, vector: Vec3) {
        if let Some(control) = &mut self.nav_control {
            control.jet_vector = vector;
            control.jet_pending = true;
        } else {
            self.jet_vector = vector;
            self.jet_pending = true;
        }
    }

    pub fn executor_status(&self) -> f64 {
        if let Some(control) = &self.nav_control {
            return control.status;
        }
        match &self.agent {
            Some(agent) => agent.helper_status(),
            None => 0.0,
        }
    }

    pub fn has_executor(&self) -> bool {
        if let Some(control) = &self.nav_control {
            return control.has_executor;
        }
        match &self.agent {
            Some(agent) => agent.helper_has_path(),
            None => false,
        }
    }

    pub fn avoid_side(&self) -> f64 {
        self.nav_control.map(|c| c.avoid_side).unwrap_or(0.0)
    }

    pub fn notify_danger(&mut self, point: Vec3) {
        self.danger_pending = true;
        self.danger_point = point;
    }

    pub fn notify_shot(&mut self, source: Option<usize>, known: bool, point: Option<Vec3>) {
        let Some(source) = source else { return };
        self.shot_pending = true;
        self.shot_source = Some(source);
        if known {
            if let Some(point) = point {
                self.shot_known = true;
                self.shot_point = point;
            }
        }
    }

    pub fn add_acoustic(&mut self, r: EntRef) {
        if self.acoustic.len() < ALERT_ACOUSTIC_CAP {
            self.acoustic.push(r);
        }
    }

    pub fn add_vehicle_note(&mut self, r: EntRef) {
        if self.vehicle_notes.len() < VEHICLE_NOTE_CAP {
            self.vehicle_notes.push(r);
        }
    }

    pub fn receive_command(&mut self, record: SquadCommand) {
        self.command_pending = true;
        self.command_record = Some(record);
    }
}
