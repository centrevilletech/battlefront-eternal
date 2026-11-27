//! Fixed-point event and timer delivery for [`super::ScriptRules`] through the
//! `EventRules` trait and `RuleActions` port.
//!
//! All callbacks inside a tick execute inside a single outer native trampoline
//! so the entire tick shares one VM instruction budget and one native-work
//! budget. Retained pending events and in-flight listener cursors ensure due
//! timers and queued events are never silently dropped across ticks.

use bfe_wide::Wide;
use crate::battle::rules::events::{
    Event, EventQueue, EventRules, ObjectHandle, Outcome, Reinforcements, RuleActions,
};
use crate::battle::spawning::TEAM_COUNT;
use crate::battle::rules::{awards::{Credit, Destroyed, SystemTransition}, objects::key};

use super::adapter::{
    ascii_bytes_eq_ignore_case, enter_actions_scope, with_actions, ScriptCallCtx, ScriptError,
    ScriptValue,
};
use super::state::{
    ActiveDeathContext, BattleOutcome, DiagnosticKind, HostState, InFlightEvent, LifecycleState,
    ListenerFamily, PendingEvent, ScriptPhase, OBJECT_USERDATA_TAG,
    TRANSIENT_ATTRIBUTED_KILLER_HANDLE,
};
use super::ScriptRules;

/// Ensure a generation/life-aware authoritative `ObjectHandle` has a cached,
/// GC-rooted userdata handle in the VM.
fn ensure_object_userdata(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    handle: ObjectHandle,
) -> Result<ScriptValue, ScriptError> {
    let Some(binding_id) = ctx.host_mut().bind_object_handle(handle.clone()) else {
        return Err(ScriptError::ResourceLimit("mission object bindings"));
    };
    let (vm, host) = ctx.split();
    let binding = host.object_binding_mut(binding_id).unwrap();
    if binding.handle == handle {
        if let Some(r) = binding.userdata_ref {
            return vm.get_ref(r).ok_or(ScriptError::InvalidHandle("object reference"));
        }
    }
    if let Some(old) = binding.userdata_ref.take() { vm.release_ref(old); }
    let life = match &handle { ObjectHandle::Prop(h) => Wide::from_f64_bits(h.life), ObjectHandle::Vehicle {life,..} => Wide::from_f64_bits(*life), _ => Wide::ZERO };
    binding.handle = handle;
    let ud = vm.create_userdata(OBJECT_USERDATA_TAG + binding_id, life, None)?;
    let val = ScriptValue::Userdata(ud);
    let vref = ctx.vm_mut().create_ref(val)?;
    if let Some(binding) = ctx.host_mut().object_binding_mut(binding_id) {
        binding.userdata_ref = Some(vref);
    }
    Ok(val)
}

/// Outer native trampoline invoked once per load phase so chunk execution,
/// `ScriptPreInit`, and `ScriptInit` share a single VM instruction budget.
pub(crate) fn load_trampoline(ctx: &mut ScriptCallCtx<'_, HostState>) -> Result<(), ScriptError> {
    let (canonical, chunk_idx) = {
        let entry = ctx.arg(0);
        let ScriptValue::String(sid) = entry else {
            return Err(ScriptError::runtime("missing entry script name"));
        };
        let req = ctx.vm().string_bytes(sid).ok_or(ScriptError::InvalidHandle("string"))?;
        let mut found = None;
        for (i, c) in ctx.host().chunks.iter().enumerate() {
            if ascii_bytes_eq_ignore_case(c.name.as_bytes(), req) {
                found = Some((c.name.clone(), i));
                break;
            }
        }
        let Some((name, idx)) = found else {
            return Err(ScriptError::runtime("missing entry script in ScriptChain"));
        };
        (name, idx)
    };

    ctx.host_mut().setup.loaded_scripts.push(canonical);
    let mut out = Vec::new();

    // 1. Top-level entry chunk (including any nested ScriptCB_DoFile calls).
    ctx.host_mut().phase = ScriptPhase::Chunk;
    let (vm, host) = ctx.split();
    let entry_cl = host.load_chunk(vm, chunk_idx)?;
    ctx.call_into(ScriptValue::Closure(entry_cl), &[], &mut out)?;

    // 2. Optional ScriptPreInit
    ctx.host_mut().phase = ScriptPhase::PreInit;
    let pre_init = ctx.vm_mut().get_global("ScriptPreInit")?;
    if pre_init.is_function() {
        ctx.call_into(pre_init, &[], &mut out)?;
    }

    // 3. Optional ScriptInit
    ctx.host_mut().phase = ScriptPhase::Init;
    let init = ctx.vm_mut().get_global("ScriptInit")?;
    if init.is_function() {
        ctx.call_into(init, &[], &mut out)?;
    }
    Ok(())
}

/// Outer native trampoline for `ScriptPostLoad` (shares `load_instruction_budget`).
pub(crate) fn post_load_trampoline(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    ctx.host_mut().phase = ScriptPhase::PostLoad;
    let func = ctx.vm_mut().get_global("ScriptPostLoad")?;
    if func.is_function() {
        let mut out = Vec::new();
        ctx.call_into(func, &[], &mut out)?;
    }
    Ok(())
}

/// Outer native trampoline invoked ONCE per tick so all Lua callbacks in the
/// tick share one VM instruction budget and one native-work budget.
pub(crate) fn tick_dispatch_trampoline(
    ctx: &mut ScriptCallCtx<'_, HostState>,
) -> Result<(), ScriptError> {
    #[cfg(test)]
    super::tests::acceptance::enter_vm(ctx.host().tick);
    let mut results = std::mem::take(&mut ctx.host_mut().callback_results);
    let outcome = dispatch_pending(ctx, &mut results);
    results.clear();
    ctx.host_mut().callback_results = results;
    #[cfg(test)]
    super::tests::acceptance::leave_vm(ctx.host().tick);
    outcome
}

fn dispatch_pending(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    call_out: &mut Vec<ScriptValue>,
) -> Result<(), ScriptError> {
    loop {
        if ctx.host().callbacks_this_tick >= ctx.host().config.max_callbacks_per_tick {
            if ctx.host().in_flight.is_some() || !ctx.host().pending_events.is_empty() {
                let tick = ctx.host().tick;
                let cap = ctx.host().config.max_callbacks_per_tick;
                ctx.host_mut().diagnostics.record_error(
                    tick,
                    ScriptPhase::Tick,
                    None,
                    DiagnosticKind::CallbackCapReached,
                    format!("tick callback cap ({cap}) reached; retaining pending events"),
                );
            }
            return Ok(());
        }

        let Some(mut current) = ctx.host_mut().next_delivery() else { return Ok(()) };

        match dispatch_single_event(ctx, &mut current, call_out) {
            Ok(true) => {}
            Ok(false) => {
                // Callback cap hit mid-event: retain exact listener cursor for next tick.
                ctx.host_mut().in_flight = Some(current);
                return Ok(());
            }
            Err(limit_err) => {
                // Preserve the cursor past the attempted callback and its committed effects.
                ctx.host_mut().in_flight = Some(current);
                return Err(limit_err);
            }
        }
    }
}

/// Dispatch listeners for `current` starting at `current.next_listener_id`.
/// Returns `Ok(true)` when all matching listeners finished, `Ok(false)` when
/// interrupted by callback cap (caller retains `current`), or `Err(limit_err)`
/// when an uncatchable VM resource limit interrupted execution.
fn dispatch_single_event(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    current: &mut InFlightEvent,
    call_out: &mut Vec<ScriptValue>,
) -> Result<bool, ScriptError> {
    let query = ListenerQuery::for_event(&current.event, ctx.host());
    match current.event.clone() {
        PendingEvent::CharacterDeath {
            victim_ref,
            victim_team,
            victim_human,
            killer_ref,
            killer_team,
            killer_human, ..
        } => {
            let Some((victim_h, resolved_victim_human)) =
                ctx.host_mut()
                    .ensure_character(victim_ref, victim_team, victim_human)
            else {
                return Err(ScriptError::ResourceLimit("mission characters"));
            };
            let (killer_h, killer_val, resolved_killer_human) = match killer_ref {
                Some(kr) => {
                    let kt = killer_team.unwrap_or(0);
                    match ctx.host_mut().ensure_character(kr, kt, killer_human) {
                        Some((h, kh)) => (Some(h), ScriptValue::Number(h as f32), kh),
                        None => (None, ScriptValue::Nil, false),
                    }
                }
                None => {
                    if let Some(kt) = killer_team {
                        if kt > 0 {
                            (
                                Some(TRANSIENT_ATTRIBUTED_KILLER_HANDLE),
                                ScriptValue::Number(TRANSIENT_ATTRIBUTED_KILLER_HANDLE as f32),
                                false,
                            )
                        } else {
                            (None, ScriptValue::Nil, false)
                        }
                    } else {
                        (None, ScriptValue::Nil, false)
                    }
                }
            };
            ctx.host_mut().active_death = Some(ActiveDeathContext {
                victim_handle: victim_h,
                victim_team,
                victim_human: resolved_victim_human,
                killer_handle: killer_h,
                killer_team,
                killer_human: resolved_killer_human,
            });
            let res = dispatch_family(
                ctx,
                query,
                &[ScriptValue::Number(victim_h as f32), killer_val],
                &mut current.next_listener_id,
                call_out,
            );
            ctx.host_mut().active_death = None;
            res
        }
        PendingEvent::TicketsChanged { team, count, .. } => {
            // If the team has since switched to unlimited, skip stale finite notification.
            if let Some(Ok(Reinforcements::Unlimited)) =
                with_actions(|a| a.reinforcements(f64::from(team)))
            {
                return Ok(true);
            }
            dispatch_family(
                ctx,
                query,
                &[
                    ScriptValue::Number(team as f32),
                    ScriptValue::Number(count as f32),
                ],
                &mut current.next_listener_id,
                call_out,
            )
        }
        PendingEvent::TeamPointsChanged { team, points, .. } => dispatch_family(
            ctx,
            query,
            &[
                ScriptValue::Number(team as f32),
                ScriptValue::Number(points as f32),
            ],
            &mut current.next_listener_id,
            call_out,
        ),
        PendingEvent::TimerElapsed { timer_id, arm_gen } => {
            if !ctx.host_mut().begin_timer_delivery(timer_id, arm_gen, current.next_listener_id) {
                return Ok(true);
            }
            dispatch_family(
                ctx,
                query,
                &[ScriptValue::Number(timer_id as f32)],
                &mut current.next_listener_id,
                call_out,
            )
        }
        PendingEvent::NamedObject { object, family, team, credit, .. } => {
            if query.next(ctx.host(), current.next_listener_id).is_none() { return Ok(true); }
            let value = ensure_object_userdata(ctx, object.clone())?;
            let killer = credit.and_then(|c| match c.character {
                Some(r) => ctx.host_mut().ensure_character(r, c.team, Some(c.human)).map(|(id, _)| id),
                None => Some(TRANSIENT_ATTRIBUTED_KILLER_HANDLE),
            });
            let args = [value, killer.map_or(ScriptValue::Nil, |id| ScriptValue::Number(id as f32))];
            ctx.host_mut().active_object = Some((object, team, credit, killer));
            ctx.host_mut().syncing_damage = family == ListenerFamily::ObjectDamage;
            let result = dispatch_family(ctx, query, &args, &mut current.next_listener_id, call_out);
            ctx.host_mut().syncing_damage = false;
            ctx.host_mut().active_object = None;
            result
        }
    }
}

#[derive(Clone, Copy)]
struct ListenerQuery {
    family: ListenerFamily,
    object: Option<(u8, u32, u32)>,
    timer: u32,
    sequence: u32,
    victory_event: bool,
}

impl ListenerQuery {
    fn for_event(event: &PendingEvent, host: &HostState) -> Self {
        let (family, object, timer, sequence) = match event {
            PendingEvent::CharacterDeath { .. } => (ListenerFamily::CharacterDeath, None, 0, 0),
            PendingEvent::TicketsChanged { seq, .. } => (ListenerFamily::TicketCountChange, None, 0, *seq),
            PendingEvent::TeamPointsChanged { seq, .. } => (ListenerFamily::TeamPointsChange, None, 0, *seq),
            PendingEvent::TimerElapsed { timer_id, .. } => (ListenerFamily::TimerElapse, None, *timer_id, 0),
            PendingEvent::NamedObject { family, object, .. } => (*family, Some(key(object)), 0, 0),
        };
        let primary = |team| team == host.config.own_team || team == host.config.foe_team;
        let victory_event = match event {
            PendingEvent::TeamPointsChanged { team, points, .. } => primary(*team)
                && host.cached_score_limit.is_some_and(|target| target > 0 && *points >= target),
            PendingEvent::TicketsChanged { team, count: 0, .. } => primary(*team),
            _ => false,
        };
        Self { family, object, timer, sequence, victory_event }
    }

    fn next(self, host: &HostState, cursor: u32) -> Option<&super::state::ListenerSlot> {
        let bucket = host.listener_bucket(self.family);
        let start = bucket.partition_point(|slot| (slot.object, slot.id) < (self.object, cursor));
        bucket[start..].iter().take_while(|slot| slot.object == self.object).find(|slot|
            (self.sequence == 0 || self.sequence > slot.min_seq) && (self.timer == 0 || self.timer == slot.timer_id))
    }
}

struct CallbackFailure {
    kind: DiagnosticKind,
    site: Option<super::state::ScriptSite>,
    message: String,
    strike: bool,
}

impl HostState {
    fn next_delivery(&mut self) -> Option<InFlightEvent> {
        self.in_flight.take().or_else(|| {
            let event = self.pop_delivery()?;
            if let PendingEvent::CharacterDeath { death: Some(death), .. } = &event {
                self.deaths.push(*death);
            }
            Some(InFlightEvent { event, next_listener_id: 0 })
        })
    }

    pub(super) fn begin_timer_delivery(&mut self, id: u32, generation: u32, cursor: u32) -> bool {
        let Some(timer) = self.timer_mut(id) else { return false };
        if timer.arm_gen != generation || (cursor == 0 && !timer.running) { return false; }
        if cursor == 0 { timer.remaining = 0.0; timer.running = false; }
        true
    }

    fn registered_listener(&mut self, query: ListenerQuery, id: u32) -> Option<&mut super::state::ListenerSlot> {
        // A callback can release its own registration.
        let bucket = self.listener_bucket_mut(query.family);
        let index = bucket.binary_search_by_key(&(query.object, id), |s| (s.object, s.id)).ok()?;
        Some(&mut bucket[index])
    }

    fn report_dropped_events(&mut self, dropped: u32) {
        let total = self.diagnostics.queue_overflows.saturating_add(dropped);
        if self.diagnostics.queue_overflows == 0 {
            let deadline = *self.degraded_deadline.get_or_insert_with(|| self.tick.saturating_add(36_000));
            let message = format!("Mission event queue full at tick {}. Dropped {dropped} incoming events. Reduce listener work or event frequency. Standings decide at tick {deadline} if still open", self.tick);
            eprintln!("{message}");
            self.diagnostics.record_error(self.tick, ScriptPhase::Tick, None, DiagnosticKind::EventQueueOverflow, message);
        }
        self.diagnostics.queue_overflows = total;
        if let Some(entry) = self.diagnostics.errors.iter_mut().find(|d| d.kind == DiagnosticKind::EventQueueOverflow) {
            entry.occurrences = total;
        }
    }

    fn mark_degraded(&mut self, id: u32, reason: &str) {
        if self.degraded_deadline.is_none() && self.outcome.is_none() {
            let deadline = self.tick.saturating_add(36_000);
            self.degraded_deadline = Some(deadline);
            let message = format!("listener {id} {reason} at tick {}. Standings decide at tick {deadline} if the mission is still open", self.tick);
            eprintln!("Mission decision deadline: {message}");
            self.diagnostics.record_error(self.tick, ScriptPhase::Tick, Some(id), DiagnosticKind::DecisionDeadline, message);
        }
    }

    fn failed_invocation(&mut self, query: ListenerQuery, id: u32, previous: u8, failure: CallbackFailure) -> Option<super::adapter::ScriptValueRef> {
        let CallbackFailure { kind, site, message, strike } = failure;
        let memory_refusal = kind == DiagnosticKind::MemoryLimit && strike;
        if memory_refusal || matches!(kind, DiagnosticKind::RuntimeError | DiagnosticKind::StackOverflow | DiagnosticKind::InstructionLimit) {
            if query.family == ListenerFamily::TimerElapse {
                self.mark_degraded(id, "abandoned its timer callback");
            } else if query.victory_event || self.attempted_result {
                self.mark_degraded(id, "abandoned its victory callback");
            }
        }
        let chunk = site.and_then(|s| self.chunks.get(s.chunk as usize)).map_or("mission", |c| c.name.as_str());
        self.diagnostics.callback_error(self.tick, id, kind, message, site, chunk);
        if kind != DiagnosticKind::InstructionLimit && !memory_refusal {
            if let Some(slot) = self.registered_listener(query, id) { slot.budget_streak = 0; }
            return None;
        }
        self.diagnostics.callback_budget_refusals = self.diagnostics.callback_budget_refusals.saturating_add(1);
        let streak = if strike { previous + 1 } else { 0 };
        if streak < 3 {
            if let Some(slot) = self.registered_listener(query, id) {
                slot.budget_streak = streak;
                slot.needs_fresh_tick = !memory_refusal;
            }
            return None;
        }
        let root = self.remove_listener_of_family(id, Some(query.family)).ok()??;
        let prior_memory_error = self.diagnostics.errors.iter().any(|error|
            error.listener_id == Some(id) && error.kind == DiagnosticKind::MemoryLimit);
        let (reason, detail) = if memory_refusal {
            ("was disabled after three consecutive resource refusals, the last at the memory limit", "listener reached a resource limit on three consecutive invocations, ending at the memory limit")
        } else if prior_memory_error {
            ("was disabled after three consecutive resource refusals", "listener reached a resource limit on three consecutive invocations")
        } else {
            ("was disabled after three work refusals", "listener exhausted its allowance on three consecutive invocations")
        };
        eprintln!("Mission listener {id} {reason} at tick {}", self.tick);
        self.mark_degraded(id, reason);
        self.diagnostics.callbacks_dropped = self.diagnostics.callbacks_dropped.saturating_add(1);
        self.diagnostics.record_error(self.tick, ScriptPhase::Tick, Some(id), DiagnosticKind::ListenerDisabled,
            detail.into());
        Some(root)
    }
}

fn dispatch_family(
    ctx: &mut ScriptCallCtx<'_, HostState>,
    query: ListenerQuery,
    args: &[ScriptValue],
    next_listener_id: &mut u32,
    call_out: &mut Vec<ScriptValue>,
) -> Result<bool, ScriptError> {
    let generation = ctx.host().timer(query.timer).map(|t| t.arm_gen);
    loop {
        if query.timer != 0 && ctx.host().timer(query.timer).map(|t| t.arm_gen) != generation {
            return Ok(true);
        }
        let Some(slot) = query.next(ctx.host(), *next_listener_id) else { return Ok(true) };
        let (listener_id, vref, streak) = (slot.id, slot.callback, slot.budget_streak);
        let host = ctx.host();
        let spent_work = ctx.vm().work_used();
        let spent_native = host.native_ops_this_tick;
        let fresh_tick = host.callbacks_this_tick == 0;
        let work_budget = host.config.tick_instruction_budget;
        let native_budget = host.config.tick_native_ops_budget;
        let learned = slot.peak_work != 0;
        let (work_needed, native_needed) = if learned {
            (slot.peak_work, slot.peak_native)
        } else {
            // Leave headroom on first observation, including for a newly registered scorer.
            (work_budget.div_ceil(2), native_budget.div_ceil(2))
        };
        let fits = !slot.needs_fresh_tick && work_needed <= work_budget.saturating_sub(spent_work)
            && native_needed <= native_budget.saturating_sub(spent_native);
        if host.callbacks_this_tick >= host.config.max_callbacks_per_tick || (!fresh_tick && !fits) {
            *next_listener_id = listener_id;
            return Ok(false);
        }

        let (peak_work, peak_native, needs_fresh_tick) = (slot.peak_work, slot.peak_native, slot.needs_fresh_tick);
        // Advance cursor before invoking so even if this listener errors or hits
        // a limit, we will not re-invoke the same listener on retry.
        *next_listener_id = listener_id.saturating_add(1);

        let func = ctx.vm().get_ref(vref).ok_or(ScriptError::InvalidHandle("callback reference"))?;
        if func.is_nil() {
            continue;
        }

        ctx.host_mut().callbacks_this_tick += 1;
        ctx.host_mut().diagnostics.callbacks_executed = ctx
            .host()
            .diagnostics
            .callbacks_executed
            .saturating_add(1);

        ctx.host_mut().attempted_result = false;
        match ctx.pcall_into(func, args, call_out) {
            Ok(Ok(())) => {
                call_out.clear();
                let work = ctx.vm().work_used().saturating_sub(spent_work);
                let native = ctx.host().native_ops_this_tick.saturating_sub(spent_native);
                if work > peak_work || native > peak_native || needs_fresh_tick || streak != 0 {
                    if let Some(slot) = ctx.host_mut().registered_listener(query, listener_id) {
                        slot.peak_work = peak_work.max(work);
                        slot.peak_native = peak_native.max(native);
                        slot.needs_fresh_tick = false;
                        slot.budget_streak = 0;
                    }
                }
            }
            Ok(Err(value)) => {
                let message = match value {
                    ScriptValue::String(id) => {
                        let bytes = ctx.vm().string_bytes(id).ok_or(ScriptError::InvalidHandle("error string"))?;
                        String::from_utf8_lossy(&bytes[..bytes.len().min(super::state::MAX_DIAGNOSTIC_BYTES)]).into_owned()
                    }
                    _ => format!("Lua raised a {} value", value.type_name()),
                };
                let site = ctx.host().resolve_site(ctx.vm().failure_site());
                let failure = CallbackFailure { kind: DiagnosticKind::RuntimeError, site, message, strike: false };
                ctx.host_mut().failed_invocation(query, listener_id, streak, failure);
                call_out.clear();
            }
            Err(error) => {
                let kind = classify_error(&error);
                let (vm, host) = ctx.split();
                let memory_refusal = matches!(error, ScriptError::MemoryLimit { .. });
                let own_work_exhaustion = if host.native_ops_this_tick > native_budget {
                    spent_native == 0
                } else {
                    fresh_tick
                };
                let site = host.resolve_site(vm.failure_site());
                let failure = CallbackFailure { kind, site, message: error.to_string(),
                    strike: memory_refusal || (kind == DiagnosticKind::InstructionLimit && own_work_exhaustion) };
                if let Some(root) = host.failed_invocation(query, listener_id, streak, failure) {
                    vm.release_ref(root);
                }
                if matches!(kind, DiagnosticKind::StackOverflow | DiagnosticKind::RuntimeError) {
                    // The protected call has already unwound to our trampoline.
                    call_out.clear();
                    continue;
                }
                // A spent allowance ends this tick's dispatch. At the memory limit the
                // tick's collection is spent too, so later listeners wait for the next tick.
                host.callback_error_recorded = true;
                return Err(error);
            }
        }
    }
}

pub(crate) fn classify_error(err: &ScriptError) -> DiagnosticKind {
    match err {
        ScriptError::Runtime(_) => DiagnosticKind::RuntimeError,
        ScriptError::InstructionLimit { .. } => DiagnosticKind::InstructionLimit,
        ScriptError::StackOverflow { .. } => DiagnosticKind::StackOverflow,
        ScriptError::MemoryLimit { .. } => DiagnosticKind::MemoryLimit,
        ScriptError::InvalidHandle(_) | ScriptError::InvalidConfig(_) => DiagnosticKind::InternalFault,
        ScriptError::ResourceLimit("mission characters" | "mission object bindings" | "userdata reservation") => DiagnosticKind::MemoryLimit,
        ScriptError::ResourceLimit(_) => DiagnosticKind::RuntimeError,
        ScriptError::Load(_) => DiagnosticKind::LoadError,
    }
}

fn standings(a: (f64, u32), b: (f64, u32)) -> Outcome {
    match a.1.cmp(&b.1) {
        std::cmp::Ordering::Greater => Outcome::Winner(a.0),
        std::cmp::Ordering::Less => Outcome::Winner(b.0),
        std::cmp::Ordering::Equal => Outcome::Draw,
    }
}

impl ScriptRules {
    /// Synchronize authoritative scores, reinforcements, and score limit from
    /// `RuleActions` into the host's zero-allocation display mirror.
    fn sync_authoritative_mirrors(&mut self, actions: &mut dyn RuleActions) {
        self.host.cached_score_limit = actions.score_limit();
        for team in 1..TEAM_COUNT {
            if let Ok(pts) = actions.team_points(team as f64) {
                self.host.cached_scores[team] = pts;
            }
            if let Ok(reinf) = actions.reinforcements(team as f64) {
                self.host.cached_reinforcements[team] = match reinf {
                    Reinforcements::Unlimited => -1,
                    Reinforcements::Finite(n) => n.min(i32::MAX as u32) as i32,
                };
            }
        }
    }

    fn publish_actions(&mut self, actions: &mut dyn RuleActions) {
        let (commands, refused) = actions.command_usage();
        let diagnostics = &mut self.host.diagnostics;
        diagnostics.peak_commands = diagnostics.peak_commands.max(commands);
        if refused != 0 {
            if diagnostics.refused_commands == 0 {
                eprintln!("Mission command limit reached at tick {}. Accepted {commands} and refused {refused}", self.host.tick);
                diagnostics.record_error(self.host.tick, ScriptPhase::Tick, None,
                    DiagnosticKind::CommandLimit, "rules tick command limit reached".into());
            }
            diagnostics.refused_commands = diagnostics.refused_commands.saturating_add(refused);
        }
        self.sync_authoritative_mirrors(actions);
        self.sync_outputs();
    }

    /// Apply any initial reinforcement counts or scores recorded during
    /// `ScriptInit` that are not already reflected in `RuleActions`, BEFORE
    /// `ScriptPostLoad` registers any listeners.
    fn apply_init_setup_before_post_load(&mut self, actions: &mut dyn RuleActions) {
        for (&team, setup) in &self.host.setup.teams {
            let team = f64::from(team);
            if let Some(target) = setup.initial_reinforcements {
                if actions.reinforcements(team) != Ok(target) {
                    match actions.set_reinforcements(team, target) {
                        Ok(()) if matches!(target, Reinforcements::Finite(_)) => self.host.emitted_ticket_seq = self.host.emitted_ticket_seq.saturating_add(1),
                        Ok(()) => {}
                        Err(e) => self.host.diagnostics.setup_refusal(self.host.tick, "SetReinforcementCount", e),
                    }
                }
            }
            if let Some(target) = setup.initial_score {
                if actions.team_points(team) != Ok(target) {
                    match actions.set_team_points(team, target) {
                        Ok(()) => self.host.emitted_points_seq = self.host.emitted_points_seq.saturating_add(1),
                        Err(e) => self.host.diagnostics.setup_refusal(self.host.tick, "SetTeamPoints", e),
                    }
                }
            }
        }
        for timer in &mut self.host.timers {
            if timer.alive && timer.running && timer.rate > 0.0
                && !actions.start_timer(timer.rules_id(), timer.remaining / timer.rate) {
                timer.running = false;
                self.host.diagnostics.setup_refusal(self.host.tick, "StartTimer", crate::battle::rules::objects::RuleError::LimitReached);
            }
        }
        self.sync_authoritative_mirrors(actions);
    }

    /// Resolve after dispatch, so a result scripted on the last eligible tick stands.
    fn check_decision_deadlines(&mut self, actions: &mut dyn RuleActions) {
        if self.host.outcome.is_some() || actions.committed_outcome().is_some() { return; }
        let tick = self.host.tick;
        if self.host.target_deadline.is_none() && actions.target_winner().is_some() {
            self.host.target_deadline = Some(tick.saturating_add(1_800));
        }
        let target_due = self.host.target_deadline.is_some_and(|due| tick >= due);
        let degraded_due = self.host.degraded_deadline.is_some_and(|due| tick >= due);
        if !target_due && !degraded_due { return; }
        let [a, b] = self.primary_scores(actions);
        if target_due && !actions.score_limit().is_some_and(|target| a.1.max(b.1) >= target) {
            self.host.target_deadline = None;
            actions.rearm_score_target();
            if !degraded_due { return; }
        }
        let result = standings(a, b);
        if actions.finish(result).is_ok() {
            self.host.outcome = Some(match result {
                Outcome::Winner(team) => BattleOutcome::Victory(team as i32),
                Outcome::Loser(team) => BattleOutcome::Defeat(team as i32),
                Outcome::Draw => BattleOutcome::Draw(Vec::new()),
            });
        }
    }

    fn primary_scores(&self, actions: &dyn RuleActions) -> [(f64, u32); 2] {
        [self.host.config.own_team, self.host.config.foe_team].map(|team| {
            let team = f64::from(team);
            (team, actions.score(team).unwrap_or(0))
        })
    }

    fn abort_battle_faulted(&mut self, actions: &mut dyn RuleActions, reason: &str) {
        if self.host.setup_failure.is_some() {
            if self.host.pending_fault_result.is_some() && actions.finish(Outcome::Draw).is_ok() {
                self.host.pending_fault_result = None;
            }
            return;
        }
        let already_aborted = self.host.diagnostics.errors.iter().any(|e| e.kind == DiagnosticKind::HostAborted);
        self.host.lifecycle = LifecycleState::Faulted;
        self.host.diagnostics.faulted = true;
        self.host.pending_events.clear();
        self.host.pending_score_changes = 0;
        self.host.in_flight = None;
        if !already_aborted {
            self.host.diagnostics.record_error(self.host.tick, self.host.phase, None,
                DiagnosticKind::HostAborted, reason.to_string());
        }
        if self.host.outcome.is_some() { return; }
        let committed = actions.committed_outcome();
        let result = *self.host.pending_fault_result.get_or_insert_with(|| {
            if let Some(result) = committed { return result; }
            if let Some(team) = actions.target_winner() { return Outcome::Winner(team); }
            let a = f64::from(self.host.config.own_team);
            let b = f64::from(self.host.config.foe_team);
            match actions.score(a).unwrap_or(0).cmp(&actions.score(b).unwrap_or(0)) {
                std::cmp::Ordering::Greater => Outcome::Winner(a),
                std::cmp::Ordering::Less => Outcome::Winner(b),
                std::cmp::Ordering::Equal => Outcome::Draw,
            }
        });
        let result = committed.unwrap_or(result);
        if committed.is_some() || actions.finish(result).is_ok() {
            self.host.outcome = Some(match result {
                Outcome::Winner(team) => BattleOutcome::Victory(team as i32),
                Outcome::Loser(team) => BattleOutcome::Defeat(team as i32),
                Outcome::Draw if committed.is_some() => BattleOutcome::Draw(Vec::new()),
                Outcome::Draw => BattleOutcome::FaultedDraw,
            });
        }
    }

    fn enqueue_world_events(&mut self, events: &EventQueue, actions: &mut dyn RuleActions) {
        let mut dropped = events.dropped();
        for id in events.expired_without_delivery() {
            if let Some(timer) = self.host.timer_mut(id.0) {
                timer.running = false;
                timer.remaining = 0.0;
            }
        }
        for ev in events.events() {
            let pending = match ev {
                Event::UnitKilled {
                    victim,
                    victim_team,
                    killer,
                    killer_team,
                    ..
                } => {
                    // Deliver OnCharacterDeath ONLY for character victims.
                    let Some(victim_ref) = *victim else {
                        continue;
                    };
                    let v_team = if victim_team.is_finite() {
                        *victim_team as i32
                    } else {
                        0
                    };
                    let v_human = actions.character_is_human(victim_ref).ok();
                    let k_team = killer_team.filter(|t| t.is_finite()).map(|t| t as i32);
                    let k_human = killer.and_then(|kr| actions.character_is_human(kr).ok());
                    PendingEvent::CharacterDeath {
                        death: None,
                        victim_ref,
                        victim_team: v_team,
                        victim_human: v_human,
                        killer_ref: *killer,
                        killer_team: k_team,
                        killer_human: k_human,
                    }
                }
                Event::MissionFlyerKilled { team, kind, credit, .. } => {
                    self.host.ordinary_award(actions, Some(*credit), *team, *kind);
                    continue;
                }
                Event::MissionSoldierKilled { victim, victim_team, human, hero, credit, death } => {
                    self.host.ordinary_award(actions, *credit, *victim_team, Destroyed::Soldier { human: *human, hero: *hero });
                    PendingEvent::CharacterDeath { death: Some(*death), victim_ref: *victim, victim_team: *victim_team, victim_human: Some(*human),
                        killer_ref: credit.and_then(|c| c.character), killer_team: credit.map(|c| c.team), killer_human: credit.map(|c| c.human) }
                }
                Event::MissionObject { object, transition, team, kind, credit, absorbed } => {
                    let family = match transition {
                        SystemTransition::Damage => ListenerFamily::ObjectDamage,
                        SystemTransition::Respawn => ListenerFamily::ObjectRespawn,
                        SystemTransition::Kill => {
                            self.host.ordinary_award(actions, *credit, *team, *kind);
                            ListenerFamily::ObjectKill
                        }
                    };
                    PendingEvent::NamedObject { object: object.clone(), family, team: *team, credit: *credit, absorbed: *absorbed }
                }
                Event::ObjectKilled { object, killer } => {
                    let team = actions.object_team(object).unwrap_or(0.0) as i32;
                    let credit = killer.and_then(|r| Some(Credit { character: Some(r),
                        team: actions.character_team(r).ok()? as i32, human: actions.character_is_human(r).ok()? }));
                    PendingEvent::NamedObject { object: object.clone(), family: ListenerFamily::ObjectKill, team, credit, absorbed: 0.0 }
                }
                Event::TicketsChanged { team, count } => {
                    self.host.delivered_ticket_seq =
                        self.host.delivered_ticket_seq.saturating_add(1);
                    let t = if team.is_finite() { *team as i32 } else { 0 };
                    PendingEvent::TicketsChanged {
                        team: t,
                        count: *count,
                        seq: self.host.delivered_ticket_seq,
                    }
                }
                Event::TeamPointsChanged { team, points } => {
                    self.host.delivered_points_seq =
                        self.host.delivered_points_seq.saturating_add(1);
                    let t = if team.is_finite() { *team as i32 } else { 0 };
                    PendingEvent::TeamPointsChanged {
                        team: t,
                        points: *points,
                        seq: self.host.delivered_points_seq,
                    }
                }
                Event::TimerElapsed { timer } => {
                    let timer_id = timer.0;
                    let Some(t) = self.host.timer(timer_id) else {
                        continue;
                    };
                    if !t.running {
                        continue;
                    }
                    PendingEvent::TimerElapsed {
                        timer_id,
                        arm_gen: t.arm_gen,
                    }
                }
                Event::PostCaptured { .. }
                | Event::PostNeutralized { .. }
                | Event::PostLost { .. }
                | Event::MobilePostOwner { .. }
                | Event::MobilePostLost { .. }
                | Event::RegionEntered { .. }
                | Event::RegionLeft { .. }
                | Event::FlagChanged { .. } => continue,
            };

            if !self.host.queue_delivery(pending) {
                dropped = dropped.saturating_add(1);
            }
        }
        if dropped != 0 {
            self.host.report_dropped_events(u32::try_from(dropped).unwrap_or(u32::MAX));
        }
    }
}

impl HostState {
    fn ordinary_award(&mut self, actions: &mut dyn RuleActions, credit: Option<Credit>, team: i32, kind: Destroyed) {
        let Some(credit) = credit else { return };
        if !self.match_flags.space_assault_enabled || kind.points() == 0 {
            return;
        }
        match kind {
            Destroyed::EligibleEmptyVehicle if team == credit.team || super::host_api::team_slot(team).is_none() => return,
            Destroyed::SelfPilotedTurret if team == credit.team => return,
            _ => (),
        }
        self.credit_points(actions, credit, team, kind.points());
    }

    pub(super) fn credit_points(&mut self, actions: &mut dyn RuleActions, credit: Credit, victim_team: i32, points: i32) {
        let Some(slot) = super::host_api::team_slot(credit.team) else { return };
        let previous = actions.team_points(f64::from(credit.team)).ok();
        let delta = if victim_team == credit.team { points.saturating_neg() } else { points };
        match actions.award(credit, delta) {
            Ok(next) => {
                self.cached_scores[slot] = next;
                if Some(next) != previous { self.emitted_points_seq = self.emitted_points_seq.saturating_add(1); }
            }
            Err(e) => self.diagnostics.setup_refusal(self.tick, "mission score", e),
        }
    }
}

impl EventRules for ScriptRules {
    fn reserve_delivery(&mut self, batch: usize, commands: usize) -> Option<()> { self.host.reserve_delivery(batch, commands) }
    fn ai_goals(&self) -> Option<&dyn crate::ai::goals::GoalTable> { Some(&self.host) }
    fn script(&self) -> Option<&ScriptRules> { Some(self) }
    #[cfg(test)]
    fn script_mut(&mut self) -> Option<&mut ScriptRules> { Some(self) }

    fn tick(&mut self, dt: f64, events: &EventQueue, actions: &mut dyn RuleActions) {
        self.host.tick = self.host.tick.saturating_add(1);
        self.host.deaths.clear();
        if self.host.lifecycle == LifecycleState::Faulted || self.host.diagnostics.faulted {
            self.abort_battle_faulted(actions, "mission script fault");
            self.publish_actions(actions);
            return;
        }

        let dt_finite = if dt.is_finite() && dt >= 0.0 { dt } else { 0.0 };

        // Run cold load + ScriptPostLoad inside the active RuleActions scope on first tick.
        if self.host.lifecycle == LifecycleState::Cold {
            if let Err(err) = self.run_load() {
                self.abort_battle_faulted(actions, &format!("mission load failed: {err}"));
                self.publish_actions(actions);
                return;
            }
        }
        if self.host.lifecycle == LifecycleState::Loaded {
            self.apply_init_setup_before_post_load(actions);
            let post_res = enter_actions_scope(actions, || self.run_post_load_in_scope());
            if let Err(err) = post_res {
                self.abort_battle_faulted(actions, &format!("ScriptPostLoad failed: {err}"));
                self.publish_actions(actions);
                return;
            }
            if let Some((outcome, recorded)) = self.host.setup_result.take() {
                match actions.finish(outcome) {
                    Ok(()) => self.host.outcome = Some(recorded),
                    Err(e) => self.host.diagnostics.setup_refusal(self.host.tick, "mission result", e),
                }
            }
        }

        self.host.phase = ScriptPhase::Tick;
        self.host.native_ops_this_tick = 0;
        self.host.callbacks_this_tick = 0;
        self.host.callback_error_recorded = false;

        // Pace collection once per rules tick, after setup and before any
        // delivery, quiet ticks included. Loading and post-load collected as in
        // preparation. Each tick's collection and rescues share one balance.
        if let Err(error) = self.vm.begin_tick(self.host.tick) {
            self.host.diagnostics.record_error(self.host.tick, ScriptPhase::Tick, None, DiagnosticKind::InternalFault, error.to_string());
            self.abort_battle_faulted(actions, "mission state is unusable");
            self.publish_actions(actions);
            return;
        }

        // Advance deterministic VO playback clock and display-remaining values of
        // already-running timers (zero allocations; authoritative expiry arrives
        // via Event::TimerElapsed from EventDriven).
        self.host.active_voices.retain_mut(|(_, time)| { *time -= dt_finite; *time > 0.0 });
        let current_tick = self.host.tick;
        for timer in &mut self.host.timers {
            if timer.alive && timer.running && timer.started_tick < current_tick {
                let next = timer.remaining - dt_finite * timer.rate;
                timer.remaining = if next > 0.0 { next } else { 0.0 };
            }
        }

        self.enqueue_world_events(events, actions);

        // Run all pending callbacks for this tick under ONE shared VM instruction
        // budget and ONE shared native-work budget.
        if self.host.in_flight.is_some() || !self.host.pending_events.is_empty() {
            #[cfg(test)]
            let start = super::tests::acceptance::dispatch_start();
            self.vm.set_instruction_budget(self.host.config.tick_instruction_budget);
            let trampoline = self.tick_trampoline;
            let res = enter_actions_scope(actions, || {
                self.vm.call_into(
                    &mut self.host,
                    ScriptValue::NativeFn(trampoline),
                    &[],
                    &mut self.call_results,
                )
            });
            #[cfg(test)]
            super::tests::acceptance::measure_dispatch(start, self.vm.work_used());
            if let Err(error) = res {
                let kind = classify_error(&error);
                if matches!(error, ScriptError::MemoryLimit { .. })
                    || matches!(kind, DiagnosticKind::InstructionLimit | DiagnosticKind::StackOverflow | DiagnosticKind::RuntimeError) {
                    // The dispatcher itself can be refused before any invocation.
                    // Its pending deliveries are still intact. No listener spent that work.
                    if !self.host.callback_error_recorded && !self.host.diagnostics.errors.iter().any(|d| d.listener_id.is_none() && d.kind == kind) {
                        self.host.diagnostics.record_error(self.host.tick, ScriptPhase::Tick, None, kind, error.to_string());
                    }
                } else {
                    if !self.host.callback_error_recorded {
                        self.host.diagnostics.record_error(self.host.tick, ScriptPhase::Tick, None, kind, error.to_string());
                    }
                    self.abort_battle_faulted(actions, "mission state is unusable");
                }
            }
        }

        self.check_decision_deadlines(actions);
        self.publish_actions(actions);
    }
}
