//! Data-driven Battlefront II melee combo state machine for `swbf2` battles.
//!
//! Authority and client prediction run the same fixed 60 Hz deterministic step
//! over validated [`ComboMachine`] definitions loaded from prepared
//! `combos.json` content. Every transcendental or min/max operation routes
//! through `v8math`, hot paths perform zero heap allocations, and `GameFamily::Swbf1`
//! battles bypass this module completely so the 2004 melee path is untouched.

pub mod data;
pub mod deflect;
pub mod powers;
pub mod sweep;

#[cfg(test)]
mod rules_tests;
#[cfg(test)]
mod tests;

use crate::movement::scene::Scene;
use crate::movement::soldier::{swbf2 as move2, Input as MoveInput, Mover};
use crate::movement::ProfileError;
use crate::ordnance::math::{self as om, V3};
use bfe_family::GameFamily;
use deflect::angle_in_range_deg;
use std::fmt;

pub use data::{
    attack_shared_id, pack_attack_key, resolve_binding_clip_duration, zero_fnv1a_lower, AimType,
    AlignMode, AnimatedMoveDef, AnimationMeta, AttackDef, AttackGroupId, BlendTimeTo, BreakRule,
    ButtonCondition, ButtonMode, ClipBindingCandidate, ClipId, ComboButton, ComboLoadError,
    ComboMachine, ComboPosture, ComboStateDef, ConditionGroup, DamageWindow, DeflectAnimDef,
    DeflectDef, EdgeSpec, EnergyRateOverride, GravityOverride, InputLockDef, InputLockMask,
    LoopMode, OffhandAnimDef, OffhandSlot, PostureCondition, PostureMask, ScalarCmp, ScalarOp,
    SoundCue, SoundId, StateAnimLayer, StateDuration, StateId, SyncTypeDef, TimePoint,
    TransitionDef, WeaponMeleeSpec, COMBO_ANIM_FPS, MAX_BLADE_EDGES, NO_STATE, NO_TRANSITION,
};
pub use deflect::{
    classify_direction, compute_aim_basis, reflect_in_ordnance, relative_horizontal_angle_deg,
    select_deflect_animation, select_deflect_direction, try_deflect, try_deflect_with_spread,
    DefenderPose, DeflectAimMode, DeflectDirection, DeflectOutcome, DeflectableKind,
    IncomingProjectile, RedirectAim,
};
pub use powers::{
    execute_force_push, execute_force_push_validated, ForcePushCandidate, ForcePushDef,
    ForcePushHit, ForcePushResult, PowerValidationError, SaberThrowCheckpoint, SaberThrowDef,
    SaberThrowPhase, SaberThrowState, SaberThrowStep, ValidatedForcePushDef,
    ValidatedSaberThrowDef,
};
pub use sweep::{
    build_damage_samples, resolve_melee_hit, resolve_melee_hit_samples, BladeSegment, BladeSweep,
    DamageSample, DamageSampleList, HitHistory, HitListRecord, HitLists, MeleeHitOutcome,
    MeleeTarget, TargetId, ThrowHitHistory, DAMAGE_SAMPLE_BASE_STEP_SECS, DAMAGE_SAMPLE_MAX_TRAVEL_SQ,
    DAMAGE_SAMPLE_MIN_STEP_SECS, HIT_LIST_LINGER_SECS, MAX_ACTIVE_HIT_LISTS, MAX_DAMAGE_SAMPLES,
    MAX_TARGETS_PER_ATTACK,
};

/// Fixed simulation tick duration (60 Hz).
pub const COMBO_FIXED_DT: f64 = 1.0 / 60.0;

/// Small tolerance matching `movement::soldier::swbf2` timer boundaries so
/// integer 30 fps frame boundaries align cleanly on 60 Hz steps.
const TIME_EPS: f64 = 1e-6;

const RAD_TO_DEG: f64 = 180.0 / std::f64::consts::PI;

/// Maximum active attacks emitted in a single tick without allocation.
pub const MAX_ACTIVE_ATTACKS: usize = 8;

/// Maximum timestamped button events retained in [`ButtonEventHistory`].
pub const MAX_BUTTON_EVENTS: usize = 8;

/// Maximum state transitions executed within a single simulation tick.
const MAX_SAME_TICK_TRANSITIONS: usize = 16;

/// Continuous hold duration in seconds required to promote a press to `Hold`.
pub const BUTTON_HOLD_MIN_SECS: f64 = 0.30;

/// Maximum interval in seconds between a `Tap` release and the next press to
/// classify the second press as `DoublePress`.
pub const BUTTON_DOUBLE_PRESS_SECS: f64 = 0.25;

/// Ramp rate (per second) for `ComboState::animated_move_blend` between `0.0`
/// and `1.0` (~`0.15` s over the full range).
pub const ANIMATED_MOVE_BLEND_RATE: f64 = 6.6666665;

/// Validation failure returned by [`ComboState::try_restore`] and
/// [`SaberThrowState::try_restore`] when a wire or caller checkpoint violates
/// state bounds or numeric invariants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    StateOutOfRange,
    InvalidButtonHistory,
    GraceStateOutOfRange,
    InvalidTime,
    InvalidDedupCount,
    DedupGroupOutOfRange,
    InvalidVector,
    InvalidDirection,
    InconsistentState,
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::StateOutOfRange => "checkpoint state index out of range",
            Self::InvalidButtonHistory => "checkpoint button_history failed validation",
            Self::GraceStateOutOfRange => "checkpoint grace_state out of range",
            Self::InvalidTime => "checkpoint time value must be finite and nonnegative",
            Self::InvalidDedupCount => "checkpoint hit list count or uniqueness is invalid",
            Self::DedupGroupOutOfRange => {
                "checkpoint hit_history attack key or shared_id out of range"
            }
            Self::InvalidVector => "checkpoint vector contains non-finite component",
            Self::InvalidDirection => "checkpoint flight direction must be unit-length when active",
            Self::InconsistentState => {
                "checkpoint derived flags or clocks disagree with state definition"
            }
        };
        f.write_str(msg)
    }
}

impl std::error::Error for CheckpointError {}

/// Single timestamped entry in the 8-slot [`ButtonEventHistory`] ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonEvent {
    pub button: ComboButton,
    /// Classification bitflags:
    /// - `0x00`: Press
    /// - `0x01`: Tap (released before `0.30` s)
    /// - `0x02`: Hold (held past `0.30` s)
    /// - `0x03`: HoldRelease
    /// - `0x04`: DoublePress (pressed within `0.25` s of a Tap release)
    /// - `0x05`: DoubleTap
    /// - `0x06`: DoubleHold
    /// - `0x07`: DoubleHoldRelease
    pub flags: u8,
    /// Timestamp on the monotonic history clock of the latest promotion or release.
    pub event_time: f64,
}

impl Default for ButtonEvent {
    fn default() -> Self {
        Self {
            button: ComboButton::Fire,
            flags: 0,
            event_time: 0.0,
        }
    }
}

impl ButtonEvent {
    pub const FLAG_PRESS: u8 = 0x00;
    pub const FLAG_TAP: u8 = 0x01;
    pub const FLAG_HOLD: u8 = 0x02;
    pub const FLAG_HOLD_RELEASE: u8 = 0x03;
    pub const FLAG_DOUBLE_PRESS: u8 = 0x04;
    pub const FLAG_DOUBLE_TAP: u8 = 0x05;
    pub const FLAG_DOUBLE_HOLD: u8 = 0x06;
    pub const FLAG_DOUBLE_HOLD_RELEASE: u8 = 0x07;
}

/// Fixed-capacity 8-event timestamped button ring used for transition window
/// matching, earliest-match candidate selection, and consumed-input pruning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonEventHistory {
    events: [ButtonEvent; MAX_BUTTON_EVENTS],
    len: u8,
    pub held_mask: u8,
    pub hold_emitted_mask: u8,
    /// Double classification for an active press, and taps eligible for a second press.
    pub double_mask: u8,
    pub tap_mask: u8,
    /// Time of the last physical edge, retained independently of consumed events.
    pub changed_at: [f64; ComboButton::COUNT],
    pub now: f64,
}

impl Default for ButtonEventHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl ButtonEventHistory {
    pub const fn new() -> Self {
        Self {
            events: [ButtonEvent {
                button: ComboButton::Fire,
                flags: 0,
                event_time: 0.0,
            }; MAX_BUTTON_EVENTS],
            len: 0,
            held_mask: 0,
            hold_emitted_mask: 0,
            double_mask: 0,
            tap_mask: 0,
            changed_at: [0.0; ComboButton::COUNT],
            now: 0.0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn events(&self) -> &[ButtonEvent] {
        let n = (self.len as usize).min(MAX_BUTTON_EVENTS);
        &self.events[..n]
    }

    pub fn from_parts(
        now: f64,
        held_mask: u8,
        hold_emitted_mask: u8,
        slice: &[ButtonEvent],
    ) -> Option<Self> {
        if slice.len() > MAX_BUTTON_EVENTS {
            return None;
        }
        let mut h = Self::new();
        h.now = now;
        h.held_mask = held_mask;
        h.hold_emitted_mask = hold_emitted_mask;
        h.events[..slice.len()].copy_from_slice(slice);
        h.len = slice.len() as u8;
        Some(h)
    }

    fn validate(&self) -> Result<(), CheckpointError> {
        let history = self;
        let valid_mask = (1u8 << ComboButton::COUNT) - 1;
        if !history.now.is_finite()
            || history.now < 0.0
            || history.len() > MAX_BUTTON_EVENTS
            || (history.held_mask | history.tap_mask) & !valid_mask != 0
            || (history.hold_emitted_mask | history.double_mask) & !history.held_mask != 0
            || history.tap_mask & history.held_mask != 0
            || history
                .changed_at
                .iter()
                .any(|t| !t.is_finite() || *t < 0.0 || *t > history.now)
        {
            return Err(CheckpointError::InvalidButtonHistory);
        }
        let mut last: [Option<ButtonEvent>; ComboButton::COUNT] = [None; ComboButton::COUNT];
        let mut previous = 0.0;
        for ev in history.events() {
            if ev.flags > ButtonEvent::FLAG_DOUBLE_HOLD_RELEASE
                || !ev.event_time.is_finite()
                || ev.event_time < previous
                || ev.event_time > history.now
            {
                return Err(CheckpointError::InvalidButtonHistory);
            }
            let index = ev.button as usize;
            if let Some(prior) = last[index] {
                let follows = match ev.flags & 3 {
                    0 => {
                        prior.flags & 1 != 0
                            && (ev.flags & 4 != 0)
                                == (prior.flags & 3 == 1
                                    && ev.event_time - prior.event_time
                                        <= BUTTON_DOUBLE_PRESS_SECS + TIME_EPS)
                    }
                    1 => prior.flags & 3 == 0 && ev.flags & 4 == prior.flags & 4,
                    2 => {
                        prior.flags & 3 == 0
                            && ev.flags & 4 == prior.flags & 4
                            && ev.event_time - prior.event_time + TIME_EPS >= BUTTON_HOLD_MIN_SECS
                    }
                    _ => prior.flags & 3 == 2 && ev.flags & 4 == prior.flags & 4,
                };
                if !follows {
                    return Err(CheckpointError::InvalidButtonHistory);
                }
            }
            last[index] = Some(*ev);
            previous = ev.event_time;
        }
        for (index, event) in last.into_iter().enumerate() {
            let bit = 1 << index;
            let held = self.held_mask & bit != 0;
            let promoted = self.hold_emitted_mask & bit != 0;
            if promoted && self.now - self.changed_at[index] + TIME_EPS < BUTTON_HOLD_MIN_SECS {
                return Err(CheckpointError::InvalidButtonHistory);
            }
            if let Some(e) = event {
                let valid = match e.flags & 3 {
                    0 => held && !promoted,
                    1 => !held && self.tap_mask & bit != 0,
                    2 => {
                        held && promoted
                            && e.event_time - self.changed_at[index] + TIME_EPS
                                >= BUTTON_HOLD_MIN_SECS
                    }
                    _ => !held && self.tap_mask & bit == 0,
                };
                if !valid
                    || (held && (e.flags & 4 != 0) != (self.double_mask & bit != 0))
                    || (e.flags & 3 != 2 && self.changed_at[index] != e.event_time)
                {
                    return Err(CheckpointError::InvalidButtonHistory);
                }
            }
        }
        Ok(())
    }

    fn push(&mut self, ev: ButtonEvent) {
        let n = (self.len as usize).min(MAX_BUTTON_EVENTS);
        if n < MAX_BUTTON_EVENTS {
            self.events[n] = ev;
            self.len = (n + 1) as u8;
        } else {
            self.events.copy_within(1..MAX_BUTTON_EVENTS, 0);
            self.events[MAX_BUTTON_EVENTS - 1] = ev;
        }
    }

    fn append(&mut self, button: ComboButton, flags: u8) {
        self.push(ButtonEvent {
            button,
            flags,
            event_time: self.now,
        });
    }

    fn release(&mut self, button: ComboButton) {
        let bit = button.bit();
        let held = self.hold_emitted_mask & bit != 0;
        let double = self.double_mask & bit != 0;
        self.append(button, 1 | ((held as u8) << 1) | ((double as u8) << 2));
        self.held_mask &= !bit;
        self.hold_emitted_mask &= !bit;
        self.double_mask &= !bit;
        if !held {
            self.tap_mask |= bit;
        }
        self.changed_at[button as usize] = self.now;
    }

    /// The edge mask can include a tap which began and ended between samples.
    pub fn update(&mut self, dt: f64, pressed: u8, held: u8) {
        self.now += dt;
        for idx in 0..ComboButton::COUNT {
            let button = ComboButton::from_index(idx).unwrap();
            let bit = button.bit();
            if pressed & bit != 0 || held & !self.held_mask & bit != 0 {
                if self.held_mask & bit != 0 {
                    self.release(button);
                }
                let double = self.tap_mask & bit != 0
                    && self.now - self.changed_at[idx] <= BUTTON_DOUBLE_PRESS_SECS + TIME_EPS;
                self.tap_mask &= !bit;
                if double {
                    self.double_mask |= bit;
                }
                self.append(
                    button,
                    if double {
                        ButtonEvent::FLAG_DOUBLE_PRESS
                    } else {
                        ButtonEvent::FLAG_PRESS
                    },
                );
                self.changed_at[idx] = self.now;
                self.held_mask |= bit;
            }
            if self.held_mask & bit == 0 {
                continue;
            }
            if self.hold_emitted_mask & bit == 0
                && self.now - self.changed_at[idx] + TIME_EPS >= BUTTON_HOLD_MIN_SECS
            {
                self.hold_emitted_mask |= bit;
                self.append(
                    button,
                    if self.double_mask & bit != 0 {
                        ButtonEvent::FLAG_DOUBLE_HOLD
                    } else {
                        ButtonEvent::FLAG_HOLD
                    },
                );
            }
            if held & bit == 0 {
                self.release(button);
            }
        }
    }

    pub fn prune_through(&mut self, consumed_time: f64) {
        let n = self.len();
        let removed = self
            .events()
            .partition_point(|ev| ev.event_time <= consumed_time + TIME_EPS);
        self.events.copy_within(removed..n, 0);
        self.len -= removed as u8;
    }

    fn discard(&mut self, discarded: u8) {
        let mut write = 0;
        for read in 0..self.len() {
            if discarded & (1 << read) == 0 {
                self.events[write] = self.events[read];
                write += 1;
            }
        }
        self.len = write as u8;
    }

    /// Return the earliest classified event, or the start of an overlapping
    /// button level, inside the inclusive acceptance interval.
    pub fn earliest_match(
        &self,
        button: ComboButton,
        mode: ButtonMode,
        start: f64,
        end: f64,
        _reference: f64,
    ) -> Option<f64> {
        let bit = button.bit();
        let in_range = |t| t >= start - TIME_EPS && t <= end + TIME_EPS;
        let matches = |flags: u8| match mode {
            ButtonMode::Press | ButtonMode::Down => flags & 3 == 0,
            ButtonMode::Tap => flags & 3 == 1,
            ButtonMode::Hold => flags & 3 == 2,
            ButtonMode::HoldRelease => flags & 3 == 3,
            ButtonMode::Release | ButtonMode::Up => flags & 1 != 0,
            ButtonMode::DoublePress => flags == 4,
            ButtonMode::DoubleTap => flags == 5,
            ButtonMode::DoubleHold => flags == 6,
            ButtonMode::DoubleHoldRelease => flags == 7,
        };
        let event = self
            .events()
            .iter()
            .find(|ev| ev.button == button && matches(ev.flags) && in_range(ev.event_time))
            .map(|ev| ev.event_time);

        // A level already in effect at the start of the window is also eligible.
        let since = self.changed_at[button as usize];
        let level = match mode {
            ButtonMode::Up if self.held_mask & bit == 0 => Some(since),
            ButtonMode::Down if self.held_mask & bit != 0 => Some(since),
            ButtonMode::Hold if self.hold_emitted_mask & bit != 0 => {
                Some(since + BUTTON_HOLD_MIN_SECS)
            }
            ButtonMode::DoubleHold if self.double_mask & self.hold_emitted_mask & bit != 0 => {
                Some(since + BUTTON_HOLD_MIN_SECS)
            }
            _ => None,
        }
        .map(|since| v8math::max(since, start))
        .filter(|&t| in_range(t));
        match (event, level) {
            (Some(a), Some(b)) => Some(v8math::min(a, b)),
            _ => event.or(level),
        }
    }

    pub fn earliest_any_press(&self, start: f64, end: f64) -> Option<f64> {
        self.events()
            .iter()
            .find(|ev| {
                ev.flags & 3 == 0
                    && ev.event_time >= start - TIME_EPS
                    && ev.event_time <= end + TIME_EPS
            })
            .map(|ev| ev.event_time)
    }
}

/// Complete, copyable per-soldier prediction checkpoint for own-body
/// reconciliation, including active hit-history lists, the 8-event button ring,
/// carry-over grace window state, and continuous button-hold timers so
/// mid-combo replays match uninterrupted execution bit-for-bit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComboCheckpoint {
    pub state: StateId,
    pub state_time: f64,
    pub eval_time: f64,
    pub anim_time: f64,
    pub state_origin_time: f64,
    pub posture_time: f64,
    pub grace_state: u16,
    pub grace_offset: f64,
    pub animated_move_stopped: bool,
    pub animated_move_blend: f64,
    pub gravity_target_latched: bool,
    pub align_stick: [f64; 2],
    pub deflecting: bool,
    pub blades_off_mask: u8,
    pub last_posture: ComboPosture,
    pub button_history: ButtonEventHistory,
    pub hit_history: HitHistory,
}

/// Complete per-soldier runtime state for the Battlefront II melee combo FSM.
/// Fixed-size and `Copy` with zero heap allocations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComboState {
    pub state: StateId,
    pub state_time: f64,
    /// Unwrapped evaluation clock for expiry, transition windows, attacks, and
    /// hit-list lifetimes (never wraps or clamps on looping clips).
    pub eval_time: f64,
    /// Rendered animation playback clock (wraps or clamps according to
    /// `LoopMode`).
    pub anim_time: f64,
    /// Value of `button_history.now` corresponding to `eval_time == 0.0` for
    /// the current state.
    pub state_origin_time: f64,
    pub posture_time: f64,
    /// Previous state kept active for `TimeEnd` carry-over grace windows that
    /// extend past its end time (`NO_STATE` when none is active).
    pub grace_state: u16,
    /// Offset from `grace_state`'s origin to `state_origin_time`.
    pub grace_offset: f64,
    pub animated_move_stopped: bool,
    /// Velocity override blend weight in `[0.0, 1.0]`, ramping at
    /// [`ANIMATED_MOVE_BLEND_RATE`] (`~6.67 / s`, `~0.15 s` full range).
    pub animated_move_blend: f64,
    pub gravity_target_latched: bool,
    /// Movement stick (forward, right) that `AlignedToThrust` turns toward. It
    /// follows the live stick, except that an `AlignedToThrust("Initial")`
    /// state keeps the value its entry tick saw.
    pub align_stick: [f64; 2],
    pub deflecting: bool,
    pub blades_off_mask: u8,
    pub last_posture: ComboPosture,
    pub button_history: ButtonEventHistory,
    pub hit_history: HitHistory,
}

impl ComboState {
    /// Initialize at the combo machine's `IDLE` state.
    pub fn new(machine: &ComboMachine) -> Self {
        let idle = machine.state(machine.idle_state);
        Self {
            state: machine.idle_state,
            state_time: 0.0,
            eval_time: 0.0,
            anim_time: 0.0,
            state_origin_time: 0.0,
            posture_time: 0.0,
            grace_state: NO_STATE,
            grace_offset: 0.0,
            animated_move_stopped: false,
            animated_move_blend: 0.0,
            gravity_target_latched: false,
            align_stick: [0.0; 2],
            deflecting: idle.deflect.is_some(),
            blades_off_mask: idle.turn_off_lightsaber,
            last_posture: ComboPosture::Stand,
            button_history: ButtonEventHistory::new(),
            hit_history: HitHistory::new(),
        }
    }

    /// Capture the complete prediction state for checkpointing or wire
    /// publication.
    pub fn checkpoint(&self) -> ComboCheckpoint {
        ComboCheckpoint {
            state: self.state,
            state_time: self.state_time,
            eval_time: self.eval_time,
            anim_time: self.anim_time,
            state_origin_time: self.state_origin_time,
            posture_time: self.posture_time,
            grace_state: self.grace_state,
            grace_offset: self.grace_offset,
            animated_move_stopped: self.animated_move_stopped,
            animated_move_blend: self.animated_move_blend,
            gravity_target_latched: self.gravity_target_latched,
            align_stick: self.align_stick,
            deflecting: self.deflecting,
            blades_off_mask: self.blades_off_mask,
            last_posture: self.last_posture,
            button_history: self.button_history,
            hit_history: self.hit_history,
        }
    }

    /// Validate and atomically restore from an authoritative prediction
    /// checkpoint. On error, `self` is left completely unchanged.
    pub fn try_restore(
        &mut self,
        machine: &ComboMachine,
        cp: ComboCheckpoint,
    ) -> Result<(), CheckpointError> {
        let Some(st_def) = machine.states.get(cp.state.0 as usize) else {
            return Err(CheckpointError::StateOutOfRange);
        };
        if cp.deflecting != st_def.deflect.is_some()
            || cp.blades_off_mask != st_def.turn_off_lightsaber
            || (cp.gravity_target_latched && st_def.gravity.is_none())
        {
            return Err(CheckpointError::InconsistentState);
        }
        if !cp.state_time.is_finite()
            || cp.state_time < 0.0
            || !cp.eval_time.is_finite()
            || cp.eval_time < 0.0
            || !cp.anim_time.is_finite()
            || cp.anim_time < 0.0
            || !cp.state_origin_time.is_finite()
            || !cp.posture_time.is_finite()
            || cp.posture_time < 0.0
            || !cp.grace_offset.is_finite()
            || cp.grace_offset < 0.0
            || !cp.animated_move_blend.is_finite()
            || !(0.0..=1.0).contains(&cp.animated_move_blend)
            || !cp.align_stick.iter().all(|v| v.is_finite())
        {
            return Err(CheckpointError::InvalidTime);
        }
        if cp.state_time > cp.eval_time + TIME_EPS {
            return Err(CheckpointError::InconsistentState);
        }
        if cp.grace_state != NO_STATE && (cp.grace_state as usize) >= machine.states.len() {
            return Err(CheckpointError::GraceStateOutOfRange);
        }
        cp.button_history.validate()?;
        let now = cp.button_history.now;
        let clip_len = st_def
            .animation
            .map_or(0.0, |clip| machine.animation(clip).duration_secs);
        if cp.state_origin_time > now + TIME_EPS
            || v8math::abs(cp.state_origin_time + cp.eval_time - now)
                > TIME_EPS * (1.0 + now / 1000.)
            || cp.anim_time != clip_playback_time(cp.eval_time, clip_len, st_def.loop_mode)
            || (cp.grace_state == NO_STATE && cp.grace_offset != 0.0)
        {
            return Err(CheckpointError::InconsistentState);
        }
        if cp.hit_history.has_duplicates() {
            return Err(CheckpointError::InvalidDedupCount);
        }
        for rec in cp.hit_history.lists() {
            if rec.len() > MAX_TARGETS_PER_ATTACK {
                return Err(CheckpointError::InvalidDedupCount);
            }
            if rec.shared_id > 0x000F_FFFF {
                return Err(CheckpointError::DedupGroupOutOfRange);
            }
            let st_idx = (rec.attack_key >> 16) as usize;
            let atk_idx = (rec.attack_key & 0xFFFF) as usize;
            let Some(rec_state) = machine.states.get(st_idx) else {
                return Err(CheckpointError::DedupGroupOutOfRange);
            };
            if atk_idx >= rec_state.attacks.len()
                || rec.shared_id != rec_state.attacks[atk_idx].shared_id
            {
                return Err(CheckpointError::DedupGroupOutOfRange);
            }
            if rec.expires_at.is_nan() || rec.expires_at == f64::NEG_INFINITY {
                return Err(CheckpointError::InvalidTime);
            }
        }

        self.state = cp.state;
        self.state_time = cp.state_time;
        self.eval_time = cp.eval_time;
        self.anim_time = cp.anim_time;
        self.state_origin_time = cp.state_origin_time;
        self.posture_time = cp.posture_time;
        self.grace_state = cp.grace_state;
        self.grace_offset = if cp.grace_state == NO_STATE {
            0.0
        } else {
            cp.grace_offset
        };
        self.animated_move_stopped = cp.animated_move_stopped;
        self.animated_move_blend = cp.animated_move_blend;
        self.gravity_target_latched = cp.gravity_target_latched;
        self.align_stick = cp.align_stick;
        self.deflecting = cp.deflecting;
        self.blades_off_mask = cp.blades_off_mask;
        self.last_posture = cp.last_posture;
        self.button_history = cp.button_history;
        self.hit_history = cp.hit_history;
        Ok(())
    }

    /// True if currently in the machine's neutral `IDLE` state.
    pub fn is_idle(&self, machine: &ComboMachine) -> bool {
        self.state == machine.idle_state
    }

    /// Whether the settled state is a standing guard that the block control can release.
    pub fn is_guarding(&self, machine: &ComboMachine) -> bool {
        let current = machine.state(self.state);
        current.deflect.is_some() && machine.state_end_time(current).is_none()
    }

    /// Enter the catch animation from its beginning and return its presentation event.
    ///
    /// A blade can return after the soldier's movement step. Ordinary state entry
    /// resets the clocks, rebases hit history and installs the state's deflection
    /// and blade flags. Movement resumes on the next soldier tick. Catch states
    /// must omit entry-only motion such as initial thrust alignment or an impulse
    /// velocity target.
    pub fn enter_catch(&mut self, machine: &ComboMachine) -> Option<StateEntry> {
        let catch = machine.catch_state?;
        Some(enter_state(machine, self, None, machine.state(catch), None))
    }

    /// Return to the neutral `IDLE` state from its beginning, clearing any
    /// active guard and resetting state clocks.
    pub fn enter_idle(&mut self, machine: &ComboMachine) -> StateEntry {
        enter_state(machine, self, None, machine.state(machine.idle_state), None)
    }
}

/// Per-tick player or AI inputs read by the combo state machine.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ComboInput {
    /// Bitmask of [`ComboButton`] rising edges on this tick.
    pub buttons_pressed: u8,
    /// Bitmask of [`ComboButton`] currently held down on this tick.
    pub buttons_held: u8,
    /// Forward/backward stick input in `[-1.0, 1.0]`.
    pub forward: f64,
    /// Right/left strafe input in `[-1.0, 1.0]` (`+1` right, `-1` left).
    pub right: f64,
    /// Set every tick when the dedicated block control uses hold semantics.
    pub hold_to_block: bool,
}

impl ComboInput {
    pub fn press(mut self, button: ComboButton) -> Self {
        self.buttons_pressed |= button.bit();
        self.buttons_held |= button.bit();
        self
    }

    pub fn hold(mut self, button: ComboButton) -> Self {
        self.buttons_held |= button.bit();
        self
    }

    pub fn stick(mut self, forward: f64, right: f64) -> Self {
        self.forward = forward;
        self.right = right;
        self
    }

    /// Dedicated stock block button. A press is durable even when already released.
    pub fn with_block_button(mut self, pressed: bool, held: bool) -> Self {
        if pressed {
            self.buttons_pressed |= ComboButton::FireSecondary.bit();
        }
        if held {
            self.buttons_held |= ComboButton::FireSecondary.bit();
        }
        self
    }

    /// Accessibility binding. Supply both the durable press and the sampled level each tick.
    pub fn with_hold_to_block(mut self, pressed: bool, held: bool) -> Self {
        self.hold_to_block = true;
        self.with_block_button(pressed, held)
    }

    fn thrust_magnitude(&self) -> f64 {
        v8math::hypot(self.forward, self.right)
    }

    fn thrust_angle_deg(&self) -> f64 {
        if self.forward == 0.0 && self.right == 0.0 {
            return 0.0;
        }
        v8math::atan2(self.right, self.forward) * RAD_TO_DEG
    }
}

/// Body facts read from locomotion at the start of a combo tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyContext {
    pub posture: ComboPosture,
    pub aim_yaw: f64,
    pub velocity_y: f64,
    pub grounded: bool,
    /// True when the soldier has non-zero horizontal movement input, choosing
    /// `EnergyRestore` vs `EnergyRestoreIdle`.
    pub moving: bool,
}

impl BodyContext {
    pub fn from_mover(mover: &Mover, move_state: &move2::State, moving: bool) -> Self {
        Self {
            posture: ComboPosture::from_movement(mover.posture, mover.air, move_state),
            aim_yaw: mover.aim_yaw,
            velocity_y: mover.velocity.y,
            grounded: mover.grounded,
            moving,
        }
    }
}

/// Stamina for individual interceptions and offhand weapons. A combo step uses
/// the soldier's per-life movement state so that its exhaustion latch persists.
pub trait CombatEnergy {
    fn balance(&self) -> f64;
    fn available(&self) -> f64;
    fn pay(&mut self, cost: f64) -> bool;
}

/// Bare balances support isolated weapon simulations without a soldier mover.
impl CombatEnergy for f64 {
    fn balance(&self) -> f64 {
        *self
    }
    fn available(&self) -> f64 {
        *self
    }
    fn pay(&mut self, cost: f64) -> bool {
        if !self.is_finite() || !cost.is_finite() || cost < 0.0 || (cost > 0.0 && cost > *self) {
            return false;
        }
        *self -= cost;
        true
    }
}

fn available_stamina(state: &move2::State) -> f64 {
    if state.exhausted {
        0.0
    } else {
        state.energy
    }
}

pub struct SoldierEnergy<'a> {
    pub state: &'a mut move2::State,
    pub tuning: &'a move2::Tuning,
}

impl CombatEnergy for SoldierEnergy<'_> {
    fn balance(&self) -> f64 {
        self.state.energy
    }
    fn available(&self) -> f64 {
        available_stamina(self.state)
    }
    fn pay(&mut self, cost: f64) -> bool {
        (cost == 0.0 || self.state.energy >= cost) && self.state.charge(cost, self.tuning)
    }
}

/// Movement, facing, and gravity overrides emitted by a combo state for this
/// tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementDirective {
    pub thrust_factor: f64,
    pub strafe_factor: f64,
    pub turn_factor: f64,
    /// Yaw added to the aim heading by `AlignedToThrust` (radians, positive
    /// turns left; zero without alignment or inside the stick's dead zone).
    pub align_turn: f64,
    /// World-space horizontal velocity override `[vx, 0.0, vz]` when
    /// `AnimatedMove` is active.
    pub animated_move: Option<V3>,
    /// Velocity override blend weight in `[0.0, 1.0]` passed through to the
    /// 2005 soldier mover.
    pub velocity_blend: f64,
    pub lock_locomotion: bool,
    pub gravity_scale: f64,
    /// One-tick vertical velocity impulse set on entry to a `GravityVelocityTarget(..., "Impulse")` state.
    pub vertical_impulse: Option<f64>,
}

impl Default for MovementDirective {
    fn default() -> Self {
        Self {
            thrust_factor: 1.0,
            strafe_factor: 1.0,
            turn_factor: 1.0,
            align_turn: 0.0,
            animated_move: None,
            velocity_blend: 1.0,
            lock_locomotion: false,
            gravity_scale: 1.0,
            vertical_impulse: None,
        }
    }
}

/// One active blade attack window during this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActiveAttack {
    pub state_id: StateId,
    pub attack_index: u16,
    pub attack_group: AttackGroupId,
    pub attack_key: u32,
    pub shared_id: u32,
    pub edge: u8,
    pub damage: f64,
    pub push: f64,
    pub damage_length: f64,
    pub damage_width: f64,
    /// Outgoing state's evaluation interval intersected with the damage window.
    pub sample_start: f64,
    pub sample_end: f64,
    window_end: f64,
    /// Normalized `[0.0, 1.0]` progress across the damage window at the start of
    /// this tick.
    pub window_progress_prev: f64,
    /// Normalized `[0.0, 1.0]` progress across the damage window at the end of
    /// this tick.
    pub window_progress_curr: f64,
}

impl ActiveAttack {
    /// Sample the outgoing clip inside this contact interval, including its
    /// single pose for a point window. Pose times wrap or hold with the clip.
    pub fn sample(
        &self,
        machine: &ComboMachine,
        mut pose: impl FnMut(f64) -> (V3, V3),
    ) -> DamageSampleList {
        self.sample_with_time(machine, |_,clip_time| pose(clip_time))
    }

    /// Sample a moving socket using both the unwrapped interval and the
    /// outgoing clip clock. Refinement measures the resulting world-space tip.
    pub fn sample_with_time(
        &self, machine: &ComboMachine, mut pose: impl FnMut(f64,f64) -> (V3,V3),
    ) -> DamageSampleList {
        let state = machine.state(self.state_id);
        let clip_len = state
            .animation
            .map_or(0.0, |clip| machine.animation(clip).duration_secs);
        build_damage_samples(
            self.sample_start,
            self.sample_end,
            self.damage_length,
            |time| pose(time,clip_playback_time(time, clip_len, state.loop_mode)),
        )
    }
}

/// Fixed-capacity list of active attacks for one tick (zero allocation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveAttackList {
    items: [ActiveAttack; MAX_ACTIVE_ATTACKS],
    len: u8,
}

impl Default for ActiveAttackList {
    fn default() -> Self {
        Self {
            items: [ActiveAttack::default(); MAX_ACTIVE_ATTACKS],
            len: 0,
        }
    }
}

impl ActiveAttackList {
    pub fn as_slice(&self) -> &[ActiveAttack] {
        &self.items[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, item: ActiveAttack) {
        self.items[self.len as usize] = item;
        self.len += 1;
    }
}

/// An entered state and the unwrapped clip clock accepted on entry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StateEntry {
    pub state: StateId,
    pub eval_time: f64,
}

impl StateEntry {
    pub fn sounds(self, machine: &ComboMachine) -> impl Iterator<Item = (usize, SoundId)> + '_ {
        machine
            .state(self.state)
            .sounds
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.at_secs.is_none().then_some((i, c.sound)))
    }
}

/// All transitions taken on this tick, including instantaneous intermediate states.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StateEntries {
    items: [StateEntry; MAX_SAME_TICK_TRANSITIONS],
    len: u8,
}

impl StateEntries {
    pub fn as_slice(&self) -> &[StateEntry] {
        &self.items[..self.len as usize]
    }
    fn push(&mut self, entry: StateEntry) {
        self.items[self.len as usize] = entry;
        self.len += 1;
    }
}

/// The outgoing state's evaluated interval. Timed sounds retain their authored
/// indices even when several are due in the same tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedCues {
    pub state: StateId,
    previous: f64,
    current: f64,
}

impl TimedCues {
    pub fn sounds(self, machine: &ComboMachine) -> impl Iterator<Item = (usize, SoundId)> + '_ {
        machine
            .state(self.state)
            .sounds
            .iter()
            .enumerate()
            .filter_map(move |(i, c)| {
                let t = c.at_secs?;
                let due = if t <= TIME_EPS {
                    self.previous <= TIME_EPS && self.current > TIME_EPS
                } else {
                    self.previous + TIME_EPS < t && t <= self.current + TIME_EPS
                };
                due.then_some((i, c.sound))
            })
    }
}

/// Complete deterministic output of one combo state machine tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepOutput {
    pub active: bool,
    pub previous_state: StateId,
    pub entered_state: Option<StateId>,
    pub play_explosion: bool,
    pub posture_override: Option<ComboPosture>,
    pub input_lock: InputLockMask,
    pub movement: MovementDirective,
    pub energy_override_active: bool,
    pub energy_delta: f64,
    /// Sum of accepted transition costs, before clamping the stamina balance (zero if
    /// none). Used by [`prepare_movement_step`] to suppress duplicate movement
    /// costs when a combo transition (such as `DEFLECT_ROLL`) hands off to a
    /// movement posture.
    pub transition_energy_spent: f64,
    pub active_attacks: ActiveAttackList,
    /// Packed record key of the first attack that exceeded the window or hit-list capacity.
    pub first_omitted_attack: Option<u32>,
    pub entries: StateEntries,
    pub timed_cues: Option<TimedCues>,
    pub deflect: Option<DeflectDef>,
    pub blades_off_mask: u8,
    /// Yaw added to the aim heading for the settled state's swing and drawn
    /// body (radians, positive turns left): its `AlignedToThrust` turn when the
    /// state has no `Deflect`, otherwise zero.
    pub swing_turn: f64,
}

/// Advance the melee combo state machine by one fixed step (`dt`, typically
/// [`COMBO_FIXED_DT`]).
///
/// For `GameFamily::Swbf1`, returns an inactive default [`StepOutput`] without
/// mutating `state` or `life`, preserving 2004 melee untouched.
///
/// Pass the same per-life stamina state on every tick, including during prediction.
/// Set `apply_default_energy_regen` to `true` when this step owns baseline
/// stamina regeneration in standalone simulation. The coupled soldier step
/// passes `false` so the mover applies baseline recovery after evaluating
/// the combo. Explicit state rates and transition costs apply here.
#[allow(clippy::too_many_arguments)]
pub fn step(
    family: GameFamily,
    machine: &ComboMachine,
    state: &mut ComboState,
    input: &ComboInput,
    body: &BodyContext,
    life: &mut move2::State,
    tuning: &move2::Tuning,
    apply_default_energy_regen: bool,
    dt: f64,
) -> StepOutput {
    if family != GameFamily::Swbf2 || !dt.is_finite() || dt <= 0.0 {
        return StepOutput::default();
    }

    if machine.states.get(state.state.0 as usize).is_none() {
        return StepOutput::default();
    }

    let mut out = StepOutput {
        active: true,
        previous_state: state.state,
        ..StepOutput::default()
    };

    if body.posture != state.last_posture {
        state.last_posture = body.posture;
        state.posture_time = 0.0;
    } else {
        state.posture_time += dt;
    }

    state
        .button_history
        .update(dt, input.buttons_pressed, input.buttons_held);
    if machine.state(state.state).aligned_to_thrust != AlignMode::Initial {
        state.align_stick = [input.forward, input.right];
    }

    let energy_before = life.energy;
    let mut tick_dt = dt;
    let mut transitions_taken = 0usize;
    let mut entry_vertical_impulse: Option<f64> = None;

    loop {
        let Some(cur) = machine.states.get(state.state.0 as usize) else {
            return StepOutput::default();
        };
        let clip_len = cur
            .animation
            .and_then(|clip| machine.animations.get(clip.0 as usize))
            .map_or(0.0, |meta| meta.duration_secs);
        let end_time = machine.state_end_time(cur);

        let prev_eval_time = state.eval_time;

        if tick_dt > 0.0 {
            state.state_time += tick_dt;
            state.eval_time = prev_eval_time + tick_dt;
            state.anim_time = clip_playback_time(state.eval_time, clip_len, cur.loop_mode);

            // State stamina rate override or baseline recovery (once per tick).
            match cur.energy_restore_rate {
                EnergyRateOverride::Override(rate) => {
                    life.apply_energy_rate(rate, tick_dt, tuning);
                    out.energy_override_active = true;
                }
                EnergyRateOverride::Default => {
                    if apply_default_energy_regen {
                        let rate = if body.grounded && !body.moving {
                            tuning.energy_restore_idle
                        } else {
                            tuning.energy_restore
                        };
                        life.apply_energy_rate(rate, tick_dt, tuning);
                    }
                }
            }

            // Emit timed sound cues and active attacks for the outgoing tick
            // before any transition rebases hit lists.
            emit_state_tick_work(
                machine,
                cur,
                clip_len,
                end_time,
                prev_eval_time,
                state.eval_time,
                &mut state.hit_history,
                &mut out,
            );
        }

        let eval_ctx = EvalContext {
            posture_time: state.posture_time,
            life,
            input,
            body,
            history: &state.button_history,
        };

        let expired = end_time.is_some_and(|e| state.eval_time + TIME_EPS >= e);
        let cur_abs_now = state.button_history.now;

        let mut chosen = None;
        let mut denied_events = 0;
        if state.grace_state != NO_STATE {
            let carried = machine.state(StateId(state.grace_state));
            let origin = state.state_origin_time - state.grace_offset;
            let reference = cur_abs_now - origin;
            let end = machine.state_end_time(carried);
            chosen = choose_transition(
                carried,
                origin,
                reference,
                end,
                Scan::Carried,
                &eval_ctx,
                &mut denied_events,
            );
            if !carried
                .transitions
                .iter()
                .flat_map(|tr| &tr.branches)
                .any(|br| {
                    br.time_end
                        .is_some_and(|te| te.resolve(end) + TIME_EPS >= reference)
                })
            {
                state.grace_state = NO_STATE;
                state.grace_offset = 0.0;
            }
        }
        if chosen.is_none() {
            // Break waits on the same state clock as the transition window. A
            // state that continues its predecessor's clip continues that clock.
            let scan = if expired {
                Scan::Expiry
            } else {
                Scan::Interrupt(state.eval_time)
            };
            let reference = if expired {
                end_time.unwrap()
            } else {
                state.eval_time
            };
            chosen = choose_transition(
                cur,
                state.state_origin_time,
                reference,
                end_time,
                scan,
                &eval_ctx,
                &mut denied_events,
            );
        }

        if chosen.is_none() && denied_events != 0 {
            state.button_history.discard(denied_events);
        }
        let hold_released = input.hold_to_block
            && tick_dt > 0.0
            && state.is_guarding(machine)
            && input.buttons_held & ComboButton::FireSecondary.bit() == 0;
        let (target, cost, consumed_abs, fallback) = if let Some(choice) = chosen {
            (
                choice.target,
                choice.cost,
                choice.consumes.then_some(choice.time),
                choice.unconditional,
            )
        } else if hold_released {
            (machine.idle_state, 0.0, None, false)
        } else if expired {
            // An unmatched deadline uses the final destination without selecting a paid action.
            let target = cur
                .transitions
                .last()
                .map_or(machine.idle_state, |last| last.target);
            (target, 0.0, None, true)
        } else {
            break;
        };
        let carry_over = end_time
            .filter(|&end| fallback && expired && state_has_post_end_window(cur, end))
            .map(|_| (cur.id.0, state.state_origin_time));
        if !life.charge(cost, tuning) {
            break;
        }
        out.transition_energy_spent += cost;
        if let Some(t_consume) = consumed_abs {
            state.button_history.prune_through(t_consume);
        }
        let next = machine.state(target);
        out.entries
            .push(enter_state(machine, state, cur.animation, next, carry_over));
        out.entered_state = Some(target);
        out.play_explosion |= next.play_explosion;
        let imp = next
            .gravity
            .filter(|g| g.impulse)
            .and_then(|g| g.velocity_target);
        if imp.is_some() {
            entry_vertical_impulse = imp;
            state.gravity_target_latched = true;
        }
        transitions_taken += 1;
        tick_dt = 0.0;
        if next.must_show_one_frame || transitions_taken >= MAX_SAME_TICK_TRANSITIONS {
            break;
        }
    }

    // Compute settled-state outputs (locks, deflect, posture, movement, blades)
    // from the final settled state after any same-tick transitions.
    if let Some(settled) = machine.states.get(state.state.0 as usize) {
        out.posture_override = settled.posture;
        out.deflect = settled.deflect;
        out.blades_off_mask = settled.turn_off_lightsaber;
        if matches!(settled.energy_restore_rate, EnergyRateOverride::Override(_)) {
            out.energy_override_active = true;
        }
        if let Some(lock) = settled.input_lock {
            if lock
                .duration
                .is_none_or(|d| state.state_time < d - TIME_EPS)
            {
                out.input_lock = lock.blocked;
            }
        }

        let settled_end = machine.state_end_time(settled);
        let settled_ctx = EvalContext {
            posture_time: state.posture_time,
            life,
            input,
            body,
            history: &state.button_history,
        };
        let mut movement = MovementDirective {
            thrust_factor: settled.thrust_factor,
            strafe_factor: settled.strafe_factor,
            turn_factor: settled.turn_factor,
            align_turn: if settled.aligned_to_thrust == AlignMode::None {
                0.0
            } else {
                aligned_turn(state.align_stick, input.forward)
            },
            vertical_impulse: entry_vertical_impulse,
            ..MovementDirective::default()
        };

        if let Some(am) = &settled.animated_move {
            if !state.animated_move_stopped {
                if let Some(until) = &am.until_cond {
                    let delay_ok = until
                        .break_rule
                        .is_none_or(|b| state.state_time + TIME_EPS >= b.delay_seconds());
                    if delay_ok
                        && until
                            .energy
                            .is_none_or(|pred| pred.test(available_stamina(settled_ctx.life)))
                        && branch_input_time(
                            until,
                            state.state_origin_time,
                            state.eval_time,
                            settled_end,
                            state.button_history.now,
                            &settled_ctx,
                        )
                        .is_some()
                    {
                        state.animated_move_stopped = true;
                    }
                }
            }
            let guard_ok = am.if_cond.as_ref().is_none_or(|c| {
                c.energy
                    .is_none_or(|pred| pred.test(available_stamina(settled_ctx.life)))
                    && branch_input_time(
                        c,
                        state.state_origin_time,
                        state.eval_time,
                        settled_end,
                        state.button_history.now,
                        &settled_ctx,
                    )
                    .is_some()
            });
            if !state.animated_move_stopped && guard_ok {
                // The stick part is measured in the turned frame, then the whole
                // velocity is turned with it, as the 2005 game does.
                let turn = movement.align_turn;
                let (st, ct) = v8math::sin_cos(turn);
                let stick_fwd = input.forward * ct - input.right * st;
                let stick_right = input.forward * st + input.right * ct;
                let (sy, cy) = v8math::sin_cos(body.aim_yaw + turn);
                let fwd_speed = am.velocity_z + am.velocity_from_thrust * stick_fwd;
                let right_speed = am.velocity_x + am.velocity_from_strafe * stick_right;
                let vx = sy * fwd_speed - cy * right_speed;
                let vz = cy * fwd_speed + sy * right_speed;
                movement.animated_move = Some([vx, 0.0, vz]);
                movement.lock_locomotion = true;
            }
        }

        if let Some(grav) = settled.gravity {
            if grav.impulse {
                movement.gravity_scale = 1.0;
            } else if let Some(target) = grav.velocity_target {
                if !state.gravity_target_latched {
                    let reached = if grav.scale > 0.0 {
                        body.velocity_y <= target
                    } else {
                        body.velocity_y >= target
                    };
                    if reached {
                        state.gravity_target_latched = true;
                    }
                }
                movement.gravity_scale = if state.gravity_target_latched {
                    if grav.zero_gravity {
                        0.0
                    } else {
                        1.0
                    }
                } else {
                    grav.scale
                };
            } else {
                movement.gravity_scale = if grav.zero_gravity { 0.0 } else { grav.scale };
            }
        }
        let blend_delta = ANIMATED_MOVE_BLEND_RATE * dt;
        if movement.animated_move.is_some() {
            state.animated_move_blend =
                om::clamp(state.animated_move_blend + blend_delta, 0.0, 1.0);
        } else {
            state.animated_move_blend =
                om::clamp(state.animated_move_blend - blend_delta, 0.0, 1.0);
        }
        movement.velocity_blend = state.animated_move_blend;
        out.swing_turn = if settled.deflect.is_none() {
            movement.align_turn
        } else {
            0.0
        };
        out.movement = movement;
    }

    state.hit_history.retire(state.eval_time);
    out.energy_delta = life.energy - energy_before;
    out
}

/// Resolve the presentation clock from a layer's unwrapped time, playback length and loop mode.
#[inline]
pub fn clip_playback_time(eval_time: f64, clip_len: f64, loop_mode: LoopMode) -> f64 {
    match loop_mode {
        LoopMode::None | LoopMode::FinalFrame => {
            if clip_len > TIME_EPS {
                v8math::min(eval_time, clip_len)
            } else {
                eval_time
            }
        }
        LoopMode::Wrap => {
            if clip_len > TIME_EPS && eval_time >= clip_len - TIME_EPS {
                let wrapped =
                    eval_time - v8math::floor((eval_time + TIME_EPS) / clip_len) * clip_len;
                v8math::max(0.0, wrapped)
            } else {
                eval_time
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_state_tick_work(
    machine: &ComboMachine,
    cur: &ComboStateDef,
    clip_len: f64,
    end_time: Option<f64>,
    prev_eval_time: f64,
    curr_eval_time: f64,
    hit_history: &mut HitHistory,
    out: &mut StepOutput,
) {
    let curr_eval_time = v8math::min(curr_eval_time, end_time.unwrap_or(curr_eval_time));
    out.timed_cues = Some(TimedCues {
        state: cur.id,
        previous: prev_eval_time,
        current: curr_eval_time,
    });

    for (atk_idx, atk) in cur.attacks.iter().enumerate() {
        let Some((win_start, win_end)) =
            cur.attack_interval(atk, clip_len, end_time, machine.anim_fps)
        else {
            continue;
        };
        if prev_eval_time <= win_end + TIME_EPS && curr_eval_time > win_start + TIME_EPS {
            if out.active_attacks.as_slice().len() == MAX_ACTIVE_ATTACKS {
                out.first_omitted_attack.get_or_insert(atk.attack_key);
                continue;
            }
            let sample_start = om::clamp(prev_eval_time, win_start, win_end);
            let sample_end = om::clamp(curr_eval_time, sample_start, win_end);
            let span = if win_end.is_infinite() {
                clip_len - win_start
            } else {
                win_end - win_start
            };
            let progress = |time| {
                if span > 0.0 {
                    om::clamp((time - win_start) / span, 0.0, 1.0)
                } else {
                    0.0
                }
            };
            out.active_attacks.push(ActiveAttack {
                sample_start,
                sample_end,
                window_end: win_end,
                state_id: cur.id,
                attack_index: atk_idx as u16,
                attack_group: atk.attack_group,
                attack_key: atk.attack_key,
                shared_id: atk.shared_id,
                edge: atk.edge,
                damage: atk.damage,
                push: atk.push,
                damage_length: atk.damage_length,
                damage_width: atk.damage_width,
                window_progress_prev: progress(sample_start),
                window_progress_curr: progress(sample_end),
            });
        }
    }
    // Activate lists in temporal order. The authored order breaks simultaneous
    // starts. An expired group cannot inherit targets from its earlier use.
    let active = &mut out.active_attacks;
    let count = active.len as usize;
    active.items[..count].sort_unstable_by(|a, b| {
        a.sample_start
            .total_cmp(&b.sample_start)
            .then(a.attack_index.cmp(&b.attack_index))
    });
    active.len = 0;
    for i in 0..count {
        let attack = active.items[i];
        hit_history.retire(attack.sample_start);
        if hit_history
            .ensure_list(attack.attack_key, attack.shared_id, attack.window_end)
            .is_some()
        {
            active.push(attack);
        } else {
            out.first_omitted_attack.get_or_insert(attack.attack_key);
        }
    }
}

fn state_has_post_end_window(state: &ComboStateDef, end_time: f64) -> bool {
    state.transitions.iter().any(|tr| {
        !tr.branches.is_empty()
            && tr.branches.iter().any(|br| {
                br.time_end
                    .is_some_and(|te| te.resolve(Some(end_time)) > end_time + TIME_EPS)
            })
    })
}

fn enter_state(
    machine: &ComboMachine,
    state: &mut ComboState,
    previous_clip: Option<ClipId>,
    next: &ComboStateDef,
    carry_over: Option<(u16, f64)>,
) -> StateEntry {
    let target = next.id;
    let continues_anim =
        !next.restart_animation && previous_clip.is_some() && previous_clip == next.animation;

    let old_clock = state.eval_time;
    let new_clock = if continues_anim { old_clock } else { 0.0 };
    state.hit_history.rebase(old_clock - new_clock, old_clock);

    let next_clip_len = next
        .animation
        .and_then(|clip| machine.animations.get(clip.0 as usize))
        .map_or(0.0, |meta| meta.duration_secs);

    state.state = target;
    state.state_time = 0.0;
    state.eval_time = new_clock;
    state.anim_time = clip_playback_time(new_clock, next_clip_len, next.loop_mode);
    state.state_origin_time = state.button_history.now - new_clock;
    if let Some((grace_id, old_origin)) = carry_over {
        state.grace_state = grace_id;
        state.grace_offset = v8math::max(0.0, state.state_origin_time - old_origin);
    } else {
        state.grace_state = NO_STATE;
        state.grace_offset = 0.0;
    }
    state.animated_move_stopped = false;
    state.gravity_target_latched = false;
    state.deflecting = next.deflect.is_some();
    state.blades_off_mask = next.turn_off_lightsaber;

    StateEntry {
        state: target,
        eval_time: new_clock,
    }
}

/// Derive the current tick's class tuning and movement actions before the
/// profiled soldier step. Explicit combo rates and paid action costs replace
/// the corresponding mover rate or cost for this tick.
pub fn prepare_movement_step(
    base_tuning: &move2::Tuning,
    base_actions: move2::Actions,
    out: &StepOutput,
) -> (move2::Tuning, move2::Actions) {
    if !out.active {
        return (*base_tuning, base_actions);
    }
    let mut tuning = *base_tuning;
    let mut actions = base_actions;

    let scale_row = |row: &mut move2::ControlSpeed| {
        row.forward *= out.movement.thrust_factor;
        row.strafe *= out.movement.strafe_factor;
        row.turn *= out.movement.turn_factor;
    };
    scale_row(&mut tuning.stand);
    scale_row(&mut tuning.crouch);
    scale_row(&mut tuning.sprint);
    scale_row(&mut tuning.jump);

    if out.energy_override_active {
        tuning.energy_restore = 0.0;
        tuning.energy_restore_idle = 0.0;
    }

    if out.input_lock.contains(InputLockMask::SPRINT) {
        actions.sprint = false;
    }
    if out.input_lock.contains(InputLockMask::CROUCH) {
        actions.roll = false;
    }

    match out.posture_override {
        Some(ComboPosture::Roll) => {
            actions.roll = true;
            if out.transition_energy_spent > 0.0 {
                tuning.energy_cost_roll = 0.0;
            }
        }
        Some(ComboPosture::Sprint) => {
            actions.sprint = true;
        }
        Some(ComboPosture::Jump) if out.transition_energy_spent > 0.0 => {
            tuning.energy_cost_jump = 0.0;
            tuning.energy_cost_jump_sprint = Some(0.0);
        }
        _ => {}
    }

    (tuning, actions)
}

/// Complete per-tick 2005 soldier mover inputs prepared from a settled
/// [`StepOutput`]: filtered [`MoveInput`], stamina-coordinated
/// [`move2::Tuning`], [`move2::Actions`], and a same-tick
/// [`move2::MovementOverride`] applied before displacement and collision.
#[derive(Clone)]
pub struct PreparedMovementStep {
    pub tuning: move2::Tuning,
    pub input: MoveInput,
    pub actions: move2::Actions,
    pub override_: move2::MovementOverride,
}

const ALIGN_THRUST_DEADZONE_SQ: f64 = 0.09;
const ALIGN_MAX_OFFSET: f64 = std::f64::consts::FRAC_PI_4;

/// The 2005 game's `AlignedToThrust` turn of the aim heading for the stick it
/// aligns to (`stick` = forward, right) and the live forward stick (radians,
/// positive turns left). A stick inside the dead zone gives no turn. A stick
/// pointing backward (live forward below zero) is mirrored into the front
/// half. Within 45 degrees of straight ahead the turn follows the stick
/// exactly, beyond that it stops at 45 degrees.
fn aligned_turn(stick: [f64; 2], live_forward: f64) -> f64 {
    let [mut forward, right] = stick;
    if forward * forward + right * right < ALIGN_THRUST_DEADZONE_SQ {
        return 0.0;
    }
    let mut left = -right;
    if live_forward < 0.0 {
        forward = -forward;
        left = -left;
    }
    if left >= forward {
        ALIGN_MAX_OFFSET
    } else if -left >= forward {
        -ALIGN_MAX_OFFSET
    } else {
        v8math::atan2(left, forward)
    }
}

/// Derive stamina-coordinated tuning, raw actions/input, and a same-tick
/// [`move2::MovementOverride`] from a settled [`StepOutput`].
///
/// Guarantees:
/// - `override_.input_lock` passes `move2::InputLockMask(out.input_lock.0)`
///   directly to the 2005 mover so raw `Jump`, `Crouch`/`Roll`, `Sprint`,
///   `Thrust`, `Strafe`, and `Turn`/`Pitch` controls are filtered before
///   posture decisions.
/// - Authored `Posture("Jump")` and `Posture("Roll")` overrides emit
///   `force_jump` / `force_roll` independently of raw input locks and zero
///   duplicate movement stamina costs (`energy_cost_jump`,
///   `energy_cost_jump_sprint`, `energy_cost_roll`) when the combo transition
///   already charged stamina.
/// - `AnimatedMove` world XZ velocity on the ground or `air_accel_xz` while
///   airborne (ramped by `out.movement.velocity_blend`), `lock_locomotion`,
///   latched `gravity_scale`, `vertical_impulse`, `end_jet`, and
///   `AlignedToThrust` run inside the 2005 mover's single displacement and
///   collision pass without standing braking or slope pull eating the override.
///   `AlignedToThrust` aligns `AnimatedMove` velocity within the forward
///   hemisphere (clamped to `+-45` degrees) without overwriting
///   `mover.aim_yaw`; [`StepOutput::swing_turn`] carries the same turn for
///   the swing and the drawn body.
pub fn prepare_movement_hook(
    base_tuning: &move2::Tuning,
    base_input: &MoveInput,
    base_actions: move2::Actions,
    mover: &Mover,
    combo_input: &ComboInput,
    out: &StepOutput,
) -> PreparedMovementStep {
    if !out.active {
        return PreparedMovementStep {
            tuning: *base_tuning,
            input: *base_input,
            actions: base_actions,
            override_: move2::MovementOverride::default(),
        };
    }

    let (tuning, actions) = prepare_movement_step(base_tuning, base_actions, out);
    let mut input = *base_input;
    let mut lock_bits = out.input_lock.0;
    if let Some(posture @ (ComboPosture::Stand | ComboPosture::Crouch)) = out.posture_override {
        // Replace the player's toggle with an absolute stance request. The mover
        // still checks overhead clearance, grounding and body locks.
        input.crouch = (posture == ComboPosture::Crouch)
            != (mover.posture == crate::movement::soldier::Posture::Crouch);
        lock_bits &= !InputLockMask::CROUCH.0;
    }

    let (velocity_xz, air_accel_xz) = match out.movement.animated_move {
        Some(v) => {
            let w = om::clamp(out.movement.velocity_blend, 0.0, 1.0);
            if mover.grounded {
                (
                    Some([
                        mover.velocity.x + (v[0] - mover.velocity.x) * w,
                        mover.velocity.z + (v[2] - mover.velocity.z) * w,
                    ]),
                    None,
                )
            } else {
                (None, Some([v[0] * w, v[2] * w]))
            }
        }
        None => (None, None),
    };

    let force_jump = out.posture_override == Some(ComboPosture::Jump);
    let force_roll = if out.posture_override == Some(ComboPosture::Roll) {
        let f = om::clamp(combo_input.forward, -1.0, 1.0);
        let s = om::clamp(combo_input.right, -1.0, 1.0);
        Some(if f >= 0.0 && v8math::abs(s) <= f {
            move2::RollDirection::Forward
        } else if s < 0.0 {
            move2::RollDirection::Left
        } else {
            move2::RollDirection::Right
        })
    } else {
        None
    };

    let override_ = move2::MovementOverride {
        velocity_xz,
        air_accel_xz,
        vertical_impulse: out.movement.vertical_impulse,
        aim_yaw: None,
        lock_locomotion: out.movement.lock_locomotion,
        gravity_scale: out.movement.gravity_scale,
        gravity_velocity_target: None,
        input_lock: move2::InputLockMask(lock_bits),
        energy_rate: out.energy_override_active.then_some(0.0),
        force_jump,
        end_jet: force_jump,
        force_roll,
    };

    PreparedMovementStep {
        tuning,
        input,
        actions,
        override_,
    }
}

/// Run one coupled 2005 hero tick: step the combo FSM, coordinate stamina
/// charges and overrides through [`move2::State::apply_energy_rate`] and
/// [`move2::State::charge`], derive a same-tick [`move2::MovementOverride`] via
/// [`prepare_movement_hook`], and advance the 2005 soldier mover through
/// [`crate::movement::advance_profiled`].
#[allow(clippy::too_many_arguments)]
pub fn step_soldier_combo_and_move(
    machine: &ComboMachine,
    combo_state: &mut ComboState,
    combo_input: &ComboInput,
    mover: &mut Mover,
    move_state: &mut move2::State,
    base_tuning: &move2::Tuning,
    base_input: &MoveInput,
    base_actions: move2::Actions,
    scene: &mut Scene<'_>,
    dt: f64,
) -> Result<(StepOutput, bool), ProfileError> {
    let moving = mover.velocity.x * mover.velocity.x + mover.velocity.z * mover.velocity.z >= 1.0;
    let body = BodyContext::from_mover(mover, move_state, moving);

    // The mover validates and commits its body atomically. Stage both per-life
    // states until it accepts the effective inputs and completes collision.
    let mut next_combo = *combo_state;
    let mut next_life = *move_state;
    mover.validate_swbf2(
        base_input,
        dt,
        &move2::Step {
            actions: base_actions,
            tuning: base_tuning,
            state: &mut next_life,
            override_: None,
        },
    )?;
    if next_life.tumble.is_some() || mover.air == crate::movement::soldier::Air::Thrown {
        next_combo = ComboState::new(machine);
        let permission = crate::movement::advance_profiled(
            mover,
            base_input,
            dt,
            scene,
            Some(move2::Step {
                actions: move2::Actions::default(),
                tuning: base_tuning,
                state: &mut next_life,
                override_: None,
            }),
        )?;
        *combo_state = next_combo;
        *move_state = next_life;
        return Ok((StepOutput::default(), permission));
    }
    let mut filtered_combo = *combo_input;
    let was_locked = mover.lock > 0.0;
    if was_locked {
        let locked_buttons = ComboButton::Fire.bit()
            | ComboButton::FireSecondary.bit()
            | ComboButton::Reload.bit();
        filtered_combo.buttons_held &= !locked_buttons;
        filtered_combo.buttons_pressed &= !locked_buttons;
    }
    let out = step(
        GameFamily::Swbf2,
        machine,
        &mut next_combo,
        &filtered_combo,
        &body,
        &mut next_life,
        base_tuning,
        false,
        dt,
    );
    let prep = prepare_movement_hook(
        base_tuning,
        base_input,
        base_actions,
        mover,
        &filtered_combo,
        &out,
    );
    let permission = crate::movement::advance_profiled(
        mover,
        &prep.input,
        dt,
        scene,
        Some(move2::Step {
            actions: prep.actions,
            tuning: &prep.tuning,
            state: &mut next_life,
            override_: Some(prep.override_),
        }),
    )?;
    if !was_locked && mover.lock > 0.0 {
        next_combo.button_history.prune_through(next_combo.eval_time);
    }
    *combo_state = next_combo;
    *move_state = next_life;
    Ok((out, permission))
}

struct EvalContext<'a> {
    posture_time: f64,
    life: &'a move2::State,
    input: &'a ComboInput,
    body: &'a BodyContext,
    history: &'a ButtonEventHistory,
}

#[derive(Clone, Copy)]
enum Scan {
    Interrupt(f64),
    Carried,
    Expiry,
}

struct Candidate {
    target: StateId,
    cost: f64,
    time: f64,
    consumes: bool,
    unconditional: bool,
}

fn choose_transition(
    state: &ComboStateDef,
    origin: f64,
    reference: f64,
    end: Option<f64>,
    scan: Scan,
    ctx: &EvalContext<'_>,
    denied_events: &mut u8,
) -> Option<Candidate> {
    let mut best: Option<Candidate> = None;
    for tr in &state.transitions {
        let affordable = ctx.life.can_charge(tr.energy_cost);
        let mut consider = |time, consumes, unconditional| {
            if best.as_ref().is_none_or(|old| time < old.time) {
                best = Some(Candidate {
                    target: tr.target,
                    cost: tr.energy_cost,
                    time,
                    consumes,
                    unconditional,
                });
            }
        };
        if affordable && tr.branches.is_empty() && matches!(scan, Scan::Expiry) {
            consider(origin + reference, false, true);
        }
        for branch in &tr.branches {
            match scan {
                Scan::Interrupt(age) => {
                    if end.is_some() && branch.break_rule.is_none() {
                        continue;
                    }
                    if branch
                        .break_rule
                        .is_some_and(|rule| age + TIME_EPS < rule.delay_seconds())
                    {
                        continue;
                    }
                }
                Scan::Carried => {
                    if !branch
                        .time_end
                        .zip(end)
                        .is_some_and(|(point, e)| point.resolve(end) > e)
                    {
                        continue;
                    }
                }
                Scan::Expiry => {}
            }
            if let Some(time) =
                branch_input_time(branch, origin, reference, end, ctx.history.now, ctx)
            {
                if affordable
                    && branch
                        .energy
                        .is_none_or(|pred| pred.test(available_stamina(ctx.life)))
                {
                    consider(
                        time,
                        !branch.buttons.is_empty() || branch.buttons_pressed_any,
                        false,
                    );
                } else {
                    // An otherwise eligible physical edge was refused. Retaining it
                    // until stamina recovers would start an action without a new press.
                    for (i, event) in ctx.history.events().iter().enumerate() {
                        if event.event_time <= time
                            && (branch.buttons.iter().any(|b| b.button == event.button)
                                || (branch.buttons_pressed_any && event.flags & 3 == 0))
                        {
                            *denied_events |= 1 << i;
                        }
                    }
                }
            }
        }
    }
    best
}

fn branch_input_time(
    cond: &ConditionGroup,
    state_origin: f64,
    ref_eval_time: f64,
    end_time: Option<f64>,
    cur_abs_now: f64,
    ctx: &EvalContext<'_>,
) -> Option<f64> {
    let ref_end = end_time.unwrap_or(ref_eval_time);
    let win_start_rel = cond.time_start.map_or(0.0, |tp| tp.resolve(Some(ref_end)));
    let win_end_rel = cond
        .time_end
        .map_or(ref_end, |tp| tp.resolve(Some(ref_end)));
    let win_start = state_origin + win_start_rel;
    let ref_abs = state_origin + ref_eval_time;
    if ref_eval_time + TIME_EPS < win_start_rel || ref_eval_time > win_end_rel + TIME_EPS {
        return None;
    }
    let search_hi = v8math::min(ref_abs, cur_abs_now);

    if search_hi + TIME_EPS < win_start {
        return None;
    }
    if let Some(cmp) = cond.time_in_posture {
        if !cmp.test(ctx.posture_time) {
            return None;
        }
    }
    if let Some(posture) = cond.posture {
        if !posture.matches(ctx.body.posture) {
            return None;
        }
    }
    if let Some(cmp) = cond.thrust {
        if !cmp.test(ctx.input.thrust_magnitude()) {
            return None;
        }
    }
    if let Some((lo, hi)) = cond.thrust_angle_deg {
        if ctx.input.thrust_magnitude() <= 1e-6 {
            return None;
        }
        let angle = ctx.input.thrust_angle_deg();
        if !angle_in_range_deg(angle, lo, hi) {
            return None;
        }
    }
    if let Some(cmp) = cond.velocity_y {
        if !cmp.test(ctx.body.velocity_y) {
            return None;
        }
    }

    let mut matched_at = win_start;
    if cond.buttons_pressed_any {
        let t = ctx.history.earliest_any_press(win_start, search_hi)?;
        matched_at = v8math::max(matched_at, t);
    }
    for btn in &cond.buttons {
        let t = ctx
            .history
            .earliest_match(btn.button, btn.mode, win_start, search_hi, ref_abs)?;
        matched_at = v8math::max(matched_at, t);
    }
    Some(matched_at)
}
