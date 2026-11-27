//! Force Push / Force Pull and Saber Throw simulation helpers for Battlefront II
//! heroes.
//!
//! Force Push and Force Pull map onto conical emitter checks with signed
//! knockback (`Push > 0` pushes away; `Push < 0` pulls toward the caster) and
//! energy drain. Saber Throw maps onto a deterministic boomerang blade flight
//! that sweeps its spinning blade segment against targets and signals the combo
//! state machine to enter `CATCH_SABRE` on return.

use super::data::{
    AttackDef, AttackGroupId, ComboLoadError, ComboMachine, DamageWindow, WeaponMeleeSpec,
};
use super::deflect::PROVISIONAL_DEFLECT_SABER_THROW;
use super::sweep::{
    resolve_melee_hit, BladeSegment, BladeSweep, MeleeHitOutcome, MeleeTarget, TargetId,
    ThrowHitHistory, MAX_TARGETS_PER_ATTACK, PROVISIONAL_DAMAGE_WIDTH_RADIUS_SCALE,
};
use super::CombatEnergy;
use super::{CheckpointError, ComboState, StateEntry};
use crate::gen::HitShape;
use crate::ordnance::math::{self as om, V3};
use std::fmt;

const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;
const TAU: f64 = 2.0 * std::f64::consts::PI;
const UNIT_DIR_SQ_TOL: f64 = 1e-3;

/// Zero-allocation validation error for [`ForcePushDef`] and [`SaberThrowDef`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerValidationError {
    InvalidEnergyDrain,
    InvalidConeLength,
    InvalidConeAngle,
    InvalidPush,
    InvalidDamage,
    InvalidVelocity,
    InvalidLifeSpan,
    InvalidReturnDuration,
    InvalidRotationRate,
    InvalidDamageLength,
    InvalidDamageWidth,
    InvalidDamageScale,
    InvalidShieldScale,
}

impl fmt::Display for PowerValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::InvalidEnergyDrain => "power energy_drain must be finite and nonnegative",
            Self::InvalidConeLength => "ForcePushDef.cone_length must be finite and positive",
            Self::InvalidConeAngle => "ForcePushDef.cone_angle_deg must be in (0, 360]",
            Self::InvalidPush => "power push must be finite",
            Self::InvalidDamage => "power damage must be finite and nonnegative",
            Self::InvalidVelocity => "SaberThrowDef.velocity must be finite and positive",
            Self::InvalidLifeSpan => "SaberThrowDef.life_span must be finite and positive",
            Self::InvalidReturnDuration => {
                "SaberThrowDef.return_duration must be finite and positive"
            }
            Self::InvalidRotationRate => "SaberThrowDef.rotation_rate must be finite",
            Self::InvalidDamageLength => "SaberThrowDef.damage_length must be finite and positive",
            Self::InvalidDamageWidth => "SaberThrowDef.damage_width must be finite and nonnegative",
            Self::InvalidDamageScale => {
                "SaberThrowDef.damage_scales entries must be finite and nonnegative"
            }
            Self::InvalidShieldScale => "SaberThrowDef.shield_scale must be finite and nonnegative",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for PowerValidationError {}

impl From<PowerValidationError> for ComboLoadError {
    fn from(err: PowerValidationError) -> Self {
        ComboLoadError {
            message: err.to_string(),
        }
    }
}

/// Maximum targets affected by a single Force Push / Pull wave without heap
/// allocation.
pub const MAX_FORCE_PUSH_TARGETS: usize = 16;

/// Provisional until verified against the 2005 game: upward velocity factor
/// added to horizontal knockback (matches the existing explosion lift ratio in
/// `combat.rs`).
pub const PROVISIONAL_FORCE_PUSH_VERTICAL_LIFT: f64 = 0.6;

/// Provisional until verified against the 2005 game: vertical speed threshold
/// above which a pushed unit enters the tumble / thrown air state.
pub const PROVISIONAL_FORCE_PUSH_THROWN_SPEED: f64 = 2.0;

/// Provisional until verified against the 2005 game: a thrown lightsaber hits
/// each target at most once on the outbound pass and once on the return pass.
pub const PROVISIONAL_SABER_THROW_SINGLE_HIT_PER_PASS: bool = true;

/// ODF-driven parameters for Force Push (`push > 0`) or Force Pull (`push < 0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForcePushDef {
    pub energy_drain: f64,
    pub cone_length: f64,
    pub cone_angle_deg: f64,
    pub max_targets: usize,
    /// Positive pushes targets away; negative pulls targets toward the caster.
    pub push: f64,
    pub damage: f64,
    pub affect_friends: bool,
}

impl ForcePushDef {
    /// Validate that all fields are finite and within safe physical ranges
    /// without heap allocation.
    pub const fn validate(&self) -> Result<(), PowerValidationError> {
        if !self.energy_drain.is_finite() || self.energy_drain < 0.0 {
            return Err(PowerValidationError::InvalidEnergyDrain);
        }
        if !self.cone_length.is_finite() || self.cone_length <= 0.0 {
            return Err(PowerValidationError::InvalidConeLength);
        }
        if !self.cone_angle_deg.is_finite()
            || self.cone_angle_deg <= 0.0
            || self.cone_angle_deg > 360.0
        {
            return Err(PowerValidationError::InvalidConeAngle);
        }
        if !self.push.is_finite() {
            return Err(PowerValidationError::InvalidPush);
        }
        if !self.damage.is_finite() || self.damage < 0.0 {
            return Err(PowerValidationError::InvalidDamage);
        }
        Ok(())
    }
}

/// Load-time validated [`ForcePushDef`] that skips repeated per-tick validation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidatedForcePushDef(ForcePushDef);

impl ValidatedForcePushDef {
    pub const fn try_new(def: ForcePushDef) -> Result<Self, PowerValidationError> {
        match def.validate() {
            Ok(()) => Ok(Self(def)),
            Err(e) => Err(e),
        }
    }

    pub const fn as_def(&self) -> &ForcePushDef {
        &self.0
    }
}

/// Candidate unit evaluated by [`execute_force_push`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForcePushCandidate {
    pub target_id: TargetId,
    pub position: V3,
    pub team: f64,
    pub alive: bool,
    /// Caller-evaluated line-of-sight flag (false skips targets occluded by
    /// terrain or walls).
    pub los_clear: bool,
}

/// Output impulse and damage applied to one struck target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ForcePushHit {
    pub target_id: TargetId,
    pub distance: f64,
    pub impulse: V3,
    pub enters_tumble: bool,
    pub damage: f64,
}

/// Fixed-capacity result list returned by [`execute_force_push`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForcePushResult {
    pub energy_spent: f64,
    hits: [ForcePushHit; MAX_FORCE_PUSH_TARGETS],
    len: u8,
}

impl ForcePushResult {
    pub fn hits(&self) -> &[ForcePushHit] {
        &self.hits[..self.len as usize]
    }
}

#[inline]
fn vec_finite(v: V3) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}

/// Execute one Force Push or Force Pull emission using a pre-validated
/// definition. Damage and impulse are uniform across `[0, cone_length]` for
/// clear-LOS targets inside the cone.
pub fn execute_force_push_validated(
    def: &ValidatedForcePushDef,
    caster_pos: V3,
    caster_aim_dir: V3,
    caster_team: f64,
    energy: &mut dyn CombatEnergy,
    candidates: &[ForcePushCandidate],
) -> Option<ForcePushResult> {
    execute_force_push_core(
        def.as_def(),
        caster_pos,
        caster_aim_dir,
        caster_team,
        energy,
        candidates,
    )
}

/// Execute one Force Push or Force Pull emission. Returns `None` without heap
/// allocation when `def` is invalid, inputs are non-finite/degenerate, or the
/// caster does not have enough energy to pay `def.energy_drain`.
pub fn execute_force_push(
    def: &ForcePushDef,
    caster_pos: V3,
    caster_aim_dir: V3,
    caster_team: f64,
    energy: &mut dyn CombatEnergy,
    candidates: &[ForcePushCandidate],
) -> Option<ForcePushResult> {
    if def.validate().is_err() {
        return None;
    }
    execute_force_push_core(
        def,
        caster_pos,
        caster_aim_dir,
        caster_team,
        energy,
        candidates,
    )
}

fn execute_force_push_core(
    def: &ForcePushDef,
    caster_pos: V3,
    caster_aim_dir: V3,
    caster_team: f64,
    energy: &mut dyn CombatEnergy,
    candidates: &[ForcePushCandidate],
) -> Option<ForcePushResult> {
    if !energy.balance().is_finite()
        || !vec_finite(caster_pos)
        || !vec_finite(caster_aim_dir)
        || om::length_sq(caster_aim_dir) <= 1e-12
        || !energy.pay(def.energy_drain)
    {
        return None;
    }

    let forward = om::normalize(caster_aim_dir);
    let half_angle_rad = v8math::max(0.0, def.cone_angle_deg) * 0.5 * DEG_TO_RAD;
    let min_cos = v8math::cos(half_angle_rad);
    let max_hits = def.max_targets.min(MAX_FORCE_PUSH_TARGETS);

    let mut hits = [ForcePushHit::default(); MAX_FORCE_PUSH_TARGETS];
    let mut len = 0usize;

    for cand in candidates {
        if !cand.alive || !cand.los_clear || !vec_finite(cand.position) {
            continue;
        }
        if !def.affect_friends && caster_team != 0.0 && cand.team == caster_team {
            continue;
        }
        let delta = om::sub(cand.position, caster_pos);
        let dist = om::length(delta);
        if dist <= 1e-6 || dist > def.cone_length {
            continue;
        }
        let dir = om::divide(delta, dist);
        let cos_angle = om::dot(dir, forward);
        if cos_angle < min_cos - 1e-9 {
            continue;
        }

        let horiz_len = v8math::hypot(delta[0], delta[2]);
        let (hx, hz) = if horiz_len > 1e-6 {
            (delta[0] / horiz_len, delta[2] / horiz_len)
        } else {
            (forward[0], forward[2])
        };
        let lift = v8math::abs(def.push) * PROVISIONAL_FORCE_PUSH_VERTICAL_LIFT;
        let impulse = [hx * def.push, lift, hz * def.push];
        let hit = ForcePushHit {
            target_id: cand.target_id,
            distance: dist,
            impulse,
            enters_tumble: lift > PROVISIONAL_FORCE_PUSH_THROWN_SPEED,
            damage: v8math::max(0.0, def.damage),
        };

        // Maintain nearest-first deterministic ordering without allocating.
        let mut insert_at = len;
        while insert_at > 0 && hits[insert_at - 1].distance > hit.distance {
            if insert_at < MAX_FORCE_PUSH_TARGETS {
                hits[insert_at] = hits[insert_at - 1];
            }
            insert_at -= 1;
        }
        if insert_at < max_hits {
            if len < max_hits {
                len += 1;
            }
            hits[insert_at] = hit;
        }
    }

    Some(ForcePushResult {
        energy_spent: def.energy_drain,
        hits,
        len: len as u8,
    })
}

/// ODF-driven parameters for `meleethrow` / `meleethrowordnance` (Saber Throw).
///
/// Damage multipliers (`damage_scales`, `shield_scale`) belong to the thrown
/// ordnance itself rather than the carried melee weapon, while `"FromEdge"`
/// dimensions can be resolved from the primary blade via
/// [`SaberThrowDef::with_primary_edge_dimensions`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SaberThrowDef {
    pub energy_drain: f64,
    pub velocity: f64,
    pub life_span: f64,
    pub return_duration: f64,
    /// Blade spin rate in revolutions per second.
    pub rotation_rate: f64,
    pub damage: f64,
    pub damage_length: f64,
    pub damage_width: f64,
    pub push: f64,
    /// Target category damage scales `[Person, Animal, Droid, Vehicle, Building]`.
    pub damage_scales: [f64; 5],
    /// Shield damage scale authored on the thrown ordnance.
    pub shield_scale: f64,
}

impl Default for SaberThrowDef {
    fn default() -> Self {
        Self {
            energy_drain: 0.0,
            velocity: 20.0,
            life_span: 1.0,
            return_duration: 0.5,
            rotation_rate: 3.0,
            damage: 200.0,
            damage_length: 1.0,
            damage_width: 0.5,
            push: 0.0,
            damage_scales: [1.0; 5],
            shield_scale: 1.0,
        }
    }
}

impl SaberThrowDef {
    /// Resolve `"FromEdge"` blade dimensions against the carried primary weapon
    /// edge (`weapon.edges[0]`) without importing the carried weapon's damage
    /// or shield scales.
    pub fn with_primary_edge_dimensions(
        mut self,
        weapon: &WeaponMeleeSpec,
        length_scale: Option<f64>,
        width_scale: Option<f64>,
    ) -> Self {
        let edge = weapon.edges[0];
        if let Some(s) = length_scale {
            self.damage_length = edge.default_length * s;
        }
        if let Some(s) = width_scale {
            self.damage_width = edge.default_width * s;
        }
        self
    }

    /// Validate that all throw parameters are finite and within safe physical
    /// ranges without heap allocation.
    pub const fn validate(&self) -> Result<(), PowerValidationError> {
        if !self.energy_drain.is_finite() || self.energy_drain < 0.0 {
            return Err(PowerValidationError::InvalidEnergyDrain);
        }
        if !self.velocity.is_finite() || self.velocity <= 0.0 {
            return Err(PowerValidationError::InvalidVelocity);
        }
        if !self.life_span.is_finite() || self.life_span <= 0.0 {
            return Err(PowerValidationError::InvalidLifeSpan);
        }
        if !self.return_duration.is_finite() || self.return_duration <= 0.0 {
            return Err(PowerValidationError::InvalidReturnDuration);
        }
        if !self.rotation_rate.is_finite() {
            return Err(PowerValidationError::InvalidRotationRate);
        }
        if !self.damage.is_finite() || self.damage < 0.0 {
            return Err(PowerValidationError::InvalidDamage);
        }
        if !self.damage_length.is_finite() || self.damage_length <= 0.0 {
            return Err(PowerValidationError::InvalidDamageLength);
        }
        if !self.damage_width.is_finite() || self.damage_width < 0.0 {
            return Err(PowerValidationError::InvalidDamageWidth);
        }
        if !self.push.is_finite() {
            return Err(PowerValidationError::InvalidPush);
        }
        let mut i = 0;
        while i < 5 {
            if !self.damage_scales[i].is_finite() || self.damage_scales[i] < 0.0 {
                return Err(PowerValidationError::InvalidDamageScale);
            }
            i += 1;
        }
        if !self.shield_scale.is_finite() || self.shield_scale < 0.0 {
            return Err(PowerValidationError::InvalidShieldScale);
        }
        Ok(())
    }
}

/// Load-time validated [`SaberThrowDef`] that skips repeated per-tick validation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidatedSaberThrowDef(SaberThrowDef);

impl ValidatedSaberThrowDef {
    pub const fn try_new(def: SaberThrowDef) -> Result<Self, PowerValidationError> {
        match def.validate() {
            Ok(()) => Ok(Self(def)),
            Err(e) => Err(e),
        }
    }

    pub const fn as_def(&self) -> &SaberThrowDef {
        &self.0
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SaberThrowPhase {
    #[default]
    Inactive = 0,
    Outbound = 1,
    Returning = 2,
}

impl SaberThrowPhase {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Inactive),
            1 => Some(Self::Outbound),
            2 => Some(Self::Returning),
            _ => None,
        }
    }
}

/// Complete, copyable prediction checkpoint for an in-flight thrown lightsaber.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SaberThrowCheckpoint {
    pub phase: SaberThrowPhase,
    pub last_sweep_phase: SaberThrowPhase,
    pub position: V3,
    pub prev_position: V3,
    pub direction: V3,
    pub elapsed: f64,
    pub return_elapsed: f64,
    pub out_time: f64,
    pub return_time: f64,
    pub spin_angle: f64,
    pub prev_spin_angle: f64,
    pub hit_history: ThrowHitHistory,
}

/// Compact per-hero runtime state for an in-flight thrown lightsaber.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SaberThrowState {
    pub phase: SaberThrowPhase,
    /// Flight phase during which the most recent `step()` sweep was generated.
    pub last_sweep_phase: SaberThrowPhase,
    pub position: V3,
    pub prev_position: V3,
    pub direction: V3,
    pub elapsed: f64,
    /// Time spent in the return phase, counted from the end of the outbound pass.
    pub return_elapsed: f64,
    /// Length of the outbound pass: `LifeSpan` scaled by the share of the full
    /// range that is free of static geometry.
    pub out_time: f64,
    /// Length of the return pass: `ReturnDuration` scaled by the same share.
    pub return_time: f64,
    pub spin_angle: f64,
    pub prev_spin_angle: f64,
    pub hit_history: ThrowHitHistory,
}

impl Default for SaberThrowState {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome of advancing [`SaberThrowState`] by one fixed tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SaberThrowStep {
    pub active: bool,
    /// Flight phase of `sweep` for this tick (`Outbound` or `Returning`).
    pub phase: SaberThrowPhase,
    pub sweep: Option<BladeSweep>,
    /// True on the tick the returning saber reaches the caster.
    pub caught: bool,
    /// State entry and cues produced by the coupled catch.
    pub catch_entry: Option<StateEntry>,
    /// True if the saber was dropped because the caster died mid-flight.
    pub cancelled: bool,
}

impl SaberThrowState {
    const OUTBOUND_KEY: u32 = 0;
    const RETURN_KEY: u32 = 1;

    pub const fn new() -> Self {
        Self {
            phase: SaberThrowPhase::Inactive,
            last_sweep_phase: SaberThrowPhase::Inactive,
            position: [0.0; 3],
            prev_position: [0.0; 3],
            direction: [0.0, 0.0, 1.0],
            elapsed: 0.0,
            return_elapsed: 0.0,
            out_time: 0.0,
            return_time: 0.0,
            spin_angle: 0.0,
            prev_spin_angle: 0.0,
            hit_history: ThrowHitHistory::new(),
        }
    }

    pub const fn is_active(&self) -> bool {
        !matches!(self.phase, SaberThrowPhase::Inactive)
    }

    /// Capture the complete in-flight thrown lightsaber state for checkpointing
    /// or wire reconciliation.
    pub fn checkpoint(&self) -> SaberThrowCheckpoint {
        SaberThrowCheckpoint {
            phase: self.phase,
            last_sweep_phase: self.last_sweep_phase,
            position: self.position,
            prev_position: self.prev_position,
            direction: self.direction,
            elapsed: self.elapsed,
            return_elapsed: self.return_elapsed,
            out_time: self.out_time,
            return_time: self.return_time,
            spin_angle: self.spin_angle,
            prev_spin_angle: self.prev_spin_angle,
            hit_history: self.hit_history,
        }
    }

    /// Validate and atomically restore from an authoritative thrown-saber
    /// checkpoint. On error, `self` is left unchanged.
    pub fn try_restore(&mut self, cp: SaberThrowCheckpoint) -> Result<(), CheckpointError> {
        let vec_ok = |v: V3| v[0].is_finite() && v[1].is_finite() && v[2].is_finite();
        if !vec_ok(cp.position)
            || !vec_ok(cp.prev_position)
            || !vec_ok(cp.direction)
        {
            return Err(CheckpointError::InvalidVector);
        }
        if cp.phase != SaberThrowPhase::Inactive
            && (om::length_sq(cp.direction) - 1.0).abs() > UNIT_DIR_SQ_TOL
        {
            return Err(CheckpointError::InvalidDirection);
        }
        if !cp.elapsed.is_finite()
            || cp.elapsed < 0.0
            || !cp.return_elapsed.is_finite()
            || cp.return_elapsed < 0.0
            || !cp.out_time.is_finite()
            || cp.out_time < 0.0
            || !cp.return_time.is_finite()
            || cp.return_time < 0.0
            || !cp.spin_angle.is_finite()
            || !cp.prev_spin_angle.is_finite()
        {
            return Err(CheckpointError::InvalidTime);
        }
        if cp.hit_history.has_duplicates() {
            return Err(CheckpointError::InvalidDedupCount);
        }
        for rec in cp.hit_history.lists() {
            if rec.len() > MAX_TARGETS_PER_ATTACK {
                return Err(CheckpointError::InvalidDedupCount);
            }
            if rec.attack_key > Self::RETURN_KEY || rec.shared_id != 0 {
                return Err(CheckpointError::DedupGroupOutOfRange);
            }
            if rec.expires_at != 0.0 {
                return Err(CheckpointError::InvalidTime);
            }
        }
        self.phase = cp.phase;
        self.last_sweep_phase = cp.last_sweep_phase;
        self.position = cp.position;
        self.prev_position = cp.prev_position;
        self.direction = cp.direction;
        self.elapsed = cp.elapsed;
        self.return_elapsed = cp.return_elapsed;
        self.out_time = cp.out_time;
        self.return_time = cp.return_time;
        self.spin_angle = cp.spin_angle;
        self.prev_spin_angle = cp.prev_spin_angle;
        self.hit_history = cp.hit_history;
        Ok(())
    }

    /// Launch using a load-time validated [`ValidatedSaberThrowDef`].
    pub fn launch_validated(
        &mut self,
        def: &ValidatedSaberThrowDef,
        origin: V3,
        aim_dir: V3,
        energy: &mut dyn CombatEnergy,
    ) -> bool {
        self.launch_core(def.as_def(), origin, aim_dir, energy, 1.0)
    }

    /// [`Self::launch_validated_reaching`] for a definition that is validated here.
    pub fn launch_reaching(
        &mut self,
        def: &SaberThrowDef,
        origin: V3,
        aim_dir: V3,
        energy: &mut dyn CombatEnergy,
        reach: f64,
    ) -> bool {
        if def.validate().is_err() {
            return false;
        }
        self.launch_core(def, origin, aim_dir, energy, reach)
    }

    /// Launch when static geometry stops the throw before the end of its range.
    /// `reach` is the free share of the full range (`Velocity x LifeSpan`), from 0 to 1:
    /// the outbound pass lasts `LifeSpan x reach` and the return pass
    /// `ReturnDuration x reach`, so a throw that meets a wall at once is caught again at once.
    pub fn launch_validated_reaching(
        &mut self,
        def: &ValidatedSaberThrowDef,
        origin: V3,
        aim_dir: V3,
        energy: &mut dyn CombatEnergy,
        reach: f64,
    ) -> bool {
        self.launch_core(def.as_def(), origin, aim_dir, energy, reach)
    }

    /// Launch the lightsaber if inactive, `def` is valid, and the caster can
    /// pay `energy_drain`. Performs zero heap allocations on failure.
    pub fn launch(
        &mut self,
        def: &SaberThrowDef,
        origin: V3,
        aim_dir: V3,
        energy: &mut dyn CombatEnergy,
    ) -> bool {
        if def.validate().is_err() {
            return false;
        }
        self.launch_core(def, origin, aim_dir, energy, 1.0)
    }

    fn launch_core(
        &mut self,
        def: &SaberThrowDef,
        origin: V3,
        aim_dir: V3,
        energy: &mut dyn CombatEnergy,
        reach: f64,
    ) -> bool {
        if self.is_active()
            || !reach.is_finite()
            || !energy.balance().is_finite()
            || !vec_finite(origin)
            || !vec_finite(aim_dir)
            || !energy.pay(def.energy_drain)
        {
            return false;
        }
        let dir = om::normalize(aim_dir);
        self.phase = SaberThrowPhase::Outbound;
        self.last_sweep_phase = SaberThrowPhase::Outbound;
        self.position = origin;
        self.prev_position = origin;
        self.direction = if om::length_sq(dir) > 1e-12 {
            dir
        } else {
            [0.0, 0.0, 1.0]
        };
        self.elapsed = 0.0;
        self.return_elapsed = 0.0;
        let reach = om::clamp(reach, 0.0, 1.0);
        self.out_time = def.life_span * reach;
        self.return_time = def.return_duration * reach;
        self.spin_angle = 0.0;
        self.prev_spin_angle = 0.0;
        self.hit_history.clear();
        true
    }

    /// Advance the thrown saber using a pre-validated definition.
    pub fn step_validated(
        &mut self,
        def: &ValidatedSaberThrowDef,
        owner_hand_pos: V3,
        owner_alive: bool,
        dt: f64,
    ) -> SaberThrowStep {
        self.step_core(def.as_def(), owner_hand_pos, owner_alive, dt)
    }

    /// Advance the thrown saber by `dt`. When the saber returns to the caster,
    /// `caught` is set and the caller can call [`ComboState::enter_catch`].
    pub fn step(
        &mut self,
        def: &SaberThrowDef,
        owner_hand_pos: V3,
        owner_alive: bool,
        dt: f64,
    ) -> SaberThrowStep {
        if def.validate().is_err() {
            return SaberThrowStep {
                active: false,
                phase: SaberThrowPhase::Inactive,
                sweep: None,
                caught: false,
                catch_entry: None,
                cancelled: false,
            };
        }
        self.step_core(def, owner_hand_pos, owner_alive, dt)
    }

    fn step_core(
        &mut self,
        def: &SaberThrowDef,
        owner_hand_pos: V3,
        owner_alive: bool,
        dt: f64,
    ) -> SaberThrowStep {
        if !self.is_active() || !vec_finite(owner_hand_pos) || !dt.is_finite() || dt <= 0.0 {
            return SaberThrowStep {
                active: false,
                phase: SaberThrowPhase::Inactive,
                sweep: None,
                caught: false,
                catch_entry: None,
                cancelled: false,
            };
        }
        if !owner_alive {
            self.phase = SaberThrowPhase::Inactive;
            self.last_sweep_phase = SaberThrowPhase::Inactive;
            self.hit_history.clear();
            return SaberThrowStep {
                active: false,
                phase: SaberThrowPhase::Inactive,
                sweep: None,
                caught: false,
                catch_entry: None,
                cancelled: true,
            };
        }

        let sweep_phase = self.phase;
        self.last_sweep_phase = sweep_phase;
        self.prev_position = self.position;
        self.prev_spin_angle = self.spin_angle;
        let spin_scale = if sweep_phase == SaberThrowPhase::Returning {
            0.5
        } else {
            1.0
        };
        self.spin_angle += def.rotation_rate * spin_scale * TAU * dt;

        let mut caught = false;
        match sweep_phase {
            SaberThrowPhase::Inactive => {}
            SaberThrowPhase::Outbound => {
                // The last outbound tick flies only what is left of the outbound time, so
                // the blade ends exactly where the free range ends.
                let flying = om::clamp(self.out_time - self.elapsed, 0.0, dt);
                self.elapsed += dt;
                self.position = om::add_scaled(self.position, self.direction, def.velocity * flying);
                if self.elapsed + 1e-6 >= self.out_time {
                    self.phase = SaberThrowPhase::Returning;
                    self.return_elapsed = v8math::max(self.elapsed - self.out_time, 0.0);
                }
            }
            SaberThrowPhase::Returning => {
                // The blade chases the release point carried with the caster: each tick it
                // covers the share of the remaining way that the clock advanced, and the
                // throw ends on the earliest tick whose clock has used up the return time.
                self.return_elapsed += dt;
                let duration = self.return_time;
                if self.return_elapsed + 1e-6 >= duration {
                    caught = true;
                } else {
                    let now = self.return_elapsed / duration;
                    let before = (self.return_elapsed - dt) / duration;
                    let share = om::clamp((now - before) / (1.0 - before), 0.0, 1.0);
                    self.position = om::lerp(self.position, owner_hand_pos, share);
                }
            }
        }

        let prev_seg =
            spinning_segment(self.prev_position, self.prev_spin_angle, def.damage_length);
        let curr_seg = spinning_segment(self.position, self.spin_angle, def.damage_length);
        let sweep = BladeSweep {
            prev: prev_seg,
            curr: curr_seg,
            radius: v8math::max(
                0.0,
                def.damage_width * PROVISIONAL_DAMAGE_WIDTH_RADIUS_SCALE,
            ),
        };

        if caught {
            // Keep `hit_history` and `last_sweep_phase` intact through this tick so
            // `test_target` on the returned catch-tick sweep still deduplicates
            // against the return pass; the next `launch()` resets `hit_history`.
            self.phase = SaberThrowPhase::Inactive;
        }

        SaberThrowStep {
            active: self.is_active(),
            phase: sweep_phase,
            sweep: Some(sweep),
            caught,
            catch_entry: None,
            cancelled: false,
        }
    }

    /// Test the thrown saber's current tick sweep against one target, applying
    /// per-pass (or per-throw) deduplication.
    pub fn test_target(
        &mut self,
        sweep: &BladeSweep,
        target_id: TargetId,
        shapes: &[HitShape],
    ) -> Option<V3> {
        let key = self.current_pass_key();
        if self.hit_history.has_hit(key, 0, target_id) {
            return None;
        }
        let (_, hit_point) = sweep.hit_shapes(shapes)?;
        if !self.hit_history.record(key, 0, 0.0, target_id) {
            return None;
        }
        Some(hit_point)
    }

    /// Resolve a thrown saber sweep against one target using a pre-validated
    /// [`ValidatedSaberThrowDef`].
    pub fn resolve_hit_validated(
        &mut self,
        def: &ValidatedSaberThrowDef,
        sweep: &BladeSweep,
        target: &MeleeTarget<'_>,
        defender_energy: Option<&mut dyn CombatEnergy>,
    ) -> MeleeHitOutcome {
        self.resolve_hit_core(def.as_def(), sweep, target, defender_energy)
    }

    /// Resolve a thrown saber sweep against one target, applying `def.damage`,
    /// `def.push`, the thrown ordnance's own `damage_scales` / `shield_scale`,
    /// per-pass deduplication, and melee block interception gated by
    /// [`PROVISIONAL_DEFLECT_SABER_THROW`].
    pub fn resolve_hit(
        &mut self,
        def: &SaberThrowDef,
        sweep: &BladeSweep,
        target: &MeleeTarget<'_>,
        defender_energy: Option<&mut dyn CombatEnergy>,
    ) -> MeleeHitOutcome {
        if def.validate().is_err() {
            return MeleeHitOutcome::Missed;
        }
        self.resolve_hit_core(def, sweep, target, defender_energy)
    }

    fn resolve_hit_core(
        &mut self,
        def: &SaberThrowDef,
        sweep: &BladeSweep,
        target: &MeleeTarget<'_>,
        defender_energy: Option<&mut dyn CombatEnergy>,
    ) -> MeleeHitOutcome {
        let key = self.current_pass_key();
        let atk = AttackDef {
            attack_group: AttackGroupId(key as u16),
            attack_key: key,
            shared_id: 0,
            edge: 0,
            window: DamageWindow::Seconds {
                start: 0.0,
                end: def.life_span,
            },
            damage_length: def.damage_length,
            damage_width: def.damage_width,
            damage: def.damage,
            push: def.push,
        };
        let throw_weapon = WeaponMeleeSpec {
            damage_scales: def.damage_scales,
            shield_scale: def.shield_scale,
            ..WeaponMeleeSpec::default()
        };
        let effective_target = MeleeTarget {
            deflect: if PROVISIONAL_DEFLECT_SABER_THROW {
                target.deflect
            } else {
                None
            },
            ..*target
        };
        resolve_melee_hit(
            self.prev_position,
            &atk,
            &throw_weapon,
            sweep,
            &mut self.hit_history,
            &effective_target,
            defender_energy,
        )
    }

    #[inline]
    fn current_pass_key(&self) -> u32 {
        if PROVISIONAL_SABER_THROW_SINGLE_HIT_PER_PASS
            && self.last_sweep_phase == SaberThrowPhase::Returning
        {
            Self::RETURN_KEY
        } else {
            Self::OUTBOUND_KEY
        }
    }

    /// Advance the thrown saber using a pre-validated definition and enter
    /// `CATCH_SABRE` on return.
    pub fn step_with_combo_validated(
        &mut self,
        def: &ValidatedSaberThrowDef,
        machine: &ComboMachine,
        combo_state: &mut ComboState,
        owner_hand_pos: V3,
        owner_alive: bool,
        dt: f64,
    ) -> SaberThrowStep {
        let mut out = self.step_validated(def, owner_hand_pos, owner_alive, dt);
        if out.caught {
            out.catch_entry = combo_state.enter_catch(machine);
        }
        out
    }

    /// Advance the thrown saber and automatically transition `combo_state` into
    /// `CATCH_SABRE` (if defined on `machine`) when caught.
    pub fn step_with_combo(
        &mut self,
        def: &SaberThrowDef,
        machine: &ComboMachine,
        combo_state: &mut ComboState,
        owner_hand_pos: V3,
        owner_alive: bool,
        dt: f64,
    ) -> SaberThrowStep {
        let mut out = self.step(def, owner_hand_pos, owner_alive, dt);
        if out.caught {
            out.catch_entry = combo_state.enter_catch(machine);
        }
        out
    }
}

fn spinning_segment(center: V3, angle: f64, length: f64) -> BladeSegment {
    let half = v8math::max(0.0, length) * 0.5;
    let (s, c) = v8math::sin_cos(angle);
    let axis = [s, 0.0, c];
    BladeSegment {
        base: om::add_scaled(center, axis, -half),
        tip: om::add_scaled(center, axis, half),
    }
}
