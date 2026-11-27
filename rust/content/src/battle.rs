//! A battle's content, fetched and parsed in the order and by the rules of
//! headless start (the local world thread's and the dedicated
//! server's load): map.json and the shared files, the battle and side the
//! query picks, that side's classes and weapons and the other sides', the
//! bodies every soldier class needs, the collision, props, vehicle pack and
//! the AI's data. Each part keeps what the JavaScript keeps; what the
//! loaders derive from it lives in the modules that derive it.

use crate::files::Files;
use crate::json;
use crate::map::{
    materialize_odf_properties, parse_odf_scalar, read_property_pairs, MapAdmission,
};
use crate::mission::{select_mission, spawn_policy, utf16_cmp, Policy, Query};
use crate::value::{Object, Value};
use crate::zero_hash;
use bfe_family::GameFamily;
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

    /// Strict optional JSON fetch for 2005 content: an absent file returns
    /// `Ok(None)`, while a file that exists on disk but fails to parse returns
    /// an `Err` naming the file path.
    fn optional_json_strict(&mut self, url: &str) -> Result<Option<Value>, String> {
        let path = url.trim_start_matches('/').to_string();
        let Some(bytes) = self.loader.files.read(&path) else {
            self.note(&path, Fetched::Missing);
            return Ok(None);
        };
        let parsed = {
            let mut cache = self.loader.parsed.borrow_mut();
            cache
                .entry(path.clone())
                .or_insert_with(|| json::parse_bytes(&bytes).map_err(|e| format!("{path}: {e}")))
                .clone()
        };
        let value = parsed?;
        self.note(&path, Fetched::Json(value.clone()));
        Ok(Some(value))
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

/// A side's part of a battle (`sides[name]` and what was fetched
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

/// Whether a loaded 2005 unit class can run through the current simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimulationSupport {
    /// Steps through the 2004 infantry baseline; 2005-specific infantry
    /// movement (sprint, stamina, rolls) is not yet simulated.
    InfantryBaseline,
    /// Steps through the 2004 infantry baseline while listing additional
    /// unsimulated 2005 class mechanics (combos, cloth, melee, force powers).
    Partial { reasons: Vec<String> },
    /// Loaded and cataloged in the roster, but not yet simulable (never crashes).
    NotYetSimulable { reasons: Vec<String> },
}

/// One loaded class record in a 2005 mission roster report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassEntry {
    pub side: String,
    pub class: String,
    pub base: String,
    pub model: Option<String>,
    pub has_actor: bool,
    pub weapons: usize,
    pub anim_banks: Vec<String>,
    pub support: SimulationSupport,
}

/// Inventory of every class and package named by a 2005 mission.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RosterReport {
    pub mission: String,
    pub requested_packages: Vec<(String, String)>,
    pub classes: Vec<ClassEntry>,
    /// Mission packages that are visual/auxiliary assets rather than unit classes
    /// (e.g. dome sub-archives in space missions).
    pub non_unit_packages: Vec<(String, String, String)>,
}

impl RosterReport {
    pub fn infantry_baseline_count(&self) -> usize {
        self.classes
            .iter()
            .filter(|c| matches!(c.support, SimulationSupport::InfantryBaseline))
            .count()
    }

    pub fn partial_count(&self) -> usize {
        self.classes
            .iter()
            .filter(|c| matches!(c.support, SimulationSupport::Partial { .. }))
            .count()
    }

    pub fn not_yet_simulable_classes(&self) -> Vec<&ClassEntry> {
        self.classes
            .iter()
            .filter(|c| matches!(c.support, SimulationSupport::NotYetSimulable { .. }))
            .collect()
    }
}

/// Additional data carried by a 2005 (`GameFamily::Swbf2`) battle load.
pub struct Swbf2Battle {
    pub map_admission: Rc<MapAdmission>,
    pub common_anim_banks: Value,
    pub common_anim_bin: Rc<[u8]>,
    pub side_anim_banks: Vec<(String, Value, Rc<[u8]>)>,
    /// Ordered animation bank names per `"<side>/<class>"` (common first, then
    /// unit-specific side banks so overrides win).
    pub unit_anim_banks: Vec<(String, Vec<String>)>,
    /// Extra clip `"<bank>:<crc>"` (or bare `"<crc>"`) -> action name aliases
    /// from weapon `comboBindings` (`clipTiming` when present, else `clips`).
    pub unit_clip_names: Vec<(String, HashMap<String, Vec<String>>)>,
    pub roster: RosterReport,
}

pub struct Battle {
    pub family: GameFamily,
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
    /// `sides`: the side played first, then the mission's others in
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
    /// Populated only for `GameFamily::Swbf2`; always `None` for 2004.
    pub swbf2: Option<Rc<Swbf2Battle>>,
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
    /// start(), headless, for `map` and the battle's query.
    pub fn load(loader: &Loader, map: &str, query_text: &str) -> Result<Battle, String> {
        let mut f = Fetch { loader, log: FetchLog::default() };
        let query = Query::parse(query_text);
        let map_dir = format!("/maps/{map}");
        // 214-219
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
        // of every side (prepareSoldiers), with droideka clips.
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
        // The AI's data (loadAIData) for Object.keys(sides).
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
            family: GameFamily::Swbf1,
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
            swbf2: None,
        })
    }

    /// Loads a battle for an explicit `GameFamily`.
    ///
    /// For `GameFamily::Swbf1`, delegates directly to [`Battle::load`] with
    /// zero behavioral changes. For `GameFamily::Swbf2`, composes the map and
    /// active mission layer(s) through [`MapAdmission`], loads the mission's
    /// sides and per-unit CRC animation banks, materializes 2005 ODF unit and
    /// weapon properties, and catalogs simulable vs not-yet-simulable classes.
    pub fn load_for_family(
        loader: &Loader,
        family: GameFamily,
        map: &str,
        query_text: &str,
    ) -> Result<Battle, String> {
        if family == GameFamily::Swbf1 {
            return Self::load(loader, map, query_text);
        }
        Self::load_swbf2(loader, map, query_text)
    }

    fn load_swbf2(loader: &Loader, map: &str, query_text: &str) -> Result<Battle, String> {
        let mut f = Fetch { loader, log: FetchLog::default() };
        let query = Query::parse(query_text);
        let map_admission = Rc::new(MapAdmission::load_for_query(
            loader.files,
            GameFamily::Swbf2,
            map,
            &query,
        )?);
        let mission_name = map_admission
            .mission_name
            .clone()
            .ok_or_else(|| format!("map {map}: no 2005 mission selected"))?;
        let mission_rec = map_admission
            .map_data
            .get("missions")
            .and_then(|m| m.get(&mission_name))
            .cloned()
            .ok_or_else(|| format!("map {map}: missing mission {mission_name}"))?;

        let common = f.json("/common/common.json")?;
        if common.as_object().is_none() {
            return Err("/common/common.json: expected JSON object".into());
        }
        let common_anim_banks = f
            .optional_json_strict("/common/anim_banks.json")?
            .unwrap_or_else(|| Value::Object(Rc::new(Object::new())));
        let common_anim_obj = common_anim_banks
            .as_object()
            .ok_or_else(|| "/common/anim_banks.json: expected JSON object".to_string())?;
        let common_banks_obj = match common_anim_obj.get("banks") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_object()
                    .ok_or_else(|| "/common/anim_banks.json.banks: expected JSON object".to_string())?,
            ),
        };
        let common_has_banks = common_banks_obj.is_some_and(|o| !o.is_empty());
        let common_anim_bin = if common_has_banks {
            f.bin("/common/anim_banks.bin")?
        } else {
            f.optional_bin("/common/anim_banks.bin")
                .unwrap_or_else(|| Rc::from(Vec::new()))
        };
        let effects = Value::Object(Rc::new(Object::new()));

        let mut common_human_banks: Vec<String> = Vec::new();
        if let Some(banks) = common_banks_obj {
            for k in banks.keys() {
                if k.starts_with("human_") {
                    common_human_banks.push(k.to_string());
                }
            }
        }

        // Load every side named by the mission in authored order.
        let mission_sides_obj = match mission_rec.get("sides") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_object().ok_or_else(|| {
                format!("map {map}: mission '{mission_name}'.sides must be a JSON object")
            })?),
        };
        let mut requested_sides: Vec<(String, Vec<String>)> = Vec::new();
        if let Some(sides_map) = mission_sides_obj {
            for (sname, list) in sides_map.entries() {
                let arr = list.as_array().ok_or_else(|| {
                    format!(
                        "map {map}: mission '{mission_name}'.sides.{sname} must be a JSON array"
                    )
                })?;
                let mut pkgs = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    let pkg = item.as_str().ok_or_else(|| {
                        format!(
                            "map {map}: mission '{mission_name}'.sides.{sname}[{i}] must be a string"
                        )
                    })?;
                    pkgs.push(pkg.to_string());
                }
                requested_sides.push((sname.to_string(), pkgs));
            }
        }
        if requested_sides.is_empty() {
            let fallback = if query.get("side").is_empty() { "rep" } else { query.get("side") };
            requested_sides.push((fallback.to_ascii_lowercase(), Vec::new()));
        }

        let mut raw_sides: Vec<SideFiles> = Vec::new();
        let mut side_anim_banks: Vec<(String, Value, Rc<[u8]>)> = Vec::new();
        let mut unit_anim_banks: Vec<(String, Vec<String>)> = Vec::new();
        let mut unit_clip_names: Vec<(String, HashMap<String, Vec<String>>)> = Vec::new();
        let mut roster = RosterReport {
            mission: mission_name.clone(),
            ..Default::default()
        };

        for (sname, pkgs) in &requested_sides {
            for pkg in pkgs {
                roster.requested_packages.push((sname.clone(), pkg.clone()));
            }
            let dir = format!("/sides/{sname}");
            let units_path = format!("{dir}/units.json");
            let raw_units_file = f.json(&units_path)?;
            let raw_units_obj = raw_units_file
                .as_object()
                .ok_or_else(|| format!("{units_path}: expected JSON object"))?;
            let raw_units_map = raw_units_obj
                .get("units")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{units_path}: missing or non-object 'units' table"))?;
            let classes_path = format!("{dir}/classes.json");
            let side_classes = f.optional_json_strict(&classes_path)?;
            if let Some(ref sc) = side_classes {
                if sc.as_object().is_none() {
                    return Err(format!("{classes_path}: expected JSON object"));
                }
            }
            let weapons_path = format!("{dir}/weapons.json");
            let weapons = f
                .optional_json_strict(&weapons_path)?
                .unwrap_or_else(|| Value::Object(Rc::new(Object::new())));
            if weapons.as_object().is_none() {
                return Err(format!("{weapons_path}: expected JSON object"));
            }
            let weapon_buf = f
                .optional_bin(&format!("{dir}/weapons.bin"))
                .unwrap_or_else(|| Rc::from(Vec::new()));
            let weapon_index_buf = f
                .optional_bin(&format!("{dir}/weapons_index.bin"))
                .unwrap_or_else(|| Rc::from(Vec::new()));
            let s_anim_path = format!("{dir}/anim_banks.json");
            let s_anim_json = f
                .optional_json_strict(&s_anim_path)?
                .unwrap_or_else(|| Value::Object(Rc::new(Object::new())));
            let s_anim_obj = s_anim_json
                .as_object()
                .ok_or_else(|| format!("{s_anim_path}: expected JSON object"))?;
            let side_banks_obj = match s_anim_obj.get("banks") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_object()
                        .ok_or_else(|| format!("{s_anim_path}.banks: expected JSON object"))?,
                ),
            };
            let side_has_banks = side_banks_obj.is_some_and(|o| !o.is_empty());
            let s_anim_bin = if side_has_banks {
                f.bin(&format!("{dir}/anim_banks.bin"))?
            } else {
                f.optional_bin(&format!("{dir}/anim_banks.bin"))
                    .unwrap_or_else(|| Rc::from(Vec::new()))
            };

            // Materialize 2005 unit ODF properties + weapon/ordnance numbers.
            let mut materialized_units = Object::new();

            {
                let um = raw_units_map;
                // Keep mission-requested classes (or all side units when the
                // mission did not list individual packages).
                let selected_names = swbf2_spawn_classes(um, pkgs, &query, |pkg| {
                    roster.non_unit_packages.push((
                        sname.clone(), pkg.to_string(),
                        "model/auxiliary package not present in side units.json".into(),
                    ));
                });

                for cname in selected_names {
                    let Some(raw_def) = um.get(&cname) else { continue };
                    validate_swbf2_unit_strict(raw_def, &format!("{units_path}: unit '{cname}'"))?;
                    let infantry = matches!(raw_def.get("base").and_then(Value::as_str), Some("soldier" | "walkerdroid"));
                    let def = swbf2_unit_definition_with_classes(raw_def, infantry, side_classes.as_ref());
                    let base = def
                        .get("base")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let model = def
                        .get("model")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let has_actor = def
                        .get("actor")
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.is_empty());
                    let weapons_len = def
                        .get("weapons")
                        .and_then(Value::as_array)
                        .map(|a| a.len())
                        .unwrap_or(0);

                    let support = classify_swbf2_class(&def, &base, has_actor);

                    roster.classes.push(ClassEntry {
                        side: sname.clone(),
                        class: cname.clone(),
                        base,
                        model,
                        has_actor,
                        weapons: weapons_len,
                        anim_banks: Vec::new(),
                        support,
                    });
                    materialized_units.set(Rc::from(cname.as_str()), def);
                }
            }

            let mut units_file_obj = raw_units_file.as_object().cloned().unwrap_or_default();
            let units_val = Value::Object(Rc::new(materialized_units));
            units_file_obj.set(Rc::from("units"), units_val.clone());
            let units_file = Value::Object(Rc::new(units_file_obj));

            side_anim_banks.push((sname.clone(), s_anim_json, s_anim_bin));
            raw_sides.push(SideFiles {
                name: sname.clone(),
                dir,
                units: units_val,
                items: object_or_empty(units_file.get("items")),
                fire_points: object_or_empty(weapons.get("firePoints")),
                units_file,
                weapons,
                weapon_buf,
                weapon_index_buf,
                fx: None,
            });
        }

        // Enrich placed armedbuilding instances with weapon definitions from
        // loaded sides (e.g. `sides/tur/units.json`) when matching by class.
        let props = enrich_placed_turret_weapons(map_admission.props.as_ref(), &raw_sides);

        // Synthesize `world.mission` team rosters from loaded simulable/partial
        // soldier classes so headless simulation admits teams cleanly without
        // attempting to spawn non-infantry classes as soldiers.
        let target = query.get("practiceTarget");
        let practice = if target.is_empty() { None } else {
            let player = query.get("unit");
            let team = |name: &str| roster.classes.iter()
                .find(|entry| entry.class == name && entry.base == "soldier" && entry.has_actor)
                .map(|entry| team_for_swbf2_side(&entry.side));
            match (team(player), team(target)) {
                (Some(1), Some(2)) | (Some(2), Some(1)) => Some([player, target]),
                _ => return Err("practice requires two installed infantry classes on opposing teams".into()),
            }
        };
        let synthesized_mission = synthesize_swbf2_mission(
            &mission_name,
            &mission_rec,
            &raw_sides,
            &roster,
            practice,
        );
        let mut world_obj = map_admission.world.as_object().cloned().unwrap_or_default();
        world_obj.set(Rc::from("mission"), synthesized_mission.clone());
        if let Some(sky) = mission_rec.get("sky").filter(|v| v.truthy()) {
            world_obj.set(Rc::from("missionSky"), sky.clone());
        }
        let world = Value::Object(Rc::new(world_obj));

        let mut policy = spawn_policy(&query, Some(&synthesized_mission));
        // If spawn_policy fell back to the 2004 default unit, pick a loaded
        // soldier class in the mission instead.
        let has_class = |side: &str, cls: &str| {
            raw_sides
                .iter()
                .find(|s| s.name == side)
                .and_then(|s| s.units.get(cls))
                .is_some_and(|d| d.get("base").and_then(Value::as_str) == Some("soldier"))
        };
        if !has_class(&policy.side, &policy.class_name) {
            let mut infantry = roster.classes.iter().filter(|c| c.base == "soldier" && c.has_actor);
            if let Some(first) = infantry.clone().find(|c| c.class == query.get("unit")).or_else(|| infantry.next())
            {
                policy.side = first.side.clone();
                policy.class_name = first.class.clone();
                policy.team = crate::mission::team_of_class(Some(&synthesized_mission), &first.class);
            } else {
                return Err(format!("map {map} mission {mission_name}: no simulable soldier class found"));
            }
        }

        // Put the played side first, matching 2004 `Battle.sides[0]` convention.
        let mut sides = Vec::with_capacity(raw_sides.len());
        if let Some(pos) = raw_sides.iter().position(|s| s.name == policy.side) {
            sides.push(raw_sides.remove(pos));
        }
        sides.extend(raw_sides);

        let side_name = policy.side.clone();
        let unit = policy.class_name.clone();
        let soldier_def = sides
            .iter()
            .find(|s| s.name == side_name)
            .and_then(|s| s.units.get(&unit))
            .cloned()
            .ok_or_else(|| format!("{unit} isn't in /sides/{side_name}"))?;

        // Load actor bodies for every loaded class that names an actor file,
        // canonicalizing `dummyroot` -> `DummyRoot` on 2005 actors.
        let mut bodies: Vec<(String, BodyFiles)> = Vec::new();
        let mut load_actor_for = |f: &mut Fetch<'_, '_>, sname: &str, sdir: &str, def: &Value| -> Result<(), String> {
            let Some(actor_rel) = def.get("actor").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
                return Ok(());
            };
            let model = str_or(def.get("model"), "undefined");
            let key = format!("{sname}/{model}");
            if bodies.iter().any(|(k, _)| *k == key) {
                return Ok(());
            }
            let stem = actor_rel.strip_suffix(".json").unwrap_or(actor_rel);
            let base_path = format!("{sdir}/{stem}");
            let actor_json_path = format!("{base_path}.json");
            let raw_actor = f.json(&actor_json_path)?;
            let actor_obj = raw_actor
                .as_object()
                .ok_or_else(|| format!("{actor_json_path}: expected JSON object"))?;
            let bones_val = actor_obj
                .get("bones")
                .ok_or_else(|| format!("{actor_json_path}: missing bones array"))?;
            crate::map::validate_skeleton_strict(bones_val, &format!("{actor_json_path}.bones"))?;
            let actor_def = canonicalize_swbf2_actor_bones(&raw_actor);
            let buf = f.bin(&format!("{base_path}.bin"))?;
            let index_buf = f.bin(&format!("{base_path}_index.bin"))?;
            bodies.push((key, BodyFiles { def: actor_def, buf, index_buf }));
            Ok(())
        };
        load_actor_for(&mut f, &side_name, &format!("/sides/{side_name}"), &soldier_def)?;
        for side in &sides {
            let Some(units) = side.units.as_object() else { continue };
            for def in units.values() {
                load_actor_for(&mut f, &side.name, &side.dir, def)?;
            }
        }

        // Resolve per-unit animation banks using each loaded actor's skeleton so
        // non-human rigs (e.g. four-armed rigs with their own complete bank) do
        // not inherit incompatible `common/human_*` banks, and low-res `*lz`
        // banks do not overwrite high-res clips.
        for entry in &mut roster.classes {
            let Some(def) = sides
                .iter()
                .find(|s| s.name == entry.side)
                .and_then(|s| s.units.get(&entry.class))
            else {
                continue;
            };
            let s_anim_json = side_anim_banks
                .iter()
                .find(|(sn, _, _)| sn == &entry.side)
                .map(|(_, meta, _)| meta);
            let Some(s_anim_json) = s_anim_json else {
                continue;
            };
            let side_bank_keys: Vec<String> = s_anim_json
                .get("banks")
                .and_then(Value::as_object)
                .map(|o| o.keys().map(str::to_string).collect())
                .unwrap_or_default();
            let actor_bones: Vec<String> = entry
                .model
                .as_deref()
                .and_then(|m| {
                    let bkey = format!("{}/{m}", entry.side);
                    bodies.iter().find(|(k, _)| *k == bkey)
                })
                .and_then(|(_, b)| b.def.get("bones"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|bone| bone.get("name").and_then(Value::as_str).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let (banks, clip_names) = resolve_swbf2_unit_banks(
                &entry.side,
                &entry.class,
                def,
                &entry.base,
                &actor_bones,
                &common_human_banks,
                s_anim_json,
                &side_bank_keys,
            )?;
            let key = format!("{}/{}", entry.side, entry.class);
            unit_anim_banks.push((key.clone(), banks.clone()));
            if !clip_names.is_empty() {
                unit_clip_names.push((key, clip_names));
            }
            entry.anim_banks = banks;
        }

        let class_data = sides.iter().map(|s| (s.name.clone(), None)).collect();
        let swbf2 = Rc::new(Swbf2Battle {
            map_admission: Rc::clone(&map_admission),
            common_anim_banks: common_anim_banks.clone(),
            common_anim_bin: common_anim_bin.clone(),
            side_anim_banks,
            unit_anim_banks,
            unit_clip_names,
            roster,
        });

        Ok(Battle {
            family: GameFamily::Swbf2,
            map: map.to_string(),
            query_text: query_text.to_string(),
            query,
            map_data: map_admission.map_data.clone(),
            anim_meta: common_anim_banks,
            common,
            effects,
            world,
            policy,
            side: side_name,
            unit,
            soldier_def,
            anim_buf: common_anim_bin,
            coll_buf: Rc::from(Vec::new()),
            coll_index_buf: Rc::from(Vec::new()),
            coll_poly_buf: Rc::from(Vec::new()),
            ground_buf: Rc::from(Vec::new()),
            ground_index_buf: Rc::from(Vec::new()),
            side_fx: None,
            props,
            props_buf: map_admission.props_buf.clone(),
            props_index_buf: map_admission.props_index_buf.clone(),
            props_coll_buf: map_admission.props_coll_buf.clone(),
            props_coll_index_buf: map_admission.props_coll_index_buf.clone(),
            props_coll_poly_buf: map_admission.props_coll_poly_buf.clone(),
            props_anim_buf: map_admission.props_anim_buf.clone(),
            vehicle_dir: format!("/maps/{map}/vehicles"),
            vehicle_def: None,
            vehicle_bufs: None,
            sides,
            bodies,
            walkers: Vec::new(),
            nav: None,
            ai_mission: None,
            class_data,
            nav_bin: None,
            fetched: f.log,
            swbf2: Some(swbf2),
        })
    }

    /// The mission block (`world.mission`), when the battle has one.
    pub fn mission(&self) -> Option<&Value> {
        self.world.get("mission").filter(|m| !m.is_null())
    }

    pub fn side(&self, name: &str) -> Option<&SideFiles> {
        self.sides.iter().find(|s| s.name == name)
    }

    /// The 2005 roster report, when loaded with `GameFamily::Swbf2`.
    pub fn roster_report(&self) -> Option<&RosterReport> {
        self.swbf2.as_ref().map(|s| &s.roster)
    }

    /// Raw bytes of the 2005 dynamic planning-group delta table, when declared
    /// by a `GameFamily::Swbf2` map (`planningGroups.data`). Always `None` for 2004.
    pub fn swbf2_planning_bin(&self) -> Option<&Rc<[u8]>> {
        self.swbf2
            .as_ref()
            .and_then(|s| s.map_admission.planning_bin.as_ref())
    }
}

fn parse_vec_array(text: &str, expected: usize) -> Option<Value> {
    let nums: Vec<f64> = text
        .split_whitespace()
        .filter_map(|p| p.parse::<f64>().ok().filter(|n| n.is_finite()))
        .collect();
    if nums.len() >= expected {
        Some(Value::Array(Rc::new(
            nums.into_iter().take(expected).map(Value::Number).collect(),
        )))
    } else {
        None
    }
}

fn parse_camera_sections(pairs: &[(String, String)]) -> Option<Value> {
    let mut sections: Vec<(String, Object)> = Vec::new();
    let mut current: Option<usize> = None;
    for (k, v) in pairs {
        if k.eq_ignore_ascii_case("CAMERASECTION") {
            let name = v.trim().to_ascii_uppercase();
            if name.is_empty() {
                current = None;
                continue;
            }
            if let Some(pos) = sections.iter().position(|(n, _)| *n == name) {
                current = Some(pos);
            } else {
                sections.push((name, Object::new()));
                current = Some(sections.len() - 1);
            }
            continue;
        }
        let Some(idx) = current else { continue };
        if matches!(k.as_str(), "EyePointOffset" | "TrackCenter" | "TrackOffset") {
            if let Some(vec3) = parse_vec_array(v, 3) {
                sections[idx].1.set(Rc::from(k.as_str()), vec3);
            }
        } else if k == "TiltValue" {
            sections[idx].1.set(Rc::from("TiltValue"), parse_odf_scalar(v));
        }
    }
    if sections.is_empty() {
        return None;
    }
    let written = sections
        .into_iter()
        .map(|(n, o)| (Rc::from(n.as_str()), Value::Object(Rc::new(o))))
        .collect();
    Some(Value::Object(Rc::new(Object::from_written(written))))
}

const ODF_KEEP_RAW_KEYS: &[&str] = &[
    "class", "base", "parents", "properties", "displayName", "weapon", "model",
    "actor", "combo", "comboBindings", "saberHilts", "clipTiming",
    "stateAnimations", "unresolvedAnimations", "missingMachine",
    "unresolvedReferences", "unresolvedParents", "HUDTag", "GeometryName",
    "HighResGeometry", "FirePointName", "OrdnanceName", "SecondaryOrdnanceName",
    "AnimationBank", "ComboAnimationBank", "LaserTexture", "ScopeTexture",
    "Effect", "WaterEffect", "ImpactEffect", "ImpactEffectTerrain",
    "ImpactEffectWater", "SoundProperty", "LightColor",
];

fn coerce_odf_scalar_value(v: &Value) -> Value {
    match v {
        Value::String(s) => parse_odf_scalar(s),
        _ => v.clone(),
    }
}

/// Applies ordered ODF property aliases matching `tools/zero_weapons.py`
/// (`ALIASES` / `ITEM_ALIASES`): walks `properties` in authored root-to-child
/// order so the later of any aliased pair wins, and falls back to object fields
/// when `properties` is absent.
fn apply_scalar_aliases(
    out: &mut Object,
    pairs: &[(String, String)],
    aliases: &[(&str, &[&str])],
) {
    if !pairs.is_empty() {
        let mut tracked: Vec<&str> = Vec::new();
        for (src, targets) in aliases {
            tracked.push(*src);
            for t in *targets {
                if !tracked.contains(t) {
                    tracked.push(*t);
                }
            }
        }
        for (k, raw) in pairs {
            let mut matched = false;
            for (src, targets) in aliases {
                if k == *src {
                    let val = parse_odf_scalar(raw);
                    for t in *targets {
                        out.set(Rc::from(*t), val.clone());
                    }
                    matched = true;
                    break;
                }
            }
            if !matched && tracked.contains(&k.as_str()) {
                out.set(Rc::from(k.as_str()), parse_odf_scalar(raw));
            }
        }
    } else {
        for (src, targets) in aliases {
            if let Some(val) = out.get(src).cloned().filter(|v| !v.is_null()) {
                for t in *targets {
                    if out.get(t).is_none_or(Value::is_null) {
                        out.set(Rc::from(*t), val.clone());
                    }
                }
            }
        }
    }
}

/// Coerces numeric strings in a nested 2005 explosion object and applies the
/// same radius and health/armor scale aliases as `tools/zero_weapons.py`
/// (`DamageRadius` -> `DamageRadiusOuter`, `HealthScale` -> `PersonScale` /
/// `AnimalScale` / `DroidScale`, `ArmorScale` -> `VehicleScale` / `BuildingScale`).
fn coerce_swbf2_explosion(val: &Value) -> Value {
    let Some(obj) = val.as_object() else {
        return val.clone();
    };
    let mut out = Object::new();
    for (k, v) in obj.entries() {
        let converted = if ODF_KEEP_RAW_KEYS.contains(&k) {
            v.clone()
        } else {
            coerce_odf_scalar_value(v)
        };
        out.set(Rc::from(k), converted);
    }
    let pairs = read_property_pairs(obj.get("properties"));
    const EXP_ALIASES: &[(&str, &[&str])] = &[
        ("DamageRadius", &["DamageRadiusOuter"]),
        ("ShakeRadius", &["ShakeRadiusOuter"]),
        ("PushRadius", &["PushRadiusOuter"]),
        ("HealthScale", &["PersonScale", "AnimalScale", "DroidScale"]),
        ("ArmorScale", &["VehicleScale", "BuildingScale"]),
    ];
    apply_scalar_aliases(&mut out, &pairs, EXP_ALIASES);
    Value::Object(Rc::new(out))
}

/// Resolves which explosion property key wins in authored `properties` order
/// among aliases (`sources`) and the canonical simulation field (`target`).
fn alias_explosion_object(
    out: &mut Object,
    pairs: &[(String, String)],
    sources: &[&str],
    target: &str,
) {
    let mut chosen_key: Option<&str> = None;
    for (k, _) in pairs {
        if k == target && out.get(target).is_some_and(|v| v.as_object().is_some()) {
            chosen_key = Some(target);
        } else if let Some(&src) = sources.iter().find(|&&s| s == k.as_str()) {
            if out.get(src).is_some_and(|v| v.as_object().is_some()) {
                chosen_key = Some(src);
            }
        }
    }
    if let Some(key) = chosen_key {
        if key != target {
            if let Some(exp) = out.get(key).cloned() {
                out.set(Rc::from(target), exp);
            }
        }
        return;
    }
    if out.get(target).is_none_or(|v| v.as_object().is_none()) {
        for &src in sources {
            if let Some(exp) = out.get(src).filter(|v| v.as_object().is_some()).cloned() {
                out.set(Rc::from(target), exp);
                break;
            }
        }
    }
}

/// Coerces numeric fields and nested explosions on a 2005 ordnance object.
///
/// Matches `tools/zero_weapons.py`:
/// - `Explosion` / `ExplosionName` alias `ExplosionImpact` in authored order.
/// - `Damage` and `MaxDamage` alias each other on non-`emitterordnance`
///   projectiles with the later key in authored `properties` order winning;
///   `emitterordnance` keeps `Damage` (no charge) and `MaxDamage` (full charge)
///   separate.
/// - Note: distance/time damage falloff (`DamageFinalDamage`,
///   `DamageTransitionDelay`, `DamageTransitionPeriod`) is not modeled yet.
fn coerce_swbf2_ordnance(val: &Value) -> Value {
    let Some(obj) = val.as_object() else {
        return val.clone();
    };
    let mut out = Object::new();
    for (k, v) in obj.entries() {
        let converted = if ODF_KEEP_RAW_KEYS.contains(&k) {
            v.clone()
        } else if matches!(
            k,
            "explosion"
                | "Explosion"
                | "ExplosionName"
                | "ExplosionImpact"
                | "ExplosionExpire"
                | "ExplosionTrigger"
                | "ExplosionDeath"
                | "secondaryExplosion"
                | "DetonateExplosion"
                | "FinalExplosion"
        ) {
            coerce_swbf2_explosion(v)
        } else {
            coerce_odf_scalar_value(v)
        };
        out.set(Rc::from(k), converted);
    }
    let pairs = read_property_pairs(obj.get("properties"));
    let is_emitter = out.get("base").and_then(Value::as_str) == Some("emitterordnance");
    alias_explosion_object(&mut out, &pairs, &["Explosion", "ExplosionName", "explosion"], "ExplosionImpact");
    if !is_emitter {
        const ORD_ALIASES: &[(&str, &[&str])] = &[
            ("MaxDamage", &["Damage"]),
            ("HealthScale", &["PersonScale", "AnimalScale", "DroidScale"]),
            ("ArmorScale", &["VehicleScale", "BuildingScale"]),
        ];
        apply_scalar_aliases(&mut out, &pairs, ORD_ALIASES);
    }
    Value::Object(Rc::new(out))
}

/// Coerces a 2005 item (mine / detpack / powerupitem) for `weapon.item`,
/// applying `ITEM_ALIASES` (`Explosion` / `ExplosionName` -> `ExplosionTrigger`,
/// `SoldierHealth` -> `PersonHealth`/`DroidHealth`, `SoldierAmmo` ->
/// `PersonAmmo`/`DroidAmmo`) and coercing nested explosions.
fn coerce_swbf2_item(val: &Value) -> Value {
    let Some(obj) = val.as_object() else {
        return val.clone();
    };
    let mut out = Object::new();
    for (k, v) in obj.entries() {
        let converted = if ODF_KEEP_RAW_KEYS.contains(&k) {
            v.clone()
        } else if matches!(
            k,
            "explosion"
                | "Explosion"
                | "ExplosionName"
                | "ExplosionTrigger"
                | "ExplosionExpire"
                | "ExplosionDeath"
        ) {
            coerce_swbf2_explosion(v)
        } else {
            coerce_odf_scalar_value(v)
        };
        out.set(Rc::from(k), converted);
    }
    let pairs = read_property_pairs(obj.get("properties"));
    alias_explosion_object(&mut out, &pairs, &["Explosion", "ExplosionName", "explosion"], "ExplosionTrigger");
    const ITEM_ALIASES: &[(&str, &[&str])] = &[
        ("SoldierHealth", &["PersonHealth", "DroidHealth"]),
        ("SoldierAmmo", &["PersonAmmo", "DroidAmmo"]),
    ];
    apply_scalar_aliases(&mut out, &pairs, ITEM_ALIASES);
    Value::Object(Rc::new(out))
}

const SWBF2_EXPLOSION_KEYS: &[&str] = &[
    "explosion",
    "Explosion",
    "ExplosionName",
    "ExplosionImpact",
    "ExplosionExpire",
    "ExplosionTrigger",
    "ExplosionDeath",
    "secondaryExplosion",
    "DetonateExplosion",
    "FinalExplosion",
];

fn lookup_case_insensitive<'a>(table: &'a Object, name: &str) -> Option<&'a Value> {
    if let Some(v) = table.get(name) {
        return Some(v);
    }
    table
        .entries()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

fn resolve_swbf2_class_entry(table: &Object, name: &str) -> Option<(String, Vec<(String, String)>)> {
    let mut chain: Vec<&Object> = Vec::new();
    let mut visited: Vec<String> = Vec::new();
    let mut current = name.to_string();
    let mut primitive_base = String::new();
    for _ in 0..16 {
        let lower = current.to_ascii_lowercase();
        if visited.iter().any(|v| v == &lower) {
            break;
        }
        visited.push(lower);
        let Some(entry) = lookup_case_insensitive(table, &current).and_then(Value::as_object) else {
            break;
        };
        chain.push(entry);
        let next_base = entry.get("base").and_then(Value::as_str).unwrap_or("");
        if next_base.is_empty() {
            break;
        }
        if lookup_case_insensitive(table, next_base).is_some() {
            current = next_base.to_string();
        } else {
            primitive_base = next_base.to_string();
            break;
        }
    }
    if chain.is_empty() {
        return None;
    }
    let mut pairs = Vec::new();
    for entry in chain.into_iter().rev() {
        pairs.extend(read_property_pairs(entry.get("properties")));
    }
    Some((primitive_base, pairs))
}

fn pairs_to_value(pairs: &[(String, String)]) -> Value {
    Value::Array(Rc::new(
        pairs
            .iter()
            .map(|(k, v)| Value::Array(Rc::new(vec![Value::str(k), Value::str(v)])))
            .collect(),
    ))
}

fn materialize_swbf2_explosion_from_classes(classes: &Value, name: &str) -> Option<Value> {
    let expc = classes.get("expc").and_then(Value::as_object)?;
    let (base, pairs) = resolve_swbf2_class_entry(expc, name)?;
    let mut obj = Object::new();
    obj.set(Rc::from("class"), Value::str(name));
    if !base.is_empty() {
        obj.set(Rc::from("base"), Value::str(&base));
    }
    obj.set(Rc::from("properties"), pairs_to_value(&pairs));
    for (k, v) in &pairs {
        obj.set(Rc::from(k.as_str()), Value::str(v));
    }
    Some(coerce_swbf2_explosion(&Value::Object(Rc::new(obj))))
}

fn materialize_swbf2_ordnance_from_classes(classes: &Value, name: &str) -> Option<Value> {
    let ordc = classes.get("ordc").and_then(Value::as_object)?;
    let (base, pairs) = resolve_swbf2_class_entry(ordc, name)?;
    let mut obj = Object::new();
    obj.set(Rc::from("class"), Value::str(name));
    if !base.is_empty() {
        obj.set(Rc::from("base"), Value::str(&base));
    }
    obj.set(Rc::from("properties"), pairs_to_value(&pairs));
    for (k, v) in &pairs {
        if SWBF2_EXPLOSION_KEYS.contains(&k.as_str()) {
            if let Some(exp) = materialize_swbf2_explosion_from_classes(classes, v) {
                obj.set(Rc::from(k.as_str()), exp);
                continue;
            }
        }
        obj.set(Rc::from(k.as_str()), Value::str(v));
    }
    Some(coerce_swbf2_ordnance(&Value::Object(Rc::new(obj))))
}

fn parse_pitch_yaw_pairs(pairs: &[(String, String)]) -> Vec<Value> {
    pairs
        .iter()
        .filter(|(k, _)| k == "ShotPatternPitchYaw")
        .filter_map(|(_, v)| parse_vec_array(v, 2))
        .collect()
}

fn resolve_swbf2_explosion_keys(obj: &Object, side_classes: Option<&Value>, keys: &[&str]) -> Option<Value> {
    for &key in keys {
        if let Some(exp_val) = obj.get(key) {
            if exp_val.as_object().is_some() {
                return Some(coerce_swbf2_explosion(exp_val));
            }
            if let Some(exp_name) = exp_val.as_str().filter(|s| !s.is_empty()) {
                if let Some(exp) = side_classes.and_then(|sc| materialize_swbf2_explosion_from_classes(sc, exp_name)) {
                    return Some(exp);
                }
            }
        }
    }
    None
}

fn coerce_swbf2_guided_missile(val: &Value, side_classes: Option<&Value>) -> Value {
    let Some(obj) = val.as_object() else {
        return val.clone();
    };
    let mut out = Object::new();
    for (k, v) in obj.entries() {
        if k == "base" {
            out.set(Rc::from("base"), Value::str("droid"));
            continue;
        }
        if matches!(k, "EyePointOffset" | "TrackCenter" | "TrackOffset") {
            if let Some(vec3) = v.as_str().and_then(|s| parse_vec_array(s, 3)) {
                out.set(Rc::from(k), vec3);
                continue;
            }
        }
        let converted = if ODF_KEEP_RAW_KEYS.contains(&k) {
            v.clone()
        } else {
            coerce_odf_scalar_value(v)
        };
        out.set(Rc::from(k), converted);
    }
    let critical = resolve_swbf2_explosion_keys(obj, side_classes, &["ExplosionCritical", "e04f08c1"]);
    let destruct = resolve_swbf2_explosion_keys(
        obj,
        side_classes,
        &["ExplosionDestruct", "8a323f32", "ExplosionName", "Explosion"],
    );
    if let Some(exp) = critical.as_ref() {
        out.set(Rc::from("explosionCritical"), exp.clone());
    }
    if out.get("explosion").is_none_or(|v| v.as_object().is_none()) {
        if let Some(exp) = destruct.or(critical) {
            out.set(Rc::from("explosion"), exp);
        }
    }
    Value::Object(Rc::new(out))
}

fn coerce_odf_object_numbers_with_classes(
    val: &Value,
    side_classes: Option<&Value>,
    allow_guided_droid: bool,
) -> Value {
    let Some(obj) = val.as_object() else {
        return val.clone();
    };
    let weapon_base = obj.get("base").and_then(Value::as_str).unwrap_or("");
    let ord_base = obj
        .get("ordnance")
        .and_then(|o| o.get("base"))
        .and_then(Value::as_str)
        .unwrap_or("");
    // 2005 dispenser and detonator weapons place their mine/item record under
    // `ordnance` instead of `item`, whereas `Defs::build` and `ItemRows` read
    // `weapon.item`. Promote mine/powerupitem ordnance to `weapon.item` when
    // `item` is absent, matching `tools/zero_weapons.py`.
    let promote_to_item = obj.get("item").is_none_or(Value::is_null)
        && obj.get("ordnance").is_some_and(|v| v.as_object().is_some())
        && (matches!(weapon_base, "dispenser" | "detonator")
            || matches!(ord_base, "mine" | "powerupitem"));
    // 2005 infantry guided-rocket remote weapons store their controllable
    // flyer under `ordnance` with `GuidedMissile = 1`, whereas the infantry
    // remote/recon runtime reads `weapon.droid`. Craft weapons keep `ordnance`
    // intact so the space flight module builds their guided rockets from it.
    let promote_to_droid = allow_guided_droid
        && !promote_to_item
        && weapon_base == "remote"
        && obj.get("droid").is_none_or(Value::is_null)
        && obj.get("ordnance").is_some_and(|o| {
            o.get("GuidedMissile")
                .map(coerce_odf_scalar_value)
                .and_then(|v| v.as_f64())
                .is_some_and(|n| n != 0.0 && n.is_finite())
        });

    let mut out = Object::new();
    for (k, v) in obj.entries() {
        if promote_to_item && k == "ordnance" {
            out.set(Rc::from("item"), coerce_swbf2_item(v));
            continue;
        }
        if promote_to_droid && k == "ordnance" {
            out.set(Rc::from("droid"), coerce_swbf2_guided_missile(v, side_classes));
            continue;
        }
        if k == "FirePointExtraOffset" {
            if let Some(vec3) = v.as_str().and_then(|s| parse_vec_array(s, 3)) {
                out.set(Rc::from(k), vec3);
                continue;
            }
        }
        let converted = if ODF_KEEP_RAW_KEYS.contains(&k) {
            v.clone()
        } else {
            match k {
                "ordnance" | "secondaryOrdnance" => coerce_swbf2_ordnance(v),
                "item" => coerce_swbf2_item(v),
                "explosion" | "Explosion" | "secondaryExplosion" | "ExplosionName"
                | "ExplosionImpact" | "ExplosionExpire" | "ExplosionTrigger"
                | "ExplosionDeath" | "DetonateExplosion" | "FinalExplosion" => {
                    coerce_swbf2_explosion(v)
                }
                _ => coerce_odf_scalar_value(v),
            }
        };
        out.set(Rc::from(k), converted);
    }
    let pairs = read_property_pairs(obj.get("properties"));
    const WEAPON_ALIASES: &[(&str, &[&str])] = &[
        ("KickSpread", &["SpreadPerShot"]),
        ("SpreadRecover", &["SpreadRecoverRate"]),
    ];
    apply_scalar_aliases(&mut out, &pairs, WEAPON_ALIASES);

    let first_charge = pairs.iter().position(|(k, _)| k == "NextCharge");
    if let Some(split_at) = first_charge {
        let base_pairs = &pairs[..split_at];
        let mut tiers: Vec<(f64, Vec<(String, String)>)> = Vec::new();
        for (k, v) in &pairs[split_at..] {
            if k == "NextCharge" {
                let at = v.trim().parse::<f64>().ok().filter(|n| n.is_finite()).unwrap_or(0.0);
                tiers.push((at, Vec::new()));
            } else if let Some((_, tier_pairs)) = tiers.last_mut() {
                tier_pairs.push((k.clone(), v.clone()));
            }
        }

        let flattened_ord = out.get("ordnance").cloned();
        let flattened_ord_class = flattened_ord
            .as_ref()
            .and_then(|o| o.get("class"))
            .and_then(Value::as_str)
            .map(str::to_string);

        // Restore base-tier scalar keys overwritten by later NextCharge tiers.
        let tier_keys: Vec<&str> = tiers
            .iter()
            .flat_map(|(_, tp)| tp.iter().map(|(k, _)| k.as_str()))
            .collect();
        for (k, v) in base_pairs {
            if k == "ShotPatternPitchYaw" || k == "OrdnanceName" || !tier_keys.contains(&k.as_str()) {
                continue;
            }
            let val = if ODF_KEEP_RAW_KEYS.contains(&k.as_str()) {
                Value::str(v)
            } else {
                parse_odf_scalar(v)
            };
            out.set(Rc::from(k.as_str()), val);
        }
        apply_scalar_aliases(&mut out, base_pairs, WEAPON_ALIASES);

        let base_pat = parse_pitch_yaw_pairs(base_pairs);
        if !base_pat.is_empty() {
            out.set(Rc::from("ShotPatternPitchYaw"), Value::Array(Rc::new(base_pat.clone())));
        }

        if let Some((_, base_ord_name)) = base_pairs.iter().rev().find(|(k, _)| k == "OrdnanceName") {
            out.set(Rc::from("OrdnanceName"), Value::str(base_ord_name));
            let matches_flattened = flattened_ord_class
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(base_ord_name));
            if !matches_flattened {
                if let Some(resolved) = side_classes
                    .and_then(|sc| materialize_swbf2_ordnance_from_classes(sc, base_ord_name))
                {
                    out.set(Rc::from("ordnance"), resolved);
                }
            }
        }

        let mut charges = Vec::with_capacity(tiers.len());
        for (at, tier_pairs) in tiers {
            let mut tier_obj = Object::new();
            tier_obj.set(Rc::from("at"), Value::Number(at));
            for (k, v) in &tier_pairs {
                if k == "ShotPatternPitchYaw" {
                    continue;
                }
                let val = if ODF_KEEP_RAW_KEYS.contains(&k.as_str()) {
                    Value::str(v)
                } else {
                    parse_odf_scalar(v)
                };
                tier_obj.set(Rc::from(k.as_str()), val);
            }
            apply_scalar_aliases(&mut tier_obj, &tier_pairs, WEAPON_ALIASES);

            let tier_pat = parse_pitch_yaw_pairs(&tier_pairs);
            if !tier_pat.is_empty() {
                tier_obj.set(Rc::from("ShotPatternPitchYaw"), Value::Array(Rc::new(tier_pat)));
            } else if !base_pat.is_empty()
                && (tier_obj.has("ShotPatternCount") || tier_obj.has("ShotsPerSalvo"))
            {
                // Clear inherited multi-bolt fan when a charge tier replaces the
                // pattern count without authoring its own pitch/yaw offsets.
                tier_obj.set(Rc::from("ShotPatternPitchYaw"), Value::Array(Rc::new(Vec::new())));
            }

            if let Some((_, tier_ord_name)) = tier_pairs.iter().rev().find(|(k, _)| k == "OrdnanceName") {
                if flattened_ord_class
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(tier_ord_name))
                {
                    if let Some(ref ord) = flattened_ord {
                        tier_obj.set(Rc::from("ordnance"), ord.clone());
                    }
                } else if let Some(resolved) = side_classes
                    .and_then(|sc| materialize_swbf2_ordnance_from_classes(sc, tier_ord_name))
                {
                    tier_obj.set(Rc::from("ordnance"), resolved);
                }
            }

            charges.push(Value::Object(Rc::new(tier_obj)));
        }
        out.set(Rc::from("charges"), Value::Array(Rc::new(charges)));
    } else {
        let pat = parse_pitch_yaw_pairs(&pairs);
        if !pat.is_empty() {
            out.set(Rc::from("ShotPatternPitchYaw"), Value::Array(Rc::new(pat)));
        }
    }

    Value::Object(Rc::new(out))
}

fn validate_swbf2_unit_strict(raw: &Value, label: &str) -> Result<(), String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| format!("{label}: expected JSON object"))?;
    if let Some(m) = obj.get("model").filter(|v| !v.is_null()) {
        if m.as_str().is_none() {
            return Err(format!("{label}.model: expected string"));
        }
    }
    if let Some(a) = obj.get("actor").filter(|v| !v.is_null()) {
        let s = a
            .as_str()
            .ok_or_else(|| format!("{label}.actor: expected path string"))?;
        if !s.is_empty() {
            crate::map::validate_rel_path(s, &format!("{label}.actor"))?;
        }
    }
    if let Some(p) = obj.get("properties").filter(|v| !v.is_null()) {
        if p.as_array().is_none() {
            return Err(format!("{label}.properties: expected JSON array"));
        }
    }
    if let Some(w_val) = obj.get("weapons").filter(|v| !v.is_null()) {
        let weapons = w_val
            .as_array()
            .ok_or_else(|| format!("{label}.weapons: expected JSON array"))?;
        for (wi, w) in weapons.iter().enumerate() {
            let wobj = w
                .as_object()
                .ok_or_else(|| format!("{label}.weapons[{wi}]: expected JSON object"))?;
            if let Some(h) = wobj.get("saberHilts").filter(|v| !v.is_null()) {
                if h.as_array().is_none() {
                    return Err(format!(
                        "{label}.weapons[{wi}].saberHilts: expected JSON array"
                    ));
                }
            }
            if let Some(cb_val) = wobj.get("comboBindings").filter(|v| !v.is_null()) {
                let bindings = cb_val.as_array().ok_or_else(|| {
                    format!("{label}.weapons[{wi}].comboBindings: expected JSON array")
                })?;
                for (bi, b) in bindings.iter().enumerate() {
                    let bobj = b.as_object().ok_or_else(|| {
                        format!("{label}.weapons[{wi}].comboBindings[{bi}]: expected JSON object")
                    })?;
                    if let Some(ct_val) = bobj.get("clipTiming").filter(|v| !v.is_null()) {
                        let ct = ct_val.as_object().ok_or_else(|| {
                            format!(
                                "{label}.weapons[{wi}].comboBindings[{bi}].clipTiming: expected JSON object"
                            )
                        })?;
                        for (act, entry) in ct.entries() {
                            if entry.is_null() {
                                continue;
                            }
                            let eobj = entry.as_object().ok_or_else(|| {
                                format!(
                                    "{label}.weapons[{wi}].comboBindings[{bi}].clipTiming.{act}: expected JSON object"
                                )
                            })?;
                            if let Some(fr) = eobj.get("frames").filter(|v| !v.is_null()) {
                                let n = fr.as_f64().ok_or_else(|| {
                                    format!(
                                        "{label}.weapons[{wi}].comboBindings[{bi}].clipTiming.{act}.frames: expected integer"
                                    )
                                })?;
                                if !n.is_finite()
                                    || n < 0.0
                                    || n.fract() != 0.0
                                    || n > f64::from(u32::MAX)
                                {
                                    return Err(format!(
                                        "{label}.weapons[{wi}].comboBindings[{bi}].clipTiming.{act}.frames: invalid frame count {n}"
                                    ));
                                }
                            }
                            if let Some(bk) = eobj.get("bank").filter(|v| !v.is_null()) {
                                if bk.as_str().is_none() {
                                    return Err(format!(
                                        "{label}.weapons[{wi}].comboBindings[{bi}].clipTiming.{act}.bank: expected string"
                                    ));
                                }
                            }
                            if let Some(cr) = eobj.get("crc").filter(|v| !v.is_null()) {
                                if cr.as_str().is_none() {
                                    return Err(format!(
                                        "{label}.weapons[{wi}].comboBindings[{bi}].clipTiming.{act}.crc: expected string"
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Classes in the order the mission assigns to slots. An empty package
/// list selects all units in UTF-16 order. A folded lookup takes its first match.
pub fn swbf2_class_names(units: &Object, packages: &[String], mut missing: impl FnMut(&str)) -> Vec<String> {
    if packages.is_empty() {
        let mut all: Vec<_> = units.keys().map(str::to_string).collect();
        all.sort_by(|a, b| utf16_cmp(a, b));
        return all;
    }
    let mut classes = Vec::new();
    for pkg in packages {
        let actual = if units.has(pkg) { Some(pkg.as_str()) } else {
            units.keys().find(|k| k.eq_ignore_ascii_case(pkg))
        };
        if let Some(name) = actual {
            if !classes.iter().any(|s| s == name) { classes.push(name.to_string()); }
        } else {
            missing(pkg);
        }
    }
    classes
}

/// Append an explicitly selected infantry class after the authored class slots.
pub fn swbf2_spawn_classes(
    units: &Object, packages: &[String], query: &Query, missing: impl FnMut(&str),
) -> Vec<String> {
    let mut names = swbf2_class_names(units, packages, missing);
    for extra in [query.get("unit"), query.get("practiceTarget")] {
        if units.get(extra).and_then(|unit| unit.get("base")).and_then(Value::as_str) == Some("soldier")
            && !names.iter().any(|name| name == extra)
        {
            names.push(extra.to_owned());
        }
    }
    names
}

/// All prepared weapons, including locked upgrades, participate in content agreement.
pub fn materialize_swbf2_unit(raw: &Value) -> Value {
    swbf2_unit_definition_with_classes(raw, false, None)
}

pub fn materialize_swbf2_unit_with_classes(raw: &Value, side_classes: Option<&Value>) -> Value {
    swbf2_unit_definition_with_classes(raw, false, side_classes)
}

fn starting_infantry_weapon(weapon: &Value) -> bool {
    let channel = weapon.get("channel").filter(|v| !v.is_null());
    let unlock = weapon.get("MedalsTypeToUnlock").filter(|v| !v.is_null());
    channel.is_none_or(|v| matches!(v.as_f64(), Some(0.0 | 1.0)))
        && unlock.is_none_or(|v| v.as_f64() == Some(-1.0))
}

#[cfg(test)]
fn swbf2_unit_definition(raw: &Value, starting_infantry: bool) -> Value {
    swbf2_unit_definition_with_classes(raw, starting_infantry, None)
}

fn swbf2_unit_definition_with_classes(
    raw: &Value,
    starting_infantry: bool,
    side_classes: Option<&Value>,
) -> Value {
    let pairs = read_property_pairs(raw.get("properties"));
    let flat = materialize_odf_properties(&pairs);
    let mut out = raw.as_object().cloned().unwrap_or_default();
    const RESERVED: &[&str] = &[
        "class", "base", "parents", "displayName", "model", "actor", "weapons",
        "properties", "bounds", "cloth", "combo", "explosion", "cameras",
    ];
    for (k, v) in flat.entries() {
        if RESERVED.contains(&k) || out.has(k) {
            continue;
        }
        if k == "PitchLimits" {
            if let Some(s) = v.as_str() {
                if let Some(pair) = parse_vec_array(s, 2) {
                    out.set(Rc::from(k), pair);
                    continue;
                }
            }
        }
        out.set(Rc::from(k), v.clone());
    }
    if !out.has("cameras") {
        if let Some(cams) = parse_camera_sections(&pairs) {
            out.set(Rc::from("cameras"), cams);
        }
    }
    if let Some(weapons) = raw.get("weapons").and_then(Value::as_array) {
        let allow_guided_droid = matches!(
            raw.get("base").and_then(Value::as_str),
            Some("soldier" | "walkerdroid")
        );
        let coerced: Vec<Value> = weapons
            .iter()
            .map(|w| coerce_odf_object_numbers_with_classes(w, side_classes, allow_guided_droid))
            .filter(|weapon| !starting_infantry || starting_infantry_weapon(weapon))
            .collect();
        out.set(Rc::from("weapons"), Value::Array(Rc::new(coerced)));
    }
    Value::Object(Rc::new(out))
}

fn canonicalize_swbf2_actor_bones(actor_def: &Value) -> Value {
    let Some(obj) = actor_def.as_object() else {
        return actor_def.clone();
    };
    let Some(bones) = obj.get("bones").and_then(Value::as_array) else {
        return actor_def.clone();
    };
    let needs_fix = bones.iter().any(|b| {
        b.get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| n.eq_ignore_ascii_case("DummyRoot") && n != "DummyRoot")
    });
    if !needs_fix {
        return actor_def.clone();
    }
    let fixed: Vec<Value> = bones
        .iter()
        .map(|b| {
            if let Some(mut bo) = b.as_object().cloned() {
                if bo
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| n.eq_ignore_ascii_case("DummyRoot"))
                {
                    bo.set(Rc::from("name"), Value::str("DummyRoot"));
                }
                Value::Object(Rc::new(bo))
            } else {
                b.clone()
            }
        })
        .collect();
    let mut out = obj.clone();
    out.set(Rc::from("bones"), Value::Array(Rc::new(fixed)));
    Value::Object(Rc::new(out))
}

/// True when an actor skeleton carries the standard bipedal upper-body bones
/// that `common/anim_banks.json` `human_*` banks drive.
pub(crate) fn has_human_upper_body_bones(bone_names: &[String]) -> bool {
    bone_names.iter().any(|b| {
        b.eq_ignore_ascii_case("bone_l_upperarm")
            || b.eq_ignore_ascii_case("bone_r_upperarm")
            || b.eq_ignore_ascii_case("bone_a_spine")
    })
}

fn is_low_res_bank(bank_name: &str, low_res_skel: Option<&str>) -> bool {
    if let Some(lz) = low_res_skel.filter(|s| !s.is_empty()) {
        if bank_name.eq_ignore_ascii_case(lz) {
            return true;
        }
    }
    bank_name.len() > 2 && bank_name.as_bytes()[bank_name.len() - 2..].eq_ignore_ascii_case(b"lz")
}

type ResolvedSwbf2Banks = (Vec<String>, HashMap<String, Vec<String>>);

#[allow(clippy::too_many_arguments)]
fn resolve_swbf2_unit_banks(
    side_name: &str,
    class_name: &str,
    def: &Value,
    base: &str,
    actor_bones: &[String],
    common_human_banks: &[String],
    side_anim_json: &Value,
    side_bank_keys: &[String],
) -> Result<ResolvedSwbf2Banks, String> {
    let (side_banks, clip_names) = swbf2_side_banks(
        side_name, class_name, def, base, common_human_banks, side_anim_json, side_bank_keys,
    )?;
    // Prepend `common/human_*` banks only for soldiers whose skeleton matches
    // the human upper-body rig (or that have no standalone side bank of their
    // own). Non-human rigs with complete side banks keep only their own banks.
    let mut banks: Vec<String> = Vec::new();
    let use_common_human = base == "soldier"
        && (has_human_upper_body_bones(actor_bones) || side_banks.is_empty());
    if use_common_human {
        for b in common_human_banks {
            if !b.is_empty() && !banks.iter().any(|x| x.eq_ignore_ascii_case(b)) {
                banks.push(b.clone());
            }
        }
    }
    for b in side_banks {
        banks.retain(|name| !name.eq_ignore_ascii_case(&b));
        banks.push(b);
    }
    Ok((banks, clip_names))
}

/// Resolve bank precedence and the ordered action aliases for each clip.
pub fn swbf2_side_banks(
    side_name: &str, class_name: &str, def: &Value, base: &str,
    common_human_banks: &[String], side_anim_json: &Value, side_bank_keys: &[String],
) -> Result<ResolvedSwbf2Banks, String> {
    let push_unique = |list: &mut Vec<String>, name: &str| {
        if !name.is_empty() && !list.iter().any(|b| b.eq_ignore_ascii_case(name)) {
            list.push(name.to_string());
        }
    };
    let find_side_bank = |name: &str| -> Option<&str> {
        side_bank_keys
            .iter()
            .find(|k| k.eq_ignore_ascii_case(name))
            .map(String::as_str)
    };
    let low_res_skel = def.get("SkeletonLowRes").and_then(Value::as_str);

    // Collect banks in priority tiers: explicit AnimationName / CustomAnimationBank
    // highest, then package banks in archive order, then weapon combo common banks.
    // Because `load_clips_by_crc` lets later banks overwrite earlier clips on name
    // collision, we emit lower-priority tiers before higher-priority tiers.
    let mut explicit_banks: Vec<String> = Vec::new();
    for key in ["AnimationName", "CustomAnimationBank"] {
        if let Some(val) = def.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            if let Some(hit) = find_side_bank(val) {
                push_unique(&mut explicit_banks, hit);
            } else if let Some(hit) = common_human_banks
                .iter()
                .find(|k| k.eq_ignore_ascii_case(val) || crate::clips::bank_stem(k).eq_ignore_ascii_case(val))
            {
                push_unique(&mut explicit_banks, hit);
            } else if base == "soldier" {
                return Err(format!(
                    "/sides/{side_name}/units.json: soldier '{class_name}' {key} '{val}' is absent from /sides/{side_name}/anim_banks.json and /common/anim_banks.json"
                ));
            }
        }
    }

    let mut pkg_banks: Vec<String> = Vec::new();
    let pkg_key = zero_hash::fnv1a_hex(class_name);
    if let Some(pkg) = side_anim_json.get("packages").and_then(|p| p.get(&pkg_key)) {
        for b in pkg.get("banks").and_then(Value::as_array).unwrap_or(&[]) {
            if let Some(name) = b.as_str() {
                if let Some(hit) = find_side_bank(name) {
                    push_unique(&mut pkg_banks, hit);
                } else {
                    push_unique(&mut pkg_banks, name);
                }
            }
        }
    }

    // ComboAnimationBank bindings on melee/lightsaber weapons. Prefer per-character
    // `clipTiming[action]` (chosen bank + CRC for this hero) when present, falling
    // back to `clips[action]` candidate lists otherwise. Multiple actions may
    // alias the same `(bank, crc)` clip, so retain every action name per key.
    let mut combo_common_banks: Vec<String> = Vec::new();
    let mut clip_names: HashMap<String, Vec<String>> = HashMap::new();
    let mut push_alias = |key: String, action: &str| {
        let entry = clip_names.entry(key).or_default();
        if !entry.iter().any(|a| a == action) {
            entry.push(action.to_string());
        }
    };
    for weapon in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
        for binding in weapon
            .get("comboBindings")
            .and_then(Value::as_array)
            .unwrap_or(&[])
        {
            if let Some(bank) = binding.get("bank").and_then(Value::as_str) {
                if let Some(hit) = find_side_bank(bank) {
                    push_unique(&mut combo_common_banks, hit);
                }
            }
            if let Some(timing_map) = binding.get("clipTiming").and_then(Value::as_object) {
                for (action, entry) in timing_map.entries() {
                    if entry.is_null() {
                        continue;
                    }
                    let chosen_bank = entry.get("bank").and_then(Value::as_str);
                    if let Some(bank) = chosen_bank {
                        if let Some(hit) = find_side_bank(bank) {
                            let is_explicit = explicit_banks
                                .iter()
                                .any(|b| b.eq_ignore_ascii_case(hit));
                            let is_pkg = pkg_banks.iter().any(|b| b.eq_ignore_ascii_case(hit));
                            if !is_explicit && !is_pkg {
                                push_unique(&mut combo_common_banks, hit);
                            }
                        }
                    }
                    if let Some(crc) = entry.get("crc").and_then(Value::as_str) {
                        let key = match chosen_bank {
                            Some(bank) if !bank.is_empty() => {
                                format!("{}:{}", bank.to_ascii_lowercase(), crc.to_ascii_lowercase())
                            }
                            _ => crc.to_ascii_lowercase(),
                        };
                        push_alias(key, action);
                    }
                }
            } else if let Some(clips_map) = binding.get("clips").and_then(Value::as_object) {
                for (action, matches) in clips_map.entries() {
                    for m in matches.as_array().unwrap_or(&[]) {
                        if let Some(bank) = m.get("bank").and_then(Value::as_str) {
                            if let Some(hit) = find_side_bank(bank) {
                                push_unique(&mut combo_common_banks, hit);
                            }
                        }
                        if let Some(crc) = m.get("crc").and_then(Value::as_str) {
                            push_alias(crc.to_ascii_lowercase(), action);
                        }
                    }
                }
            }
        }
    }

    // Order side banks from lowest priority to highest priority:
    // weapon common combo banks -> package banks (reverse archive order so first
    // in archive wins) -> explicit AnimationName / CustomAnimationBank (wins all).
    let mut side_banks: Vec<String> = Vec::new();
    let push_highest_wins = |list: &mut Vec<String>, name: &str| {
        if name.is_empty() {
            return;
        }
        list.retain(|b| !b.eq_ignore_ascii_case(name));
        list.push(name.to_string());
    };
    for b in &combo_common_banks {
        push_highest_wins(&mut side_banks, b);
    }
    for b in pkg_banks.iter().rev() {
        push_highest_wins(&mut side_banks, b);
    }
    for b in &explicit_banks {
        push_highest_wins(&mut side_banks, b);
    }

    // Drop low-res (`*lz`) banks when a full-resolution side bank is present so
    // reduced-bone low-res clips never overwrite full-rig clips.
    let has_full_res_side = side_banks
        .iter()
        .any(|b| !is_low_res_bank(b, low_res_skel));
    if has_full_res_side {
        side_banks.retain(|b| !is_low_res_bank(b, low_res_skel));
    }

    Ok((side_banks, clip_names))
}

fn classify_swbf2_class(def: &Value, base: &str, has_actor: bool) -> SimulationSupport {
    if base != "soldier" {
        let reason = match base {
            "flyer" | "commandflyer" => {
                format!("starfighter/flyer class (base={base}); 2005 side vehicle packs are not yet wired to the fleet simulator")
            }
            "remoteterminal" => "remoteterminal class is not yet simulated".into(),
            "armedbuilding" => "turret class in side roster (placed on map, not a playable infantry class)".into(),
            other => format!("class base '{other}' is not yet simulated"),
        };
        return SimulationSupport::NotYetSimulable { reasons: vec![reason] };
    }
    if !has_actor {
        return SimulationSupport::NotYetSimulable {
            reasons: vec!["soldier class has no actor skeleton/mesh".into()],
        };
    }
    let mut partial = Vec::new();
    if def.get("combo").is_some_and(Value::truthy) {
        partial.push("hero combo state machine".into());
    }
    if def.get("cloth").is_some_and(Value::truthy) {
        partial.push("cloth simulation".into());
    }
    let mut has_melee = false;
    let mut has_force = false;
    for w in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
        match w.get("base").and_then(Value::as_str).unwrap_or("") {
            "melee" | "meleethrow" => has_melee = true,
            "areaeffectweapon" => has_force = true,
            _ => {}
        }
    }
    if has_melee {
        partial.push("lightsaber/melee combo weapons".into());
    }
    if has_force {
        partial.push("force/area-effect powers".into());
    }
    if partial.is_empty() {
        SimulationSupport::InfantryBaseline
    } else {
        SimulationSupport::Partial { reasons: partial }
    }
}

fn enrich_placed_turret_weapons(props: Option<&Value>, sides: &[SideFiles]) -> Option<Value> {
    let props = props?;
    let instances = props.get("instances").and_then(Value::as_array)?;
    let mut updated = Vec::with_capacity(instances.len());
    for inst in instances {
        if inst.get("base").and_then(Value::as_str) == Some("armedbuilding")
            && inst.get("weapon").is_none_or(Value::is_null)
        {
            let cls = inst.get("class").and_then(Value::as_str).unwrap_or("");
            let found_weapon = sides.iter().find_map(|s| {
                s.units
                    .as_object()
                    .and_then(|u| {
                        u.get(cls)
                            .or_else(|| u.entries().find(|(k, _)| k.eq_ignore_ascii_case(cls)).map(|(_, v)| v))
                    })
                    .and_then(|u| u.get("weapons"))
                    .and_then(|w| w.at(0))
                    .cloned()
            });
            if let Some(w) = found_weapon {
                let mut obj = inst.as_object().cloned().unwrap_or_default();
                obj.set(Rc::from("weapon"), w);
                updated.push(Value::Object(Rc::new(obj)));
                continue;
            }
        }
        updated.push(inst.clone());
    }
    let mut out = props.as_object().cloned().unwrap_or_default();
    out.set(Rc::from("instances"), Value::Array(Rc::new(updated)));
    Some(Value::Object(Rc::new(out)))
}

fn team_for_swbf2_side(side: &str) -> u32 {
    match side.to_ascii_lowercase().as_str() {
        "rep" | "all" => 1,
        "cis" | "imp" => 2,
        _ => 3,
    }
}

fn synthesize_swbf2_mission(
    mission_name: &str,
    mission_rec: &Value,
    sides: &[SideFiles],
    roster: &RosterReport,
    practice: Option<[&str; 2]>,
) -> Value {
    let mut team1_classes: Vec<String> = Vec::new();
    let mut team2_classes: Vec<String> = Vec::new();
    for entry in &roster.classes {
        if entry.base != "soldier" || !entry.has_actor {
            continue;
        }
        if practice.is_some_and(|pair| !pair.contains(&entry.class.as_str())) { continue; }
        match team_for_swbf2_side(&entry.side) {
            1 => team1_classes.push(entry.class.clone()),
            2 => team2_classes.push(entry.class.clone()),
            _ => {}
        }
    }
    // If all soldier classes came from one side bucket, split deterministically
    // so both teams still have playable entries.
    if team1_classes.is_empty() && !team2_classes.is_empty() {
        team1_classes = team2_classes.clone();
    } else if team2_classes.is_empty() && !team1_classes.is_empty() {
        team2_classes = team1_classes.clone();
    }

    // Developer stand-in team roster until mission Lua scripts supply authored
    // `SetTeamName` / `AddUnitClass` / `SetUnitCount` / `SetReinforcementCount`.
    // Both teams receive the same stand-in unit count so roster asymmetry does
    // not tilt stand-in battles, and quotas spread evenly across loaded classes.
    let shared_unit_count = if practice.is_some() { 1 } else {
        (team1_classes.len().max(team2_classes.len()) * 2).clamp(4, 16)
    };
    let make_team = |name: &str, classes: &[String]| -> Value {
        let unit_count = if practice.is_some_and(|pair| classes.iter().any(|c| c == pair[1])) {
            3
        } else { shared_unit_count };
        let base_quota = (unit_count / classes.len().max(1)).max(1);
        let remainder = unit_count.saturating_sub(base_quota * classes.len());
        let entries: Vec<Value> = classes
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let quota = base_quota + usize::from(i < remainder);
                Value::Array(Rc::new(vec![Value::str(c), Value::Number(quota as f64)]))
            })
            .collect();
        Value::Object(Rc::new(Object::from_written(vec![
            (Rc::from("name"), Value::str(name)),
            (Rc::from("classes"), Value::Array(Rc::new(entries))),
            (Rc::from("units"), Value::Number(unit_count as f64)),
            (Rc::from("reinforcements"), Value::Number(150.0)),
            (Rc::from("bleed"), Value::Array(Rc::new(Vec::new()))),
            (Rc::from("standIn"), Value::Bool(true)),
        ])))
    };

    let side_names: Vec<Value> = sides.iter().map(|s| Value::str(&s.name)).collect();
    let has_gcw = sides.iter().any(|s| s.name == "all" || s.name == "imp");
    let has_cw = sides.iter().any(|s| s.name == "rep" || s.name == "cis");
    let (t1_name, t2_name, era) = if has_gcw && has_cw {
        ("heroes", "villains", "gcw")
    } else if has_cw {
        ("rep", "cis", "cw")
    } else {
        ("all", "imp", "gcw")
    };

    let teams = Value::Object(Rc::new(Object::from_written(vec![
        (Rc::from("1"), make_team(t1_name, &team1_classes)),
        (Rc::from("2"), make_team(t2_name, &team2_classes)),
    ])));

    let mut m = Object::from_written(vec![
        (Rc::from("name"), Value::str(mission_name)),
        (Rc::from("era"), Value::str(era)),
        (Rc::from("sides"), Value::Array(Rc::new(side_names))),
        (Rc::from("teams"), teams),
        (Rc::from("synthesized"), Value::Bool(true)),
        (Rc::from("standIn"), Value::Bool(true)),
    ]);
    if let Some(layers) = mission_rec.get("layers") {
        m.set(Rc::from("layers"), layers.clone());
    }
    if let Some(sound) = mission_rec.get("sound") {
        m.set(Rc::from("sound"), sound.clone());
    }
    Value::Object(Rc::new(m))
}

#[cfg(test)]
mod swbf2_tests {
    use super::*;

    #[test]
    fn practice_infantry_starts_with_normal_trigger_channels() {
        let raw = crate::json::parse(r#"{"weapons":[{"channel":0},{"channel":3},{"channel":1}]}"#).unwrap();
        let channels = |starting| {
            let unit = swbf2_unit_definition(&raw, starting);
            unit.get("weapons").unwrap().as_array().unwrap().iter()
                .map(|w| w.get("channel").unwrap().as_f64().unwrap() as u8).collect::<Vec<_>>()
        };
        assert_eq!(channels(true), [0, 1]);
        assert_eq!(channels(false), [0, 3, 1]);
    }

    use crate::files::MemFiles;

    fn f32_bytes(xs: &[f32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn u32_bytes(xs: &[u32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn swbf2_battle_loads_roster_materializes_odfs_and_lists_non_simulable_classes() {
        let mut files = MemFiles::default();
        files.insert("common/common.json", br#"{"family":"swbf2","textures":{}}"#.to_vec());
        let jump_crc = zero_hash::crc_hex("human_rifle_jump");
        let root_crc = zero_hash::crc_hex("DummyRoot");
        files.insert(
            "common/anim_banks.json",
            format!(
                r#"{{"fps":30,"banks":{{"human_1":{{"clipCount":1,"clips":[{{"crc":"{jump_crc}","flags":0,"frames":2,"bones":1,"offset":0,"tracks":["{root_crc}"]}}]}}}}}}"#
            )
            .into_bytes(),
        );
        files.insert("common/anim_banks.bin", f32_bytes(&[0.0; 14]));
        files.insert("maps/arena/collision.bin", f32_bytes(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0]));
        files.insert("maps/arena/collision_index.bin", u32_bytes(&[0, 1, 2]));
        files.insert("maps/arena/collision_poly.bin", u32_bytes(&[0]));
        files.insert("maps/arena/collision_mask.bin", vec![0]);
        files.insert(
            "maps/arena/map.json",
            br#"{
                "family": "swbf2",
                "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                "meshes": [],
                "lights": [],
                "regions": [],
                "deathRegions": [],
                "paths": [],
                "spawns": {"blue_sp": [[0,1,0,0]]},
                "commandPosts": [],
                "missions": {
                    "arena_m1": {
                        "layers": ["obj"],
                        "sides": {
                            "blue": ["test_pilot", "test_fighter", "test_terminal", "test_dome"]
                        }
                    }
                },
                "layers": {
                    "obj": {
                        "meshes": [],
                        "placedObjects": [],
                        "spawns": {},
                        "regions": [],
                        "lights": [],
                        "paths": [],
                        "commandPosts": [{"name":"cp1","team":1,"spawnPath":"blue_sp","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}]
                    }
                }
            }"#
            .to_vec(),
        );
        files.insert(
            "sides/blue/units.json",
            br#"{
                "units": {
                    "test_pilot": {
                        "class": "test_pilot",
                        "base": "soldier",
                        "model": "test_pilot_rig",
                        "actor": "actors/test_pilot_rig.json",
                        "properties": [
                            ["MaxHealth", "250.0"],
                            ["MaxSpeed", "5.5"],
                            ["CAMERASECTION", "STAND"],
                            ["EyePointOffset", "0.0 1.5 0.0"],
                            ["TrackCenter", "0.0 1.6 0.0"],
                            ["TrackOffset", "0.0 0.0 2.5"],
                            ["TiltValue", "8.0"]
                        ],
                        "weapons": [{
                            "class": "test_blaster",
                            "base": "cannon",
                            "RoundsPerClip": "24",
                            "ShotDelay": "0.25",
                            "ordnance": {
                                "class": "test_blaster_ord",
                                "base": "bolt",
                                "Velocity": "180.0",
                                "LifeSpan": "1.2",
                                "MaxDamage": "65.0",
                                "explosion": {"Damage": "40.0", "DamageRadiusOuter": "4.5"}
                            }
                        }, {"class":"ribbon_tool","channel":3,"MedalsTypeToUnlock":"7"},
                           {"class":"chalk","channel":"1","MedalsTypeToLock":"4"},
                           {"class":"gated_chalk","channel":1,"MedalsTypeToUnlock":"4"},
                           {"class":"lamp","channel":1,"MedalsTypeToUnlock":"-1"}]
                    },
                    "test_fighter": {
                        "class": "test_fighter",
                        "base": "flyer",
                        "model": "test_fighter_rig",
                        "actor": null,
                        "properties": [["MaxHealth", "1200"]],
                        "weapons": [{"class":"wing_lamp","channel":2}]
                    },
                    "test_terminal": {
                        "class": "test_terminal",
                        "base": "remoteterminal",
                        "model": "test_console",
                        "actor": null,
                        "properties": [["MaxHealth", "4200"]],
                        "weapons": []
                    }
                }
            }"#
            .to_vec(),
        );
        files.insert("sides/blue/weapons.json", br#"{"models":{},"firePoints":{}}"#.to_vec());
        files.insert("sides/blue/weapons.bin", Vec::new());
        files.insert("sides/blue/weapons_index.bin", Vec::new());
        files.insert("sides/blue/anim_banks.json", br#"{"fps":30,"banks":{},"packages":{}}"#.to_vec());
        files.insert("sides/blue/anim_banks.bin", Vec::new());
        // Intentionally lowercase `dummyroot` to test canonicalization.
        files.insert(
            "sides/blue/actors/test_pilot_rig.json",
            br#"{"bones":[{"name":"dummyroot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"tex"}]}"#.to_vec(),
        );
        files.insert("sides/blue/actors/test_pilot_rig.bin", f32_bytes(&[0.0; 27]));
        files.insert("sides/blue/actors/test_pilot_rig_index.bin", u32_bytes(&[0, 1, 2]));

        let loader = Loader::new(&files);
        let battle = Battle::load_for_family(&loader, GameFamily::Swbf2, "arena", "?mission=arena_m1").unwrap();
        let names = |unit: &Value| -> Vec<String> {
            unit.get("weapons").unwrap().as_array().unwrap().iter()
                .map(|w| w.get("class").unwrap().as_str().unwrap().to_owned()).collect()
        };
        assert_eq!(names(&battle.soldier_def), ["test_blaster", "chalk", "lamp"]);
        let classes = &battle.sides[0].units;
        assert_eq!(names(classes.get("test_pilot").unwrap()), names(&battle.soldier_def));
        assert_eq!(names(classes.get("test_fighter").unwrap()), ["wing_lamp"]);
        assert_eq!(battle.family, GameFamily::Swbf2);
        assert_eq!(battle.unit, "test_pilot");
        assert_eq!(battle.soldier_def.get("MaxHealth").and_then(Value::as_f64), Some(250.0));
        assert!(battle.soldier_def.get("cameras").and_then(|c| c.get("STAND")).is_some());
        let ord = battle.soldier_def.get("weapons").unwrap().at(0).unwrap().get("ordnance").unwrap();
        assert_eq!(ord.get("Velocity").and_then(Value::as_f64), Some(180.0));
        assert_eq!(ord.get("MaxDamage").and_then(Value::as_f64), Some(65.0));
        assert_eq!(ord.get("Damage").and_then(Value::as_f64), Some(65.0));
        assert_eq!(
            ord.get("explosion")
                .and_then(|e| e.get("DamageRadiusOuter"))
                .and_then(Value::as_f64),
            Some(4.5)
        );
        // `dummyroot` was canonicalized to `DummyRoot`.
        assert_eq!(
            battle.bodies[0].1.def.get("bones").unwrap().at(0).unwrap().get("name").unwrap().as_str(),
            Some("DummyRoot")
        );
        let report = battle.roster_report().unwrap();
        assert_eq!(report.infantry_baseline_count(), 1);
        assert_eq!(report.not_yet_simulable_classes().len(), 2);
        assert_eq!(report.non_unit_packages.len(), 1);
        assert_eq!(report.non_unit_packages[0].1, "test_dome");

        let content = crate::content::Content::build(battle).unwrap();
        assert!(!content.collision.has_terrain);
        assert_eq!(content.unit_clips.len(), 1);
        assert_eq!(content.unit_clips[0].0, "blue/test_pilot");
        assert!(content.unit_clips[0].1.get("human_rifle_jump").is_some());
        assert_eq!(content.unit_anim_stats[0].1.matched_tracks, 1);
        let defs = crate::defs::Defs::build(&content).unwrap();
        assert!(defs.classes.iter().any(|u| defs.names.text(u.name) == "test_pilot"));
    }

    #[test]
    fn swbf2_non_human_rig_binds_own_full_res_bank_without_common_human_or_lz() {
        let mut files = MemFiles::default();
        files.insert("common/common.json", br#"{"family":"swbf2","textures":{}}"#.to_vec());
        let root_crc = zero_hash::crc_hex("DummyRoot");
        let human_arm_crc = zero_hash::crc_hex("bone_l_upperarm");
        let quad_limb_crc = zero_hash::crc_hex("bone_limb_fl");
        let clip_crc = zero_hash::crc_hex("human_rifle_jump");
        files.insert(
            "common/anim_banks.json",
            format!(
                r#"{{"fps":30,"banks":{{"human_synthetic_0":{{"clips":[{{"crc":"{clip_crc}","flags":0,"frames":2,"offset":0,"tracks":["{root_crc}","{human_arm_crc}"]}}]}}}}}}"#
            )
            .into_bytes(),
        );
        files.insert("common/anim_banks.bin", f32_bytes(&[0.0; 28]));
        files.insert("maps/arena/collision.bin", f32_bytes(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0]));
        files.insert("maps/arena/collision_index.bin", u32_bytes(&[0, 1, 2]));
        files.insert("maps/arena/collision_poly.bin", u32_bytes(&[0]));
        files.insert(
            "maps/arena/map.json",
            br#"{
                "family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],
                "spawns":{"sp1":[[0,1,0,0]]},
                "commandPosts":[{"name":"cp1","team":1,"spawnPath":"sp1","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],
                "missions":{"arena_m1":{"layers":[],"sides":{"gold":["quad_unit"]}}},
                "layers":{}
            }"#.to_vec(),
        );
        let pkg_hash = zero_hash::fnv1a_hex("quad_unit");
        files.insert(
            "sides/gold/units.json",
            br#"{"units":{"quad_unit":{"class":"quad_unit","base":"soldier","model":"quad_rig","actor":"actors/quad_rig.json","properties":[["SkeletonName","quad"],["SkeletonLowRes","quadlz"],["MaxHealth","300"]],"weapons":[]}}}"#.to_vec(),
        );
        files.insert("sides/gold/weapons.json", br#"{"models":{},"firePoints":{}}"#.to_vec());
        files.insert("sides/gold/weapons.bin", Vec::new());
        files.insert("sides/gold/weapons_index.bin", Vec::new());
        files.insert(
            "sides/gold/anim_banks.json",
            format!(
                r#"{{
                    "fps":30,
                    "packages":{{"{pkg_hash}":{{"banks":["quad","quadlz"]}}}},
                    "banks":{{
                        "quad":{{"clips":[{{"crc":"11223344","flags":0,"frames":2,"offset":0,"tracks":["{root_crc}","{quad_limb_crc}"]}}]}},
                        "quadlz":{{"clips":[{{"crc":"11223344","flags":0,"frames":2,"offset":0,"tracks":["{root_crc}"]}}]}}
                    }}
                }}"#
            )
            .into_bytes(),
        );
        files.insert("sides/gold/anim_banks.bin", f32_bytes(&[0.0; 28]));
        files.insert(
            "sides/gold/actors/quad_rig.json",
            br#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},{"name":"bone_limb_fl","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]}"#.to_vec(),
        );
        files.insert("sides/gold/actors/quad_rig.bin", f32_bytes(&[0.0; 27]));
        files.insert("sides/gold/actors/quad_rig_index.bin", u32_bytes(&[0, 1, 2]));

        let battle = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1").unwrap();
        let entry = &battle.roster_report().unwrap().classes[0];
        assert_eq!(entry.anim_banks, vec!["quad"]);
        let content = crate::content::Content::build(battle).unwrap();
        let (_, stats) = &content.unit_anim_stats[0];
        assert_eq!(stats.banks, vec!["quad"]);
        assert_eq!(stats.matched_tracks, 2);
        assert_eq!(stats.unmatched_tracks, 0);
        assert_eq!(stats.unbound_share(), 0.0);
    }

    #[test]
    fn swbf2_battle_rejects_missing_side_malformed_anim_banks_and_unknown_mission() {
        let make_files = || {
            let mut files = MemFiles::default();
            files.insert("common/common.json", br#"{"family":"swbf2","textures":{}}"#.to_vec());
            files.insert("common/anim_banks.json", br#"{"fps":30,"banks":{}}"#.to_vec());
            files.insert("common/anim_banks.bin", Vec::new());
            files.insert("maps/arena/collision.bin", f32_bytes(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0]));
            files.insert("maps/arena/collision_index.bin", u32_bytes(&[0, 1, 2]));
            files.insert("maps/arena/collision_poly.bin", u32_bytes(&[0]));
            files.insert(
                "maps/arena/map.json",
                br#"{
                    "family": "swbf2", "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                    "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                    "paths": [], "spawns": {"sp1": [[0,1,0,0]]},
                    "commandPosts": [{"name":"cp1","team":1,"spawnPath":"sp1","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],
                    "missions": {"arena_m1": {"layers": [], "sides": {"blue": ["test_pilot"]}}},
                    "layers": {}
                }"#.to_vec(),
            );
            files.insert(
                "sides/blue/units.json",
                br#"{"units":{"test_pilot":{"class":"test_pilot","base":"soldier","model":"rig","actor":"actors/rig.json","properties":[["MaxHealth","200"]],"weapons":[]}}}"#.to_vec(),
            );
            files.insert("sides/blue/weapons.json", br#"{"models":{},"firePoints":{}}"#.to_vec());
            files.insert("sides/blue/weapons.bin", Vec::new());
            files.insert("sides/blue/weapons_index.bin", Vec::new());
            files.insert("sides/blue/anim_banks.json", br#"{"fps":30,"banks":{},"packages":{}}"#.to_vec());
            files.insert("sides/blue/anim_banks.bin", Vec::new());
            files.insert(
                "sides/blue/actors/rig.json",
                br#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]}"#.to_vec(),
            );
            files.insert("sides/blue/actors/rig.bin", f32_bytes(&[0.0; 27]));
            files.insert("sides/blue/actors/rig_index.bin", u32_bytes(&[0, 1, 2]));
            files
        };

        // 1. Unknown mission in query must error.
        let files = make_files();
        let err = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_typo")
            .err()
            .unwrap();
        assert!(err.contains("arena_typo"), "expected mission name: {err}");

        // 2. Mission naming a side with missing units.json must error naming the file.
        let mut files = make_files();
        files.insert(
            "maps/arena/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {"sp1": [[0,1,0,0]]},
                "commandPosts": [{"name":"cp1","team":1,"spawnPath":"sp1","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],
                "missions": {"arena_m1": {"layers": [], "sides": {"missing_side": ["unit_a"]}}},
                "layers": {}
            }"#.to_vec(),
        );
        let err = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1")
            .err()
            .unwrap();
        assert!(err.contains("sides/missing_side/units.json"), "expected missing side path: {err}");

        // 3. Malformed common/anim_banks.json must error naming the file.
        let mut files = make_files();
        files.insert("common/anim_banks.json", b"{ invalid".to_vec());
        let err = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1")
            .err()
            .unwrap();
        assert!(err.contains("anim_banks.json"), "expected anim_banks.json: {err}");

        // 4. Malformed sides/blue/anim_banks.json must error naming the file.
        let mut files = make_files();
        files.insert("sides/blue/anim_banks.json", b"[broken".to_vec());
        let err = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1")
            .err()
            .unwrap();
        assert!(err.contains("sides/blue/anim_banks.json"), "expected side anim_banks.json: {err}");

        // 5. Wrong top-level JSON types (arrays) on units.json and anim_banks.json must error.
        for (path, label) in [
            ("sides/blue/units.json", "units.json"),
            ("common/anim_banks.json", "common/anim_banks.json"),
            ("sides/blue/anim_banks.json", "sides/blue/anim_banks.json"),
        ] {
            let mut files = make_files();
            files.insert(path, b"[]".to_vec());
            let err = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1")
                .err()
                .unwrap_or_else(|| panic!("expected error for array {path}"));
            assert!(err.contains(label), "{path}: unexpected error {err}");
        }

        // 6. Mission sides of wrong type (string instead of object) must error.
        let mut files = make_files();
        files.insert(
            "maps/arena/map.json",
            br#"{"family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],"spawns":{"sp1":[[0,1,0,0]]},"commandPosts":[{"name":"cp1","team":1,"spawnPath":"sp1","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],"missions":{"arena_m1":{"layers":[],"sides":"blue"}},"layers":{}}"#.to_vec(),
        );
        let err = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1")
            .err()
            .unwrap();
        assert!(err.contains("sides"), "expected sides error: {err}");

        // 7. Actor bone cycle must error.
        let mut files = make_files();
        files.insert(
            "sides/blue/actors/rig.json",
            br#"{"bones":[{"name":"a","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},{"name":"b","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]}"#.to_vec(),
        );
        let err = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1")
            .err()
            .unwrap();
        assert!(err.contains("cycle"), "expected actor skeleton cycle error: {err}");

        // 8. Wrong field types on a selected unit definition must error.
        for (tag, unit_json) in [
            ("model_num", r#"{"class":"test_pilot","base":"soldier","model":5,"actor":"actors/rig.json","properties":[],"weapons":[]}"#),
            ("props_obj", r#"{"class":"test_pilot","base":"soldier","model":"rig","actor":"actors/rig.json","properties":{"a":1},"weapons":[]}"#),
            ("weapons_str", r#"{"class":"test_pilot","base":"soldier","model":"rig","actor":"actors/rig.json","properties":[],"weapons":"x"}"#),
            ("hilts_str", r#"{"class":"test_pilot","base":"soldier","model":"rig","actor":"actors/rig.json","properties":[],"weapons":[{"base":"melee","saberHilts":"x"}]}"#),
            ("cliptiming_str", r#"{"class":"test_pilot","base":"soldier","model":"rig","actor":"actors/rig.json","properties":[],"weapons":[{"base":"melee","comboBindings":[{"clipTiming":"x"}]}]}"#),
            ("cliptiming_huge", r#"{"class":"test_pilot","base":"soldier","model":"rig","actor":"actors/rig.json","properties":[],"weapons":[{"base":"melee","comboBindings":[{"clipTiming":{"a":{"bank":"b","crc":"01020304","frames":1e30}}}]}]}"#),
            ("actor_escape", r#"{"class":"test_pilot","base":"soldier","model":"rig","actor":"../../etc/passwd","properties":[],"weapons":[]}"#),
        ] {
            let mut files = make_files();
            files.insert(
                "sides/blue/units.json",
                format!(r#"{{"units":{{"test_pilot":{unit_json}}}}}"#).into_bytes(),
            );
            assert!(
                Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1").is_err(),
                "expected error for malformed unit case {tag}"
            );
        }
    }

    #[test]
    fn swbf2_bank_precedence_honors_explicit_bank_over_common_and_preserves_saber_hilts() {
        let mut files = MemFiles::default();
        files.insert("common/common.json", br#"{"family":"swbf2","textures":{}}"#.to_vec());
        files.insert("common/anim_banks.json", br#"{"fps":30,"banks":{}}"#.to_vec());
        files.insert("common/anim_banks.bin", Vec::new());
        files.insert("maps/arena/collision.bin", f32_bytes(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0]));
        files.insert("maps/arena/collision_index.bin", u32_bytes(&[0, 1, 2]));
        files.insert("maps/arena/collision_poly.bin", u32_bytes(&[0]));
        files.insert(
            "maps/arena/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {"sp1": [[0,1,0,0]]},
                "commandPosts": [{"name":"cp1","team":1,"spawnPath":"sp1","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],
                "missions": {"arena_m1": {"layers": [], "sides": {"gold": ["test_duelist"]}}},
                "layers": {}
            }"#.to_vec(),
        );
        let root_crc = zero_hash::crc_hex("DummyRoot");
        let arm_crc = zero_hash::crc_hex("bone_l_upperarm");
        let shared_clip_crc = zero_hash::crc_hex("stand_attack_alpha");
        let fallback_clip_crc = zero_hash::crc_hex("stand_attack_beta");
        let pkg_key = zero_hash::fnv1a_hex("test_duelist");

        files.insert(
            "sides/gold/units.json",
            format!(
                r#"{{
                    "units": {{
                        "test_duelist": {{
                            "class": "test_duelist",
                            "base": "soldier",
                            "model": "duelist_rig",
                            "actor": "actors/duelist_rig.json",
                            "properties": [["MaxHealth","500"],["AnimationName","duelist_own"]],
                            "weapons": [{{
                                "base": "melee",
                                "saberHilts": [{{
                                    "model": "test_hilt",
                                    "attachment": {{"name": "hp_weapons", "boneCrc": "{root_crc}"}},
                                    "blades": [{{"socket": "hp_fire", "length": 1.2, "width": 0.05}}]
                                }}],
                                "comboBindings": [{{
                                    "bank": "shared_sabre",
                                    "mode": "melee",
                                    "machine": "test_duelist",
                                    "combo": "00000001",
                                    "clipTiming": {{
                                        "swing_alpha_upper": {{"bank": "duelist_own", "crc": "{shared_clip_crc}", "frames": 16, "fps": 30}},
                                        "swing_alpha": {{"bank": "duelist_own", "crc": "{shared_clip_crc}", "frames": 16, "fps": 30}},
                                        "swing_beta": {{"bank": "shared_sabre", "crc": "{fallback_clip_crc}", "frames": 10, "fps": 30}},
                                        "missing_action": null
                                    }}
                                }}]
                            }}]
                        }}
                    }}
                }}"#
            )
            .into_bytes(),
        );
        files.insert("sides/gold/weapons.json", br#"{"models":{},"firePoints":{}}"#.to_vec());
        files.insert("sides/gold/weapons.bin", Vec::new());
        files.insert("sides/gold/weapons_index.bin", Vec::new());

        // `shared_sabre` has 8 frames for `shared_clip_crc` and 10 frames for `fallback_clip_crc`.
        // `duelist_own` overrides `shared_clip_crc` with 16 frames -> duration must be (16 - 1) / 30 = 0.5s.
        let shared_floats_len = 2 * 8 * 7 + 2 * 10 * 7;
        let own_offset = shared_floats_len;
        let total_floats = own_offset + 2 * 16 * 7;
        files.insert(
            "sides/gold/anim_banks.json",
            format!(
                r#"{{
                    "fps": 30,
                    "packages": {{"{pkg_key}": {{"banks": ["duelist_own", "duelist_ownlz"]}}}},
                    "banks": {{
                        "shared_sabre": {{"clips": [
                            {{"crc": "{shared_clip_crc}", "flags": 0, "frames": 8, "offset": 0, "tracks": ["{root_crc}", "{arm_crc}"]}},
                            {{"crc": "{fallback_clip_crc}", "flags": 0, "frames": 10, "offset": 112, "tracks": ["{root_crc}", "{arm_crc}"]}}
                        ]}},
                        "duelist_own": {{"clips": [
                            {{"crc": "{shared_clip_crc}", "flags": 0, "frames": 16, "offset": {own_offset}, "tracks": ["{root_crc}", "{arm_crc}"]}}
                        ]}},
                        "duelist_ownlz": {{"clips": [
                            {{"crc": "{shared_clip_crc}", "flags": 0, "frames": 4, "offset": 0, "tracks": ["{root_crc}"]}}
                        ]}}
                    }}
                }}"#
            )
            .into_bytes(),
        );
        files.insert("sides/gold/anim_banks.bin", f32_bytes(&vec![0.0; total_floats]));
        files.insert(
            "sides/gold/actors/duelist_rig.json",
            br#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},{"name":"bone_l_upperarm","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]}"#.to_vec(),
        );
        files.insert("sides/gold/actors/duelist_rig.bin", f32_bytes(&[0.0; 27]));
        files.insert("sides/gold/actors/duelist_rig_index.bin", u32_bytes(&[0, 1, 2]));

        let battle = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "arena", "?mission=arena_m1").unwrap();
        let entry = &battle.roster_report().unwrap().classes[0];
        // Weapon common bank precedes explicit AnimationName so explicit AnimationName wins; `*lz` dropped.
        assert_eq!(entry.anim_banks, vec!["shared_sabre", "duelist_own"]);

        // saberHilts and clipTiming are preserved verbatim on the materialized weapon.
        let unit = battle.sides[0].units.get("test_duelist").unwrap();
        let weapon = unit.get("weapons").unwrap().at(0).unwrap();
        let hilts = weapon.get("saberHilts").unwrap().as_array().unwrap();
        assert_eq!(hilts.len(), 1);
        assert_eq!(hilts[0].get("model").and_then(Value::as_str), Some("test_hilt"));
        assert_eq!(
            hilts[0].get("attachment").and_then(|a| a.get("boneCrc")).and_then(Value::as_str),
            Some(root_crc.as_str())
        );

        let content = crate::content::Content::build(battle).unwrap();
        let (_, clips) = &content.unit_clips[0];
        let a_up = clips.get("swing_alpha_upper").expect("swing_alpha_upper");
        assert_eq!(a_up.frames_f64(), 16.0);
        let a = clips.get("swing_alpha").expect("swing_alpha");
        assert_eq!(a.frames_f64(), 16.0);
        assert!((a.duration - 15.0 / 30.0).abs() < 1e-12);
        let c = clips.get("swing_beta").expect("swing_beta");
        assert_eq!(c.frames_f64(), 10.0);
        assert!((c.duration - 9.0 / 30.0).abs() < 1e-12);
    }

    #[test]
    fn swbf2_ordnance_maps_explosions_damage_order_and_dispenser_items() {
        let raw_unit = crate::json::parse(
            r#"{
                "class": "test_demo",
                "base": "soldier",
                "properties": [["MaxHealth", "300"]],
                "weapons": [
                    {
                        "class": "test_rocket",
                        "base": "launcher",
                        "ordnance": {
                            "class": "test_rocket_ord",
                            "base": "missile",
                            "properties": [["Damage", "40"], ["MaxDamage", "125"]],
                            "Damage": "40",
                            "MaxDamage": "125",
                            "ExplosionName": {
                                "class": "test_rocket_exp",
                                "base": "explosion",
                                "properties": [["Damage", "220"], ["DamageRadius", "5.5"], ["HealthScale", "1.2"]],
                                "Damage": "220",
                                "DamageRadius": "5.5",
                                "HealthScale": "1.2"
                            }
                        }
                    },
                    {
                        "class": "test_grenade",
                        "base": "grenade",
                        "ordnance": {
                            "class": "test_grenade_ord",
                            "base": "sticky",
                            "properties": [["MaxDamage", "90"], ["Damage", "60"]],
                            "MaxDamage": "90",
                            "Damage": "60",
                            "ExplosionExpire": {
                                "class": "test_grenade_exp",
                                "base": "explosion",
                                "Damage": "350",
                                "DamageRadiusInner": "3.0",
                                "DamageRadiusOuter": "6.0"
                            }
                        }
                    },
                    {
                        "class": "test_beam_force",
                        "base": "cannon",
                        "ordnance": {
                            "class": "test_force_ord",
                            "base": "emitterordnance",
                            "properties": [["Damage", "50"], ["MaxDamage", "150"]],
                            "Damage": "50",
                            "MaxDamage": "150",
                            "Explosion": {
                                "class": "test_force_exp",
                                "base": "explosion",
                                "Damage": "75",
                                "DamageRadius": "4.0"
                            }
                        }
                    },
                    {
                        "class": "test_detpack",
                        "base": "detonator",
                        "ordnance": {
                            "class": "test_detpack_mine",
                            "base": "mine",
                            "MaxHealth": "40",
                            "ExplosionTrigger": {
                                "class": "test_detpack_exp",
                                "base": "explosion",
                                "Damage": "500",
                                "DamageRadiusOuter": "7.5"
                            },
                            "ExplosionDeath": {
                                "class": "test_detpack_death_exp",
                                "base": "explosion",
                                "Damage": "250",
                                "DamageRadiusOuter": "5.0"
                            }
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        let unit = materialize_swbf2_unit(&raw_unit);
        let weapons = unit.get("weapons").unwrap().as_array().unwrap();

        // 1. Rocket: later MaxDamage (125) overrides earlier Damage (40); ExplosionName -> ExplosionImpact
        //    with DamageRadius -> DamageRadiusOuter and HealthScale -> PersonScale/AnimalScale/DroidScale.
        let rocket_ord = weapons[0].get("ordnance").unwrap();
        assert_eq!(rocket_ord.get("Damage").and_then(Value::as_f64), Some(125.0));
        let rocket_exp = rocket_ord.get("ExplosionImpact").unwrap();
        assert_eq!(rocket_exp.get("Damage").and_then(Value::as_f64), Some(220.0));
        assert_eq!(rocket_exp.get("DamageRadiusOuter").and_then(Value::as_f64), Some(5.5));
        assert_eq!(rocket_exp.get("PersonScale").and_then(Value::as_f64), Some(1.2));
        assert_eq!(rocket_exp.get("DroidScale").and_then(Value::as_f64), Some(1.2));

        // 2. Grenade: later Damage (60) overrides earlier MaxDamage (90); ExplosionExpire coerced.
        let gren_ord = weapons[1].get("ordnance").unwrap();
        assert_eq!(gren_ord.get("Damage").and_then(Value::as_f64), Some(60.0));
        let gren_exp = gren_ord.get("ExplosionExpire").unwrap();
        assert_eq!(gren_exp.get("Damage").and_then(Value::as_f64), Some(350.0));
        assert_eq!(gren_exp.get("DamageRadiusOuter").and_then(Value::as_f64), Some(6.0));

        // 3. emitterordnance keeps Damage and MaxDamage separate; Explosion -> ExplosionImpact.
        let force_ord = weapons[2].get("ordnance").unwrap();
        assert_eq!(force_ord.get("Damage").and_then(Value::as_f64), Some(50.0));
        assert_eq!(force_ord.get("MaxDamage").and_then(Value::as_f64), Some(150.0));
        let force_exp = force_ord.get("ExplosionImpact").unwrap();
        assert_eq!(force_exp.get("DamageRadiusOuter").and_then(Value::as_f64), Some(4.0));

        // 4. Detonator mine promoted from `ordnance` to `item` with coerced ExplosionTrigger & ExplosionDeath.
        assert!(weapons[3].get("ordnance").is_none());
        let item = weapons[3].get("item").expect("detonator mine promoted to weapon.item");
        assert_eq!(item.get("base").and_then(Value::as_str), Some("mine"));
        assert_eq!(item.get("MaxHealth").and_then(Value::as_f64), Some(40.0));
        assert_eq!(
            item.get("ExplosionTrigger")
                .and_then(|e| e.get("Damage"))
                .and_then(Value::as_f64),
            Some(500.0)
        );
        assert_eq!(
            item.get("ExplosionDeath")
                .and_then(|e| e.get("DamageRadiusOuter"))
                .and_then(Value::as_f64),
            Some(5.0)
        );
    }

    #[test]
    fn swbf2_weapon_charge_tiers_and_shot_pattern_pitch_yaw() {
        let side_classes = crate::json::parse(
            r#"{
                "ordc": {
                    "inv_parent_bolt": {
                        "name": "inv_parent_bolt",
                        "base": "bolt",
                        "properties": [
                            ["LifeSpan", "1.4"],
                            ["Velocity", "73.0"],
                            ["MaxDamage", "41.0"],
                            ["DamageFinalDamage", "19.0"],
                            ["DamageTransitionDelay", "0.15"],
                            ["DamageTransitionPeriod", "0.35"]
                        ]
                    },
                    "inv_spread_bolt": {
                        "name": "inv_spread_bolt",
                        "base": "inv_parent_bolt",
                        "properties": []
                    }
                },
                "expc": {}
            }"#,
        )
        .unwrap();

        let raw_unit = crate::json::parse(
            r#"{
                "class": "inv_unit",
                "base": "soldier",
                "properties": [["MaxHealth", "350"]],
                "weapons": [{
                    "class": "inv_charge_rifle",
                    "base": "cannon",
                    "MaxPressedTime": "1.6",
                    "ShotsPerSalvo": "1",
                    "ShotPatternCount": "1",
                    "ShotPatternPitchYaw": "-0.1 1.4",
                    "OrdnanceName": "inv_heavy_bolt",
                    "NextCharge": "0.55",
                    "properties": [
                        ["OrdnanceName", "inv_spread_bolt"],
                        ["ShotPatternPitchYaw", "-0.2 -1.7"],
                        ["ShotPatternPitchYaw", "0.3 0.1"],
                        ["ShotPatternPitchYaw", "-0.1 1.4"],
                        ["ShotPatternCount", "3"],
                        ["ShotsPerSalvo", "3"],
                        ["MaxPressedTime", "1.6"],
                        ["NextCharge", "0.55"],
                        ["OrdnanceName", "inv_heavy_bolt"],
                        ["ShotsPerSalvo", "1"],
                        ["ShotPatternCount", "1"]
                    ],
                    "ordnance": {
                        "class": "inv_heavy_bolt",
                        "base": "bolt",
                        "properties": [["Velocity", "215.0"], ["LifeSpan", "1.1"], ["MaxDamage", "135.0"]],
                        "Velocity": "215.0",
                        "LifeSpan": "1.1",
                        "MaxDamage": "135.0"
                    }
                }]
            }"#,
        )
        .unwrap();

        let unit = materialize_swbf2_unit_with_classes(&raw_unit, Some(&side_classes));
        let gun = &unit.get("weapons").unwrap().as_array().unwrap()[0];

        // Base tier keeps invented 3-bolt asymmetric pattern and resolves base ordnance from side classes.
        assert_eq!(gun.get("ShotsPerSalvo").and_then(Value::as_f64), Some(3.0));
        assert_eq!(gun.get("ShotPatternCount").and_then(Value::as_f64), Some(3.0));
        let pat = gun.get("ShotPatternPitchYaw").unwrap().as_array().unwrap();
        assert_eq!(pat.len(), 3);
        assert_eq!(pat[0].at(0).and_then(Value::as_f64), Some(-0.2));
        assert_eq!(pat[0].at(1).and_then(Value::as_f64), Some(-1.7));
        assert_eq!(pat[1].at(1).and_then(Value::as_f64), Some(0.1));
        assert_eq!(pat[2].at(1).and_then(Value::as_f64), Some(1.4));
        let base_ord = gun.get("ordnance").unwrap();
        assert_eq!(base_ord.get("class").and_then(Value::as_str), Some("inv_spread_bolt"));
        assert_eq!(base_ord.get("base").and_then(Value::as_str), Some("bolt"));
        assert_eq!(base_ord.get("Velocity").and_then(Value::as_f64), Some(73.0));
        assert_eq!(base_ord.get("Damage").and_then(Value::as_f64), Some(41.0));
        assert_eq!(base_ord.get("DamageFinalDamage").and_then(Value::as_f64), Some(19.0));

        // Charge tier keeps focused 1-bolt shot with cleared pattern and charged ordnance.
        let charges = gun.get("charges").unwrap().as_array().unwrap();
        assert_eq!(charges.len(), 1);
        assert_eq!(charges[0].get("at").and_then(Value::as_f64), Some(0.55));
        assert_eq!(charges[0].get("ShotsPerSalvo").and_then(Value::as_f64), Some(1.0));
        assert_eq!(charges[0].get("ShotPatternPitchYaw").unwrap().as_array().unwrap().len(), 0);
        let hi_ord = charges[0].get("ordnance").unwrap();
        assert_eq!(hi_ord.get("class").and_then(Value::as_str), Some("inv_heavy_bolt"));
        assert_eq!(hi_ord.get("Velocity").and_then(Value::as_f64), Some(215.0));
        assert_eq!(hi_ord.get("Damage").and_then(Value::as_f64), Some(135.0));
    }

    #[test]
    fn swbf2_guided_missile_remote_promotes_only_infantry_weapons_and_keeps_both_explosions() {
        let side_classes = crate::json::parse(
            r#"{
                "ordc": {},
                "expc": {
                    "test_noncrit_exp": {
                        "name": "test_noncrit_exp",
                        "base": "explosion",
                        "properties": [
                            ["Damage", "110.0"],
                            ["DamageRadius", "7.0"],
                            ["Effect", "test_small_fx"]
                        ]
                    },
                    "test_crit_exp": {
                        "name": "test_crit_exp",
                        "base": "explosion",
                        "properties": [
                            ["Damage", "310.0"],
                            ["DamageRadius", "13.0"],
                            ["Effect", "test_big_fx"]
                        ]
                    }
                }
            }"#,
        )
        .unwrap();

        let raw_soldier = crate::json::parse(
            r#"{
                "class": "test_trooper",
                "base": "soldier",
                "properties": [["MaxHealth", "400"]],
                "weapons": [{
                    "class": "test_tube",
                    "base": "remote",
                    "SwitchImmediately": "1",
                    "FirePointExtraOffset": "0.1 0.4 1.5",
                    "ordnance": {
                        "class": "test_seeker_body",
                        "base": "flyer",
                        "GuidedMissile": "1",
                        "GeometryName": "test_shell_mesh",
                        "MinSpeed": "12.0",
                        "MaxSpeed": "18.0",
                        "BoostSpeed": "44.0",
                        "EnergyBar": "100",
                        "EnergyAutoRestore": "-4.0",
                        "EnergyBoostDrain": "15",
                        "GuidedMissileDamage": "250.0",
                        "GuidedMissileDamageBoost": "600.0",
                        "GuidedMissileBoostExplosionSpeedThreshold": "0.75",
                        "TrackOffset": "0.0 1.2 4.0",
                        "ExplosionDestruct": "test_noncrit_exp",
                        "ExplosionCritical": "test_crit_exp"
                    }
                }]
            }"#,
        )
        .unwrap();

        let unit = materialize_swbf2_unit_with_classes(&raw_soldier, Some(&side_classes));
        let gun = &unit.get("weapons").unwrap().as_array().unwrap()[0];
        assert!(gun.get("ordnance").is_none());
        assert_eq!(gun.get("FirePointExtraOffset").and_then(|v| v.at(2)).and_then(Value::as_f64), Some(1.5));
        let droid = gun.get("droid").expect("infantry guided missile promoted to weapon.droid");
        assert_eq!(droid.get("base").and_then(Value::as_str), Some("droid"));
        assert_eq!(droid.get("GuidedMissile").and_then(Value::as_f64), Some(1.0));
        assert_eq!(droid.get("BoostSpeed").and_then(Value::as_f64), Some(44.0));
        assert_eq!(droid.get("TrackOffset").and_then(|v| v.at(2)).and_then(Value::as_f64), Some(4.0));
        let destruct = droid.get("explosion").expect("destruct explosion resolved");
        assert_eq!(destruct.get("Damage").and_then(Value::as_f64), Some(110.0));
        let critical = droid.get("explosionCritical").expect("critical explosion resolved");
        assert_eq!(critical.get("Damage").and_then(Value::as_f64), Some(310.0));

        // A starfighter/gunship remote guided rocket keeps its `ordnance` intact for space flight.
        let raw_craft = crate::json::parse(
            r#"{
                "class": "test_gunship",
                "base": "flyer",
                "properties": [["MaxHealth", "1200"]],
                "weapons": [{
                    "class": "test_craft_missile",
                    "base": "remote",
                    "SwitchImmediately": "1",
                    "ordnance": {
                        "class": "test_craft_missile_body",
                        "base": "flyer",
                        "GuidedMissile": "1",
                        "MinSpeed": "90.0",
                        "MaxSpeed": "110.0",
                        "BoostSpeed": "190.0"
                    }
                }]
            }"#,
        )
        .unwrap();
        let craft = swbf2_unit_definition_with_classes(&raw_craft, false, Some(&side_classes));
        let craft_gun = &craft.get("weapons").unwrap().as_array().unwrap()[0];
        assert!(craft_gun.get("droid").is_none(), "craft weapon must not be rewritten into a recon droid");
        let craft_ord = craft_gun.get("ordnance").expect("craft weapon must keep its ordnance record");
        assert_eq!(craft_ord.get("GuidedMissile").and_then(Value::as_f64), Some(1.0));
        assert_eq!(craft_ord.get("BoostSpeed").and_then(Value::as_f64), Some(190.0));
    }
}
