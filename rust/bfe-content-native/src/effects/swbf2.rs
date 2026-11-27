//! Mission-selected visual resources for Battlefront II.
use crate::{
    geometry::{floats, words},
    loader::EnvironmentInstance,
    textures::Library,
};
use bfe_content::{models::ModelMap, Object, Value};
use serde::Deserialize;
use std::{io::ErrorKind, rc::Rc};

/// The mission effect pack layout this build reads; the preparation tool writes the same
/// number (`PACK_VERSION` in `tools/zero_fx2.py`).
const PACK_VERSION: f64 = 10.;

/// A pack written for another layout still loads, but a binding or link it lacks or
/// names differently shows nothing, so the log says so.
fn version_note(file: &str, pack: &Value) -> Option<String> {
    let found = pack.get("version").and_then(Value::as_f64);
    (found != Some(PACK_VERSION)).then(|| {
        let found = found.map_or_else(|| "none".to_string(), |v| v.to_string());
        format!(
            "Mission effect pack {file} has layout version {found}, this build reads version \
             {PACK_VERSION}: some effects may not show until the game files are prepared again"
        )
    })
}

pub(crate) fn compile(library: &Value) -> Result<super::Effects, String> {
    let mut effects = super::Effects::compile(library)?;
    for definition in effects.descriptor["definitions"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        let geometry = &mut definition["geometry"];
        if geometry["blend"] == "blur" && geometry["blurValue"].is_number() {
            // The display host has a screen-copy material. Keep the particle on the CPU quad path.
            geometry["blend"] = "refraction".into();
        }
        if geometry["type"] == "streak" {
            // Adjacent samples define the strip even when both particle velocities are zero.
            geometry["type"] = "trail".into();
            definition["gpu"] = false.into();
        }
    }
    Ok(effects)
}

// Find record boundaries independently of their contents. A balanced record with
// an invalid token can then be rejected without losing the following siblings.
// The native JSON reader still validates every retained value.
struct Field<'a> {
    name: String,
    raw: &'a str,
    nesting: usize,
}

fn quoted_len(bytes: &[u8]) -> Result<usize, &'static str> {
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok(i + 1),
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
    Err("unterminated string in effect table")
}

fn fields(text: &str) -> Result<Vec<Field<'_>>, &'static str> {
    let mut rest = text
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or("expected an effect table")?
        .trim_start();
    let mut fields = Vec::new();
    while !rest.is_empty() {
        if !rest.starts_with('"') {
            return Err("expected an effect record name");
        }
        let key_end = quoted_len(rest.as_bytes())?;
        let name =
            serde_json::from_str(&rest[..key_end]).map_err(|_| "invalid effect record name")?;
        rest = rest[key_end..]
            .trim_start()
            .strip_prefix(':')
            .ok_or("expected : after effect record name")?;
        let bytes = rest.as_bytes();
        let (mut i, mut depth, mut nesting) = (0, 0usize, 0);
        while let Some(&byte) = bytes.get(i) {
            match byte {
                b'"' => {
                    i += quoted_len(&bytes[i..])?;
                    continue;
                }
                b'{' | b'[' => {
                    depth += 1;
                    nesting = nesting.max(depth);
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => break,
                _ => {}
            }
            i += 1;
        }
        if depth != 0 {
            return Err("unterminated container in effect table");
        }
        fields.push(Field {
            name,
            raw: &rest[..i],
            nesting,
        });
        if i == bytes.len() {
            break;
        }
        rest = rest[i + 1..].trim_start();
        if rest.is_empty() {
            return Err("missing effect record after comma");
        }
    }
    Ok(fields)
}

fn object(rows: Vec<(Rc<str>, Value)>) -> Value {
    Value::Object(Rc::new(Object::from_written(rows)))
}

fn parse_pack(text: &str) -> Result<Value, String> {
    let mut pack = Vec::new();
    for Field { name: key, raw, .. } in fields(text)? {
        let value = match key.as_str() {
            "effects" | "extensions" | "beams" | "bindings" | "models" | "textures" | "flashes"
            | "mounts" | "links" | "rounds" => {
                let mut table = Vec::new();
                match fields(raw) {
                    Ok(entries) => {
                        for Field { name, raw, nesting } in entries {
                            if matches!(key.as_str(), "effects" | "extensions") && nesting > 32 {
                                eprintln!(
                                    "Skipping mission effect {key}/{name}: nesting exceeds 32"
                                );
                                continue;
                            }
                            match bfe_content::json::parse(raw) {
                                Ok(value) => table.push((Rc::from(name), value)),
                                Err(e) => eprintln!("Skipping mission effect {key}/{name}: {e}"),
                            }
                        }
                    }
                    Err(e) => eprintln!("Skipping mission effect table {key}: {e}"),
                }
                object(table)
            }
            _ => match bfe_content::json::parse(raw) {
                Ok(value) => value,
                Err(e) => {
                    eprintln!("Skipping mission effect field {key}: {e}");
                    continue;
                }
            },
        };
        pack.push((Rc::from(key), value));
    }
    Ok(object(pack))
}

fn checked(library: &Value) -> Result<super::Effects, String> {
    let compiled = compile(library)?;
    // The display deserializes these same descriptors. Catch a malformed typed field
    // here, while it can still be attributed to an individual group.
    for def in compiled.descriptor["definitions"]
        .as_array()
        .into_iter()
        .flatten()
    {
        bfe_presentation::Definition::deserialize(def).map_err(|e| e.to_string())?;
    }
    Ok(compiled)
}

fn admit(mut pack: Value) -> Result<(Value, super::Effects), String> {
    let empty = object(Vec::new());
    let library = pack.get("effects").unwrap_or(&empty);
    if let Ok(compiled) = checked(library) {
        return Ok((pack, compiled));
    }
    let mut kept = Vec::new();
    for (name, group) in library
        .as_object()
        .ok_or("expected an effect library")?
        .entries()
    {
        let row = (Rc::from(name), group.clone());
        match checked(&object(vec![row.clone()])) {
            Ok(_) => kept.push(row),
            Err(e) => eprintln!("Skipping mission effect {name}: {e}"),
        }
    }
    let groups = object(kept);
    let compiled = checked(&groups)?;
    if let Value::Object(table) = &mut pack {
        Rc::make_mut(table).set(Rc::from("effects"), groups);
    }
    Ok((pack, compiled))
}

pub(crate) fn read(textures: &Library, directory: &str) -> Option<(Value, super::Effects)> {
    let file = format!("{directory}/effects.json");
    let load = || {
        let bytes = match std::fs::read(textures.path(&file)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let pack = parse_pack(&String::from_utf8_lossy(&bytes))?;
        if let Some(note) = version_note(&file, &pack) {
            eprintln!("{note}");
        }
        admit(pack).map(Some)
    };
    load().unwrap_or_else(|e| {
        eprintln!("Skipping unreadable mission effect pack {file}: {e}");
        None
    })
}

pub(crate) fn models(
    textures: &Library,
    directory: &str,
    pack: &Value,
) -> Result<ModelMap, String> {
    let records = pack.get("models");
    if records
        .and_then(Value::as_object)
        .is_none_or(|obj| obj.is_empty())
    {
        return Ok(ModelMap::default());
    }
    let vertices = floats(&textures.read(&format!("{directory}/models.bin"))?)?;
    let indices = words(&textures.read(&format!("{directory}/models_index.bin"))?)?;
    if let Ok(models) =
        bfe_content::models::model_pack_swbf2(records, &vertices, &indices, directory)
    {
        return Ok(models);
    }
    let mut models = ModelMap::default();
    for (name, parts) in records
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|o| o.entries())
    {
        let entry = object(vec![(Rc::from(name), parts.clone())]);
        match bfe_content::models::model_pack_swbf2(Some(&entry), &vertices, &indices, directory) {
            Ok(pack) => {
                for (key, model) in pack.iter() {
                    models.set(key, model.clone());
                }
            }
            Err(e) => eprintln!("Skipping mission effect model {name}: {e}"),
        }
    }
    Ok(models)
}

pub(crate) fn instances(pack: &Value) -> Vec<EnvironmentInstance> {
    let Some(items) = pack.get("instances") else {
        return Vec::new();
    };
    let Some(items) = items.as_array() else {
        eprintln!("Skipping invalid mission effect instance list");
        return Vec::new();
    };
    fn placed(item: &Value) -> Result<EnvironmentInstance, &'static str> {
        let effect = item
            .get("effect")
            .and_then(Value::as_str)
            .ok_or("missing placed effect")?;
        let joint = item
            .get("hardpoint")
            .and_then(Value::as_str)
            .ok_or("missing effect hardpoint")?;
        let matrix = item
            .get("matrix")
            .and_then(Value::as_array)
            .filter(|m| m.len() == 16)
            .ok_or("expected a placed effect matrix")?;
        let mut transform = [0.; 16];
        for (dest, source) in transform.iter_mut().zip(matrix) {
            *dest = source
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("invalid effect matrix element")?;
        }
        Ok(EnvironmentInstance {
            effect: effect.into(),
            hardpoint: joint.into(),
            matrix: transform,
        })
    }
    items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match placed(item) {
            Ok(instance) => Some(instance),
            Err(e) => {
                eprintln!("Skipping mission effect instance {i}: {e}");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_of_another_layout_version_is_named_in_the_log() {
        let pack = |text| parse_pack(text).unwrap();
        assert_eq!(version_note("p/effects.json", &pack(r#"{"version":10,"effects":{}}"#)), None);
        let older = version_note("p/effects.json", &pack(r#"{"version":9,"effects":{}}"#)).unwrap();
        assert!(older.starts_with("Mission effect pack p/effects.json has layout version 9,"), "{older}");
        let unmarked = version_note("p/effects.json", &pack(r#"{"effects":{}}"#)).unwrap();
        assert!(unmarked.contains("layout version none"), "{unmarked}");
    }

    #[test]
    fn unreadable_siblings_leave_the_other_groups_in_order() {
        for depth in [100, 400] {
            let nested = format!("{}0{}", "[".repeat(depth * 4), "]".repeat(depth * 4));
            let text = format!(
                r#"{{"effects":{{"reed":[],"coil":{nested},"fray":[17],"garbled":{{"a":[wrong_token,"]"],"escaped":"a\\"}},"twist":[{{"geometry":{{}}}}],"bud":[]}},"extensions":{{"coil":{nested}}}}}"#,
            );
            let (pack, compiled) = admit(parse_pack(&text).unwrap()).unwrap();
            let names: Vec<_> = compiled
                .handles
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();
            assert_eq!(names, ["reed", "bud"]);
            assert_eq!(
                pack.get("effects")
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                names
            );
            assert!(pack.get("extensions").unwrap().get("coil").is_none());
        }
    }
}
