//! Authority-side mission script host for *Star Wars: Battlefront II* (2005).
//!
//! Runs a prepared 2005 mission's compiled Lua 5.0 script chain inside the
//! simulation through [`bfe_script`] and the battle rules interface
//! (`EventRules` / `RuleActions`). The 2004 game (`GameFamily::Swbf1`) never
//! constructs or invokes anything in this module.

pub mod adapter;
mod events;
mod host_api;
pub mod rng;
pub mod state;

use bfe_wide::Wide;
use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

#[cfg(test)]
use crate::battle::rules::events::Reinforcements;
#[cfg(test)]
use crate::world::owner::World;

use adapter::{ScriptError, ScriptNativeFnId, ScriptValue, ScriptVm};
use rng::SimScriptRng;

pub use state::{
    AiGoalRecord, AiGoalTarget, AudioConfigSetup, AudioStreamRecord, BattleOutcome,
    CompactOutcome, CriticalSystemRecord, DataFileRequest, DiagnosticKind, HostState,
    LifecycleState, ListenerFamily, LiveTelemetry, LoadSetup, MatchFlags, ObjectIdentityKey,
    ObjectiveEntry, ScriptChain, ScriptChunk, ScriptDiagnostic, ScriptDiagnostics,
    ScriptDisplaySnapshot, ScriptInspector, ScriptNotification, ScriptPhase, ScriptRulesConfig,
    ScriptTelemetryHandle, ScriptTimer, SequencedNotification, TeamSetup, UnitClassSetup,
    MAX_DIAGNOSTICS, MAX_LISTENERS, MAX_NOTIFICATIONS, MAX_PENDING_EVENTS, MAX_TIMERS,
    MAX_UNBOUND_NAMES, PROVISIONAL_ASSAULT_NUM_BOTS, PROVISIONAL_ASSAULT_SCORE_LIMIT,
    PROVISIONAL_HUNT_SCORE_LIMIT, PROVISIONAL_UBER_SCORE_LIMIT,
};

/// Default command budget per tick requested for scripted 2005 battles.
// Allows a command per native call plus one award per queued event, with
// room for setup writes on the activation tick.
pub const DEFAULT_SCRIPT_COMMAND_LIMIT: u32 = 4_096;
/// Default concurrent timer slots requested for scripted 2005 battles.
pub const DEFAULT_SCRIPT_TIMER_LIMIT: u32 = MAX_TIMERS as u32;

/// Persistent mission state borrowed at one authority observation.
pub struct MissionView<'a> {
    pub host: &'a HostState,
    pub rules: &'a super::rules::events::EventDriven,
    pub battle: &'a crate::gen::Battle,
    pub human_spawn: &'a crate::gen::HumanSpawn,
}

/// Two-team score and result from one completed rules observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissionScore {
    pub script_tick: u32,
    pub teams: [u32; 2],
    pub target: Option<u32>,
    pub outcome: Option<CompactOutcome>,
    pub over: bool,
    pub game_over: bool,
    pub finished: bool,
    pub faulted: bool,
}

impl MissionView<'_> {
    pub fn score(&self) -> MissionScore {
        let teams = self.rules.team_points();
        MissionScore {
            script_tick: self.host.tick,
            teams: [teams[1], teams[2]],
            target: self.rules.score_limit(),
            outcome: self.host.compact_outcome(),
            over: self.battle.over,
            game_over: self.battle.game_over,
            finished: self.battle.finished,
            faulted: self.host.diagnostics.faulted,
        }
    }

    /// Absolute deadline at 60 Hz, in the calling authority's tick domain.
    pub fn timer_due(&self, timer_id: u32, observed_tick: u32) -> Option<u32> {
        let timer = self.host.timer(timer_id)?;
        if !timer.alive || !timer.running { return None; }
        let (_, ticks) = self.rules.running_timers().find(|(id, _)| *id == timer.rules_id())?;
        Some(observed_tick.saturating_add(ticks.ceil() as u32))
    }

    /// The timer the script shows on the HUD, as the authority and a local
    /// world both read it: its deadline while it runs (none once the match is
    /// over, whose clock stands still with its rules) and the ticks it has left.
    pub fn shown_timer(&self, observed_tick: u32) -> Option<ShownTimer<'_>> {
        let id = self.host.hud_timer?;
        let timer = self.host.timer(id).filter(|timer| timer.alive)?;
        let due = if self.battle.over { None } else { self.timer_due(id, observed_tick) };
        Some(ShownTimer::of(timer, due))
    }
}

/// A timer the script shows on the HUD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShownTimer<'a> {
    pub name: &'a [u8],
    /// The tick it is due at while it runs.
    pub due: Option<u32>,
    /// The ticks it has left, running or stopped.
    pub left: u32,
}

impl<'a> ShownTimer<'a> {
    pub fn of(timer: &'a state::ScriptTimer, due: Option<u32>) -> Self {
        let left = (timer.remaining * 60.).ceil().clamp(0., f64::from(u32::MAX)) as u32;
        Self { name: &timer.name_bytes, due, left }
    }
}

/// Error returned during admission-time script loading or `ScriptPostLoad`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptHostError {
    /// Entry script was missing from the supplied [`ScriptChain`].
    MissingEntry(String),
    /// Host is already in terminal `Faulted` state after a prior lifecycle failure.
    Faulted,
    /// VM initialization or chunk/callback execution failed in `phase`.
    Execution { phase: ScriptPhase, error: ScriptError },
}

fn diagnostic_message(error: &ScriptError) -> std::borrow::Cow<'_, str> {
    match error {
        ScriptError::Runtime(text) => {
            let end = text.floor_char_boundary(state::MAX_DIAGNOSTIC_BYTES);
            std::borrow::Cow::Borrowed(&text[..end])
        }
        other => std::borrow::Cow::Owned(other.to_string()),
    }
}

impl fmt::Display for ScriptHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptHostError::MissingEntry(name) => {
                write!(f, "mission entry script '{name}' missing from ScriptChain")
            }
            ScriptHostError::Faulted => {
                write!(f, "mission script host is in terminal faulted state")
            }
            ScriptHostError::Execution { phase, error } => {
                write!(f, "mission script error during {phase:?}: {}", diagnostic_message(error))
            }
        }
    }
}

impl std::error::Error for ScriptHostError {}

/// Authority-side mission script rule set implementing `EventRules`.
///
/// One instance lives per admitted 2005 battle inside `RuleSet::EventDriven`,
/// owning the Lua 5.0 VM, deterministic script RNG stream, host state, and
/// pre-reserved per-tick dispatch buffers.
pub struct ScriptRules {
    pub(crate) vm: ScriptVm<HostState>,
    pub(crate) host: HostState,
    pub(crate) rng: SimScriptRng,
    entry_name: String,
    pub(crate) menu_title: Option<String>,
    load_trampoline: ScriptNativeFnId,
    post_load_trampoline: ScriptNativeFnId,
    pub(crate) tick_trampoline: ScriptNativeFnId,
    pub(crate) call_results: Vec<ScriptValue>,
    telemetry: Option<ScriptTelemetryHandle>,
    #[cfg(test)]
    inspector: Option<ScriptInspector>,
}

impl ScriptRules {
    /// Construct a new script-driven rule set for `chain` and `config`.
    ///
    /// Does not execute any script code until [`ScriptRules::run_load`] or the
    /// first authoritative tick runs.
    pub fn new(chain: ScriptChain, config: ScriptRulesConfig) -> Result<Self, ScriptHostError> {
        Self::with_goal_handles(chain, config, Default::default())
    }

    /// Construct a world-bound handler before its entry chunk or setup runs.
    pub fn with_goal_handles(chain: ScriptChain, config: ScriptRulesConfig, handles: crate::ai::goals::GoalHandles) -> Result<Self, ScriptHostError> {
        if config.max_callbacks_per_tick == 0 {
            return Err(ScriptHostError::Execution { phase: ScriptPhase::Chunk, error: ScriptError::InvalidConfig("zero callback capacity") });
        }
        let entry_name = chain.entry.clone();
        let rng = SimScriptRng::from_battle_seed(config.seed);
        let load_limits = config.load_limits();
        let host = HostState::with_goal_handles(chain, config, handles);

        let mut vm = adapter::new_vm(load_limits, Box::new(rng.clone()))
            .map_err(|error| ScriptHostError::Execution {
                phase: ScriptPhase::Chunk,
                error,
            })?;
        vm.reserve_userdata(512).map_err(|error| ScriptHostError::Execution {phase:ScriptPhase::Chunk,error})?;
        host_api::register_all(&mut vm).map_err(|error| ScriptHostError::Execution {
            phase: ScriptPhase::Chunk,
            error,
        })?;
        // These entry points live for the VM's lifetime and are private to the host.
        let mut root_entry = |function| -> Result<_, ScriptHostError> {
            let init_error = |error| ScriptHostError::Execution { phase: ScriptPhase::Chunk, error };
            let (id, _root) = vm.create_rooted_native_fn(function).map_err(init_error)?;
            Ok(id)
        };
        let load_trampoline = root_entry(events::load_trampoline)?;
        let post_load_trampoline = root_entry(events::post_load_trampoline)?;
        let tick_trampoline = root_entry(events::tick_dispatch_trampoline)?;

        Ok(Self {
            vm,
            host,
            rng,
            entry_name,
            menu_title: None,
            load_trampoline,
            post_load_trampoline,
            tick_trampoline,
            call_results: Vec::with_capacity(16),
            telemetry: None,
            #[cfg(test)]
            inspector: None,
        })
    }

    /// Attach a zero-allocation [`ScriptTelemetryHandle`] updated in place on each tick.
    pub fn attach_telemetry(&mut self) -> ScriptTelemetryHandle {
        if let Some(ref existing) = self.telemetry {
            return existing.clone();
        }
        let initial = self
            .host
            .build_live_telemetry(self.rng.state(), self.rng.draw_count());
        let handle = ScriptTelemetryHandle {
            inner: Rc::new(Cell::new(initial)),
        };
        self.telemetry = Some(handle.clone());
        handle
    }

    /// Attach an opt-in rich [`ScriptInspector`] for tests/diagnostics.
    /// Not attached by default so steady-state production ticks allocate nothing.
    #[cfg(test)]
    pub fn attach_inspector(&mut self) -> ScriptInspector {
        if let Some(ref existing) = self.inspector {
            return existing.clone();
        }
        let snapshot = self
            .host
            .build_display_snapshot(self.rng.state(), self.rng.draw_count());
        let insp = ScriptInspector {
            inner: Rc::new(RefCell::new(snapshot)),
        };
        self.inspector = Some(insp.clone());
        insp
    }

    pub fn setup_failure(&self) -> Option<&str> {
        self.host.setup_failure.as_deref()
    }

    pub(crate) fn startup_message(title: Option<&str>) -> String {
        let label = title.unwrap_or("The selected mission");
        format!("{label} could not start. The battle ended in a draw.")
    }

    fn fail_setup(&mut self, phase: ScriptPhase, error: &ScriptError) {
        self.host.lifecycle = LifecycleState::Faulted;
        self.host.diagnostics.faulted = true;
        self.host.setup_result = None;
        self.host.outcome = Some(BattleOutcome::FaultedDraw);
        self.host.pending_fault_result = Some(crate::battle::rules::events::Outcome::Draw);
        let site = self.host.resolve_site(self.vm.failure_site());
        let location = site.and_then(|s| self.host.chunks.get(s.chunk as usize)
            .map(|chunk| format!("{} function {} instruction {}", chunk.name, s.function, s.instruction)))
            .unwrap_or_else(|| format!("{} {phase:?} entry", self.entry_name));
        let detail = format!("mission {:?} failed in {phase:?} at {location}: {:?}", self.entry_name, diagnostic_message(error));
        eprintln!("{detail}");
        self.host.diagnostics.record_error(self.host.tick, phase, None, events::classify_error(error), detail);
        self.host.setup_failure = Some(Self::startup_message(self.menu_title.as_deref()));
        self.sync_outputs();
    }

    /// Execute the load phase: top-level entry chunk (with `ScriptCB_DoFile`),
    /// `ScriptPreInit()` if defined, and `ScriptInit()` if defined, all sharing
    /// a single `load_instruction_budget`.
    ///
    /// On failure, transitions to terminal [`LifecycleState::Faulted`] so a
    /// half-initialized VM can never be silently retried.
    pub fn run_load(&mut self) -> Result<&LoadSetup, ScriptHostError> {
        match self.host.lifecycle {
            LifecycleState::Loaded | LifecycleState::Active => return Ok(&self.host.setup),
            LifecycleState::Faulted => return Err(ScriptHostError::Faulted),
            LifecycleState::Cold => {}
        }

        if self
            .host
            .lookup_chunk_bytes(self.entry_name.as_bytes())
            .is_none()
        {
            self.fail_setup(ScriptPhase::Chunk, &ScriptError::runtime("entry chunk is missing"));
            return Err(ScriptHostError::MissingEntry(self.entry_name.clone()));
        }

        let entry_sid = self.vm.intern_bytes(self.entry_name.as_bytes()).map_err(|error| {
            self.fail_setup(ScriptPhase::Chunk, &error);
            ScriptHostError::Execution {
                phase: ScriptPhase::Chunk,
                error,
            }
        })?;
        let res = self.vm.call_into(
            &mut self.host,
            ScriptValue::NativeFn(self.load_trampoline),
            &[ScriptValue::String(entry_sid)],
            &mut self.call_results,
        );
        match res {
            Ok(()) => {
                self.host.lifecycle = LifecycleState::Loaded;
                self.sync_outputs();
                Ok(&self.host.setup)
            }
            Err(error) => {
                let phase = self.host.phase;
                self.fail_setup(phase, &error);
                Err(ScriptHostError::Execution { phase, error })
            }
        }
    }

    /// Execute `ScriptPostLoad()` inside an active `RuleActions` scope.
    pub(crate) fn run_post_load_in_scope(&mut self) -> Result<(), ScriptHostError> {
        match self.host.lifecycle {
            LifecycleState::Active => return Ok(()),
            LifecycleState::Faulted => return Err(ScriptHostError::Faulted),
            LifecycleState::Cold => {
                self.run_load()?;
            }
            LifecycleState::Loaded => {}
        }
        let remaining = self.host.config.load_instruction_budget.saturating_sub(self.vm.work_used());
        self.vm.set_instruction_budget(remaining);
        let res = self.vm.call_into(
            &mut self.host,
            ScriptValue::NativeFn(self.post_load_trampoline),
            &[],
            &mut self.call_results,
        );
        match res {
            Ok(()) => {
                self.host.lifecycle = LifecycleState::Active;
                self.sync_outputs();
                Ok(())
            }
            Err(error) => {
                self.fail_setup(ScriptPhase::PostLoad, &error);
                Err(ScriptHostError::Execution {
                    phase: ScriptPhase::PostLoad,
                    error,
                })
            }
        }
    }

    /// Read-only access to the host state.
    pub fn host(&self) -> &HostState {
        &self.host
    }

    /// Mutable access to the host state.
    pub fn host_mut(&mut self) -> &mut HostState {
        &mut self.host
    }

    /// Fixed-width deterministic digest over all authoritative host state.
    pub fn authoritative_digest(&self) -> Wide {
        self.host.execution_digest(self.vm.state_digest(), self.rng_state())
    }

    /// On-demand display/inspection snapshot (constructed outside hot ticks).
    pub fn display_snapshot(&self) -> ScriptDisplaySnapshot {
        self.host
            .build_display_snapshot(self.rng.state(), self.rng.draw_count())
    }

    /// Deterministic RNG state and draw count.
    pub fn rng_state(&self) -> (u32, u32) {
        (self.rng.state(), self.rng.draw_count())
    }

    pub(crate) fn sync_outputs(&mut self) {
        if let Some(ref handle) = self.telemetry {
            let live = self
                .host
                .build_live_telemetry(self.rng.state(), self.rng.draw_count());
            handle.inner.set(live);
        }
        #[cfg(test)]
        if let Some(ref insp) = self.inspector {
            *insp.inner.borrow_mut() = self
                .host
                .build_display_snapshot(self.rng.state(), self.rng.draw_count());
        }
    }
}

/// Apply load-time mission declarations (`ScriptInit`) to `world` before
/// spawning begins: installs team names, unit counts/capacities, class rosters,
/// `TeamDesc` rows, initial reinforcement counts (including `-1.0` for
/// unlimited tickets), and spawn wave/slot delays on the canonical store.
#[cfg(test)]
fn apply_load_setup_to_world(setup: &LoadSetup, world: &mut World) {
    use crate::battle::spawning::{make_teams, CLASS_CAPACITY, TEAM_COUNT};
    use crate::gen::{SpawnClass, TeamDesc};

    if world.sim.store.spawning.teams.len() < TEAM_COUNT {
        let default_cap = setup
            .teams
            .values()
            .filter_map(|t| t.unit_count)
            .max()
            .unwrap_or(16) as f64;
        world.sim.store.spawning.teams = make_teams(default_cap);
    }

    for (&team, team_setup) in &setup.teams {
        if team < 1 || (team as usize) >= TEAM_COUNT {
            continue;
        }
        let team_idx = team as usize;
        let team_f = f64::from(team);
        let reinf_val = team_setup.initial_reinforcements.map(|reinf| match reinf {
            Reinforcements::Finite(n) => f64::from(n),
            Reinforcements::Unlimited => -1.0,
        });
        if let Some(val) = reinf_val {
            world.sim.store.battle.reinforcements.set(team_f, val);
        }

        let desc = TeamDesc {
            units: team_setup.unit_count.map(f64::from),
            reinforcements: reinf_val,
            bleed: None,
        };
        world.sim.store.battle.teams.set(team_f, desc);

        let name_id = team_setup
            .name
            .as_deref()
            .map(|n| world.sim.strings.intern_str(n));
        let mut classes = Vec::with_capacity(team_setup.classes.len().min(CLASS_CAPACITY));
        for cls in team_setup.classes.iter().take(CLASS_CAPACITY) {
            classes.push(SpawnClass {
                name: world.sim.strings.intern_str(&cls.class_name),
                target: f64::from(cls.min_units.max(0)),
                hero: false,
            });
        }

        let sp_team = &mut world.sim.store.spawning.teams[team_idx];
        if let Some(id) = name_id {
            sp_team.name = Some(id);
        }
        if let Some(count) = team_setup.unit_count {
            sp_team.unit_count = f64::from(count);
            sp_team.capacity = f64::from(count);
        }
        if let Some(val) = reinf_val {
            sp_team.tickets = val;
        }
        if !classes.is_empty() {
            sp_team.classes = classes;
        }
    }

    if let Some(wave) = setup.spawn_wave_delay {
        if wave.is_finite() && wave >= 0.0 {
            world.sim.store.spawning.a = wave;
        }
    }
    if let Some(slot) = setup.spawn_slot_delay {
        if slot.is_finite() && slot >= 0.0 {
            world.sim.store.spawning.b = slot;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
