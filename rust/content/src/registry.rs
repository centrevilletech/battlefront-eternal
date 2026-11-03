//! The ids the wire gives content: src/runtime/network-entity-registry.js
//! (registryContent, createEntityRegistry's class, weapon and post tables)
//! and src/runtime/network-combat-registry.js (createCombatRegistry: every
//! combat definition, by the path a sorted walk of the battle's content
//! first meets it on). A client and its server each make these from their
//! own content and compare them before reading any numeric field.

use crate::content::Content;
use crate::mission::utf16_cmp;
use crate::value::{Object, Value};
use std::collections::HashSet;
use std::rc::Rc;

/// game.js `data` (3386) as registryContent picks from it: the battle's
/// common, effects, props, vehicles and world, and its fielded sides (by
/// sorted name) and their effects. A side is game.js's `sides[name]` record
/// ({ dir, units, items, texture, firePoints }; `texture` is a function,
/// which the walk never enters, so it is left out).
pub fn registry_content(c: &Content) -> Value {
    let b = &c.battle;
    let fielded: Vec<String> = match b.world.get("mission").and_then(|m| m.get("sides")) {
        Some(Value::Array(list)) if !list.is_empty() => list.iter().map(|v| crate::odf::to_text(Some(v))).collect(),
        _ => b.sides.iter().map(|s| s.name.clone()).collect(),
    };
    let mut names: Vec<String> = Vec::new();
    for name in fielded {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.sort_by(|a, x| utf16_cmp(a, x));
    let mut sides: Vec<(Rc<str>, Value)> = Vec::new();
    let mut side_fx: Vec<(Rc<str>, Value)> = Vec::new();
    for name in &names {
        let Some(side) = b.side(name) else { continue };
        let record = Object::from_written(vec![
            (Rc::from("dir"), Value::str(&side.dir)),
            (Rc::from("units"), side.units.clone()),
            (Rc::from("items"), side.items.clone()),
            (Rc::from("firePoints"), side.fire_points.clone()),
        ]);
        sides.push((Rc::from(name.as_str()), Value::Object(Rc::new(record))));
        // game.js: sideFx holds a side's effects.json when it has one.
        let fx = if side.name == b.side { b.side_fx.clone() } else { side.fx.clone() };
        if let Some(fx) = fx.filter(|f| f.truthy()) {
            side_fx.push((Rc::from(name.as_str()), fx));
        }
    }
    let or_undefined = |v: &Option<Value>| v.clone();
    let mut top: Vec<(Rc<str>, Value)> = vec![(Rc::from("common"), b.common.clone()), (Rc::from("effects"), b.effects.clone())];
    // props and vehicles are null in game.js when absent (the walk skips
    // them either way).
    if let Some(p) = or_undefined(&b.props) {
        top.push((Rc::from("props"), p));
    }
    if let Some(v) = or_undefined(&b.vehicle_def) {
        top.push((Rc::from("vehicles"), v));
    }
    top.push((Rc::from("world"), b.world.clone()));
    top.push((Rc::from("sides"), Value::Object(Rc::new(Object::from_written(sides)))));
    top.push((Rc::from("sideFx"), Value::Object(Rc::new(Object::from_written(side_fx)))));
    Value::Object(Rc::new(Object::from_written(top)))
}

const COMBAT_BASES: [&str; 11] = ["explosion", "droid", "mine", "powerupitem", "grenade", "sticky", "missile", "shell", "bullet", "beam", "fatray"];

fn finite_number(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(n)) if n.is_finite())
}

/// Whether the walk registers a value (network-combat-registry.js 11-12).
fn is_combat_definition(o: &Object) -> bool {
    if let Some(Value::String(base)) = o.get("base") {
        if COMBAT_BASES.contains(&&**base) {
            return true;
        }
    }
    if finite_number(o.get("Velocity")) && finite_number(o.get("LifeSpan")) {
        return true;
    }
    matches!(o.get("Effect"), Some(Value::String(_)))
        && (finite_number(o.get("Damage")) || finite_number(o.get("DamageRadiusOuter")) || finite_number(o.get("LightRadius")))
}

/// createCombatRegistry(content): each definition's path (as
/// JSON.stringify writes it) and the definition, by id - 1.
pub fn combat_registry(content: &Value) -> Result<Vec<(String, Value)>, String> {
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    fn identity(v: &Value) -> Option<usize> {
        match v {
            Value::Object(o) => Some(Rc::as_ptr(o) as *const u8 as usize),
            Value::Array(a) => Some(Rc::as_ptr(a) as *const u8 as usize),
            _ => None,
        }
    }
    fn visit(value: &Value, path: &mut Vec<String>, seen: &mut HashSet<usize>, out: &mut Vec<(String, Value)>) -> Result<(), String> {
        // Only objects and arrays (a falsy or primitive value stops here).
        let Some(id) = identity(value) else { return Ok(()) };
        if seen.contains(&id) {
            return Ok(());
        }
        if path.len() > 32 {
            return Err("combat registry content depth exceeded".into());
        }
        seen.insert(id);
        let keys: Vec<String> = match value {
            Value::Array(items) => {
                // An array of primitives is data, not definitions (null is
                // an object to typeof, so an array that starts with one goes on).
                if let Some(first) = items.first() {
                    if !matches!(first, Value::Null | Value::Object(_) | Value::Array(_)) {
                        return Ok(());
                    }
                }
                (0..items.len()).map(|i| i.to_string()).collect()
            }
            Value::Object(o) => {
                if is_combat_definition(o) {
                    if out.len() >= 65535 {
                        return Err("combat definition registry capacity exceeded".into());
                    }
                    let mut text = String::from("[");
                    for (i, p) in path.iter().enumerate() {
                        if i > 0 {
                            text.push(',');
                        }
                        crate::dump::string(&mut text, p);
                    }
                    text.push(']');
                    out.push((text, value.clone()));
                }
                o.keys().map(str::to_string).collect()
            }
            _ => unreachable!(),
        };
        // Object.keys(value).sort(): UTF-16 order (array indices as text).
        let mut keys = keys;
        keys.sort_by(|a, b| utf16_cmp(a, b));
        for key in keys {
            let child = match value {
                Value::Array(items) => items[key.parse::<usize>().expect("an index")].clone(),
                Value::Object(o) => o.get(&key).cloned().expect("an own key"),
                _ => unreachable!(),
            };
            path.push(key);
            visit(&child, path, seen, out)?;
            path.pop();
        }
        Ok(())
    }
    visit(content, &mut Vec::new(), &mut seen, &mut out)?;
    Ok(out)
}

/// network-entity-registry.js name(): a non-empty string of at most 128
/// UTF-16 units.
fn valid_name(v: Option<&Value>) -> Result<String, String> {
    match v {
        Some(Value::String(s)) if !s.is_empty() && s.encode_utf16().count() <= 128 => Ok(s.to_string()),
        _ => Err("invalid entity registry name".into()),
    }
}

/// `table(names)`: the distinct names, sorted (UTF-16 order); id = index + 1.
fn table(names: Vec<Option<Value>>) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let n = valid_name(name.as_ref())?;
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out.sort_by(|a, b| utf16_cmp(a, b));
    if out.len() > 0xffff {
        return Err("entity registry capacity exceeded".into());
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct EntityRegistry {
    /// Combat definitions: JSON path text and definition, id = index + 1.
    pub combat: Vec<(String, Value)>,
    /// Class, weapon and post names, id = index + 1.
    pub classes: Vec<String>,
    pub weapons: Vec<String>,
    pub posts: Vec<String>,
}

/// The battle's posts as the registry names them (entity-spawn-posts.js
/// entityPostNames): each map post (`desc.name || '#<index>'`,
/// commandposts.js Post), then `transport-<i>` for each vehicle slot.
pub fn post_names(c: &Content) -> Vec<Option<Value>> {
    let b = &c.battle;
    let mut out: Vec<Option<Value>> = Vec::new();
    for (i, desc) in b.world.get("commandPosts").filter(|p| p.truthy()).and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
        out.push(Some(match desc.get("name") {
            Some(n) if n.truthy() => n.clone(),
            _ => Value::str(&format!("#{i}")),
        }));
    }
    let slots = b.vehicle_def.as_ref().and_then(|v| v.get("spawns")).and_then(Value::as_array).map(|s| s.len()).unwrap_or(0);
    for i in 0..slots {
        out.push(Some(Value::str(&format!("transport-{i}"))));
    }
    out
}

/// createEntityRegistry(data, { posts }): the content tables.
pub fn entity_registry(c: &Content) -> Result<EntityRegistry, String> {
    let content = registry_content(c);
    let mut classes: Vec<Option<Value>> = Vec::new();
    let mut weapons: Vec<Option<Value>> = Vec::new();
    if let Some(sides) = content.get("sides").and_then(Value::as_object) {
        for side in sides.values() {
            for (class, def) in side.get("units").and_then(Value::as_object).map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
                classes.push(Some(Value::str(class)));
                for weapon in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
                    weapons.push(weapon.get("class").cloned());
                }
            }
        }
    }
    if let Some(vehicle_classes) = c.battle.vehicle_def.as_ref().and_then(|v| v.get("classes")).and_then(Value::as_object) {
        for (class, def) in vehicle_classes.entries() {
            classes.push(Some(Value::str(class)));
            for seat in def.get("seats").and_then(Value::as_array).unwrap_or(&[]) {
                for weapon in seat.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
                    weapons.push(weapon.get("def").and_then(|d| d.get("class")).cloned());
                }
            }
        }
    }
    if let Some(instances) = c.battle.props.as_ref().and_then(|p| p.get("instances")).and_then(Value::as_array) {
        for instance in instances {
            if let Some(class) = instance.get("weapon").and_then(|w| w.get("class")).filter(|x| x.truthy()) {
                weapons.push(Some(class.clone()));
            }
        }
    }
    let classes = table(classes)?;
    let weapons = table(weapons)?;
    let posts = table(post_names(c))?;
    if classes.is_empty() {
        return Err("entity registry has no classes".into());
    }
    let combat = combat_registry(&content)?;
    Ok(EntityRegistry { combat, classes, weapons, posts })
}
