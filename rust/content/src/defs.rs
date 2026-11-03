//! The battle's definitions, linked and interned: each side's classes with
//! their bodies, clips, weapons and drops; each weapon with its model's
//! muzzle, ordnance and item; the items with their radii and pickup centres;
//! the vehicle classes with their seats and templates; the props. Every
//! definition keeps its record (the parsed JSON object, by identity, as the
//! JavaScript keeps the object) for the fields the simulation reads by name,
//! and carries the ids the wire gives it (src/registry.rs).
//!
//! Identity matters: a class's weapon entries are objects of their own even
//! when two classes carry the same weapon class, and the combat registry
//! names a definition by the object, not by its contents.

use crate::content::Content;
use crate::derived::{droideka_guns, item_radius, weapon_muzzle, DroidekaGun, Muzzle};
use crate::intern::{Name, Names};
use crate::registry::{entity_registry, EntityRegistry};
use crate::three::Vector3;
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// A value's JavaScript identity (objects and arrays).
pub fn identity(v: &Value) -> Option<usize> {
    match v {
        Value::Object(o) => Some(Rc::as_ptr(o) as *const u8 as usize),
        Value::Array(a) => Some(Rc::as_ptr(a) as *const u8 as usize),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct Drop {
    pub class: Option<Name>,
    pub probability: Option<f64>,
    /// The side's item class it names (game.js: `sides[unit.side].items`).
    pub item: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ClassDef {
    pub name: Name,
    pub side: Name,
    pub base: Option<Name>,
    /// units.json's class record.
    pub record: Value,
    /// Index into `Content::actors` (soldiers and droidekas).
    pub body: Option<usize>,
    /// Index into `Content::body_clips`.
    pub clips: Option<usize>,
    /// Index into `Content::walker_clips` (droidekas).
    pub walker_clips: Option<usize>,
    /// Indices into `Defs::weapons`, in the class's order.
    pub weapons: Vec<usize>,
    pub drops: Vec<Drop>,
    /// units.js buildBody: getting up from prone, a dive (s).
    pub getup_time: f64,
    pub dive_time: f64,
    /// droideka.js arm: its guns' bones and barrel tips.
    pub droideka_guns: Vec<DroidekaGun>,
    /// The wire's class id.
    pub registry_id: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct WeaponDef {
    /// Index into `Defs::classes`.
    pub owner: usize,
    /// Its place in the class's weapons.
    pub slot: usize,
    pub class: Option<Name>,
    pub base: Option<Name>,
    pub geometry: Option<Name>,
    pub record: Value,
    /// combat.js: the held model's muzzle, for the first weapon of each
    /// model a class holds (None for the others and model-less weapons).
    pub muzzle: Option<Muzzle>,
    /// Index into `Defs::items` (a dispenser's or mine layer's item).
    pub item: Option<usize>,
    /// The wire's weapon id (by weapon class name).
    pub registry_id: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct ItemDef {
    /// Where it was found: "<side>/<item>", "<side>/<class>/weapon/<i>",
    /// "placed/<i>".
    pub key: String,
    pub class: Option<Name>,
    pub base: Option<Name>,
    pub record: Value,
    /// items.js itemState: the model's radius and pickup centre.
    pub radius: f64,
    pub pickup_center: Option<Vector3>,
}

#[derive(Clone, Debug)]
pub struct SeatDef {
    pub name: Option<Name>,
    pub record: Value,
    /// Its weapons' records ({ def, ... }).
    pub weapons: Vec<Value>,
    /// The wire's weapon id of each.
    pub weapon_ids: Vec<Option<u16>>,
}

#[derive(Clone, Debug)]
pub struct VehicleClassDef {
    pub name: Name,
    pub base: Option<Name>,
    pub model: Option<Name>,
    pub record: Value,
    /// Index into `Content::vehicle_templates` (its model's).
    pub template: Option<usize>,
    pub seats: Vec<SeatDef>,
    pub registry_id: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct PropDef {
    /// Its place in props.json's instances.
    pub index: usize,
    pub class: Option<Name>,
    pub base: Option<Name>,
    pub model: Option<Name>,
    pub record: Value,
    /// Index into `Content::prop_bodies`.
    pub bodies: Option<usize>,
    /// Its weapon's wire id.
    pub weapon_id: Option<u16>,
}

pub struct Defs {
    pub names: Names,
    pub classes: Vec<ClassDef>,
    pub weapons: Vec<WeaponDef>,
    pub items: Vec<ItemDef>,
    pub vehicles: Vec<VehicleClassDef>,
    pub props: Vec<PropDef>,
    pub registry: EntityRegistry,
    combat_ids: HashMap<usize, u16>,
}

fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

fn table_id(table: &[String], name: Option<&str>) -> Option<u16> {
    let name = name?;
    table.iter().position(|n| n == name).map(|i| (i + 1) as u16)
}

impl Defs {
    pub fn build(c: &Content) -> Result<Defs, String> {
        let registry = entity_registry(c)?;
        let combat_ids: HashMap<usize, u16> =
            registry.combat.iter().enumerate().filter_map(|(i, (_, v))| identity(v).map(|id| (id, (i + 1) as u16))).collect();
        let b = &c.battle;
        let mut names = Names::default();
        let intern = |names: &mut Names, v: Option<&Value>| text(v).map(|s| names.intern(s));
        let mut classes = Vec::new();
        let mut weapons = Vec::new();
        let mut items: Vec<ItemDef> = Vec::new();
        let item_def = |key: String, record: &Value, names: &mut Names, items: &mut Vec<ItemDef>| -> usize {
            let model = match record.get("GeometryName") {
                Some(g) if g.truthy() => c.weapon_models.get(g.as_str().unwrap_or("")),
                _ => None,
            };
            let (radius, pickup_center) = item_radius(model);
            items.push(ItemDef {
                key,
                class: text(record.get("class")).map(|s| names.intern(s)),
                base: text(record.get("base")).map(|s| names.intern(s)),
                record: record.clone(),
                radius,
                pickup_center,
            });
            items.len() - 1
        };
        for side in &b.sides {
            let side_name = names.intern(&side.name);
            // The side's item classes first: drops name them.
            let mut side_items: Vec<(String, usize)> = Vec::new();
            if let Some(list) = side.items.as_object() {
                for (name, def) in list.entries() {
                    let i = item_def(format!("{}/{name}", side.name), def, &mut names, &mut items);
                    side_items.push((name.to_string(), i));
                }
            }
            let Some(units) = side.units.as_object() else { continue };
            for (class_name, def) in units.entries() {
                let index = classes.len();
                let name = names.intern(class_name);
                let base = intern(&mut names, def.get("base"));
                let model = text(def.get("model")).unwrap_or("undefined");
                let body_key = format!("{}/{model}", side.name);
                let body = c.actors.iter().position(|(k, _)| *k == body_key);
                let clips = c.body_clips.iter().position(|(k, _)| *k == body_key);
                let walker_clips = c.walker_clips.iter().position(|(k, _)| *k == format!("{}/{class_name}", side.name));
                let (getup_time, dive_time) = match clips {
                    Some(i) => {
                        let bank = &c.body_clips[i].1;
                        let getup = bank.get("human_rifle_prone_getup_stand").map(|x| x.duration).unwrap_or(0.0);
                        let dive = crate::DIVE_LENGTH * bank.get("human_rifle_diveforward").map(|x| x.duration).unwrap_or(0.0);
                        (getup, dive)
                    }
                    None => (0.0, 0.0),
                };
                let guns = match (text(def.get("base")), body) {
                    (Some("walkerdroid"), Some(i)) => droideka_guns(&c.actors[i].1, def.get("weapons")),
                    _ => Vec::new(),
                };
                let mut weapon_ids = Vec::new();
                let mut held: Vec<String> = Vec::new();
                for (slot, w) in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
                    let geometry = match w.get("GeometryName") {
                        Some(Value::String(s)) if !s.is_empty() => Some(s.to_string()),
                        _ => None,
                    };
                    let muzzle = match &geometry {
                        Some(g) if !held.contains(g) && c.weapon_models.has(g) => {
                            held.push(g.clone());
                            Some(weapon_muzzle(c.weapon_models.get(g).map(|m| m.as_slice()), side.fire_points.get(g), Some(w)))
                        }
                        _ => None,
                    };
                    let item = match w.get("item").filter(|x| x.as_object().is_some()) {
                        Some(record) => Some(item_def(format!("{}/{class_name}/weapon/{slot}", side.name), record, &mut names, &mut items)),
                        None => None,
                    };
                    weapons.push(WeaponDef {
                        owner: index,
                        slot,
                        class: intern(&mut names, w.get("class")),
                        base: intern(&mut names, w.get("base")),
                        geometry: geometry.as_deref().map(|g| names.intern(g)),
                        record: w.clone(),
                        muzzle,
                        item,
                        registry_id: table_id(&registry.weapons, text(w.get("class"))),
                    });
                    weapon_ids.push(weapons.len() - 1);
                }
                let drops = def
                    .get("drops")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .map(|d| Drop {
                        class: intern(&mut names, d.get("class")),
                        probability: d.get("probability").and_then(Value::as_f64),
                        item: text(d.get("class")).and_then(|k| side_items.iter().find(|(n, _)| n == k).map(|(_, i)| *i)),
                    })
                    .collect();
                classes.push(ClassDef {
                    name,
                    side: side_name,
                    base,
                    record: def.clone(),
                    body,
                    clips,
                    walker_clips,
                    weapons: weapon_ids,
                    drops,
                    getup_time,
                    dive_time,
                    droideka_guns: guns,
                    registry_id: table_id(&registry.classes, Some(class_name)),
                });
            }
        }
        if let Some(placed) = b.props.as_ref().and_then(|p| p.get("items")).and_then(Value::as_array) {
            for (i, entry) in placed.iter().enumerate() {
                if let Some(record) = entry.get("item").filter(|x| x.as_object().is_some()) {
                    item_def(format!("placed/{i}"), record, &mut names, &mut items);
                }
            }
        }
        let mut vehicles = Vec::new();
        if let Some(list) = b.vehicle_def.as_ref().and_then(|v| v.get("classes")).and_then(Value::as_object) {
            for (class_name, def) in list.entries() {
                let model = text(def.get("model"));
                let seats = def
                    .get("seats")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .map(|seat| {
                        let weapons: Vec<Value> = seat.get("weapons").and_then(Value::as_array).unwrap_or(&[]).to_vec();
                        SeatDef {
                            name: intern(&mut names, seat.get("name")),
                            record: seat.clone(),
                            weapon_ids: weapons.iter().map(|w| table_id(&registry.weapons, text(w.get("def").and_then(|d| d.get("class"))))).collect(),
                            weapons,
                        }
                    })
                    .collect();
                vehicles.push(VehicleClassDef {
                    name: names.intern(class_name),
                    base: intern(&mut names, def.get("base")),
                    model: model.map(|m| names.intern(m)),
                    record: def.clone(),
                    template: model.and_then(|m| c.vehicle_templates.iter().position(|t| t.name == m)),
                    seats,
                    registry_id: table_id(&registry.classes, Some(class_name)),
                });
            }
        }
        let mut props = Vec::new();
        if let Some(list) = b.props.as_ref().and_then(|p| p.get("instances")).and_then(Value::as_array) {
            for (index, desc) in list.iter().enumerate() {
                props.push(PropDef {
                    index,
                    class: intern(&mut names, desc.get("class")),
                    base: intern(&mut names, desc.get("base")),
                    model: intern(&mut names, desc.get("model")),
                    record: desc.clone(),
                    bodies: c.prop_bodies.iter().position(|p| p.instance == index),
                    weapon_id: table_id(&registry.weapons, text(desc.get("weapon").and_then(|w| w.get("class")))),
                });
            }
        }
        Ok(Defs { names, classes, weapons, items, vehicles, props, registry, combat_ids })
    }

    /// The wire's combat id of a definition (by identity, as
    /// network-combat-registry.js definitionId).
    pub fn combat_id(&self, def: &Value) -> Option<u16> {
        identity(def).and_then(|id| self.combat_ids.get(&id).copied())
    }

    pub fn class(&self, side: &str, name: &str) -> Option<&ClassDef> {
        let side = self.names.get(side)?;
        let name = self.names.get(name)?;
        self.classes.iter().find(|c| c.side == side && c.name == name)
    }
}
