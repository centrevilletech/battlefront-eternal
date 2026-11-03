//! Which battle a map plays and which side and class load first: ports of
//! src/mission.js (battlesOf, pickMission, selectMission, selectAIMission)
//! and src/spawn-screen/policy.js (spawnPolicy), which decide what a battle
//! fetches. Pure functions of map.json and the battle's query, as there.

use crate::js::to_number;
use crate::value::{Object, Value};
use std::rc::Rc;

/// A query string read as URLSearchParams reads it: `get` is the first
/// value for a name ("" when absent, as `params.get(name) || ""`).
#[derive(Clone, Debug, Default)]
pub struct Query {
    pairs: Vec<(String, String)>,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Query {
    pub fn parse(text: &str) -> Query {
        let text = text.strip_prefix('?').unwrap_or(text);
        let pairs = text
            .split('&')
            .filter(|part| !part.is_empty())
            .map(|part| match part.split_once('=') {
                Some((k, v)) => (percent_decode(k), percent_decode(v)),
                None => (percent_decode(part), String::new()),
            })
            .collect();
        Query { pairs }
    }

    /// `params.get(name) || ""`.
    pub fn get(&self, name: &str) -> &str {
        self.pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or("")
    }
}

pub const ERAS: [&str; 2] = ["cw", "gcw"];
pub const DEFAULT_ERA: &str = "cw";
pub const DEFAULT_UNIT: &str = "rep_inf_jet_trooper";

/// The era an army fights in, or None for a local side.
pub fn era_of_side(side: &str) -> Option<&'static str> {
    match side {
        "rep" | "cis" => Some("cw"),
        "all" | "imp" => Some("gcw"),
        _ => None,
    }
}

/// UTF-16 code unit order (JavaScript's `<` on strings).
pub fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

#[derive(Clone, Debug)]
pub struct BattleInfo {
    pub name: String,
    pub era: Option<Value>,
    pub historical: bool,
    pub sides: Value,
    pub playable: bool,
}

/// `battlesOf(world)`: the battles map.json holds, in name order.
pub fn battles_of(world: &Value) -> Vec<BattleInfo> {
    let mut out: Vec<BattleInfo> = Vec::new();
    if let Some(missions) = world.get("missions").and_then(Value::as_object) {
        for (name, battle) in missions.entries() {
            let mission = battle.get("mission");
            let era = mission.and_then(|m| m.get("era")).filter(|e| e.truthy()).cloned();
            let historical = mission.and_then(|m| m.get("historical")).map(Value::truthy).unwrap_or(false);
            let sides = mission.and_then(|m| m.get("sides")).filter(|s| s.truthy()).cloned().unwrap_or_else(|| Value::Array(Rc::new(Vec::new())));
            let playable = !mission.and_then(|m| m.get("world")).map(Value::truthy).unwrap_or(false);
            out.push(BattleInfo { name: name.to_string(), era, historical, sides, playable });
        }
    }
    // Distinct keys: any stable sort gives the comparator's order.
    out.sort_by(|a, b| utf16_cmp(&a.name, &b.name));
    out
}

fn era_is(era: &Option<Value>, name: &str) -> bool {
    matches!(era, Some(Value::String(s)) if &**s == name)
}

fn includes_str(list: &Value, item: &str) -> bool {
    list.as_array().map(|a| a.iter().any(|v| v.as_str() == Some(item))).unwrap_or(false)
}

/// `pickMission(world, query)`.
pub fn pick_mission(world: &Value, query: &Query) -> Option<String> {
    if world.get("missions").map(Value::truthy) != Some(true) {
        return world.get("mission").and_then(|m| m.get("name")).and_then(Value::as_str).map(str::to_string);
    }
    // reader(): String(query.get(key) || "").toLowerCase()
    let get = |key: &str| query.get(key).to_lowercase();
    let playable: Vec<BattleInfo> = battles_of(world).into_iter().filter(|b| b.playable).collect();
    let wanted = get("mission");
    if let Some(named) = playable.iter().find(|b| b.name == wanted) {
        return Some(named.name.clone());
    }
    let regular: Vec<&BattleInfo> = playable.iter().filter(|b| !b.historical).collect();
    let mut ordered: Vec<&BattleInfo> = regular.iter().copied().filter(|b| era_is(&b.era, DEFAULT_ERA)).collect();
    ordered.extend(regular.iter().copied().filter(|b| !era_is(&b.era, DEFAULT_ERA)));
    let of_era = |era: &str| ordered.iter().find(|b| era_is(&b.era, era)).map(|b| b.name.clone());
    let era = get("era");
    if ERAS.contains(&era.as_str()) {
        if let Some(name) = of_era(&era) {
            return Some(name);
        }
    }
    let side = get("side");
    let by_side = match era_of_side(&side) {
        Some(side_era) => of_era(side_era),
        None => ordered.iter().find(|b| includes_str(&b.sides, &side)).map(|b| b.name.clone()),
    };
    by_side.or_else(|| ordered.first().map(|b| b.name.clone()))
}

/// `selectMission(world, query)`: map.json as the battle plays it, a new
/// object with the picked battle's mission, spawnScreen and vehicles in
/// their places. The parts are shared with map.json's own, as the spread
/// shares them.
pub fn select_mission(world: &Value, query: &Query) -> Value {
    if world.get("missions").map(Value::truthy) != Some(true) {
        return world.clone();
    }
    let name = pick_mission(world, query);
    let battle = name.as_deref().and_then(|n| world.get("missions").and_then(|m| m.get(n))).filter(|b| b.truthy());
    let part = |key: &str| battle.and_then(|b| b.get(key)).filter(|v| v.truthy()).cloned().unwrap_or(Value::Null);
    let mut object: Object = world.as_object().cloned().unwrap_or_default();
    object.set(Rc::from("mission"), part("mission"));
    object.set(Rc::from("spawnScreen"), part("spawnScreen"));
    object.set(Rc::from("vehicles"), part("vehicles"));
    Value::Object(Rc::new(object))
}

/// `selectAIMission(data, mission)`.
pub fn select_ai_mission(data: Option<&Value>, mission: Option<&Value>) -> Option<Value> {
    let data = data?;
    let object = match data.as_object() {
        Some(o) => o,
        None => return Some(data.clone()),
    };
    let name = match mission {
        Some(Value::String(s)) => Some(s.to_string()),
        Some(m) => m.get("name").and_then(Value::as_str).map(str::to_string),
        None => None,
    };
    let scripts = data.get("scripts").and_then(Value::as_object);
    let held = match (&name, scripts) {
        (Some(n), Some(s)) => !n.is_empty() && s.has(n),
        _ => false,
    };
    let mut out = object.clone();
    out.set(Rc::from("mission"), if held { Value::str(name.as_deref().unwrap_or("")) } else { Value::Null });
    let mut variants: Vec<String> = Vec::new();
    if held {
        let n = name.as_deref().unwrap_or("");
        variants = scripts.map(|s| s.keys().filter(|k| *k != n && k.starts_with(n)).map(str::to_string).collect()).unwrap_or_default();
        variants.sort_by(|a, b| utf16_cmp(a, b));
    }
    out.set(Rc::from("variants"), Value::Array(Rc::new(variants.iter().map(|s| Value::str(s)).collect())));
    Some(Value::Object(Rc::new(out)))
}

/// `teamList(mission)`: the teams in number order, [number, team].
fn team_list(mission: Option<&Value>) -> Vec<(f64, Value)> {
    let empty = Value::Object(Rc::new(Object::new()));
    let teams = mission.and_then(|m| m.get("teams")).filter(|t| t.truthy()).unwrap_or(&empty);
    let mut list: Vec<(f64, Value)> = match teams.as_object() {
        Some(o) => o
            .entries()
            .map(|(k, v)| (to_number(Some(&Value::str(k))), if v.truthy() { v.clone() } else { Value::Object(Rc::new(Object::new())) }))
            .collect(),
        None => Vec::new(),
    };
    // Team keys are numbers: the comparator a - b is consistent, so the
    // stable order is V8's.
    list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    list
}

fn class_name_of(entry: &Value) -> Option<&Value> {
    entry.at(0)
}

/// `playableClasses(team)`: the roster without its hero.
pub fn playable_classes(team: Option<&Value>) -> Vec<String> {
    let hero = team.and_then(|t| t.get("hero"));
    let classes = team.and_then(|t| t.get("classes")).and_then(Value::as_array).unwrap_or(&[]);
    classes
        .iter()
        .filter_map(class_name_of)
        .filter(|name| name.truthy() && !hero.map(|h| h.same(name)).unwrap_or(false))
        .filter_map(|name| name.as_str().map(str::to_string))
        .collect()
}

/// `teamOfClass(mission, className)`.
pub fn team_of_class(mission: Option<&Value>, class_name: &str) -> f64 {
    for (team, value) in team_list(mission) {
        let hero = value.get("hero").and_then(Value::as_str) == Some(class_name);
        let listed = value
            .get("classes")
            .and_then(Value::as_array)
            .map(|c| c.iter().any(|e| class_name_of(e).and_then(Value::as_str) == Some(class_name)))
            .unwrap_or(false);
        if hero || listed {
            return team;
        }
    }
    1.0
}

/// `sideOfClass(mission, className, fallback)`.
pub fn side_of_class(mission: Option<&Value>, class_name: &str, fallback: &str) -> String {
    let prefix = class_name.split('_').next().unwrap_or("").to_lowercase();
    let listed = match mission.and_then(|m| m.get("sides")).filter(|s| s.truthy()) {
        Some(sides) => includes_str(sides, &prefix),
        None => ["rep", "cis", "imp", "all"].contains(&prefix.as_str()),
    };
    if listed {
        prefix
    } else {
        fallback.to_string()
    }
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub screen: bool,
    pub side: String,
    pub class_name: String,
    pub team: f64,
    pub post: Option<String>,
}

/// `spawnPolicy(query, mission)`.
pub fn spawn_policy(query: &Query, mission: Option<&Value>) -> Policy {
    let network = !query.get("network").is_empty();
    let side = query.get("side").to_lowercase();
    let unit = if network { "" } else { query.get("unit") };
    let post = if network { None } else { Some(query.get("spawn")).filter(|s| !s.is_empty()).map(str::to_string) };
    let forced = query.get("spawnscreen");
    let screen = match forced {
        "1" => true,
        "0" => false,
        _ => network || !(!side.is_empty() || !unit.is_empty() || post.is_some()),
    };
    let mut class_name = unit.to_string();
    if class_name.is_empty() && !side.is_empty() {
        let prefix = format!("{side}_");
        for (_, team) in team_list(mission) {
            class_name = playable_classes(Some(&team)).into_iter().find(|n| n.to_lowercase().starts_with(&prefix)).unwrap_or_default();
            if !class_name.is_empty() {
                break;
            }
        }
    }
    if class_name.is_empty() && screen {
        let number = team_of_class(mission, DEFAULT_UNIT);
        let key = crate::js::number_to_string(number);
        let team = mission.and_then(|m| m.get("teams")).and_then(|t| t.get(&key));
        class_name = playable_classes(team).into_iter().next().unwrap_or_default();
    }
    if class_name.is_empty() {
        class_name = DEFAULT_UNIT.to_string();
    }
    let team = team_of_class(mission, &class_name);
    let side_name = if class_name.to_lowercase().starts_with(&format!("{side}_")) {
        side.clone()
    } else {
        let own = side_of_class(mission, &class_name, "");
        if !own.is_empty() {
            own
        } else if !side.is_empty() {
            side.clone()
        } else {
            side_of_class(mission, &class_name, "rep")
        }
    };
    Policy { screen, side: side_name, class_name, team, post }
}
