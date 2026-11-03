//! A battle's content, fetched and parsed in the order and by the rules of
//! src/game.js's headless start (the world worker's and the dedicated
//! server's load): map.json and the shared files, the battle and side the
//! query picks, that side's classes and weapons and the other sides', the
//! bodies every soldier class needs, the collision, props, vehicle pack and
//! the AI's data. Each part keeps what the JavaScript keeps; what the
//! loaders derive from it lives in the modules that derive it.

use crate::files::Files;
use crate::json;
use crate::mission::{select_mission, spawn_policy, Policy, Query};
use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A file as a battle met it.
#[derive(Clone, Debug)]
pub enum Fetched {
    Json(Value),
    Bin(Rc<[u8]>),
    Missing,
}

/// Fetches files for battles, parsing each JSON file once however many
/// battles read it.
pub struct Loader<'f> {
    pub files: &'f dyn Files,
    parsed: RefCell<HashMap<String, Result<Value, String>>>,
}

impl<'f> Loader<'f> {
    pub fn new(files: &'f dyn Files) -> Self {
        Loader { files, parsed: RefCell::new(HashMap::new()) }
    }
}

/// One battle's fetches, in the order made (repeats keep their first place).
#[derive(Default)]
pub struct FetchLog {
    pub order: Vec<String>,
    pub entries: HashMap<String, Fetched>,
}

struct Fetch<'a, 'f> {
    loader: &'a Loader<'f>,
    log: FetchLog,
}

impl<'a, 'f> Fetch<'a, 'f> {
    fn note(&mut self, path: &str, entry: Fetched) {
        if !self.log.entries.contains_key(path) {
            self.log.order.push(path.to_string());
        }
        self.log.entries.insert(path.to_string(), entry);
    }

    /// `fetch(url).then(r => r.json())`: an error for a missing file.
    fn json(&mut self, url: &str) -> Result<Value, String> {
        let path = url.trim_start_matches('/').to_string();
        let bytes = match self.loader.files.read(&path) {
            Some(bytes) => bytes,
            None => {
                self.note(&path, Fetched::Missing);
                return Err(format!("404 {url}"));
            }
        };
        let parsed = {
            let mut cache = self.loader.parsed.borrow_mut();
            cache.entry(path.clone()).or_insert_with(|| json::parse_bytes(&bytes).map_err(|e| format!("{path}: {e}"))).clone()
        };
        let value = parsed?;
        self.note(&path, Fetched::Json(value.clone()));
        Ok(value)
    }

    /// `optional(fetch(url).then(ok).then(r => r.json()))`: None when the
    /// file is missing (or does not parse, as the promise rejects then too).
    fn optional_json(&mut self, url: &str) -> Option<Value> {
        self.json(url).ok()
    }

    fn bin(&mut self, url: &str) -> Result<Rc<[u8]>, String> {
        let path = url.trim_start_matches('/').to_string();
        match self.loader.files.read(&path) {
            Some(bytes) => {
                self.note(&path, Fetched::Bin(bytes.clone()));
                Ok(bytes)
            }
            None => {
                self.note(&path, Fetched::Missing);
                Err(format!("404 {url}"))
            }
        }
    }

    fn optional_bin(&mut self, url: &str) -> Option<Rc<[u8]>> {
        self.bin(url).ok()
    }
}

/// A side's part of a battle (game.js `sides[name]` and what was fetched
/// for it): units.json, weapons.json and its packs, effects.json.
#[derive(Clone, Debug)]
pub struct SideFiles {
    pub name: String,
    /// `/sides/<name>`
    pub dir: String,
    /// units.json, whole.
    pub units_file: Value,
    /// Its `units` (class name -> class).
    pub units: Value,
    /// Its `items`, or {}.
    pub items: Value,
    /// weapons.json.
    pub weapons: Value,
    /// weapons.json's `firePoints`, or {}.
    pub fire_points: Value,
    pub weapon_buf: Rc<[u8]>,
    pub weapon_index_buf: Rc<[u8]>,
    /// effects.json, when the side has one.
    pub fx: Option<Value>,
}

/// A soldier body's files (actors/<model>).
#[derive(Clone, Debug)]
pub struct BodyFiles {
    pub def: Value,
    pub buf: Rc<[u8]>,
    pub index_buf: Rc<[u8]>,
}

/// A droideka's own clips (actors/<clips>).
#[derive(Clone, Debug)]
pub struct WalkerFiles {
    pub meta: Value,
    pub buf: Rc<[u8]>,
}

/// The vehicle pack's binaries.
#[derive(Clone, Debug)]
pub struct VehicleBufs {
    pub models: Rc<[u8]>,
    pub models_index: Rc<[u8]>,
    pub extras: Rc<[u8]>,
    pub extras_index: Rc<[u8]>,
    pub animations: Option<Rc<[u8]>>,
}

pub struct Battle {
    pub map: String,
    pub query_text: String,
    pub query: Query,
    pub map_data: Value,
    pub anim_meta: Value,
    pub common: Value,
    pub effects: Value,
    /// map.json as the battle plays it (selectMission).
    pub world: Value,
    pub policy: Policy,
    /// The side and class played first.
    pub side: String,
    pub unit: String,
    pub soldier_def: Value,
    pub anim_buf: Rc<[u8]>,
    pub coll_buf: Rc<[u8]>,
    pub coll_index_buf: Rc<[u8]>,
    pub coll_poly_buf: Rc<[u8]>,
    pub ground_buf: Rc<[u8]>,
    pub ground_index_buf: Rc<[u8]>,
    pub side_fx: Option<Value>,
    pub props: Option<Value>,
    pub props_buf: Option<Rc<[u8]>>,
    pub props_index_buf: Option<Rc<[u8]>>,
    pub props_coll_buf: Option<Rc<[u8]>>,
    pub props_coll_index_buf: Option<Rc<[u8]>>,
    pub props_coll_poly_buf: Option<Rc<[u8]>>,
    pub props_anim_buf: Option<Rc<[u8]>>,
    pub vehicle_dir: String,
    pub vehicle_def: Option<Value>,
    pub vehicle_bufs: Option<VehicleBufs>,
    /// game.js `sides`: the side played first, then the mission's others in
    /// the order their loads finish (the mission's order: every side has the
    /// same files).
    pub sides: Vec<SideFiles>,
    /// Bodies by "<side>/<model>", in the order first needed.
    pub bodies: Vec<(String, BodyFiles)>,
    /// Droideka clips by "<side>/<clips>".
    pub walkers: Vec<(String, WalkerFiles)>,
    pub nav: Option<Value>,
    pub ai_mission: Option<Value>,
    /// Each side's ai.json, in `sides` order.
    pub class_data: Vec<(String, Option<Value>)>,
    pub nav_bin: Option<Rc<[u8]>>,
    pub fetched: FetchLog,
}

fn str_or<'a>(v: Option<&'a Value>, fallback: &'a str) -> &'a str {
    v.and_then(Value::as_str).unwrap_or(fallback)
}

fn object_or_empty(v: Option<&Value>) -> Value {
    match v {
        Some(x) if x.truthy() => x.clone(),
        _ => Value::Object(Rc::new(Default::default())),
    }
}

fn has_keys(v: Option<&Value>) -> bool {
    v.and_then(Value::as_object).map(|o| !o.is_empty()).unwrap_or(false)
}

impl Battle {
    /// game.js start(), headless, for `map` and the battle's query.
    pub fn load(loader: &Loader, map: &str, query_text: &str) -> Result<Battle, String> {
        let mut f = Fetch { loader, log: FetchLog::default() };
        let query = Query::parse(query_text);
        let map_dir = format!("/maps/{map}");
        // game.js 214-219
        let map_data = f.json(&format!("{map_dir}/map.json"))?;
        let anim_meta = f.json("/common/anims.json")?;
        let common = f.json("/common/common.json")?;
        let effects = f.json("/common/effects.json")?;
        // 222-227
        let world = select_mission(&map_data, &query);
        let mission = world.get("mission").filter(|m| !m.is_null());
        let policy = spawn_policy(&query, mission);
        let side_name = policy.side.clone();
        let unit = policy.class_name.clone();
        let side_dir = format!("/sides/{side_name}");
        // 229-247
        let side_units = f.json(&format!("{side_dir}/units.json"))?;
        let weapons_def = f.json(&format!("{side_dir}/weapons.json"))?;
        let soldier_def = side_units.get("units").and_then(|u| u.get(&unit)).cloned().ok_or_else(|| format!("{unit} isn't in {side_dir}"))?;
        let model = str_or(soldier_def.get("model"), "undefined").to_string();
        let body_file = format!("{side_dir}/actors/{model}");
        let actor_def = f.json(&format!("{body_file}.json"))?;
        let actor_buf = f.bin(&format!("{body_file}.bin"))?;
        let actor_index_buf = f.bin(&format!("{body_file}_index.bin"))?;
        let anim_buf = f.bin("/common/anims.bin")?;
        let coll_buf = f.bin(&format!("{map_dir}/collision.bin"))?;
        let coll_index_buf = f.bin(&format!("{map_dir}/collision_index.bin"))?;
        let coll_poly_buf = f.bin(&format!("{map_dir}/collision_poly.bin"))?;
        let ground_buf = f.bin(&format!("{map_dir}/terrain_collision.bin"))?;
        let ground_index_buf = f.bin(&format!("{map_dir}/terrain_collision_index.bin"))?;
        // 251-262
        let walker = soldier_def.get("base").and_then(Value::as_str) == Some("walkerdroid");
        let clips_name = str_or(soldier_def.get("clips"), "undefined").to_string();
        let walker_files = if walker {
            let meta = f.json(&format!("{side_dir}/actors/{clips_name}.json"))?;
            let buf = f.bin(&format!("{side_dir}/actors/{clips_name}.bin"))?;
            Some(WalkerFiles { meta, buf })
        } else {
            None
        };
        let side_fx = f.optional_json(&format!("{side_dir}/effects.json"));
        if side_fx.as_ref().map(|fx| has_keys(fx.get("models"))).unwrap_or(false) {
            f.bin(&format!("{side_dir}/fxmodels.bin"))?;
            f.bin(&format!("{side_dir}/fxmodels_index.bin"))?;
        }
        // 263-287 (headless: no arms, first-person clips, sky or turret extras)
        let props = f.optional_json(&format!("{map_dir}/props.json"));
        let (mut props_buf, mut props_index_buf, mut props_coll_buf, mut props_coll_index_buf, mut props_coll_poly_buf) = (None, None, None, None, None);
        if props.is_some() {
            props_buf = Some(f.bin(&format!("{map_dir}/props.bin"))?);
            props_index_buf = Some(f.bin(&format!("{map_dir}/props_index.bin"))?);
            props_coll_buf = f.optional_bin(&format!("{map_dir}/props_collision.bin"));
            props_coll_index_buf = f.optional_bin(&format!("{map_dir}/props_collision_index.bin"));
            props_coll_poly_buf = f.optional_bin(&format!("{map_dir}/props_collision_poly.bin"));
        }
        let props_anim_buf = if props.as_ref().map(|p| has_keys(p.get("animations"))).unwrap_or(false) {
            f.optional_bin(&format!("{map_dir}/props_anims.bin"))
        } else {
            None
        };
        // 293-301
        let weapon_buf = f.bin(&format!("{side_dir}/weapons.bin"))?;
        let weapon_index_buf = f.bin(&format!("{side_dir}/weapons_index.bin"))?;
        f.bin("/common/fxmodels.bin")?;
        f.bin("/common/fxmodels_index.bin")?;
        // 302-312
        let vehicles_path = world.get("vehicles").filter(|v| v.truthy());
        let vehicles_text = match vehicles_path {
            Some(Value::String(s)) => s.to_string(),
            Some(_) => "[object Object]".to_string(),
            None => "vehicles/".to_string(),
        };
        let vehicle_dir = format!("{map_dir}/{}", match vehicles_text.rfind('/') {
            Some(at) => &vehicles_text[..at],
            None => vehicles_text.as_str(),
        });
        let (vehicle_def, vehicle_bufs) = match vehicles_path {
            Some(_) => {
                let def = f.json(&format!("{map_dir}/{vehicles_text}"))?;
                let models = f.bin(&format!("{vehicle_dir}/models.bin"))?;
                let models_index = f.bin(&format!("{vehicle_dir}/models_index.bin"))?;
                let extras = f.bin(&format!("{vehicle_dir}/extras.bin"))?;
                let extras_index = f.bin(&format!("{vehicle_dir}/extras_index.bin"))?;
                let animations = if def.get("animations").map(Value::truthy).unwrap_or(false) {
                    Some(f.bin(&format!("{vehicle_dir}/animations.bin"))?)
                } else {
                    None
                };
                (Some(def), Some(VehicleBufs { models, models_index, extras, extras_index, animations }))
            }
            None => (None, None),
        };
        // 315-331: the other sides.
        let mut sides = vec![SideFiles {
            name: side_name.clone(),
            dir: side_dir.clone(),
            units: object_or_empty(side_units.get("units")),
            items: object_or_empty(side_units.get("items")),
            fire_points: object_or_empty(weapons_def.get("firePoints")),
            units_file: side_units.clone(),
            weapons: weapons_def.clone(),
            weapon_buf: weapon_buf.clone(),
            weapon_index_buf: weapon_index_buf.clone(),
            fx: side_fx.clone(),
        }];
        let mut others: Vec<String> = Vec::new();
        if let Some(list) = mission.and_then(|m| m.get("sides")).and_then(Value::as_array) {
            for name in list.iter().filter_map(Value::as_str) {
                if name != side_name && !others.iter().any(|o| o == name) {
                    others.push(name.to_string());
                }
            }
        }
        for name in others {
            let dir = format!("/sides/{name}");
            let loaded = (|| -> Result<SideFiles, String> {
                let units_file = f.json(&format!("{dir}/units.json"))?;
                let weapons = f.json(&format!("{dir}/weapons.json"))?;
                let fx = f.optional_json(&format!("{dir}/effects.json"));
                let weapon_buf = f.bin(&format!("{dir}/weapons.bin"))?;
                let weapon_index_buf = f.bin(&format!("{dir}/weapons_index.bin"))?;
                if fx.as_ref().map(|x| has_keys(x.get("models"))).unwrap_or(false) {
                    f.bin(&format!("{dir}/fxmodels.bin"))?;
                    f.bin(&format!("{dir}/fxmodels_index.bin"))?;
                }
                Ok(SideFiles {
                    name: name.clone(),
                    dir: dir.clone(),
                    units: object_or_empty(units_file.get("units")),
                    items: object_or_empty(units_file.get("items")),
                    fire_points: object_or_empty(weapons.get("firePoints")),
                    units_file,
                    weapons,
                    weapon_buf,
                    weapon_index_buf,
                    fx,
                })
            })();
            // A side that wasn't extracted fields no units.
            if let Ok(side) = loaded {
                sides.push(side);
            }
        }
        // The bodies: the class played, then every soldier and droideka class
        // of every side (units.js prepareSoldiers), with droideka clips.
        let mut bodies: Vec<(String, BodyFiles)> = vec![(format!("{side_name}/{model}"), BodyFiles { def: actor_def, buf: actor_buf, index_buf: actor_index_buf })];
        let mut walkers: Vec<(String, WalkerFiles)> = Vec::new();
        if let Some(w) = walker_files {
            walkers.push((format!("{side_name}/{clips_name}"), w));
        }
        for side in &sides {
            let units = match side.units.as_object() {
                Some(u) => u.clone(),
                None => continue,
            };
            for def in units.values() {
                let base = def.get("base").and_then(Value::as_str);
                if base != Some("soldier") && base != Some("walkerdroid") {
                    continue;
                }
                let dir = format!("{}/actors", side.dir);
                let model = str_or(def.get("model"), "undefined");
                let key = format!("{}/{model}", side.name);
                if !bodies.iter().any(|(k, _)| *k == key) {
                    let def_json = f.json(&format!("{dir}/{model}.json"))?;
                    let buf = f.bin(&format!("{dir}/{model}.bin"))?;
                    let index_buf = f.bin(&format!("{dir}/{model}_index.bin"))?;
                    bodies.push((key, BodyFiles { def: def_json, buf, index_buf }));
                }
                if base == Some("walkerdroid") {
                    let clips = str_or(def.get("clips"), "undefined");
                    let key = format!("{}/{clips}", side.name);
                    if !walkers.iter().any(|(k, _)| *k == key) {
                        let meta = f.json(&format!("{dir}/{clips}.json"))?;
                        let buf = f.bin(&format!("{dir}/{clips}.bin"))?;
                        walkers.push((key, WalkerFiles { meta, buf }));
                    }
                }
            }
        }
        // The AI's data (src/ai/index.js loadAIData) for Object.keys(sides).
        let nav = f.optional_json(&format!("{map_dir}/ai-nav.json"));
        let ai_mission = f.optional_json(&format!("{map_dir}/ai-mission.json"));
        let class_data: Vec<(String, Option<Value>)> =
            sides.iter().map(|s| s.name.clone()).collect::<Vec<_>>().into_iter().map(|name| {
                let data = f.optional_json(&format!("/sides/{name}/ai.json"));
                (name, data)
            }).collect();
        let nav_bin = match nav.as_ref().and_then(|n| n.get("bin")).and_then(|b| b.get("file")).filter(|v| v.truthy()) {
            Some(file) => {
                let name = match file {
                    Value::String(s) => s.to_string(),
                    other => crate::dump::value_text(other),
                };
                f.optional_bin(&format!("{map_dir}/{name}"))
            }
            None => None,
        };
        Ok(Battle {
            map: map.to_string(),
            query_text: query_text.to_string(),
            query,
            map_data,
            anim_meta,
            common,
            effects,
            world,
            policy,
            side: side_name,
            unit,
            soldier_def,
            anim_buf,
            coll_buf,
            coll_index_buf,
            coll_poly_buf,
            ground_buf,
            ground_index_buf,
            side_fx,
            props,
            props_buf,
            props_index_buf,
            props_coll_buf,
            props_coll_index_buf,
            props_coll_poly_buf,
            props_anim_buf,
            vehicle_dir,
            vehicle_def,
            vehicle_bufs,
            sides,
            bodies,
            walkers,
            nav,
            ai_mission,
            class_data,
            nav_bin,
            fetched: f.log,
        })
    }

    /// The mission block (`world.mission`), when the battle has one.
    pub fn mission(&self) -> Option<&Value> {
        self.world.get("mission").filter(|m| !m.is_null())
    }

    pub fn side(&self, name: &str) -> Option<&SideFiles> {
        self.sides.iter().find(|s| s.name == name)
    }
}
