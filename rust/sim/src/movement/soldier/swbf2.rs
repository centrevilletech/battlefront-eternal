//! Battlefront II infantry movement. Authority and prediction supply the same
//! class tuning and restore the same per-life state before replay.

#![deny(clippy::disallowed_methods)]

use crate::movement::ProfileError;
use super::{
    clamp, js_max, js_min, stick, Air, Input, Jet, Mode, Mover, Posture, Scene, Trig, V3, CROUCH_MODE, GRAVITY,
    HARD_LOCK, LAND_LOCK, STAND_MODE,
};

pub mod body;
pub mod classes;
pub mod words;
// A non-hover jetpack's flight still uses the existing jet coefficient: no class of the 2005
// battles flies one (the hover flight below is the game's).
use super::JET_LOW_SPEED as PROVISIONAL_JET_LOW_SPEED;

mod combat;
pub use combat::{InputLockMask, MovementOverride};

#[cfg(test)]
mod tests;

// Contact geometry and rolling friction still need in-game measurements.
const PROVISIONAL_ROLL_BRAKE: f64 = 0.0;
const PROVISIONAL_ROLL_LEDGE_AIR: Air = Air::Fall;
const PROVISIONAL_FORWARD_ROLL_SIDE_RATIO: f64 = 1.0;
const PROVISIONAL_STANDING_BRAKE: f64 = STAND_MODE.brake;
const PROVISIONAL_CROUCH_BRAKE: f64 = CROUCH_MODE.brake;
const PROVISIONAL_ROLL_POSTURE: Posture = Posture::Crouch;
const SWBF2_AIR_DRAG: f64 = 0.012;
const SWBF2_HARD_LANDING: f64 = 1.5;
const SWBF2_FALL_WINDOW: f64 = 1.0;
const SWBF2_UNSUPPORTED_GRACE: f64 = 0.3;
const SWBF2_LOCKED_ACCELERATION: f64 = 1.7;
const SWBF2_STEEP_NORMAL: f64 = 0.7;
const SWBF2_STEEP_ACCELERATION: f64 = 0.1;
/// Hover flight: the velocity's decay rate a second, and the aim's height lift and scale.
const SWBF2_HOVER_DECAY: f64 = -2.0;
const SWBF2_HOVER_AIM_LIFT: f64 = 0.08;
const SWBF2_HOVER_AIM_RISE: f64 = 2.0;
/// The turn scale standing, crouched, sprinting and in the jet: the posture's ControlSpeed turn row
/// for a player, four times MaxTurnSpeed with no row for a bot (the game's soldier tick).
fn ground_turn(row: f64, ai: bool) -> f64 {
    if ai { SWBF2_AI_TURN } else { row }
}

/// The turn scale in a jump, a fall, a roll or a tumble: the row for everyone, times four for a bot.
fn air_turn(row: f64, ai: bool) -> f64 {
    if ai { SWBF2_AI_TURN * row } else { row }
}

/// A bot's jump leaves along its jet vector only when the vector's squared length is over this.
const SWBF2_DIRECTED_JUMP_MIN: f64 = 0.1;
/// The height the directed jump gives the vector before normalizing it.
const SWBF2_DIRECTED_JUMP_LIFT: f64 = 0.01;

pub(crate) const THROWN_AIR_DRAG: f64 = 0.012;
const THROWN_MIN_AIR_TIME: f64 = 0.3;
const THROWN_BOUNCE_WINDOW: f64 = 0.5;
pub(crate) const THROWN_RECOVER_DELAY: f64 = 0.5;
pub(crate) const THROWN_RECOVER_DURATION: f64 = 0.5;
const THROWN_TUMBLE_SPEED_SQ: f64 = 36.0;
const THROWN_MAYBE_TUMBLE_SPEED_SQ: f64 = 9.0;
const THROWN_RECOVER_FAIL_SPEED_SQ: f64 = 64.0;
pub(crate) const TUMBLE_FRICTION: f64 = 2.5;
pub(crate) const TUMBLE_MIN_TIME: f64 = 0.5;
pub(crate) const TUMBLE_MAX_TIME: f64 = 0.75;
pub(crate) const TUMBLE_END_SPEED_SQ: f64 = 16.0;
pub(crate) const TUMBLE_CLIP_DURATION: f64 = 32.0 / 30.0;
pub(crate) const TUMBLE_CLIP_SPEED: f64 = 7.14;
pub(crate) const LANDED_FRICTION: f64 = 10.0;
pub(crate) const LANDED_STOP_SPEED_SQ: f64 = 0.36;
pub(crate) const LANDED_MAX_TIME: f64 = 1.0;
/// Numeric guard avoiding `atan2(0, 0)` when aligning tumble heading to horizontal velocity.
const TUMBLE_HEADING_SPEED_SQ_EPSILON: f64 = 0.01;

/// Standard human stand-up clip durations when a class inherits the shared rig.
pub const DEFAULT_GETUP_FRONT_SECONDS: f64 = 57.0 / 30.0;
pub const DEFAULT_GETUP_BACK_SECONDS: f64 = 52.0 / 30.0;

/// A resolved ControlSpeed row, ordered forward, strafe, then turn.
#[derive(Clone, Copy, Debug)]
pub struct ControlSpeed {
    pub forward: f64,
    pub strafe: f64,
    pub turn: f64,
}

/// Resolved per-posture `ControlSpeed` turn multipliers for a 2005 class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurnScales {
    pub stand: f64,
    pub crouch: f64,
    pub sprint: f64,
    pub jump: f64,
    pub jet: f64,
    pub roll: f64,
    pub tumble: f64,
}

impl Default for TurnScales {
    fn default() -> Self {
        Self {
            stand: 1.0,
            crouch: 1.0,
            sprint: 1.0,
            jump: 1.0,
            jet: 1.0,
            roll: 1.0,
            tumble: 1.0,
        }
    }
}

impl TurnScales {
    /// Turn-rate multiplier for the active 2005 posture and air state.
    pub fn for_state(
        self,
        posture: crate::gen::Posture,
        air: Option<crate::gen::Air>,
        sprinting: bool,
        rolling: bool,
    ) -> f64 {
        match air {
            Some(crate::gen::Air::Thrown) => self.tumble,
            Some(crate::gen::Air::Jet) => self.jet,
            Some(crate::gen::Air::Jump | crate::gen::Air::Fall) => self.jump,
            None if rolling => self.roll,
            None if sprinting => self.sprint,
            None if posture == crate::gen::Posture::Crouch => self.crouch,
            None => self.stand,
        }
    }
}

impl ControlSpeed {
    fn mode(self, brake: f64) -> Mode {
        Mode {
            forward: self.forward,
            strafe: self.strafe,
            turn: self.turn,
            brake,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Jetpack {
    /// JetJump, JetPush, JetFuelRechargeRate, JetFuelCost, JetFuelInitialCost,
    /// JetFuelMinBorder and the hover flag from JetType.
    pub physics: Jet,
    pub acceleration: f64,
    /// One-shot stamina charge paid on ignition (JetEnergyDrain).
    pub energy_cost: f64,
    /// Non-hover melee classes perform a Force double jump instead of entering flight.
    pub force_jump: bool,
}

/// Class data resolved through ODF inheritance. The mover's usual class
/// fields still supply speed, strafe speed, turn speed and acceleration.
/// Rates and costs are finite and nonnegative. The floor is at or below zero.
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    pub stand: ControlSpeed,
    pub crouch: ControlSpeed,
    pub sprint: ControlSpeed,
    pub jump: ControlSpeed,
    pub jet: ControlSpeed,
    /// None for a class without jet or Force flight.
    pub jetpack: Option<Jetpack>,
    pub roll: ControlSpeed,
    pub tumble: ControlSpeed,
    pub sprint_accelerate_time: f64,
    pub energy_capacity: f64,
    /// Lowest balance after a charge, zero for ordinary stock infantry.
    pub energy_floor: f64,
    pub energy_restore: f64,
    pub energy_restore_idle: f64,
    pub energy_drain_sprint: f64,
    pub energy_min_sprint: f64,
    pub energy_cost_jump: f64,
    /// EnergyCostJumpSprint when supplied by the class.
    pub energy_cost_jump_sprint: Option<f64>,
    pub energy_cost_roll: f64,
    pub jump_height: f64,
    pub jump_forward_speed_factor: f64,
    pub jump_strafe_speed_factor: f64,
    pub roll_speed_factor: f64,
    /// Frame count of the equipped roll action. Zero means the action is absent.
    pub roll_frames: u32,
    /// `RecoverFromTumble`: whether pressing jump midair recovers out of a throw.
    pub recover_from_tumble: bool,
    /// Stand-up lock duration from a face-down landing, in seconds.
    pub getup_front_seconds: f64,
    /// Stand-up lock duration from a face-up landing, in seconds.
    pub getup_back_seconds: f64,
}

fn nonnegative(value: f64) -> bool { value.is_finite() && value >= 0.0 }

impl Tuning {
    /// May also be checked when resolving a prepared class.
    pub fn validate(&self) -> Result<(), ProfileError> {
        let controls = [self.stand, self.crouch, self.sprint, self.jump, self.jet, self.roll, self.tumble];
        let scalars = [self.sprint_accelerate_time, self.energy_capacity, self.energy_restore,
            self.energy_restore_idle, self.energy_drain_sprint, self.energy_min_sprint,
            self.energy_cost_jump, self.energy_cost_roll, self.jump_height,
            self.jump_forward_speed_factor, self.jump_strafe_speed_factor,
            self.roll_speed_factor, self.getup_front_seconds, self.getup_back_seconds];
        if !controls.into_iter().all(|row| [row.forward, row.strafe, row.turn].into_iter().all(nonnegative))
            || !scalars.into_iter().all(nonnegative) || !(2.0 * GRAVITY * self.jump_height).is_finite() {
            return Err(ProfileError::InvalidTuning);
        }
        if !self.energy_floor.is_finite() || self.energy_floor > 0.0
            || self.energy_min_sprint > self.energy_capacity
            || self.energy_cost_jump_sprint.is_some_and(|cost| !nonnegative(cost)) {
            return Err(ProfileError::InvalidTuning);
        }
        if let Some(pack) = self.jetpack {
            let j = pack.physics;
            if ![j.jump, j.push, j.recharge, j.cost, j.initial, j.min_border, pack.acceleration, pack.energy_cost]
                .into_iter().all(nonnegative) {
                return Err(ProfileError::InvalidTuning);
            }
        }
        Ok(())
    }

    /// Elapsed time must exceed this animation window to end a roll.
    pub fn roll_seconds(&self) -> f64 {
        0.75 * f64::from(self.roll_frames.saturating_sub(1)) / 30.0
    }

    /// Resolved per-posture turn speed scales from this class's `ControlSpeed`.
    pub fn turn_scales(&self) -> TurnScales {
        TurnScales {
            stand: self.stand.turn,
            crouch: self.crouch.turn,
            sprint: self.sprint.turn,
            jump: self.jump.turn,
            jet: self.jet.turn,
            roll: self.roll.turn,
            tumble: self.tumble.turn,
        }
    }

}

/// Supplemental infantry actions alongside the common stick and posture input.
#[derive(Clone, Copy, Debug, Default)]
pub struct Actions {
    /// Held each tick while sprint is requested.
    pub sprint: bool,
    /// One press for a roll. Repeated delivery of the press is handled upstream.
    pub roll: bool,
    /// A bot's soldier. The game turns it at four times MaxTurnSpeed, and leaves out the
    /// ControlSpeed turn row standing, crouched, sprinting and in the jet (a player's turn keeps
    /// the row everywhere; jump, fall, roll and tumble apply it to both).
    pub ai: bool,
}

/// A bot soldier's turn rate over MaxTurnSpeed.
pub const SWBF2_AI_TURN: f64 = 4.0;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollDirection {
    Forward = 1,
    Left = 2,
    Right = 3,
}

#[derive(Clone, Copy, Debug)]
pub struct Roll {
    pub direction: RollDirection,
    pub elapsed: f64,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TumblePhase {
    /// Airborne after a push, pull, or blast.
    Thrown = 1,
    /// Tumbling along the ground after a fast landing.
    Tumble = 2,
    /// Sliding to a halt on the ground after landing or tumbling.
    Landed = 3,
    /// Standing back up with movement, turning and weapons locked.
    GetUp = 4,
    /// Midair recovery flip for classes with `RecoverFromTumble`.
    Recover = 5,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tumble {
    pub phase: TumblePhase,
    pub elapsed: f64,
    pub air_time: f64,
    pub angle: f64,
    pub spin_rate: f64,
    pub front: bool,
    pub bounce: bool,
    /// Initial airborne pose direction in body space (0=front, 1=back, 2=left, 3=right).
    pub fly_dir: u8,
    /// Body yaw offset from `aim_yaw` aligned to ground travel and eased back on stand-up.
    pub heading_offset: f64,
    pub cycle_phase: f64,
    /// True once the life dies during a tumble so it neither recovers nor stands up.
    pub dead: bool,
}

fn deterministic_unit(mut x: u32) -> f64 {
    x = x.wrapping_add(0x9e37_79b9);
    x = (x ^ (x >> 16)).wrapping_mul(0x7feb_352d);
    x = (x ^ (x >> 15)).wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    f64::from(x) * (1.0 / 4_294_967_296.0)
}

fn wrap_pi(mut a: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let pi = std::f64::consts::PI;
    if !a.is_finite() {
        return 0.0;
    }
    while a > pi {
        a -= tau;
    }
    while a < -pi {
        a += tau;
    }
    a
}

fn wrap_tau(mut a: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    if !a.is_finite() {
        return 0.0;
    }
    while a >= tau {
        a -= tau;
    }
    while a < 0.0 {
        a += tau;
    }
    a
}

pub(crate) fn wrap_unit(mut x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    while x >= 1.0 {
        x -= 1.0;
    }
    while x < 0.0 {
        x += 1.0;
    }
    x
}

fn ground_heading_offset(vx: f64, vz: f64, aim_yaw: f64, front: bool) -> f64 {
    let travel = v8math::atan2(vx, vz);
    let aligned = if front { travel } else { travel + std::f64::consts::PI };
    wrap_pi(aligned - aim_yaw)
}

impl Tumble {
    pub fn enter(velocity: V3, aim_yaw: f64, seed: u32) -> Self {
        let sy = v8math::sin(aim_yaw);
        let cy = v8math::cos(aim_yaw);
        let forward = velocity.x * sy + velocity.z * cy;
        let right = velocity.z * sy - velocity.x * cy;
        let fly_dir = if v8math::abs(right) > v8math::abs(forward) {
            if right < 0.0 { 2 } else { 3 }
        } else if forward < 0.0 {
            1
        } else {
            0
        };
        let mixed = seed
            ^ (velocity.x as f32).to_bits().rotate_left(17)
            ^ (velocity.z as f32).to_bits().rotate_right(11);
        let spin_rate = deterministic_unit(mixed) * 20.0 - 10.0;
        Self {
            phase: TumblePhase::Thrown,
            elapsed: 0.0,
            air_time: 0.0,
            angle: 0.0,
            spin_rate,
            front: true,
            bounce: false,
            fly_dir,
            heading_offset: 0.0,
            cycle_phase: 0.0,
            dead: false,
        }
    }

    /// Enter ground tumble directly from a fast horizontal push on the ground.
    pub fn enter_ground(velocity: V3, aim_yaw: f64) -> Self {
        let sy = v8math::sin(aim_yaw);
        let cy = v8math::cos(aim_yaw);
        let forward = velocity.x * sy + velocity.z * cy;
        let front = forward > 0.0;
        let speed_xz_sq = velocity.x * velocity.x + velocity.z * velocity.z;
        let heading_offset = if speed_xz_sq > TUMBLE_HEADING_SPEED_SQ_EPSILON {
            ground_heading_offset(velocity.x, velocity.z, aim_yaw, front)
        } else {
            0.0
        };
        Self {
            phase: TumblePhase::Tumble,
            elapsed: 0.0,
            air_time: 0.0,
            angle: 0.0,
            spin_rate: 0.0,
            front,
            bounce: false,
            fly_dir: if front { 0 } else { 1 },
            heading_offset,
            cycle_phase: 0.0,
            dead: false,
        }
    }

    pub fn valid(&self) -> bool {
        nonnegative(self.elapsed)
            && nonnegative(self.air_time)
            && self.angle.is_finite()
            && self.spin_rate.is_finite()
            && self.heading_offset.is_finite()
            && self.cycle_phase.is_finite()
            && self.fly_dir < 4
    }
}

/// What a body says about its footing, as the world left it for this tick.
#[derive(Clone, Copy, Debug)]
pub struct BodyFacts {
    pub air: Air,
    pub grounded: bool,
    pub lock: f64,
    pub posture: Posture,
}

/// Extra movement state for one soldier life. All time values are in seconds.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub energy: f64,
    /// Latched by an overdraw and cleared on recovery to EnergyMinSprint.
    pub exhausted: bool,
    /// Time spent accelerating, capped at the class's sprint acceleration time.
    pub sprint_time: Option<f64>,
    pub roll: Option<Roll>,
    /// Posture cooldown after a soft landing; blocks jump, crouch, sprint and roll.
    pub land_cooldown: f64,
    /// Time spent unsupported while staying in Stand/Crouch before entering Fall.
    pub unsupported_time: Option<f64>,
    /// Incremented on every ground jump and Force double jump in this life.
    pub jump_count: u32,
    pub tumble: Option<Tumble>,
}

impl State {
    pub fn new(tuning: &Tuning) -> Self {
        Self {
            energy: tuning.energy_capacity,
            exhausted: false,
            sprint_time: None,
            roll: None,
            land_cooldown: 0.0,
            unsupported_time: None,
            jump_count: 0,
            tumble: None,
        }
    }

    /// Ends every window of this life that the body's own fields no longer allow.
    ///
    /// A body can be moved by something other than its own step: a push, a pull or
    /// a blast throws it, a power tumbles it, a seat takes it and puts it down. The
    /// windows below belong to the state the body was in. The game keeps them as
    /// the time since the state began and restarts that time on every state change,
    /// so none carries over; here each one ends when the body shows that the state
    /// is gone. Returns whether one ended. Values that are out of range stay for
    /// the step's own validation to refuse.
    pub fn end_windows_the_body_left(&mut self, body: BodyFacts) -> bool {
        let airborne = body.air != Air::None;
        let held_down = !body.grounded || airborne || body.lock > 0.0 || self.tumble.is_some();
        let mut ended = false;
        if airborne && self.unsupported_time.is_some() {
            self.unsupported_time = None;
            ended = true;
        }
        if self.sprint_time.is_some() && (held_down || self.exhausted || body.posture != Posture::Stand) {
            self.sprint_time = None;
            ended = true;
        }
        if self.roll.is_some() && (held_down || body.posture != PROVISIONAL_ROLL_POSTURE) {
            self.roll = None;
            ended = true;
        }
        ended
    }

    /// Whether mouse look should use the class's `tumble` turn scale.
    pub fn is_tumble_look(self) -> bool {
        self.tumble.is_some_and(|t| {
            matches!(
                t.phase,
                TumblePhase::Thrown | TumblePhase::Tumble | TumblePhase::Landed
            )
        })
    }

    /// Whether the shared stamina latch permits a cost. An overdraw is permitted once.
    pub fn can_charge(&self, cost: f64) -> bool {
        nonnegative(cost) && (cost == 0.0 || !self.exhausted)
    }

    /// Debit a resolved class cost, returning whether the action can proceed.
    pub fn charge(&mut self, cost: f64, tuning: &Tuning) -> bool {
        if !self.can_charge(cost) { return false; }
        if cost == 0.0 { return true; }
        let balance = self.energy - cost;
        self.exhausted = balance < 0.0;
        if self.exhausted { self.sprint_time = None; }
        self.energy = js_max(balance, tuning.energy_floor);
        true
    }

    /// Restore a positive amount and rearm the latch at the class threshold.
    pub fn recover(&mut self, amount: f64, tuning: &Tuning) {
        if !amount.is_finite() || amount <= 0.0 { return; }
        self.energy = js_min(self.energy + amount, tuning.energy_capacity);
        if self.energy >= tuning.energy_min_sprint { self.exhausted = false; }
    }

    /// Apply one signed recovery rate for an action. A blocked debit returns false.
    pub fn apply_energy_rate(&mut self, rate: f64, dt: f64, tuning: &Tuning) -> bool {
        let amount = rate * dt;
        if !nonnegative(dt) || !amount.is_finite() { return false; }
        if amount < 0.0 { return self.charge(-amount, tuning); }
        self.recover(amount, tuning);
        true
    }

    /// Suggested posture for combat's body and head queries. Combat integration
    /// and the rolling hit geometry still need verification.
    pub fn hit_posture(&self, stance: Posture) -> Posture {
        if self.roll.is_some()
            || matches!(
                self.tumble.map(|t| t.phase),
                Some(TumblePhase::Tumble | TumblePhase::Landed)
            )
        {
            PROVISIONAL_ROLL_POSTURE
        } else {
            stance
        }
    }
}

/// State and inputs for one tick, sent directly until both are replicated.
pub struct Step<'a> {
    pub actions: Actions,
    pub tuning: &'a Tuning,
    pub state: &'a mut State,
    pub override_: Option<MovementOverride>,
}

impl Mover {
    fn finite_body(&self) -> bool {
        [self.position, self.velocity, self.normal, self.settled, self.avoidance, self.contact_mean]
            .iter().all(|v| v.arr().into_iter().all(f64::is_finite))
            && [self.aim_yaw, self.aim_pitch, self.state_time, self.lock, self.fuel,
                self.dive, self.ground_adjustment, self.collision_damage, self.contact_count].into_iter().all(f64::is_finite)
    }

    fn finish_motion(&self, trig: &mut Trig, yaw: f64) -> Result<(), ProfileError> {
        // Finite inputs can overflow during drive. Never query geometry with
        // a nonfinite position, velocity, or heading.
        if !self.finite_body() { return Err(ProfileError::InvalidState); }
        trig.after_turn(yaw, self.aim_yaw);
        Ok(())
    }

    /// Validate the complete tick before touching a retained body or its life state.
    /// Returns whether a validated roll is active at the beginning of the tick.
    pub(crate) fn validate_swbf2(&self, input: &Input, dt: f64, step: &Step<'_>)
        -> Result<bool, ProfileError> {
        if !nonnegative(dt) { return Err(ProfileError::InvalidDelta); }
        let tuning = step.tuning;
        let state = &*step.state;
        tuning.validate()?;
        if let Some(controls) = &step.override_ { controls.validate(dt)?; }
        let roll_target = self.max_speed * tuning.roll_speed_factor;
        if ![self.max_speed, self.max_strafe, self.max_jump, self.max_turn, self.max_pitch, self.accel, self.getup_time,
             self.dive_time, roll_target * roll_target].into_iter().all(nonnegative)
            || !self.max_fly_height.is_finite()
            || !self.pitch_limits.into_iter().chain(self.threshold).chain(self.scale).all(f64::is_finite) {
            return Err(ProfileError::InvalidTuning);
        }
        if self.posture == Posture::Prone || !self.finite_body() || !(tuning.energy_floor..=tuning.energy_capacity).contains(&state.energy)
            || (state.energy < 0.0 && !state.exhausted)
            || !nonnegative(state.land_cooldown)
            || state.unsupported_time.is_some_and(|t| !nonnegative(t) || self.air != Air::None)
            || state.sprint_time.is_some_and(|t| !(0.0..=tuning.sprint_accelerate_time).contains(&t)
                || state.exhausted || !self.grounded || self.air != Air::None || self.lock > 0.0
                || self.posture != Posture::Stand)
            || (self.air == Air::Jet && tuning.jetpack.is_none())
            || ![input.forward, input.right, input.turn, input.pitch].into_iter().all(f64::is_finite)
            || input.jet_direction.is_some_and(|v| !v.arr().into_iter().all(f64::is_finite)) {
            return Err(ProfileError::InvalidState);
        }
        if ![tuning.energy_drain_sprint, tuning.energy_restore, tuning.energy_restore_idle]
            .into_iter().all(|rate| (rate * dt).is_finite())
            || tuning.jetpack.is_some_and(|p| !(p.physics.cost * dt).is_finite() || !(p.physics.recharge * dt).is_finite()) {
            return Err(ProfileError::InvalidDelta);
        }
        let rolling = state.roll.is_some_and(|r| nonnegative(r.elapsed) && r.elapsed <= tuning.roll_seconds()
            && tuning.roll_frames > 0);
        if state.tumble.is_some_and(|t| !t.valid() || state.sprint_time.is_some()) {
            return Err(ProfileError::InvalidState);
        }
        if rolling && (self.posture != PROVISIONAL_ROLL_POSTURE || state.sprint_time.is_some()
            || state.tumble.is_some()
            || !self.grounded || self.air != Air::None || self.lock > 0.0) {
            return Err(ProfileError::InvalidState);
        }
        Ok(rolling)
    }

    pub(in crate::movement) fn advance_swbf2(
        &mut self, input: &Input, dt: f64, scene: &mut Scene, step: Step<'_>,
    ) -> Result<bool, ProfileError> {
        let rolling = self.validate_swbf2(input, dt, &step)?;
        let Step { actions, tuning, state, override_ } = step;
        if dt == 0.0 { return Ok(!rolling && state.tumble.is_none() && state.sprint_time.is_none() && self.lock == 0.0); }

        // Finite operands can still overflow during integration. Publish the
        // body and its per-life state together only after the full step succeeds.
        let mut next = self.clone();
        let mut life = *state;
        if life.roll.is_some() && !rolling {
            life.roll = None;
            // Discard stale roll momentum before ordinary ground movement resumes.
            if next.grounded { next.velocity = V3::ZERO; next.finish_roll(); }
        }
        let controls = override_.unwrap_or_default();
        let (input, actions) = controls.input_lock.filter(*input, actions);
        if life.tumble.is_none() && next.air != Air::Thrown {
            if let Some(yaw) = controls.aim_yaw { next.aim_yaw = yaw; }
        }
        let permission = next.step_swbf2(&input, dt, scene, actions, tuning, &mut life, &controls)?;
        if let Some(rate) = controls.energy_rate { life.apply_energy_rate(rate, dt, tuning); }
        if !next.grounded || next.air != Air::None || next.lock > 0.0 {
            life.sprint_time = None;
        }
        if !next.finite_body() || !life.energy.is_finite() {
            return Err(ProfileError::InvalidState);
        }
        *self = next;
        *state = life;
        Ok(permission)
    }

    fn step_swbf2(&mut self, input: &Input, dt: f64, scene: &mut Scene, actions: Actions,
        tuning: &Tuning, state: &mut State, controls: &MovementOverride) -> Result<bool, ProfileError> {
        let yaw = self.aim_yaw;
        let mut trig = Trig { sin0: v8math::sin(yaw), cos0: v8math::cos(yaw), ..Trig::default() };
        if self.air == Air::Jet && tuning.jetpack.is_some_and(|p| p.physics.hover) {
            trig.sin_pitch = v8math::sin(self.aim_pitch);
            trig.cos_pitch = v8math::cos(self.aim_pitch);
        }
        self.jet = tuning.jetpack.map_or_else(Jet::default, |pack| pack.physics);
        let recharge_fuel = tuning.jetpack.is_some() && self.air != Air::Jet;
        let locked = self.tick_timers(dt, recharge_fuel);
        if self.lock <= 1e-9 {
            self.lock = 0.0;
        }
        let land_cooling = state.land_cooldown > 0.0;
        state.land_cooldown = js_max(state.land_cooldown - dt, 0.0);
        if state.land_cooldown <= 1e-9 {
            state.land_cooldown = 0.0;
        }
        self.dive = 0.0;
        if self.air == Air::Thrown && state.tumble.is_none() {
            state.tumble = Some(Tumble::enter(self.velocity, self.aim_yaw, 1));
        }
        if let Some(tumble) = state.tumble {
            state.roll = None;
            state.sprint_time = None;
            state.tumble = self.tumble_tick(dt, trig, scene, tuning, tumble)?;
            return Ok(false);
        }
        if self.air == Air::None {
            if self.grounded {
                state.unsupported_time = None;
            } else if state.roll.is_none() {
                let elapsed = match state.unsupported_time {
                    Some(t) => t + dt,
                    None => 0.0,
                };
                if elapsed > SWBF2_FALL_WINDOW + 1e-9 {
                    state.unsupported_time = None;
                    self.set_air(Air::Fall);
                } else if self.posture == Posture::Crouch && elapsed > SWBF2_UNSUPPORTED_GRACE + 1e-9 {
                    self.set_posture(Posture::Stand, scene);
                    state.unsupported_time = Some(if self.posture == Posture::Stand {
                        SWBF2_UNSUPPORTED_GRACE
                    } else {
                        elapsed
                    });
                } else {
                    state.unsupported_time = Some(elapsed);
                }
            }
        } else {
            state.unsupported_time = None;
        }

        let on_foot = !locked && self.grounded && self.air == Air::None;
        let grace_jump = !locked
            && self.air == Air::None
            && state.roll.is_none()
            && state.unsupported_time.is_some_and(|t| t < SWBF2_UNSUPPORTED_GRACE);
        let f = clamp(input.forward, -1.0, 1.0);
        let s = clamp(input.right, -1.0, 1.0);
        let directed = f * f + s * s >= 0.81;
        let stick_roll = !land_cooling && actions.roll && directed && (f > 0.0 || s.abs() > -0.839 * f);
        let try_roll = on_foot && state.roll.is_none() && (controls.force_roll.is_some() || stick_roll);
        if try_roll && tuning.roll_speed_factor > 0.0 && tuning.roll_frames > 0
            && state.charge(tuning.energy_cost_roll, tuning) {
            let direction = controls.force_roll.unwrap_or_else(|| {
                if f > 0.0 && s.abs() <= PROVISIONAL_FORWARD_ROLL_SIDE_RATIO * f { RollDirection::Forward }
                else if s < 0.0 { RollDirection::Left } else { RollDirection::Right }
            });
            self.boost_roll(&trig, self.max_speed * tuning.roll_speed_factor);
            self.posture = PROVISIONAL_ROLL_POSTURE;
            self.state_time = 0.0;
            state.unsupported_time = None;
            state.roll = Some(Roll { direction, elapsed: 0.0 });
        }
        if let Some(roll) = state.roll {
            state.sprint_time = None;
            state.unsupported_time = None;
            state.roll = self.roll_tick(input, dt, trig, scene, tuning, roll, controls, actions.ai)?;
            return Ok(false);
        }

        let free = on_foot && !try_roll;
        let can_posture = free || grace_jump;
        let continuing = state.sprint_time.is_some();
        let forward = self.velocity.x * trig.sin0 + self.velocity.z * trig.cos0;
        // Crouch and jump are exclusive posture requests. A crouched jump
        // requests clearance to stand; a later press can launch.
        let jump_requested = (!land_cooling && input.jump) || controls.force_jump;
        let change_stance = can_posture && !continuing
            && (((!land_cooling && input.crouch) && !controls.force_jump)
                || (self.posture == Posture::Crouch && jump_requested));
        let mut jumping = false;
        if change_stance {
            let next = if self.posture == Posture::Crouch { Posture::Stand } else { Posture::Crouch };
            self.set_posture(next, scene);
        } else if can_posture && jump_requested && self.posture == Posture::Stand {
            let cost = if forward > self.max_speed + 0.0001 {
                js_max(tuning.energy_cost_jump, tuning.energy_cost_jump_sprint.unwrap_or(0.0))
            } else { tuning.energy_cost_jump };
            jumping = state.charge(cost, tuning);
            if jumping {
                self.launch_swbf2_jump(tuning, &trig, input);
                state.unsupported_time = None;
                state.jump_count = state.jump_count.wrapping_add(1);
            }
        }

        // A failed roll consumes the posture request, including the sprint-stop test.
        let sprinting = if try_roll {
            continuing
        } else {
            free && !land_cooling && !change_stance && !jumping && self.posture == Posture::Stand && actions.sprint
                && !state.exhausted && tuning.sprint.forward > tuning.stand.forward && directed && f > 0.0
                && if continuing { s.abs() < 1.732 * f && forward > 0.2 }
                   else { s.abs() < 0.577 * f && forward > 0.9 * self.max_speed }
        };
        let sprint_ramp = if sprinting {
            let time = js_min(state.sprint_time.unwrap_or(0.0) + dt, tuning.sprint_accelerate_time);
            state.sprint_time = Some(time);
            state.charge(tuning.energy_drain_sprint * dt, tuning);
            if tuning.sprint_accelerate_time == 0.0 { 1.0 } else { time / tuning.sprint_accelerate_time }
        } else {
            state.sprint_time = None;
            if controls.energy_rate.is_none() && self.grounded && self.air == Air::None {
                let v = self.velocity;
                let rate = if v.x * v.x + v.z * v.z < 1.0 { tuning.energy_restore_idle } else { tuning.energy_restore };
                state.recover(rate * dt, tuning);
            }
            0.0
        };

        if self.air == Air::Jet && controls.end_jet {
            self.set_air(Air::Fall);
        }
        let was_jetting = self.air == Air::Jet;
        if was_jetting {
            let pack = tuning.jetpack.ok_or(ProfileError::InvalidState)?;
            if self.fly_swbf2_jet(input, dt, &trig, tuning.jet, pack, controls, actions.ai) {
                self.finish_motion(&mut trig, yaw)?;
                self.ceiling(dt);
                if self.grounded {
                    self.land_swbf2(state);
                }
                self.collide(&trig, scene);
                return Ok(!locked && self.lock == 0.0);
            }
            self.set_air(Air::Fall);
        }
        let mode = match self.air {
            Air::Thrown => tuning.tumble.mode(0.0),
            Air::Jump | Air::Fall => tuning.jump.mode(0.0),
            _ if sprinting => {
                let mut mode = tuning.sprint.mode(PROVISIONAL_STANDING_BRAKE);
                mode.forward = 1.0 + (mode.forward - 1.0) * sprint_ramp;
                mode
            }
            _ if self.posture == Posture::Crouch => tuning.crouch.mode(PROVISIONAL_CROUCH_BRAKE),
            _ => {
                let mut mode = tuning.stand.mode(PROVISIONAL_STANDING_BRAKE);
                // Standing, the stick drives backward toward MaxStrafeSpeed.
                if input.forward < 0.0 && self.max_speed > 0.0 {
                    mode.forward *= self.max_strafe / self.max_speed;
                }
                mode
            }
        };
        if !self.grounded
            && self.air != Air::None
            && !jumping
            && !controls.owns_horizontal()
            && controls.vertical_impulse.is_none()
            && controls.gravity_scale == 1.0
            && controls.gravity_velocity_target.is_none()
        {
            let speed = self.velocity.length();
            if speed > 0.0 {
                self.velocity.multiply_scalar(v8math::exp(-SWBF2_AIR_DRAG * speed * dt));
            }
        }
        if matches!(self.air, Air::Jump | Air::Fall) && !controls.owns_horizontal() {
            // A body touching a surface stops moving into it.
            if self.grounded {
                let into = self.velocity.dot(self.normal);
                if into < 0.0 {
                    self.velocity.add_scaled(self.normal, -into);
                }
            }
            // The take-off tick moves with the launch velocity alone.
            if !jumping {
                self.steer_swbf2_air(input, dt, &trig, tuning.jump, locked);
            }
            controls.integrate(self, dt);
            self.turn(input, dt, locked, air_turn(mode.turn, actions.ai));
        } else if self.air == Air::None && !controls.owns_horizontal() {
            self.drive_swbf2_ground(input, dt, &trig, locked, mode);
            controls.integrate(self, dt);
            self.turn(input, dt, locked, ground_turn(mode.turn, actions.ai));
        } else {
            let turn = if self.air == Air::None { ground_turn(mode.turn, actions.ai) } else { air_turn(mode.turn, actions.ai) };
            controls.translate(self, input, dt, &trig, locked, Mode { turn, ..mode });
        }
        self.finish_motion(&mut trig, yaw)?;
        if self.air != Air::None {
            self.ceiling(dt);
            if !was_jetting
                && !controls.end_jet
                && !jumping
                && input.jump
                && matches!(self.air, Air::Jump | Air::Fall)
                && (!self.grounded || self.velocity.dot(self.normal) > super::LEAVING)
            {
                if let Some(pack) = tuning.jetpack {
                    self.start_swbf2_jet(input, &trig, tuning, state, pack);
                }
            }
        }
        self.collide(&trig, scene);
        if self.grounded
            && self.air != Air::None
            && self.air != Air::Jet
            && (self.velocity.dot(self.normal) <= super::LEAVING || self.state_time > LAND_LOCK)
        {
            self.land_swbf2(state);
        }
        Ok(!sprinting && !locked && self.lock == 0.0)
    }

    fn start_swbf2_jet(
        &mut self,
        input: &Input,
        trig: &Trig,
        tuning: &Tuning,
        state: &mut State,
        pack: Jetpack,
    ) -> bool {
        let jet = pack.physics;
        if (jet.jump == 0.0 && jet.push == 0.0)
            || self.fuel < jet.min_border
            || !state.can_charge(pack.energy_cost)
        {
            return false;
        }
        state.charge(pack.energy_cost, tuning);
        self.fuel = js_max(self.fuel - jet.initial, 0.0);
        let (f, s) = stick(input);
        let (sin, cos) = (trig.sin1, trig.cos1);
        let forward = js_max(f, -0.1);
        let mut dir = V3::new(sin * forward - 0.1 * s * cos, 1.1, cos * forward + 0.1 * s * sin);
        dir.normalize();
        if let Some(aimed) = input.jet_direction {
            dir = aimed;
        }
        self.velocity.add_scaled(dir, jet.jump);
        if pack.force_jump && !jet.hover {
            let jump_v = (2.0 * GRAVITY * tuning.jump_height).sqrt();
            let new_vy = if self.velocity.y >= 0.5 * jump_v {
                1.5 * jump_v
            } else {
                self.velocity.y + jump_v
            };
            let forward_axis = V3::new(sin, 0.0, cos);
            let right_axis = V3::new(-cos, 0.0, sin);
            let along = f * if f > 0.0 { self.max_speed } else { self.max_strafe };
            let across = s * self.max_strafe;
            self.velocity = V3::new(0.0, new_vy, 0.0);
            self.velocity.add_scaled(forward_axis, along);
            self.velocity.add_scaled(right_axis, across);
            self.grounded = false;
            self.normal = V3::UP;
            self.set_air(Air::Jump);
            state.jump_count = state.jump_count.wrapping_add(1);
        } else {
            self.set_air(Air::Jet);
        }
        true
    }

    fn land_swbf2(&mut self, state: &mut State) {
        let airborne = self.air;
        let airtime = self.state_time;
        self.air = Air::None;
        self.posture = Posture::Stand;
        self.state_time = 0.0;
        state.unsupported_time = None;
        if matches!(airborne, Air::Jump | Air::Fall) && airtime > SWBF2_HARD_LANDING {
            self.lock = HARD_LOCK;
        } else {
            state.land_cooldown = LAND_LOCK;
        }
    }

    fn fly_swbf2_jet(&mut self, input: &Input, dt: f64, trig: &Trig, row: ControlSpeed, pack: Jetpack, controls: &MovementOverride, ai: bool) -> bool {
        let (f, s) = stick(input);
        let jet = pack.physics;
        self.posture = Posture::Stand;
        if jet.cost > 0.0 {
            self.fuel -= jet.cost * dt;
            if self.fuel < 0.0 {
                self.fuel = 0.0;
                return false;
            }
        }
        if input.jump {
            return false;
        }
        if jet.hover {
            self.hover_swbf2(f, s, dt, trig, row, pack.acceleration);
        } else {
            self.velocity.y += jet.push * dt;
            let forward = PROVISIONAL_JET_LOW_SPEED * (f * row.forward);
            let strafe = PROVISIONAL_JET_LOW_SPEED * (s * row.strafe);
            if !controls.owns_horizontal() {
                self.drive_axes(forward, strafe, 0.0, dt, true, trig.sin0, trig.cos0, pack.acceleration);
            }
            controls.integrate(self, dt);
        }
        self.aim_yaw -= self.max_turn * ground_turn(row.turn, ai) * input.turn * dt;
        true
    }

    /// Hover flight (JetType hover), the game's hover posture, one rule for players and bots.
    /// The velocity decays by exp(-2 dt); then along the aim (its height raised by 0.08 and
    /// doubled, then normalized) and along the right axis it moves toward the stick's speeds
    /// (along the aim MaxSpeed forward or MaxStrafeSpeed back, sideways MaxStrafeSpeed, each
    /// times the class's jet ControlSpeed row) by at most JetAcceleration x dt each, both
    /// measured before either change. No gravity: with the stick at rest the body hangs.
    fn hover_swbf2(&mut self, f: f64, s: f64, dt: f64, trig: &Trig, row: ControlSpeed, acceleration: f64) {
        if self.grounded {
            let into = self.velocity.dot(self.normal);
            if into < 0.0 {
                self.velocity.add_scaled(self.normal, -into);
            }
        }
        let decay = v8math::exp(SWBF2_HOVER_DECAY * dt);
        self.velocity = V3::new(self.velocity.x * decay, self.velocity.y * decay, self.velocity.z * decay);
        let mut aim = V3::new(
            trig.sin0 * trig.cos_pitch,
            (trig.sin_pitch + SWBF2_HOVER_AIM_LIFT) * SWBF2_HOVER_AIM_RISE,
            trig.cos0 * trig.cos_pitch,
        );
        aim.normalize();
        let right = V3::new(-trig.cos0, 0.0, trig.sin0);
        let along = f * if f > 0.0 { self.max_speed } else { self.max_strafe } * row.forward;
        let across = s * self.max_strafe * row.strafe;
        let step = acceleration * dt;
        let push_along = clamp(along - self.velocity.dot(aim), -step, step);
        let push_across = clamp(across - self.velocity.dot(right), -step, step);
        self.velocity.add_scaled(aim, push_along);
        self.velocity.add_scaled(right, push_across);
        self.position.add_scaled(self.velocity, dt);
    }

    /// Ground driving. The stick sets a target speed on each axis of the ground
    /// plane (MaxSpeed or MaxStrafeSpeed times the posture's row, already in
    /// `mode`), and each axis moves toward it by at most Acceleration x dt, both
    /// ways: 1.7 times that while a landing lock holds the soldier, a tenth of it on
    /// ground steeper than SWBF2_STEEP_NORMAL. On a surface, gravity along an axis
    /// shifts that window, a target that climbs is shortened by the square of its
    /// climb, and a standing body keeps no speed along the surface normal.
    fn drive_swbf2_ground(&mut self, input: &Input, dt: f64, trig: &Trig, locked: bool, mode: Mode) {
        let (f, s) = if locked { (0.0, 0.0) } else { stick(input) };
        let n = self.normal;
        let facing = V3::new(trig.sin0, 0.0, trig.cos0);
        let mut forward_axis = facing;
        forward_axis.add_scaled(n, -facing.dot(n));
        forward_axis.normalize();
        let mut right_axis = V3::cross(forward_axis, n);
        right_axis.normalize();
        let contact = self.grounded;
        if contact {
            // A body standing on a surface keeps no speed along its normal.
            let along = self.velocity.dot(n);
            self.velocity.add_scaled(n, -along);
        }
        let mut want_f = self.max_speed * f * mode.forward;
        let mut want_r = self.max_strafe * s * mode.strafe;
        let (mut pull_f, mut pull_r) = (0.0, 0.0);
        let mut step = self.accel * dt;
        if locked {
            step *= SWBF2_LOCKED_ACCELERATION;
        }
        if contact {
            let climb = forward_axis.y * want_f + right_axis.y * want_r;
            let wanted = want_f * want_f + want_r * want_r;
            if climb > 0.0 && wanted > 0.0 {
                let keep = 1.0 - climb * climb / wanted;
                want_f *= keep;
                want_r *= keep;
            }
            pull_f = -forward_axis.y * GRAVITY * dt;
            pull_r = -right_axis.y * GRAVITY * dt;
            if js_max(n.y, 0.0) < SWBF2_STEEP_NORMAL {
                step *= SWBF2_STEEP_ACCELERATION;
            }
        }
        let vf = self.velocity.dot(forward_axis);
        let vr = self.velocity.dot(right_axis);
        let change_f = clamp(want_f - vf, pull_f - step, pull_f + step);
        let change_r = clamp(want_r - vr, pull_r - step, pull_r + step);
        self.velocity.add_scaled(forward_axis, change_f);
        self.velocity.add_scaled(right_axis, change_r);
    }

    /// Airborne stick steering: the stick accelerates the body by the class
    /// acceleration times the jump `ControlSpeed` row along the facing and side
    /// axes, with no speed target. Only air drag limits the result.
    fn steer_swbf2_air(&mut self, input: &Input, dt: f64, trig: &Trig, row: ControlSpeed, locked: bool) {
        let (f, s) = if locked { (0.0, 0.0) } else { stick(input) };
        let forward = V3::new(trig.sin0, 0.0, trig.cos0);
        let right = V3::new(-trig.cos0, 0.0, trig.sin0);
        self.velocity.add_scaled(forward, row.forward * f * self.accel * dt);
        self.velocity.add_scaled(right, row.strafe * s * self.accel * dt);
    }

    /// The soldier's jump. A jump that carries a bot's jet vector (only the AI controller
    /// stores one; a squared length over 0.1) leaves along the vector's flat direction
    /// (its height replaced by 0.01, then normalized) at MaxSpeed x JumpForwardSpeedFactor,
    /// adding that direction's small height share to the take-off speed; the speed it had
    /// is dropped. Every other jump keeps its momentum, scaled per axis.
    fn launch_swbf2_jump(&mut self, tuning: &Tuning, trig: &Trig, input: &Input) {
        let take_off = (2.0 * GRAVITY * tuning.jump_height).sqrt();
        if let Some(v) = input.jet_direction.filter(|v| v.dot(*v) > SWBF2_DIRECTED_JUMP_MIN) {
            let mut along = V3::new(v.x, SWBF2_DIRECTED_JUMP_LIFT, v.z);
            along.normalize();
            let speed = self.max_speed * tuning.jump_forward_speed_factor;
            self.velocity = V3::new(along.x * speed, take_off + along.y * speed, along.z * speed);
            self.grounded = false;
            self.normal = V3::UP;
            self.set_air(Air::Jump);
            return;
        }
        let forward = V3::new(trig.sin0, 0.0, trig.cos0);
        let right = V3::new(-trig.cos0, 0.0, trig.sin0);
        let component = self.velocity.dot(forward);
        let along = component * if component > 0.0 { tuning.jump_forward_speed_factor } else { tuning.jump_strafe_speed_factor };
        let across = self.velocity.dot(right) * tuning.jump_strafe_speed_factor;
        self.velocity = V3::new(0.0, self.velocity.y + take_off, 0.0);
        self.velocity.add_scaled(forward, along);
        self.velocity.add_scaled(right, across);
        self.grounded = false;
        self.normal = V3::UP;
        self.set_air(Air::Jump);
    }

    fn boost_roll(&mut self, trig: &Trig, target: f64) {
        let forward = V3::new(trig.sin0, 0.0, trig.cos0);
        let side = V3::new(-trig.cos0, 0.0, trig.sin0);
        let along = self.velocity.dot(forward);
        let across = self.velocity.dot(side);
        let speed_sq = along * along + across * across;
        if speed_sq > 0.01 && speed_sq < target * target {
            let gain = target / speed_sq.sqrt() - 1.0;
            self.velocity.add_scaled(forward, js_max(along, 0.0) * gain);
            self.velocity.add_scaled(side, across * gain);
        }
    }

    fn roll_tick(&mut self, input: &Input, dt: f64, mut trig: Trig, scene: &mut Scene,
        tuning: &Tuning, mut roll: Roll, controls: &MovementOverride, ai: bool) -> Result<Option<Roll>, ProfileError> {
        let yaw = self.aim_yaw;
        let mode = tuning.roll.mode(PROVISIONAL_ROLL_BRAKE);
        // The roll row applies to everyone, as in the air.
        controls.translate(self, input, dt, &trig, false, Mode { turn: air_turn(mode.turn, ai), ..mode });
        self.finish_motion(&mut trig, yaw)?;
        self.collide_as(PROVISIONAL_ROLL_POSTURE, &trig, scene);
        roll.elapsed += dt;
        if !self.grounded {
            self.set_air(PROVISIONAL_ROLL_LEDGE_AIR);
            Ok(None)
        } else if roll.elapsed > tuning.roll_seconds() {
            self.finish_roll();
            Ok(None)
        } else {
            Ok(Some(roll))
        }
    }

    fn finish_roll(&mut self) {
        self.posture = Posture::Stand;
        self.state_time = 0.0;
    }

    fn apply_tumble_drag(&mut self, dt: f64) {
        let speed_sq = self.velocity.length_sq();
        if speed_sq > 0.0 {
            let speed = v8math::sqrt(speed_sq);
            let factor = v8math::exp(-THROWN_AIR_DRAG * speed * dt);
            self.velocity.multiply_scalar(factor);
        }
    }

    fn tumble_tick(
        &mut self,
        dt: f64,
        mut trig: Trig,
        scene: &mut Scene,
        tuning: &Tuning,
        mut t: Tumble,
    ) -> Result<Option<Tumble>, ProfileError> {
        let yaw = self.aim_yaw;
        match t.phase {
            TumblePhase::Thrown => {
                self.posture = Posture::Stand;
                self.air = Air::Thrown;
                self.apply_tumble_drag(dt);
                self.velocity.y -= GRAVITY * dt;
                let v = self.velocity;
                self.position.add_scaled(v, dt);
                self.ceiling(dt);
                self.finish_motion(&mut trig, yaw)?;
                self.collide_as(Posture::Stand, &trig, scene);
                t.elapsed += dt;
                t.air_time += dt;
                t.angle = wrap_tau(t.angle + t.spin_rate * dt);
                if !t.dead
                    && tuning.recover_from_tumble
                    && t.elapsed > THROWN_RECOVER_DELAY
                    && !self.grounded
                {
                    t.phase = TumblePhase::Recover;
                    t.elapsed = 0.0;
                    t.angle = 0.0;
                    self.air = Air::Fall;
                    self.state_time = 0.0;
                    return Ok(Some(t));
                }
                if t.elapsed > THROWN_MIN_AIR_TIME && self.grounded {
                    t.front = t.angle <= std::f64::consts::PI;
                    t.bounce = t.air_time <= THROWN_BOUNCE_WINDOW;
                    let speed_xz_sq = self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z;
                    if speed_xz_sq > TUMBLE_HEADING_SPEED_SQ_EPSILON {
                        t.heading_offset = ground_heading_offset(
                            self.velocity.x,
                            self.velocity.z,
                            self.aim_yaw,
                            t.front,
                        );
                    }
                    let enter_tumble = if speed_xz_sq > THROWN_TUMBLE_SPEED_SQ {
                        true
                    } else if speed_xz_sq > THROWN_MAYBE_TUMBLE_SPEED_SQ {
                        deterministic_unit((t.angle as f32).to_bits() ^ (self.position.x as f32).to_bits()) < 0.7
                    } else {
                        false
                    };
                    t.phase = if enter_tumble {
                        TumblePhase::Tumble
                    } else {
                        TumblePhase::Landed
                    };
                    t.elapsed = 0.0;
                    t.angle = 0.0;
                    t.cycle_phase = 0.0;
                    self.air = Air::None;
                    self.posture = Posture::Crouch;
                    self.state_time = 0.0;
                }
                Ok(Some(t))
            }
            TumblePhase::Tumble => {
                self.posture = Posture::Crouch;
                self.air = Air::None;
                self.apply_tumble_drag(dt);
                let damp = js_max(0.0, 1.0 - TUMBLE_FRICTION * dt);
                self.velocity.x *= damp;
                self.velocity.z *= damp;
                if !self.grounded {
                    self.velocity.y -= GRAVITY * dt;
                }
                let v = self.velocity;
                self.position.add_scaled(v, dt);
                self.finish_motion(&mut trig, yaw)?;
                self.collide_as(Posture::Crouch, &trig, scene);
                let speed_xz_sq = self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z;
                let speed_xz = if speed_xz_sq > 0.0 { v8math::sqrt(speed_xz_sq) } else { 0.0 };
                let rate = speed_xz / TUMBLE_CLIP_SPEED;
                t.elapsed += dt;
                t.cycle_phase = wrap_unit(t.cycle_phase + dt * rate / TUMBLE_CLIP_DURATION);
                if speed_xz_sq > TUMBLE_HEADING_SPEED_SQ_EPSILON {
                    t.heading_offset = ground_heading_offset(
                        self.velocity.x,
                        self.velocity.z,
                        self.aim_yaw,
                        t.front,
                    );
                }
                if t.elapsed > TUMBLE_MIN_TIME
                    && (speed_xz_sq < TUMBLE_END_SPEED_SQ || t.elapsed >= TUMBLE_MAX_TIME)
                {
                    if t.cycle_phase > 0.40 && t.cycle_phase < 0.90 {
                        t.front = !t.front;
                        t.heading_offset = wrap_pi(t.heading_offset + std::f64::consts::PI);
                    }
                    t.bounce = false;
                    t.phase = TumblePhase::Landed;
                    t.elapsed = 0.0;
                    self.posture = Posture::Crouch;
                    self.state_time = 0.0;
                }
                Ok(Some(t))
            }
            TumblePhase::Landed => {
                self.posture = Posture::Crouch;
                self.air = Air::None;
                self.apply_tumble_drag(dt);
                let damp = js_max(0.0, 1.0 - LANDED_FRICTION * dt);
                self.velocity.x *= damp;
                self.velocity.z *= damp;
                if !self.grounded {
                    self.velocity.y -= GRAVITY * dt;
                }
                let v = self.velocity;
                self.position.add_scaled(v, dt);
                self.finish_motion(&mut trig, yaw)?;
                self.collide_as(Posture::Crouch, &trig, scene);
                t.elapsed += dt;
                let speed_xz_sq = self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z;
                if self.grounded
                    && (speed_xz_sq < LANDED_STOP_SPEED_SQ || t.elapsed > LANDED_MAX_TIME)
                {
                    self.velocity.x = 0.0;
                    self.velocity.z = 0.0;
                    if !t.dead && self.headroom(scene) {
                        t.phase = TumblePhase::GetUp;
                        t.elapsed = 0.0;
                        let duration = if t.front {
                            tuning.getup_front_seconds
                        } else {
                            tuning.getup_back_seconds
                        };
                        self.lock = duration;
                        self.posture = Posture::Stand;
                        self.state_time = 0.0;
                    } else if !t.dead {
                        // No room to stand where the slide ended (a low roof, a stair's
                        // underside). Waiting for room would hold him down for good, so the
                        // tumble ends in the crouch the mover already keeps without headroom:
                        // he is in control again and stands when there is room.
                        self.posture = Posture::Crouch;
                        self.air = Air::None;
                        self.lock = 0.0;
                        self.state_time = 0.0;
                        return Ok(None);
                    }
                }
                Ok(Some(t))
            }
            TumblePhase::GetUp => {
                if t.dead {
                    t.phase = TumblePhase::Landed;
                    t.elapsed = 0.0;
                    self.lock = 0.0;
                    self.posture = Posture::Crouch;
                    return Ok(Some(t));
                }
                self.posture = Posture::Stand;
                self.air = Air::None;
                if self.grounded {
                    self.velocity.x = 0.0;
                    self.velocity.z = 0.0;
                } else {
                    self.velocity.y -= GRAVITY * dt;
                }
                let v = self.velocity;
                self.position.add_scaled(v, dt);
                self.finish_motion(&mut trig, yaw)?;
                self.collide_as(Posture::Stand, &trig, scene);
                t.elapsed += dt;
                t.heading_offset *= v8math::pow(0.90, 30.0 * dt);
                let duration = if t.front {
                    tuning.getup_front_seconds
                } else {
                    tuning.getup_back_seconds
                };
                if t.elapsed >= duration {
                    self.lock = 0.0;
                    self.posture = Posture::Stand;
                    self.air = Air::None;
                    self.state_time = 0.0;
                    Ok(None)
                } else {
                    self.lock = js_max(duration - t.elapsed, 0.0);
                    Ok(Some(t))
                }
            }
            TumblePhase::Recover => {
                self.posture = Posture::Stand;
                self.air = Air::Fall;
                self.apply_tumble_drag(dt);
                self.velocity.y -= GRAVITY * dt;
                let v = self.velocity;
                self.position.add_scaled(v, dt);
                self.ceiling(dt);
                self.finish_motion(&mut trig, yaw)?;
                self.collide_as(Posture::Stand, &trig, scene);
                t.elapsed += dt;
                if self.grounded {
                    let vn = self.velocity.dot(self.normal);
                    let tan = V3::new(
                        self.velocity.x - self.normal.x * vn,
                        self.velocity.y - self.normal.y * vn,
                        self.velocity.z - self.normal.z * vn,
                    );
                    if tan.length_sq() > THROWN_RECOVER_FAIL_SPEED_SQ {
                        t.phase = TumblePhase::Tumble;
                        t.elapsed = 0.0;
                        t.angle = 0.0;
                        t.front = true;
                        t.heading_offset = ground_heading_offset(
                            self.velocity.x,
                            self.velocity.z,
                            self.aim_yaw,
                            true,
                        );
                        self.air = Air::None;
                        self.posture = Posture::Crouch;
                        self.state_time = 0.0;
                        Ok(Some(t))
                    } else {
                        self.land();
                        Ok(None)
                    }
                } else if t.elapsed >= THROWN_RECOVER_DURATION {
                    self.air = Air::Fall;
                    self.state_time = 2.0;
                    Ok(None)
                } else {
                    Ok(Some(t))
                }
            }
        }
    }
}
