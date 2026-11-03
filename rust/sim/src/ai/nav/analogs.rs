//! The movement request and aim point turned into the unit's four
//! analogs (nav/analogs.js), plus the navigation half of the AI input
//! step's pure part. `navigationInput` itself runs the unit's control,
//! so it is a Nav service method (nav/mod.rs) built on `analogs` and
//! `apply_jet_and_posture` here.

use super::control::MovementRequest;
use super::executor::near_radius;
use super::math::{near_waypoint, tiny, Vec3};
use super::units::{kind, Air, NavUnit, Posture};

/// Class coefficients (analogs.js:33).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coefficients {
    pub forward: f64,
    pub strafe: f64,
    pub yaw: f64,
    pub pitch: f64,
}

/// analogs.js:33.
pub fn coefficients(unit: &NavUnit) -> Coefficients {
    let def = unit.class_def.as_ref();
    let s = unit.soldier.as_ref();
    Coefficients {
        forward: def.and_then(|d| d.max_speed).or_else(|| s.and_then(|s| s.max_speed)).unwrap_or(9.0),
        strafe: def.and_then(|d| d.max_strafe_speed).or_else(|| s.and_then(|s| s.max_strafe)).unwrap_or(7.0),
        yaw: def.and_then(|d| d.max_turn_speed).or_else(|| s.and_then(|s| s.max_turn)).unwrap_or(2.0),
        pitch: def.and_then(|d| d.max_pitch_speed).or_else(|| s.and_then(|s| s.max_pitch)).unwrap_or(0.5),
    }
}

/// The four analogs and the input edges navigation writes
/// (analogs.js:51's out, and the intent fields of analogs.js:208).
/// The booleans are the presses the JavaScript sets true; fields it
/// leaves alone stay as the caller had them, which for a fresh intent
/// is false. `jet_direction` models the field as written at
/// analogs.js:221: not written, null, or the request vector itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub pitch: f64,
    pub jump: bool,
    pub crouch: bool,
    pub prone: bool,
    pub jet_direction: Option<Option<Vec3>>,
}

/// analogs.js:51. `request` is the control's movement request (a missing
/// one is the JavaScript's `!request?.valid`); `aim` the aim point or
/// None for along-the-path; `facing` the options' transient facing
/// request; `executor_hold` is (has executor, its hold), the two things
/// options.executor is read for.
pub fn analogs(unit: &NavUnit, request: Option<&MovementRequest>, aim: Option<Vec3>, dt: f64, facing: bool, executor: Option<bool>) -> Intent {
    let coef = coefficients(unit);
    let p = unit.position;
    let yaw = unit.soldier.as_ref().and_then(|s| s.aim_yaw).unwrap_or(0.0);
    let fx = v8math::sin(yaw);
    let fz = v8math::cos(yaw);
    let can_strafe = coef.strafe > tiny();
    let mut invalid = request.map(|r| !r.valid).unwrap_or(true);
    let mut forward = 0.0;
    let mut right = 0.0;
    if !invalid {
        let request = request.expect("valid implies present");
        let t = request.steering_target;
        let dx = t.x - p.x;
        let dz = t.z - p.z;
        let zero = v8math::abs(dx) < tiny() && v8math::abs(dz) < tiny();
        let r = near_radius(unit);
        let dist2 = dx * dx + dz * dz;
        let inside = dist2 < r * r;
        let slow_no_path = inside && executor.is_none();
        let slow_held = inside && executor.unwrap_or(false);
        let mut stop = zero;
        if !zero {
            if slow_no_path && unit.kind == kind::WALKER {
                stop = true;
            } else if dist2 < near_waypoint() && unit.kind == kind::DROIDEKA && t.x == request.waypoint.x && t.z == request.waypoint.z {
                stop = true;
            }
        }
        if stop {
            invalid = true;
        } else {
            let l = v8math::sqrt(dist2);
            let ux = dx / l;
            let uz = dz / l;
            let a = fx * ux + fz * uz;
            let b = -(fz * ux - fx * uz);
            let slow = if slow_no_path || slow_held { v8math::min(v8math::max(l / r, 0.0), 1.0) } else { 1.0 };
            if can_strafe {
                let u = a / coef.forward;
                let v = b / coef.strafe;
                if v8math::abs(u) <= v8math::abs(v) {
                    forward = (u / v8math::abs(v)) * slow;
                    right = (if v <= 0.0 { -1.0 } else { 1.0 }) * slow;
                } else {
                    right = (v / v8math::abs(u)) * slow;
                    forward = (if u <= 0.0 { -1.0 } else { 1.0 }) * slow;
                }
            } else {
                forward = v8math::min(v8math::max(a, 0.0), 1.0) * slow;
            }
        }
    }
    let mut out = Intent { forward, right, ..Intent::default() };
    let point = if !can_strafe && !invalid {
        request.expect("valid implies present").steering_target
    } else {
        aim.unwrap_or_else(|| along_path(unit, request))
    };
    turn_toward(unit, point, dt, &coef, facing, &mut out);
    out
}

/// The third aim state (analogs.js:118).
pub fn along_path(unit: &NavUnit, request: Option<&MovementRequest>) -> Vec3 {
    let p = unit.position;
    let yaw = unit.soldier.as_ref().and_then(|s| s.aim_yaw).unwrap_or(0.0);
    if let Some(request) = request.filter(|r| r.valid) {
        let t = request.steering_target;
        let (dx, dy, dz) = (t.x - p.x, t.y - p.y, t.z - p.z);
        let r = near_radius(unit);
        if dx * dx + dz * dz > r * r {
            let l = v8math::sqrt(dx * dx + dy * dy + dz * dz);
            return Vec3::new(p.x + 30.0 * dx / l, p.y + 30.0 * dy / l, p.z + 30.0 * dz / l);
        }
    }
    Vec3::new(p.x + 30.0 * v8math::sin(yaw), p.y, p.z + 30.0 * v8math::cos(yaw))
}

/// The Soldier branch of the aim conversion (analogs.js:141).
fn turn_toward(unit: &NavUnit, point: Vec3, dt: f64, coef: &Coefficients, facing: bool, out: &mut Intent) {
    let s = unit.soldier.as_ref();
    let yaw = s.and_then(|s| s.aim_yaw).unwrap_or(0.0);
    let pitch = s.and_then(|s| s.aim_pitch).unwrap_or(0.0);
    let p = unit.position;
    let fx = v8math::sin(yaw);
    let fz = v8math::cos(yaw);
    let mut nx = point.x - p.x;
    let mut nz = point.z - p.z;
    let hl = v8math::sqrt(nx * nx + nz * nz);
    let mut turn = 0.0;
    if hl > 0.0 {
        nx /= hl;
        nz /= hl;
        turn = -(fz * nx - fx * nz);
    }
    let yaw_step = dt * coef.yaw;
    if yaw_step > 0.0 {
        turn /= yaw_step;
    }
    let limit = if facing { 2.0 } else { 1.0 };
    out.turn = v8math::min(v8math::max(turn, -limit), limit);

    let cp = v8math::cos(pitch);
    let (ax, ay, az) = (fx * cp, v8math::sin(pitch), fz * cp);
    let (rx, rz) = (fz, -fx);
    let origin = unit.origin.unwrap_or(Vec3::new(p.x, p.y + 1.7, p.z));
    let mut vx = point.x - origin.x;
    let mut vy = point.y - origin.y;
    let mut vz = point.z - origin.z;
    let across = vx * rx + vz * rz;
    vx -= across * rx;
    vz -= across * rz;
    let ahead = vx * ax + vy * ay + vz * az;
    if ahead <= 0.0 {
        out.pitch = 0.0;
        return;
    }
    let l = v8math::sqrt(vx * vx + vy * vy + vz * vz);
    vx /= l;
    vy /= l;
    vz /= l;
    let cx = ay * vz - az * vy;
    let cz = ax * vy - ay * vx;
    let mut up = -(rx * cx + rz * cz);
    let pitch_step = dt * coef.pitch;
    if pitch_step > 0.0 {
        up /= pitch_step;
    }
    out.pitch = v8math::min(v8math::max(up, -1.0), 1.0);
}

/// The jet and posture branches of navigationInput (analogs.js:212-241),
/// over the control's jet state, which the service holds: given the
/// control's (jet pending, jetting, jet vector, stance) it returns the
/// updated (pending, jetting) with the intent filled in.
pub fn apply_jet_and_posture(unit: &NavUnit, intent: &mut Intent, jet_pending: bool, jetting: bool, jet_vector: Vec3, stance: f64) -> (bool, bool) {
    let Some(s) = unit.soldier.as_ref() else {
        return (jet_pending, jetting);
    };
    let free = !(s.lock > 0.0);
    let mut jetting = jetting;
    if jet_pending || jetting {
        if free {
            if s.air == Air::Jump {
                intent.jump = true;
                let v = jet_vector;
                intent.jet_direction = Some(if v.x * v.x + v.y * v.y + v.z * v.z > 0.0 { Some(v) } else { None });
            } else if !jetting {
                intent.jump = true;
                jetting = true;
            } else if s.air != Air::Jet {
                jetting = false;
            }
        }
        return (false, jetting);
    }
    if free && s.air == Air::None && !s.dive {
        let want = if stance == 2.0 { Posture::Prone } else if stance == 1.0 { Posture::Crouch } else { Posture::Stand };
        let have = s.posture;
        if want != have {
            if want == Posture::Prone {
                intent.prone = true;
            } else if want == Posture::Crouch {
                intent.crouch = true;
            } else if have == Posture::Crouch {
                intent.crouch = true;
            } else {
                intent.prone = true;
            }
        }
    }
    (jet_pending, jetting)
}
