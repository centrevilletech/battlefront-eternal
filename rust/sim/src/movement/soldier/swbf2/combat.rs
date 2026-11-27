//! Effective movement controls for one equipped combat action.

use super::{Actions, Air, Input, Mode, Mover, ProfileError, RollDirection, Trig, GRAVITY, V3};

const AIR_DRAG_COEFF: f64 = 0.012;

/// InputLock bits shared with combo actions. Fire and reload belong to the weapon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputLockMask(pub u16);

impl InputLockMask {
    pub const JUMP: Self = Self(1 << 2);
    pub const CROUCH: Self = Self(1 << 3);
    pub const SPRINT: Self = Self(1 << 4);
    pub const THRUST: Self = Self(1 << 6);
    pub const STRAFE: Self = Self(1 << 7);
    pub const TURN: Self = Self(1 << 8);
    pub const ALL: Self = Self(0x01ff);

    fn blocks(self, control: Self) -> bool { self.0 & control.0 != 0 }

    pub(super) fn filter(self, mut input: Input, mut actions: Actions) -> (Input, Actions) {
        if self.blocks(Self::JUMP) { input.jump = false; }
        if self.blocks(Self::CROUCH) { input.crouch = false; actions.roll = false; }
        if self.blocks(Self::SPRINT) { actions.sprint = false; }
        if self.blocks(Self::THRUST) { input.forward = 0.0; }
        if self.blocks(Self::STRAFE) { input.right = 0.0; }
        if self.blocks(Self::TURN) { input.turn = 0.0; input.pitch = 0.0; }
        (input, actions)
    }
}

/// An action's movement for this tick. Collision still owns the final position.
#[derive(Clone, Copy, Debug)]
pub struct MovementOverride {
    /// Absolute world X and Z speeds on ground. An explicit zero stops planar movement.
    pub velocity_xz: Option<[f64; 2]>,
    /// World X and Z acceleration while airborne, applied after quadratic air drag.
    pub air_accel_xz: Option<[f64; 2]>,
    /// Entry-tick vertical speed, before this tick's gravity.
    pub vertical_impulse: Option<f64>,
    /// Absolute facing, in radians, before evaluating movement.
    pub aim_yaw: Option<f64>,
    /// Preserve incoming horizontal motion without stick drive, brake or slope pull.
    pub lock_locomotion: bool,
    /// Signed multiplier of airborne gravity, as supplied by the action.
    pub gravity_scale: f64,
    /// A negative terminal speed limits descent, after gravity.
    pub gravity_velocity_target: Option<f64>,
    pub input_lock: InputLockMask,
    /// Signed stamina units/second in place of ordinary passive recovery.
    pub energy_rate: Option<f64>,
    /// Request an authored ground jump independently of the raw button.
    pub force_jump: bool,
    /// Cancel an active jet state into a normal fall and prevent re-entering jet this tick.
    pub end_jet: bool,
    /// Request the selected roll action independently of the raw stick.
    pub force_roll: Option<RollDirection>,
}

impl Default for MovementOverride {
    fn default() -> Self {
        Self {
            velocity_xz: None,
            air_accel_xz: None,
            vertical_impulse: None,
            aim_yaw: None,
            lock_locomotion: false,
            gravity_scale: 1.0,
            gravity_velocity_target: None,
            input_lock: InputLockMask::default(),
            energy_rate: None,
            force_jump: false,
            end_jet: false,
            force_roll: None,
        }
    }
}

impl MovementOverride {
    pub(super) fn validate(&self, dt: f64) -> Result<(), ProfileError> {
        if !(GRAVITY * self.gravity_scale).is_finite()
            || self.input_lock.0 & !InputLockMask::ALL.0 != 0
            || self.velocity_xz.is_some_and(|xz| !xz.into_iter().all(f64::is_finite))
            || self.air_accel_xz.is_some_and(|xz| !xz.into_iter().all(f64::is_finite))
            || [self.vertical_impulse, self.aim_yaw, self.gravity_velocity_target, self.energy_rate]
                .into_iter().flatten().any(|v| !v.is_finite()) {
            return Err(ProfileError::InvalidState);
        }
        if self.energy_rate.is_some_and(|rate| !(rate * dt).is_finite())
            || self.air_accel_xz.is_some_and(|xz| !(xz[0] * dt).is_finite() || !(xz[1] * dt).is_finite()) {
            return Err(ProfileError::InvalidDelta);
        }
        Ok(())
    }

    pub(super) fn owns_horizontal(&self) -> bool {
        self.lock_locomotion || self.velocity_xz.is_some() || self.air_accel_xz.is_some()
    }

    pub(super) fn translate(&self, m: &mut Mover, input: &Input, dt: f64, trig: &Trig, locked: bool, mode: Mode) {
        if !self.owns_horizontal() {
            let (forward, strafe, brake) = m.drive_controls(input, locked, mode);
            m.drive_axes(forward, strafe, brake, dt, false, trig.sin0, trig.cos0, m.accel);
        }
        self.integrate(m, dt);
        m.turn(input, dt, locked, mode.turn);
    }

    pub(super) fn integrate(&self, m: &mut Mover, dt: f64) {
        if let Some([x, z]) = self.velocity_xz {
            m.velocity.x = x;
            m.velocity.z = z;
        } else if let Some([ax, az]) = self.air_accel_xz {
            let speed = v8math::sqrt(
                m.velocity.x * m.velocity.x
                    + m.velocity.y * m.velocity.y
                    + m.velocity.z * m.velocity.z,
            );
            let drag = v8math::exp(-AIR_DRAG_COEFF * speed * dt);
            m.velocity.x = m.velocity.x * drag + ax * dt;
            m.velocity.y *= drag;
            m.velocity.z = m.velocity.z * drag + az * dt;
        }
        if let Some(y) = self.vertical_impulse {
            m.velocity.y = y;
            if m.grounded && y > 0.0 {
                m.grounded = false;
                m.normal = V3::UP;
                m.set_air(Air::Jump);
            }
        }
        if !m.grounded {
            m.velocity.y -= m.gravity() * self.gravity_scale * dt;
            if let Some(target) = self.gravity_velocity_target {
                if target < 0.0 && m.velocity.y < target { m.velocity.y = target; }
            }
        }
        m.position.add_scaled(m.velocity, dt);
    }
}
