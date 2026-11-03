//! What other parts of the battle ask of a unit's body between its ticks
//! (src/soldier.js, src/droideka.js): a player's mouse look, the aim's
//! pitch range, a droideka's shield (toggled by its secondary trigger,
//! taking damage first), its guns in turn and where it sees from, and a
//! body's spawn.

use super::shapes::{arr, v3};
use super::world::UnitsWorld;
use crate::gen::{Air, Droideka, LastGun, Posture, Soldier};
use crate::rig::droideka::DroidekaRig;
use three::{clamp, Vector3};

/// soldier.js ENGINE: the mouse's counts per radian per unit of
/// MaxTurnSpeed, the rate a mouse may ask for (in stick units), and the
/// prone pitch limit.
const LOOK_SCALE: f64 = 2048.0;
const LOOK_CLAMP: f64 = 16.0;
const PRONE_PITCH: f64 = 0.4;
const DOWN: Vector3 = Vector3 { x: 0.0, y: -1.0, z: 0.0 };

impl Soldier {
    /// `mode.turn` (soldier.js MODES): the posture's turn scale.
    pub fn mode_turn(&self) -> f64 {
        match self.air {
            Some(Air::Jet) => 1.0,
            Some(_) => 0.5,
            None if self.dive != 0.0 && !self.dive.is_nan() => 0.6,
            None => match self.posture {
                Posture::Stand | Posture::Crouch => 1.0,
                Posture::Prone => 0.5,
            },
        }
    }

    /// pitchRange(): prone on the ground, ±0.4; else the class's limits.
    pub fn pitch_range(&self) -> [f64; 2] {
        if self.posture == Posture::Prone && self.air.is_none() {
            return [-PRONE_PITCH, PRONE_PITCH];
        }
        self.pitch_limits
    }

    /// look(dx, dy, sensitivity, invert) (soldier.js:244): mouse look in raw
    /// counts, the rate a tick may take capped.
    pub fn look(&mut self, dx: f64, dy: f64, sensitivity: f64, invert: bool) {
        if self.lock > 0.0 {
            return;
        }
        let cap = LOOK_CLAMP * LOOK_SCALE * self.tick_length;
        let x = clamp(self.look_x + dx * sensitivity, -cap, cap);
        let y = clamp(self.look_y + (if invert { dy } else { -dy }) * sensitivity, -cap, cap);
        let rate = self.max_turn / LOOK_SCALE;
        self.aim_yaw -= (x - self.look_x) * rate * self.mode_turn();
        self.aim_pitch += (y - self.look_y) * rate;
        self.look_x = x;
        self.look_y = y;
        let [low, high] = self.pitch_range();
        self.aim_pitch = clamp(self.aim_pitch, low, high);
    }

    /// lookRaw(dx, dy, sensitivity, invert) (soldier.js:261): pointer counts
    /// as displacement, uncapped.
    pub fn look_raw(&mut self, dx: f64, dy: f64, sensitivity: f64, invert: bool) {
        if self.lock > 0.0 {
            return;
        }
        let rate = self.max_turn / LOOK_SCALE;
        self.aim_yaw -= dx * sensitivity * rate * self.mode_turn();
        let [low, high] = self.pitch_range();
        self.aim_pitch = clamp(self.aim_pitch + (if invert { dy } else { -dy }) * sensitivity * rate, low, high);
    }

    /// spawn(x, y, z, yaw) (soldier.js:201): fresh, on the ground under the
    /// point (a ray down from 1.5 m above it, 50 m long).
    pub fn spawn(&mut self, x: f64, y: f64, z: f64, yaw: f64, world: &mut dyn UnitsWorld) {
        self.pass_through = None;
        self.position = [x, y, z];
        let origin = Vector3 { x, y: y + 1.5, z };
        let hit = world.cast(&origin, &DOWN, 50.0);
        if let Some(hit) = &hit {
            self.position[1] = hit.point.y;
        }
        self.velocity = [0.0; 3];
        self.normal = [0.0, 1.0, 0.0];
        self.aim_yaw = yaw;
        self.aim_pitch = -0.07;
        self.posture = Posture::Stand;
        self.air = None;
        self.dive = 0.0;
        self.state_time = 0.0;
        self.lock = 0.0;
        self.fuel = if self.jet.recharge > 0.0 { 0.5 } else { 0.0 };
        self.grounded = hit.is_some();
        self.ground_adjustment = 0.0;
        self.previous = self.position;
        self.settled = self.position;
    }
}

impl Droideka {
    /// look(dx, dy, sensitivity, invert) (droideka.js:344): its tick turns
    /// and pitches by what the mouse gave.
    pub fn look(&mut self, dx: f64, dy: f64, sensitivity: f64, invert: bool) {
        self.look_x += dx * sensitivity;
        self.look_y += (if invert { dy } else { -dy }) * sensitivity;
    }

    pub fn pitch_range(&self) -> [f64; 2] {
        self.pitch_limits
    }

    /// eye(feet, out): EyePointOffset over its origin (1 m rolled).
    pub fn eye(&self, feet: &Vector3) -> Vector3 {
        Vector3 { x: feet.x, y: feet.y + if self.rolled() { 1.0 } else { self.eye_height }, z: feet.z }
    }

    /// toggleShield(): each press of the secondary trigger raises or lowers
    /// it (not while rolled or dead, and only with some left to raise).
    pub fn toggle_shield(&mut self) {
        let rolled = self.rolled();
        let dead = self.dead;
        let Some(shield) = &mut self.shield else { return };
        if rolled || dead {
            return;
        }
        shield.on = !shield.on && shield.value > 0.0;
    }

    /// absorb(amount, onShield) (droideka.js:226): the shield takes the
    /// damage first, by the attack's ShieldScale; returns what is left for
    /// health. `hit(body, taken)` is onShieldHit, called where the
    /// JavaScript calls it (after the shield took its part, before it drops).
    pub fn absorb(&mut self, amount: f64, on_shield: f64, mut hit: impl FnMut(&Droideka, f64)) -> f64 {
        match &self.shield {
            Some(shield) if shield.on && on_shield > 0.0 => {}
            _ => return amount,
        }
        let shield = self.shield.as_mut().unwrap();
        let taken = v8math::min(shield.value, on_shield);
        shield.value -= taken;
        if taken > 0.0 {
            hit(self, taken);
        }
        let shield = self.shield.as_mut().unwrap();
        if shield.value <= 0.0 {
            shield.on = false;
        }
        amount * (1.0 - taken / on_shield)
    }

    /// nextGun(out) (droideka.js:237): the next gun's muzzle, in turn (its
    /// barrels alternating), off the hidden gun rig.
    pub fn next_gun(&mut self, rig: &mut DroidekaRig) -> Option<Vector3> {
        let guns = rig.barrels.len() as f64;
        if guns == 0.0 {
            return None;
        }
        let index = self.gun % guns;
        let barrels = rig.barrels.get(index as usize).map_or(0, |b| b.len()) as f64;
        let barrel = (self.gun / guns).floor() % if barrels > 0.0 { barrels } else { 1.0 };
        self.last_gun = Some(LastGun { index, barrel });
        self.gun = (self.gun + 1.0) % (guns * 2.0);
        self.gun_at(rig, self.last_gun.clone())
    }

    /// gunAt(out, selected) (droideka.js:246): a barrel's place in the world.
    pub fn gun_at(&self, rig: &mut DroidekaRig, selected: Option<LastGun>) -> Option<Vector3> {
        let s = selected?;
        if s.index < 0.0 || s.barrel < 0.0 || s.index.fract() != 0.0 || s.barrel.fract() != 0.0 {
            return None;
        }
        rig.gun_at(s.index as usize, s.barrel as usize)
    }

    /// spawn(x, y, z, yaw) (droideka.js:319) up to its mode: on the ground
    /// under the point, still, its shield full and down. The caller enters
    /// mode 0 and resets the rig's animator.
    pub fn spawn_place(&mut self, x: f64, y: f64, z: f64, yaw: f64, world: &mut dyn UnitsWorld) {
        self.position = [x, y, z];
        let from = Vector3 { x, y: y + 1.5, z };
        if let Some(hit) = world.cast(&from, &DOWN, 50.0) {
            self.position[1] = hit.point.y;
        }
        self.velocity = [0.0; 3];
        self.aim_yaw = yaw;
        self.aim_pitch = 0.0;
        self.legs = 0.0;
        self.spin = 0.0;
        self.dead = false;
        // Each life's shield starts full and down.
        if let Some(shield) = &mut self.shield {
            shield.on = false;
            shield.value = shield.max;
        }
    }
}

#[allow(unused_imports)]
use {arr as _arr, v3 as _v3};
