//! The vehicle AI's state and views as the replay harness's captures hold
//! them (feature `replay`): tools/sim-capture/vehicle-ai.mjs writes them, and
//! these read them back, for the vehicle AI's own replays
//! (tests/replay_vehicle_ai.rs) and for the AI phase's replays that carry
//! the vehicle AI's state (a control's steering bag and intent, a hull's
//! decay, the patterns' vehicle state, the service's reservations, notifier,
//! pads, support hubs and attack-run distances).

use std::collections::HashMap;

use three::{Matrix4, Quaternion, Vector3};
use sim_replay::{same_number, Node, V};

use super::controllers::role_of;
use super::patterns::PatternState;
use super::world::{ClassView, Obj, Orbit, Ref, Request, SeatIntent, Steer, StuckAt};
use crate::vehicles::BodyId;


pub fn vec3_of(v: V) -> Option<Vector3> {
    v.vec3().map(|a| Vector3::new(a[0], a[1], a[2]))
}
pub fn vec3(v: V) -> Vector3 {
    vec3_of(v).unwrap_or_else(|| panic!("not a Vector3: {}", v.show()))
}
pub fn opt_num(v: V) -> Option<f64> {
    if v.is_nullish() {
        None
    } else {
        Some(v.opt_num().unwrap_or(f64::NAN))
    }
}
pub fn num_or_nan(v: V) -> f64 {
    v.opt_num().unwrap_or(f64::NAN)
}
pub fn opt_str(v: V) -> Option<String> {
    match v.str() {
        Some(s) => Some(s.to_string()),
        None => v.opt_num().map(bfe_content::js::number_to_string),
    }
}
pub fn quat(v: V) -> Quaternion {
    match v.node() {
        Some(Node::Quat(q)) => Quaternion::new(q[0], q[1], q[2], q[3]),
        _ => panic!("not a Quaternion: {}", v.show()),
    }
}
pub fn mat4(v: V) -> Matrix4 {
    match v.node() {
        Some(Node::Mat4(e)) => Matrix4 { elements: *e },
        _ => panic!("not a Matrix4: {}", v.show()),
    }
}
/// `Number.parseFloat(x)` of a captured value (a setting's text).
pub fn parse_float(v: V) -> f64 {
    match v.str() {
        Some(s) => bfe_content::js::parse_float(s),
        None => match v.opt_num() {
            Some(n) => {
                if n == 0.0 {
                    0.0
                } else {
                    n
                }
            }
            None => f64::NAN,
        },
    }
}
pub fn same(a: f64, b: f64) -> bool {
    same_number(a, b)
}
pub fn same_opt(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => same(x, y),
        _ => false,
    }
}
pub fn same_vec(a: &Vector3, b: &Vector3) -> bool {
    same(a.x, b.x) && same(a.y, b.y) && same(a.z, b.z)
}

/// Handles to tokens: a capture's handle (kind, key) is one object.
#[derive(Default)]
pub struct Ids {
    pub map: HashMap<String, Obj>,
    pub names: HashMap<Obj, String>,
}
impl Ids {
    pub fn key(v: V) -> Option<String> {
        let (kind, key) = v.handle()?;
        let k = match key.str() {
            Some(s) => s.to_string(),
            None => key.opt_num().map(|n| if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("x{:x}", n.to_bits()) }).unwrap_or_default(),
        };
        Some(format!("{kind}:{k}"))
    }
    pub fn named(&mut self, key: String) -> Obj {
        let n = self.map.len() as Obj + 1;
        let id = *self.map.entry(key.clone()).or_insert(n);
        self.names.entry(id).or_insert(key);
        id
    }
    pub fn of(&mut self, v: V) -> Option<Obj> {
        Self::key(v).map(|k| self.named(k))
    }
    pub fn name(&self, o: Obj) -> String {
        self.names.get(&o).cloned().unwrap_or_else(|| format!("#{o}"))
    }
}

pub fn body_of(v: V) -> Option<BodyId> {
    v.handle_num("body").map(|n| n as BodyId)
}

// ---------------------------------------------------------------- views

pub fn class_view(v: V) -> Option<ClassView> {
    if v.is_nullish() {
        return None;
    }
    let bounds = v.get("bounds");
    let bounds = if bounds.len() == 2 {
        let a = bounds.at(0);
        let b = bounds.at(1);
        Some([[num_or_nan(a.at(0)), num_or_nan(a.at(1)), num_or_nan(a.at(2))], [num_or_nan(b.at(0)), num_or_nan(b.at(1)), num_or_nan(b.at(2))]])
    } else {
        None
    };
    let no_enter = v.get("NoEnterVehicles");
    let role_raw = v.get("role").opt_num();
    let unit_type = v.get("UnitType").str().map(String::from).or_else(|| if v.get("UnitType").is_nullish() { v.get("unitType").str().map(String::from) } else { None });
    let moving = v.get("MovingTurnOnly");
    Some(ClassView {
        ai_size_type: opt_str(v.get("AISizeType")),
        forward_speed: opt_num(v.get("ForwardSpeed")),
        strafe_speed: opt_num(v.get("StrafeSpeed")),
        turn_rate: opt_num(v.get("TurnRate")),
        pitch_rate: opt_num(v.get("PitchRate")),
        moving_turn_only: if moving.is_nullish() { None } else { Some(moving.truthy()) },
        max_speed: opt_num(v.get("MaxSpeed")),
        min_speed: opt_num(v.get("MinSpeed")),
        max_turn_speed: opt_num(v.get("MaxTurnSpeed")),
        max_pitch_speed: opt_num(v.get("MaxPitchSpeed")),
        max_yaw_speed: opt_num(v.get("MaxYawSpeed")),
        vehicle_type: v.get("VehicleType").str().map(String::from),
        landing_time: opt_num(v.get("LandingTime")),
        landing_speed: opt_num(v.get("LandingSpeed")),
        landed_height: opt_num(v.get("LandedHeight")),
        bounds,
        no_enter_vehicles: if no_enter.is_nullish() { 0.0 } else { parse_float(no_enter) },
        role: role_of(role_raw, unit_type.as_deref()),
    })
}

pub fn steer_of(ids: &mut Ids, v: V) -> Option<Steer> {
    if v.is_undefined() {
        return None;
    }
    let mut s = Steer::empty();
    let n = |k: &str| if v.has(k) { Some(num_or_nan(v.get(k))) } else { None };
    let b = |k: &str| if v.has(k) { Some(v.get(k).truthy()) } else { None };
    s.moving_turn_at = n("movingTurnAt");
    s.loop_ = b("loop");
    s.loop_at = n("loopAt");
    s.min_speed = n("minSpeed");
    s.min_speed_at = n("minSpeedAt");
    if v.has("orbit") {
        let o = v.get("orbit");
        s.orbit = Some(if o.is_nullish() {
            None
        } else {
            let t = o.get("target");
            let target = if t.is_nullish() { None } else { Some(Ref { obj: ids.of(t.get("unit")).expect("orbit target"), generation: num_or_nan(t.get("generation")) }) };
            Some(Orbit { target, offset: vec3(o.get("offset")), radius: num_or_nan(o.get("radius")), factor: num_or_nan(o.get("factor")) })
        });
    }
    s.land = b("land");
    s.path_override = b("pathOverride");
    s.path_altitude = b("pathAltitude");
    s.throttle = n("throttle");
    s.recovery = n("recovery");
    s.recovery_since = n("recoverySince");
    s.last_recovery = n("lastRecovery");
    s.saved_forward = if v.has("savedForward") { vec3_of(v.get("savedForward")) } else { None };
    s.hold_since = n("holdSince");
    s.holding = b("holding");
    s.turn_yaw = n("turnYaw");
    s.turn_at = n("turnAt");
    s.walk_since = n("walkSince");
    if v.has("stuckAt") {
        let a = v.get("stuckAt");
        s.stuck_at = Some(if a.is_nullish() { None } else { Some(StuckAt { x: num_or_nan(a.get("x")), z: num_or_nan(a.get("z")) }) });
    }
    s.stuck_count = n("stuckCount");
    Some(s)
}

pub fn steer_diff(ids: &Ids, mine: &Option<Steer>, theirs: &Option<Steer>) -> Option<String> {
    match (mine, theirs) {
        (None, None) => None,
        (Some(_), None) => Some("the port has a steering bag, the capture none".into()),
        (None, Some(_)) => Some("the capture has a steering bag, the port none".into()),
        (Some(a), Some(b)) => {
            let nums: [(&str, Option<f64>, Option<f64>); 14] = [
                ("movingTurnAt", a.moving_turn_at, b.moving_turn_at),
                ("loopAt", a.loop_at, b.loop_at),
                ("minSpeed", a.min_speed, b.min_speed),
                ("minSpeedAt", a.min_speed_at, b.min_speed_at),
                ("throttle", a.throttle, b.throttle),
                ("recovery", a.recovery, b.recovery),
                ("recoverySince", a.recovery_since, b.recovery_since),
                ("lastRecovery", a.last_recovery, b.last_recovery),
                ("holdSince", a.hold_since, b.hold_since),
                ("turnYaw", a.turn_yaw, b.turn_yaw),
                ("turnAt", a.turn_at, b.turn_at),
                ("walkSince", a.walk_since, b.walk_since),
                ("stuckCount", a.stuck_count, b.stuck_count),
                ("savedForward.x", a.saved_forward.map(|v| v.x), b.saved_forward.map(|v| v.x)),
            ];
            for (k, x, y) in nums {
                if !same_opt(x, y) {
                    return Some(format!("steer.{k}: the capture has {y:?}, the port {x:?}"));
                }
            }
            let bools: [(&str, Option<bool>, Option<bool>); 5] = [
                ("loop", a.loop_, b.loop_),
                ("land", a.land, b.land),
                ("pathOverride", a.path_override, b.path_override),
                ("pathAltitude", a.path_altitude, b.path_altitude),
                ("holding", a.holding, b.holding),
            ];
            for (k, x, y) in bools {
                if x != y {
                    return Some(format!("steer.{k}: the capture has {y:?}, the port {x:?}"));
                }
            }
            if !same_opt(a.saved_forward.map(|v| v.y), b.saved_forward.map(|v| v.y)) || !same_opt(a.saved_forward.map(|v| v.z), b.saved_forward.map(|v| v.z)) {
                return Some(format!("steer.savedForward: the capture has {:?}, the port {:?}", b.saved_forward, a.saved_forward));
            }
            let orbit_same = match (&a.orbit, &b.orbit) {
                (None, None) | (Some(None), Some(None)) => true,
                (Some(Some(x)), Some(Some(y))) => {
                    x.target.map(|t| (t.obj, t.generation.to_bits())) == y.target.map(|t| (t.obj, t.generation.to_bits()))
                        && same_vec(&x.offset, &y.offset)
                        && same(x.radius, y.radius)
                        && same(x.factor, y.factor)
                }
                _ => false,
            };
            if !orbit_same {
                return Some(format!("steer.orbit: the capture has {:?}, the port {:?} ({})", b.orbit, a.orbit, a.orbit.flatten().and_then(|o| o.target).map(|t| ids.name(t.obj)).unwrap_or_default()));
            }
            let stuck_same = match (&a.stuck_at, &b.stuck_at) {
                (None, None) | (Some(None), Some(None)) => true,
                (Some(Some(x)), Some(Some(y))) => same(x.x, y.x) && same(x.z, y.z),
                _ => false,
            };
            if !stuck_same {
                return Some(format!("steer.stuckAt: the capture has {:?}, the port {:?}", b.stuck_at, a.stuck_at));
            }
            None
        }
    }
}

pub fn intent_of(v: V) -> Option<SeatIntent> {
    if v.is_nullish() {
        return None;
    }
    Some(SeatIntent {
        up: num_or_nan(v.get("up")),
        down: num_or_nan(v.get("down")),
        brake: v.get("brake").truthy(),
        trick: v.get("trick").truthy(),
        immelmann: v.get("immelmann").truthy(),
        boost: v.get("boost").truthy(),
    })
}

pub fn request_of(v: V) -> Request {
    Request {
        valid: v.get("valid").truthy(),
        steering_target: vec3_of(v.get("steeringTarget")).unwrap_or_default(),
        waypoint: vec3_of(v.get("waypoint")).unwrap_or_default(),
        segment_direction: vec3_of(v.get("segmentDirection")).unwrap_or_default(),
        mode: num_or_nan(v.get("mode")),
        lower: num_or_nan(v.get("lower")),
        upper: num_or_nan(v.get("upper")),
        blend: v.get("blend").opt_num(),
        throttle: v.get("throttle").opt_num(),
    }
}

pub fn pattern_state_of(v: V) -> Option<PatternState> {
    if v.is_nullish() {
        return None;
    }
    Some(PatternState {
        toggle: v.get("toggle").truthy(),
        entry: vec3_of(v.get("entry")).unwrap_or_default(),
        destination: vec3_of(v.get("destination")).unwrap_or_default(),
        best: num_or_nan(v.get("best")),
        progress_at: num_or_nan(v.get("progressAt")),
        fallback: v.get("fallback").truthy(),
        loop_: v.get("loop").truthy(),
        inverted: v.get("inverted").truthy(),
        flip_a: v.get("flipA").truthy(),
        flip_b: v.get("flipB").truthy(),
    })
}

