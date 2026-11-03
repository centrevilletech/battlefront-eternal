//! src/flight/ai.js: a bot pilot's stick (`pilotInput`), from the steering
//! point its path planning gives (or what it aims at), and the 2004 game's
//! altitude stage (`raiseOverTerrain`), which raises that point so the
//! craft clears what lies ahead by 25 m. The vehicle AI calls these (its
//! steering, src/ai/classic/vehicles/steering.js); all motion stays the
//! flight model's.

use super::config::{throttle_for, Tuning};
use super::model::{or_zero, DOWN};
use super::FlightState;
use crate::rig::math;
use crate::vehicles::{BodyId, Hit, Hull};
use three::{clamp, Vector3};
use v8math::{abs, floor, hypot, max, min};

const PI: f64 = std::f64::consts::PI;

/// The terrain probes ahead: (share of the look-ahead, angle off the nose).
const PROBES: [(f64, f64); 5] = [(1.0, 0.0), (0.7, PI / 4.0), (0.7, -PI / 4.0), (0.5, PI / 2.0), (0.5, -PI / 2.0)];

/// What the pilot asks of the battle: the collision world, the AI's
/// environment's terrain and water heights, and the rounds in flight.
pub trait PilotWorld {
    /// CollisionWorld.cast, leaving out `ignore`.
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit>;
    /// `env.height(x, z)`, None where the environment has no height.
    fn terrain(&mut self, x: f64, z: f64) -> Option<f64>;
    /// `env.water(x, z)`, None where the environment has no water.
    fn water(&mut self, x: f64, z: f64) -> Option<f64>;
    /// Whether a round in flight that is not dead is guided at the hull's
    /// entry (`targetId`) and within `range` of `position`
    /// (`ordnance.rounds.some(...)`).
    fn incoming(&mut self, target_id: f64, position: &Vector3, range: f64) -> bool;
}

/// The AI's environment as the pilot reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotEnv {
    /// `env.minFlyHeight ?? -200`.
    pub min_fly_height: f64,
    /// `env.maxFlyHeight` (None: undefined, no cap).
    pub max_fly_height: Option<f64>,
    /// `Boolean(env.hasPath)`.
    pub has_path: bool,
    /// The battle's flight environment's `space`.
    pub space: bool,
}

/// A steering request (src/ai/classic/nav: a valid one has a steering point
/// and a mode; the tow-cable run's carries its blend and throttle).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Request {
    pub valid: bool,
    pub steering_target: Option<Vector3>,
    pub mode: f64,
    /// `request.blend` where it is a number.
    pub blend: Option<f64>,
    /// `request.throttle` (NaN: not a finite number).
    pub throttle: f64,
}

/// The unit's steering bag as the pilot reads it (`unit.steer`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steer {
    pub land: bool,
    pub loop_: bool,
    pub path_altitude: bool,
}

/// The pilot's stick (`out`). The landing path leaves trick, immelmann and
/// boost unset, which seatAnalogs (its caller) reads through Boolean():
/// false here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stick {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub pitch: f64,
    pub up: f64,
    pub down: f64,
    pub brake: bool,
    pub trick: bool,
    pub immelmann: bool,
    pub boost: bool,
}

/// raiseOverTerrain's blend: a request of mode 1 eases toward the point's
/// own height near it; a number takes that share of the climb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    None,
    Ease,
    Share(f64),
}

impl Hull {
    /// raiseOverTerrain(v, env, point, blend): `point` raised (never
    /// lowered) so the craft clears what lies ahead by 25 m; whether
    /// something other than terrain was ahead.
    pub fn raise_over_terrain<W: PilotWorld>(&self, w: &mut W, env: &PilotEnv, point: &mut Vector3, blend: Blend) -> bool {
        let c = &self.flight.config;
        let p = self.position;
        let speed = self.flight.airspeed;
        let ahead = max(1.5 * speed, 1.0);
        let low = env.min_fly_height;
        let terrain = |w: &mut W, x: f64, z: f64| match w.terrain(x, z) {
            Some(h) if h.is_finite() => h,
            _ => f64::NEG_INFINITY,
        };
        let mut nose = Vector3::new(0.0, 0.0, 1.0);
        nose.apply_quaternion(&self.quaternion);
        let mut height = f64::NEG_INFINITY;
        for (share, angle) in PROBES {
            let cos = math::cos(angle);
            let sin = math::sin(angle);
            let reach = ahead * share;
            let h = terrain(w, p.x + (nose.x * cos - nose.z * sin) * reach, p.z + (nose.z * cos + nose.x * sin) * reach);
            height = max(max(height, h), low);
        }
        let mut from = p;
        from.y -= 1.5 * or_zero(self.flight.radius);
        let met = w.cast(&from, &nose, ahead, Some(self.body));
        let obstacle = met.is_some_and(|hit| !hit.terrain);
        if let (true, Some(hit)) = (obstacle, met) {
            let above = Vector3::new(hit.point.x, max(hit.point.y, p.y) + 200.0, hit.point.z);
            let roof = w.cast(&above, &DOWN, above.y - hit.point.y, Some(self.body));
            let summit = max(hit.point.y, roof.map_or(f64::NEG_INFINITY, |r| r.point.y));
            let share = max(hit.distance / ahead, 1e-3);
            height = max(height, if summit > p.y { p.y + (summit - p.y) / share } else { summit });
        }
        let water = w.water(p.x, p.z);
        let under = max(terrain(w, p.x, p.z), match water {
            Some(h) if h.is_finite() => h,
            _ => f64::NEG_INFINITY,
        });
        let top = max(height, under) + 25.0;
        let mut y = p.y + hypot(point.x - p.x, point.z - p.z) * (top - p.y) / ahead;
        match blend {
            Blend::Share(share) => y = point.y + (y - point.y) * clamp(share, 0.0, 1.0),
            Blend::Ease => {
                let r = max(2.0 * speed / c.turn_rate, 2.0);
                let w = (clamp(point.distance_to(&p), 1.0, r) - 1.0) / (r - 1.0);
                y = point.y + (y - point.y) * w;
            }
            Blend::None => {}
        }
        if y > point.y {
            point.y = y;
        }
        obstacle
    }

    /// pilotInput(unit, request, aim, dt, env, out): a bot pilot's stick
    /// for the craft he flies. `tuning`: its class's live tuning (the
    /// trick's cost); `ai_target_id`: the hull's registry entry's id (the
    /// rounds guided at it).
    #[allow(clippy::too_many_arguments)]
    pub fn pilot_input<W: PilotWorld>(&self, w: &mut W, env: &PilotEnv, tuning: &Tuning, request: Option<&Request>, aim: Option<&Vector3>, steer: &Steer, ai_target_id: f64) -> Stick {
        let c = &self.flight.config;
        let stop = throttle_for(c, max(c.min_speed, 0.0));
        let slow = throttle_for(c, max(c.min_speed, 0.25 * c.max_speed));
        let travel = if c.cruise_speed > 0.0 { 0.0 } else { 0.5 };
        let valid = request.is_some_and(|r| r.valid);
        let mut target: Option<Vector3> = if valid { request.and_then(|r| r.steering_target) } else { aim.copied() };
        let mut out = Stick::default();
        if steer.land {
            out.forward = stop;
            out.down = if self.flight.state == Some(FlightState::Flying) && self.flight.landing_reason == super::Message::PressSpace { 1.0 } else { 0.0 };
            if !env.space && self.flight.landing_reason == super::Message::NoLandingZone {
                out.pitch = -0.3;
            }
            return out;
        }
        let mode = if valid { request.unwrap().mode } else { -1.0 };
        let moving = valid && mode != 0.0;
        if self.flight.grounded && moving {
            out.up = if floor(2.0 * or_zero(self.flight.state_time)) % 2.0 == 0.0 { 1.0 } else { 0.0 };
        }
        let own_heights = (env.has_path && steer.path_altitude) && mode != 0.0 && mode != 1.0 && mode != 3.0;
        let mut obstacle = false;
        if let (Some(t), true, false, false) = (target, valid, own_heights, self.flight.grounded) {
            let mut raised = t;
            let r = request.unwrap();
            let blend = if mode == 1.0 {
                Blend::Ease
            } else if mode == 3.0 && r.blend.is_some() {
                // (A blend of 0 is a number: a share of none of the climb.)
                Blend::Share(r.blend.unwrap())
            } else {
                Blend::None
            };
            obstacle = self.raise_over_terrain(w, env, &mut raised, blend);
            if let Some(cap) = env.max_fly_height {
                if raised.y > cap {
                    raised.y = cap;
                }
            }
            target = Some(raised);
        }
        if let Some(t) = target {
            let mut inverse = self.quaternion;
            inverse.invert();
            let mut delta = t;
            delta.sub(&self.position).apply_quaternion(&inverse).normalize();
            out.turn = clamp(-math::atan2(delta.x, delta.z) * 1.4, -1.0, 1.0);
            out.pitch = clamp(math::atan2(delta.y, hypot(delta.x, delta.z)) * 1.4, -1.0, 1.0);
            match request.map(|r| r.throttle).filter(|x| x.is_finite()) {
                Some(throttle) => out.forward = 2.0 * max(throttle, min(1.0, 5.0 / c.max_speed)) - 1.0,
                None => {
                    out.forward = if abs(out.turn) > 0.5 { 0.8 * slow } else { max(if delta.z > 0.95 { 0.4 } else { 0.0 }, travel) };
                }
            }
            if obstacle {
                out.forward = min(out.forward, slow);
            }
        }
        out.trick = steer.loop_ && floor(self.flight.clock * 60.0) % 4.0 == 0.0;
        out.immelmann = self.flight.power >= tuning.trick_cost && !self.flight.trick.on() && w.incoming(ai_target_id, &self.position, 300.0);
        out.boost = target.is_some_and(|t| out.forward > 0.3 && self.flight.power > 50.0 && t.distance_to(&self.position) > 800.0);
        if self.flight.terrain_warning && !own_heights {
            out.pitch = 0.9;
            out.forward = slow;
        }
        out
    }
}
