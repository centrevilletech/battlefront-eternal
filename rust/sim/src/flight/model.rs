//! src/flight/model.js: a craft's flight, one 1/60 s step (`stepFlight`):
//! its clocks and the landing site under it, the takeoff key, then by its
//! flight state the takeoff's lift, the landing's descent, or flight (the
//! stick's rates, boost, the tricks, the roll, boundary recovery, the move
//! and what it strikes), then power, the stick's return and the heading
//! projections; and the queries it is made of (the 27-cast `sweep`, the
//! ground under the craft, the top of what it rests in, the landing site).
//!
//! The JavaScript's module-level scratch vectors are written before each
//! read, so here they are locals.

use super::config::{throttle_speed, Config, Tuning};
use super::state::{Message, Trick};
use super::{Environment, FlightState, FlightWorld};
use crate::rig::math;
use crate::vehicles::{Hit, Hull};
use three::{clamp, Matrix4, Quaternion, Vector3};
use v8math::{abs, max, min, pow, sign};

pub(crate) const X: Vector3 = Vector3::new(1.0, 0.0, 0.0);
pub(crate) const Y: Vector3 = Vector3::new(0.0, 1.0, 0.0);
pub(crate) const Z: Vector3 = Vector3::new(0.0, 0.0, 1.0);
pub(crate) const DOWN: Vector3 = Vector3::new(0.0, -1.0, 0.0);
const PI: f64 = std::f64::consts::PI;
/// The ground a landing settles on: flatter than this (the normal's
/// height; the 2004 game's 0.9375).
const SETTLE: f64 = 0.9375;
/// sweep()'s columns: the middle one only, along the fuselage.
const FUSELAGE: bool = true;

/// `alpha(rate, dt)`: `1 - Math.exp(-rate*dt)`.
fn alpha(rate: f64, dt: f64) -> f64 {
    1.0 - math::exp(-rate * dt)
}

/// JavaScript's truthiness of a number (a boolean as 0 or 1, undefined as NaN).
#[inline(always)]
pub fn truthy(x: f64) -> bool {
    !(x == 0.0 || x.is_nan())
}

/// `x || 0`.
#[inline(always)]
pub fn or_zero(x: f64) -> f64 {
    if truthy(x) {
        x
    } else {
        0.0
    }
}

/// An input object as stepFlight reads it: each key's value as a number (a
/// boolean as 0 or 1), NaN where it is undefined; the stick keeps null and
/// undefined (None) apart from numbers, for `??`.
#[derive(Clone, Copy, Debug)]
pub struct FlightInput {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub pitch: f64,
    pub up: f64,
    pub down: f64,
    pub takeoff: f64,
    pub stick_x: Option<f64>,
    pub stick_y: Option<f64>,
    pub boost: f64,
    pub trick: f64,
    pub immelmann: f64,
    pub roll_lock: f64,
    pub target_lock: f64,
    pub bot: f64,
    pub response_curve: f64,
    /// `input.steeringAssist === false`.
    pub no_assist: bool,
}

impl Default for FlightInput {
    /// An object with none of the keys (every one undefined).
    fn default() -> Self {
        FlightInput {
            forward: f64::NAN,
            right: f64::NAN,
            turn: f64::NAN,
            pitch: f64::NAN,
            up: f64::NAN,
            down: f64::NAN,
            takeoff: f64::NAN,
            stick_x: None,
            stick_y: None,
            boost: f64::NAN,
            trick: f64::NAN,
            immelmann: f64::NAN,
            roll_lock: f64::NAN,
            target_lock: f64::NAN,
            bot: f64::NAN,
            response_curve: f64::NAN,
            no_assist: false,
        }
    }
}

/// What sweep() finds nearest: how far along, where, its normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub distance: f64,
    pub point: Vector3,
    pub normal: Vector3,
}

/// The hull at rest for sweep(): surfaces inside the box it fills there are
/// looked past.
#[derive(Clone, Copy, Debug)]
pub struct Rest {
    pub position: Vector3,
    pub quaternion: Quaternion,
}

/// landingSite(): the floor's height and normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    pub y: f64,
    pub normal: Vector3,
}

/// groundBelow(): the ray's hit and the height over it.
#[derive(Clone, Copy, Debug)]
pub struct GroundHit {
    pub hit: Hit,
    pub height: f64,
}

/// levelQuaternion(q, out): the frame with q's heading, level.
pub fn level_quaternion(q: &Quaternion) -> Quaternion {
    let mut forward = Z;
    forward.apply_quaternion(q);
    forward.y = 0.0;
    if forward.length_sq() < 1e-8 {
        forward = X;
        forward.apply_quaternion(q).cross(&Y);
    }
    forward.normalize();
    let mut right = Vector3::default();
    right.cross_vectors(&Y, &forward).normalize();
    let mut m = Matrix4::default();
    m.make_basis(&right, &Y, &forward);
    let mut out = Quaternion::default();
    out.set_from_rotation_matrix(&m);
    out
}

/// insideAtRest(b, rest, point): a point inside the box b placed at rest
/// (`inverse` the rest's quaternion inverted).
fn inside_at_rest(min: &Vector3, max: &Vector3, rest: &Rest, inverse: &Quaternion, point: &Vector3) -> bool {
    let mut local = *point;
    local.sub(&rest.position).apply_quaternion(inverse);
    local.x > min.x - 0.01 && local.x < max.x + 0.01 && local.y > min.y - 0.01 && local.y < max.y + 0.01 && local.z > min.z - 0.01 && local.z < max.z + 0.01
}

impl Hull {
    /// collision.cast with the hull's own body left out.
    fn flight_cast<W: FlightWorld>(&self, w: &mut W, origin: &Vector3, direction: &Vector3, far: f64) -> Option<Hit> {
        w.cast(origin, direction, far, Some(self.body))
    }

    /// sweep(v, from, to, q, below, rest, fuselage): a grid of rays across
    /// the hull's box (27, or the middle column's 9) from `from` to `to`,
    /// each looking past what lies no higher than `below` and what the hull
    /// at `rest` overlaps; the nearest contact.
    #[allow(clippy::too_many_arguments)]
    pub fn sweep<W: FlightWorld>(&self, w: &mut W, from: &Vector3, to: &Vector3, q: &Quaternion, below: f64, rest: Option<&Rest>, fuselage: bool) -> Option<Contact> {
        let mut travel = *to;
        travel.sub(from);
        let distance = travel.length();
        if distance < 1e-8 {
            return None;
        }
        travel.divide_scalar(distance);
        let mut nearest: Option<Contact> = None;
        let (bmin, bmax) = (self.bounds_min, self.bounds_max);
        let rest_inverse = rest.map(|r| {
            let mut q = r.quaternion;
            q.invert();
            q
        });
        let mid_x = (bmin.x + bmax.x) / 2.0;
        let xs_all = [bmin.x, mid_x, bmax.x];
        let xs: &[f64] = if fuselage { &xs_all[1..2] } else { &xs_all };
        let ys = [bmin.y + 0.12, (bmin.y + bmax.y) / 2.0, bmax.y];
        let zs = [bmin.z, (bmin.z + bmax.z) / 2.0, bmax.z];
        for &x in xs {
            for &y in &ys {
                for &z in &zs {
                    let mut origin = Vector3::new(x, y, z);
                    origin.apply_quaternion(q).add(from);
                    let mut contact: Option<Hit> = None;
                    let mut skipped = 0.0;
                    let mut pass = 0;
                    while pass < 4 && contact.is_none() && skipped < distance + 0.15 {
                        contact = self.flight_cast(w, &origin, &travel, distance + 0.15 - skipped);
                        match contact {
                            Some(c) if c.point.y <= below || rest.is_some_and(|r| inside_at_rest(&bmin, &bmax, r, rest_inverse.as_ref().unwrap(), &c.point)) => {
                                skipped += c.distance + 0.01;
                                origin.add_scaled_vector(&travel, c.distance + 0.01);
                                contact = None;
                            }
                            _ => break,
                        }
                        pass += 1;
                    }
                    if let Some(c) = contact {
                        if nearest.is_none_or(|n| skipped + c.distance < n.distance) {
                            nearest = Some(Contact { distance: skipped + c.distance, point: c.point, normal: c.normal });
                        }
                    }
                }
            }
        }
        nearest
    }

    /// restingTop(v, q, at): the top of what a hull resting at `at` with
    /// frame q sits in (-Infinity: nothing), from 9 rays down from just
    /// above its box's floor.
    pub fn resting_top<W: FlightWorld>(&self, w: &mut W, q: &Quaternion, at: &Vector3) -> f64 {
        let (bmin, bmax) = (self.bounds_min, self.bounds_max);
        let mut top = f64::NEG_INFINITY;
        let reach = match self.stand_height() {
            Some(stand) => max(2.0, bmin.y + stand + 2.22),
            None => 2.0,
        };
        for x in [bmin.x, (bmin.x + bmax.x) / 2.0, bmax.x] {
            for z in [bmin.z, (bmin.z + bmax.z) / 2.0, bmax.z] {
                let mut from = Vector3::new(x, bmin.y + 2.12, z);
                from.apply_quaternion(q).add(at);
                if let Some(under) = self.flight_cast(w, &from, &DOWN, reach) {
                    top = max(top, under.point.y + 0.05);
                }
            }
        }
        top
    }

    /// landingSite(v): a landing zone the craft is in, else ground within
    /// landingHeight under its box flat enough to settle on (not in space).
    pub fn landing_site<W: FlightWorld>(&self, w: &mut W, env: &Environment, t: &Tuning) -> Option<Site> {
        let zone = env.landing_zones.as_ref().and_then(|zones| {
            zones.iter().find(|z| {
                let p = &self.position;
                p.x >= z.min[0] && p.x <= z.max[0] && p.y >= z.min[1] && p.y <= z.max[1] && p.z >= z.min[2] && p.z <= z.max[2]
            })
        });
        let mut origin = self.position;
        origin.y += self.bounds_min.y + 0.3;
        let ground = self.flight_cast(w, &origin, &DOWN, t.landing_height + 0.3);
        if let Some(z) = zone {
            return Some(Site { y: z.floor_y, normal: Y });
        }
        if !env.space {
            if let Some(g) = ground {
                if g.normal.y >= t.max_slope {
                    return Some(Site { y: g.point.y, normal: g.normal });
                }
            }
        }
        None
    }

    /// transition(v, state): a new flight state, its clock from 0, its
    /// start where the craft is.
    pub fn transition(&mut self, state: FlightState) {
        let f = &mut self.flight;
        f.state = Some(state);
        f.state_time = 0.0;
        f.start = self.position;
        f.start_q = self.quaternion;
    }

    /// abandonLift(v): a lift nobody flies, still over its pad, goes back
    /// down onto it.
    pub fn abandon_lift(&mut self) {
        if self.flight.state == Some(FlightState::TakingOff) && self.flight.gear >= 1.0 {
            self.transition(FlightState::Landing);
        }
    }

    /// startTrick(v, kind, direction).
    fn start_trick(&mut self, t: &Tuning, kind: Trick, direction: f64) {
        let f = &mut self.flight;
        if f.trick.on() || f.power < t.trick_cost || f.trick_cooldown > 0.0 || f.state != Some(FlightState::Flying) {
            return;
        }
        f.trick_right = X;
        f.trick_right.apply_quaternion(&self.quaternion);
        if kind == Trick::Immelmann {
            let mut half = Quaternion::default();
            half.set_from_axis_angle(&X, -PI);
            f.trick_loop_q = self.quaternion;
            f.trick_loop_q.multiply(&half);
            f.trick_end_q = level_quaternion(&f.trick_loop_q);
        }
        f.power -= t.trick_cost;
        f.trick = kind;
        f.trick_time = 0.0;
        f.trick_direction = direction;
        f.trick_pending = -1.0;
    }

    /// heightAbove(v): how high the craft's origin is over what lies below
    /// it (1024 for nothing within 1024 m).
    pub fn height_above<W: FlightWorld>(&self, w: &mut W) -> f64 {
        match self.ground_below(w) {
            Some(g) => g.height,
            None => 1024.0,
        }
    }

    /// groundBelow(v, out): the 1024 m ray down from the craft's origin
    /// (from 0.1 m over its gear's lowest point where that is higher).
    pub fn ground_below<W: FlightWorld>(&self, w: &mut W) -> Option<GroundHit> {
        let lift = match self.flight.gear_foot {
            Some(g) if g < 0.1 => 0.1 - g,
            _ => 0.0,
        };
        let mut from = self.position;
        from.y += lift;
        let below = self.flight_cast(w, &from, &DOWN, 1024.0 + lift)?;
        Some(GroundHit { hit: below, height: below.distance - lift })
    }

    /// standHeight(v): how high a landed craft's origin stands on its gear
    /// (None without gear).
    pub fn stand_height(&self) -> Option<f64> {
        self.flight.gear_foot.map(|g| max(g, -self.bounds_min.y))
    }

    /// settledHeight(v): on its gear, else its collision 0.15 m clear.
    fn settled_height(&self) -> f64 {
        match self.stand_height() {
            Some(s) => s,
            None => -self.bounds_min.y + 0.15,
        }
    }

    /// noseSpeed(v, c, dt, throttle): the speed along the nose, toward the
    /// stick's target at the craft's acceleration.
    fn nose_speed(&self, c: &Config, t: &Tuning, dt: f64, throttle: f64) -> f64 {
        let mut forward = Z;
        forward.apply_quaternion(&self.quaternion);
        let boosting = self.flight.boosting;
        let rate = c.acceleration * (if boosting { t.boost_acceleration } else { 1.0 }) * dt;
        let current = self.velocity.dot(&forward);
        clamp(if boosting { c.max_speed } else { throttle_speed(c, throttle) }, current - rate, current + rate)
    }

    /// lean(v, c, dt, per): the drawn hull's lean into a turn.
    fn lean(&mut self, c: &Config, dt: f64, per: f64) {
        let mut inverse = self.quaternion;
        inverse.invert();
        let mut local = self.velocity;
        local.apply_quaternion(&inverse);
        let f = &mut self.flight;
        let pull = (self.yaw_rate * local.z - f.roll_rate * local.y) / per;
        f.bank += (-c.bank_angle * pull - f.bank) * c.bank_filter * dt;
    }

    /// levelRoll(v, c): the roll rate that levels the wings.
    fn level_roll(&self, c: &Config) -> f64 {
        let mut left = X;
        left.apply_quaternion(&self.quaternion);
        -c.level_filter * left.y
    }

    /// turnFrame(v, pitchStep, yawStep, rollStep): the frame turned about
    /// the rates' common axis in its own frame.
    fn turn_frame(&mut self, pitch_step: f64, yaw_step: f64, roll_step: f64) {
        let mut axis = Vector3::new(pitch_step, yaw_step, roll_step);
        let angle = axis.length();
        if angle > 1e-12 {
            axis.divide_scalar(angle);
            let mut dq = Quaternion::default();
            dq.set_from_axis_angle(&axis, angle);
            self.quaternion.multiply(&dq).normalize();
        }
    }

    /// lift(v, c, dt, height, rise, throttle, stickX, stickY, pitchRate):
    /// the takeoff and the landing.
    #[allow(clippy::too_many_arguments)]
    fn lift(&mut self, c: &Config, t: &Tuning, dt: f64, height: f64, rise: f64, throttle: f64, stick_x: f64, stick_y: f64, pitch_rate: f64) {
        let p = 1.0 - self.flight.gear;
        let lifted = clamp(height / c.takeoff_height, 0.0, 1.0);
        self.flight.airspeed = self.nose_speed(c, t, dt, throttle);
        self.tilt_rate *= max(0.0, 1.0 - (1.0 - p) * c.level_filter * dt);
        let mut forward = Z;
        forward.apply_quaternion(&self.quaternion);
        self.tilt_rate -= 0.5 * c.level_filter * (1.0 - p) * forward.y;
        self.tilt_rate += (stick_y * pitch_rate * p - self.tilt_rate) * min(1.0, c.pitch_filter * dt);
        self.yaw_rate += (-stick_x * c.turn_rate * lifted - self.yaw_rate) * min(1.0, c.turn_filter * dt);
        self.flight.roll_rate = self.level_roll(c);
        let mut velocity = Z;
        velocity.apply_quaternion(&self.quaternion).multiply_scalar(self.flight.airspeed * p).add_scaled_vector(&Y, (0.875 * lifted + 0.125) * rise * (1.0 - p));
        self.velocity = velocity;
        self.lean(c, dt, c.turn_rate * max(abs(c.max_speed), 1e-6));
        let (pitch, yaw, roll) = (-self.tilt_rate * dt, self.yaw_rate * dt, self.flight.roll_rate * dt);
        self.turn_frame(pitch, yaw, roll);
    }

    /// stepFlight(v, dt, input): a craft's flight, one step.
    pub fn step_flight<W: FlightWorld>(&mut self, w: &mut W, dt: f64, input: &FlightInput) {
        let env = w.environment().clone();
        let t = w.tuning(self.flight.class);
        let c = self.flight.config;
        self.flight.previous_q = self.quaternion;
        self.flight.clock += dt;
        self.flight.state_time += dt;
        self.flight.trick_cooldown = max(0.0, self.flight.trick_cooldown - dt);
        self.flight.landing_message_time = max(0.0, self.flight.landing_message_time - dt);
        self.flight.warning = if self.flight.landing_message_time > 0.0 { self.flight.landing_message } else { Message::None };
        let site = self.landing_site(w, &env, &t);
        self.flight.floor_y = match site {
            Some(s) => s.y,
            None => f64::NAN,
        };
        self.flight.altitude = match site {
            Some(s) => max(0.0, self.position.y + self.bounds_min.y - s.y),
            None => 1000.0,
        };
        // Where a landing soon settles: the TO LAND prompt, and where a bot
        // presses the key.
        self.flight.landing_reason = if site.is_none() { Message::NoLandingZone } else { Message::PressSpace };
        // The stick, as the hull takes it.
        let assist = !input.no_assist && env.assists && self.flight.assist_id >= 0.0;
        let curve = if truthy(input.response_curve) { input.response_curve } else { t.response_curve };
        let response = |x: f64| sign(x) * pow(abs(x), curve) * (if assist { t.sticky_scale } else { 1.0 });
        let sx = clamp(input.stick_x.unwrap_or(self.flight.stick_x), -1.0, 1.0);
        let sy = clamp(input.stick_y.unwrap_or(self.flight.stick_y), -1.0, 1.0);
        let throttle = clamp(or_zero(input.forward), -1.0, 1.0);
        let pitch_rate = if truthy(input.bot) { c.pitch_rate } else { c.turn_rate };
        let action = truthy(input.takeoff) || truthy(input.up) || truthy(input.down);
        if action && self.flight.action_held != Some(true) {
            // The key refuses nothing: landed, the craft lifts off; flying,
            // it starts down; late in a takeoff it goes back down, late in a
            // landing back up. A space map lands only in its zones.
            let progress = 1.0 - self.flight.gear;
            match self.flight.state {
                Some(FlightState::Landed) => {
                    self.transition(FlightState::TakingOff);
                    self.flight.gear = 1.0;
                }
                Some(FlightState::Flying) if !self.flight.trick.on() => {
                    if env.space && site.is_none() {
                        let reason = self.flight.landing_reason;
                        self.flight.landing_message = reason;
                        self.flight.warning = reason;
                        self.flight.landing_message_time = 2.0;
                    } else {
                        self.transition(FlightState::Landing);
                    }
                }
                Some(FlightState::TakingOff) if progress > 0.75 => self.transition(FlightState::Landing),
                Some(FlightState::Landing) if progress < 0.25 => self.transition(FlightState::TakingOff),
                _ => {}
            }
        }
        self.flight.action_held = Some(action);
        self.flight.boosting = false;
        // A takeoff goes over to flight once TakeoffHeight above where it
        // rests, its gear stowing over TakeoffTime; a landing's gear comes
        // down over LandingTime.
        let mut height = 0.0;
        let mut ground: Option<GroundHit> = None;
        if self.flight.state == Some(FlightState::TakingOff) {
            height = self.height_above(w);
            if height > c.takeoff_height + c.landed_height {
                self.flight.gear = max(0.0, self.flight.gear - dt / c.takeoff_time);
            }
            if self.flight.gear <= 0.0 {
                self.flight.gear = 0.0;
                self.transition(FlightState::Flying);
            }
        } else if self.flight.state == Some(FlightState::Landing) {
            ground = self.ground_below(w);
            height = match ground {
                Some(g) => g.height,
                None => 1024.0,
            };
            self.flight.gear = min(1.0, self.flight.gear + dt / c.landing_time);
        }
        match self.flight.state {
            Some(FlightState::Landed) => {
                self.velocity.set(0.0, 0.0, 0.0);
                self.flight.airspeed = 0.0;
                self.yaw_rate = 0.0;
                self.tilt_rate = 0.0;
                self.flight.roll_rate = 0.0;
                self.flight.bank = 0.0;
            }
            Some(FlightState::TakingOff) => self.take_off(w, &c, &t, dt, height, throttle, response(sx), response(sy), pitch_rate),
            Some(FlightState::Landing) => self.land(w, &c, &t, dt, height, ground, throttle, response(sx), response(sy), pitch_rate),
            Some(FlightState::Flying) => self.fly_on(w, &env, &c, &t, dt, input, throttle, sx, sy, pitch_rate, &response),
            _ => {}
        }
        if !self.flight.boosting {
            self.flight.power = min(100.0, self.flight.power + t.power_recharge * dt);
        }
        self.flight.stick_x *= math::exp(-t.stick_return * dt);
        self.flight.stick_y *= math::exp(-t.stick_return * dt);
        self.flight.grounded = self.flight.state == Some(FlightState::Landed);
        self.sleeping = Some(!self.occupied() && self.flight.grounded);
        self.flight.throttle = self.flight.airspeed;
        // Heading projections, read by the AI's and the seats' services;
        // the roll is the drawn lean.
        let mut forward = Z;
        forward.apply_quaternion(&self.quaternion);
        self.yaw = math::atan2(forward.x, forward.z);
        self.tilt = math::asin(clamp(forward.y, -1.0, 1.0));
        self.roll = self.flight.bank;
        if let Some(seat) = self.seats.first_mut() {
            seat.aim_yaw = self.yaw;
            seat.pitch = self.tilt;
        }
        if self.flight.landing_message_time > 0.0 {
            self.flight.warning = self.flight.landing_message;
        }
        self.sync_body(w);
    }

    /// stepFlight's takeoff: the lift, then the move, the pad it lifts from
    /// and what its parked hull overlapped looked past; under a roof it
    /// slides along it, anything else holds it where it is.
    #[allow(clippy::too_many_arguments)]
    fn take_off<W: FlightWorld>(&mut self, w: &mut W, c: &Config, t: &Tuning, dt: f64, height: f64, throttle: f64, sx: f64, sy: f64, pitch_rate: f64) {
        self.lift(c, t, dt, height, c.takeoff_speed, throttle, sx, sy, pitch_rate);
        let mut next = self.position;
        next.add_scaled_vector(&self.velocity, dt);
        let (start, start_q) = (self.flight.start, self.flight.start_q);
        let pad = self.resting_top(w, &start_q, &start);
        let parked = Rest { position: start, quaternion: start_q };
        let (position, quaternion) = (self.position, self.quaternion);
        let mut contact = self.sweep(w, &position, &next, &quaternion, pad, Some(&parked), false);
        if contact.is_some() {
            self.flight.warning = Message::TakeoffBlocked;
        }
        if contact.is_some() && next.y > self.position.y {
            next.y = self.position.y;
            contact = self.sweep(w, &position, &next, &quaternion, pad, Some(&parked), false);
        }
        if contact.is_none() {
            self.position = next;
        }
    }

    /// stepFlight's landing: the lift run back, coming to rest on the
    /// ground under its middle (or on the water), its whole hull meeting
    /// what it runs into on its way, its fuselage what it comes down on.
    #[allow(clippy::too_many_arguments)]
    fn land<W: FlightWorld>(&mut self, w: &mut W, c: &Config, t: &Tuning, dt: f64, height: f64, ground: Option<GroundHit>, throttle: f64, sx: f64, sy: f64, pitch_rate: f64) {
        self.lift(c, t, dt, height, -c.landing_speed, throttle, sx, sy, pitch_rate);
        let mut next = self.position;
        next.add_scaled_vector(&self.velocity, dt);
        let rest = self.settled_height();
        let wet = if w.water_over(next.x, next.z) { w.water_level() } else { f64::NEG_INFINITY };
        let floor = max(
            match ground {
                Some(g) => g.hit.point.y + rest,
                None => f64::NEG_INFINITY,
            },
            wet,
        );
        let down = next.y <= floor;
        if down {
            next.y = floor;
        }
        let level = level_quaternion(&self.quaternion);
        let below = if floor.is_finite() { self.resting_top(w, &level, &Vector3::new(next.x, floor, next.z)) } else { f64::NEG_INFINITY };
        let mut along = Vector3::new(next.x, self.position.y, next.z);
        let (position, quaternion) = (self.position, self.quaternion);
        if let Some(wall) = self.sweep(w, &position, &along, &quaternion, below, None, false) {
            // Run into on its way, it stops there, hurt as it would be in
            // flight.
            let impact = max(0.0, -(along.x - self.position.x) * wall.normal.x - (along.z - self.position.z) * wall.normal.z) / dt;
            if impact > t.lethal_impact_speed {
                self.damage(w, f64::INFINITY, crate::gen::OrdnanceOwner::Null);
            } else {
                let amount = self.max_health * min(t.scrape_damage_limit, pow(impact / t.impact_damage_speed, 2.0));
                self.damage(w, amount, crate::gen::OrdnanceOwner::Null);
            }
            along = self.position;
        }
        let quaternion = self.quaternion;
        let blocked = self.sweep(w, &along, &Vector3::new(along.x, next.y, along.z), &quaternion, below, None, FUSELAGE);
        if blocked.is_some() {
            self.flight.warning = Message::LandingBlocked;
        }
        // What it was held back from slows it, as a bump does in flight.
        let to = Vector3::new(along.x, if blocked.is_some() { self.position.y } else { next.y }, along.z);
        self.velocity.sub_vectors(&to, &self.position).divide_scalar(dt);
        self.position = to;
        // Down on flat enough ground it lands; a hull it comes down on sends
        // it back up; on water or a steeper slope it stays on its way down.
        if let Some(g) = ground {
            if self.alive && down && blocked.is_none() && floor == g.hit.point.y + rest {
                if g.hit.body.is_some_and(|b| w.body_is_hull(b)) {
                    self.transition(FlightState::TakingOff);
                } else if g.hit.normal.y > SETTLE {
                    self.transition(FlightState::Landed);
                    self.flight.gear = 1.0;
                    self.flight.airspeed = 0.0;
                    self.flight.bank = 0.0;
                    self.velocity.set(0.0, 0.0, 0.0);
                    self.quaternion = level;
                }
            }
        }
    }

    /// stepFlight's flight: boost, the tricks, the stick's rates, the roll,
    /// boundary recovery, the move and what it strikes, the terrain warning.
    #[allow(clippy::too_many_arguments)]
    fn fly_on<W: FlightWorld>(
        &mut self,
        w: &mut W,
        env: &Environment,
        c: &Config,
        t: &Tuning,
        dt: f64,
        input: &FlightInput,
        throttle: f64,
        sx: f64,
        sy: f64,
        pitch_rate: f64,
        response: &dyn Fn(f64) -> f64,
    ) {
        let roll = clamp(or_zero(input.right), -1.0, 1.0);
        if !truthy(input.boost) {
            self.flight.boost_exhausted = false;
        }
        self.flight.boosting = truthy(input.boost) && throttle > 0.0 && self.flight.power > 0.0 && !self.flight.boost_exhausted;
        if self.flight.boosting {
            self.flight.power = max(0.0, self.flight.power - t.power_drain * dt);
            if !truthy(self.flight.power) {
                self.flight.boost_exhausted = true;
            }
        }
        self.flight.airspeed = self.nose_speed(c, t, dt, throttle);
        if truthy(input.trick) && self.flight.trick_held != Some(true) {
            if self.flight.trick_pending >= 0.0 && self.flight.clock - self.flight.trick_pending <= t.trick_window {
                self.start_trick(t, Trick::Loop, 1.0);
            } else {
                self.flight.trick_pending = self.flight.clock;
            }
        }
        self.flight.trick_held = Some(truthy(input.trick));
        if truthy(input.immelmann) && self.flight.immelmann_held != Some(true) {
            self.start_trick(t, Trick::Immelmann, 1.0);
        }
        self.flight.immelmann_held = Some(truthy(input.immelmann));
        if self.flight.trick_pending >= 0.0 && self.flight.clock - self.flight.trick_pending > t.trick_window {
            self.flight.trick_pending = -1.0;
        }
        // A short wait lets a double tap take precedence over mouse noise.
        if self.flight.trick_pending >= 0.0 && (truthy(roll) || abs(sx) > 0.3) && self.flight.clock - self.flight.trick_pending > 0.06 {
            self.start_trick(t, Trick::Barrel, sign(if truthy(roll) { roll } else { sx }));
        }
        // The stick asks for a turn at TurnRate and a pitch, the rates
        // follow it at TurnFilter and PitchFilter; boost costs a share.
        let authority = if self.flight.boosting { t.boost_turn } else { 1.0 };
        self.yaw_rate += (-response(sx) * c.turn_rate * authority - self.yaw_rate) * min(1.0, c.turn_filter * dt);
        self.tilt_rate += (response(sy) * pitch_rate * authority - self.tilt_rate) * min(1.0, c.pitch_filter * dt);
        self.flight.roll_lock = truthy(input.roll_lock);
        // A/D roll the frame; let go, its wings level (not in space), and
        // the roll lock holds the roll as it is.
        let roll_rate = if self.flight.roll_lock {
            0.0
        } else if truthy(roll) {
            roll * t.roll_rate
        } else if env.space || self.flight.trick.on() {
            0.0
        } else {
            self.level_roll(c)
        };
        let mut immelmann_roll = -1.0;
        let mut lateral_step = 0.0;
        let mut pitch_step = -self.tilt_rate * dt;
        let mut roll_step = roll_rate * dt;
        let mut yaw_step = self.yaw_rate * dt;
        if self.flight.trick.on() {
            let f = &mut self.flight;
            let duration = match f.trick {
                Trick::Barrel => t.barrel_time,
                Trick::Loop => t.loop_time,
                _ => t.immelmann_time,
            };
            let a = f.trick_time / duration;
            let b = min(1.0, (f.trick_time + dt) / duration);
            yaw_step = 0.0;
            match f.trick {
                Trick::Barrel => {
                    roll_step = (b - a) * PI * 2.0 * f.trick_direction;
                    lateral_step = t.barrel_displacement * (math::cos(a * PI) - math::cos(b * PI)) * 0.5 * f.trick_direction;
                }
                Trick::Loop => {
                    pitch_step = -(b - a) * PI * 2.0;
                    roll_step = 0.0;
                }
                _ => {
                    pitch_step = -(min(b / 0.7, 1.0) - min(a / 0.7, 1.0)) * PI;
                    roll_step = (max(0.0, (b - 0.7) / 0.3) - max(0.0, (a - 0.7) / 0.3)) * PI;
                }
            }
            if f.trick == Trick::Immelmann && b >= 0.7 {
                immelmann_roll = (b - 0.7) / 0.3;
            }
            f.trick_time += dt;
            if b >= 1.0 {
                f.trick = Trick::None;
                f.trick_cooldown = t.trick_cooldown;
            }
        }
        self.turn_frame(pitch_step, yaw_step, roll_step);
        if immelmann_roll >= 0.0 {
            let (from, to) = (self.flight.trick_loop_q, self.flight.trick_end_q);
            self.quaternion.slerp_quaternions(&from, &to, min(1.0, immelmann_roll));
        }
        self.flight.roll_rate = roll_step / dt;
        // Boundary recovery changes the attitude smoothly; the velocity
        // always follows it.
        let mut forward = Z;
        forward.apply_quaternion(&self.quaternion);
        let mut recovery = false;
        let mut desired = forward;
        let ceiling = env.ceiling.unwrap_or(env.max_fly_height);
        if !env.space && self.position.y > ceiling {
            desired.y = -0.5;
            recovery = true;
            self.flight.warning = Message::Ceiling;
        }
        if let Some((lo, hi)) = env.bounds {
            let (min_v, max_v) = (Vector3::new(lo[0], lo[1], lo[2]), Vector3::new(hi[0], hi[1], hi[2]));
            let p = self.position;
            if p.x < min_v.x || p.x > max_v.x || p.z < min_v.z || p.z > max_v.z || env.space && (p.y < min_v.y || p.y > max_v.y) {
                desired.add_vectors(&min_v, &max_v).multiply_scalar(0.5).sub(&p).normalize();
                recovery = true;
                self.flight.warning = Message::Leaving;
            }
        }
        if recovery {
            let mut dq = Quaternion::default();
            dq.set_from_unit_vectors(&forward, desired.normalize());
            let target = dq;
            dq.slerp_quaternions(&Quaternion::default(), &target, alpha(t.recovery_sharpness, dt));
            self.quaternion.premultiply(&dq).normalize();
        }
        let mut velocity = Z;
        velocity.apply_quaternion(&self.quaternion).multiply_scalar(self.flight.airspeed);
        self.velocity = velocity;
        self.lean(c, dt, max(abs(c.max_speed), 1e-6));
        let mut next = self.position;
        next.add_scaled_vector(&self.velocity, dt).add_scaled_vector(&self.flight.trick_right, lateral_step);
        let (position, quaternion) = (self.position, self.quaternion);
        let contact = self.sweep(w, &position, &next, &quaternion, f64::NEG_INFINITY, None, false);
        if let Some(contact) = contact {
            let speed = self.velocity.length();
            let impact = max(0.0, -self.velocity.dot(&contact.normal));
            self.flight.trick = Trick::None;
            self.flight.trick_cooldown = t.trick_cooldown;
            let velocity = self.velocity;
            self.position.add_scaled_vector(&velocity, max(0.0, contact.distance - 0.2) / max(speed, 1.0));
            if impact > t.lethal_impact_speed && impact / speed > t.lethal_impact_cosine {
                self.damage(w, f64::INFINITY, crate::gen::OrdnanceOwner::Null);
            } else {
                let amount = self.max_health * min(t.scrape_damage_limit, pow(impact / t.impact_damage_speed, 2.0));
                self.damage(w, amount, crate::gen::OrdnanceOwner::Null);
                self.velocity.add_scaled_vector(&contact.normal, impact + 2.0);
                if self.velocity.length_sq() > 1.0 {
                    forward = Z;
                    forward.apply_quaternion(&self.quaternion);
                    desired = self.velocity;
                    desired.normalize();
                    let mut dq = Quaternion::default();
                    dq.set_from_unit_vectors(&forward, &desired);
                    self.quaternion.premultiply(&dq);
                }
                self.flight.airspeed = max(self.flight.config.min_speed, self.velocity.length());
            }
        } else {
            self.position = next;
        }
        // The way it is going: a craft that flies backward looks behind.
        forward = Z;
        forward.apply_quaternion(&self.quaternion).multiply_scalar(if self.flight.airspeed < 0.0 { -1.0 } else { 1.0 });
        let ahead = abs(self.flight.airspeed) * t.terrain_lookahead;
        let position = self.position;
        self.flight.terrain_warning = ahead > 0.0 && self.flight_cast(w, &position, &forward, ahead).is_some();
    }
}
