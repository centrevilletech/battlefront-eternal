//! The map's water as the world's simulation sees it
//! (src/runtime/water-state.js WaterState): its surface height bobbing on
//! the tick clock, which cells of its grid are water, where a segment goes
//! down through it, and what it does to a soldier each tick (his wakes and
//! the splash of a fall into it, his gun under the surface, drowning).

use super::shapes::arr;
use crate::gen::WaterContact;
use three::Vector3;
use std::collections::HashSet;

const BOB: f64 = 0.06; // m
const BOB_RATE: f64 = 0.3; // Hz
const WAKE_LIFE: f64 = 2.0;
const WAKE_RADIUS: f64 = 0.25;
const SPLASH_SPEED: f64 = 5.0; // m/s down
const SPLASH_GAP: f64 = 1.0; // s
const DROWN_AFTER: f64 = 3.0; // s
const DROWN_DAMAGE: f64 = 30.0; // per second

/// What a soldier's tick in the water asks for besides damage: a wake's
/// ripple (the page's) and the splash of a fall into it (the game's
/// watersplash_md effect), where.
pub trait WaterEffects {
    fn ripple(&mut self, at: &Vector3, life: f64, radius: f64, velocity: &Vector3);
    fn splash(&mut self, at: &Vector3);
}

/// WaterState.
#[derive(Clone, Debug)]
pub struct WaterState {
    pub height: f64,
    pub cell: f64,
    pub offset: [f64; 2],
    pub grid: f64,
    /// `cells`: the water cells' numbers (j * grid + i).
    pub cells: HashSet<u64>,
    pub bobbing: bool,
    pub simulation_time: f64,
    pub physics_wave: f64,
}

impl WaterState {
    /// step(dt): the clock and the bob.
    pub fn step(&mut self, dt: f64) {
        self.simulation_time += dt;
        self.physics_wave = if self.bobbing { BOB * v8math::sin(2.0 * std::f64::consts::PI * BOB_RATE * self.simulation_time) } else { 0.0 };
    }

    /// `level`: the surface's height now, bob included.
    pub fn level(&self) -> f64 {
        self.height + self.physics_wave
    }

    /// over(x, z): whether (x, z) is over a water cell (points off the grid
    /// take the nearest edge cell).
    pub fn over(&self, x: f64, z: f64) -> bool {
        let n = self.grid;
        let i = v8math::min(v8math::max(((x + self.offset[0]) / self.cell).floor(), 0.0), n - 1.0);
        let j = v8math::min(v8math::max(((z + self.offset[1]) / self.cell).floor(), 0.0), n - 1.0);
        let key = j * n + i;
        // A Set of numbers: SameValueZero (NaN never a cell's number here).
        !key.is_nan() && self.cells.contains(&(key + 0.0).to_bits())
    }

    /// crossing(from, to): where a segment goes down through the surface
    /// over water, or None. Things only meet water coming down through it.
    pub fn crossing(&self, from: &Vector3, to: &Vector3) -> Option<Vector3> {
        let level = self.level();
        if !(from.y >= level && to.y < level) {
            return None;
        }
        let t = (from.y - level) / (from.y - to.y);
        let mut out = Vector3::default();
        out.lerp_vectors(from, to, t);
        out.y = level;
        if self.over(out.x, out.z) {
            Some(out)
        } else {
            None
        }
    }

    /// soldierStep(dt, contact, feet, head, gun, velocity, splash): one tick
    /// of a soldier and the water; returns the damage to take.
    #[allow(clippy::too_many_arguments)]
    pub fn soldier_step(&self, dt: f64, contact: &mut WaterContact, feet: &Vector3, head: &Vector3, gun: &Vector3, velocity: &Vector3, effects: &mut dyn WaterEffects) -> f64 {
        self.soldier_step_over(dt, contact, feet, head, gun, velocity, &mut |x, z| self.over(x, z), effects)
    }

    /// soldier_step with `over` answered by `over` (the replays serve it).
    #[allow(clippy::too_many_arguments)]
    pub fn soldier_step_over(
        &self,
        dt: f64,
        contact: &mut WaterContact,
        feet: &Vector3,
        head: &Vector3,
        gun: &Vector3,
        velocity: &Vector3,
        over: &mut dyn FnMut(f64, f64) -> bool,
        effects: &mut dyn WaterEffects,
    ) -> f64 {
        let level = self.level();
        let wet = level > feet.y && over(feet.x, feet.z);
        contact.gun_under = Some(gun.y < level && over(gun.x, gun.z));
        let mut damage = 0.0;
        let or_zero = |x: Option<f64>| x.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0);
        if wet {
            let wake = or_zero(contact.wake_clock) + dt;
            contact.wake_clock = Some(wake);
            let interval = 1.0 / (0.3 * velocity.length_sq() + 4.0);
            if wake > interval && velocity.length_sq() > 0.0 {
                let mut at = *feet;
                at.add_scaled_vector(velocity, 0.03);
                effects.ripple(&at, WAKE_LIFE, WAKE_RADIUS, velocity);
                contact.wake_clock = Some(v8math::min(wake - interval, interval));
            }
            let splash = or_zero(contact.splash_clock) + dt;
            contact.splash_clock = Some(splash);
            if velocity.y < -SPLASH_SPEED && splash > SPLASH_GAP {
                contact.splash_clock = Some(0.0);
                effects.splash(&Vector3 { x: feet.x, y: level, z: feet.z });
            }
        }
        if head.y < level && over(head.x, head.z) {
            let mut under = or_zero(contact.under) + dt;
            if under >= DROWN_AFTER {
                under -= 1.0;
                damage = DROWN_DAMAGE;
            }
            contact.under = Some(under);
        } else {
            contact.under = Some(0.0);
        }
        damage
    }
}

#[allow(unused_imports)]
use arr as _arr;
