//! Host state owned by [`super::ScriptRules`]: prepared script chain,
//! load-time declarations, timer registry, listener registry, generation/life-
//! aware character and object handle tables, AI goals, recorded notifications,
//! and bounded diagnostics.
//!
//! Uses only `Vec`, `VecDeque`, and `BTreeMap` (never `HashMap` / `HashSet`) so
//! iteration order is identical on every platform and run.

use bfe_wide::{Fnv, Wide};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use crate::ai::goals::{GoalArg, GoalHandles, GoalKind, GoalRow, GoalTable, GOAL_SLOTS};
use crate::battle::rules::events::{ObjectHandle, Reinforcements};
use crate::battle::spawning::TEAM_COUNT;
use crate::gen::{Character, SpawnPost};
use crate::store::{Ref, StrId};

use super::adapter::{
    ascii_bytes_eq_ignore_case, InstructionSite, ScriptClosureId, ScriptError, ScriptLimits, ScriptValue,
    ScriptValueRef, ScriptVm, MAX_EXACT_LUA_HANDLE,
};

/// Userdata tag used for world-object handles passed into Lua callbacks.
pub const OBJECT_USERDATA_TAG: u32 = 0x4F42_4A31;

/// Maximum retained diagnostics to prevent unbounded memory growth.
pub const MAX_DIAGNOSTICS: usize = 64;
pub(super) const MAX_DIAGNOSTIC_BYTES: usize = 512;
/// Maximum retained notifications in the sequenced ring buffer.
pub const MAX_NOTIFICATIONS: usize = 256;
/// Maximum simultaneous script timers.
pub const MAX_TIMERS: usize = 64;
/// Maximum distinct unbound host function names recorded.
pub const MAX_UNBOUND_NAMES: usize = 128;
/// Standalone host capacity for physical events before a battle supplies its producer bound.
pub const MAX_PENDING_EVENTS: usize = 1024;
/// Maximum simultaneously registered script listeners.
pub const MAX_LISTENERS: usize = 512;
/// Maximum simultaneous AI goals retained by the script host.
pub const MAX_AI_GOALS: usize = GOAL_SLOTS;
/// Maximum mission objective rows retained by the script host.
pub const MAX_OBJECTIVES: usize = 128;
/// Maximum critical systems registered for Space Assault.
pub const MAX_CRITICAL_SYSTEMS: usize = 32;
/// Maximum camera shots recorded during load.
pub const MAX_CAMERA_SHOTS: usize = 64;
/// Maximum audio streams recorded during load.
pub const MAX_AUDIO_STREAMS: usize = 64;

/// Default point target shared by Hero Assault and Space Assault.
pub const DEFAULT_ASSAULT_SCORE_LIMIT: u32 = 180;
pub use DEFAULT_ASSAULT_SCORE_LIMIT as PROVISIONAL_ASSAULT_SCORE_LIMIT;
/// Provisional Instant Action fallback for Hunt score limits when unconfigured.
pub const PROVISIONAL_HUNT_SCORE_LIMIT: u32 = 50;
/// Provisional Instant Action fallback for XL/Uber score limits when unconfigured.
pub const PROVISIONAL_UBER_SCORE_LIMIT: u32 = 400;
/// Provisional Instant Action fallback for Assault bot counts when unconfigured.
pub const PROVISIONAL_ASSAULT_NUM_BOTS: u32 = 16;

/// One compiled Lua 5.0 mission script chunk (`scr_`/`BODY` payload).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptChunk {
    /// Script name as stored in the archive or prepared manifest.
    pub name: String,
    /// Raw compiled Lua 5.0 bytecode bytes.
    pub bytes: Rc<[u8]>,
}

/// Prepared mission script chain passed from content loading into admission.
///
/// Backed by shared `Rc<[ScriptChunk]>` storage so replay and `ScriptCB_DoFile`
/// do not clone chunk byte vectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptChain {
    /// Entry script name (matched ASCII case-insensitively against `chunks`).
    pub entry: String,
    /// Shared slice of compiled chunks available to the entry script and `ScriptCB_DoFile`.
    pub chunks: Rc<[ScriptChunk]>,
}

impl ScriptChain {
    /// Construct a chain from an entry name and its compiled script chunks.
    pub fn new(entry: impl Into<String>, chunks: Vec<ScriptChunk>) -> Self {
        Self {
            entry: entry.into(),
            chunks: Rc::from(chunks),
        }
    }

}

/// Execution phase of the mission script host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptPhase {
    Chunk,
    PreInit,
    Init,
    PostLoad,
    Tick,
}

/// Lifecycle state of the mission script host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    Cold,
    Loaded,
    Active,
    Faulted,
}

/// Configuration for a [`super::ScriptRules`] battle instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptRulesConfig {
    pub network: bool,
    /// Battle seed used to seed the deterministic script RNG stream.
    pub seed: u32,
    /// Primary team 1 index (default `1`).
    pub own_team: i32,
    /// Primary team 2 index (default `2`).
    pub foe_team: i32,
    /// When `true`, calling an unbound global host function returns a Lua
    /// runtime error (test/dev strictness). When `false` (release default),
    /// unbound calls are recorded in diagnostics and degrade to a no-op.
    pub strict_unbound: bool,
    /// Explicit score target returned by `ScriptCB_GetAssaultScoreLimit()`.
    /// When `None`, uses `RuleActions::score_limit()`, then [`DEFAULT_ASSAULT_SCORE_LIMIT`].
    pub assault_score_limit: Option<u32>,
    /// Explicit score target returned by `ScriptCB_GetHuntScoreLimit()`.
    pub hunt_score_limit: Option<u32>,
    /// Explicit time limit in seconds returned by `ScriptCB_GetHuntMaxTimeLimit()`.
    pub hunt_max_time_limit: Option<u32>,
    /// Explicit score target returned by `ScriptCB_GetUberScoreLimit()`.
    pub uber_score_limit: Option<u32>,
    /// Explicit bot count returned by `ScriptCB_GetASSNumBots()`.
    pub assault_num_bots: Option<u32>,
    /// Number of local split-screen cameras returned by `ScriptCB_GetNumCameras()` (PC default `1`).
    pub num_cameras: u32,
    /// Deterministic simulated duration (seconds) of a voice-over audio cue
    /// before `AudioStreamComplete` returns `1` (default `0.0` = immediate).
    pub voice_over_duration_ms: u32,
    /// Returned by `IsCampaign()`.
    pub is_campaign: bool,
    /// Returned by `GetWorldFilename()`.
    pub world_filename: String,
    /// Work shared by the entry, includes, pre-init, init and post-load.
    pub load_instruction_budget: u32,
    /// Work shared by every callback in one authority tick.
    pub tick_instruction_budget: u32,
    /// Shared native host-call budget across a single tick.
    pub tick_native_ops_budget: u32,
    /// Maximum number of Lua callback invocations executed in a single tick.
    pub max_callbacks_per_tick: usize,
    /// Maximum Lua/native call depth.
    pub max_call_depth: usize,
    /// Maximum operand stack slots.
    pub max_stack_slots: usize,
    /// Maximum tracked VM GC heap size in bytes.
    pub memory_limit: usize,
}

impl Default for ScriptRulesConfig {
    fn default() -> Self {
        Self {
            network: false,
            seed: 1,
            own_team: 1,
            foe_team: 2,
            strict_unbound: false,
            assault_score_limit: None,
            hunt_score_limit: None,
            hunt_max_time_limit: None,
            uber_score_limit: None,
            assault_num_bots: None,
            num_cameras: 1,
            voice_over_duration_ms: 0,
            is_campaign: false,
            world_filename: String::new(),
            load_instruction_budget: ScriptLimits::LOAD_BUDGET,
            tick_instruction_budget: ScriptLimits::TICK_BUDGET,
            tick_native_ops_budget: 2_048,
            max_callbacks_per_tick: 256,
            max_call_depth: 200,
            max_stack_slots: 16_384,
            memory_limit: 16 * 1024 * 1024,
        }
    }
}

impl ScriptRulesConfig {
    /// Strict test/developer configuration that fails loudly on any unbound host call.
    pub fn strict(seed: u32) -> Self {
        Self {
            seed,
            strict_unbound: true,
            ..Self::default()
        }
    }

    pub fn effective_assault_score_limit(&self, admitted_limit: Option<u32>) -> u32 {
        self.assault_score_limit
            .or(admitted_limit)
            .unwrap_or(DEFAULT_ASSAULT_SCORE_LIMIT)
    }

    pub fn effective_hunt_score_limit(&self) -> u32 {
        self.hunt_score_limit
            .unwrap_or(PROVISIONAL_HUNT_SCORE_LIMIT)
    }

    pub fn effective_hunt_max_time_limit(&self) -> u32 {
        self.hunt_max_time_limit.unwrap_or(0)
    }

    pub fn effective_uber_score_limit(&self) -> u32 {
        self.uber_score_limit
            .unwrap_or(PROVISIONAL_UBER_SCORE_LIMIT)
    }

    pub fn effective_assault_num_bots(&self) -> u32 {
        self.assault_num_bots
            .unwrap_or(PROVISIONAL_ASSAULT_NUM_BOTS)
    }

    pub(crate) fn load_limits(&self) -> ScriptLimits {
        ScriptLimits {
            instruction_budget: self.load_instruction_budget,
            max_call_depth: self.max_call_depth,
            max_stack_slots: self.max_stack_slots,
            memory_limit: self.memory_limit,
        }
    }

}

/// One unit class declaration recorded from `AddUnitClass`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitClassSetup {
    pub class_name: String,
    pub min_units: i32,
    pub max_units: Option<i32>,
}

/// Declarative per-team setup collected during load/init phases.
///
/// Authoritative live scores and reinforcements live in the simulation and are
/// read/written through `RuleActions` once active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamSetup {
    pub team: i32,
    pub name: Option<String>,
    pub icons: Vec<String>,
    pub unit_count: Option<u32>,
    /// Initial reinforcement count requested during load/init.
    pub initial_reinforcements: Option<Reinforcements>,
    /// Initial team points requested during load/init.
    pub initial_score: Option<u32>,
    pub classes: Vec<UnitClassSetup>,
}

impl TeamSetup {
    pub fn new(team: i32) -> Self {
        Self {
            team,
            name: None,
            icons: Vec::new(),
            unit_count: None,
            initial_reinforcements: None,
            initial_score: None,
            classes: Vec::new(),
        }
    }
}

/// Recorded `ReadDataFile(path, ...)` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataFileRequest {
    pub path: String,
    pub args: Vec<String>,
}

/// Recorded `OpenAudioStream(lvl, stream)` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioStreamRecord {
    pub handle: u32,
    pub lvl_path: String,
    pub stream_id: String,
    pub appended: Vec<(String, String)>,
}

/// Recorded audio configuration calls made during mission setup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioConfigSetup {
    pub ambient_music: Vec<(i32, f64, String, i32, i32)>,
    pub victory_music: Vec<(i32, String)>,
    pub defeat_music: Vec<(i32, String)>,
    pub sound_effects: Vec<(String, String, Option<i32>)>,
    pub bleeding_vo: Vec<(i32, i32, String, i32)>,
    pub low_reinforcements_vo: Vec<(i32, i32, String, f64, i32)>,
    pub out_of_bounds_vo: Vec<(i32, String)>,
    pub doppler_factor: Option<f64>,
    pub scaled_parameters: Vec<(String, String, f64)>,
}

/// Load-time mission declarations gathered during chunk, `ScriptPreInit`, and `ScriptInit`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadSetup {
    pub loaded_scripts: Vec<String>,
    pub data_files: Vec<DataFileRequest>,
    pub teams: BTreeMap<i32, TeamSetup>,
    pub world_extents: Option<f64>,
    pub max_fly_height: Option<f64>,
    pub max_player_fly_height: Option<f64>,
    pub min_fly_height: Option<f64>,
    pub min_player_fly_height: Option<f64>,
    pub dense_environment: bool,
    pub ai_vehicle_notify_radius: Option<f64>,
    pub spawn_wave_delay: Option<f64>,
    pub spawn_slot_delay: Option<f64>,
    pub attacking_team: Option<i32>,
    pub landing_regions: Vec<String>,
    pub death_regions: Vec<String>,
    pub disabled_barriers: Vec<String>,
    pub blocked_arcs: Vec<String>,
    pub camera_shots: Vec<[f64; 7]>,
    pub audio_streams: Vec<AudioStreamRecord>,
    pub audio_config: AudioConfigSetup,
    pub hero_classes: Vec<String>,
}

/// Mode-level flags configured by the mission script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchFlags {
    pub sp_hero_rules: bool,
    pub num_bots: Option<u32>,
    pub flag_gameplay_type: String,
    pub hero_music_enabled: bool,
    pub hero_vo_enabled: bool,
    pub hud_score_limit_mode: Option<i32>,
    pub space_assault_enabled: bool,
}

impl Default for MatchFlags {
    fn default() -> Self {
        Self {
            sp_hero_rules: false,
            num_bots: None,
            flag_gameplay_type: "none".to_string(),
            hero_music_enabled: true,
            hero_vo_enabled: true,
            hud_score_limit_mode: None,
            space_assault_enabled: false,
        }
    }
}

/// Typed target argument passed to `AddAIGoal`.
#[derive(Clone, Debug, PartialEq)]
pub enum AiGoalTarget {
    /// Named region, flag, or mission instance (exact raw Lua bytes).
    Name(Vec<u8>),
    /// Character handle index.
    Character(Ref<Character>),
    /// Resolved authoritative world object handle.
    Object(ObjectHandle),
}

/// Weighted AI goal issued by `AddAIGoal` for the AI goal layer.
#[derive(Clone, Debug, PartialEq)]
pub struct AiGoalRecord {
    pub slot: u8,
    pub id: u32,
    pub team: u8,
    pub kind: GoalKind,
    pub weight: f32,
    pub target1: Option<AiGoalTarget>,
    pub target2: Option<AiGoalTarget>,
}

impl AiGoalTarget {
    pub(super) fn view(target: Option<&Self>) -> GoalArg<'_> {
        match target {
            None => GoalArg::None,
            Some(Self::Name(bytes)) => GoalArg::Name(bytes),
            Some(Self::Character(character)) => GoalArg::Character(*character),
            Some(Self::Object(object @ ObjectHandle::Prop(_))) => GoalArg::Placement(object),
            Some(Self::Object(object)) => GoalArg::Object(object),
        }
    }
}

impl GoalTable for HostState {
    fn next_handle(&self) -> u32 { self.goal_handles.next() }
    fn handles(&self) -> Option<&GoalHandles> { Some(&self.goal_handles) }
    fn revision(&self) -> u32 { self.goal_revision }
    fn len(&self) -> usize { self.ai_goals.len() }
    fn row(&self, index: usize) -> GoalRow<'_> {
        let goal = &self.ai_goals[index];
        GoalRow {
            slot: goal.slot,
            handle: goal.id,
            team: goal.team,
            kind: goal.kind,
            weight: goal.weight,
            target: AiGoalTarget::view(goal.target1.as_ref()),
            extra: AiGoalTarget::view(goal.target2.as_ref()),
        }
    }
}

/// Countdown timer registered by Lua (`CreateTimer` .. `DestroyTimer`) and backed
/// by `RuleActions::start_timer` / `stop_timer` for authoritative expiry.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptTimer {
    pub id: u32,
    pub name_bytes: Vec<u8>,
    pub remaining: f64,
    pub rate: f64,
    pub running: bool,
    pub alive: bool,
    /// Monotonic generation incremented on every start/stop/reset/rate/destroy
    /// so stale queued `TimerElapsed` events are ignored cleanly.
    pub arm_gen: u32,
    /// Tick on which `StartTimer` was invoked so a newly started timer does not
    /// decrement on the same tick.
    pub started_tick: u32,
}

impl ScriptTimer {
    #[inline]
    pub fn rules_id(&self) -> StrId {
        StrId(self.id)
    }

    pub fn name_lossy(&self) -> String {
        String::from_utf8_lossy(&self.name_bytes).into_owned()
    }
}

/// Generation- and life-aware identity key for an authoritative [`ObjectHandle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObjectIdentityKey {
    Door(u32),
    Prop {
        kind: u8,
        slot: u32,
        generation: u32,
        life_bits: Wide,
    },
    Post {
        is_mobile: bool,
        slot: u32,
        generation: u32,
    },
    Vehicle {
        slot: u32,
        generation: u32,
        life_bits: Wide,
    },
}

impl ObjectIdentityKey {
    pub fn of(handle: &ObjectHandle) -> Self {
        match handle {
            ObjectHandle::Door(i) => Self::Door(*i),
            ObjectHandle::Prop(h) => Self::Prop {
                kind: h.kind as u8,
                slot: h.slot,
                generation: h.generation,
                life_bits: Wide::from_f64_bits(h.life),
            },
            ObjectHandle::Post(SpawnPost::Post(r)) => Self::Post {
                is_mobile: false,
                slot: r.slot,
                generation: r.gen,
            },
            ObjectHandle::Post(SpawnPost::MobilePost(r)) => Self::Post {
                is_mobile: true,
                slot: r.slot,
                generation: r.gen,
            },
            ObjectHandle::Vehicle { vehicle, life } => Self::Vehicle {
                slot: vehicle.slot,
                generation: vehicle.gen,
                life_bits: Wide::from_f64_bits(*life),
            },
        }
    }
}

/// Space Assault critical-system registration from `SpaceAssaultAddCriticalSystem`.
#[derive(Clone, Debug, PartialEq)]
pub struct CriticalSystemRecord {
    pub object: ObjectHandle,
    pub name: String,
    pub points: i32,
    pub hud_u: f32,
    pub hud_v: f32,
    pub show_marker: bool,
}

/// HUD objective entry registered by `AddMissionObjective` / `ActivateObjective` / `CompleteObjective`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveEntry {
    pub team: i32,
    pub text_key: String,
    pub popup_key: Option<String>,
    pub active: bool,
    pub completed: bool,
}

/// Recorded replicable presentation/HUD/audio notification with no simulation effect yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptNotification {
    AddMissionObjective {
        team: i32,
        text_key: String,
        popup_key: Option<String>,
    },
    ActivateObjective {
        text_key: String,
    },
    CompleteObjective {
        text_key: String,
    },
    ShowTeamPoints {
        team: i32,
        enabled: bool,
    },
    ShowHuntScoreLimit {
        mode: i32,
    },
    ShowMessageText {
        key: Rc<str>,
        team: Option<i32>,
    },
    BroadcastVoiceOver {
        sound_id: Rc<str>,
        team: Option<i32>,
    },
    MapHideCommandPosts {
        hide: bool,
    },
    DisableSmallMapMiniMap,
    ShowTimer {
        timer_name: Option<String>,
    },
    AddMissionHint {
        key: String,
    },
}

/// Sequenced notification entry in the bounded ring buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedNotification {
    pub seq: u32,
    pub tick: u32,
    pub notification: ScriptNotification,
}

/// Authoritative battle outcome accepted by `RuleActions::finish`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleOutcome {
    Victory(i32),
    Defeat(i32),
    Draw(Vec<i32>),
    /// Authority-visible draw forced when the mission script faults unrecoverably.
    FaultedDraw,
}

/// Category of runtime diagnostic recorded by the script host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticKind {
    RuntimeError,
    InstructionLimit,
    StackOverflow,
    MemoryLimit,
    LoadError,
    CallbackCapReached,
    NativeOpsCapReached,
    CommandLimit,
    EventQueueOverflow,
    RefusedWrite,
    InvalidArgument,
    ListenerDisabled,
    InternalFault,
    HostAborted,
    DecisionDeadline,
}

/// A position in the prepared chain, independent of VM slot reuse or stripped debug data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptSite {
    pub chunk: u32,
    pub function: u32,
    pub instruction: usize,
}

/// One recorded error or safety event from script execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptDiagnostic {
    pub tick: u32,
    pub phase: ScriptPhase,
    pub listener_id: Option<u32>,
    pub kind: DiagnosticKind,
    pub site: Option<ScriptSite>,
    pub occurrences: u32,
    pub message: String,
}

/// Bounded diagnostic counters and logs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptDiagnostics {
    pub ignored_platform_hints: u32,
    pub errors: Vec<ScriptDiagnostic>,
    pub unbound_calls: BTreeMap<String, u32>,
    pub unbound_total: u32,
    pub missing_objects: u32,
    pub ambiguous_objects: u32,
    pub not_available_ops: u32,
    pub refused_writes: u32,
    pub queue_overflows: u32,
    pub callbacks_executed: u32,
    pub callbacks_dropped: u32,
    pub callback_errors: u32,
    pub callback_budget_refusals: u32,
    pub unlisted_callback_errors: u32,
    pub peak_commands: usize,
    pub refused_commands: u32,
    pub faulted: bool,
}

impl ScriptDiagnostics {
    pub(super) fn setup_refusal(&mut self, tick: u32, op: &str, error: crate::battle::rules::objects::RuleError) {
        self.refused_writes = self.refused_writes.saturating_add(1);
        self.record_error(tick, ScriptPhase::Init, None, DiagnosticKind::RefusedWrite, format!("{op}: {error:?}"));
    }

    pub fn record_error(
        &mut self,
        tick: u32,
        phase: ScriptPhase,
        listener_id: Option<u32>,
        kind: DiagnosticKind,
        message: String,
    ) {
        if self.errors.len() == MAX_DIAGNOSTICS && matches!(kind, DiagnosticKind::HostAborted | DiagnosticKind::CommandLimit | DiagnosticKind::EventQueueOverflow) {
            if let Some(index) = self.errors.iter().rposition(|d| !matches!(d.kind, DiagnosticKind::HostAborted | DiagnosticKind::CommandLimit | DiagnosticKind::EventQueueOverflow)) {
                self.errors.remove(index);
            }
        }
        if self.errors.len() < MAX_DIAGNOSTICS {
            let message = if message.capacity() > MAX_DIAGNOSTIC_BYTES {
                message[..message.floor_char_boundary(MAX_DIAGNOSTIC_BYTES)].to_owned()
            } else {
                message
            };
            self.errors.push(ScriptDiagnostic {
                tick,
                phase,
                listener_id,
                kind,
                site: None,
                occurrences: 1,
                message,
            });
        }
    }

    /// Keep the first example of each instruction, even if its error payload changes.
    pub(super) fn callback_error(&mut self, tick: u32, listener: u32, kind: DiagnosticKind,
        message: String, site: Option<ScriptSite>, chunk: &str) {
        self.callback_errors = self.callback_errors.saturating_add(1);
        if let Some(diagnostic) = self.errors.iter_mut().find(|d| d.phase == ScriptPhase::Tick
            && d.kind == kind && d.site == site && (site.is_some() || d.listener_id == Some(listener))) {
            diagnostic.occurrences = diagnostic.occurrences.saturating_add(1);
        } else if self.errors.len() < MAX_DIAGNOSTICS {
            let location = site.map_or_else(|| format!("{chunk} listener {listener}"),
                |s| format!("{chunk} function {} instruction {}", s.function, s.instruction));
            self.record_error(tick, ScriptPhase::Tick, Some(listener), kind, format!("{location}: {message}"));
            let diagnostic = self.errors.last_mut().unwrap();
            diagnostic.site = site;
            eprintln!("Mission callback failed at {:?}", diagnostic.message);
        } else {
            self.unlisted_callback_errors = self.unlisted_callback_errors.saturating_add(1);
        }
    }

    pub fn record_unbound(&mut self, name: &str) {
        self.unbound_total = self.unbound_total.saturating_add(1);
        if let Some(count) = self.unbound_calls.get_mut(name) {
            *count = count.saturating_add(1);
        } else if self.unbound_calls.len() < MAX_UNBOUND_NAMES {
            self.unbound_calls.insert(name.to_string(), 1);
        }
    }
}

/// Event listener family validated by `Release*` host calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenerFamily {
    CharacterDeath,
    TicketCountChange,
    TimerElapse,
    TeamPointsChange,
    ObjectKill,
    ObjectDamage,
    ObjectRespawn,
}

#[derive(Clone, Debug)]
pub(crate) struct ListenerSlot {
    pub id: u32,
    pub callback: ScriptValueRef,
    pub budget_streak: u8,
    pub peak_work: u32,
    pub peak_native: u32,
    pub needs_fresh_tick: bool,
    pub min_seq: u32,
    pub object: Option<(u8, u32, u32)>,
    pub timer_id: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CharacterEntry {
    pub handle: u32,
    pub sim_ref: Ref<Character>,
    pub last_team: i32,
    pub last_human: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ObjectBinding {
    pub handle: ObjectHandle,
    pub userdata_ref: Option<ScriptValueRef>,
}

/// Retained event awaiting delivery inside `ScriptRules`.
#[derive(Clone, Debug)]
pub(crate) enum PendingEvent {
    CharacterDeath {
        death: Option<crate::units::infantry::contact::DeathRecord>,
        victim_ref: Ref<Character>,
        victim_team: i32,
        victim_human: Option<bool>,
        killer_ref: Option<Ref<Character>>,
        killer_team: Option<i32>,
        killer_human: Option<bool>,
    },
    NamedObject {
        absorbed: f64,
        object: ObjectHandle,
        family: ListenerFamily,
        team: i32,
        credit: Option<super::super::rules::awards::Credit>,
    },
    TicketsChanged {
        team: i32,
        count: u32,
        seq: u32,
    },
    TeamPointsChanged {
        team: i32,
        points: u32,
        seq: u32,
    },
    TimerElapsed {
        timer_id: u32,
        arm_gen: u32,
    },
}

impl PendingEvent {
    fn scoreboard_slot(&self) -> Option<usize> {
        let (team, offset) = match *self {
            Self::TeamPointsChanged { team, .. } => (team, 0),
            Self::TicketsChanged { team, .. } => (team, TEAM_COUNT),
            _ => return None,
        };
        let slot = usize::try_from(team).ok()?;
        (slot < TEAM_COUNT).then_some(offset + slot)
    }
}


/// In-progress event retained across ticks when a tick budget interrupts
/// delivery between listeners.
#[derive(Clone, Debug)]
pub(crate) struct InFlightEvent {
    pub event: PendingEvent,
    pub next_listener_id: u32,
}

/// Compact fixed-size `Copy` outcome tag for zero-allocation live telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactOutcome {
    Victory(i32),
    Defeat(i32),
    Draw,
    FaultedDraw,
}

/// Fixed-size zero-allocation live telemetry updated in place at end of tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveTelemetry {
    pub tick: u32,
    pub scores: [u32; TEAM_COUNT],
    /// Authoritative reinforcement counts (`-1` = unlimited, `i32::MIN` = unset).
    pub reinforcements: [i32; TEAM_COUNT],
    pub hud_timer_id: Option<u32>,
    pub hud_timer_remaining: f64,
    pub hud_timer_running: bool,
    pub outcome: Option<CompactOutcome>,
    pub ai_goal_count: u32,
    pub notification_seq: u32,
    pub callbacks_executed: u32,
    pub callbacks_dropped: u32,
    pub callback_errors: u32,
    pub callback_budget_refusals: u32,
    pub refused_commands: u32,
    pub error_count: u32,
    pub unbound_total: u32,
    pub missing_objects: u32,
    pub not_available_ops: u32,
    pub refused_writes: u32,
    pub queue_overflows: u32,
    pub faulted: bool,
    pub rng_state: u32,
    pub rng_draws: u32,
    pub published_digest: Wide,
}

/// Lightweight zero-allocation handle to observe live telemetry of a running
/// [`super::ScriptRules`] instance moved into `World`.
#[derive(Clone, Debug)]
pub struct ScriptTelemetryHandle {
    pub(crate) inner: Rc<Cell<LiveTelemetry>>,
}

impl ScriptTelemetryHandle {
    pub fn read(&self) -> LiveTelemetry {
        self.inner.get()
    }
}

/// On-demand display/inspection snapshot constructed outside hot ticks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptDisplaySnapshot {
    pub tick: u32,
    pub lifecycle: Option<LifecycleState>,
    pub team_scores: BTreeMap<i32, u32>,
    pub team_reinforcements: BTreeMap<i32, i32>,
    pub ai_goals: Vec<AiGoalRecord>,
    pub critical_systems: Vec<CriticalSystemRecord>,
    pub hud_timer: Option<(String, f64, bool)>,
    pub objectives: Vec<ObjectiveEntry>,
    pub notifications: Vec<SequencedNotification>,
    pub outcome: Option<BattleOutcome>,
    pub diagnostics: ScriptDiagnostics,
    pub rng_state: u32,
    pub rng_draws: u32,
    pub published_digest: Wide,
}

/// Opt-in rich inspector handle for tests (never allocated or attached in
/// production admission).
#[derive(Clone, Debug)]
pub struct ScriptInspector {
    pub(crate) inner: Rc<RefCell<ScriptDisplaySnapshot>>,
}

impl ScriptInspector {
    pub fn snapshot(&self) -> ScriptDisplaySnapshot {
        self.inner.borrow().clone()
    }
}

/// Transient character death context active only while a single `OnCharacterDeath`
/// event is being dispatched (preserves launch-snapshot killer team even when the
/// character changed teams or no longer exists).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ActiveDeathContext {
    pub victim_handle: u32,
    pub victim_team: i32,
    pub victim_human: bool,
    pub killer_handle: Option<u32>,
    pub killer_team: Option<i32>,
    pub killer_human: bool,
}

/// Reserved transient handle used when a `UnitKilled` event carries an
/// attributed `killer_team` with no surviving `Ref<Character>`.
pub(crate) const TRANSIENT_ATTRIBUTED_KILLER_HANDLE: u32 = MAX_EXACT_LUA_HANDLE;

/// Complete `'static` host state passed by `&mut HostState` into every VM call.
pub struct HostState {
    /// The native deaths delivered to the script on this rules tick, in callback
    /// order. A death waiting behind a refusal arrives on a later tick and keeps
    /// the tick it happened on.
    pub deaths: Vec<crate::units::infantry::contact::DeathRecord>,
    pub config: ScriptRulesConfig,
    pub phase: ScriptPhase,
    pub lifecycle: LifecycleState,
    pub tick: u32,
    pub setup: LoadSetup,
    pub match_flags: MatchFlags,
    pub ai_goals: Vec<AiGoalRecord>,
    pub timers: Vec<ScriptTimer>,
    pub hud_timer: Option<u32>,
    pub critical_systems: Vec<CriticalSystemRecord>,
    pub linked_critical_systems: Vec<Vec<String>>,
    pub space_assault_bitmaps: Vec<String>,
    pub space_teams: Option<[i32;2]>,
    pub objectives: Vec<ObjectiveEntry>,
    pub team_points_visible: [bool; TEAM_COUNT],
    pub command_posts_hidden: bool,
    pub minimap_disabled: bool,
    pub(crate) labels: VecDeque<(ScriptValueRef, Rc<str>)>,
    pub notifications: VecDeque<SequencedNotification>,
    pub next_notification_seq: u32,
    pub dropped_notifications: u32,
    pub outcome: Option<BattleOutcome>,
    pub diagnostics: ScriptDiagnostics,

    // Authoritative mirrors synced directly from `RuleActions` (never authoritative source).
    pub(crate) cached_scores: [u32; TEAM_COUNT],
    pub(crate) cached_reinforcements: [i32; TEAM_COUNT],
    pub(crate) cached_score_limit: Option<u32>,

    // Shared prepared script chain (first-occurrence ASCII case-insensitive lookup).
    pub(crate) chunks: Rc<[ScriptChunk]>,
    chain_digest: Wide,
    // Indexed by VM prototype slot. Reusing a slot replaces its generation and position.
    function_sites: Vec<(u32, u32, u32)>,

    // Indexed listener buckets by event family (real removal on release).
    pub(crate) listeners_death: Vec<ListenerSlot>,
    pub(crate) listeners_tickets: Vec<ListenerSlot>,
    pub(crate) listeners_points: Vec<ListenerSlot>,
    pub(crate) listeners_timer: Vec<ListenerSlot>,
    pub(crate) listeners_obj_kill: Vec<ListenerSlot>,
    pub(crate) listeners_obj_damage: Vec<ListenerSlot>,
    pub(crate) listeners_obj_respawn: Vec<ListenerSlot>,
    pub(crate) listener_families: BTreeMap<u32, ListenerFamily>,

    // Generation-aware character handles & generation/life-aware object bindings.
    pub(crate) characters: Vec<CharacterEntry>,
    pub(crate) char_by_ref: BTreeMap<(u32, u32), u32>,
    pub(crate) object_bindings: Vec<ObjectBinding>,
    pub(crate) object_by_key: BTreeMap<(u8,u32,u32), u32>,
    pub(crate) active_object: Option<(ObjectHandle, i32, Option<super::super::rules::awards::Credit>, Option<u32>)>,

    // Sequence counters so listeners registered after an earlier mutation do not
    // receive stale events from before registration (including init->postload).
    pub(crate) emitted_ticket_seq: u32,
    pub(crate) delivered_ticket_seq: u32,
    pub(crate) emitted_points_seq: u32,
    pub(crate) delivered_points_seq: u32,

    // Active deterministic voice-over playback completion clock.
    pub(crate) active_voices: Vec<(Rc<str>,f64)>,
    pub(crate) setup_cost: u32,

    // Monotonic exact-f32 handle allocators (bounded by MAX_EXACT_LUA_HANDLE).
    pub(crate) goal_handles: GoalHandles,
    pub(crate) goal_revision: u32,
    pub(crate) next_timer_id: u32,
    pub(crate) next_listener_id: u32,
    pub(crate) next_audio_stream_id: u32,
    pub(crate) next_char_handle: u32,
    pub(crate) next_object_binding_id: u32,

    // Per-tick shared native-work budget & retained delivery queue.
    pub(crate) native_ops_this_tick: u32,
    pub(crate) callbacks_this_tick: usize,
    pub(crate) callback_results: Vec<ScriptValue>,
    pub(crate) callback_error_recorded: bool,
    pub(crate) setup_failure: Option<String>,
    pub(crate) setup_result: Option<(crate::battle::rules::events::Outcome, BattleOutcome)>,
    pub(crate) target_deadline: Option<u32>,
    pub(crate) degraded_deadline: Option<u32>,
    pub(crate) attempted_result: bool,
    pub(crate) pending_fault_result: Option<crate::battle::rules::events::Outcome>,
    pub(crate) pending_events: VecDeque<PendingEvent>,
    pub(super) pending_score_changes: usize,
    world_event_limit: usize,
    score_change_limit: usize,
    // Native damage already applied, awaiting its notification, by exact object life.
    shield_debits: Vec<(ObjectIdentityKey, f64, usize)>,
    pub(crate) syncing_damage: bool,
    pub(crate) in_flight: Option<InFlightEvent>,
    pub(crate) active_death: Option<ActiveDeathContext>,
}

impl HostState {
    pub fn new(chain: ScriptChain, config: ScriptRulesConfig) -> Self {
        Self::with_goal_handles(chain, config, GoalHandles::default())
    }

    pub(super) fn with_goal_handles(chain: ScriptChain, config: ScriptRulesConfig, goal_handles: GoalHandles) -> Self {
        let mut setup = LoadSetup::default();
        setup.teams.insert(config.own_team, TeamSetup::new(config.own_team));
        setup.teams.insert(config.foe_team, TeamSetup::new(config.foe_team));

        Self {
            config,
            phase: ScriptPhase::Chunk,
            lifecycle: LifecycleState::Cold,
            tick: 0,
            deaths: Vec::with_capacity(MAX_PENDING_EVENTS),
            setup,
            match_flags: MatchFlags::default(),
            ai_goals: Vec::with_capacity(GOAL_SLOTS),
            timers: Vec::with_capacity(MAX_TIMERS),
            hud_timer: None,
            critical_systems: Vec::with_capacity(MAX_CRITICAL_SYSTEMS),
            linked_critical_systems: Vec::new(),
            space_assault_bitmaps: Vec::new(),
            space_teams: None,
            objectives: Vec::with_capacity(16),
            team_points_visible: [false; TEAM_COUNT], command_posts_hidden: false, minimap_disabled: false,
            labels: VecDeque::with_capacity(MAX_NOTIFICATIONS),
            notifications: VecDeque::with_capacity(MAX_NOTIFICATIONS),
            next_notification_seq: 1,
            dropped_notifications: 0,
            outcome: None,
            diagnostics: ScriptDiagnostics::default(),
            cached_scores: [0; TEAM_COUNT],
            cached_reinforcements: [i32::MIN; TEAM_COUNT],
            cached_score_limit: None,
            chain_digest: {
                let mut digest = MissionDigest::new();
                digest.field(&chain.entry);
                for chunk in chain.chunks.iter() {
                    digest.field(&chunk.name);
                    digest.field(&chunk.bytes.len());
                    digest.bytes(&chunk.bytes);
                }
                digest.finish()
            },
            chunks: chain.chunks,
            function_sites: Vec::new(),
            listeners_death: Vec::with_capacity(8),
            listeners_tickets: Vec::with_capacity(8),
            listeners_points: Vec::with_capacity(8),
            listeners_timer: Vec::with_capacity(16),
            listeners_obj_kill: Vec::with_capacity(32),
            listeners_obj_damage: Vec::with_capacity(16),
            listeners_obj_respawn: Vec::with_capacity(16),
            listener_families: BTreeMap::new(),
            characters: Vec::with_capacity(64),
            char_by_ref: BTreeMap::new(),
            object_bindings: Vec::with_capacity(64),
            object_by_key: BTreeMap::new(),
            active_object: None,
            emitted_ticket_seq: 0,
            delivered_ticket_seq: 0,
            emitted_points_seq: 0,
            delivered_points_seq: 0,
            active_voices: Vec::with_capacity(32), setup_cost: 0,
            goal_handles,
            goal_revision: 0,
            next_timer_id: 1,
            next_listener_id: 1,
            next_audio_stream_id: 1,
            next_char_handle: 1,
            next_object_binding_id: 1,
            native_ops_this_tick: 0,
            callbacks_this_tick: 0,
            callback_results: Vec::with_capacity(16),
            callback_error_recorded: false,
            setup_failure: None,
            setup_result: None,
            target_deadline: None,
            degraded_deadline: None,
            attempted_result: false,
            pending_fault_result: None,
            pending_events: VecDeque::with_capacity(MAX_PENDING_EVENTS + super::DEFAULT_SCRIPT_COMMAND_LIMIT as usize + 2 * TEAM_COUNT),
            pending_score_changes: 0,
            world_event_limit: MAX_PENDING_EVENTS,
            score_change_limit: super::DEFAULT_SCRIPT_COMMAND_LIMIT as usize + 2 * TEAM_COUNT,
            shield_debits: Vec::with_capacity(MAX_PENDING_EVENTS),
            syncing_damage: false,
            in_flight: None,
            active_death: None,
        }
    }

    pub(super) fn load_chunk(&mut self, vm: &mut ScriptVm<Self>, chunk: usize) -> Result<ScriptClosureId, ScriptError> {
        let sites = &mut self.function_sites;
        vm.load_chunk_with_functions(&self.chunks[chunk].bytes, &mut |(slot, generation), ordinal| {
            let index = slot as usize;
            if sites.len() <= index {
                sites.resize(index + 1, (0, 0, 0));
            }
            sites[index] = (generation, chunk as u32, ordinal);
        })
    }

    pub(super) fn resolve_site(&self, site: Option<InstructionSite>) -> Option<ScriptSite> {
        let site = site?;
        let (slot, generation) = site.function;
        let &(saved, chunk, function) = self.function_sites.get(slot as usize)?;
        (saved == generation).then_some(ScriptSite { chunk, function, instruction: site.instruction })
    }

    /// Charge one unit against the per-tick native work budget during `Tick`.
    #[inline]
    pub(crate) fn charge(&mut self, cost: u32) -> Result<(), super::adapter::ScriptError> {
        if self.phase != ScriptPhase::Tick {
            self.setup_cost = self.setup_cost.saturating_add(cost);
            if self.setup_cost > 16_384 { return Err(super::adapter::ScriptError::ResourceLimit("mission setup storage")); }
            return Ok(());
        }
        self.native_ops_this_tick = self.native_ops_this_tick.saturating_add(cost);
        if self.native_ops_this_tick > self.config.tick_native_ops_budget {
            return Err(super::adapter::ScriptError::InstructionLimit {
                limit: self.config.tick_native_ops_budget,
            });
        }
        Ok(())
    }

    pub fn lookup_chunk_bytes(&self, name: &[u8]) -> Option<&ScriptChunk> {
        self.chunks
            .iter()
            .find(|chunk| ascii_bytes_eq_ignore_case(chunk.name.as_bytes(), name))
    }

    pub fn team_setup_mut(&mut self, team: i32) -> &mut TeamSetup {
        self.setup
            .teams
            .entry(team)
            .or_insert_with(|| TeamSetup::new(team))
    }

    pub fn team_setup(&self, team: i32) -> Option<&TeamSetup> {
        self.setup.teams.get(&team)
    }

    pub fn push_notification(&mut self, notification: ScriptNotification) {
        if self.notifications.len() >= MAX_NOTIFICATIONS {
            self.notifications.pop_front();
            self.dropped_notifications = self.dropped_notifications.saturating_add(1);
        }
        let seq = self.next_notification_seq;
        self.next_notification_seq = self.next_notification_seq.saturating_add(1);
        self.notifications.push_back(SequencedNotification {
            seq,
            tick: self.tick,
            notification,
        });
    }

    /// Cursor accompanying the current persistent state, zero before any notification.
    pub fn last_notification_sequence(&self) -> u32 {
        self.next_notification_seq.saturating_sub(1)
    }

    /// Resume after the last delivered sequence. A gap or future cursor requires a fresh snapshot.
    pub fn notifications_after(&self, last_delivered: u32) -> Option<impl Iterator<Item=&SequencedNotification>> {
        let oldest = self.notifications.front().map_or(self.next_notification_seq, |n| n.seq);
        if last_delivered > self.last_notification_sequence() || last_delivered.saturating_add(1) < oldest {
            return None;
        }
        Some(self.notifications.iter().filter(move |n| n.seq > last_delivered))
    }
    pub fn score_limit(&self) -> u32 { self.config.effective_assault_score_limit(self.cached_score_limit) }

    pub(crate) fn decode_object(&self, tag: u32, payload: Wide) -> Option<ObjectHandle> {
        let mut object = self.object_binding(tag.checked_sub(OBJECT_USERDATA_TAG)?)?.handle.clone();
        match &mut object {
            ObjectHandle::Prop(h) => h.life = payload.to_f64_bits(),
            ObjectHandle::Vehicle { life,.. } => *life = payload.to_f64_bits(),
            ObjectHandle::Post(_) | ObjectHandle::Door(_) => {}
        }
        Some(object)
    }

    /// Take queued notifications for a fixture assertion.
    #[cfg(test)]
    pub(crate) fn drain_notifications(&mut self) -> Vec<SequencedNotification> {
        self.notifications.drain(..).collect()
    }

    /// Register or look up a generation-aware character handle without overwriting
    /// its stored human flag when `human` is `None` (e.g. stale/departed character).
    pub(crate) fn ensure_character(
        &mut self,
        sim_ref: Ref<Character>,
        team: i32,
        human: Option<bool>,
    ) -> Option<(u32, bool)> {
        let key = (sim_ref.slot, sim_ref.gen);
        if let Some(&handle) = self.char_by_ref.get(&key) {
            let resolved_human = if let Some(entry) = self.characters.get_mut((handle - 1) as usize)
            {
                entry.last_team = team;
                if let Some(h) = human {
                    entry.last_human = h;
                }
                entry.last_human
            } else {
                human.unwrap_or(false)
            };
            return Some((handle, resolved_human));
        }
        if self.characters.len() >= 1024 || self.next_char_handle >= TRANSIENT_ATTRIBUTED_KILLER_HANDLE {
            return None;
        }
        let handle = self.next_char_handle;
        self.next_char_handle += 1;
        let resolved_human = human.unwrap_or(false);
        self.characters.push(CharacterEntry {
            handle,
            sim_ref,
            last_team: team,
            last_human: resolved_human,
        });
        self.char_by_ref.insert(key, handle);
        Some((handle, resolved_human))
    }

    pub(crate) fn character(&self, handle: u32) -> Option<&CharacterEntry> {
        handle
            .checked_sub(1)
            .and_then(|idx| self.characters.get(idx as usize))
    }

    /// Look up or allocate a generation- and life-aware binding for an
    /// authoritative [`ObjectHandle`].
    pub(crate) fn bind_object_handle(&mut self, handle: ObjectHandle) -> Option<u32> {
        let key = crate::battle::rules::objects::key(&handle);
        if let Some(&id) = self.object_by_key.get(&key) {
            return Some(id);
        }
        if self.object_bindings.len() >= 4096 || self.next_object_binding_id > MAX_EXACT_LUA_HANDLE {
            return None;
        }
        let id = self.next_object_binding_id;
        self.next_object_binding_id += 1;
        self.object_bindings.push(ObjectBinding {
            handle,
            userdata_ref: None,
        });
        self.object_by_key.insert(key, id);
        Some(id)
    }

    pub(crate) fn object_binding(&self, id: u32) -> Option<&ObjectBinding> {
        id.checked_sub(1)
            .and_then(|idx| self.object_bindings.get(idx as usize))
    }

    pub(crate) fn object_binding_mut(&mut self, id: u32) -> Option<&mut ObjectBinding> {
        id.checked_sub(1)
            .and_then(|idx| self.object_bindings.get_mut(idx as usize))
    }

    pub(super) fn record_shield_debit(&mut self, event: &PendingEvent) {
        let PendingEvent::NamedObject { object, absorbed, .. } = event else { return };
        if *absorbed <= 0.0 { return; }
        let key = ObjectIdentityKey::of(object);
        match self.shield_debits.binary_search_by_key(&key, |row| row.0) {
            Ok(i) => {
                self.shield_debits[i].1 += absorbed;
                self.shield_debits[i].2 += 1;
            }
            Err(i) => self.shield_debits.insert(i, (key, *absorbed, 1)),
        }
    }

    pub(super) fn deliver_shield_debit(&mut self, event: &PendingEvent) {
        let PendingEvent::NamedObject { object, absorbed, .. } = event else { return };
        if *absorbed <= 0.0 { return; }
        let key = ObjectIdentityKey::of(object);
        let i = self.shield_debits.binary_search_by_key(&key, |row| row.0)
            .expect("queued shield damage");
        if self.shield_debits[i].2 == 1 {
            self.shield_debits.remove(i);
        } else {
            self.shield_debits[i].1 = jsrt::js_max(0.0, self.shield_debits[i].1 - absorbed);
            self.shield_debits[i].2 -= 1;
        }
    }

    /// A charge propagated by a damage callback must preserve native hits on
    /// linked parts whose own callbacks have yet to observe their damage.
    pub(super) fn shield_write(&self, object: &ObjectHandle, charge: f64) -> f64 {
        if self.syncing_damage {
            let key = ObjectIdentityKey::of(object);
            if let Ok(i) = self.shield_debits.binary_search_by_key(&key, |row| row.0) {
                return jsrt::js_max(0.0, charge - self.shield_debits[i].1);
            }
        }
        jsrt::js_max(0.0, charge)
    }

    pub fn timer(&self, id: u32) -> Option<&ScriptTimer> {
        self.timers.iter().find(|t| t.id == id && t.alive)
    }

    pub fn timer_mut(&mut self, id: u32) -> Option<&mut ScriptTimer> {
        self.timers.iter_mut().find(|t| t.id == id && t.alive)
    }

    pub fn find_timer_by_bytes(&self, name: &[u8]) -> Option<u32> {
        self.timers
            .iter()
            .rev()
            .find(|t| t.alive && ascii_bytes_eq_ignore_case(&t.name_bytes, name))
            .map(|t| t.id)
    }

    pub fn create_timer(&mut self, name_bytes: Vec<u8>) -> Option<u32> {
        let live_count = self.timers.iter().filter(|t| t.alive).count();
        if live_count >= MAX_TIMERS || self.next_timer_id > MAX_EXACT_LUA_HANDLE {
            return None;
        }
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        let timer = ScriptTimer {
            id,
            name_bytes,
            remaining: 0.0,
            rate: 1.0,
            running: false,
            alive: true,
            arm_gen: 1,
            started_tick: self.tick,
        };
        if let Some(dead_slot) = self.timers.iter_mut().find(|t| !t.alive) {
            *dead_slot = timer;
        } else {
            self.timers.push(timer);
        }
        Some(id)
    }

    // One complete producer batch of physical events, one change per accepted command,
    // and up to two retained values (points and tickets) per team after compaction.
    pub(super) fn reserve_delivery(&mut self, batch: usize, commands: usize) -> Option<()> {
        self.world_event_limit = batch;
        self.score_change_limit = commands.checked_add(2 * TEAM_COUNT)?;
        let total = batch.checked_add(self.score_change_limit)?;
        self.pending_events.try_reserve_exact(total.saturating_sub(self.pending_events.len())).ok()?;
        self.shield_debits.try_reserve_exact(batch.saturating_sub(self.shield_debits.len())).ok()
    }

    pub(super) fn queue_delivery(&mut self, event: PendingEvent) -> bool {
        if let Some(incoming) = event.scoreboard_slot() {
            if self.pending_score_changes >= self.score_change_limit {
                // Absolute values supersede older undelivered samples only under backpressure.
                // Keep each final sample at its original position relative to other event kinds.
                let mut newest = [None; 2 * TEAM_COUNT];
                for (index, row) in self.pending_events.iter().enumerate() {
                    if let Some(slot) = row.scoreboard_slot() { newest[slot] = Some(index); }
                }
                newest[incoming] = None;
                let before = self.pending_events.len();
                let mut index = 0;
                self.pending_events.retain(|row| {
                    let keep = row.scoreboard_slot().is_none_or(|slot| newest[slot] == Some(index));
                    index += 1;
                    keep
                });
                self.pending_score_changes -= before - self.pending_events.len();
            }
            self.pending_score_changes += 1;
        } else if self.pending_events.len() - self.pending_score_changes >= self.world_event_limit {
            if let PendingEvent::TimerElapsed { timer_id, arm_gen } = event {
                self.begin_timer_delivery(timer_id, arm_gen, 0);
            }
            return false;
        }
        self.record_shield_debit(&event);
        self.pending_events.push_back(event);
        true
    }

    pub(super) fn pop_delivery(&mut self) -> Option<PendingEvent> {
        let event = self.pending_events.pop_front()?;
        if event.scoreboard_slot().is_some() { self.pending_score_changes -= 1; }
        self.deliver_shield_debit(&event);
        Some(event)
    }

    pub(crate) fn invalidate_pending_timer(&mut self, timer_id: u32) {
        self.pending_events
            .retain(|ev| !matches!(ev, PendingEvent::TimerElapsed { timer_id: id, .. } if *id == timer_id));
        if let Some(ref f) = self.in_flight {
            if matches!(f.event, PendingEvent::TimerElapsed { timer_id: id, .. } if id == timer_id)
            {
                self.in_flight = None;
            }
        }
    }

    pub(crate) fn add_listener(
        &mut self,
        family: ListenerFamily,
        callback: ScriptValueRef,
        object: Option<(u8, u32, u32)>,
        timer_id: u32,
    ) -> Option<u32> {
        if self.listener_families.len() >= MAX_LISTENERS
            || self.next_listener_id > MAX_EXACT_LUA_HANDLE
        {
            return None;
        }
        let id = self.next_listener_id;
        self.next_listener_id += 1;
        let min_seq = match family {
            ListenerFamily::TicketCountChange => self.emitted_ticket_seq,
            ListenerFamily::TeamPointsChange => self.emitted_points_seq,
            _ => 0,
        };
        let slot = ListenerSlot {
            id,
            callback,
            budget_streak: 0,
            peak_work: 0,
            peak_native: 0,
            needs_fresh_tick: false,
            min_seq,
            object,
            timer_id,
        };
        let bucket = self.listener_bucket_mut(family);
        let index = bucket.partition_point(|s| (s.object, s.id) < (object, id));
        bucket.insert(index, slot);
        self.listener_families.insert(id, family);
        Some(id)
    }

    pub(crate) fn remove_listener_of_family(
        &mut self,
        id: u32,
        expected_family: Option<ListenerFamily>,
    ) -> Result<Option<ScriptValueRef>, ListenerFamily> {
        let Some(&actual_family) = self.listener_families.get(&id) else {
            return Ok(None);
        };
        if let Some(expected) = expected_family {
            if actual_family != expected {
                return Err(actual_family);
            }
        }
        self.listener_families.remove(&id);
        let bucket = self.listener_bucket_mut(actual_family);
        if let Some(pos) = bucket.iter().position(|s| s.id == id) {
            let removed = bucket.remove(pos);
            return Ok(Some(removed.callback));
        }
        Ok(None)
    }

    pub(crate) fn listener_bucket(&self, family: ListenerFamily) -> &[ListenerSlot] {
        match family {
            ListenerFamily::CharacterDeath => &self.listeners_death,
            ListenerFamily::TicketCountChange => &self.listeners_tickets,
            ListenerFamily::TimerElapse => &self.listeners_timer,
            ListenerFamily::TeamPointsChange => &self.listeners_points,
            ListenerFamily::ObjectKill => &self.listeners_obj_kill,
            ListenerFamily::ObjectDamage => &self.listeners_obj_damage,
            ListenerFamily::ObjectRespawn => &self.listeners_obj_respawn,
        }
    }

    pub(super) fn listener_bucket_mut(&mut self, family: ListenerFamily) -> &mut Vec<ListenerSlot> {
        match family {
            ListenerFamily::CharacterDeath => &mut self.listeners_death,
            ListenerFamily::TicketCountChange => &mut self.listeners_tickets,
            ListenerFamily::TimerElapse => &mut self.listeners_timer,
            ListenerFamily::TeamPointsChange => &mut self.listeners_points,
            ListenerFamily::ObjectKill => &mut self.listeners_obj_kill,
            ListenerFamily::ObjectDamage => &mut self.listeners_obj_damage,
            ListenerFamily::ObjectRespawn => &mut self.listeners_obj_respawn,
        }
    }

    pub fn scores(&self) -> &[u32; TEAM_COUNT] { &self.cached_scores }
    pub fn reinforcements(&self) -> &[i32; TEAM_COUNT] { &self.cached_reinforcements }

    /// Inspect the quiescent script authority, including pending deliveries,
    /// callback roots, allocation cursors and all state retained by native calls.
    pub(super) fn execution_digest(&self, vm: Wide, rng: (u32, u32)) -> Wide {
        let mut h = MissionDigest::new();
        h.field(&(self.chain_digest, vm, rng));
        macro_rules! fields { ($($field:ident),+ $(,)?) => { $(h.field(&self.$field);)+ }; }
        h.field(&self.goal_handles.next());
        fields!(config, phase, lifecycle, tick, setup, match_flags, ai_goals, timers,
            hud_timer, critical_systems, linked_critical_systems, space_assault_bitmaps, space_teams, objectives,
            team_points_visible, command_posts_hidden, minimap_disabled, notifications, next_notification_seq,
            dropped_notifications, outcome, diagnostics, cached_scores, cached_reinforcements, cached_score_limit,
            listener_families, characters, char_by_ref, object_by_key, active_object,
            emitted_ticket_seq, delivered_ticket_seq, emitted_points_seq, delivered_points_seq,
            active_voices, setup_cost, goal_revision, next_timer_id, next_listener_id, next_audio_stream_id,
            next_char_handle, next_object_binding_id, native_ops_this_tick, callbacks_this_tick,
            pending_fault_result, pending_events, shield_debits, in_flight, active_death);
        if self.target_deadline.is_some() || self.degraded_deadline.is_some() {
            h.field(&(self.target_deadline, self.degraded_deadline));
        }
        for (id,label) in &self.labels { h.field(&(id.local_key(),label)); }
        for b in &self.object_bindings { h.field(&(&b.handle,b.userdata_ref.map(|r|r.local_key()))); }
        for bucket in [&self.listeners_death,&self.listeners_tickets,&self.listeners_points,&self.listeners_timer,
            &self.listeners_obj_kill,&self.listeners_obj_damage,&self.listeners_obj_respawn] {
            h.field(&bucket.len());
            for s in bucket { h.field(&(s.id,s.callback.local_key(),s.min_seq,s.object,s.timer_id,s.budget_streak,s.peak_work,s.peak_native,s.needs_fresh_tick)); }
        }
        h.finish()
    }

    /// Fixed-width checksum for the copyable tick telemetry and its presentation queues.
    pub fn published_digest(&self, rng_state: u32, rng_draws: u32) -> Wide {
        let mut h = Fnv::new();
        let mix = |h: &mut Fnv, bytes: &[u8]| h.bytes(bytes);
        let mix_u32 = |h: &mut Fnv, v: u32| mix(h, &v.to_le_bytes());
        let mix_f64 = |h: &mut Fnv, v: f64| mix(h, &v.to_le_bytes());
        let mix_word = |h: &mut Fnv, v: Wide| mix(h, &v.to_le_bytes());
        // A counter keeps the eight little-endian bytes it has always contributed.
        let mix_count = |h: &mut Fnv, v: u32| h.small(v);
        let mix_len = |h: &mut Fnv, n: usize| mix_count(h, u32::try_from(n).unwrap_or(u32::MAX));
        let mix_i32 = |h: &mut Fnv, v: i32| mix(h, &v.to_le_bytes());
        let mix_bytes = |h: &mut Fnv, b: &[u8]| {
            mix_len(h, b.len());
            mix(h, b);
        };

        mix_count(&mut h, self.tick);
        mix(&mut h, &[self.lifecycle as u8, self.phase as u8]);
        for s in self.cached_scores {
            mix_u32(&mut h, s);
        }
        for r in self.cached_reinforcements {
            mix_i32(&mut h, r);
        }
        match self.cached_score_limit {
            None => mix(&mut h, &[0]),
            Some(l) => {
                mix(&mut h, &[1]);
                mix_u32(&mut h, l);
            }
        }
        match &self.outcome {
            None => mix(&mut h, &[0]),
            Some(BattleOutcome::Victory(t)) => {
                mix(&mut h, &[1]);
                mix_i32(&mut h, *t);
            }
            Some(BattleOutcome::Defeat(t)) => {
                mix(&mut h, &[2]);
                mix_i32(&mut h, *t);
            }
            Some(BattleOutcome::Draw(ts)) => {
                mix(&mut h, &[3]);
                mix_len(&mut h, ts.len());
                for &t in ts {
                    mix_i32(&mut h, t);
                }
            }
            Some(BattleOutcome::FaultedDraw) => mix(&mut h, &[4]),
        }

        // All live timers (including pending non-HUD timers).
        mix_len(&mut h, self.timers.len());
        for t in &self.timers {
            if !t.alive {
                continue;
            }
            mix_u32(&mut h, t.id);
            mix_bytes(&mut h, &t.name_bytes);
            mix_f64(&mut h, t.remaining);
            mix_f64(&mut h, t.rate);
            mix(&mut h, &[u8::from(t.running)]);
            mix_count(&mut h, t.arm_gen);
        }
        mix_u32(&mut h, self.hud_timer.unwrap_or(0));

        // Retained pending events & in-flight cursor.
        mix_len(&mut h, self.pending_events.len());
        mix_u32(
            &mut h,
            self.in_flight.as_ref().map_or(0, |f| f.next_listener_id),
        );

        // Active listeners across all families.
        for family in [
            ListenerFamily::CharacterDeath,
            ListenerFamily::TicketCountChange,
            ListenerFamily::TimerElapse,
            ListenerFamily::TeamPointsChange,
            ListenerFamily::ObjectKill,
            ListenerFamily::ObjectDamage,
            ListenerFamily::ObjectRespawn,
        ] {
            let bucket = self.listener_bucket(family);
            mix_len(&mut h, bucket.len());
            for slot in bucket {
                mix_u32(&mut h, slot.id);
                mix_count(&mut h, slot.min_seq);
                mix_u32(&mut h, slot.timer_id);
                if let Some((kind, index, generation)) = slot.object {
                    mix(&mut h, &[kind]); mix_u32(&mut h, index); mix_u32(&mut h, generation);
                }
            }
        }

        // AI goals including typed targets.
        mix_len(&mut h, self.ai_goals.len());
        for g in &self.ai_goals {
            mix_u32(&mut h, g.id);
            mix(&mut h, &[g.slot, g.team, g.kind as u8]);
            mix_u32(&mut h, g.weight.to_bits());
            for target in [&g.target1, &g.target2] {
                match target {
                    None => mix(&mut h, &[0]),
                    Some(AiGoalTarget::Name(b)) => {
                        mix(&mut h, &[1]);
                        mix_bytes(&mut h, b);
                    }
                    Some(AiGoalTarget::Character(c)) => {
                        mix(&mut h, &[2]);
                        mix_u32(&mut h, c.slot); mix_u32(&mut h, c.gen);
                    }
                    Some(AiGoalTarget::Object(o)) => {
                        mix(&mut h, &[3]);
                        match ObjectIdentityKey::of(o) {
                            ObjectIdentityKey::Door(instance) => { mix(&mut h, &[4]); mix_u32(&mut h, instance); }
                            ObjectIdentityKey::Prop { kind, slot, generation, life_bits } => {
                                mix(&mut h, &[1, kind]);
                                mix_u32(&mut h, slot);
                                mix_u32(&mut h, generation);
                                mix_word(&mut h, life_bits);
                            }
                            ObjectIdentityKey::Post { is_mobile, slot, generation } => {
                                mix(&mut h, &[2, u8::from(is_mobile)]);
                                mix_u32(&mut h, slot);
                                mix_u32(&mut h, generation);
                            }
                            ObjectIdentityKey::Vehicle { slot, generation, life_bits } => {
                                mix(&mut h, &[3]);
                                mix_u32(&mut h, slot);
                                mix_u32(&mut h, generation);
                                mix_word(&mut h, life_bits);
                            }
                        }
                    }
                }
            }
        }

        // Objectives.
        mix_len(&mut h, self.objectives.len());
        for obj in &self.objectives {
            mix_i32(&mut h, obj.team);
            mix_bytes(&mut h, obj.text_key.as_bytes());
            mix(&mut h, &[u8::from(obj.active), u8::from(obj.completed)]);
        }

        for system in &self.critical_systems {
            mix_bytes(&mut h, system.name.as_bytes());
            mix_i32(&mut h, system.points);
            mix_u32(&mut h, system.hud_u.to_bits());
            mix_u32(&mut h, system.hud_v.to_bits());
            mix(&mut h, &[u8::from(system.show_marker)]);
        }

        // Notifications with seq & payload.
        mix_count(&mut h, self.next_notification_seq);
        mix_count(&mut h, self.dropped_notifications);
        mix_len(&mut h, self.notifications.len());
        for n in &self.notifications {
            mix_count(&mut h, n.seq);
            mix_count(&mut h, n.tick);
            match &n.notification {
                ScriptNotification::AddMissionObjective { team, text_key, popup_key } => {
                    mix(&mut h, &[1]);
                    mix_i32(&mut h, *team);
                    mix_bytes(&mut h, text_key.as_bytes());
                    mix_bytes(&mut h, popup_key.as_deref().unwrap_or("").as_bytes());
                }
                ScriptNotification::ActivateObjective { text_key } => {
                    mix(&mut h, &[2]);
                    mix_bytes(&mut h, text_key.as_bytes());
                }
                ScriptNotification::CompleteObjective { text_key } => {
                    mix(&mut h, &[3]);
                    mix_bytes(&mut h, text_key.as_bytes());
                }
                ScriptNotification::ShowTeamPoints { team, enabled } => {
                    mix(&mut h, &[4, u8::from(*enabled)]);
                    mix_i32(&mut h, *team);
                }
                ScriptNotification::ShowHuntScoreLimit { mode } => {
                    mix(&mut h, &[5]);
                    mix_i32(&mut h, *mode);
                }
                ScriptNotification::ShowMessageText { key, team } => {
                    mix(&mut h, &[6]);
                    mix_bytes(&mut h, key.as_bytes());
                    mix_i32(&mut h, team.unwrap_or(-1));
                }
                ScriptNotification::BroadcastVoiceOver { sound_id, team } => {
                    mix(&mut h, &[7]);
                    mix_bytes(&mut h, sound_id.as_bytes());
                    mix_i32(&mut h, team.unwrap_or(-1));
                }
                ScriptNotification::MapHideCommandPosts { hide } => {
                    mix(&mut h, &[8, u8::from(*hide)]);
                }
                ScriptNotification::DisableSmallMapMiniMap => mix(&mut h, &[9]),
                ScriptNotification::ShowTimer { timer_name } => {
                    mix(&mut h, &[10]);
                    mix_bytes(&mut h, timer_name.as_deref().unwrap_or("").as_bytes());
                }
                ScriptNotification::AddMissionHint { key } => {
                    mix(&mut h, &[11]);
                    mix_bytes(&mut h, key.as_bytes());
                }
            }
        }

        // Diagnostics & unbound counts.
        mix_count(&mut h, self.diagnostics.callbacks_executed);
        mix_count(&mut h, self.diagnostics.callbacks_dropped);
        mix_count(&mut h, self.diagnostics.callback_errors);
        mix_count(&mut h, self.diagnostics.callback_budget_refusals);
        mix_count(&mut h, self.diagnostics.unlisted_callback_errors);
        mix_count(&mut h, self.diagnostics.refused_commands);
        mix_count(&mut h, self.diagnostics.unbound_total);
        mix_count(&mut h, self.diagnostics.missing_objects);
        mix_count(&mut h, self.diagnostics.ambiguous_objects);
        mix_count(&mut h, self.diagnostics.not_available_ops);
        mix_count(&mut h, self.diagnostics.refused_writes);
        mix_count(&mut h, self.diagnostics.queue_overflows);
        mix(&mut h, &[u8::from(self.diagnostics.faulted)]);
        mix_len(&mut h, self.diagnostics.errors.len());
        for err in &self.diagnostics.errors {
            mix_count(&mut h, err.tick);
            mix(&mut h, &[err.phase as u8, err.kind as u8]);
            mix_u32(&mut h, err.listener_id.unwrap_or(0));
            mix_count(&mut h, err.occurrences);
            mix_bytes(&mut h, err.message.as_bytes());
        }
        for (name, &count) in &self.diagnostics.unbound_calls {
            mix_bytes(&mut h, name.as_bytes());
            mix_u32(&mut h, count);
        }

        // RNG & VO clock.
        for (voice, time) in &self.active_voices { mix_bytes(&mut h,voice.as_bytes()); mix_f64(&mut h, *time); }
        mix_u32(&mut h, rng_state);
        mix_count(&mut h, rng_draws);
        h.finish()
    }

    pub(super) fn compact_outcome(&self) -> Option<CompactOutcome> {
        self.outcome.as_ref().map(|outcome| match outcome {
            BattleOutcome::Victory(team) => CompactOutcome::Victory(*team),
            BattleOutcome::Defeat(team) => CompactOutcome::Defeat(*team),
            BattleOutcome::Draw(_) => CompactOutcome::Draw,
            BattleOutcome::FaultedDraw => CompactOutcome::FaultedDraw,
        })
    }

    pub(crate) fn build_live_telemetry(&self, rng_state: u32, rng_draws: u32) -> LiveTelemetry {
        let (hud_rem, hud_run) = self
            .hud_timer
            .and_then(|id| self.timer(id))
            .map_or((0.0, false), |t| (t.remaining, t.running));
        let outcome = self.compact_outcome();
        LiveTelemetry {
            tick: self.tick,
            scores: self.cached_scores,
            reinforcements: self.cached_reinforcements,
            hud_timer_id: self.hud_timer,
            hud_timer_remaining: hud_rem,
            hud_timer_running: hud_run,
            outcome,
            ai_goal_count: self.ai_goals.len() as u32,
            notification_seq: self.last_notification_sequence(),
            callbacks_executed: self.diagnostics.callbacks_executed,
            callbacks_dropped: self.diagnostics.callbacks_dropped,
            callback_errors: self.diagnostics.callback_errors,
            callback_budget_refusals: self.diagnostics.callback_budget_refusals,
            refused_commands: self.diagnostics.refused_commands,
            error_count: self.diagnostics.errors.len() as u32,
            unbound_total: self.diagnostics.unbound_total,
            missing_objects: self.diagnostics.missing_objects,
            not_available_ops: self.diagnostics.not_available_ops,
            refused_writes: self.diagnostics.refused_writes,
            queue_overflows: self.diagnostics.queue_overflows,
            faulted: self.diagnostics.faulted,
            rng_state,
            rng_draws,
            published_digest: self.published_digest(rng_state, rng_draws),
        }
    }

    pub(crate) fn build_display_snapshot(
        &self,
        rng_state: u32,
        rng_draws: u32,
    ) -> ScriptDisplaySnapshot {
        let mut team_scores = BTreeMap::new();
        let mut team_reinforcements = BTreeMap::new();
        for team in 1..TEAM_COUNT {
            team_scores.insert(team as i32, self.cached_scores[team]);
            if self.cached_reinforcements[team] != i32::MIN {
                team_reinforcements.insert(team as i32, self.cached_reinforcements[team]);
            }
        }
        let hud_timer = self
            .hud_timer
            .and_then(|id| self.timer(id))
            .map(|t| (t.name_lossy(), t.remaining, t.running));
        ScriptDisplaySnapshot {
            tick: self.tick,
            lifecycle: Some(self.lifecycle),
            team_scores,
            team_reinforcements,
            ai_goals: self.ai_goals.clone(),
            critical_systems: self.critical_systems.clone(),
            hud_timer,
            objectives: self.objectives.clone(),
            notifications: self.notifications.iter().cloned().collect(),
            outcome: self.outcome.clone(),
            diagnostics: self.diagnostics.clone(),
            rng_state,
            rng_draws,
            published_digest: self.published_digest(rng_state, rng_draws),
        }
    }
}


/// Stream ordered, escaped Rust field encodings into the diagnostic checksum.
/// Handles use VM-local slot and generation. The encoding is version-local.
pub(crate) struct MissionDigest(Fnv);
impl MissionDigest {
    pub(crate) fn new() -> Self { Self(Fnv::new()) }
    fn bytes(&mut self, bytes:&[u8]) { self.0.bytes(bytes); }
    pub(crate) fn finish(self) -> Wide { self.0.finish() }
    pub(crate) fn field(&mut self, field:&impl std::fmt::Debug) {
        use std::fmt::Write;
        write!(self, "{field:?}\0").expect("infallible digest sink");
    }
}
impl std::fmt::Write for MissionDigest {
    fn write_str(&mut self,s:&str)->std::fmt::Result { self.bytes(s.as_bytes()); Ok(()) }
}
