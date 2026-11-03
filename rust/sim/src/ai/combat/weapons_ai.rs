//! How a Classic bot uses its weapons (src/ai/classic/weapons-ai.js): what
//! it knows about each weapon, which weapon it picks for a target, and how
//! its controller's two fire requests become trigger presses.

use std::collections::HashMap;
use std::rc::Rc;
use three::Vector3;

use super::aim::{is_cannon, is_launcher, lead_point, resolve};
use super::math::{angle, dot32, f, length_sq32, lookup_cos, normalize_or_x, FLT_MAX};
use super::world::{draw_int, draw_range, index_of, Gen, Obj, QueryFilter, Ref, UnitView, World};
use crate::combat::defs::{Js, JsObj};

const INT_MAX: f64 = 2147483647.0;

/// weapons-ai.js:48 `ENGINE_DEFAULTS`.
pub const DEFAULT_MIN_RANGE: f64 = 0.0;
pub const DEFAULT_OPTIMAL_RANGE: f64 = 30.0;
pub const DEFAULT_MAX_RANGE: f64 = 60.0;
pub const DEFAULT_RELATION_MASK: u32 = 1;
pub const DEFAULT_CATEGORY_MASK: u32 = 0xffff_ff9f;
pub const DEFAULT_BARRAGE_MIN: f64 = 3.0;
pub const DEFAULT_BARRAGE_MAX: f64 = 6.0;
pub const DEFAULT_BARRAGE_DELAY: f64 = 2.0;

/// A weapon class record's AI view (weapons-ai.js:76).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponAi {
    pub min_range: f64,
    pub optimal_range: f64,
    pub max_range: f64,
    /// `implicitRange` (truthy).
    pub implicit_range: bool,
    pub extreme_range: f64,
    pub relation_mask: u32,
    pub category_mask: u32,
    pub barrage_min: f64,
    pub barrage_max: f64,
    pub barrage_delay: f64,
    /// `triggerSingle` (truthy).
    pub trigger_single: bool,
    pub zoom_max: f64,
    pub action: f64,
}

/// A side's ai.json weapon record as registerAIData keeps it (its `ai`,
/// and its charge levels' `at` and `ai`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registered {
    pub ai: Js,
    pub levels: Vec<(Js, Js)>,
}

/// weapons-ai.js:61-62: the weapons' AI records by class name. The
/// JavaScript's map is the process's and is never cleared (quirks
/// P16-1); a battle holds its own here.
#[derive(Clone, Debug, Default)]
pub struct WeaponAiTable {
    pub registered: HashMap<String, Registered>,
}

impl WeaponAiTable {
    /// weapons-ai.js:151 `registerAIData(aiJson)`: every weapon record
    /// with an `ai` (the later side's wins a shared name).
    pub fn register(&mut self, weapons: &[(String, Registered)]) -> usize {
        let mut n = 0;
        for (name, rec) in weapons {
            if !rec.ai.truthy() {
                continue;
            }
            self.registered.insert(name.clone(), rec.clone());
            n += 1;
        }
        n
    }

    /// weapons-ai.js:137 `registeredAI(def)`.
    fn registered_ai(&self, def: &JsObj) -> Option<Js> {
        let class = def.get("class").str()?;
        let rec = self.registered.get(class)?;
        let at = def.get("at");
        if !at.is_undefined() && !rec.levels.is_empty() {
            let at = f(at.num());
            if let Some((_, ai)) = rec.levels.iter().find(|(l_at, _)| f(l_at.num()) == at) {
                if ai.truthy() {
                    return Some(ai.clone());
                }
            }
        }
        if rec.ai.is_nullish() {
            None
        } else {
            Some(rec.ai.clone())
        }
    }

    /// weapons-ai.js:67 `weaponAI(def)`: its own `ai`, else the registered
    /// record of its class, else the engine defaults and its flags.
    pub fn weapon_ai(&self, def: &JsObj) -> WeaponAi {
        let own = def.get("ai");
        let rec: Option<Js> = if own.is_nullish() { self.registered_ai(def) } else { Some(own.clone()) };
        let rec = rec.filter(|r| r.truthy());
        let undefined = Js::Undefined;
        let ai_get = |key: &str| -> Js {
            match &rec {
                Some(r) => r.get(key).clone(),
                None => undefined.clone(),
            }
        };
        // Without a record, the flattened keys a class record may carry.
        let raw = |key: &str| -> Option<f64> {
            if rec.is_some() {
                None
            } else {
                num(def.get(key))
            }
        };
        let flags = if rec.is_none() { Some(mask_from_flags(def)) } else { None };
        let pick = |key: &str, raw_key: &str, d: f64| -> f64 {
            let v = ai_get(key);
            if !v.is_nullish() {
                v.num()
            } else {
                raw(raw_key).unwrap_or(d)
            }
        };
        let implicit = {
            let v = ai_get("implicitRange");
            if !v.is_nullish() {
                v.truthy()
            } else {
                raw("MaxRange").is_none()
            }
        };
        let extreme = {
            let v = ai_get("extremeRange");
            if !v.is_nullish() {
                v.num()
            } else {
                let e = num(def.get("ExtremeRange"));
                let l = num(def.get("LockOnRange"));
                // num(x) || 0
                let e = match e {
                    Some(x) if !(x == 0.0 || x.is_nan()) => x,
                    _ => 0.0,
                };
                let l = match l {
                    Some(x) if !(x == 0.0 || x.is_nan()) => x,
                    _ => 0.0,
                };
                v8math::max(e, l)
            }
        };
        let relation = {
            let v = ai_get("relationMask");
            if !v.is_nullish() {
                v8math::to_uint32(v.num())
            } else {
                flags.map(|f| f.0).unwrap_or(DEFAULT_RELATION_MASK)
            }
        };
        let category = {
            let v = ai_get("categoryMask");
            if !v.is_nullish() {
                v8math::to_uint32(v.num())
            } else {
                flags.map(|f| f.1).unwrap_or(DEFAULT_CATEGORY_MASK)
            }
        };
        let trigger_single = def.get("base").is_str("melee") || {
            let v = ai_get("triggerSingle");
            if !v.is_nullish() {
                v.truthy()
            } else {
                def.get("TriggerSingle").num() > 0.0
            }
        };
        let zoom_max = {
            let v = ai_get("zoomMax");
            if !v.is_nullish() {
                v.num()
            } else {
                def.get("ZoomMax").or(1.0)
            }
        };
        let action = {
            let v = ai_get("action");
            if !v.is_nullish() {
                v.num()
            } else {
                action_of(def.get("AnimationBank"))
            }
        };
        WeaponAi {
            min_range: f(pick("minRange", "MinRange", DEFAULT_MIN_RANGE)),
            optimal_range: f(pick("optimalRange", "OptimalRange", DEFAULT_OPTIMAL_RANGE)),
            max_range: f(pick("maxRange", "MaxRange", DEFAULT_MAX_RANGE)),
            implicit_range: implicit,
            extreme_range: f(extreme),
            relation_mask: relation,
            category_mask: category,
            barrage_min: pick("barrageMin", "BarrageMin", DEFAULT_BARRAGE_MIN),
            barrage_max: pick("barrageMax", "BarrageMax", DEFAULT_BARRAGE_MAX),
            barrage_delay: f(pick("barrageDelay", "BarrageDelay", DEFAULT_BARRAGE_DELAY)),
            trigger_single,
            zoom_max: f(zoom_max),
            action,
        }
    }
}

/// weapons-ai.js:100 `num(value)`: a flattened key's number
/// (Number.parseFloat of its text), undefined for nothing or no number.
pub fn num(value: &Js) -> Option<f64> {
    let text = match value {
        Js::Undefined | Js::Null => return None,
        Js::Str(s) if s.is_empty() => return None,
        Js::Str(s) => s.to_string(),
        Js::Num(x) => jsrt::numfmt::number_to_string(*x),
        Js::Bool(b) => if *b { "true".into() } else { "false".into() },
        Js::Arr(items) => items
            .iter()
            .map(|x| match x {
                Js::Undefined | Js::Null => String::new(),
                Js::Num(n) => jsrt::numfmt::number_to_string(*n),
                Js::Str(s) => s.to_string(),
                Js::Bool(b) => b.to_string(),
                _ => "[object Object]".into(),
            })
            .collect::<Vec<_>>()
            .join(","),
        Js::Obj(_) => "[object Object]".into(),
    };
    let n = bfe_content::js::parse_float(&text);
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// weapons-ai.js:113 `maskFromFlags(def)`: TargetAll first, then each
/// flag its bit (bitwise in 32 bits, as JavaScript's `|`, `&` and `>>> 0`).
fn mask_from_flags(def: &JsObj) -> (u32, u32) {
    let mut relation = DEFAULT_RELATION_MASK;
    let mut category = DEFAULT_CATEGORY_MASK;
    let truthy_number = |v: &Js| {
        let n = v.num();
        !(n == 0.0 || n.is_nan())
    };
    let all = def.get("TargetAll");
    if !all.is_undefined() {
        category = if truthy_number(all) { 0xffff_ffff } else { 0 };
    }
    for (key, bit) in [("TargetEnemy", 1u32), ("TargetNeutral", 2), ("TargetFriendly", 4)] {
        let v = def.get(key);
        if !v.is_undefined() {
            relation = if truthy_number(v) { relation | bit } else { relation & !bit };
        }
    }
    for (key, bit) in [
        ("TargetPerson", 1u32),
        ("TargetAnimal", 2),
        ("TargetDroid", 4),
        ("TargetVehicle", 8),
        ("TargetBuilding", 16),
        ("TargetBuildingDead", 32),
        ("TargetBuildingUnbuilt", 64),
    ] {
        let v = def.get(key);
        if !v.is_undefined() {
            category = if truthy_number(v) { category | bit } else { category & !bit };
        }
    }
    (relation, category)
}

/// String(x) of a definition's value (for its lower-cased names).
pub fn js_string(v: &Js) -> String {
    match v {
        Js::Undefined => "undefined".into(),
        Js::Null => "null".into(),
        Js::Bool(b) => b.to_string(),
        Js::Num(x) => jsrt::numfmt::number_to_string(*x),
        Js::Str(s) => s.to_string(),
        Js::Arr(items) => items.iter().map(|x| if x.is_nullish() { String::new() } else { js_string(x) }).collect::<Vec<_>>().join(","),
        Js::Obj(_) => "[object Object]".into(),
    }
}

/// weapons-ai.js:128 `actionOf(bank)`: the AnimationBank's action class.
pub fn action_of(bank: &Js) -> f64 {
    const BANKS: [&str; 10] = ["rifle", "bazooka", "pistol", "grenade", "tool", "lightsabre", "sbdroid", "bdroid", "cannon", "gam"];
    const ACTION: [f64; 10] = [1.0, 3.0, 0.0, 2.0, 6.0, 4.0, 1.0, 1.0, 3.0, 4.0];
    let text = if bank.is_nullish() { String::new() } else { js_string(bank) };
    let lower = text.to_lowercase();
    let i = BANKS.iter().position(|b| *b == lower).unwrap_or(0);
    ACTION[i]
}

/// weapons-ai.js:133 `ACTION`.
pub mod action {
    pub const PISTOL: f64 = 0.0;
    pub const RIFLE: f64 = 1.0;
    pub const GRENADE: f64 = 2.0;
    pub const LAUNCHER: f64 = 3.0;
    pub const MELEE: f64 = 4.0;
    pub const TOOL: f64 = 6.0;
}

// ---------------------------------------------------------------------------
// Entity facts (weapons-ai.js:193).

/// weapons-ai.js:193 `infantryKind`.
fn infantry_kind(kind: f64) -> bool {
    kind == 0.0 || kind == 1.0 || kind == 3.0
}

/// weapons-ai.js:199 `forwardSpeed(unit)`: the class's ForwardSpeed, else
/// MaxSpeed, as float32.
pub fn forward_speed(u: &UnitView) -> f64 {
    let Some(c) = &u.class else { return 0.0 };
    let v = if !c.forward_speed.is_nullish() {
        c.forward_speed.num()
    } else if !c.max_speed.is_nullish() {
        c.max_speed.num()
    } else {
        0.0
    };
    f(v)
}

/// weapons-ai.js:209 `categoryOf(unit)`: person 0, animal 1, droid 2,
/// vehicle 3, building 4 (NaN for a HealthType naming an inherited
/// property of a plain object: quirks P16-3).
pub fn category_of(u: &UnitView) -> f64 {
    if let Some(c) = u.category {
        if c.is_finite() && c.fract() == 0.0 {
            return c;
        }
    }
    let raw = u.class.as_ref().map(|c| c.health_type.clone()).unwrap_or_default();
    let text = if raw.is_nullish() { String::new() } else { js_string(&raw) };
    match text.to_lowercase().as_str() {
        "person" => return 0.0,
        "animal" => return 1.0,
        "droid" => return 2.0,
        "vehicle" => return 3.0,
        "building" => return 4.0,
        // `type in CATEGORIES` finds Object.prototype's own lower-case
        // names; CATEGORIES[type] is then a function or an object.
        "constructor" | "__proto__" => return f64::NAN,
        _ => {}
    }
    let k = u.kind;
    if k == 2.0 || k == 5.0 || k == 6.0 {
        return 3.0;
    }
    if k == 8.0 || k == 9.0 {
        return 4.0;
    }
    0.0
}

/// `1 << x` (ToInt32, shift count & 31).
pub fn shl1(x: f64) -> u32 {
    1u32 << ((v8math::to_int32(x) as u32) & 31)
}

/// weapons-ai.js:220 `relationBit(relation)`.
pub fn relation_bit(relation: f64) -> u32 {
    shl1(relation + 1.0)
}

// ---------------------------------------------------------------------------
// Ranges (weapons-ai.js:230).

/// weapons-ai.js:230 `effectiveRange(unit, controller, def)`.
pub fn effective_range(table: &WeaponAiTable, kind: f64, range_scale: Option<f64>, def: &JsObj) -> f64 {
    let a = table.weapon_ai(def);
    let mut r = a.max_range;
    if a.implicit_range && !infantry_kind(kind) {
        r = f(r + r);
    }
    let k = f(range_scale.unwrap_or(1.0));
    let kr = f(k * r);
    if kr <= a.extreme_range {
        return kr;
    }
    if a.extreme_range <= r {
        r
    } else {
        a.extreme_range
    }
}

/// weapons-ai.js:242 `expandedRange(unit, controller, def)`.
pub fn expanded_range(table: &WeaponAiTable, u: &UnitView, range_scale: Option<f64>, def: &JsObj) -> f64 {
    f(effective_range(table, u.kind, range_scale, def) + f(forward_speed(u) * 5.0))
}

/// weapons-ai.js:248 `maxExpandedRange(unit, controller)`.
pub fn max_expanded_range(table: &WeaponAiTable, u: &UnitView, range_scale: Option<f64>) -> f64 {
    let mut best = 0.0;
    let Some(arsenal) = &u.arsenal else { return best };
    for w in &arsenal.weapons {
        let levels: Vec<&Rc<JsObj>> = match &w.levels {
            Some(l) => l.iter().collect(),
            None => vec![&w.def],
        };
        for level in levels {
            let r = expanded_range(table, u, range_scale, level);
            if best <= r {
                best = r;
            }
        }
    }
    best
}

/// weapons-ai.js:544 `selectedWeapon(unit, controller)`: the current
/// weapon of the AI's channel (the arsenal index; no fallback).
pub fn selected_weapon<W: World + ?Sized>(w: &W, unit: Obj, controller: Option<Obj>) -> Option<usize> {
    let u = w.unit(unit);
    let arsenal = u.arsenal.as_ref()?;
    let channel = controller.and_then(|c| w.ctl(c).perception.as_ref().map(|p| p.fire.channel)).unwrap_or(0.0);
    arsenal.current(channel)
}

/// The unit's weapon `index`'s current def.
pub fn def_at<W: World + ?Sized>(w: &W, unit: Obj, index: usize) -> Rc<JsObj> {
    w.unit(unit).arsenal.as_ref().expect("an arsenal").weapons[index].def.clone()
}

/// weapons-ai.js:262 `sensorRange(unit, controller)`.
pub fn sensor_range<W: World + ?Sized>(w: &W, table: &WeaponAiTable, unit: Obj, controller: Obj) -> f64 {
    match selected_weapon(w, unit, Some(controller)) {
        Some(i) => {
            let def = def_at(w, unit, i);
            let rs = w.ctl(controller).range_scale;
            expanded_range(table, w.unit(unit), rs, &def)
        }
        None => 0.0,
    }
}

/// weapons-ai.js:270 `lowerBand(unit, controller)`.
pub fn lower_band<W: World + ?Sized>(w: &W, table: &WeaponAiTable, unit: Obj, controller: Obj) -> f64 {
    let Some(i) = selected_weapon(w, unit, Some(controller)) else { return 0.0 };
    let a = table.weapon_ai(&def_at(w, unit, i));
    let lo = f(a.min_range + 10.0);
    if a.optimal_range <= lo {
        a.optimal_range
    } else {
        lo
    }
}

/// weapons-ai.js:277 `upperBand(unit, controller, sight)`.
pub fn upper_band<W: World + ?Sized>(w: &W, table: &WeaponAiTable, unit: Obj, controller: Obj, sight: f64) -> f64 {
    let Some(i) = selected_weapon(w, unit, Some(controller)) else { return 0.0 };
    let def = def_at(w, unit, i);
    let a = table.weapon_ai(&def);
    let e = f(effective_range(table, w.unit(unit).kind, w.ctl(controller).range_scale, &def) - 10.0);
    let hi = if e <= a.optimal_range { a.optimal_range } else { e };
    let s = f(sight - 10.0);
    if s <= hi {
        s
    } else {
        hi
    }
}

// ---------------------------------------------------------------------------
// Ammunition (weapons-ai.js:292).

/// weapons-ai.js:292 `ammoCount(weapon)`.
pub fn ammo_count(wv: &super::world::WeaponView) -> f64 {
    if wv.bottomless || wv.reserve == f64::INFINITY {
        return INT_MAX;
    }
    v8math::trunc(f(f(wv.clip + wv.reserve) * wv.rounds.num()))
}

/// weapons-ai.js:300 `capacity(weapon)`.
pub fn capacity(wv: &super::world::WeaponView) -> f64 {
    let ammo = wv.def.get("ammo");
    let clips = if ammo.num() > 0.0 { ammo.num() } else { FLT_MAX };
    let rounds = if wv.bottomless { INT_MAX } else { wv.rounds.nn(1.0) };
    f(clips * rounds)
}

// ---------------------------------------------------------------------------
// Utility (weapons-ai.js:310).

const SCALE_KEYS: [&str; 5] = ["PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale"];

/// weapons-ai.js:312 `typeScale(rec, category)`.
fn type_scale(rec: &Js, category: f64) -> f64 {
    let key = index_of(category).and_then(|i| SCALE_KEYS.get(i).copied());
    match key {
        Some(k) => f(rec.get(k).nn(1.0)),
        None => f(1.0),
    }
}

/// weapons-ai.js:318 `blastFalloff`.
fn blast_falloff(distance: f64, inner: f64, outer: f64, radius: f64) -> f64 {
    let d = if radius > 0.0 { f(distance - f(radius * 0.5)) } else { f(distance) };
    if d < inner {
        return 1.0;
    }
    if d < outer {
        return f(f(outer - d) / f(outer - inner));
    }
    0.0
}

/// weapons-ai.js:326 `blastDamage`.
fn blast_damage(explosion: &Js, category: f64, radius: f64) -> f64 {
    if !explosion.truthy() || !matches!(explosion, Js::Obj(_) | Js::Arr(_)) {
        return 0.0;
    }
    let fall = blast_falloff(2.0, f(explosion.get("DamageRadiusInner").or(0.0)), f(explosion.get("DamageRadiusOuter").or(0.0)), radius);
    let value = f(fall * f(explosion.get("Damage").or(0.0)));
    f(type_scale(explosion, category) * value)
}

/// weapons-ai.js:394 `maxHealthOf(unit)`.
fn max_health_of(u: &UnitView) -> f64 {
    let v = if !u.combat_max_health.is_nullish() {
        u.combat_max_health.num()
    } else {
        let c = u.class.as_ref().map(|c| c.max_health.clone()).unwrap_or_default();
        if !c.is_nullish() {
            c.num()
        } else if !u.max_health.is_nullish() {
            u.max_health.num()
        } else {
            0.0
        }
    };
    f(v)
}

/// weapons-ai.js:338 `weaponUtility(owner, weapon, target)`: (ok, score).
pub fn weapon_utility<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, owner: Obj, weapon: usize, target: Obj) -> (bool, f64) {
    let def = def_at(w, owner, weapon);
    let ordnance = def.get("ordnance").clone();
    let owner_kind = w.unit(owner).kind;
    let owner_soldier = owner_kind == 0.0;
    let owner_hero = w.unit(owner).hero;
    let t = w.unit(target);
    let target_kind = t.kind;
    let accept = |score: f64| (true, f(score));
    let base = def.get("base");
    if !ordnance.truthy() && base.is_str("melee") && (target_kind == 0.0 || target_kind == 3.0) {
        return accept(1000.0);
    }
    if owner_soldier && owner_hero && is_cannon(Some(&def)) {
        return accept(1000.0);
    }
    if t.trap {
        return (false, 0.0);
    }
    if owner_soldier && target_kind == 0.0 && table.weapon_ai(&def).category_mask & shl1(category_of(t)) == 0 {
        return (false, 0.0);
    }
    if !ordnance.truthy() {
        return (false, 0.0);
    }
    let category = category_of(t);
    let radius = w.sphere(target).radius;
    let mut damage = f(f(ordnance.get("Damage").or(0.0)) * type_scale(&ordnance, category));
    damage = f(blast_damage(ordnance.get("ExplosionImpact"), category, radius) + damage);
    damage = f(blast_damage(ordnance.get("ExplosionExpire"), category, radius) + damage);
    let wv = &w.unit(owner).arsenal.as_ref().unwrap().weapons[weapon];
    let cap = capacity(wv);
    let value = f((if cap <= 50.0 { cap } else { 50.0 }) * damage);
    let t = w.unit(target);
    if f(max_health_of(t) * 0.5) < value {
        return accept(damage);
    }
    let ok = owner_kind;
    if (ok == 6.0 || ok == 2.0 || ok == 8.0 || ok == 7.0) && damage > 0.0 {
        return accept(damage);
    }
    if target_kind != 6.0 && target_kind != 2.0 && (!(max_health_of(t) < FLT_MAX) || !(target_kind == 9.0 || target_kind == 8.0)) {
        return (false, 0.0);
    }
    if !(damage > 0.0) {
        return (false, 0.0);
    }
    let b = ordnance.get("base");
    let b = b.str();
    if matches!(b, Some("grenade" | "sticky" | "haywire" | "beacon")) {
        if let Some(v) = t.velocity {
            if length_sq32(v.x, v.y, v.z) < 144.0 {
                return accept(damage);
            }
        }
        return (false, 0.0);
    }
    if matches!(b, Some("beam" | "missile" | "shell")) {
        return accept(damage);
    }
    (false, 0.0)
}

// ---------------------------------------------------------------------------
// Weapon selection (weapons-ai.js:412).

/// selectWeapon's answer (weapons-ai.js:409).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    pub ok: bool,
    pub index: Option<usize>,
    pub action: f64,
    pub in_band: bool,
}

impl Default for Choice {
    fn default() -> Self {
        Choice { ok: false, index: None, action: 6.0, in_band: false }
    }
}

/// The per-target-controller cluster timestamps (weapons-ai.js:487).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterState {
    pub scan_at: f64,
    pub flagged_at: f64,
    pub generation: f64,
}

/// weapons-ai.js:487 `clusters`: keyed by the target's controller, else
/// the target (a module WeakMap in the JavaScript; the battle's here).
#[derive(Clone, Debug, Default)]
pub struct Clusters {
    pub states: HashMap<Obj, ClusterState>,
}

impl Clusters {
    /// weapons-ai.js:489 `clusterState(target, create)`: the key's state
    /// (made when asked), reset when a controller-less target's life moved
    /// on. Returns the key.
    pub fn state(&mut self, target: Obj, controller: Option<Obj>, generation: f64, create: bool) -> Option<Obj> {
        let key = controller.unwrap_or(target);
        if !self.states.contains_key(&key) && create {
            self.states.insert(key, ClusterState { scan_at: f64::NEG_INFINITY, flagged_at: f64::NEG_INFINITY, generation });
        }
        let s = self.states.get_mut(&key)?;
        if controller.is_none() && s.generation != generation {
            s.scan_at = f64::NEG_INFINITY;
            s.flagged_at = f64::NEG_INFINITY;
            s.generation = generation;
        }
        Some(key)
    }
}

/// weapons-ai.js:475 `grenadePriority`.
fn grenade_priority(clusters: &mut Clusters, controller_roll: Option<f64>, target: &UnitView, def: &JsObj, owner_soldier: bool, now: f64) -> bool {
    if !owner_soldier || !def.get("base").is_str("grenade") {
        return false;
    }
    if let Some(key) = clusters.state(target.obj, target.controller, target.generation, false) {
        let s = clusters.states[&key];
        if f(now) < f(s.flagged_at + 5.0) {
            return true;
        }
    }
    let id = controller_roll.unwrap_or(0.0);
    id % 10.0 == v8math::trunc(v8math::trunc(now) / 3.0) % 10.0
}

/// weapons-ai.js:412 `selectWeapon(controller, unit, target, switchAllowed,
/// world)`.
pub fn select_weapon<W: World + ?Sized>(w: &mut W, cx: &mut Shared, controller: Option<Obj>, unit: Obj, target: Option<Obj>, switch_allowed: bool) -> Choice {
    let mut out = Choice::default();
    let count = w.unit(unit).arsenal.as_ref().map_or(0, |a| a.weapons.len());
    let Some(target) = target else { return out };
    if count == 0 {
        return out;
    }
    let now = w.time();
    let origin = w.origin(unit);
    let aim = w.aim_point(target);
    let d2 = length_sq32(f(aim.x - origin.x), f(aim.y - origin.y), f(aim.z - origin.z));
    let owner_soldier = w.unit(unit).kind == 0.0;
    let range_scale = controller.map(|c| w.ctl(c).range_scale).unwrap_or(None);
    let roll = controller.map(|c| w.ctl(c).roll);
    let mut best: Option<usize> = None;
    let mut best_score = -1.0;
    let mut best_big = false;
    let mut best_in_band = false;
    for i in 0..count {
        if !(ammo_count(&w.unit(unit).arsenal.as_ref().unwrap().weapons[i]) > 0.0) {
            continue;
        }
        let (ok, score) = weapon_utility(w, &cx.weapons, unit, i, target);
        if !ok {
            continue;
        }
        let def = def_at(w, unit, i);
        let cap = capacity(&w.unit(unit).arsenal.as_ref().unwrap().weapons[i]);
        let big = cap > 20.0;
        let better = best_score < score;
        let a = cx.weapons.weapon_ai(&def);
        let range = effective_range(&cx.weapons, w.unit(unit).kind, range_scale, &def);
        if d2 <= f(a.min_range * a.min_range) || f(range * range) <= d2 {
            // Out of the band: a fallback while nothing in band is found.
            if best_in_band {
                continue;
            }
            let far = expanded_range(&cx.weapons, w.unit(unit), range_scale, &def);
            if !(d2 < f(far * far)) {
                continue;
            }
            if if best_big { big && better } else { big || better } {
                best_score = score;
                best = Some(i);
                best_big = big;
            }
            continue;
        }
        let mut take = !best_in_band || (if best_big { big && better } else { big || better });
        let t = w.unit(target);
        if grenade_priority(&mut cx.clusters, roll, t, &def, owner_soldier, now) {
            take = true;
        }
        if take {
            best_big = big;
            best_in_band = true;
            best_score = score;
            best = Some(i);
        }
    }
    let Some(best) = best else { return out };
    out.ok = true;
    out.index = Some(best);
    out.action = cx.weapons.weapon_ai(&def_at(w, unit, best)).action;
    out.in_band = best_in_band;
    if switch_allowed && selected_weapon(w, unit, controller) != Some(best) {
        if let Some(c) = controller {
            switch_weapon(w, c, unit, best);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The controller's weapon state (weapons-ai.js:508).

/// A run-length button (weapons-ai.js:511).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Button {
    pub count: f64,
    pub code: f64,
}

/// The automatic cannon's burst (weapons-ai.js:517).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub state: f64,
    pub heat_limit: f64,
    pub last_end: f64,
    pub saved_ammo: f64,
    pub count: f64,
}

impl Default for Burst {
    fn default() -> Self {
        Burst { state: 0.0, heat_limit: 0.0, last_end: -100.0, saved_ammo: 0.0, count: 0.0 }
    }
}

/// weapons-ai.js:508 `FireControl`.
#[derive(Clone, Debug, PartialEq)]
pub struct FireControl {
    pub channel: f64,
    pub buttons: [Button; 2],
    pub last_release: f64,
    pub charging: bool,
    pub hold: f64,
    pub sniper_active: bool,
    pub sniper_deadline: f64,
    pub burst: Burst,
    pub weapon_target: Option<Ref>,
    pub protected: Vec<Ref>,
}

impl Default for FireControl {
    fn default() -> Self {
        FireControl {
            channel: 0.0,
            buttons: [Button::default(); 2],
            last_release: -1.0,
            charging: false,
            hold: 0.0,
            sniper_active: false,
            sniper_deadline: 0.0,
            burst: Burst::default(),
            weapon_target: None,
            protected: Vec::new(),
        }
    }
}

impl FireControl {
    /// weapons-ai.js:522 `reset()` (the burst keeps all but its state).
    pub fn reset(&mut self) {
        self.channel = 0.0;
        for b in &mut self.buttons {
            b.count = 0.0;
            b.code = 0.0;
        }
        self.last_release = -1.0;
        self.charging = false;
        self.hold = 0.0;
        self.sniper_active = false;
        self.sniper_deadline = 0.0;
        self.burst.state = 0.0;
        self.weapon_target = None;
        self.protected.clear();
    }
}

/// What the combat services share across controllers: the weapons' AI
/// records, the cluster timestamps.
#[derive(Clone, Debug, Default)]
pub struct Shared {
    pub weapons: WeaponAiTable,
    pub clusters: Clusters,
}

fn fire_mut<W: World + ?Sized>(w: &mut W, controller: Obj) -> Option<&mut FireControl> {
    w.ctl_mut(controller).perception.as_mut().map(|p| &mut p.fire)
}

/// weapons-ai.js:558 `switchWeapon(controller, unit, index)`.
pub fn switch_weapon<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj, index: usize) -> bool {
    let has_fire = w.ctl(controller).perception.is_some();
    let channel = match w.unit(unit).arsenal.as_ref().and_then(|a| a.weapons.get(index)) {
        Some(wv) => wv.channel.clone(),
        None => return false,
    };
    if !has_fire {
        return false;
    }
    let fire = fire_mut(w, controller).unwrap();
    fire.sniper_active = false;
    fire.last_release = -1.0;
    fire.charging = false;
    fire.burst.state = 0.0;
    fire.channel = channel.num();
    w.arsenal_select(unit, index)
}

/// weapons-ai.js:587 `setTarget(controller, unit, target, world)`.
pub fn set_target<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Option<Obj>, target: Option<Obj>) {
    let r = target.map(|t| Ref { obj: t, generation: w.unit(t).generation });
    w.ctl_mut(controller).target = r;
    if w.ctl(controller).perception.is_none() {
        return;
    }
    let Some(unit) = unit else { return };
    let Some(index) = selected_weapon(w, unit, Some(controller)) else { return };
    let mut kept = target;
    if let Some(t) = target {
        let a = table.weapon_ai(&def_at(w, unit, index));
        let (ut, tt) = (w.unit(unit).team, w.unit(t).team);
        let rel = w.relation(ut, tt);
        if a.relation_mask & relation_bit(rel) == 0 || a.category_mask & shl1(category_of(w.unit(t))) == 0 {
            kept = None;
        }
    }
    let kr = kept.map(|k| Ref { obj: k, generation: w.unit(k).generation });
    fire_mut(w, controller).unwrap().weapon_target = kr;
}

/// weapons-ai.js:609 `pressButton(button, down)`.
pub fn press_button(button: &mut Button, down: bool) {
    let c = button.count;
    if !down {
        if c == 0.0 {
            button.count = -2.0;
        } else if c > 0.0 {
            button.code = if c < 70.0 { 1.0 } else { 0.0 };
            button.count = -1.0;
        } else {
            button.count = v8math::max(c - 1.0, -100.0);
        }
        return;
    }
    if c == 0.0 {
        button.count = 2.0;
    } else if c < 0.0 {
        if button.code == 1.0 {
            button.code = 2.0;
        }
        button.count = 1.0;
    } else {
        button.code = if c < 70.0 { 0.0 } else { 3.0 };
        button.count = v8math::min(c + 1.0, 100.0);
    }
}

/// weapons-ai.js:633 `triggerFromCounts(previous, current)`: (held, pressed).
pub fn trigger_from_counts(previous: f64, current: f64) -> (bool, bool) {
    (current > 0.0, current > 0.0 && !(previous > 0.0))
}

// ---------------------------------------------------------------------------
// The fire-request consumer (weapons-ai.js:666).

/// weapons-ai.js:666 `consumeFire(controller, unit, world)`: writes the
/// unit's intent buttons and returns the shaped request.
pub fn consume_fire<W: World + ?Sized>(w: &mut W, cx: &mut Shared, controller: Obj, unit: Option<Obj>) -> bool {
    if w.ctl(controller).perception.is_none() {
        return false;
    }
    let Some(unit) = unit else { return false };
    let now = w.time();
    let weapon = selected_weapon(w, unit, Some(controller));
    let (fire_a, fire_b, target) = {
        let c = w.ctl(controller);
        (c.fire_a, c.fire_b, c.target)
    };
    let mut request = fire_a;
    if let Some(index) = weapon {
        if !fire_a && fire_b {
            if let Some(t) = resolve(w, target) {
                request = validate_fire(w, &cx.weapons, controller, unit, t);
            }
        }
        let def = def_at(w, unit, index);
        if charge_weapon(&w.unit(unit).arsenal.as_ref().unwrap().weapons[index]) {
            request = charge_request(w, controller, unit, index, request, now);
        } else if !cx.weapons.weapon_ai(&def).trigger_single {
            if is_cannon(Some(&def)) {
                request = burst_request(w, &cx.weapons, controller, unit, index, request, now);
            }
        } else {
            request = semiauto_request(w, &cx.weapons, controller, unit, index, request, now);
        }
        if request && protected_veto(w, controller, unit, Some(index)) {
            request = false;
        }
        prepare_spread(w, &cx.weapons, controller, unit, Some(index));
    }
    let fire = fire_mut(w, controller).unwrap();
    let channel = fire.channel;
    let a = index_of(channel).filter(|&i| i < 2).expect("consumeFire: the fire channel's button (0 or 1)");
    let b = index_of(1.0 - channel).filter(|&i| i < 2).expect("consumeFire: the other button (0 or 1)");
    press_button(&mut fire.buttons[a], request);
    press_button(&mut fire.buttons[b], false);
    let (b0, b1) = (fire.buttons[0].count, fire.buttons[1].count);
    let u = w.unit(unit);
    if u.intent && u.intent_buttons {
        w.set_intent_buttons(unit, b0, b1);
    }
    request
}

/// weapons-ai.js:702 `chargeWeapon(weapon)`.
pub fn charge_weapon(wv: &super::world::WeaponView) -> bool {
    let def = match &wv.levels {
        Some(l) if !l.is_empty() => l[0].clone(),
        _ => wv.def.clone(),
    };
    if is_launcher(Some(&def)) {
        return false;
    }
    if !(def.get("MaxPressedTime").num() > 0.0) {
        return false;
    }
    is_cannon(Some(&def)) || def.get("base").is_str("grenade")
}

/// weapons-ai.js:716 `chargeRequest`.
fn charge_request<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj, index: usize, mut request: bool, now: f64) -> bool {
    let wv = w.unit(unit).arsenal.as_ref().unwrap().weapons[index].clone();
    let kind = w.unit(unit).kind;
    if request {
        let gravity = weapon_gravity(&wv);
        let last_release = w.ctl(controller).perception.as_ref().unwrap().fire.last_release;
        if gravity == 0.0 || f(last_release + 4.0) <= f(now) {
            if !w.ctl(controller).perception.as_ref().unwrap().fire.charging {
                let most_raw = match wv.limit {
                    Some(l) => l,
                    None => {
                        let level0 = wv.levels.as_ref().and_then(|l| l.first().cloned());
                        let from_level = level0.map(|l| l.get("MaxPressedTime").clone()).unwrap_or_default();
                        if !from_level.is_nullish() {
                            from_level.num()
                        } else {
                            wv.def.get("MaxPressedTime").num()
                        }
                    }
                };
                let most = f(most_raw);
                let mut hold = most;
                if kind == 0.0 && !wv.def.get("base").is_str("repair") {
                    hold = draw_range(w, Gen::A, f(most * 0.25), most);
                }
                let fire = fire_mut(w, controller).unwrap();
                fire.charging = true;
                fire.hold = hold;
            }
        } else {
            request = false;
        }
    }
    let fire = fire_mut(w, controller).unwrap();
    if !fire.charging {
        return false;
    }
    if wv.charging && fire.hold <= f(wv.held) {
        fire.last_release = now;
        fire.charging = false;
        return false;
    }
    let _ = request;
    true
}

/// weapons-ai.js:737 `weaponGravity(weapon)`.
fn weapon_gravity(wv: &super::world::WeaponView) -> f64 {
    let o = wv.def.get("ordnance");
    if !o.truthy() {
        return 0.0;
    }
    if matches!(o.get("base").str(), Some("shell" | "grenade" | "sticky" | "haywire" | "beacon")) {
        -1.0
    } else {
        0.0
    }
}

/// weapons-ai.js:747 `semiautoRequest`.
fn semiauto_request<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, index: usize, mut request: bool, now: f64) -> bool {
    let wv = w.unit(unit).arsenal.as_ref().unwrap().weapons[index].clone();
    if sniper_weapon(table, Some(&wv.def)) {
        request = sniper_gate(w, controller, &wv, request, now);
    }
    if !request {
        return false;
    }
    let fire = &w.ctl(controller).perception.as_ref().unwrap().fire;
    let ch = index_of(fire.channel).filter(|&i| i < 2).expect("semiauto: the fire channel's button");
    let count = fire.buttons[ch].count;
    if count > 0.0 {
        return count == 1.0;
    }
    if count == -1.0 {
        if matches!(wv.def.get("base").str(), Some("dispenser" | "remote" | "detonator" | "shield")) {
            let c = w.ctl_mut(controller);
            c.fire_a = false;
            c.fire_b = false;
        }
        return false;
    }
    true
}

/// weapons-ai.js:764 `sniperWeapon(weapon)`: TriggerSingle with ZoomMax >= 6.
pub fn sniper_weapon(table: &WeaponAiTable, def: Option<&JsObj>) -> bool {
    match def {
        Some(d) => {
            let a = table.weapon_ai(d);
            a.trigger_single && a.zoom_max >= 6.0
        }
        None => false,
    }
}

/// weapons-ai.js:770 `sniperGate`.
fn sniper_gate<W: World + ?Sized>(w: &mut W, controller: Obj, wv: &super::world::WeaponView, request: bool, now: f64) -> bool {
    {
        let fire = fire_mut(w, controller).unwrap();
        if fire.sniper_active && f(fire.sniper_deadline) < f(now) {
            fire.sniper_active = false;
        }
    }
    let active = w.ctl(controller).perception.as_ref().unwrap().fire.sniper_active;
    if !active && request && weapon_idle(wv) {
        let d = draw_range(w, Gen::A, 2.0, 3.0);
        let fire = fire_mut(w, controller).unwrap();
        fire.sniper_active = true;
        fire.sniper_deadline = f(f(now) + d);
        return true;
    }
    false
}

/// weapons-ai.js:782 `weaponIdle(weapon)`.
fn weapon_idle(wv: &super::world::WeaponView) -> bool {
    !wv.reloading && !wv.charging && !wv.busy && !(wv.wait > 1e-6) && !wv.overheated
}

/// weapons-ai.js:790 `burstRequest`.
fn burst_request<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, index: usize, request: bool, now: f64) -> bool {
    let wv = w.unit(unit).arsenal.as_ref().unwrap().weapons[index].clone();
    if !request {
        fire_mut(w, controller).unwrap().burst.state = 0.0;
        return false;
    }
    let heat = f(wv.heat);
    let ammo = ammo_count(&wv);
    let a = table.weapon_ai(&wv.def);
    let single = a.trigger_single;
    {
        let b = &mut fire_mut(w, controller).unwrap().burst;
        if b.state == 0.0 {
            b.heat_limit = 0.0;
            b.last_end = -100.0;
            b.saved_ammo = 0.0;
            b.count = 0.0;
            b.state = 1.0;
        }
    }
    if fire_mut(w, controller).unwrap().burst.state == 1.0 {
        let count = if single { 1.0 } else { draw_int(w, Gen::A, a.barrage_min, a.barrage_max) };
        let b = &mut fire_mut(w, controller).unwrap().burst;
        b.count = count;
        b.state = 2.0;
    }
    let b = &mut fire_mut(w, controller).unwrap().burst;
    if b.state == 2.0 && f(f(a.barrage_delay) + f(b.last_end)) < f(now) {
        let shots = v8math::max(wv.def.get("ShotsPerSalvo").or(1.0), 1.0);
        let salvo_heat = f(f(shots) * f(wv.def.get("HeatPerShot").or(0.0)));
        let mut needed = f(salvo_heat * v8math::ceil(f(b.count / f(shots))));
        if needed >= 1.0 {
            needed = f(0.99);
        }
        if needed < f(1.0 - heat) {
            b.saved_ammo = ammo;
            b.state = 3.0;
            b.heat_limit = f(f(1.0 - (if salvo_heat >= 1.0 { f(0.99) } else { salvo_heat })) - f(0.0001));
        }
    }
    if b.state == 3.0 {
        if heat < b.heat_limit && b.saved_ammo - ammo < b.count {
            return true;
        }
        b.last_end = now;
        b.state = 1.0;
    }
    false
}

// ---------------------------------------------------------------------------
// Fire validation (weapons-ai.js:847).

/// weapons-ai.js:847 `validateFire(controller, unit, target, world)`.
pub fn validate_fire<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, target: Obj) -> bool {
    let has_perception = w.ctl(controller).perception.is_some();
    let weapon = selected_weapon(w, unit, Some(controller));
    let (Some(index), true) = (weapon, has_perception) else { return false };
    if resolve(w, w.ctl(controller).target) != Some(target) {
        return false;
    }
    let def = def_at(w, unit, index);
    let (lead, _) = lead_point(w, 0.0, unit, target, Some(&def));
    let point = w.aim_point(target);
    facing_and_range(w, table, controller, unit, &lead, &point)
}

/// weapons-ai.js:863 `facingAndRange(controller, unit, lead, point)`.
pub fn facing_and_range<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, lead: &Vector3, point: &Vector3) -> bool {
    if !firing_cone(w, table, controller, unit, lead) {
        return false;
    }
    let index = selected_weapon(w, unit, Some(controller)).expect("facingAndRange after firingCone: a weapon");
    let def = def_at(w, unit, index);
    let range = v8math::min(effective_range(table, w.unit(unit).kind, w.ctl(controller).range_scale, &def), harpoon_reach(&def));
    let origin = w.origin(unit);
    let d2 = length_sq32(f(point.x - origin.x), f(point.y - origin.y), f(point.z - origin.z));
    d2 < f(range * range)
}

/// weapons-ai.js:878 `harpoonReach(def)`.
pub fn harpoon_reach(def: &JsObj) -> f64 {
    if def.get("ordnance").get("base").is_str("towcable") {
        v8math::max(20.0, def.get("LockOnRange").or(0.0))
    } else {
        f64::INFINITY
    }
}

/// weapons-ai.js:889 `firingCone(controller, unit, point)`.
pub fn firing_cone<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, point: &Vector3) -> bool {
    let weapon = selected_weapon(w, unit, Some(controller));
    let aimer = w.ctl(controller).perception.as_ref().map(|p| p.aimer.direction);
    let (Some(index), Some(q)) = (weapon, aimer) else { return false };
    let normal = lookup_cos(angle::fire());
    let melee_cos = lookup_cos(angle::melee());
    let origin = w.origin(unit);
    let fwd = w.forward(unit);
    let rx = f(point.x - origin.x);
    let ry = f(point.y - origin.y);
    let rz = f(point.z - origin.z);
    let def = def_at(w, unit, index);
    if sniper_weapon(table, Some(&def)) && w.unit(unit).kind == 0.0 {
        return perp_sq(rx, ry, rz, &fwd) < 0.0625;
    }
    let n = normalize_or_x(rx, ry, rz);
    let melee = def.get("base").is_str("melee");
    let limit = if melee { melee_cos } else { normal };
    if dot32(fwd.x, fwd.y, fwd.z, n.x, n.y, n.z) <= limit {
        return false;
    }
    let n = normalize_or_x(q.x, q.y, q.z);
    if dot32(n.x, n.y, n.z, fwd.x, fwd.y, fwd.z) <= limit && !melee {
        return false;
    }
    if length_sq32(rx, ry, rz) < 2500.0 {
        return true;
    }
    perp_sq(rx, ry, rz, &fwd) < 400.0
}

/// weapons-ai.js:910 `perpSq`: |r x d|^2 in float32.
fn perp_sq(rx: f64, ry: f64, rz: f64, d: &Vector3) -> f64 {
    let cx = f(f(ry * f(d.z)) - f(rz * f(d.y)));
    let cy = f(f(rz * f(d.x)) - f(rx * f(d.z)));
    let cz = f(f(rx * f(d.y)) - f(ry * f(d.x)));
    length_sq32(cx, cy, cz)
}

// ---------------------------------------------------------------------------
// Friendly-human protection (weapons-ai.js:925).

/// weapons-ai.js:925 `rebuildProtectedHumans(world)`.
pub fn rebuild_protected_humans<W: World + ?Sized>(w: &mut W) {
    let mut humans = Vec::new();
    for u in w.living() {
        if w.unit(u).human {
            humans.push(u);
        }
    }
    if humans.len() >= 9 {
        return;
    }
    for u in w.living() {
        if let Some(c) = w.unit(u).controller {
            if let Some(p) = w.ctl_mut(c).perception.as_mut() {
                p.fire.protected.clear();
            }
        }
    }
    for human in humans {
        let (position, team, generation) = {
            let h = w.unit(human);
            (h.position, h.team, h.generation)
        };
        let near = w.query(&position, 50.0, QueryFilter::AllyOrNeutral { except: human, team });
        let n = near.len().min(20);
        for &u in near.iter().take(n) {
            if let Some(c) = w.unit(u).controller {
                if let Some(p) = w.ctl_mut(c).perception.as_mut() {
                    if p.fire.protected.len() < 8 {
                        p.fire.protected.push(Ref { obj: human, generation });
                    }
                }
            }
        }
    }
}

/// weapons-ai.js:948 `protectedVeto(controller, unit, weapon)`.
pub fn protected_veto<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj, weapon: Option<usize>) -> bool {
    let base = weapon.map(|i| def_at(w, unit, i).get("base").clone()).unwrap_or_default();
    if matches!(base.str(), Some("repair" | "dispenser" | "remote" | "detonator")) {
        return false;
    }
    let protected = match w.ctl(controller).perception.as_ref() {
        Some(p) if !p.fire.protected.is_empty() => p.fire.protected.clone(),
        _ => return false,
    };
    let origin = w.origin(unit);
    let fwd = w.forward(unit);
    for r in protected {
        let Some(human) = resolve(w, Some(r)) else { continue };
        let s = w.sphere(human);
        let rx = f(s.center.x - origin.x);
        let ry = f(s.center.y - origin.y);
        let rz = f(s.center.z - origin.z);
        let cx = f(f(f(fwd.y) * rz) - f(f(fwd.z) * ry));
        let cy = f(f(f(fwd.z) * rx) - f(f(fwd.x) * rz));
        let cz = f(f(f(fwd.x) * ry) - f(f(fwd.y) * rx));
        let radius = f(s.radius);
        if length_sq32(cx, cy, cz) < f(radius * radius) && dot32(fwd.x, fwd.y, fwd.z, rx, ry, rz) > 0.0 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Spread (weapons-ai.js:977).

/// weapons-ai.js:980 `sniperYaw(distanceSq)`.
pub fn sniper_yaw(distance_sq: f64) -> f64 {
    const STEPS: [(f64, f64); 6] = [(400.0, 0.0997), (1600.0, 0.0499), (3600.0, 0.0333), (6400.0, 0.025), (10000.0, 0.02), (14400.0, 0.0167)];
    for (limit, yaw) in STEPS {
        if distance_sq < f(limit) {
            return f(yaw);
        }
    }
    f(0.0143)
}

/// weapons-ai.js:985 `prepareSpread(controller, unit, weapon)`.
pub fn prepare_spread<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, weapon: Option<usize>) {
    let Some(index) = weapon else { return };
    weapon_mut(w, unit, index).yaw_spread_override = None;
    let def = def_at(w, unit, index);
    if w.unit(unit).human || def.get("YawSpread").truthy() || !sniper_weapon(table, Some(&def)) {
        return;
    }
    let Some(target) = resolve(w, w.ctl(controller).target) else { return };
    let p = w.unit(unit).position;
    let q = w.unit(target).position;
    let d2 = length_sq32(f(p.x - q.x), f(p.y - q.y), f(p.z - q.z));
    let s = weapon_mut(w, unit, index).sniper_scale.unwrap_or(0.0);
    weapon_mut(w, unit, index).yaw_spread_override = Some(f(f(s) * sniper_yaw(d2)) * (180.0 / std::f64::consts::PI));
}

/// A unit's weapon at `index`, to write its AI settings.
pub fn weapon_mut<W: World + ?Sized>(w: &mut W, unit: Obj, index: usize) -> &mut super::world::WeaponView {
    &mut w.unit_mut(unit).arsenal.as_mut().expect("an arsenal").weapons[index]
}
