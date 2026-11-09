//! hullFacts and vehicleClassView (seats.js). The raw target and cooked
//! controls intentionally use different class definitions for one hull.
use super::AiInterner;
use super::{class_facts, optional_string, scalar};
use crate::gen::*;
use crate::world::admit::AdmitError;
use bfe_content::js::{js_max, parse_float_value};
use bfe_content::value::{Object, Value};
use std::rc::Rc;

/// These are the earlier fleet owner's immutable construction receipts.
/// Bounds and imported motion must come from that same constructed hull.
pub struct HullFacts<'a> {
    pub hull: Ref<Vehicle>,
    pub member: Option<Member>,
    pub def: DefId,
    pub definition: &'a Value,
    pub name: &'a str,
    pub base: &'a str,
    pub target_class: &'a Value,
    pub category: f64,
    pub bounds: [f64; 6],
    pub max_health: f64,
    pub speed: f64,
    pub turn_rate: f64,
    pub slot_desc: Option<&'a Value>,
    /// Seat definitions in the fleet's constructed order, with each
    /// seat's settings and its actual number of admitted weapons.
    pub seats: Vec<(&'a Value, u32)>,
}

/// setting in seats.js: authored names take precedence over the folded
/// hash key. Null coalesces to the hash while other false values survive.
pub fn setting<'a>(settings: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    let settings = settings?;
    let named = settings.get(name).filter(|v| !v.is_null());
    named.or_else(|| {
        let hash = name.encode_utf16().fold(0x811c9dc5u32, |h, u| {
            (h ^ ((u as u32 & 255) | 32)).wrapping_mul(0x01000193)
        });
        settings.get(&format!("{hash:08x}"))
    })
}

fn number(value: Option<&Value>, fallback: f64, first: bool) -> f64 {
    let value = if first {
        value
            .map(|v| v.as_array().map_or(Some(v), |a| a.first()))
            .flatten()
    } else {
        value
    };
    let n = parse_float_value(value);
    if n.is_finite() {
        n
    } else {
        fallback
    }
}

fn class_setting<'a>(def: &'a Value, name: &str) -> Option<&'a Value> {
    setting(def.get("settings"), name).or_else(|| {
        def.get("seats")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .skip(1)
            .find_map(|s| setting(s.get("settings"), name))
    })
}

/// vehicleClassView derives only the fields copied by classFacts. Flight
/// tuning fields do not enter those rows; its authored handling still does.
pub fn vehicle_class_view(input: &HullFacts<'_>) -> Value {
    let s = input.definition.get("settings");
    let get = |key, default| number(setting(s, key), default, true);
    let mut o = Object::new();
    let mut put = |name: &str, value| o.set(Rc::from(name), value);
    put("base", Value::str(input.base));
    let vehicle_type = super::string(Some(
        setting(s, "VehicleType")
            .filter(|v| !v.is_null())
            .unwrap_or(&Value::String(Rc::from(""))),
    ))
    .to_lowercase();
    put(
        "VehicleType",
        if vehicle_type.is_empty() {
            Value::Null
        } else {
            Value::str(&vehicle_type)
        },
    );
    let size = setting(s, "AISizeType")
        .filter(|v| !v.is_null())
        .or_else(|| {
            // Array.find tests undefined, so an explicit null at the first
            // seat stops the search and then coalesces to the base default.
            input
                .definition
                .get("seats")
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .find_map(|seat| setting(seat.get("settings"), "AISizeType"))
        })
        .filter(|v| !v.is_null())
        .cloned()
        .or_else(|| match input.base {
            "flyer" => Some(Value::str("FLYER")),
            "hover" => Some(Value::str("HOVER")),
            _ => None,
        });
    if let Some(size) = size {
        put("AISizeType", size);
    }
    put(
        "MaxHealth",
        Value::Number(get("MaxHealth", input.max_health)),
    );
    put(
        "bounds",
        Value::Array(Rc::new(
            input
                .bounds
                .chunks(3)
                .map(|v| Value::Array(Rc::new(v.iter().map(|v| Value::Number(*v)).collect())))
                .collect(),
        )),
    );
    let mut n = |name: &str, value| put(name, Value::Number(value));
    match input.base {
        "flyer" => {
            // flightSpeeds reads classSetting and flightHandling reads
            // body-only turn/pitch, before the same defaults as the page.
            n(
                "MinSpeed",
                number(class_setting(input.definition, "MinSpeed"), 10.0, false),
            );
            n(
                "MaxSpeed",
                number(class_setting(input.definition, "MaxSpeed"), 30.0, false),
            );
            n("StrafeSpeed", 0.0);
            n(
                "TurnRate",
                js_max(number(setting(s, "TurnRate"), 2.0, false), 1.5),
            );
            n("PitchRate", number(setting(s, "PitchRate"), 1.0, false));
            for (name, default) in [
                ("LandingTime", 5.0),
                ("LandingSpeed", 3.0),
                ("LandedHeight", 0.0),
            ] {
                n(
                    name,
                    number(class_setting(input.definition, name), default, false),
                );
            }
        }
        "walker" | "commandwalker" => {
            let turn = get("MaxTurnSpeed", 2.0);
            n("MaxSpeed", get("MaxSpeed", 20.0));
            n("MaxTurnSpeed", turn);
            n(
                "MaxYawSpeed",
                if setting(s, "MaxYawSpeed").is_none() {
                    1.0
                } else {
                    js_max(get("MaxYawSpeed", 1.0), turn)
                },
            );
            n("MaxPitchSpeed", get("MaxPitchSpeed", 0.5));
            n("StrafeSpeed", 0.0);
            n("TurnRate", turn);
            n("PitchRate", get("MaxPitchSpeed", 0.5));
        }
        _ => {
            n("ForwardSpeed", get("ForwardSpeed", input.speed));
            n("StrafeSpeed", get("StrafeSpeed", 0.0));
            n("TurnRate", get("TurnRate", input.turn_rate));
            n("PitchRate", get("PitchRate", 1.0));
            put(
                "MovingTurnOnly",
                Value::Bool(get("MovingTurnOnly", 0.0) != 0.0),
            );
        }
    }
    Value::Object(Rc::new(o))
}

pub fn hull_facts(
    strings: &mut dyn AiInterner,
    input: &HullFacts<'_>,
) -> Result<AiHullDefinition, AdmitError> {
    let kind = match input.base {
        "walker" | "commandwalker" => 2.0,
        "flyer" => 5.0,
        _ => 6.0,
    };
    // The traversal size bypasses vehicleClassView and reads the raw key.
    let traversal = input
        .definition
        .get("settings")
        .filter(|v| !v.is_null())
        .unwrap_or(input.definition)
        .get("AISizeType");
    let mut row = AiHullDefinition {
        hull: input.hull,
        member: input.member,
        kind,
        category: input.category,
        policy: class_facts(
            strings,
            input.def,
            input.definition,
            Some(input.name),
            input.target_class,
        )?,
        traversal_size: scalar(strings, traversal)?,
        max_speed: parse_float_value(
            input
                .definition
                .get("settings")
                .and_then(|s| s.get("MaxSpeed")),
        ),
        forward_speed: parse_float_value(
            input
                .definition
                .get("settings")
                .and_then(|s| s.get("ForwardSpeed")),
        ),
        slot_post: input
            .slot_desc
            .and_then(|s| s.get("post"))
            .is_some_and(Value::truthy),
        decay_time: input
            .slot_desc
            .and_then(|s| s.get("decayTime"))
            .filter(|v| !v.is_null())
            .map(|v| bfe_content::js::to_number(Some(v))),
        seats: Vec::new(),
    };
    let cooked = vehicle_class_view(input);
    for (index, (settings, weapons)) in input.seats.iter().enumerate() {
        let mut definition = cooked.as_object().cloned().unwrap_or_default();
        if index != 0 {
            let rate = number(
                setting(Some(settings), "TurnRate"),
                number(
                    setting(input.definition.get("settings"), "TurnRate"),
                    input.turn_rate,
                    true,
                ),
                true,
            );
            for (name, value) in [
                ("MaxSpeed", 0.0),
                ("ForwardSpeed", 0.0),
                ("StrafeSpeed", 0.0),
                ("TurnRate", rate),
                ("PitchRate", rate),
            ] {
                definition.set(Rc::from(name), Value::Number(value));
            }
        }
        row.seats.push(AiSeatDefinition {
            policy: class_facts(
                strings,
                input.def,
                input.definition,
                Some(input.name),
                &Value::Object(Rc::new(definition)),
            )?,
            kind: if index == 0 { kind } else { 7.0 },
            weapons: *weapons,
            yaw: optional_string(strings, settings.get("YawLimits")),
            pitch: optional_string(strings, settings.get("PitchLimits")),
        });
    }
    Ok(row)
}
