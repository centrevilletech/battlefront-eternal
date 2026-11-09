//! postAI/attachPostAI in post-data.js. Joining by authored world name
//! matters for unnamed posts whose runtime registry name is synthesized.
use super::{error, present, string};
use crate::world::admit::AdmitError;
use bfe_content::js::{js_atan2, js_hypot, to_number};
use bfe_content::regions::build_region;
use bfe_content::value::{Object, Value};
use std::rc::Rc;

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(Rc::new(Object::from_written(
        entries.into_iter().map(|(k, v)| (Rc::from(k), v)).collect(),
    )))
}

fn array(values: impl IntoIterator<Item = f64>) -> Value {
    Value::Array(Rc::new(values.into_iter().map(Value::Number).collect()))
}

fn capture(region: &Value) -> Result<Value, AdmitError> {
    let built = build_region(region);
    // captureOf uses the authored center before the matrix translation.
    // buildRegion's half includes the scale of that same matrix.
    let center = if let Some(center) = region.get("center").filter(|v| v.truthy()) {
        center.clone()
    } else {
        let matrix = region
            .get("matrix")
            .and_then(Value::as_array)
            .ok_or_else(|| error("AI capture has no center or matrix"))?;
        array((12..15).map(|i| to_number(matrix.get(i))))
    };
    Ok(object(vec![
        ("center", center),
        ("half", array(built.half)),
    ]))
}

/// The attached record is a value, so the exact same postFacts builder
/// handles fallback map metadata and authored AI mission records.
pub fn attach_post_ai(
    desc: &Value,
    runtime_name: Option<&Value>,
    position: [f64; 3],
    index: usize,
    mission: Option<&Value>,
    spawns: Option<&Value>,
) -> Result<Value, AdmitError> {
    let records = mission
        .and_then(|m| m.get("posts"))
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    let world_name = match desc.get("name") {
        Some(v @ Value::String(_)) => Some(v),
        _ => runtime_name,
    };
    let record = if let Some(name) = world_name.filter(|v| v.truthy()) {
        let name = string(Some(name)).to_lowercase();
        // Map's repeated names keep the last authored record.
        records.iter().rev().find(|r| {
            r.get("name")
                .filter(|v| v.truthy())
                .is_some_and(|v| string(Some(v)).to_lowercase() == name)
        })
    } else {
        records.iter().find(|r| {
            !r.get("name").is_some_and(Value::truthy)
                && r.get("position").filter(|p| p.truthy()).is_some_and(|p| {
                    js_hypot(&std::array::from_fn::<_, 3, _>(|i| {
                        to_number(p.at(i)) - position[i]
                    })) < 0.01
                })
        })
    };
    if let Some(record) = record {
        let path = |key| {
            record
                .get(key)
                .filter(|v| v.truthy())
                .map(|p| {
                    object(vec![
                        ("name", p.get("name").cloned().unwrap_or(Value::Null)),
                        (
                            "present",
                            Value::Bool(p.get("present").is_some_and(Value::truthy)),
                        ),
                    ])
                })
                .unwrap_or(Value::Null)
        };
        let integer = |key| Value::Number(jsrt::to_int32(to_number(record.get(key))) as f64);
        let mut ai = Object::new();
        let mut put = |key: &str, v| ai.set(Rc::from(key), v);
        // Missing numeric metadata must stay missing, to encode NaN rather
        // than null's numeric zero in postFacts.
        for key in ["order", "valueBleed"] {
            if let Some(v) = record.get(key) {
                put(key, v.clone());
            }
        }
        for (field, key) in [("atk", "atk"), ("def", "def")] {
            let v = record
                .get("values")
                .and_then(|v| v.get(key))
                .ok_or_else(|| error("AI post has no class weights"))?;
            put(field, v.clone());
        }
        put("bans", integer("bans"));
        put("allyCount", integer("allyCount"));
        put("classSpawnPoints", integer("classSpawnPoints"));
        put("spawnPath", path("spawnPath"));
        put("allyPath", path("allyPath"));
        put(
            "radius",
            record
                .get("radius")
                .filter(|v| v.truthy())
                .cloned()
                .unwrap_or(Value::Number(0.0)),
        );
        put(
            "yaw",
            present(record.get("yaw"))
                .cloned()
                .unwrap_or(Value::Number(0.0)),
        );
        let region = record
            .get("captureRegion")
            .filter(|r| r.get("present").is_some_and(Value::truthy));
        put(
            "capture",
            region.map(capture).transpose()?.unwrap_or(Value::Null),
        );
        return Ok(Value::Object(Rc::new(ai)));
    }
    let spawn = desc
        .get("spawnPath")
        .filter(|v| v.truthy())
        .map(|name| {
            let mut points = desc
                .get("spawnPoints")
                .and_then(Value::as_array)
                .unwrap_or(&[]);
            if points.is_empty() {
                let key = string(Some(name)).to_lowercase();
                // Object.fromEntries also keeps the last spelling if case
                // folding maps more than one authored key to the same name.
                points = spawns
                    .and_then(Value::as_object)
                    .and_then(|o| o.entries().filter(|(k, _)| k.to_lowercase() == key).last())
                    .and_then(|(_, p)| p.as_array())
                    .unwrap_or(&[]);
            }
            object(vec![
                ("name", name.clone()),
                ("present", Value::Bool(!points.is_empty())),
            ])
        })
        .unwrap_or(Value::Null);
    let yaw = desc
        .get("matrix")
        .filter(|v| v.truthy())
        .map(|m| js_atan2(to_number(m.at(8)), to_number(m.at(10))))
        .unwrap_or(0.0);
    Ok(object(vec![
        ("order", Value::Number(index as f64)),
        ("atk", array([100.0; 6])),
        ("def", array([100.0; 6])),
        ("bans", Value::Number(0.0)),
        (
            "valueBleed",
            present(desc.get("valueBleed"))
                .cloned()
                .unwrap_or(Value::Number(100.0)),
        ),
        ("spawnPath", spawn),
        ("allyPath", Value::Null),
        ("allyCount", Value::Number(0.0)),
        ("classSpawnPoints", Value::Number(0.0)),
        (
            "capture",
            desc.get("capture")
                .filter(|v| v.truthy())
                .map(capture)
                .transpose()?
                .unwrap_or(Value::Null),
        ),
        ("radius", Value::Number(0.0)),
        ("yaw", Value::Number(yaw)),
    ]))
}
