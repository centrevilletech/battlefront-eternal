//! PageCameras' immutable camera_init and camera_add_definition payloads.
//! JSON bytes are retained, since serde_json's number formatting differs
//! from JSON.stringify and would change the recorded scratch input.
use bfe_content::{battle::Battle, js::{json_stringify, number_to_string, to_number}, value::{Object, Value}};
use bfe_presentation::layout::{camera_limits, camera_motion};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct Payloads {
    pub init: Vec<u8>,
    pub definition: Vec<u8>,
}

/// game.js's simulation camera has no cosmetic motions or spawn shots.
/// The display owner uses those common tables independently of this owner.
pub fn aim(battle: &Battle) -> Payloads { payloads(&battle.soldier_def, None, None) }
pub fn display(battle: &Battle) -> Payloads {
    payloads(&battle.soldier_def, battle.common.get("cameraMotions"), battle.world.get("spawnScreen").and_then(|s| s.get("cameraShots")))
}

// Motion indices come from the generated presentation contract. Their
// authored names are property lookups, never object-entry iteration order.
// Preserve each selected object whole, including its own property order.
pub fn payloads(def: &Value, motions: Option<&Value>, shots: Option<&Value>) -> Payloads {
    let keys = (0..camera_limits::MOTIONS).map(|index| {
        let name = match index as u32 {
            camera_motion::ROLL_LEFT => "RollLeft", camera_motion::ROLL_RIGHT => "RollRight",
            camera_motion::STAND_TO_PRONE => "StandToProne", camera_motion::CROUCH_TO_PRONE => "CrouchToProne",
            camera_motion::PRONE_TO_STAND => "ProneToStand", camera_motion::CROUCH_TO_STAND => "CrouchToStand",
            camera_motion::LAND => "Land", _ => "undefined",
        };
        motions.and_then(|m| m.get(name)).filter(|v| v.truthy()).cloned().unwrap_or_else(|| object(vec![
            ("Vec", array(vec![])), ("Slope", array(vec![])), ("Ore", array(vec![])), ("RSlope", array(vec![])), ("Time", array(vec![])),
        ]))
    }).collect();
    // JSON.stringify retains insertion order for these named properties.
    // A sorted JSON map would alter the byte payload despite equal content.
    let init = object(vec![("definitions", array(vec![])), ("motions", array(keys)),
        ("spawnShots", shots.filter(|s| s.truthy()).cloned().unwrap_or_else(|| array(vec![])))]);
    Payloads { init: encode(&init), definition: encode(&definition(def)) }
}

/// Ports definition(def). Zoom falls back to that posture before STAND;
/// an empty section object is truthy and therefore suppresses that fallback.
/// Multiplication order is authored degrees times the already rounded DEG.
pub fn definition(def: &Value) -> Value {
    let source = def.get("cameras");
    let mut sections = Vec::with_capacity(6);
    let deg = std::f64::consts::PI / 180.0;
    // Six sections are emitted even when all use the same STAND source.
    // Their indices belong to the camera state machine, so do not deduplicate.
    for zoom in [false, true] {
        for name in ["STAND", "CROUCH", "PRONE"] {
            let held = if zoom { source.and_then(|s| s.get(&format!("{name}ZOOM"))).filter(|v| v.truthy()) } else { None };
            let section = held.or_else(|| source.and_then(|s| s.get(name)).filter(|v| v.truthy()))
                .or_else(|| source.and_then(|s| s.get("STAND")).filter(|v| v.truthy()));
            // Vectors use || while TiltValue uses ??: an empty vector is
            // truthy and a zero tilt is an authored value, not a missing one.
            let field = |name| section.and_then(|s| s.get(name)).filter(|v| v.truthy()).cloned();
            let eye = field("EyePointOffset").or_else(|| field("TrackCenter")).unwrap_or_else(|| vector([0.0, 1.8, 0.0]));
            let center = field("TrackCenter").or_else(|| field("EyePointOffset")).unwrap_or_else(|| vector([0.0, 1.8, 0.0]));
            let offset = field("TrackOffset").unwrap_or_else(|| vector([0.0, 0.0, 3.5]));
            let tilt = number(section.and_then(|s| s.get("TiltValue")), 5.0) * deg;
            sections.push(object(vec![("eye", eye), ("center", center), ("offset", offset), ("tilt", Value::Number(tilt))]));
        }
    }
    object(vec![("third", Value::Number(number(def.get("ThirdPersonFOV"), 65.0) * deg)),
        ("first", Value::Number(number(def.get("FirstPersonFOV"), 70.0) * deg)), ("sections", array(sections))])
}
// The nullish defaults run before numeric multiplication in JS. This
// helper keeps zero and false, and lets conversion supply NaN when needed.
fn number(value: Option<&Value>, default: f64) -> f64 {
    value.filter(|v| !matches!(v, Value::Null)).map(|v| to_number(Some(v))).unwrap_or(default)
}
fn vector(value: [f64; 3]) -> Value { array(value.into_iter().map(Value::Number).collect()) }
fn array(value: Vec<Value>) -> Value { Value::Array(Rc::new(value)) }
fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(Rc::new(Object::from_written(entries.into_iter().map(|(k, v)| (Rc::from(k), v)).collect())))
}

/// Ports encode's negative-zero replacer before JSON.stringify. Finite
/// numbers use V8's decimal spelling; nonfinite numbers serialize as null.
/// Literal sentinel strings are replaced too, as replaceAll does in JS.
pub fn encode(value: &Value) -> Vec<u8> {
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Number(v) if *v == 0.0 && v.is_sign_negative() => out.push_str("-0.0"),
            Value::Number(v) if !v.is_finite() => out.push_str("null"),
            Value::Number(v) => out.push_str(&number_to_string(*v)),
            Value::Array(values) => {
                out.push('[');
                for (i, v) in values.iter().enumerate() { if i != 0 { out.push(','); } write(v, out); }
                out.push(']');
            }
            Value::Object(values) => {
                out.push('{');
                for (i, (key, v)) in values.entries().enumerate() {
                    if i != 0 { out.push(','); }
                    out.push_str(&json_stringify(Some(&Value::str(key)))); out.push(':'); write(v, out);
                }
                out.push('}');
            }
            _ => out.push_str(&json_stringify(Some(value))),
        }
    }
    let mut out = String::new(); write(value, &mut out);
    out.replace("\"__camera_negative_zero__\"", "-0.0").into_bytes()
}

/// Camera construction needs an interleave: copy init bytes to scratch,
/// dispatch camera_init, then supply its returned handle to the definition.
/// The native sequence binds scratch pointers itself. A comparison supplies
/// each recorded sim_scratch return before asking for the corresponding call.
pub fn build_init(cx: &mut super::Cx) -> Result<super::Calls, super::super::AdmitError> {
    let bytes = aim(cx.battle).init;
    // Scratch is an allocation result, not a content-derived address. The
    // native body reads the call's scratch bytes and never the address
    // word; live construction, with no module memory, passes zero there.
    let pointer = cx.returned("sim_scratch").unwrap_or(0.0);
    Ok(vec![(super::super::Stage::Cameras, "camera_init", super::Call {
        args: vec![pointer, bytes.len() as f64], scratch: bytes, ..Default::default()
    })])
}

pub fn build_definition(cx: &mut super::Cx) -> Result<super::Calls, super::super::AdmitError> {
    let bytes = aim(cx.battle).definition;
    let stage = super::super::Stage::Cameras;
    // A display owner may already occupy a camera slot. Use the actual
    // creation return rather than assuming the authority is owner one.
    let handle = cx.returned("camera_init").ok_or(super::super::AdmitError::Unavailable(stage))?;
    let pointer = cx.returned("sim_scratch").unwrap_or(0.0);
    Ok(vec![(stage, "camera_add_definition", super::Call {
        args: vec![handle, pointer, bytes.len() as f64], scratch: bytes, ..Default::default()
    })])
}

/// A recorded comparison may pre-feed the complete camera construction.
/// Locate each scratch result within its call interval: a later owner may
/// have allocated scratch again, so the table's latest pointer is insufficient.
/// Live construction uses build_init/build_definition across the return edge.
pub fn build(cx: &mut super::Cx) -> Result<super::Calls, super::super::AdmitError> {
    let stage = super::super::Stage::Cameras;
    let missing = || super::super::AdmitError::Unavailable(stage);
    let index = cx.returned.iter().rposition(|(name, _)| *name == "camera_init").ok_or_else(missing)?;
    let init_pointer = cx.returned[..index].iter().rev().find(|(name, _)| *name == "sim_scratch").map(|(_, value)| *value).ok_or_else(missing)?;
    let end = cx.returned[index + 1..].iter().position(|(name, _)| *name == "camera_add_definition").map_or(cx.returned.len(), |offset| index + 1 + offset);
    let definition_pointer = cx.returned[index + 1..end].iter().rev().find(|(name, _)| *name == "sim_scratch").map(|(_, value)| *value).ok_or_else(missing)?;
    let payloads = aim(cx.battle);
    Ok(vec![(stage, "camera_init", super::Call { args: vec![init_pointer, payloads.init.len() as f64], scratch: payloads.init, ..Default::default() }),
        (stage, "camera_add_definition", super::Call { args: vec![cx.returned[index].1, definition_pointer, payloads.definition.len() as f64], scratch: payloads.definition, ..Default::default() })])
}
