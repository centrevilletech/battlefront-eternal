//! src/flight/pilotless.js: a craft in flight that nobody flies. Empty,
//! after its pilot bailed out or was taken out of it, it comes down:
//! nothing holds it up, its speed carries on and gravity bends its path,
//! the nose following the path, until it strikes the ground, a building,
//! the water or a death region, which destroys it.

use super::model::Z;
use super::state::Trick;
use super::FlightWorld;
use crate::rig::math;
use crate::vehicles::Hull;
use three::{clamp, Quaternion};
use v8math::max;

/// m/s², as shells and grenades fall (ordnance.js).
const GRAVITY: f64 = 9.8;
/// 1/s: how fast the nose swings onto the path.
const NOSE: f64 = 3.0;

impl Hull {
    /// stepPilotless(v, dt).
    pub fn step_pilotless<W: FlightWorld>(&mut self, w: &mut W, dt: f64) {
        self.flight.previous_q = self.quaternion;
        self.flight.clock = super::model::or_zero(self.flight.clock) + dt;
        self.flight.state_time = super::model::or_zero(self.flight.state_time) + dt;
        self.flight.stick_x = 0.0;
        self.flight.stick_y = 0.0;
        self.flight.boosting = false;
        self.flight.trick = Trick::None;
        self.velocity.y -= GRAVITY * dt;
        let mut next = self.position;
        next.add_scaled_vector(&self.velocity, dt);
        let (position, quaternion) = (self.position, self.quaternion);
        let contact = self.sweep(w, &position, &next, &quaternion, f64::NEG_INFINITY, None, false);
        match contact {
            Some(c) => {
                let velocity = self.velocity;
                self.position.add_scaled_vector(&velocity, max(0.0, c.distance - 0.2) / max(velocity.length(), 1.0));
            }
            None => self.position = next,
        }
        let mut path = self.velocity;
        if path.length_sq() > 1e-6 {
            let mut forward = Z;
            forward.apply_quaternion(&self.quaternion);
            let mut swing = Quaternion::default();
            swing.set_from_unit_vectors(&forward, path.normalize());
            let mut none = Quaternion::default();
            none.identity().slerp(&swing, 1.0 - math::exp(-NOSE * dt));
            self.quaternion.premultiply(&none).normalize();
        }
        let speed = self.velocity.length();
        self.flight.throttle = speed;
        self.flight.airspeed = speed;
        self.flight.grounded = false;
        self.sleeping = Some(false);
        let mut forward = Z;
        forward.apply_quaternion(&self.quaternion);
        self.yaw = math::atan2(forward.x, forward.z);
        self.tilt = math::asin(clamp(forward.y, -1.0, 1.0));
        if let Some(seat) = self.seats.first_mut() {
            seat.aim_yaw = self.yaw;
            seat.pitch = self.tilt;
        }
        self.sync_body(w);
        let p = self.position;
        let wet = w.water_over(p.x, p.z) && p.y < w.water_level();
        if contact.is_some() || wet || w.system_death_region(&p) || p.y < -300.0 {
            self.damage(w, f64::INFINITY, crate::gen::OrdnanceOwner::Null);
        }
    }
}
