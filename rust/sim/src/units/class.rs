//! A body's numbers from its class (its ODF definition in a side's
//! units.json), as the Soldier and Droideka constructors and units.js
//! buildBody take them: what a soldier moves and jumps at, his jetpack, what
//! landing too hard does to him (src/collision-damage.js collisionRule), a
//! droideka's turn, ball and eye; the mission's ceiling; a soldier's get-up
//! and dive times from his clips.

use crate::gen::{CollisionRule, Droideka, Jet, Posture, Soldier};
use bfe_content::js::{is_js_space, number_to_string, string_to_number};
use bfe_content::Value;

/// A soldier's collision damage rule by default (collision-damage.js
/// SOLDIER_COLLISION): 10 3 0 and 1 0 0.
pub const SOLDIER_COLLISION: CollisionRule = CollisionRule { threshold: [10.0, 3.0, 0.0], scale: [1.0, 0.0, 0.0] };
/// A droideka's (DROIDEKA_COLLISION): 7.5 2.5 0 and 1 0 0.
pub const DROIDEKA_COLLISION: CollisionRule = CollisionRule { threshold: [7.5, 2.5, 0.0], scale: [1.0, 0.0, 0.0] };

/// `String(value)` of a definition's value.
fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.to_string(),
        Value::Number(n) => number_to_string(*n),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Array(_) | Value::Object(_) => "[object Object]".into(),
    }
}

/// triple(value, defaults) (collision-damage.js): what the ODF sets over the
/// defaults, as the original game reads "%f %f %f" into fields that keep
/// their defaults. An array's numbers count as they are (anything else in it
/// is no number); a string is split at white space and each part read as
/// Number reads it.
fn triple(value: Option<&Value>, defaults: [f64; 3]) -> [f64; 3] {
    let values: Vec<f64> = match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(|v| if let Value::Number(n) = v { *n } else { f64::NAN }).collect(),
        Some(other) => {
            let s = js_string(other);
            let t = s.trim_matches(is_js_space);
            // "".split(/\s+/) is [""], which Number reads as 0.
            if t.is_empty() {
                vec![0.0]
            } else {
                t.split(is_js_space).filter(|p| !p.is_empty()).map(string_to_number).collect()
            }
        }
    };
    let mut out = defaults;
    for (i, d) in defaults.iter().enumerate() {
        out[i] = match values.get(i) {
            Some(v) if v.is_finite() => *v,
            _ => *d,
        };
    }
    out
}

/// collisionRule(def, defaults).
pub fn collision_rule(def: &Value, defaults: &CollisionRule) -> CollisionRule {
    CollisionRule { threshold: triple(def.get("CollisionThreshold"), defaults.threshold), scale: triple(def.get("CollisionScale"), defaults.scale) }
}

/// An array's first two numbers as `[low, high] = array` reads them (NaN
/// for what is missing or no number).
fn pair(v: &Value) -> [f64; 2] {
    let at = |i: usize| v.as_array().and_then(|a| a.get(i)).and_then(|x| x.as_f64()).unwrap_or(f64::NAN);
    [at(0), at(1)]
}

/// `def[key] ?? fallback` for a number.
fn num_or(def: &Value, key: &str, fallback: f64) -> f64 {
    match def.get(key) {
        None | Some(Value::Null) => fallback,
        Some(Value::Number(n)) => *n,
        Some(other) => string_to_number(&js_string(other)),
    }
}

/// The soldier class's numbers (soldier.js CLASS_DEFAULTS and the jetpack's).
const CLASS_DEFAULTS: [(&str, f64); 6] = [("MaxSpeed", 9.0), ("MaxStrafeSpeed", 7.0), ("MaxJumpSpeed", 8.0), ("MaxTurnSpeed", 2.0), ("MaxPitchSpeed", 0.5), ("Acceleraton", 70.0)];
const JET_DEFAULTS: [(&str, f64); 6] = [("JetJump", 7.0), ("JetPush", 14.0), ("JetFuelRechargeRate", 0.0), ("JetFuelCost", 0.2), ("JetFuelInitialCost", 0.0), ("JetFuelMinBorder", 0.1)];

/// A new soldier's body for a class (the Soldier constructor and buildBody):
/// `max_fly_height` the mission's ceiling, `getup` and `dive` his get-up and
/// dive times.
pub fn soldier_body(def: &Value, max_fly_height: f64, getup: f64, dive: f64) -> Soldier {
    let value = |key: &str| -> f64 {
        let d = CLASS_DEFAULTS.iter().chain(JET_DEFAULTS.iter()).find(|(k, _)| *k == key).map(|(_, v)| *v).unwrap_or(f64::NAN);
        num_or(def, key, d)
    };
    // `def.PitchLimits ?? [-0.8, 1.1]`, read as [low, high].
    let pitch_limits = match def.get("PitchLimits") {
        None | Some(Value::Null) => [-0.8, 1.1],
        Some(v) => pair(v),
    };
    let jet_type = def.get("JetType").filter(|v| v.truthy()).map(js_string).unwrap_or_default();
    Soldier {
        max_speed: value("MaxSpeed"),
        max_strafe: value("MaxStrafeSpeed"),
        max_jump: value("MaxJumpSpeed"),
        max_turn: value("MaxTurnSpeed"),
        max_pitch: value("MaxPitchSpeed"),
        accel: value("Acceleraton"),
        pitch_limits,
        jet: Jet {
            jump: value("JetJump"),
            push: value("JetPush"),
            recharge: value("JetFuelRechargeRate"),
            cost: value("JetFuelCost"),
            initial: value("JetFuelInitialCost"),
            min_border: value("JetFuelMinBorder"),
            hover: jet_type.to_lowercase() == "hover",
        },
        fuel: 0.0,
        collision_rule: collision_rule(def, &SOLDIER_COLLISION),
        collision_damage: 0.0,
        max_fly_height,
        getup_time: getup,
        dive_time: dive,
        normal: [0.0, 1.0, 0.0],
        posture: Posture::Stand,
        dive_from: Posture::Stand,
        tick_length: 1.0 / 60.0,
        mover: -1.0,
        ..Default::default()
    }
}

/// A new droideka's numbers for a class (the Droideka constructor): its
/// turn, pitch range, clips' speed, turn threshold, ball and eye, its
/// collision rule, DummyRoot's index (`root_index`, -1 without one).
pub fn droideka_body(def: &Value, root_index: f64, max_fly_height: f64) -> Droideka {
    // `(def.PitchLimits || [-28.6, 28.6])`, in degrees.
    let pitch = match def.get("PitchLimits").filter(|v| v.truthy()) {
        None => [-28.6, 28.6],
        Some(v) => pair(v),
    };
    let eye = def
        .get("cameras")
        .and_then(|c| c.get("STAND"))
        .and_then(|s| s.get("EyePointOffset"))
        .filter(|v| v.truthy())
        .and_then(|v| v.as_array())
        .and_then(|a| a.get(1))
        .and_then(|v| v.as_f64())
        .unwrap_or(1.8);
    Droideka {
        max_turn: num_or(def, "MaxTurnSpeed", 2.0),
        pitch_limits: [(pitch[0] * std::f64::consts::PI) / 180.0, (pitch[1] * std::f64::consts::PI) / 180.0],
        speed: num_or(def, "AnimationSpeed", 1.0),
        turn_threshold: num_or(def, "TurnThreshold", 0.5),
        ball_radius: num_or(def, "BallRadius", 0.65),
        max_spin: num_or(def, "BallMoveSpeed", 10.0) * num_or(def, "BallSpinScale", 2.0),
        max_ball_slope: v8math::cos((num_or(def, "MaxBallAngle", 35.0) * std::f64::consts::PI) / 180.0),
        eye_height: eye,
        root_index,
        max_fly_height,
        collision_rule: collision_rule(def, &DROIDEKA_COLLISION),
        grounded: true,
        ..Default::default()
    }
}
