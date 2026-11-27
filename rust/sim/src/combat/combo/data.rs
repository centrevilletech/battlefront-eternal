//! Compact, validated Battlefront II melee combo state machine definitions
//! built once at battle load from prepared `combos.json` trees.
//!
//! No retail game data or hero tables live here. Runtime definitions come from
//! prepared content and are validated for finite numbers, bounded edge indices,
//! and resolved state references before any tick executes.

use super::deflect::DeflectDirection;
use super::{HitHistory, COMBO_FIXED_DT, MAX_ACTIVE_ATTACKS, MAX_ACTIVE_HIT_LISTS, TIME_EPS};
use crate::movement::soldier::{swbf2 as move2, Air, Posture};
use serde_json::Value;
use std::fmt;

/// Nominal animation frame rate used by `comb` frame counts and `zaa_` clips.
pub const COMBO_ANIM_FPS: f64 = 30.0;

/// Maximum number of distinct blade/attack edges supported per melee weapon.
pub const MAX_BLADE_EDGES: usize = 4;

/// Sentinel for an unused transition slot (`u16::MAX`).
pub const NO_TRANSITION: u16 = u16::MAX;

/// Sentinel stored in [`super::ComboState::grace_state`] when no carry-over
/// grace window from a completed state is active.
pub const NO_STATE: u16 = u16::MAX;

/// Index into [`ComboMachine::states`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct StateId(pub u16);

/// Index into [`ComboMachine::animations`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClipId(pub u16);

/// Unique hit-deduplication group id shared across `Attack` blocks that carry
/// the same nonzero low-20-bit `AttackId` hash (even across adjacent states).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttackGroupId(pub u16);

/// Case-folded 32-bit FNV-1a hash used by 2005 config identifiers (ORs each
/// byte with `0x20` before XOR and prime multiply; empty string hashes to `0`).
pub fn zero_fnv1a_lower(name: &str) -> u32 {
    if name.is_empty() {
        return 0;
    }
    let mut h = 0x811c_9dc5u32;
    for b in name.bytes() {
        h = (h ^ ((b | 0x20) as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

/// Low-20-bit shared attack group key derived from an authored `AttackId`.
/// Zero denotes an unshared attack record.
#[inline]
pub fn attack_shared_id(name: &str) -> u32 {
    zero_fnv1a_lower(name) & 0x000F_FFFF
}

/// Pack a unique per-record attack key `(state_id << 16) | attack_index`.
#[inline]
pub const fn pack_attack_key(state_id: StateId, attack_index: u16) -> u32 {
    ((state_id.0 as u32) << 16) | (attack_index as u32)
}

/// Index into [`ComboMachine::sounds`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SoundId(pub u16);

/// Movement posture vocabulary referenced by combo `Posture` conditions and
/// state overrides.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComboPosture {
    #[default]
    Stand = 0,
    Crouch = 1,
    Sprint = 2,
    Jump = 3,
    Jet = 4,
    Roll = 5,
    Tumble = 6,
}

impl ComboPosture {
    pub const fn bit(self) -> u8 {
        1u8 << (self as u8)
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Stand),
            1 => Some(Self::Crouch),
            2 => Some(Self::Sprint),
            3 => Some(Self::Jump),
            4 => Some(Self::Jet),
            5 => Some(Self::Roll),
            6 => Some(Self::Tumble),
            _ => None,
        }
    }

    /// Derive the combo posture from the 2005 soldier movement solver state.
    pub fn from_movement(posture: Posture, air: Air, state: &move2::State) -> Self {
        match air {
            Air::Jet => Self::Jet,
            Air::Thrown => Self::Tumble,
            Air::Jump | Air::Fall => Self::Jump,
            Air::None => {
                if state.roll.is_some() {
                    Self::Roll
                } else if state.sprint_time.is_some() {
                    Self::Sprint
                } else if posture == Posture::Crouch {
                    Self::Crouch
                } else {
                    Self::Stand
                }
            }
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "stand" => Some(Self::Stand),
            "crouch" => Some(Self::Crouch),
            "sprint" => Some(Self::Sprint),
            "jump" => Some(Self::Jump),
            "jet" => Some(Self::Jet),
            "roll" => Some(Self::Roll),
            "tumble" => Some(Self::Tumble),
            _ => None,
        }
    }
}

/// Bitmask over [`ComboPosture`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PostureMask(pub u8);

impl PostureMask {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(0x7F);

    pub const fn from_posture(p: ComboPosture) -> Self {
        Self(p.bit())
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, p: ComboPosture) -> bool {
        (self.0 & p.bit()) != 0
    }
}

/// Posture predicate supporting positive tokens, `"Any"`, and `"!Posture"`
/// exclusions (for example `("Any", "!Stand", "!Jump", "!Jet")`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostureCondition {
    pub include: PostureMask,
    pub exclude: PostureMask,
}

impl PostureCondition {
    pub const fn any_of(include: PostureMask) -> Self {
        Self {
            include,
            exclude: PostureMask::NONE,
        }
    }

    pub const fn matches(self, posture: ComboPosture) -> bool {
        self.include.contains(posture) && !self.exclude.contains(posture)
    }
}

/// Logical input buttons tested by combo `Button(name, mode)` conditions.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComboButton {
    Fire = 0,
    FireSecondary = 1,
    Jump = 2,
    Crouch = 3,
    Sprint = 4,
    Reload = 5,
}

impl ComboButton {
    pub const COUNT: usize = 6;

    pub const fn bit(self) -> u8 {
        1u8 << (self as u8)
    }

    pub const fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::Fire),
            1 => Some(Self::FireSecondary),
            2 => Some(Self::Jump),
            3 => Some(Self::Crouch),
            4 => Some(Self::Sprint),
            5 => Some(Self::Reload),
            _ => None,
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "fire" => Some(Self::Fire),
            "firesecondary" => Some(Self::FireSecondary),
            "jump" => Some(Self::Jump),
            "crouch" => Some(Self::Crouch),
            "sprint" => Some(Self::Sprint),
            "reload" => Some(Self::Reload),
            _ => None,
        }
    }
}

/// Trigger evaluation mode in `Button(button, mode)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonMode {
    Press,
    Tap,
    Hold,
    HoldRelease,
    DoublePress,
    DoubleTap,
    DoubleHold,
    DoubleHoldRelease,
    Release,
    Down,
    Up,
}

impl ButtonMode {
    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "press" => Some(Self::Press),
            "tap" => Some(Self::Tap),
            "hold" => Some(Self::Hold),
            "holdrelease" => Some(Self::HoldRelease),
            "doublepress" => Some(Self::DoublePress),
            "doubletap" => Some(Self::DoubleTap),
            "doublehold" => Some(Self::DoubleHold),
            "doubleholdrelease" => Some(Self::DoubleHoldRelease),
            "release" => Some(Self::Release),
            "down" => Some(Self::Down),
            "up" => Some(Self::Up),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonCondition {
    pub button: ComboButton,
    pub mode: ButtonMode,
}

/// Bitflags for `InputLock` actions blocked while a combo state is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputLockMask(pub u16);

impl InputLockMask {
    pub const NONE: Self = Self(0);
    pub const FIRE: Self = Self(1 << 0);
    pub const FIRE_SECONDARY: Self = Self(1 << 1);
    pub const JUMP: Self = Self(1 << 2);
    pub const CROUCH: Self = Self(1 << 3);
    pub const SPRINT: Self = Self(1 << 4);
    pub const RELOAD: Self = Self(1 << 5);
    pub const THRUST: Self = Self(1 << 6);
    pub const STRAFE: Self = Self(1 << 7);
    pub const TURN: Self = Self(1 << 8);
    /// Action and locomotion locks covered by `InputLock("All")`. Turning stays
    /// unlocked unless requested explicitly; both keyboard and mouse turn speed
    /// are scaled by `TurnFactor`.
    pub const ALL: Self = Self(0x00FF);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// The 2005 game's `Thrust` token stands for the whole movement stick:
    /// locking it stops walking forward, backward and sideways, and `!Thrust`
    /// frees all of it. The game has no `Strafe` token and skips it, so it
    /// changes nothing here either.
    fn parse_token(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "all" => Some(Self::ALL),
            "fire" => Some(Self::FIRE),
            "firesecondary" => Some(Self::FIRE_SECONDARY),
            "jump" => Some(Self::JUMP),
            "crouch" => Some(Self::CROUCH),
            "sprint" => Some(Self::SPRINT),
            "reload" => Some(Self::RELOAD),
            "thrust" => Some(Self::THRUST.union(Self::STRAFE)),
            "strafe" => Some(Self::NONE),
            "turn" => Some(Self::TURN),
            _ => None,
        }
    }
}

/// Timed or full-state input lock declared by a state's `InputLock(...)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputLockDef {
    /// `None` locks for the whole state; `Some(seconds)` locks while
    /// `state_time < seconds`.
    pub duration: Option<f64>,
    pub blocked: InputLockMask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarOp {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
}

impl ScalarOp {
    fn parse(token: &str) -> Option<Self> {
        match token.trim() {
            ">" => Some(Self::Gt),
            ">=" => Some(Self::Ge),
            "<" => Some(Self::Lt),
            "<=" => Some(Self::Le),
            "==" | "=" => Some(Self::Eq),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScalarCmp {
    pub op: ScalarOp,
    pub value: f64,
}

impl ScalarCmp {
    pub fn test(self, lhs: f64) -> bool {
        match self.op {
            ScalarOp::Gt => lhs > self.value,
            ScalarOp::Ge => lhs >= self.value,
            ScalarOp::Lt => lhs < self.value,
            ScalarOp::Le => lhs <= self.value,
            ScalarOp::Eq => lhs == self.value,
        }
    }
}

/// Point in time relative to either the start or the end of a state's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimePoint {
    FromStart(f64),
    FromEnd(f64),
}

impl TimePoint {
    pub fn resolve(self, end_time: Option<f64>) -> f64 {
        match self {
            Self::FromStart(s) => s,
            Self::FromEnd(offset) => end_time.unwrap_or(0.0) + offset,
        }
    }
}

/// Interrupt rule on a transition branch (`Break`). Without `Break`, a matched
/// transition is queued and fires only when the current state completes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BreakRule {
    Immediate,
    AfterSeconds(f64),
}

impl BreakRule {
    pub const fn delay_seconds(self) -> f64 {
        match self {
            Self::Immediate => 0.0,
            Self::AfterSeconds(s) => s,
        }
    }
}

/// Conjunction (`If` or `Or`) of predicates gating a transition or `AnimatedMove`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConditionGroup {
    pub break_rule: Option<BreakRule>,
    pub time_start: Option<TimePoint>,
    pub time_end: Option<TimePoint>,
    pub time_in_posture: Option<ScalarCmp>,
    pub buttons: Vec<ButtonCondition>,
    pub buttons_pressed_any: bool,
    pub posture: Option<PostureCondition>,
    pub energy: Option<ScalarCmp>,
    pub thrust: Option<ScalarCmp>,
    pub thrust_angle_deg: Option<(f64, f64)>,
    pub velocity_y: Option<ScalarCmp>,
}

/// Directed edge from one combo state to `target`.
///
/// An empty `branches` list denotes the unconditional fallthrough taken when
/// the state completes. Non-empty `branches` are combined with logical OR
/// (`If` followed by zero or more `Or` blocks).
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionDef {
    pub target: StateId,
    pub energy_cost: f64,
    pub branches: Vec<ConditionGroup>,
}

/// State lifetime rule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum StateDuration {
    /// Runs until interrupted by a `Break` transition (`Duration(0.0)`).
    Indefinite,
    /// Completes once the state's animation clock reaches this time in seconds.
    Seconds(f64),
    /// Completes when the state's animation clip finishes (or immediately after
    /// `MustShowOneFrame` when no clip is attached).
    #[default]
    ClipLength,
}

/// Active damage time window for one [`AttackDef`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DamageWindow {
    /// Absolute time range `[start, end]` in seconds on the animation clock.
    Seconds { start: f64, end: f64 },
    /// Animation frame range `[start, end]` at [`COMBO_ANIM_FPS`].
    Frames { start: f64, end: f64 },
    /// Normalized `[start, end]` fraction of the current clip's duration.
    FromAnim { start: f64, end: f64 },
}

impl DamageWindow {
    /// Resolve into `(start_secs, end_secs)` on the state's evaluation clock.
    /// `ref_duration` is the explicit state duration when present, otherwise
    /// the state's animation clip duration.
    pub fn resolve_seconds(self, ref_duration: f64, _anim_fps: f64) -> (f64, f64) {
        match self {
            Self::Seconds { start, end } => (start, end),
            Self::Frames { start, end } => (
                quantize_cs(v8math::max(0.0, start) / COMBO_ANIM_FPS, 0, 1023),
                quantize_cs(v8math::max(0.0, end) / COMBO_ANIM_FPS, 0, 1023),
            ),
            Self::FromAnim { start, end } => (start * ref_duration, end * ref_duration),
        }
    }
}

/// One active melee hit window attached to a combo state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackDef {
    pub attack_group: AttackGroupId,
    /// Unique per-record identifier `(state_id << 16) | attack_index`.
    pub attack_key: u32,
    /// Nonzero low-20-bit hash of `AttackId` when shared across attacks, or `0`
    /// when unshared.
    pub shared_id: u32,
    pub edge: u8,
    pub window: DamageWindow,
    pub damage_length: f64,
    pub damage_width: f64,
    pub damage: f64,
    pub push: f64,
}

/// One directional block-reaction animation declared by `DeflectAnimation(clip, dir)`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeflectAnimDef {
    pub clip_name: Box<str>,
    pub direction: DeflectDirection,
    pub blend_in: f64,
    pub blend_out: f64,
    pub loop_mode: LoopMode,
    pub aim_type: Option<AimType>,
    pub low_res_pose_secs: Option<f64>,
}

/// Blaster and melee deflection zone active during a state (`Deflect`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeflectDef {
    /// Minimum horizontal angle in degrees relative to hero forward (negative = left).
    pub min_angle_deg: f64,
    /// Maximum horizontal angle in degrees relative to hero forward (positive = right).
    pub max_angle_deg: f64,
    /// Extra energy drained per deflected projectile or melee strike.
    pub energy_cost: f64,
    /// Start index into [`ComboMachine::deflect_animations`].
    pub deflect_anim_start: u16,
    /// Count of reaction clips in [`ComboMachine::deflect_animations`].
    pub deflect_anim_len: u16,
    /// Machine-local reaction record for each direction, or `u16::MAX`.
    pub reaction_ids: [u16; 6],
}

impl DeflectDef {
    /// True when the state declares `DeflectAnimation` reaction clips (standing
    /// block); false for attack-time deflect windows such as dash attacks.
    pub const fn has_reactions(&self) -> bool {
        self.deflect_anim_len > 0
    }
}

/// How a state aligns facing with movement input (`AlignedToThrust`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignMode {
    #[default]
    None,
    Initial,
    Continuous,
}

/// State override of the hero's stamina regeneration rate (`EnergyRestoreRate`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum EnergyRateOverride {
    /// Use the class's normal `EnergyRestore` / `EnergyRestoreIdle` rates.
    #[default]
    Default,
    /// Replace stamina rate with a fixed units/second value (0 pauses regen;
    /// negative drains stamina continuously).
    Override(f64),
}

/// Scripted horizontal velocity override (`AnimatedMove`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatedMoveDef {
    pub velocity_z: f64,
    pub velocity_x: f64,
    pub velocity_from_thrust: f64,
    pub velocity_from_strafe: f64,
    pub if_cond: Option<ConditionGroup>,
    pub until_cond: Option<ConditionGroup>,
}

/// Aerial gravity and vertical velocity override (`Gravity`, `GravityVelocityTarget`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GravityOverride {
    pub scale: f64,
    pub velocity_target: Option<f64>,
    pub impulse: bool,
    pub zero_gravity: bool,
}

impl Default for GravityOverride {
    fn default() -> Self {
        Self {
            scale: 1.0,
            velocity_target: None,
            impulse: false,
            zero_gravity: false,
        }
    }
}

/// Timed or entry sound cue declared inside a combo state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundCue {
    pub sound: SoundId,
    /// `None` triggers on state entry; `Some(seconds)` triggers when
    /// `anim_time` crosses `seconds`.
    pub at_secs: Option<f64>,
}

/// Playback loop mode declared by an animation's `Loop(...)` child.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopMode {
    /// Play once and stop at clip end (`Loop("None")`, `Loop(0)`, or absent).
    #[default]
    None,
    /// Wrap continuously back to zero at clip end (`Loop()`, `Loop("Loop")`, or `Loop(1)`).
    Wrap,
    /// Play once and hold the final frame indefinitely (`Loop("FinalFrame")` or `Loop(2)`).
    FinalFrame,
}

impl LoopMode {
    fn parse(vals: &[Value]) -> Result<Self, ComboLoadError> {
        if vals.is_empty() {
            return Ok(Self::Wrap);
        }
        if vals.len() > 1 {
            return Err(ComboLoadError::new("Loop expects at most 1 argument"));
        }
        let first = &vals[0];
        if let Some(s) = first.as_str() {
            if s.eq_ignore_ascii_case("None") {
                return Ok(Self::None);
            }
            if s.eq_ignore_ascii_case("Loop") {
                return Ok(Self::Wrap);
            }
            if s.eq_ignore_ascii_case("FinalFrame") {
                return Ok(Self::FinalFrame);
            }
        }
        if let Some(n) = as_f64(first) {
            if n == 0.0 {
                return Ok(Self::None);
            }
            if n == 1.0 {
                return Ok(Self::Wrap);
            }
            if n == 2.0 {
                return Ok(Self::FinalFrame);
            }
        }
        Err(ComboLoadError::new(format!(
            "Loop unknown mode '{}'",
            first
        )))
    }
}

/// Aiming layer declared by an animation's `AimType(...)` child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AimType {
    None,
    FullBody,
    Torso,
}

impl AimType {
    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "fullbody" => Some(Self::FullBody),
            "torso" => Some(Self::Torso),
            _ => None,
        }
    }
}

/// Target-specific blend time declared by `BlendTimeTo(target_clip, secs)`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendTimeTo {
    pub target_clip: Box<str>,
    pub time_secs: f64,
}

/// Cross-clip synchronization rule declared by `SyncType(clip, mode)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncTypeDef {
    pub sync_clip: Box<str>,
    pub mode: Box<str>,
}

/// Candidate animation clip entry from a hero weapon's prepared
/// `comboBindings[].clips[action]` list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipBindingCandidate<'a> {
    pub bank: &'a str,
    pub frames: u32,
    pub fps: f64,
}

/// Resolve an action's duration in seconds from prepared bank candidates
/// scoped strictly to the character's animation package.
///
/// A clip of `N` frames (`N >= 1`) lasts `(N - 1) / fps` seconds. Priority:
/// 1. `hero_bank` (when present and non-empty), matching `bank` case-insensitively.
/// 2. `package_banks`, checked in package order (typically character `AnimationName`
///    first and shared prefix bank last).
///
/// Candidates belonging to banks outside `package_banks` are
/// never selected.
pub fn resolve_binding_clip_duration(
    hero_bank: Option<&str>,
    package_banks: &[&str],
    candidates: &[ClipBindingCandidate<'_>],
) -> Option<f64> {
    let valid = |c: &&ClipBindingCandidate<'_>| c.frames >= 1 && c.fps.is_finite() && c.fps > 0.0;
    if let Some(hb) = hero_bank
        .filter(|s| !s.is_empty() && package_banks.iter().any(|p| p.eq_ignore_ascii_case(s)))
    {
        if let Some(c) = candidates
            .iter()
            .find(|c| c.bank.eq_ignore_ascii_case(hb) && valid(c))
        {
            return Some((c.frames - 1) as f64 / c.fps);
        }
    }
    for &bank in package_banks {
        if bank.is_empty() {
            continue;
        }
        if let Some(c) = candidates
            .iter()
            .find(|c| c.bank.eq_ignore_ascii_case(bank) && valid(c))
        {
            return Some((c.frames - 1) as f64 / c.fps);
        }
    }
    None
}

/// Animation clip metadata referenced by states.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationMeta {
    pub name: Box<str>,
    pub duration_secs: f64,
    pub blend_in: f64,
    pub blend_out: f64,
    pub loop_mode: LoopMode,
    pub aim_type: Option<AimType>,
    pub blend_time_to: Vec<BlendTimeTo>,
    pub sync_type: Option<SyncTypeDef>,
    pub low_res_pose_secs: Option<f64>,
}

/// Per-state animation layer metadata (primary/upper or split-body lower).
#[derive(Clone, Debug, PartialEq)]
pub struct StateAnimLayer {
    pub clip: ClipId,
    pub blend_in: f64,
    pub blend_out: f64,
    pub loop_mode: LoopMode,
    pub aim_type: Option<AimType>,
    pub blend_time_to: Vec<BlendTimeTo>,
    pub sync_type: Option<SyncTypeDef>,
    pub low_res_pose_secs: Option<f64>,
}

/// Offhand Force-power animation slot declared at combo root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffhandSlot {
    Fire,
    Fire2,
    Charge,
}

impl OffhandSlot {
    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_uppercase().as_str() {
            "FIRE" => Some(Self::Fire),
            "FIRE2" => Some(Self::Fire2),
            "CHARGE" => Some(Self::Charge),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OffhandAnimDef {
    pub clip_name: Box<str>,
    pub slot: OffhandSlot,
    pub blend_in: f64,
    pub blend_out: f64,
    pub loop_mode: LoopMode,
    pub aim_type: Option<AimType>,
}

/// One validated state inside a [`ComboMachine`].
#[derive(Clone, Debug, PartialEq)]
pub struct ComboStateDef {
    pub id: StateId,
    /// Primary (or upper-body) animation clip driving state timing.
    pub animation: Option<ClipId>,
    /// Optional lower-body animation clip for split-body states that declare
    /// two `Animation` children (e.g. `_upper` then `_lower`).
    pub lower_animation: Option<ClipId>,
    /// Per-state presentation metadata for `animation`.
    pub anim_layer: Option<StateAnimLayer>,
    /// Per-state presentation metadata for `lower_animation`.
    pub lower_anim_layer: Option<StateAnimLayer>,
    /// Effective loop mode for this state's primary animation (inherits root
    /// animation `Loop(...)` and allows per-state override).
    pub loop_mode: LoopMode,
    pub duration: StateDuration,
    clip_deadline: f64,
    pub posture: Option<ComboPosture>,
    pub must_show_one_frame: bool,
    pub restart_animation: bool,
    pub energy_restore_rate: EnergyRateOverride,
    pub play_explosion: bool,
    /// Bit `i` set disables blade edge `i`; `0xFF` disables all blades.
    pub turn_off_lightsaber: u8,
    pub aligned_to_thrust: AlignMode,
    pub thrust_factor: f64,
    pub strafe_factor: f64,
    pub turn_factor: f64,
    pub animated_move: Option<AnimatedMoveDef>,
    pub gravity: Option<GravityOverride>,
    pub input_lock: Option<InputLockDef>,
    pub deflect: Option<DeflectDef>,
    pub attacks: Vec<AttackDef>,
    pub sounds: Vec<SoundCue>,
    pub transitions: Vec<TransitionDef>,
}

impl ComboStateDef {
    pub fn new(id: StateId) -> Self {
        Self {
            id,
            animation: None,
            lower_animation: None,
            anim_layer: None,
            lower_anim_layer: None,
            loop_mode: LoopMode::None,
            duration: StateDuration::ClipLength,
            clip_deadline: 0.0,
            posture: None,
            must_show_one_frame: false,
            restart_animation: false,
            energy_restore_rate: EnergyRateOverride::Default,
            play_explosion: false,
            turn_off_lightsaber: 0,
            aligned_to_thrust: AlignMode::None,
            thrust_factor: 1.0,
            strafe_factor: 1.0,
            turn_factor: 1.0,
            animated_move: None,
            gravity: None,
            input_lock: None,
            deflect: None,
            attacks: Vec::new(),
            sounds: Vec::new(),
            transitions: Vec::new(),
        }
    }

    pub(super) fn attack_interval(
        &self,
        attack: &AttackDef,
        clip_len: f64,
        deadline: Option<f64>,
        fps: f64,
    ) -> Option<(f64, f64)> {
        if self.turn_off_lightsaber & (1 << attack.edge) != 0 {
            return None;
        }
        let (start, end) = attack
            .window
            .resolve_seconds(deadline.unwrap_or(clip_len), fps);
        let holds_end = deadline.is_none()
            && self.loop_mode == LoopMode::FinalFrame
            && clip_len > TIME_EPS
            && match attack.window {
                DamageWindow::FromAnim { end, .. } => end >= 1.0 - TIME_EPS,
                _ => end >= clip_len - TIME_EPS,
            };
        Some((start, if holds_end { f64::INFINITY } else { end }))
    }

    pub fn has_attack_group(&self, group: AttackGroupId) -> bool {
        self.attacks.iter().any(|a| a.attack_group == group)
    }
}

/// Per-edge weapon geometry fallback when an `Attack` omits length or width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeSpec {
    pub default_length: f64,
    pub default_width: f64,
}

impl Default for EdgeSpec {
    fn default() -> Self {
        Self {
            default_length: 1.0,
            default_width: 0.1,
        }
    }
}

/// Static melee weapon multipliers resolved from the equipped weapon's ODF.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponMeleeSpec {
    pub combo_hash: u32,
    pub num_edges: u8,
    pub edges: [EdgeSpec; MAX_BLADE_EDGES],
    /// Multipliers ordered as [`crate::combat::impacts::HealthScale`]:
    /// Person, Animal, Droid, Vehicle, Building.
    pub damage_scales: [f64; 5],
    pub shield_scale: f64,
    pub has_explosion: bool,
}

impl Default for WeaponMeleeSpec {
    fn default() -> Self {
        Self {
            combo_hash: 0,
            num_edges: 1,
            edges: [EdgeSpec::default(); MAX_BLADE_EDGES],
            damage_scales: [1.0; 5],
            shield_scale: 1.0,
            has_explosion: false,
        }
    }
}

impl WeaponMeleeSpec {
    /// Validate weapon edge counts, positive finite edge dimensions, and
    /// finite nonnegative damage/shield multipliers before battle load.
    pub fn validate(&self) -> Result<(), ComboLoadError> {
        if self.num_edges == 0 || (self.num_edges as usize) > MAX_BLADE_EDGES {
            return Err(ComboLoadError::new(format!(
                "weapon num_edges {} out of range 1..={MAX_BLADE_EDGES}",
                self.num_edges
            )));
        }
        for (i, edge) in self.edges.iter().enumerate() {
            if i < self.num_edges as usize {
                if !edge.default_length.is_finite() || edge.default_length <= 0.0 {
                    return Err(ComboLoadError::new(format!(
                        "weapon edge {i} default_length must be finite and positive"
                    )));
                }
                if !edge.default_width.is_finite() || edge.default_width <= 0.0 {
                    return Err(ComboLoadError::new(format!(
                        "weapon edge {i} default_width must be finite and positive"
                    )));
                }
            } else {
                check_nonneg(edge.default_length, "weapon inactive edge default_length")?;
                check_nonneg(edge.default_width, "weapon inactive edge default_width")?;
            }
        }
        for (i, &scale) in self.damage_scales.iter().enumerate() {
            check_nonneg(scale, &format!("weapon damage_scales[{i}]"))?;
        }
        check_nonneg(self.shield_scale, "weapon shield_scale")?;
        Ok(())
    }
}

/// Complete, immutable melee combo FSM built at battle load.
#[derive(Clone, Debug, PartialEq)]
pub struct ComboMachine {
    pub name_hash: u32,
    pub anim_fps: f64,
    pub idle_state: StateId,
    pub catch_state: Option<StateId>,
    pub states: Vec<ComboStateDef>,
    pub state_names: Vec<Box<str>>,
    pub animations: Vec<AnimationMeta>,
    pub deflect_animations: Vec<DeflectAnimDef>,
    pub offhand_animations: Vec<OffhandAnimDef>,
    pub throw_animation: Option<Box<str>>,
    pub sounds: Vec<Box<str>>,
    pub attack_groups: Vec<Box<str>>,
    pub attack_group_shared_ids: Vec<u32>,
}

/// Validation or parse failure when loading prepared `combos.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboLoadError {
    pub message: String,
}

impl ComboLoadError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ComboLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ComboLoadError {}

impl ComboMachine {
    #[inline]
    pub fn state(&self, id: StateId) -> &ComboStateDef {
        &self.states[id.0 as usize]
    }

    #[inline]
    pub fn state_name(&self, id: StateId) -> &str {
        &self.state_names[id.0 as usize]
    }

    #[inline]
    pub fn animation(&self, id: ClipId) -> &AnimationMeta {
        &self.animations[id.0 as usize]
    }

    #[inline]
    pub fn sound_name(&self, id: SoundId) -> &str {
        &self.sounds[id.0 as usize]
    }

    /// Active turn-rate multiplier for the settled combo state, or `0.0` while
    /// an `InputLock("Turn")` window is active.
    pub fn active_turn_factor(&self, state: &super::ComboState) -> f64 {
        let Some(settled) = self.states.get(state.state.0 as usize) else {
            return 1.0;
        };
        if let Some(lock) = settled.input_lock {
            if lock.blocked.contains(InputLockMask::TURN)
                && lock
                    .duration
                    .is_none_or(|d| state.state_time < d - TIME_EPS)
            {
                return 0.0;
            }
        }
        settled.turn_factor
    }

    /// Return the authored `DeflectAnimation` reaction clips for one `DeflectDef`.
    #[inline]
    pub fn deflect_animations_for(&self, deflect: &DeflectDef) -> &[DeflectAnimDef] {
        let start = deflect.deflect_anim_start as usize;
        let end = start.saturating_add(deflect.deflect_anim_len as usize);
        self.deflect_animations.get(start..end).unwrap_or(&[])
    }

    /// Look up a state by exact name (case-insensitive fallback).
    pub fn find_state(&self, name: &str) -> Option<StateId> {
        self.state_names
            .iter()
            .position(|s| s.as_ref() == name || s.eq_ignore_ascii_case(name))
            .map(|i| StateId(i as u16))
    }

    /// Resolve a state's end time in seconds on the animation clock, or `None`
    /// when the state runs indefinitely (`Duration(0.0)` or a `ClipLength`
    /// state whose animation loops or holds its final frame).
    pub fn state_end_time(&self, state: &ComboStateDef) -> Option<f64> {
        match state.duration {
            StateDuration::Indefinite => None,
            StateDuration::Seconds(s) => Some(s),
            StateDuration::ClipLength => {
                if state.loop_mode != LoopMode::None {
                    return None;
                }
                Some(state.clip_deadline)
            }
        }
    }

    fn validate_attack_capacity(&self, state: &ComboStateDef) -> Result<(), ComboLoadError> {
        if state.attacks.len() <= MAX_ACTIVE_ATTACKS {
            return Ok(());
        }
        let clip_len = state
            .animation
            .map_or(0.0, |id| self.animations[id.0 as usize].duration_secs);
        let deadline = self.state_end_time(state);
        let mut windows = Vec::new();
        for attack in &state.attacks {
            let Some((start, end)) =
                state.attack_interval(attack, clip_len, deadline, self.anim_fps)
            else {
                continue;
            };
            // The first advance must cross the start. The last may start exactly at the end.
            let first_tick = v8math::floor((start + TIME_EPS) / COMBO_FIXED_DT);
            let last_tick = v8math::floor((end + TIME_EPS) / COMBO_FIXED_DT);
            let previous = first_tick * COMBO_FIXED_DT;
            if deadline.is_some_and(|d| start + TIME_EPS >= d || previous + TIME_EPS >= d) {
                continue;
            }
            let sample_start = v8math::min(v8math::max(previous, start), end);
            windows.push((sample_start, first_tick, last_tick, end, attack));
        }
        windows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut ends = Vec::with_capacity(MAX_ACTIVE_ATTACKS);
        let mut lists = HitHistory::new();
        for (sample_start, first_tick, last_tick, end, attack) in windows {
            ends.retain(|last| *last >= first_tick);
            if ends.len() == MAX_ACTIVE_ATTACKS {
                return Err(ComboLoadError::new(format!(
                    "state '{}' exceeds {MAX_ACTIVE_ATTACKS} attack windows in one tick",
                    self.state_name(state.id)
                )));
            }
            ends.push(last_tick);
            lists.retire(sample_start);
            if lists
                .ensure_list(attack.attack_key, attack.shared_id, end)
                .is_none()
            {
                return Err(ComboLoadError::new(format!(
                    "state '{}' exceeds {MAX_ACTIVE_HIT_LISTS} live attack hit lists",
                    self.state_name(state.id)
                )));
            }
        }
        Ok(())
    }

    /// Verify every reference, range, and floating-point value before simulation.
    pub fn validate(&self) -> Result<(), ComboLoadError> {
        if !self.anim_fps.is_finite() || self.anim_fps <= 0.0 {
            return Err(ComboLoadError::new("anim_fps must be finite and positive"));
        }
        if self.states.is_empty() {
            return Err(ComboLoadError::new("combo machine has no states"));
        }
        if self.states.len() != self.state_names.len() {
            return Err(ComboLoadError::new("state_names length mismatch"));
        }
        if self.states.len() >= NO_TRANSITION as usize {
            return Err(ComboLoadError::new("combo machine exceeds u16 state limit"));
        }
        if (self.idle_state.0 as usize) >= self.states.len() {
            return Err(ComboLoadError::new("idle_state out of bounds"));
        }
        if let Some(catch) = self.catch_state {
            if (catch.0 as usize) >= self.states.len() {
                return Err(ComboLoadError::new("catch_state out of bounds"));
            }
        }
        for anim in &self.animations {
            check_nonneg(anim.duration_secs, "animation duration")?;
            check_nonneg(anim.blend_in, "animation blend_in")?;
            check_nonneg(anim.blend_out, "animation blend_out")?;
            for bt in &anim.blend_time_to {
                check_nonneg(bt.time_secs, "animation blend_time_to")?;
            }
            if let Some(t) = anim.low_res_pose_secs {
                check_nonneg(t, "animation low_res_pose")?;
            }
        }
        for (idx, st) in self.states.iter().enumerate() {
            if st.id.0 as usize != idx {
                return Err(ComboLoadError::new(format!(
                    "state id mismatch at index {idx}"
                )));
            }
            let st_name = self.state_names[idx].as_ref();
            for clip in [st.animation, st.lower_animation].into_iter().flatten() {
                let Some(meta) = self.animations.get(clip.0 as usize) else {
                    return Err(ComboLoadError::new(format!(
                        "combo 0x{:08x} state '{st_name}' clip out of bounds",
                        self.name_hash
                    )));
                };
                if !meta.duration_secs.is_finite() || meta.duration_secs <= 0.0 {
                    return Err(ComboLoadError::new(format!(
                        "combo 0x{:08x} state '{st_name}' clip '{}' has unresolved or zero duration",
                        self.name_hash, meta.name
                    )));
                }
            }
            if let StateDuration::Seconds(s) = st.duration {
                check_nonneg(s, "state duration")?;
            }
            if let EnergyRateOverride::Override(rate) = st.energy_restore_rate {
                check_finite(rate, "energy_restore_rate")?;
            }
            check_nonneg(st.thrust_factor, "thrust_factor")?;
            check_nonneg(st.strafe_factor, "strafe_factor")?;
            check_nonneg(st.turn_factor, "turn_factor")?;
            if let Some(am) = &st.animated_move {
                check_finite(am.velocity_z, "animated_move.velocity_z")?;
                check_finite(am.velocity_x, "animated_move.velocity_x")?;
                check_finite(
                    am.velocity_from_thrust,
                    "animated_move.velocity_from_thrust",
                )?;
                check_finite(
                    am.velocity_from_strafe,
                    "animated_move.velocity_from_strafe",
                )?;
                if let Some(c) = &am.if_cond {
                    validate_condition(c)?;
                }
                if let Some(c) = &am.until_cond {
                    validate_condition(c)?;
                }
            }
            if let Some(grav) = &st.gravity {
                check_finite(grav.scale, "gravity.scale")?;
                if let Some(v) = grav.velocity_target {
                    check_finite(v, "gravity.velocity_target")?;
                }
            }
            if let Some(lock) = &st.input_lock {
                if let Some(d) = lock.duration {
                    check_nonneg(d, "input_lock.duration")?;
                }
            }
            if let Some(deflect) = &st.deflect {
                check_finite(deflect.min_angle_deg, "deflect.min_angle_deg")?;
                check_finite(deflect.max_angle_deg, "deflect.max_angle_deg")?;
                check_nonneg(deflect.energy_cost, "deflect.energy_cost")?;
                let end = (deflect.deflect_anim_start as usize)
                    .saturating_add(deflect.deflect_anim_len as usize);
                if end > self.deflect_animations.len() {
                    return Err(ComboLoadError::new("deflect animation range out of bounds"));
                }
            }
            for atk in &st.attacks {
                if (atk.attack_group.0 as usize) >= self.attack_groups.len() {
                    return Err(ComboLoadError::new("attack_group out of bounds"));
                }
                if (atk.edge as usize) >= MAX_BLADE_EDGES {
                    return Err(ComboLoadError::new(format!(
                        "attack edge {} out of bounds",
                        atk.edge
                    )));
                }
                match atk.window {
                    DamageWindow::Seconds { start, end } | DamageWindow::Frames { start, end } => {
                        check_finite(start, "attack window start")?;
                        check_finite(end, "attack window end")?;
                        if end < start {
                            return Err(ComboLoadError::new("attack window end < start"));
                        }
                    }
                    DamageWindow::FromAnim { start, end } => {
                        check_finite(start, "attack window start")?;
                        check_finite(end, "attack window end")?;
                        if end < start {
                            return Err(ComboLoadError::new("attack window end < start"));
                        }
                        let has_ref_duration = st.animation.is_some()
                            || matches!(st.duration, StateDuration::Seconds(s) if s > 0.0);
                        if !has_ref_duration {
                            return Err(ComboLoadError::new(format!(
                                "combo 0x{:08x} state '{st_name}' FromAnim attack requires a state clip or explicit positive duration",
                                self.name_hash
                            )));
                        }
                    }
                }
                check_nonneg(atk.damage_length, "attack damage_length")?;
                check_nonneg(atk.damage_width, "attack damage_width")?;
                check_nonneg(atk.damage, "attack damage")?;
                check_finite(atk.push, "attack push")?;
            }
            self.validate_attack_capacity(st)?;
            for cue in &st.sounds {
                if (cue.sound.0 as usize) >= self.sounds.len() {
                    return Err(ComboLoadError::new("sound id out of bounds"));
                }
                if let Some(secs) = cue.at_secs {
                    check_nonneg(secs, "sound cue time")?;
                }
            }
            if st.transitions.len() >= NO_TRANSITION as usize {
                return Err(ComboLoadError::new("too many transitions in state"));
            }
            for tr in &st.transitions {
                if (tr.target.0 as usize) >= self.states.len() {
                    return Err(ComboLoadError::new("transition target out of bounds"));
                }
                check_nonneg(tr.energy_cost, "transition energy_cost")?;
                for br in &tr.branches {
                    validate_condition(br)?;
                }
            }
        }
        Ok(())
    }

    /// Parse and validate one weapon's combo machine from the prepared
    /// `combos.json` entry format (`nameHash`, `offhandAnimations`,
    /// `animations`, `throwAnimations`, `states`).
    ///
    /// `weapon` supplies per-edge blade fallback dimensions (`default_length`,
    /// `default_width`) when an `Attack` omits `DamageLength`/`DamageDistance`
    /// or `DamageWidth`, or specifies `"FromEdge"`. `anim_fps` is the animation
    /// bank frame rate (typically `30.0`) used to convert `"Frames"` values.
    /// `clip_duration` resolves an animation action name for this hero (using
    /// hero-bank priority over shared fallback banks; see
    /// [`resolve_binding_clip_duration`]) to its positive duration in seconds.
    /// Every state `Animation` clip must resolve to a positive finite duration.
    pub fn from_json_value(
        value: &Value,
        weapon: &WeaponMeleeSpec,
        anim_fps: f64,
        mut clip_duration: impl FnMut(&str) -> Option<f64>,
    ) -> Result<Self, ComboLoadError> {
        weapon.validate()?;
        if !anim_fps.is_finite() || anim_fps <= 0.0 {
            return Err(ComboLoadError::new("anim_fps must be finite and positive"));
        }
        let obj = value
            .as_object()
            .ok_or_else(|| ComboLoadError::new("combo entry must be a JSON object"))?;

        let name_hash = match obj.get("nameHash").and_then(Value::as_str) {
            Some(hex) => parse_hex_u32(hex)
                .ok_or_else(|| ComboLoadError::new(format!("invalid nameHash '{hex}'")))?,
            None => 0,
        };

        let mut animations: Vec<AnimationMeta> = Vec::new();
        for rec in opt_array_obj(obj, "animations", "combo")? {
            let name = first_str(rec.get("values"))
                .ok_or_else(|| ComboLoadError::new("Animation missing clip name"))?;
            let resolved = resolve_clip_secs(
                name,
                rec.get("duration").and_then(as_f64),
                &mut clip_duration,
            )?;
            let mut meta = AnimationMeta {
                name: name.into(),
                duration_secs: resolved.unwrap_or(0.0),
                blend_in: 0.0,
                blend_out: 0.0,
                loop_mode: LoopMode::None,
                aim_type: None,
                blend_time_to: Vec::new(),
                sync_type: None,
                low_res_pose_secs: None,
            };
            let children = opt_array(rec, "children", "Animation")?;
            apply_anim_children(&mut meta, children, anim_fps)?;
            intern_anim(&mut animations, meta)?;
        }

        let mut offhand_animations = Vec::new();
        for rec in opt_array_obj(obj, "offhandAnimations", "combo")? {
            let vals = opt_array(rec, "values", "OffhandAnimation")?;
            let clip_name = vals
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| ComboLoadError::new("OffhandAnimation missing clip name"))?;
            let slot = match vals.get(1) {
                None => OffhandSlot::Fire,
                Some(v) => {
                    let tok = v.as_str().ok_or_else(|| {
                        ComboLoadError::new("OffhandAnimation slot must be a string")
                    })?;
                    OffhandSlot::parse(tok).ok_or_else(|| {
                        ComboLoadError::new(format!("unknown OffhandAnimation slot '{tok}'"))
                    })?
                }
            };
            let mut meta = AnimationMeta {
                name: clip_name.into(),
                duration_secs: 0.0,
                blend_in: 0.0,
                blend_out: 0.0,
                loop_mode: LoopMode::None,
                aim_type: None,
                blend_time_to: Vec::new(),
                sync_type: None,
                low_res_pose_secs: None,
            };
            let children = opt_array(rec, "children", "OffhandAnimation")?;
            apply_anim_children(&mut meta, children, anim_fps)?;
            offhand_animations.push(OffhandAnimDef {
                clip_name: clip_name.into(),
                slot,
                blend_in: meta.blend_in,
                blend_out: meta.blend_out,
                loop_mode: meta.loop_mode,
                aim_type: meta.aim_type,
            });
        }

        let mut throw_animation = None;
        for rec in opt_array_obj(obj, "throwAnimations", "combo")? {
            let vals = opt_array(rec, "values", "ThrowAnimation")?;
            let clip_name = vals
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| ComboLoadError::new("ThrowAnimation missing clip name"))?;
            let children = opt_array(rec, "children", "ThrowAnimation")?;
            if !children.is_empty() {
                let mut tmp = AnimationMeta {
                    name: clip_name.into(),
                    duration_secs: 0.0,
                    blend_in: 0.0,
                    blend_out: 0.0,
                    loop_mode: LoopMode::None,
                    aim_type: None,
                    blend_time_to: Vec::new(),
                    sync_type: None,
                    low_res_pose_secs: None,
                };
                apply_anim_children(&mut tmp, children, anim_fps)?;
            }
            if throw_animation.is_none() {
                throw_animation = Some(Box::<str>::from(clip_name));
            }
        }

        let raw_states = obj
            .get("states")
            .and_then(Value::as_array)
            .ok_or_else(|| ComboLoadError::new("combo missing states array"))?;
        if raw_states.is_empty() {
            return Err(ComboLoadError::new("combo has empty states array"));
        }
        if raw_states.len() >= NO_TRANSITION as usize {
            return Err(ComboLoadError::new("combo machine exceeds u16 state limit"));
        }

        // First pass: assign compact StateIds in authored order.
        let mut state_names: Vec<Box<str>> = Vec::with_capacity(raw_states.len());
        for raw in raw_states {
            let name = raw
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| first_str(raw.get("values")))
                .ok_or_else(|| ComboLoadError::new("state entry missing name"))?;
            if state_names
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(name))
            {
                return Err(ComboLoadError::new(format!(
                    "duplicate state name '{name}'"
                )));
            }
            state_names.push(name.into());
        }

        let mut deflect_animations: Vec<DeflectAnimDef> = Vec::new();
        let mut sounds: Vec<Box<str>> = Vec::new();
        let mut attack_groups: Vec<Box<str>> = Vec::new();
        let mut attack_group_shared_ids: Vec<u32> = Vec::new();
        let mut states = Vec::with_capacity(raw_states.len());

        // Second pass: parse each state's children and resolve state targets.
        for (idx, raw) in raw_states.iter().enumerate() {
            let state_name = state_names[idx].as_ref();
            let state_id = u16::try_from(idx)
                .map(StateId)
                .map_err(|_| ComboLoadError::new("state index exceeds u16 limit"))?;
            let mut st = ComboStateDef::new(state_id);
            for node in opt_array(raw, "children", state_name)? {
                let key = node
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ComboLoadError::new("state child missing key"))?;
                let vals = opt_array(node, "values", key)?;
                let kids = opt_array(node, "children", key)?;
                parse_state_child(
                    &mut st,
                    name_hash,
                    state_name,
                    key,
                    vals,
                    kids,
                    weapon,
                    anim_fps,
                    &state_names,
                    &mut animations,
                    &mut deflect_animations,
                    &mut sounds,
                    &mut attack_groups,
                    &mut attack_group_shared_ids,
                    &mut clip_duration,
                )?;
            }
            if let Some(clip) = st.animation {
                st.clip_deadline = quantize_cs(animations[clip.0 as usize].duration_secs, 0, 1022);
            }
            states.push(st);
        }

        let idle_state = state_names
            .iter()
            .position(|s| s.eq_ignore_ascii_case("IDLE"))
            .map(|i| StateId(i as u16))
            .ok_or_else(|| ComboLoadError::new("missing neutral state"))?;
        let catch_state = state_names
            .iter()
            .position(|s| s.eq_ignore_ascii_case("CATCH_SABRE"))
            .map(|i| StateId(i as u16));

        let machine = Self {
            name_hash,
            anim_fps,
            idle_state,
            catch_state,
            states,
            state_names,
            animations,
            deflect_animations,
            offhand_animations,
            throw_animation,
            sounds,
            attack_groups,
            attack_group_shared_ids,
        };
        machine.validate()?;
        Ok(machine)
    }

    /// Load and validate one hero weapon's combo machine from a prepared side
    /// `combos.json` map (`{ "<fnv_hex>": { ... } }`), selected by
    /// `weapon.combo_hash` so every hero supplies its own weapon edge geometry
    /// and per-hero clip durations.
    pub fn load_combo_by_hash(
        json_text: &str,
        weapon: &WeaponMeleeSpec,
        anim_fps: f64,
        clip_duration: impl FnMut(&str) -> Option<f64>,
    ) -> Result<Self, ComboLoadError> {
        weapon.validate()?;
        let root: Value = serde_json::from_str(json_text)
            .map_err(|e| ComboLoadError::new(format!("invalid combos.json: {e}")))?;
        let map = root
            .as_object()
            .ok_or_else(|| ComboLoadError::new("combos.json root must be an object"))?;
        let wanted = weapon.combo_hash;
        let entry = map.iter().find(|(k, v)| {
            parse_hex_u32(k) == Some(wanted)
                || v.get("nameHash")
                    .and_then(Value::as_str)
                    .and_then(parse_hex_u32)
                    == Some(wanted)
        });
        let Some((key, val)) = entry else {
            return Err(ComboLoadError::new(format!(
                "combo hash 0x{wanted:08x} not found in combos.json"
            )));
        };
        let outer_hash = parse_hex_u32(key);
        let inner_hash = match val.get("nameHash") {
            None => None,
            Some(v) => {
                let s = v
                    .as_str()
                    .ok_or_else(|| ComboLoadError::new("combo nameHash must be a hex string"))?;
                let parsed = parse_hex_u32(s)
                    .ok_or_else(|| ComboLoadError::new(format!("invalid nameHash '{s}'")))?;
                Some(parsed)
            }
        };
        if let (Some(oh), Some(ih)) = (outer_hash, inner_hash) {
            if oh != ih {
                return Err(ComboLoadError::new(format!(
                    "combo key 0x{oh:08x} conflicts with inner nameHash 0x{ih:08x}"
                )));
            }
        }
        let mut machine = Self::from_json_value(val, weapon, anim_fps, clip_duration)?;
        if machine.name_hash == 0 {
            if let Some(parsed) = outer_hash {
                machine.name_hash = parsed;
            }
        }
        Ok(machine)
    }
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    u32::from_str_radix(
        s.trim().trim_start_matches("0x").trim_start_matches("0X"),
        16,
    )
    .ok()
}

fn resolve_clip_secs(
    name: &str,
    json_duration: Option<f64>,
    clip_duration: &mut impl FnMut(&str) -> Option<f64>,
) -> Result<Option<f64>, ComboLoadError> {
    if let Some(d) = clip_duration(name) {
        if !d.is_finite() || d <= 0.0 {
            return Err(ComboLoadError::new(format!(
                "clip '{name}' resolved to non-positive duration {d}"
            )));
        }
        return Ok(Some(d));
    }
    if let Some(d) = json_duration {
        if !d.is_finite() || d <= 0.0 {
            return Err(ComboLoadError::new(format!(
                "clip '{name}' JSON duration must be finite and positive, got {d}"
            )));
        }
        return Ok(Some(d));
    }
    Ok(None)
}

fn check_finite(v: f64, field: &str) -> Result<(), ComboLoadError> {
    if !v.is_finite() {
        return Err(ComboLoadError::new(format!("{field} must be finite")));
    }
    Ok(())
}

fn check_nonneg(v: f64, field: &str) -> Result<(), ComboLoadError> {
    if !v.is_finite() || v < 0.0 {
        return Err(ComboLoadError::new(format!(
            "{field} must be finite and nonnegative"
        )));
    }
    Ok(())
}

fn validate_condition(cond: &ConditionGroup) -> Result<(), ComboLoadError> {
    if let Some(BreakRule::AfterSeconds(s)) = cond.break_rule {
        check_nonneg(s, "break delay")?;
    }
    for tp in [cond.time_start, cond.time_end].into_iter().flatten() {
        match tp {
            TimePoint::FromStart(s) | TimePoint::FromEnd(s) => check_finite(s, "time window")?,
        }
    }
    if let Some(c) = cond.time_in_posture {
        check_finite(c.value, "time_in_posture")?;
    }
    if let Some(c) = cond.energy {
        check_finite(c.value, "energy condition")?;
    }
    if let Some(c) = cond.thrust {
        check_finite(c.value, "thrust condition")?;
    }
    if let Some((lo, hi)) = cond.thrust_angle_deg {
        check_finite(lo, "thrust_angle min")?;
        check_finite(hi, "thrust_angle max")?;
        if lo > hi {
            return Err(ComboLoadError::new("thrust_angle min > max"));
        }
    }
    if let Some(c) = cond.velocity_y {
        check_finite(c.value, "velocity_y condition")?;
    }
    Ok(())
}

/// Time fields use single-precision multiplication before integer truncation.
fn quantize_cs(secs: f64, min_cs: i32, max_cs: i32) -> f64 {
    let counts = ((secs as f32 * 100.0) as i32).clamp(min_cs, max_cs);
    f64::from(counts) / 100.0
}

fn opt_array<'a>(node: &'a Value, field: &str, ctx: &str) -> Result<&'a [Value], ComboLoadError> {
    match node.get(field) {
        None => Ok(&[]),
        Some(v) => v
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| ComboLoadError::new(format!("{ctx} '{field}' must be an array"))),
    }
}

fn opt_array_obj<'a>(
    obj: &'a serde_json::Map<String, Value>,
    field: &str,
    ctx: &str,
) -> Result<&'a [Value], ComboLoadError> {
    match obj.get(field) {
        None => Ok(&[]),
        Some(v) => v
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| ComboLoadError::new(format!("{ctx} '{field}' must be an array"))),
    }
}

fn first_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
}

/// Strictly read a JSON number without coercing string tokens into numbers.
fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn intern_anim(
    anims: &mut Vec<AnimationMeta>,
    meta: AnimationMeta,
) -> Result<ClipId, ComboLoadError> {
    if let Some(pos) = anims.iter().position(|a| a.name == meta.name) {
        if meta.duration_secs > 0.0 && anims[pos].duration_secs == 0.0 {
            anims[pos].duration_secs = meta.duration_secs;
        }
        return u16::try_from(pos)
            .map(ClipId)
            .map_err(|_| ComboLoadError::new("animations table exceeds u16 limit"));
    }
    let id = u16::try_from(anims.len())
        .map(ClipId)
        .map_err(|_| ComboLoadError::new("animations table exceeds u16 limit"))?;
    anims.push(meta);
    Ok(id)
}

fn intern_str(table: &mut Vec<Box<str>>, name: &str) -> Result<u16, ComboLoadError> {
    if let Some(pos) = table.iter().position(|s| s.as_ref() == name) {
        return u16::try_from(pos)
            .map_err(|_| ComboLoadError::new("string table exceeds u16 limit"));
    }
    let id = u16::try_from(table.len())
        .map_err(|_| ComboLoadError::new("string table exceeds u16 limit"))?;
    table.push(name.into());
    Ok(id)
}

fn req_f64(vals: &[Value], idx: usize, key: &str) -> Result<f64, ComboLoadError> {
    let v = vals.get(idx).and_then(as_f64).ok_or_else(|| {
        ComboLoadError::new(format!("{key} missing numeric argument at slot {idx}"))
    })?;
    check_finite(v, key)?;
    Ok(v)
}

fn req_nonneg_f64(vals: &[Value], idx: usize, key: &str) -> Result<f64, ComboLoadError> {
    let v = req_f64(vals, idx, key)?;
    check_nonneg(v, key)?;
    Ok(v)
}

fn req_str<'a>(vals: &'a [Value], idx: usize, key: &str) -> Result<&'a str, ComboLoadError> {
    vals.get(idx)
        .and_then(Value::as_str)
        .ok_or_else(|| ComboLoadError::new(format!("{key} missing string argument at slot {idx}")))
}

fn apply_anim_children(
    meta: &mut AnimationMeta,
    children: &[Value],
    _anim_fps: f64,
) -> Result<(), ComboLoadError> {
    for node in children {
        let key = node
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| ComboLoadError::new("Animation child missing key"))?;
        let vals = opt_array(node, "values", key)?;
        match key {
            "BlendInTime" | "BlendTimeIn" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new(format!("{key} expects 1 argument")));
                }
                meta.blend_in = req_nonneg_f64(vals, 0, key)?;
            }
            "BlendOutTime" | "BlendTimeOut" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new(format!("{key} expects 1 argument")));
                }
                meta.blend_out = req_nonneg_f64(vals, 0, key)?;
            }
            "BlendTimeTo" => {
                if vals.len() != 2 {
                    return Err(ComboLoadError::new(
                        "BlendTimeTo expects 2 arguments (target_clip, seconds)",
                    ));
                }
                let target_clip = req_str(vals, 0, "BlendTimeTo")?;
                let time_secs = req_nonneg_f64(vals, 1, "BlendTimeTo")?;
                meta.blend_time_to.push(BlendTimeTo {
                    target_clip: target_clip.into(),
                    time_secs,
                });
            }
            "Loop" => {
                meta.loop_mode = LoopMode::parse(vals)?;
            }
            "AimType" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new("AimType expects 1 argument"));
                }
                let tok = req_str(vals, 0, "AimType")?;
                meta.aim_type = Some(
                    AimType::parse(tok)
                        .ok_or_else(|| ComboLoadError::new(format!("unknown AimType '{tok}'")))?,
                );
            }
            "SyncType" => {
                if vals.len() != 2 {
                    return Err(ComboLoadError::new(
                        "SyncType expects 2 arguments (clip, mode)",
                    ));
                }
                let sync_clip = req_str(vals, 0, "SyncType")?;
                let mode = req_str(vals, 1, "SyncType")?;
                meta.sync_type = Some(SyncTypeDef {
                    sync_clip: sync_clip.into(),
                    mode: mode.into(),
                });
            }
            "LowResPose" => {
                if vals.is_empty() || vals.len() > 2 {
                    return Err(ComboLoadError::new(
                        "LowResPose expects 1 or 2 arguments (time [, unit])",
                    ));
                }
                let secs = parse_nonneg_time_secs(&vals[0], vals.get(1), "LowResPose")?;
                meta.low_res_pose_secs = Some(secs);
            }
            other => {
                return Err(ComboLoadError::new(format!(
                    "unknown Animation child key '{other}'"
                )));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_state_child(
    st: &mut ComboStateDef,
    name_hash: u32,
    state_name: &str,
    key: &str,
    vals: &[Value],
    kids: &[Value],
    weapon: &WeaponMeleeSpec,
    anim_fps: f64,
    state_names: &[Box<str>],
    animations: &mut Vec<AnimationMeta>,
    deflect_animations: &mut Vec<DeflectAnimDef>,
    sounds: &mut Vec<Box<str>>,
    attack_groups: &mut Vec<Box<str>>,
    attack_group_shared_ids: &mut Vec<u32>,
    clip_duration: &mut impl FnMut(&str) -> Option<f64>,
) -> Result<(), ComboLoadError> {
    match key {
        "Posture" => {
            if vals.len() != 1 {
                return Err(ComboLoadError::new(format!(
                    "state '{state_name}' Posture expects 1 token"
                )));
            }
            let tok = req_str(vals, 0, "State Posture")?;
            st.posture = Some(ComboPosture::parse(tok).ok_or_else(|| {
                ComboLoadError::new(format!(
                    "state '{state_name}' unknown Posture token '{tok}'"
                ))
            })?);
        }
        "Duration" => {
            if vals.is_empty() || vals.len() > 2 {
                return Err(ComboLoadError::new(
                    "Duration expects 1 or 2 arguments (value [, unit])",
                ));
            }
            let secs = quantize_cs(
                parse_nonneg_time_secs(&vals[0], vals.get(1), "Duration")?,
                0,
                1022,
            );
            if secs == 0.0 {
                st.duration = StateDuration::Indefinite;
            } else {
                st.duration = StateDuration::Seconds(secs);
            }
        }
        "MustShowOneFrame" => {
            if !vals.is_empty() {
                return Err(ComboLoadError::new("MustShowOneFrame takes no arguments"));
            }
            st.must_show_one_frame = true;
        }
        "RestartAnimation" => {
            if !vals.is_empty() {
                return Err(ComboLoadError::new("RestartAnimation takes no arguments"));
            }
            st.restart_animation = true;
        }
        "Animation" => {
            if vals.len() != 1 {
                return Err(ComboLoadError::new(format!(
                    "state '{state_name}' Animation expects 1 clip name"
                )));
            }
            let name = req_str(vals, 0, "State Animation")?;
            let clip_id = if let Some(pos) = animations.iter().position(|a| a.name.as_ref() == name)
            {
                if animations[pos].duration_secs <= 0.0 {
                    let resolved = resolve_clip_secs(name, None, clip_duration)?.ok_or_else(
                        || {
                            ComboLoadError::new(format!(
                                "combo 0x{name_hash:08x} state '{state_name}' clip '{name}' has unresolved duration"
                            ))
                        },
                    )?;
                    animations[pos].duration_secs = resolved;
                }
                ClipId(pos as u16)
            } else {
                let resolved = resolve_clip_secs(name, None, clip_duration)?.ok_or_else(|| {
                    ComboLoadError::new(format!(
                        "combo 0x{name_hash:08x} state '{state_name}' clip '{name}' has unresolved duration"
                    ))
                })?;
                let meta = AnimationMeta {
                    name: name.into(),
                    duration_secs: resolved,
                    blend_in: 0.0,
                    blend_out: 0.0,
                    loop_mode: LoopMode::None,
                    aim_type: None,
                    blend_time_to: Vec::new(),
                    sync_type: None,
                    low_res_pose_secs: None,
                };
                intern_anim(animations, meta)?
            };
            let mut state_meta = animations[clip_id.0 as usize].clone();
            apply_anim_children(&mut state_meta, kids, anim_fps)?;
            let layer = StateAnimLayer {
                clip: clip_id,
                blend_in: state_meta.blend_in,
                blend_out: state_meta.blend_out,
                loop_mode: state_meta.loop_mode,
                aim_type: state_meta.aim_type,
                blend_time_to: state_meta.blend_time_to,
                sync_type: state_meta.sync_type,
                low_res_pose_secs: state_meta.low_res_pose_secs,
            };
            if st.animation.is_none() {
                st.animation = Some(clip_id);
                st.loop_mode = layer.loop_mode;
                st.anim_layer = Some(layer);
            } else if st.lower_animation.is_none() {
                st.lower_animation = Some(clip_id);
                st.lower_anim_layer = Some(layer);
            } else {
                return Err(ComboLoadError::new(format!(
                    "combo 0x{name_hash:08x} state '{state_name}' declares more than 2 Animation layers"
                )));
            }
        }
        "EnergyRestoreRate" => {
            if vals.is_empty() {
                st.energy_restore_rate = EnergyRateOverride::Default;
            } else if vals.len() == 1 {
                let rate = req_f64(vals, 0, "EnergyRestoreRate")?;
                st.energy_restore_rate = EnergyRateOverride::Override(rate);
            } else {
                return Err(ComboLoadError::new(
                    "EnergyRestoreRate expects 0 or 1 numeric argument",
                ));
            }
        }
        "PlayExplosion" => {
            if !vals.is_empty() {
                return Err(ComboLoadError::new("PlayExplosion takes no arguments"));
            }
            st.play_explosion = true;
        }
        "TurnOffLightsaber" => {
            if vals.is_empty() {
                st.turn_off_lightsaber = 0xFF;
            } else {
                let mut mask = st.turn_off_lightsaber;
                for (i, _) in vals.iter().enumerate() {
                    let edge = req_f64(vals, i, "TurnOffLightsaber")?;
                    let e = edge as i32;
                    if edge.fract() != 0.0 || !(0..MAX_BLADE_EDGES as i32).contains(&e) {
                        return Err(ComboLoadError::new(format!(
                            "TurnOffLightsaber invalid edge index {edge}"
                        )));
                    }
                    mask |= 1u8 << (e as u8);
                }
                st.turn_off_lightsaber = mask;
            }
        }
        "AlignedToThrust" => {
            if vals.is_empty() {
                st.aligned_to_thrust = AlignMode::Continuous;
            } else if vals.len() == 1 {
                let tok = req_str(vals, 0, "AlignedToThrust")?;
                if tok.eq_ignore_ascii_case("Initial") {
                    st.aligned_to_thrust = AlignMode::Initial;
                } else {
                    return Err(ComboLoadError::new(format!(
                        "unknown AlignedToThrust mode '{tok}'"
                    )));
                }
            } else {
                return Err(ComboLoadError::new(
                    "AlignedToThrust expects 0 or 1 argument",
                ));
            }
        }
        "ThrustFactor" => {
            if vals.len() != 1 {
                return Err(ComboLoadError::new("ThrustFactor expects 1 argument"));
            }
            st.thrust_factor = req_nonneg_f64(vals, 0, "ThrustFactor")?;
        }
        "StrafeFactor" => {
            if vals.len() != 1 {
                return Err(ComboLoadError::new("StrafeFactor expects 1 argument"));
            }
            st.strafe_factor = req_nonneg_f64(vals, 0, "StrafeFactor")?;
        }
        "TurnFactor" => {
            if vals.len() != 1 {
                return Err(ComboLoadError::new("TurnFactor expects 1 argument"));
            }
            st.turn_factor = req_nonneg_f64(vals, 0, "TurnFactor")?;
        }
        "AnimatedMove" => {
            let mut am = AnimatedMoveDef::default();
            for node in kids {
                let k = node
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ComboLoadError::new("AnimatedMove child missing key"))?;
                let cvals = opt_array(node, "values", k)?;
                let ckids = opt_array(node, "children", k)?;
                match k {
                    "VelocityZ" => {
                        if cvals.len() != 1 {
                            return Err(ComboLoadError::new("VelocityZ expects 1 argument"));
                        }
                        am.velocity_z = crate::ordnance::math::clamp(
                            req_f64(cvals, 0, "VelocityZ")?,
                            -40.96,
                            40.94,
                        );
                    }
                    "VelocityX" => {
                        if cvals.len() != 1 {
                            return Err(ComboLoadError::new("VelocityX expects 1 argument"));
                        }
                        am.velocity_x = crate::ordnance::math::clamp(
                            req_f64(cvals, 0, "VelocityX")?,
                            -40.96,
                            40.94,
                        );
                    }
                    "VelocityFromThrust" => {
                        if cvals.len() != 1 {
                            return Err(ComboLoadError::new(
                                "VelocityFromThrust expects 1 argument",
                            ));
                        }
                        am.velocity_from_thrust = crate::ordnance::math::clamp(
                            req_f64(cvals, 0, "VelocityFromThrust")?,
                            0.0,
                            25.5,
                        );
                    }
                    "VelocityFromStrafe" => {
                        if cvals.len() != 1 {
                            return Err(ComboLoadError::new(
                                "VelocityFromStrafe expects 1 argument",
                            ));
                        }
                        am.velocity_from_strafe = crate::ordnance::math::clamp(
                            req_f64(cvals, 0, "VelocityFromStrafe")?,
                            0.0,
                            25.5,
                        );
                    }
                    "If" => {
                        am.if_cond = Some(parse_condition_group(ckids)?);
                    }
                    "Until" => {
                        am.until_cond = Some(parse_condition_group(ckids)?);
                    }
                    other => {
                        return Err(ComboLoadError::new(format!(
                            "unknown AnimatedMove key '{other}'"
                        )));
                    }
                }
            }
            st.animated_move = Some(am);
        }
        "Gravity" => {
            if vals.len() != 1 {
                return Err(ComboLoadError::new("Gravity expects 1 numeric argument"));
            }
            let g = st.gravity.get_or_insert_with(GravityOverride::default);
            g.scale = req_f64(vals, 0, "Gravity")?;
        }
        "GravityVelocityTarget" => {
            if vals.is_empty() {
                return Err(ComboLoadError::new(
                    "GravityVelocityTarget expects at least 1 argument",
                ));
            }
            let g = st.gravity.get_or_insert_with(GravityOverride::default);
            g.velocity_target = Some(req_f64(vals, 0, "GravityVelocityTarget")?);
            for (i, _) in vals.iter().enumerate().skip(1) {
                let flag = req_str(vals, i, "GravityVelocityTarget flag")?;
                if flag.eq_ignore_ascii_case("Impulse") {
                    g.impulse = true;
                } else if flag.eq_ignore_ascii_case("ZeroGravity") {
                    g.zero_gravity = true;
                } else {
                    return Err(ComboLoadError::new(format!(
                        "unknown GravityVelocityTarget flag '{flag}'"
                    )));
                }
            }
        }
        "InputLock" => {
            st.input_lock = Some(parse_input_lock(vals)?);
        }
        "Sound" => {
            let name = req_str(vals, 0, "Sound")?;
            if vals.len() > 3 {
                return Err(ComboLoadError::new(
                    "Sound expects at most 3 arguments (name [, time [, unit]])",
                ));
            }
            let at_secs = if let Some(time_val) = vals.get(1) {
                Some(quantize_cs(
                    parse_nonneg_time_secs(time_val, vals.get(2), "Sound")?,
                    0,
                    1023,
                ))
            } else {
                None
            };
            let sound = SoundId(intern_str(sounds, name)?);
            st.sounds.push(SoundCue { sound, at_secs });
        }
        "Deflect" => {
            let anim_start = u16::try_from(deflect_animations.len())
                .map_err(|_| ComboLoadError::new("deflect_animations exceeds u16 limit"))?;
            let mut min_angle_deg: Option<f64> = None;
            let mut max_angle_deg: Option<f64> = None;
            let mut energy_cost = 0.0;
            for node in kids {
                let k = node
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ComboLoadError::new("Deflect child missing key"))?;
                let cvals = opt_array(node, "values", k)?;
                let ckids = opt_array(node, "children", k)?;
                match k {
                    "DeflectAngle" => {
                        if cvals.len() != 2 {
                            return Err(ComboLoadError::new(
                                "DeflectAngle expects 2 arguments (min, max)",
                            ));
                        }
                        min_angle_deg = Some(req_f64(cvals, 0, "DeflectAngle min")?);
                        max_angle_deg = Some(req_f64(cvals, 1, "DeflectAngle max")?);
                    }
                    "EnergyCost" => {
                        if cvals.len() != 1 {
                            return Err(ComboLoadError::new(
                                "Deflect EnergyCost expects 1 argument",
                            ));
                        }
                        energy_cost = req_nonneg_f64(cvals, 0, "Deflect EnergyCost")?;
                    }
                    "DeflectAnimation" => {
                        if cvals.len() != 2 {
                            return Err(ComboLoadError::new(
                                "DeflectAnimation expects 2 arguments (clip_name, direction)",
                            ));
                        }
                        let clip_name = req_str(cvals, 0, "DeflectAnimation clip")?;
                        let dir_tok = req_str(cvals, 1, "DeflectAnimation direction")?;
                        let direction = DeflectDirection::parse(dir_tok).ok_or_else(|| {
                            ComboLoadError::new(format!(
                                "unknown DeflectAnimation direction '{dir_tok}'"
                            ))
                        })?;
                        let mut meta = AnimationMeta {
                            name: clip_name.into(),
                            duration_secs: 0.0,
                            blend_in: 0.0,
                            blend_out: 0.0,
                            loop_mode: LoopMode::None,
                            aim_type: None,
                            blend_time_to: Vec::new(),
                            sync_type: None,
                            low_res_pose_secs: None,
                        };
                        if !ckids.is_empty() {
                            apply_anim_children(&mut meta, ckids, anim_fps)?;
                        }
                        if deflect_animations.len() >= u16::MAX as usize {
                            return Err(ComboLoadError::new(
                                "deflect_animations exceeds u16 limit",
                            ));
                        }
                        deflect_animations.push(DeflectAnimDef {
                            clip_name: clip_name.into(),
                            direction,
                            blend_in: meta.blend_in,
                            blend_out: meta.blend_out,
                            loop_mode: meta.loop_mode,
                            aim_type: meta.aim_type,
                            low_res_pose_secs: meta.low_res_pose_secs,
                        });
                    }
                    other => {
                        return Err(ComboLoadError::new(format!(
                            "unknown Deflect child key '{other}'"
                        )));
                    }
                }
            }
            let (Some(lo), Some(hi)) = (min_angle_deg, max_angle_deg) else {
                return Err(ComboLoadError::new(format!(
                    "combo 0x{name_hash:08x} state '{state_name}' Deflect missing required DeflectAngle"
                )));
            };
            let anim_len = u16::try_from(deflect_animations.len() - anim_start as usize)
                .map_err(|_| ComboLoadError::new("deflect_animations exceeds u16 limit"))?;
            let mut reaction_ids = [u16::MAX; 6];
            for (offset, clip) in deflect_animations[anim_start as usize..].iter().enumerate() {
                let id = &mut reaction_ids[clip.direction as usize];
                if *id == u16::MAX {
                    *id = anim_start + offset as u16;
                }
            }
            st.deflect = Some(DeflectDef {
                reaction_ids,
                min_angle_deg: lo,
                max_angle_deg: hi,
                energy_cost,
                deflect_anim_start: anim_start,
                deflect_anim_len: anim_len,
            });
        }
        "Attack" => {
            let atk = parse_attack(
                st.id,
                st.attacks.len(),
                kids,
                weapon,
                attack_groups,
                attack_group_shared_ids,
            )?;
            st.attacks.push(atk);
        }
        "Transition" => {
            // Only slot 0 is the target state name; ignore any trailing comment-hash slots.
            let target_name = req_str(vals, 0, "Transition")?;
            let target = state_names
                .iter()
                .position(|s| s.as_ref() == target_name || s.eq_ignore_ascii_case(target_name))
                .map(|i| StateId(i as u16))
                .ok_or_else(|| {
                    ComboLoadError::new(format!(
                        "Transition references unknown state '{target_name}'"
                    ))
                })?;
            let mut energy_cost = 0.0;
            let mut branches = Vec::new();
            for node in kids {
                let k = node
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ComboLoadError::new("Transition child missing key"))?;
                let cvals = opt_array(node, "values", k)?;
                let ckids = opt_array(node, "children", k)?;
                match k {
                    "EnergyCost" => {
                        if cvals.len() != 1 {
                            return Err(ComboLoadError::new(
                                "Transition EnergyCost expects 1 argument",
                            ));
                        }
                        energy_cost = req_nonneg_f64(cvals, 0, "Transition EnergyCost")?;
                    }
                    "If" | "Or" => {
                        branches.push(parse_condition_group(ckids)?);
                    }
                    other => {
                        return Err(ComboLoadError::new(format!(
                            "unknown Transition child key '{other}'"
                        )));
                    }
                }
            }
            if st.transitions.len() >= NO_TRANSITION as usize {
                return Err(ComboLoadError::new("too many transitions in state"));
            }
            st.transitions.push(TransitionDef {
                target,
                energy_cost,
                branches,
            });
        }
        other => {
            return Err(ComboLoadError::new(format!(
                "combo 0x{name_hash:08x} state '{state_name}' unknown key '{other}'"
            )));
        }
    }
    Ok(())
}

fn parse_time_unit(unit: Option<&Value>, key: &str) -> Result<bool, ComboLoadError> {
    let Some(u) = unit else {
        return Ok(false);
    };
    let s = u
        .as_str()
        .ok_or_else(|| ComboLoadError::new(format!("{key} time unit must be a string")))?;
    if s.eq_ignore_ascii_case("Frames") {
        Ok(true)
    } else if s.eq_ignore_ascii_case("Seconds") {
        Ok(false)
    } else {
        Err(ComboLoadError::new(format!(
            "{key} unknown time unit '{s}'"
        )))
    }
}

fn parse_nonneg_time_secs(
    raw_val: &Value,
    unit: Option<&Value>,
    key: &str,
) -> Result<f64, ComboLoadError> {
    let raw = as_f64(raw_val)
        .ok_or_else(|| ComboLoadError::new(format!("{key} missing numeric value")))?;
    check_nonneg(raw, key)?;
    let is_frames = parse_time_unit(unit, key)?;
    Ok(if is_frames { raw / COMBO_ANIM_FPS } else { raw })
}

fn parse_input_lock(vals: &[Value]) -> Result<InputLockDef, ComboLoadError> {
    let mut idx = 0usize;
    let mut duration = None;
    if let Some(first) = vals.first() {
        if let Some(v) = as_f64(first) {
            check_nonneg(v, "InputLock duration")?;
            idx = 1;
            let raw_secs = match vals.get(1).and_then(Value::as_str) {
                Some(u) if u.eq_ignore_ascii_case("Frames") => {
                    idx = 2;
                    v / COMBO_ANIM_FPS
                }
                Some(u) if u.eq_ignore_ascii_case("Seconds") => {
                    idx = 2;
                    v
                }
                _ => v,
            };
            let secs = quantize_cs(raw_secs, 0, 1023);
            // A leading 0.0 (or 0 Frames) locks for the whole state, matching
            // Duration(0.0) and omitted duration.
            duration = if secs == 0.0 { None } else { Some(secs) };
        } else if first.as_str().is_none() {
            return Err(ComboLoadError::new(
                "InputLock first argument must be a duration or action token",
            ));
        }
    }
    let mut mask = InputLockMask::NONE;
    for v in &vals[idx..] {
        let tok = v
            .as_str()
            .ok_or_else(|| ComboLoadError::new("InputLock action token must be a string"))?;
        if let Some(neg) = tok.strip_prefix('!') {
            let flag = InputLockMask::parse_token(neg)
                .ok_or_else(|| ComboLoadError::new(format!("unknown InputLock token '{tok}'")))?;
            mask = mask.without(flag);
        } else {
            let flag = InputLockMask::parse_token(tok)
                .ok_or_else(|| ComboLoadError::new(format!("unknown InputLock token '{tok}'")))?;
            mask = mask.union(flag);
        }
    }
    Ok(InputLockDef {
        duration,
        blocked: mask,
    })
}

fn parse_edge_dim(vals: &[Value], key: &str) -> Result<(f64, bool), ComboLoadError> {
    if vals.is_empty() || vals.len() > 2 {
        return Err(ComboLoadError::new(format!(
            "{key} expects 1 or 2 arguments (value [, \"FromEdge\"])"
        )));
    }
    let from_edge = match vals.get(1) {
        None => false,
        Some(u) => {
            let tok = u
                .as_str()
                .ok_or_else(|| ComboLoadError::new(format!("{key} modifier must be a string")))?;
            if tok.eq_ignore_ascii_case("FromEdge") {
                true
            } else {
                return Err(ComboLoadError::new(format!(
                    "{key} unknown modifier '{tok}'"
                )));
            }
        }
    };
    if from_edge {
        let scale = req_nonneg_f64(vals, 0, key)?;
        if scale <= 0.0 {
            return Err(ComboLoadError::new(format!(
                "{key} FromEdge factor must be positive"
            )));
        }
        Ok((scale, true))
    } else {
        let v = req_f64(vals, 0, key)?;
        if v < 0.0 {
            // Negative bare dimension encodes an edge-relative factor by magnitude.
            Ok((-v, true))
        } else {
            Ok((v, false))
        }
    }
}

fn parse_attack(
    state_id: StateId,
    attack_index: usize,
    kids: &[Value],
    weapon: &WeaponMeleeSpec,
    attack_groups: &mut Vec<Box<str>>,
    attack_group_shared_ids: &mut Vec<u32>,
) -> Result<AttackDef, ComboLoadError> {
    let mut explicit_id: Option<&str> = None;
    let mut edge: u8 = 0;
    let mut window: Option<DamageWindow> = None;
    let mut length_spec: Option<(f64, bool)> = None;
    let mut width_spec: Option<(f64, bool)> = None;
    let mut damage = 0.0;
    let mut push = 0.0;

    for node in kids {
        let k = node
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| ComboLoadError::new("Attack child missing key"))?;
        let vals = opt_array(node, "values", k)?;
        match k {
            "AttackId" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new("AttackId expects 1 string argument"));
                }
                explicit_id = Some(req_str(vals, 0, "AttackId")?);
            }
            "Edge" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new("Edge expects 1 integer argument"));
                }
                let v = req_f64(vals, 0, "Edge")?;
                let e = v as i32;
                if v.fract() != 0.0 || !(0..weapon.num_edges as i32).contains(&e) {
                    return Err(ComboLoadError::new(format!(
                        "invalid attack edge {v} for weapon with {} edges",
                        weapon.num_edges
                    )));
                }
                edge = e as u8;
            }
            "DamageTime" => {
                if vals.len() < 2 || vals.len() > 3 {
                    return Err(ComboLoadError::new(
                        "DamageTime expects 2 or 3 arguments (start, end [, unit])",
                    ));
                }
                let start = req_f64(vals, 0, "DamageTime start")?;
                let end = req_f64(vals, 1, "DamageTime end")?;
                window = Some(match vals.get(2) {
                    None => DamageWindow::Seconds {
                        start: quantize_cs(v8math::max(0.0, start), 0, 1023),
                        end: quantize_cs(v8math::max(0.0, end), 0, 1023),
                    },
                    Some(v) => {
                        let mode = v.as_str().ok_or_else(|| {
                            ComboLoadError::new("DamageTime unit must be a string")
                        })?;
                        if mode.eq_ignore_ascii_case("FromAnim") {
                            DamageWindow::FromAnim {
                                start: quantize_cs(v8math::min(v8math::abs(start), 1.0), 0, 100),
                                end: quantize_cs(v8math::min(v8math::abs(end), 1.0), 0, 100),
                            }
                        } else if mode.eq_ignore_ascii_case("Frames") {
                            DamageWindow::Frames {
                                start: v8math::max(0.0, start),
                                end: v8math::max(0.0, end),
                            }
                        } else if mode.eq_ignore_ascii_case("Seconds") {
                            DamageWindow::Seconds {
                                start: quantize_cs(v8math::max(0.0, start), 0, 1023),
                                end: quantize_cs(v8math::max(0.0, end), 0, 1023),
                            }
                        } else {
                            return Err(ComboLoadError::new(format!(
                                "DamageTime unknown unit '{mode}'"
                            )));
                        }
                    }
                });
            }
            "DamageLength" | "DamageDistance" => {
                length_spec = Some(parse_edge_dim(vals, k)?);
            }
            "DamageWidth" => {
                width_spec = Some(parse_edge_dim(vals, "DamageWidth")?);
            }
            "Damage" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new("Damage expects 1 argument"));
                }
                damage = req_nonneg_f64(vals, 0, "Damage")?;
            }
            "Push" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new("Push expects 1 argument"));
                }
                push = req_f64(vals, 0, "Push")?;
            }
            other => {
                return Err(ComboLoadError::new(format!(
                    "unknown Attack child key '{other}'"
                )));
            }
        }
    }

    let window = window.ok_or_else(|| ComboLoadError::new("Attack missing required DamageTime"))?;
    let edge_spec = weapon.edges[edge as usize];
    let damage_length = match length_spec {
        Some((scale, true)) => {
            let resolved = edge_spec.default_length * scale;
            if !resolved.is_finite() || resolved <= 0.0 {
                return Err(ComboLoadError::new(format!(
                    "edge-relative DamageLength {resolved} must be positive"
                )));
            }
            resolved
        }
        Some((v, false)) => v,
        None => edge_spec.default_length,
    };
    let damage_width = match width_spec {
        Some((scale, true)) => {
            let resolved = edge_spec.default_width * scale;
            if !resolved.is_finite() || resolved <= 0.0 {
                return Err(ComboLoadError::new(format!(
                    "edge-relative DamageWidth {resolved} must be positive"
                )));
            }
            resolved
        }
        Some((v, false)) => v,
        None => edge_spec.default_width,
    };

    let atk_idx_u16 = u16::try_from(attack_index)
        .map_err(|_| ComboLoadError::new("attack index exceeds u16 limit"))?;
    let attack_key = pack_attack_key(state_id, atk_idx_u16);
    let shared_id = explicit_id.map(attack_shared_id).unwrap_or(0);
    let group_id = if shared_id != 0 {
        if let Some(pos) = attack_group_shared_ids
            .iter()
            .position(|&id| id == shared_id)
        {
            AttackGroupId(pos as u16)
        } else {
            let pos = u16::try_from(attack_groups.len())
                .map_err(|_| ComboLoadError::new("attack_groups exceeds u16 limit"))?;
            attack_groups.push(explicit_id.unwrap_or("").into());
            attack_group_shared_ids.push(shared_id);
            AttackGroupId(pos)
        }
    } else {
        let pos = u16::try_from(attack_groups.len())
            .map_err(|_| ComboLoadError::new("attack_groups exceeds u16 limit"))?;
        let label = match explicit_id {
            Some(name) => name.into(),
            None => format!("__s{}_a{}", state_id.0, attack_index).into(),
        };
        attack_groups.push(label);
        attack_group_shared_ids.push(0);
        AttackGroupId(pos)
    };

    Ok(AttackDef {
        attack_group: group_id,
        attack_key,
        shared_id,
        edge,
        window,
        damage_length,
        damage_width,
        damage,
        push,
    })
}

fn parse_time_point(vals: &[Value], key: &str) -> Result<TimePoint, ComboLoadError> {
    if vals.is_empty() || vals.len() > 3 {
        return Err(ComboLoadError::new(format!(
            "{key} expects 1 to 3 arguments"
        )));
    }
    // Preserve JSON argument types: a string token in slot 0 reads as 0.0
    // rather than being coerced to a number.
    let raw = if vals[0].is_string() {
        0.0
    } else {
        req_f64(vals, 0, key)?
    };
    match vals.len() {
        1 => Ok(TimePoint::FromStart(quantize_cs(raw, -512, 511))),
        2 => {
            let tok = req_str(vals, 1, key)?;
            if tok.eq_ignore_ascii_case("FromEnd") {
                Ok(TimePoint::FromEnd(quantize_cs(raw, -512, 511)))
            } else if tok.eq_ignore_ascii_case("Frames") {
                Ok(TimePoint::FromStart(quantize_cs(
                    raw / COMBO_ANIM_FPS,
                    -512,
                    511,
                )))
            } else if tok.eq_ignore_ascii_case("Seconds") {
                Ok(TimePoint::FromStart(quantize_cs(raw, -512, 511)))
            } else {
                Err(ComboLoadError::new(format!("{key} unknown unit '{tok}'")))
            }
        }
        3 => {
            let is_frames = parse_time_unit(Some(&vals[1]), key)?;
            let rel = req_str(vals, 2, key)?;
            if !rel.eq_ignore_ascii_case("FromEnd") {
                return Err(ComboLoadError::new(format!("{key} unknown anchor '{rel}'")));
            }
            let secs = if is_frames { raw / COMBO_ANIM_FPS } else { raw };
            Ok(TimePoint::FromEnd(quantize_cs(secs, -512, 511)))
        }
        _ => unreachable!(),
    }
}

fn parse_scalar_cmp(vals: &[Value], key: &str) -> Result<ScalarCmp, ComboLoadError> {
    if vals.len() != 2 {
        return Err(ComboLoadError::new(format!(
            "{key} expects 2 arguments (op, value)"
        )));
    }
    let op_str = req_str(vals, 0, key)?;
    let op = ScalarOp::parse(op_str)
        .ok_or_else(|| ComboLoadError::new(format!("{key} unknown comparator '{op_str}'")))?;
    let value = req_f64(vals, 1, key)?;
    Ok(ScalarCmp { op, value })
}

fn parse_condition_group(kids: &[Value]) -> Result<ConditionGroup, ComboLoadError> {
    let mut cond = ConditionGroup::default();
    for node in kids {
        let k = node
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| ComboLoadError::new("Condition child missing key"))?;
        let vals = opt_array(node, "values", k)?;
        match k {
            "Break" => {
                if vals.is_empty() {
                    cond.break_rule = Some(BreakRule::Immediate);
                } else {
                    if vals.len() > 2 {
                        return Err(ComboLoadError::new(
                            "Break expects at most 2 arguments (delay [, unit])",
                        ));
                    }
                    let secs = quantize_cs(
                        parse_nonneg_time_secs(&vals[0], vals.get(1), "Break")?,
                        0,
                        511,
                    );
                    cond.break_rule = Some(if secs == 0.0 {
                        BreakRule::Immediate
                    } else {
                        BreakRule::AfterSeconds(secs)
                    });
                }
            }
            "TimeStart" => {
                cond.time_start = Some(parse_time_point(vals, "TimeStart")?);
            }
            "TimeEnd" => {
                cond.time_end = Some(parse_time_point(vals, "TimeEnd")?);
            }
            "TimeInPosture" => {
                cond.time_in_posture = Some(parse_scalar_cmp(vals, "TimeInPosture")?);
            }
            "Button" => {
                if vals.is_empty() || vals.len() > 2 {
                    return Err(ComboLoadError::new(
                        "Button expects 1 or 2 arguments (name [, mode])",
                    ));
                }
                let b_str = req_str(vals, 0, "Button")?;
                let m_str = match vals.get(1) {
                    None => "Press",
                    Some(_) => req_str(vals, 1, "Button mode")?,
                };
                let button = ComboButton::parse(b_str)
                    .ok_or_else(|| ComboLoadError::new(format!("unknown Button '{b_str}'")))?;
                let mode = ButtonMode::parse(m_str)
                    .ok_or_else(|| ComboLoadError::new(format!("unknown Button mode '{m_str}'")))?;
                cond.buttons.push(ButtonCondition { button, mode });
            }
            "ButtonsPressed" => {
                if vals.len() != 1 {
                    return Err(ComboLoadError::new("ButtonsPressed expects 1 argument"));
                }
                let tok = req_str(vals, 0, "ButtonsPressed")?;
                if !tok.eq_ignore_ascii_case("Any") {
                    return Err(ComboLoadError::new(format!(
                        "unknown ButtonsPressed token '{tok}'"
                    )));
                }
                cond.buttons_pressed_any = true;
            }
            "Posture" => {
                if vals.is_empty() {
                    return Err(ComboLoadError::new(
                        "Condition Posture expects at least 1 token",
                    ));
                }
                let mut include = PostureMask::NONE;
                let mut exclude = PostureMask::NONE;
                for (i, _) in vals.iter().enumerate() {
                    let tok = req_str(vals, i, "Condition Posture")?;
                    if let Some(neg) = tok.strip_prefix('!') {
                        let p = ComboPosture::parse(neg).ok_or_else(|| {
                            ComboLoadError::new(format!("unknown Condition Posture token '{tok}'"))
                        })?;
                        exclude = exclude.union(PostureMask::from_posture(p));
                    } else if tok.eq_ignore_ascii_case("Any") {
                        include = PostureMask::ALL;
                    } else {
                        let p = ComboPosture::parse(tok).ok_or_else(|| {
                            ComboLoadError::new(format!("unknown Condition Posture token '{tok}'"))
                        })?;
                        include = include.union(PostureMask::from_posture(p));
                    }
                }
                if include == PostureMask::NONE && exclude != PostureMask::NONE {
                    include = PostureMask::ALL;
                }
                if include == PostureMask::NONE {
                    return Err(ComboLoadError::new(
                        "Condition Posture resolved to empty mask",
                    ));
                }
                cond.posture = Some(PostureCondition { include, exclude });
            }
            "Energy" => {
                cond.energy = Some(parse_scalar_cmp(vals, "Energy")?);
            }
            "Thrust" => {
                cond.thrust = Some(parse_scalar_cmp(vals, "Thrust")?);
            }
            "ThrustAngle" => {
                if vals.len() != 2 {
                    return Err(ComboLoadError::new(
                        "ThrustAngle expects 2 arguments (min, max)",
                    ));
                }
                let lo = req_f64(vals, 0, "ThrustAngle min")?;
                let hi = req_f64(vals, 1, "ThrustAngle max")?;
                cond.thrust_angle_deg = Some((lo, hi));
            }
            "VelocityY" => {
                cond.velocity_y = Some(parse_scalar_cmp(vals, "VelocityY")?);
            }
            other => {
                return Err(ComboLoadError::new(format!(
                    "unknown Condition child key '{other}'"
                )));
            }
        }
    }
    Ok(cond)
}
