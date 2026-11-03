//! A battle's canonical dump: the same lines, in the same order, as
//! tools/content-dump.mjs's dumpBattle writes from the JavaScript.
//!
//! Lines are grouped; a group is written as one line (`group`, its name,
//! its line count and the SHA-256 of its lines) unless the dump is full,
//! which writes every line (to find where two dumps part).

use crate::battle::{Battle, Fetched, Loader};
use crate::catalog::Planned;
use crate::clips::Clips;
use crate::content::Content;
use crate::derived::{droideka_guns, item_radius, weapon_muzzle, Muzzle};
use crate::dump::{self, Dump};
use crate::mission::utf16_cmp;
use crate::models::{ModelMap, PART_FIELDS};
use crate::odf;
use crate::regions::{build_region, BuiltRegion};
use crate::three::{Matrix4, Vector3};
use crate::value::Value;
use std::collections::HashMap;

/// Payloads already made, by file path (a file's payload does not depend on
/// the battle that loaded it).
#[derive(Default)]
pub struct PayloadCache {
    files: HashMap<String, String>,
}

// Canonical text pieces (src/dump.rs).
pub fn n(x: f64) -> String {
    let mut s = String::new();
    dump::number(&mut s, x);
    s
}
pub fn st(text: &str) -> String {
    let mut s = String::new();
    dump::string(&mut s, text);
    s
}
pub fn opt(v: Option<&Value>) -> String {
    match v {
        Some(v) => dump::value_text(v),
        None => "u".into(),
    }
}
pub fn list(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}
pub fn nums(xs: &[f64]) -> String {
    list(xs.iter().map(|&x| n(x)))
}
/// An object's text with its keys in JavaScript's order (array-index keys
/// first, ascending; a repeated key keeps its first place, last value).
pub fn obj(pairs: Vec<(&str, String)>) -> String {
    let mut firsts: Vec<(&str, String)> = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        match firsts.iter_mut().find(|(f, _)| *f == k) {
            Some(slot) => slot.1 = v,
            None => firsts.push((k, v)),
        }
    }
    let mut indices: Vec<(u32, (&str, String))> = Vec::new();
    let mut others: Vec<(&str, String)> = Vec::new();
    for (k, v) in firsts {
        match crate::value::array_index(k) {
            Some(i) => indices.push((i, (k, v))),
            None => others.push((k, v)),
        }
    }
    indices.sort_by_key(|(i, _)| *i);
    let ordered = indices.into_iter().map(|(_, e)| e).chain(others);
    format!("{{{}}}", ordered.map(|(k, v)| format!("{}:{}", st(k), v)).collect::<Vec<_>>().join(","))
}
fn vec3(v: &Vector3) -> String {
    nums(&[v.x, v.y, v.z])
}
fn mat(m: &Matrix4) -> String {
    nums(&m.elements)
}

/// Lines under one name.
pub struct Group {
    pub name: String,
    pub lines: Vec<(String, String)>,
}

impl Group {
    pub fn new(name: impl Into<String>) -> Self {
        Group { name: name.into(), lines: Vec::new() }
    }
    pub fn line(&mut self, key: impl Into<String>, payload: impl Into<String>) {
        self.lines.push((key.into(), payload.into()));
    }
    pub fn write(self, d: &mut Dump, full: bool) {
        if full {
            for (k, p) in &self.lines {
                d.line(&self.name, k, p);
            }
        } else {
            let mut text = String::new();
            for (k, p) in &self.lines {
                text.push_str(k);
                text.push('\t');
                text.push_str(p);
                text.push('\n');
            }
            d.line("group", &self.name, &format!("{}:sha256:{}", self.lines.len(), dump::hex(&dump::sha256(text.as_bytes()))));
        }
    }
}

fn battle_lines(d: &mut Dump, b: &Battle, cache: &mut PayloadCache) {
    d.value("battle", "query", &Value::str(&b.query_text));
    d.value("battle", "mission", b.mission().and_then(|m| m.get("name")).unwrap_or(&Value::Null));
    d.value("battle", "side", &Value::str(&b.side));
    d.value("battle", "unit", &Value::str(&b.unit));
    d.number("battle", "team", b.policy.team);
    d.line("battle", "sides", &dump::payload(&list(b.sides.iter().map(|s| st(&s.name)))));
    d.value("battle", "vehicles", b.world.get("vehicles").unwrap_or(&Value::Null));
    d.line("battle", "bodies", &dump::payload(&list(b.bodies.iter().map(|(k, _)| st(k)))));
    d.line("battle", "walkers", &dump::payload(&list(b.walkers.iter().map(|(k, _)| st(k)))));
    let mut paths: Vec<&String> = b.fetched.order.iter().collect();
    paths.sort_by(|a, c| utf16_cmp(a, c));
    for path in paths {
        let text = match &b.fetched.entries[path] {
            Fetched::Missing => "missing".to_string(),
            Fetched::Json(v) => {
                let p = cache.files.entry(path.clone()).or_insert_with(|| dump::payload(&dump::value_text(v)));
                format!("json {p}")
            }
            Fetched::Bin(bytes) => {
                let p = cache.files.entry(path.clone()).or_insert_with(|| format!("u8[{}]:sha256:{}", bytes.len(), dump::hex(&dump::sha256(bytes))));
                format!("bin {p}")
            }
        };
        d.line("file", path, &text);
    }
}

fn pack_group(d: &mut Dump, name: &str, pack: &ModelMap, full: bool) {
    let mut g = Group::new(format!("pack/{name}"));
    for (model, parts) in pack.iter() {
        for (i, part) in parts.iter().enumerate() {
            let key = format!("{model}/{i}");
            let meta = obj(PART_FIELDS.iter().map(|&f| (f, opt(part.field(f)))).collect());
            g.line(format!("{key}/meta"), dump::payload(&meta));
            g.line(format!("{key}/data"), dump::typed(&part.data));
            g.line(format!("{key}/index"), dump::typed(&part.index));
            g.line(format!("{key}/sphere"), nums(&[part.sphere_center.x, part.sphere_center.y, part.sphere_center.z, part.sphere_radius]));
        }
    }
    g.write(d, full);
}

fn clips_group(d: &mut Dump, name: &str, clips: &Clips, full: bool) {
    let mut g = Group::new(format!("clips/{name}"));
    for clip in &clips.list {
        let record = obj(vec![
            ("fps", n(clip.fps)),
            ("frames", dump::value_text(&clip.frames)),
            ("tracks", nums(&clip.tracks)),
            ("start", n(clip.start as f64)),
            ("length", n((clip.end - clip.start) as f64)),
            ("duration", n(clip.duration)),
            ("rootMotion", if clip.root_motion { "true".into() } else { "false".into() }),
            ("stride", n(clip.stride)),
            ("speed", n(clip.speed)),
        ]);
        g.line(clip.name.clone(), dump::payload(&record));
    }
    g.write(d, full);
}

fn region_text(r: &BuiltRegion) -> String {
    obj(vec![
        ("type", opt(r.kind.as_ref())),
        ("half", nums(&r.half)),
        ("radius", n(r.radius)),
        ("center", vec3(&r.center)),
        ("inverse", mat(&r.inverse)),
    ])
}

/// The readers' numbers for one setting value: parseFloat, numberOf,
/// Number of the first, scanVector, pair, parseInt.
fn readers(v: Option<&Value>) -> String {
    list(vec![
        n(crate::js::parse_float_value(v)),
        n(odf::number_of(v, f64::NAN)),
        n(odf::number_first(v)),
        match odf::scan_vector(v) {
            Some(x) => nums(&x),
            None => "null".into(),
        },
        match odf::pair(v) {
            Some(x) => nums(&x),
            None => "null".into(),
        },
        n(odf::parse_int10(v)),
    ])
}

fn keys_of(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_object).map(|o| o.keys().map(str::to_string).collect()).unwrap_or_default()
}

pub fn dump_battle(c: &Content, cache: &mut PayloadCache, full: bool) -> String {
    let b = &c.battle;
    let mut d = Dump::default();
    battle_lines(&mut d, b, cache);

    // Model packs, and the battle's weapon models.
    for (side, pack) in &c.weapon_packs {
        pack_group(&mut d, &format!("weapons/{side}"), pack, full);
    }
    if let Some(p) = &c.vehicle_models {
        pack_group(&mut d, "vehicles/models", p, full);
    }
    if let Some(p) = &c.vehicle_extras {
        pack_group(&mut d, "vehicles/extras", p, full);
    }
    if let Some(p) = &c.props_models {
        pack_group(&mut d, "props", p, full);
    }
    let mut g = Group::new("weaponModels");
    for (name, source) in &c.weapon_model_sources {
        g.line(name.clone(), st(source));
    }
    g.write(&mut d, full);

    // Bodies and their clips.
    for (key, actor) in &c.actors {
        let mut g = Group::new(format!("actor/{key}"));
        for (i, bone) in actor.bones.iter().enumerate() {
            let q = &bone.quaternion;
            g.line(format!("bone/{i}"), list(vec![
                st(&bone.name),
                n(bone.parent),
                vec3(&bone.position),
                nums(&[q.x, q.y, q.z, q.w]),
                vec3(&bone.scale),
            ]));
            g.line(format!("world/{i}"), mat(&bone.world));
            g.line(format!("inverse/{i}"), mat(&bone.inverse));
        }
        g.line("bind", mat(&actor.bind_matrix));
        g.line("positions", dump::typed(&actor.positions));
        g.line("skin", dump::typed(&actor.skin_index));
        g.line("attach", opt(actor.def.get("attach")));
        g.write(&mut d, full);
    }
    for (key, clips) in &c.body_clips {
        clips_group(&mut d, &format!("anims/{key}"), clips, full);
        let getup = clips.get("human_rifle_prone_getup_stand").map(|c| c.duration).unwrap_or(0.0);
        let dive = crate::DIVE_LENGTH * clips.get("human_rifle_diveforward").map(|c| c.duration).unwrap_or(0.0);
        d.line("times", key, &nums(&[getup, dive]));
    }
    for (key, clips) in &c.walker_clips {
        clips_group(&mut d, &format!("walker/{key}"), clips, full);
    }

    // Droidekas' barrels.
    for side in &b.sides {
        let Some(units) = side.units.as_object() else { continue };
        for (class, def) in units.entries() {
            if def.get("base").and_then(Value::as_str) != Some("walkerdroid") {
                continue;
            }
            let model = def.get("model").and_then(Value::as_str).unwrap_or("undefined");
            let Some(actor) = c.actor(&format!("{}/{model}", side.name)) else { continue };
            let guns = droideka_guns(actor, def.get("weapons"));
            let text = list(guns.iter().map(|gun| {
                list(vec![
                    n(gun.bone as f64),
                    match &gun.tips {
                        Some(tips) => list(tips.iter().map(vec3)),
                        None => "null".into(),
                    },
                ])
            }));
            d.line("droideka", &format!("{}/{class}", side.name), &dump::payload(&text));
        }
    }

    // Held weapons' muzzles (combat.js 207-229): per class, the first weapon
    // of each model.
    for side in &b.sides {
        let Some(units) = side.units.as_object() else { continue };
        for (class, def) in units.entries() {
            let mut held: Vec<String> = Vec::new();
            for weapon in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
                let name = match weapon.get("GeometryName") {
                    Some(Value::String(s)) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                if held.contains(&name) || !c.weapon_models.has(&name) {
                    continue;
                }
                held.push(name.clone());
                let parts = c.weapon_models.get(&name).map(|m| m.as_slice());
                let point = weapon_muzzle(parts, side.fire_points.get(&name), Some(weapon));
                let text = match point {
                    Muzzle::Authored(v) => opt(v.as_ref()),
                    Muzzle::Found(p) => nums(&p),
                };
                d.line("muzzle", &format!("{}/{class}/{name}", side.name), &text);
            }
        }
    }

    // Items' radii and pickup centres: each side's item classes, every
    // weapon's item, the placed items.
    let item_line = |d: &mut Dump, key: &str, def: Option<&Value>| {
        let model = match def.and_then(|x| x.get("GeometryName")) {
            Some(g) if g.truthy() => c.weapon_models.get(g.as_str().unwrap_or("")),
            _ => None,
        };
        let (radius, center) = item_radius(model);
        d.line("item", key, &list(vec![n(radius), center.map(|v| vec3(&v)).unwrap_or_else(|| "u".into())]));
    };
    for side in &b.sides {
        if let Some(items) = side.items.as_object() {
            for (name, def) in items.entries() {
                item_line(&mut d, &format!("{}/{name}", side.name), Some(def));
            }
        }
        let Some(units) = side.units.as_object() else { continue };
        for (class, def) in units.entries() {
            for (i, weapon) in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
                if let Some(item) = weapon.get("item").filter(|x| x.as_object().is_some()) {
                    item_line(&mut d, &format!("{}/{class}/weapon/{i}", side.name), Some(item));
                }
            }
        }
    }
    if let Some(placed) = b.props.as_ref().and_then(|p| p.get("items")).and_then(Value::as_array) {
        for (i, entry) in placed.iter().enumerate() {
            if let Some(item) = entry.get("item").filter(|x| x.as_object().is_some()) {
                item_line(&mut d, &format!("placed/{i}"), Some(item));
            }
        }
    }

    // The map's collision.
    let col = &c.collision;
    d.typed("collision", "triBounds", &col.tri_bounds);
    match &col.grid {
        Some(grid) => {
            d.line("collision", "grid", &dump::payload(&nums(&[grid.min_x, grid.min_z, grid.max_x, grid.max_z, grid.size, grid.nx, grid.nz])));
            d.typed("collision", "grid.cells", &grid.cells);
            d.typed("collision", "grid.start", &grid.start);
        }
        None => d.line("collision", "grid", "null"),
    }
    d.number("collision", "killY", col.kill_y);

    // The vehicle pack's templates.
    for t in &c.vehicle_templates {
        d.line("vehicle", &format!("{}/wheels", t.name), &dump::payload(&list(t.wheel_radii.iter().map(|(k, r)| list(vec![st(k), n(*r)])))));
        d.line("vehicle", &format!("{}/muzzles", t.name), &dump::payload(&list(t.muzzle_tips.iter().map(|(k, p)| list(vec![st(k), nums(p)])))));
        d.line("vehicle", &format!("{}/shape", t.name), &dump::payload(&list(vec![dump::typed(&t.shape.positions), dump::typed(&t.shape.indices), dump::typed(&t.shape.polygons)].into_iter().map(|s| st(&s)))));
        match &t.clips {
            Some(clips) => clips_group(&mut d, &format!("vehicle/{}", t.name), clips, full),
            None => d.line("vehicle", &format!("{}/clips", t.name), "null"),
        }
    }
    if let Some(bl) = &c.vehicle_blockers {
        d.line("vehicle", "blockers", &dump::payload(&list(vec![dump::typed(&bl.positions), dump::typed(&bl.indices), dump::typed(&bl.polygons)].into_iter().map(|s| st(&s)))));
    }

    // The props' bodies and clips.
    for pb in &c.prop_bodies {
        for (look, body) in [("built", &pb.built), ("destroyed", &pb.destroyed), ("unbuilt", &pb.unbuilt)] {
            if let Some(body) = body {
                d.line("prop.body", &format!("{}/{look}", pb.instance), &dump::payload(&list(vec![st(&dump::typed(&body.indices)), st(&dump::typed(&body.polygons))])));
            }
        }
    }
    for (model, clips) in &c.prop_clips {
        match clips {
            Some(clips) => clips_group(&mut d, &format!("prop/{model}"), clips, full),
            None => d.line("prop.clips", model, "null"),
        }
    }

    // Regions: the battle's death regions, every post's capture region,
    // the light regions.
    for (region, built) in &c.death_regions {
        d.line("region", &format!("death/{}", opt(region.get("name"))), &dump::payload(&region_text(built)));
    }
    for (i, post) in b.world.get("commandPosts").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
        if let Some(capture) = post.get("capture").filter(|x| x.truthy()) {
            d.line("region", &format!("capture/{i}"), &dump::payload(&region_text(&build_region(capture))));
        }
    }
    for (i, region) in b.world.get("lightRegions").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
        d.line("region", &format!("light/{i}"), &dump::payload(&region_text(&build_region(region))));
    }

    // ODF settings through every reader.
    if let Some(def) = &b.vehicle_def {
        for (class, cdef) in def.get("classes").and_then(Value::as_object).map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
            let mut keys: Vec<String> = keys_of(cdef.get("settings"));
            for seat in cdef.get("seats").and_then(Value::as_array).unwrap_or(&[]) {
                for k in keys_of(seat.get("settings")) {
                    if !keys.contains(&k) {
                        keys.push(k);
                    }
                }
            }
            let text = obj(keys.iter().map(|k| (k.as_str(), readers(odf::class_setting(Some(cdef), k)))).collect());
            d.line("odf", &format!("vehicle/{class}"), &dump::payload(&text));
            for (i, seat) in cdef.get("seats").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
                let keys = keys_of(seat.get("settings"));
                let text = obj(keys.iter().map(|k| (k.as_str(), readers(odf::setting(seat.get("settings"), k)))).collect());
                d.line("odf", &format!("vehicle/{class}/seat/{i}"), &dump::payload(&text));
            }
        }
    }
    if let Some(props) = &b.props {
        for (i, inst) in props.get("instances").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
            let keys = keys_of(inst.get("settings"));
            let text = obj(keys.iter().map(|k| (k.as_str(), readers(odf::setting(inst.get("settings"), k)))).collect());
            d.line("odf", &format!("prop/{i}"), &dump::payload(&text));
        }
    }

    // The wire's content ids.
    match crate::registry::entity_registry(c) {
        Ok(r) => {
            d.line("registry", "combat", &dump::payload(&list(r.combat.iter().map(|(p, _)| st(p)))));
            d.line("registry", "classes", &dump::payload(&list(r.classes.iter().map(|s| st(s)))));
            d.line("registry", "weapons", &dump::payload(&list(r.weapons.iter().map(|s| st(s)))));
            d.line("registry", "posts", &dump::payload(&list(r.posts.iter().map(|s| st(s)))));
        }
        Err(e) => d.line("registry", "error", &dump::payload(&st(&e))),
    }

    // The AI's route table.
    match &c.route_words {
        None => d.line("nav", "routes", "null"),
        Some(Ok(words)) => d.typed("nav", "routes", words),
        Some(Err(e)) => d.line("nav", "routes", &format!("error {}", st(e))),
    }
    d.text
}

/// A planned battle's dump, or its error line (as the JavaScript tool
/// writes a battle it cannot load).
pub fn dump_planned(loader: &Loader, p: &Planned, cache: &mut PayloadCache, full: bool) -> String {
    match Battle::load(loader, &p.map, &p.query).and_then(Content::build) {
        Ok(content) => dump_battle(&content, cache, full),
        Err(error) => format!("error\t{}\t{}\n", p.name(), dump::value_text(&Value::str(&error))),
    }
}
