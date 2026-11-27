//! Object iteration that survives 2005 admission. Named record fields still
//! use the canonical JSON encoding. Arrays already carry their own order.
use super::{digest, serialize};
use bfe_content::{battle::swbf2_side_banks, value::Object, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

pub(super) fn keys(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Object::keys)
        .collect()
}

// Exact names are independent. Only siblings colliding in a case-insensitive
// lookup can exchange places and change its first/last match.
fn folded_ties(value: Option<&Value>) -> Vec<Vec<&str>> {
    let mut groups = BTreeMap::<String, Vec<&str>>::new();
    for key in keys(value) {
        groups
            .entry(key.to_ascii_lowercase())
            .or_default()
            .push(key);
    }
    groups.into_values().filter(|keys| keys.len() > 1).collect()
}

pub(super) fn map(
    world: &Value,
    mission: Option<&Value>,
    active_layers: &[String],
) -> serde_json::Value {
    let layers = world.get("layers");
    let sources =
        std::iter::once(world).chain(active_layers.iter().filter_map(|name| layers?.get(name)));
    let mut spawns = Object::new();
    let mut posts = Vec::new();
    let mut group_ties = Vec::new();
    for source in sources {
        if let Some(paths) = source.get("spawns").and_then(Value::as_object) {
            for (name, points) in paths.entries() {
                spawns.set(Rc::from(name), points.clone());
            }
        }
        for post in source
            .get("commandPosts")
            .and_then(Value::as_array)
            .unwrap_or(&[])
        {
            if let Some(path) = post
                .get("spawnPath")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                posts.push(path);
            }
        }
        group_ties.push(folded_ties(
            source.get("worldAnimations").and_then(|a| a.get("groups")),
        ));
    }
    // Map admission retains only the command posts' paths, in post order.
    // Without posts, spawn selection uses the initial composed path.
    let spawn_order = if posts.is_empty() {
        spawns.keys().next().into_iter().collect::<Vec<_>>()
    } else {
        posts
            .into_iter()
            .filter_map(|path| {
                if spawns.has(path) {
                    Some(path)
                } else {
                    spawns.keys().find(|key| key.eq_ignore_ascii_case(path))
                }
            })
            .collect()
    };
    serde_json::json!([
        keys(mission.and_then(|m| m.get("sides"))),
        active_layers,
        spawn_order,
        group_ties
    ])
}

pub(super) fn units(
    side: &str,
    items: Option<&Value>,
    units: &[(String, Value)],
    common_banks: &[String],
    side_meta: &Value,
) -> Result<serde_json::Value, String> {
    let bank_keys: Vec<_> = keys(side_meta.get("banks"))
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut selections = Vec::new();
    for (name, unit) in units {
        let base = unit.get("base").and_then(Value::as_str).unwrap_or("");
        let (banks, aliases) =
            swbf2_side_banks(side, name, unit, base, common_banks, side_meta, &bank_keys)?;
        let aliases: BTreeMap<_, _> = aliases.into_iter().collect();
        // An explicit null can win over an item promoted from ordnance.
        let items = unit
            .get("weapons")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(|w| w.get("item"))
            .map(|v| serialize(v).map(|b| digest(&b)))
            .collect::<Result<Vec<_>, _>>()?;
        selections.push(serde_json::json!([name, items, banks, aliases]));
    }
    Ok(serde_json::json!([keys(items), selections]))
}

pub(super) fn banks(file: &Value, common: bool) -> serde_json::Value {
    let banks = file.get("banks");
    let human: Vec<_> = keys(banks)
        .into_iter()
        .filter(|k| common && k.starts_with("human_"))
        .collect();
    serde_json::json!([human, folded_ties(banks)])
}

pub(super) fn props(file: &Value) -> serde_json::Value {
    serde_json::json!(["models", "collision", "skeletons"].map(|key| folded_ties(file.get(key))))
}

pub(super) fn hash(value: &Value, selection: &serde_json::Value) -> Result<String, String> {
    let mut bytes = serialize(value)?;
    bytes.push(0); // Not a byte in canonical JSON.
    serde_json::to_writer(&mut bytes, selection).map_err(|e| e.to_string())?;
    Ok(digest(&bytes))
}
