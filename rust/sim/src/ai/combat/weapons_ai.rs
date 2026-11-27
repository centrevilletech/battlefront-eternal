//! How a Classic bot uses its weapons: what
//! it knows about each weapon, which weapon it picks for a target, and how
//! its controller's two fire requests become trigger presses.

use std::collections::HashMap;
use crate::ai::owned::scratch::Inline;
use std::{rc::Rc,cell::RefCell};
use three::Vector3;

use super::aim::{is_cannon, is_launcher, lead_point, resolve};
use super::math::{angle, dot32, f, length_sq32, lookup_cos, normalize_or_x, FLT_MAX};
use super::world::{draw_int, draw_range, index_of, Gen, Obj, QueryFilter, Ref, UnitView, World};
use crate::combat::defs::{Js, JsObj};

const INT_MAX: f64 = 2147483647.0;

/// `ENGINE_DEFAULTS`.
pub const DEFAULT_MIN_RANGE: f64 = 0.0;
pub const DEFAULT_OPTIMAL_RANGE: f64 = 30.0;
pub const DEFAULT_MAX_RANGE: f64 = 60.0;
pub const DEFAULT_RELATION_MASK: u32 = 1;
pub const DEFAULT_CATEGORY_MASK: u32 = 0xffff_ff9f;
pub const DEFAULT_BARRAGE_MIN: f64 = 3.0;
pub const DEFAULT_BARRAGE_MAX: f64 = 6.0;
pub const DEFAULT_BARRAGE_DELAY: f64 = 2.0;

/// A weapon class record's AI view.
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
    pub ai_category_mask: u32,
    pub barrage_min: f64,
    pub barrage_max: f64,
    pub barrage_delay: f64,
    /// `triggerSingle` (truthy).
    pub trigger_single: bool,
    pub zoom_max: f64,
    pub action: f64,
    pub swbf2_action: f64,
    /// Immutable definition flags used by consumeFire and prepareSpread.
    pub cannon: bool,
    pub authored_yaw: bool,
}

/// A side's ai.json weapon record as registerAIData keeps it (its `ai`,
/// and its charge levels' `at` and `ai`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registered {
    pub ai: Js,
    pub levels: Vec<(Js, Js)>,
}

const TRAIT_TRIGGER_SINGLE:u8=1;
const TRAIT_CANNON:u8=2;
const TRAIT_AUTHORED_YAW:u8=4;
const TRAIT_SNIPER:u8=8;
fn weapon_traits(ai:&WeaponAi)->u8 {
    (if ai.trigger_single {TRAIT_TRIGGER_SINGLE}else{0})|
    (if ai.cannon {TRAIT_CANNON}else{0})|
    (if ai.authored_yaw {TRAIT_AUTHORED_YAW}else{0})|
    (if ai.trigger_single&&ai.zoom_max>=6. {TRAIT_SNIPER}else{0})
}
#[derive(Clone,Debug)]
struct Prepared {
    // Retaining the definition prevents reuse of its numeric address key.
    _definition:Rc<JsObj>,ai:WeaponAi,traits:u8,
}

/// the weapons' AI records by class name. The
/// JavaScript's map is the process's and is never cleared (quirks
/// P16-1); a battle holds its own here.
#[derive(Clone, Debug, Default)]
pub struct WeaponAiTable {
    pub registered: HashMap<String, Registered>,
    prepared:HashMap<usize,Prepared>,
    sealed:bool,
}

impl WeaponAiTable {
    /// `registerAIData(aiJson)`: every weapon record
    /// with an `ai` (the later side's wins a shared name).
    pub fn register(&mut self, weapons: &[(String, Registered)]) -> usize {
        assert!(!self.sealed,"AI weapon records are loaded before native play");
        self.prepared.clear();
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

    /// `registeredAI(def)`.
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

    /// `weaponAI(def)`: its own `ai`, else the registered
    /// record of its class, else the engine defaults and its flags.
    /// Resolve all definition names once, retaining the objects so pointer
    /// identity cannot be recycled while a prepared numeric record is live.
    pub fn prepare_class(&mut self,class:&crate::combat::defs::WeaponClass) {
        self.prepared.reserve(class.levels.len());
        for level in &class.levels {
            let record=self.read_weapon_ai(&level.obj);
            self.prepared.insert(Rc::as_ptr(&level.obj) as usize,Prepared {_definition:level.obj.clone(),ai:record,traits:weapon_traits(&record)});
        }
    }
    pub fn seal(&mut self) {self.sealed=true;}
    /// A subsequent battle loads new sides before services share the table.
    /// registerAIData clears prepared records at its usual observation point.
    pub(crate) fn resume(&mut self) {self.sealed=false;}
    pub fn weapon_ai(&self,def:&JsObj)->WeaponAi {
        if let Some(record)=self.prepared.get(&(def as *const JsObj as usize)) {return record.ai;}
        assert!(!self.sealed,"native AI weapon definition was not prepared");
        self.read_weapon_ai(def)
    }
    // consumeFire and prepareSpread need only immutable flags. Returning a
    // byte avoids copying every range, mask and barrage number for that read.
    fn traits(&self,def:&JsObj)->u8 {
        if let Some(record)=self.prepared.get(&(def as *const JsObj as usize)) {return record.traits;}
        assert!(!self.sealed,"native AI weapon definition was not prepared");
        weapon_traits(&self.read_weapon_ai(def))
    }
    fn read_weapon_ai(&self, def: &JsObj) -> WeaponAi {

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
        let ai_category = flags.map(|f| f.2).unwrap_or(DEFAULT_CATEGORY_MASK);
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
        let swbf2_action = if matches!(def.get("base").str(), Some("melee" | "meleethrow")) {
            action::MELEE
        } else {
            action
        };
        WeaponAi {
            min_range: f(pick("minRange", "MinRange", DEFAULT_MIN_RANGE)),
            optimal_range: f(pick("optimalRange", "OptimalRange", DEFAULT_OPTIMAL_RANGE)),
            max_range: f(pick("maxRange", "MaxRange", DEFAULT_MAX_RANGE)),
            implicit_range: implicit,
            extreme_range: f(extreme),
            relation_mask: relation,
            category_mask: category,
            ai_category_mask: ai_category,
            barrage_min: pick("barrageMin", "BarrageMin", DEFAULT_BARRAGE_MIN),
            barrage_max: pick("barrageMax", "BarrageMax", DEFAULT_BARRAGE_MAX),
            barrage_delay: f(pick("barrageDelay", "BarrageDelay", DEFAULT_BARRAGE_DELAY)),
            trigger_single,
            zoom_max: f(zoom_max),
            action,
            swbf2_action,
            cannon: is_cannon(Some(def)),
            authored_yaw: def.get("YawSpread").truthy(),
        }
    }
}

/// `num(value)`: a flattened key's number
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

/// `maskFromFlags(def)`: TargetAll first, then each
/// flag its bit; the 2005 AI category mask starts from the same default
/// independently and applies only `AITarget*` overrides.
fn mask_from_flags(def: &JsObj) -> (u32, u32, u32) {
    let mut relation = DEFAULT_RELATION_MASK;
    let mut category = DEFAULT_CATEGORY_MASK;
    let mut ai_category = DEFAULT_CATEGORY_MASK;
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
    let ai_all = def.get("AITargetAll");
    if !ai_all.is_undefined() {
        ai_category = if truthy_number(ai_all) { 0xffff_ffff } else { 0 };
    }
    for (key, bit) in [
        ("AITargetPerson", 1u32),
        ("AITargetAnimal", 2),
        ("AITargetDroid", 4),
        ("AITargetVehicle", 8),
        ("AITargetBuilding", 16),
        ("AITargetBuildingDead", 32),
        ("AITargetBuildingUnbuilt", 64),
    ] {
        let v = def.get(key);
        if !v.is_undefined() {
            ai_category = if truthy_number(v) { ai_category | bit } else { ai_category & !bit };
        }
    }
    (relation, category, ai_category)
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

/// `actionOf(bank)`: the AnimationBank's action class.
pub fn action_of(bank: &Js) -> f64 {
    const BANKS: [&str; 10] = ["rifle", "bazooka", "pistol", "grenade", "tool", "lightsabre", "sbdroid", "bdroid", "cannon", "gam"];
    const ACTION: [f64; 10] = [1.0, 3.0, 0.0, 2.0, 6.0, 4.0, 1.0, 1.0, 3.0, 4.0];
    let text = if bank.is_nullish() { String::new() } else { js_string(bank) };
    let lower = text.to_lowercase();
    let i = BANKS.iter().position(|b| *b == lower).unwrap_or(0);
    ACTION[i]
}

/// `ACTION`.
pub mod action {
    pub const PISTOL: f64 = 0.0;
    pub const RIFLE: f64 = 1.0;
    pub const GRENADE: f64 = 2.0;
    pub const LAUNCHER: f64 = 3.0;
    pub const MELEE: f64 = 4.0;
    pub const TOOL: f64 = 6.0;
}

// ---------------------------------------------------------------------------
// Entity facts.

/// `infantryKind`.
fn infantry_kind(kind: f64) -> bool {
    kind == 0.0 || kind == 1.0 || kind == 3.0
}

/// `forwardSpeed(unit)`: the class's ForwardSpeed, else
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

/// `categoryOf(unit)`: person 0, animal 1, droid 2,
/// vehicle 3, building 4 (NaN for a HealthType naming an inherited
/// property of a plain object).
pub fn category_of(u: &UnitView) -> f64 {
    if let Some(c) = u.category {
        if c.is_finite() && c.fract() == 0.0 {
            return c;
        }
    }
    if let Some(category)=u.class.as_ref().and_then(|c|c.prepared_category) {return category;}
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

/// `relationBit(relation)`.
pub fn relation_bit(relation: f64) -> u32 {
    shl1(relation + 1.0)
}

// ---------------------------------------------------------------------------
// Ranges.

/// `effectiveRange(unit, controller, def)`.
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

/// `expandedRange(unit, controller, def)`.
pub fn expanded_range(table: &WeaponAiTable, u: &UnitView, range_scale: Option<f64>, def: &JsObj) -> f64 {
    expanded_motion_range(table,u.kind,forward_speed(u),range_scale,def)
}
fn expanded_motion_range(table:&WeaponAiTable,kind:f64,speed:f64,range_scale:Option<f64>,def:&JsObj)->f64 {
    f(effective_range(table,kind,range_scale,def)+f(speed*5.0))
}

/// `maxExpandedRange(unit, controller)`.
pub fn max_expanded_range<W:World+?Sized>(w:&W,table:&WeaponAiTable,unit:Obj,range_scale:Option<f64>)->f64 {
    let mut best=0.;let (kind,speed)=w.unit_weapon_motion(unit);
    for i in 0..w.weapon_count(unit) {
        w.visit_weapon_levels(unit,i,&mut |level| {let r=expanded_motion_range(table,kind,speed,range_scale,level);if best<=r {best=r;}});
    }
    best
}

/// `selectedWeapon(unit, controller)`: the current
/// weapon of the AI's channel (the arsenal index; no fallback).
pub fn selected_weapon<W: World + ?Sized>(w: &W, unit: Obj, controller: Option<Obj>) -> Option<usize> {
    if !w.has_arsenal(unit) {return None;}
    let channel = controller.and_then(|c| w.ctl(c).perception.as_ref().map(|p| p.fire.channel)).unwrap_or(0.0);
    w.current_weapon(unit,channel)
}

/// The unit's weapon `index`'s current def.
pub fn def_at<W: World + ?Sized>(w: &W, unit: Obj, index: usize) -> Rc<JsObj> {
    w.weapon_definition(unit,index).expect("arsenal member")
}

/// `sensorRange(unit, controller)`.
pub fn sensor_range<W: World + ?Sized>(w: &W, table: &WeaponAiTable, unit: Obj, controller: Obj) -> f64 {
    match selected_weapon(w, unit, Some(controller)) {
        Some(i) => {
            let def = def_at(w, unit, i);
            let rs = w.ctl(controller).range_scale;
            let (kind,speed)=w.unit_weapon_motion(unit);
            expanded_motion_range(table,kind,speed,rs,&def)
        }
        None => 0.0,
    }
}

/// `lowerBand(unit, controller)`.
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

/// `upperBand(unit, controller, sight)`.
pub fn upper_band<W: World + ?Sized>(w: &W, table: &WeaponAiTable, unit: Obj, controller: Obj, sight: f64) -> f64 {
    let Some(i) = selected_weapon(w, unit, Some(controller)) else { return 0.0 };
    let def = def_at(w, unit, i);
    let a = table.weapon_ai(&def);
    let e = f(effective_range(table, w.unit_scalars(unit).kind, w.ctl(controller).range_scale, &def) - 10.0);
    let hi = if e <= a.optimal_range { a.optimal_range } else { e };
    let s = f(sight - 10.0);
    if s <= hi {
        s
    } else {
        hi
    }
}

// ---------------------------------------------------------------------------
// Ammunition.

/// `ammoCount(weapon)`.
pub fn ammo_count(wv: &super::world::WeaponView) -> f64 {
    if wv.bottomless || wv.reserve == f64::INFINITY {
        return INT_MAX;
    }
    v8math::trunc(f(f(wv.clip + wv.reserve) * wv.rounds.num()))
}

/// `capacity(weapon)`.
pub fn capacity(wv: &super::world::WeaponView) -> f64 {
    let ammo = wv.def.get("ammo");
    let clips = if ammo.num() > 0.0 { ammo.num() } else { FLT_MAX };
    let rounds = if wv.bottomless { INT_MAX } else { wv.rounds.nn(1.0) };
    f(clips * rounds)
}

// ---------------------------------------------------------------------------
// Utility.

const SCALE_KEYS: [&str; 5] = ["PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale"];

/// `typeScale(rec, category)`.
fn type_scale(rec: &Js, category: f64) -> f64 {
    let key = index_of(category).and_then(|i| SCALE_KEYS.get(i).copied());
    match key {
        Some(k) => f(rec.get(k).nn(1.0)),
        None => f(1.0),
    }
}

/// `blastFalloff`.
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

/// `blastDamage`.
fn blast_damage(explosion: &Js, category: f64, radius: f64) -> f64 {
    if !explosion.truthy() || !matches!(explosion, Js::Obj(_) | Js::Arr(_)) {
        return 0.0;
    }
    let fall = blast_falloff(2.0, f(explosion.get("DamageRadiusInner").or(0.0)), f(explosion.get("DamageRadiusOuter").or(0.0)), radius);
    let value = f(fall * f(explosion.get("Damage").or(0.0)));
    f(type_scale(explosion, category) * value)
}

/// `maxHealthOf(unit)`.
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

/// `weaponUtility(owner, weapon, target)`: (ok, score).
pub fn weapon_utility<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, owner: Obj, weapon: usize, target: Obj) -> (bool, f64) {
    let def = def_at(w, owner, weapon);
    let ordnance = def.get("ordnance").clone();
    let owner_kind = w.unit_scalars(owner).kind;
    let owner_soldier = owner_kind == 0.0;
    let owner_hero = w.unit_scalars(owner).hero;
    let t = w.unit(target);
    let target_kind = t.kind;
    let accept = |score: f64| (true, f(score));
    let base = def.get("base");
    if !ordnance.truthy() && base.is_str("melee") && (target_kind == 0.0 || target_kind == 3.0) {
        return accept(1000.0);
    }
    if !w.uses_swbf2() && owner_soldier && owner_hero && is_cannon(Some(&def)) {
        return accept(1000.0);
    }
    if t.trap {
        return (false, 0.0);
    }
    let ai = table.weapon_ai(&def);
    let mask = if w.uses_swbf2() { ai.ai_category_mask } else { ai.category_mask };
    if owner_soldier && target_kind == 0.0 && mask & shl1(category_of(&t)) == 0 {
        return (false, 0.0);
    }
    if !ordnance.truthy() {
        return (false, 0.0);
    }
    let category = category_of(&t);
    let radius = w.sphere(target).radius;
    let mut damage = f(f(ordnance.get("Damage").or(0.0)) * type_scale(&ordnance, category));
    damage = f(blast_damage(ordnance.get("ExplosionImpact"), category, radius) + damage);
    damage = f(blast_damage(ordnance.get("ExplosionExpire"), category, radius) + damage);
    let wv = w.weapon(owner,weapon).expect("arsenal member");
    let cap = capacity(&wv);
    let value = f((if cap <= 50.0 { cap } else { 50.0 }) * damage);
    let t = w.unit(target);
    if f(max_health_of(&t) * 0.5) < value {
        return accept(damage);
    }
    let ok = owner_kind;
    if (ok == 6.0 || ok == 2.0 || ok == 8.0 || ok == 7.0) && damage > 0.0 {
        return accept(damage);
    }
    if target_kind != 6.0 && target_kind != 2.0 && (!(max_health_of(&t) < FLT_MAX) || !(target_kind == 9.0 || target_kind == 8.0)) {
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
// Weapon selection.

/// selectWeapon's answer.
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

/// The per-target-controller cluster timestamps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterState {
    pub scan_at: f64,
    pub flagged_at: f64,
    pub generation: f64,
}

/// `clusters`: keyed by the target's controller, else
/// the target.
#[derive(Clone, Debug, Default)]
pub struct Clusters {
    pub states: HashMap<Obj, ClusterState>,
}

impl Clusters {
    /// `clusterState(target, create)`: the key's state
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

/// `grenadePriority`.
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

/// Whether `unit` carries a loaded weapon that can engage `target` at any distance.
pub fn has_weapon_for_target<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, unit: Obj, target: Obj) -> bool {
    let count = w.weapon_count(unit);
    for i in 0..count {
        let Some(wv) = w.weapon(unit, i) else { continue };
        if !(ammo_count(&wv) > 0.0) {
            continue;
        }
        if weapon_utility(w, table, unit, i, target).0 {
            return true;
        }
    }
    false
}

/// `selectWeapon(controller, unit, target, switchAllowed,
/// world)`.
pub fn select_weapon<W: World + ?Sized>(w: &mut W, cx: &mut Shared, controller: Option<Obj>, unit: Obj, target: Option<Obj>, switch_allowed: bool) -> Choice {
    let mut out = Choice::default();
    let count = w.weapon_count(unit);
    let Some(target) = target else { return out };
    if count == 0 {
        return out;
    }
    let now = w.time();
    let origin = w.origin(unit);
    let aim = w.aim_point(target);
    let d2 = length_sq32(f(aim.x - origin.x), f(aim.y - origin.y), f(aim.z - origin.z));
    let owner_soldier = w.unit_scalars(unit).kind == 0.0;
    let range_scale = controller.map(|c| w.ctl(c).range_scale).unwrap_or(None);
    let roll = controller.map(|c| w.ctl(c).roll);
    let swbf2 = w.uses_swbf2();
    let mut best: Option<usize> = None;
    let mut best_score = -1.0;
    let mut best_big = false;
    let mut best_in_band = false;
    for i in 0..count {
        if !(ammo_count(&w.weapon(unit,i).expect("arsenal member")) > 0.0) {
            continue;
        }
        let melee_throw = swbf2 && def_at(w, unit, i).get("base").is_str("meleethrow");
        if switch_allowed && melee_throw && w.melee_throw_reloading(unit) {
            continue;
        }
        let (ok, score) = weapon_utility(w, &cx.weapons, unit, i, target);
        if !ok {
            continue;
        }
        let def = def_at(w, unit, i);
        let cap = capacity(&w.weapon(unit,i).expect("arsenal member"));
        let big = cap > 20.0;
        let better = best_score < score;
        let a = cx.weapons.weapon_ai(&def);
        let range = effective_range(&cx.weapons, w.unit_scalars(unit).kind, range_scale, &def);
        if d2 <= f(a.min_range * a.min_range) || f(range * range) <= d2 {
            // Out of the band: a fallback while nothing in band is found.
            if best_in_band || melee_throw {
                continue;
            }
            let (kind,speed)=w.unit_weapon_motion(unit);
            let far=expanded_motion_range(&cx.weapons,kind,speed,range_scale,&def);
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
        if grenade_priority(&mut cx.clusters.borrow_mut(), roll, &t, &def, owner_soldier, now) {
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
    let best_ai = cx.weapons.weapon_ai(&def_at(w, unit, best));
    out.action = if swbf2 { best_ai.swbf2_action } else { best_ai.action };
    out.in_band = best_in_band;
    if switch_allowed && selected_weapon(w, unit, controller) != Some(best) {
        if let Some(c) = controller {
            switch_weapon(w, c, unit, best);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The controller's weapon state.

/// A run-length button.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Button {
    pub count: f64,
    pub code: f64,
}

/// The automatic cannon's burst.
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

/// `FireControl`.
#[derive(Clone, Debug, PartialEq)]
pub struct FireControl {
    pub channel: f64,
    pub buttons: [Button; 2],
    pub last_release: f64,
    pub charging: bool,
    pub hold: f64,
    pub sniper_active: bool,
    pub sniper_deadline: f64,
    pub single_waiting: bool,
    pub single_deadline: f64,
    pub melee_fire_raw: bool,
    pub burst: Burst,
    pub weapon_target: Option<Ref>,
    pub protected: Inline<Ref,8>,
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
            single_waiting: true,
            single_deadline: -1.0,
            melee_fire_raw: false,
            burst: Burst::default(),
            weapon_target: None,
            protected: Inline::default(),
        }
    }
}

impl FireControl {
    /// `reset()` (the burst keeps all but its state).
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
        self.single_waiting = true;
        self.single_deadline = -1.0;
        self.melee_fire_raw = false;
        self.burst.state = 0.0;
        self.weapon_target = None;
        self.protected.clear();
    }
}

/// What the combat services share across controllers: the weapons' AI
/// records, the cluster timestamps.
#[derive(Debug,Default)]
pub struct Shared {
    pub weapons:Rc<WeaponAiTable>,
    pub clusters:Rc<RefCell<Clusters>>,
}
impl Clone for Shared {
    fn clone(&self)->Self {Self {weapons:self.weapons.clone(),clusters:Rc::new(RefCell::new(self.clusters.borrow().clone()))}}
}
impl Shared {
    /// Native vehicle dispatch and combat use the same cluster authority. A
    /// scope borrows it only for the pure edit, never across a world query.
    pub fn share(&self)->Self {Self {weapons:self.weapons.clone(),clusters:self.clusters.clone()}}
    pub fn register(&mut self,records:&[(String,Registered)])->usize {
        Rc::get_mut(&mut self.weapons).expect("AI records load before native services bind").register(records)
    }
    pub fn prepare_class(&mut self,class:&crate::combat::defs::WeaponClass) {
        Rc::get_mut(&mut self.weapons).expect("AI classes load before native services bind").prepare_class(class);
    }
    pub fn begin(&mut self,targets:usize) {
        Rc::get_mut(&mut self.weapons).expect("AI load completes before native services bind").seal();
        self.clusters.borrow_mut().states.reserve(targets);
    }
}

fn fire_mut<W: World + ?Sized>(w: &mut W, controller: Obj) -> Option<&mut FireControl> {
    w.ctl_mut(controller).perception.as_mut().map(|p| &mut p.fire)
}

/// `switchWeapon(controller, unit, index)`.
pub fn switch_weapon<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj, index: usize) -> bool {
    let has_fire = w.ctl(controller).perception.is_some();
    let channel = match w.weapon(unit,index) {
        Some(wv) => wv.channel.clone(),
        None => return false,
    };
    if !has_fire {
        return false;
    }
    let fire = fire_mut(w, controller).unwrap();
    fire.sniper_active = false;
    fire.single_waiting = true;
    fire.single_deadline = -1.0;
    fire.melee_fire_raw = false;
    fire.last_release = -1.0;
    fire.charging = false;
    fire.burst.state = 0.0;
    fire.channel = channel.num();
    w.arsenal_select(unit, index)
}

/// `setTarget(controller, unit, target, world)`.
pub fn set_target<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Option<Obj>, target: Option<Obj>) {
    let r = target.map(|t| Ref { obj: t, generation: w.unit_scalars(t).generation });
    w.ctl_mut(controller).target = r;
    if w.ctl(controller).perception.is_none() {
        return;
    }
    let Some(unit) = unit else { return };
    let Some(index) = selected_weapon(w, unit, Some(controller)) else { return };
    let mut kept = target;
    if let Some(t) = target {
        let a = table.weapon_ai(&def_at(w, unit, index));
        let (ut, tt) = (w.unit_scalars(unit).team, w.unit_scalars(t).team);
        let rel = w.relation(ut, tt);
        let mask = if w.uses_swbf2() { a.ai_category_mask } else { a.category_mask };
        if a.relation_mask & relation_bit(rel) == 0 || mask & shl1(category_of(&w.unit(t))) == 0 {
            kept = None;
        }
    }
    let kr = kept.map(|k| Ref { obj: k, generation: w.unit_scalars(k).generation });
    fire_mut(w, controller).unwrap().weapon_target = kr;
}

/// `pressButton(button, down)`.
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

/// `triggerFromCounts(previous, current)`: (held, pressed).
pub fn trigger_from_counts(previous: f64, current: f64) -> (bool, bool) {
    (current > 0.0, current > 0.0 && !(previous > 0.0))
}

// ---------------------------------------------------------------------------
// The fire-request consumer.

/// `consumeFire(controller, unit, world)`: writes the
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
    let mut melee_fire_raw = false;
    if let Some(index) = weapon {
        if !fire_a && fire_b {
            if let Some(t) = resolve(w, target) {
                request = validate_fire(w, &cx.weapons, controller, unit, t);
            }
        }
        let def = def_at(w, unit, index);
        melee_fire_raw = request && def.get("base").is_str("melee");
        if w.weapon_chargeable(unit,index) {
            request = charge_request(w, controller, unit, index, request, now);
        } else {
            let traits=cx.weapons.traits(&def);
            if traits&TRAIT_TRIGGER_SINGLE!=0 {
                request = semiauto_request(w, &cx.weapons, controller, unit, index, request, now);
            } else if traits&TRAIT_CANNON!=0 {
                request = burst_request(w, &cx.weapons, controller, unit, index, request, now);
            }
        }
        if request && protected_veto(w, controller, unit, Some(index)) {
            request = false;
        }
        prepare_spread(w, &cx.weapons, controller, unit, Some(index));
    }
    let fire = fire_mut(w, controller).unwrap();
    fire.melee_fire_raw = melee_fire_raw;
    let channel = fire.channel;
    let a = index_of(channel).filter(|&i| i < 2).expect("consumeFire: the fire channel's button (0 or 1)");
    let b = index_of(1.0 - channel).filter(|&i| i < 2).expect("consumeFire: the other button (0 or 1)");
    press_button(&mut fire.buttons[a], request);
    press_button(&mut fire.buttons[b], false);
    let (b0, b1) = (fire.buttons[0].count, fire.buttons[1].count);
    if w.unit_has_buttons(unit) {
        w.set_intent_buttons(unit, b0, b1);
    }
    request
}

/// `chargeWeapon(weapon)`.
pub fn charge_weapon(wv: &super::world::WeaponView) -> bool {
    let def = match &wv.levels {
        Some(l) if !l.is_empty() => l[0].clone(),
        _ => wv.def.clone(),
    };
    charge_definition(&def)
}

/// chargeWeapon uses immutable level-zero facts, prepared at class load.
pub fn charge_definition(def:&JsObj)->bool {
    if is_launcher(Some(def)) {
        return false;
    }
    if !(def.get("MaxPressedTime").num() > 0.0) {
        return false;
    }
    is_cannon(Some(def)) || def.get("base").is_str("grenade")
}

/// `chargeRequest`.
fn charge_request<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj, index: usize, mut request: bool, now: f64) -> bool {
    let wv = w.weapon(unit,index).expect("arsenal member");
    let kind = w.unit_scalars(unit).kind;
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

/// `weaponGravity(weapon)`.
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

fn swbf2_skill_t(skill: f64) -> f64 {
    let clamped = v8math::min(v8math::max(skill, 1.0), 20.0);
    f(f(clamped - 1.0) / 19.0)
}

fn swbf2_skill_scale(skill: f64) -> f64 {
    f(f(0.1) + f(f(0.8) * swbf2_skill_t(skill)))
}

fn step_swbf2_single_trigger(
    waiting: &mut bool,
    deadline: &mut f64,
    request: bool,
    now: f64,
    skill: f64,
    draw: impl FnOnce(f64, f64) -> f64,
) -> bool {
    if !request {
        *waiting = true;
        *deadline = -1.0;
        return false;
    }
    if !*waiting {
        let inv = v8math::max(0.0, f(1.0 - swbf2_skill_scale(skill)));
        let wait = draw(f(f(0.2) * inv), f(f(2.0) * inv));
        *waiting = true;
        *deadline = f(f(now) + wait);
        return true;
    }
    if f(*deadline) < f(now) {
        *waiting = false;
    }
    false
}

fn swbf2_burst_count(barrage_min: f64, barrage_max: f64, skill: f64, draw: impl FnOnce(f64, f64) -> f64) -> f64 {
    let t = swbf2_skill_t(skill);
    let span = v8math::max(0.0, f(barrage_max - barrage_min));
    let extra = if span > 0.0 { f(draw(0.0, span) * t) } else { 0.0 };
    v8math::max(2.0, v8math::trunc(f(f(barrage_min) + extra)))
}

fn swbf2_burst_delay(barrage_delay: f64, skill: f64) -> f64 {
    f(f(barrage_delay) * f(f(1.6) - swbf2_skill_scale(skill)))
}

/// `semiautoRequest`.
fn semiauto_request<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, index: usize, mut request: bool, now: f64) -> bool {
    let wv = w.weapon(unit,index).expect("arsenal member");
    let sniper = sniper_weapon(table, Some(&wv.def));
    if sniper {
        request = sniper_gate(w, controller, &wv, request, now);
    } else if w.uses_swbf2() {
        let skill = w.team_skill(w.unit_scalars(unit).team);
        let (mut waiting, mut deadline) = {
            let fire = &w.ctl(controller).perception.as_ref().unwrap().fire;
            (fire.single_waiting, fire.single_deadline)
        };
        let emitted = step_swbf2_single_trigger(&mut waiting, &mut deadline, request, now, skill, |lo, hi| {
            if lo == hi { lo } else { draw_range(w, Gen::A, lo, hi) }
        });
        let dst = fire_mut(w, controller).unwrap();
        dst.single_waiting = waiting;
        dst.single_deadline = deadline;
        if emitted && matches!(wv.def.get("base").str(), Some("dispenser" | "remote" | "detonator" | "shield")) {
            let c = w.ctl_mut(controller);
            c.fire_a = false;
            c.fire_b = false;
        }
        return emitted;
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

/// `sniperWeapon(weapon)`: TriggerSingle with ZoomMax >= 6.
pub fn sniper_weapon(table: &WeaponAiTable, def: Option<&JsObj>) -> bool {
    match def {
        Some(d) => {
            table.traits(d)&TRAIT_SNIPER!=0
        }
        None => false,
    }
}

/// `sniperGate`.
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

/// `weaponIdle(weapon)`.
fn weapon_idle(wv: &super::world::WeaponView) -> bool {
    !wv.reloading && !wv.charging && !wv.busy && !(wv.wait > 1e-6) && !wv.overheated
}

/// `burstRequest`.
fn burst_request<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, index: usize, request: bool, now: f64) -> bool {
    let wv=w.weapon_for_burst(unit,index,request);
    if !request {
        fire_mut(w, controller).unwrap().burst.state = 0.0;
        return false;
    }
    let wv=wv.expect("arsenal member");
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
    let swbf2 = w.uses_swbf2();
    let skill = if swbf2 { w.team_skill(w.unit_scalars(unit).team) } else { 0.0 };
    if fire_mut(w, controller).unwrap().burst.state == 1.0 {
        let count = if single {
            1.0
        } else if swbf2 {
            swbf2_burst_count(a.barrage_min, a.barrage_max, skill, |lo, hi| draw_range(w, Gen::A, lo, hi))
        } else {
            draw_int(w, Gen::A, a.barrage_min, a.barrage_max)
        };
        let b = &mut fire_mut(w, controller).unwrap().burst;
        b.count = count;
        b.state = 2.0;
    }
    let delay = if swbf2 { swbf2_burst_delay(a.barrage_delay, skill) } else { f(a.barrage_delay) };
    let b = &mut fire_mut(w, controller).unwrap().burst;
    if b.state == 2.0 && f(delay + f(b.last_end)) < f(now) {
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
// Fire validation.

/// `validateFire(controller, unit, target, world)`.
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

/// `facingAndRange(controller, unit, lead, point)`.
pub fn facing_and_range<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, lead: &Vector3, point: &Vector3) -> bool {
    if !firing_cone(w, table, controller, unit, lead) {
        return false;
    }
    let index = selected_weapon(w, unit, Some(controller)).expect("facingAndRange after firingCone: a weapon");
    let def = def_at(w, unit, index);
    let range = v8math::min(effective_range(table, w.unit_scalars(unit).kind, w.ctl(controller).range_scale, &def), harpoon_reach(&def));
    let origin = w.origin(unit);
    let d2 = length_sq32(f(point.x - origin.x), f(point.y - origin.y), f(point.z - origin.z));
    d2 < f(range * range)
}

/// `harpoonReach(def)`.
pub fn harpoon_reach(def: &JsObj) -> f64 {
    if def.get("ordnance").get("base").is_str("towcable") {
        v8math::max(20.0, def.get("LockOnRange").or(0.0))
    } else {
        f64::INFINITY
    }
}

/// `firingCone(controller, unit, point)`.
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
    if sniper_weapon(table, Some(&def)) && w.unit_scalars(unit).kind == 0.0 {
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

/// `perpSq`: |r x d|^2 in float32.
fn perp_sq(rx: f64, ry: f64, rz: f64, d: &Vector3) -> f64 {
    let cx = f(f(ry * f(d.z)) - f(rz * f(d.y)));
    let cy = f(f(rz * f(d.x)) - f(rx * f(d.z)));
    let cz = f(f(rx * f(d.y)) - f(ry * f(d.x)));
    length_sq32(cx, cy, cz)
}

// ---------------------------------------------------------------------------
// Friendly-human protection.

/// `rebuildProtectedHumans(world)`.
pub fn rebuild_protected_humans<W: World + ?Sized>(w: &mut W) {
    let mut humans = w.object_buffer();
    for u in w.living() {
        if w.unit_scalars(u).human {
            humans.push(u);
        }
    }
    if humans.len() >= 9 {
        return;
    }
    for u in w.living() {
        if let Some(c) = w.unit_controller(u) {
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
            if let Some(c) = w.unit_controller(u) {
                if let Some(p) = w.ctl_mut(c).perception.as_mut() {
                    if p.fire.protected.len() < 8 {
                        p.fire.protected.push(Ref { obj: human, generation });
                    }
                }
            }
        }
    }
}

/// `protectedVeto(controller, unit, weapon)`.
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
// Spread.

/// `sniperYaw(distanceSq)`.
pub fn sniper_yaw(distance_sq: f64) -> f64 {
    const STEPS: [(f64, f64); 6] = [(400.0, 0.0997), (1600.0, 0.0499), (3600.0, 0.0333), (6400.0, 0.025), (10000.0, 0.02), (14400.0, 0.0167)];
    for (limit, yaw) in STEPS {
        if distance_sq < f(limit) {
            return f(yaw);
        }
    }
    f(0.0143)
}

/// `prepareSpread(controller, unit, weapon)`.
pub fn prepare_spread<W: World + ?Sized>(w: &mut W, table: &WeaponAiTable, controller: Obj, unit: Obj, weapon: Option<usize>) {
    let Some(index) = weapon else { return };
    w.set_weapon_yaw(unit,index,None);
    let def = def_at(w, unit, index);
    if w.unit_scalars(unit).human {return;}
    let traits=table.traits(&def);
    if traits&TRAIT_AUTHORED_YAW!=0 || traits&TRAIT_SNIPER==0 {return;}
    let Some(target) = resolve(w, w.ctl(controller).target) else { return };
    let p = w.unit_scalars(unit).position;
    let q = w.unit_scalars(target).position;
    let d2 = length_sq32(f(p.x - q.x), f(p.y - q.y), f(p.z - q.z));
    let s = w.weapon_settings(unit,index).sniper_scale.unwrap_or(0.0);
    w.set_weapon_yaw(unit,index,Some(f(f(s) * sniper_yaw(d2)) * (180.0 / std::f64::consts::PI)));
}

#[cfg(test)]
mod swbf2_cadence_tests {
    use super::*;

    #[test]
    fn single_trigger_cadence_paces_shots_by_skill_and_emits_single_tick_pulses() {
        let measure_pulses = |skill: f64| {
            let (mut waiting, mut deadline) = (true, -1.0);
            let mut fired = Vec::new();
            for tick in 0..300 {
                let now = tick as f64 / 60.0;
                // Deterministic midpoint of [lo, hi].
                if step_swbf2_single_trigger(&mut waiting, &mut deadline, true, now, skill, |lo, hi| f(0.5 * f(lo + hi))) {
                    fired.push(tick);
                }
            }
            fired
        };
        let low = measure_pulses(4.0);
        let mid = measure_pulses(11.0);
        let high = measure_pulses(17.0);
        assert!(low.len() < mid.len() && mid.len() < high.len(), "higher skill fires faster: low={} mid={} high={}", low.len(), mid.len(), high.len());
        for w in mid.windows(2) {
            assert!(w[1] - w[0] > 1, "each single-trigger shot is separated by its cooldown");
        }
        // Releasing resets the wait so the next sustained request emits on the following tick.
        let (mut waiting, mut deadline) = (true, -1.0);
        assert!(!step_swbf2_single_trigger(&mut waiting, &mut deadline, false, 1.0, 14.0, |lo, _| lo));
        assert!(!step_swbf2_single_trigger(&mut waiting, &mut deadline, true, 1.02, 14.0, |lo, _| lo));
        assert!(step_swbf2_single_trigger(&mut waiting, &mut deadline, true, 1.04, 14.0, |lo, _| lo));
        assert!(!step_swbf2_single_trigger(&mut waiting, &mut deadline, true, 1.05, 14.0, |lo, _| lo));
    }

    #[test]
    fn burst_count_and_delay_scale_with_skill() {
        let low_count = swbf2_burst_count(3.0, 9.0, 4.0, |_, hi| hi);
        let high_count = swbf2_burst_count(3.0, 9.0, 17.0, |_, hi| hi);
        assert!(low_count < high_count, "higher skill fires longer bursts: {low_count} vs {high_count}");
        assert_eq!(swbf2_burst_count(1.0, 1.0, 1.0, |lo, _| lo), 2.0, "minimum burst floor is 2");
        let low_delay = swbf2_burst_delay(2.0, 4.0);
        let high_delay = swbf2_burst_delay(2.0, 17.0);
        assert!(low_delay > high_delay, "higher skill waits less between bursts: {low_delay} vs {high_delay}");
    }

    #[test]
    fn ai_target_flags_and_swbf2_melee_actions_preserve_classic_masks() {
        let table = WeaponAiTable::default();
        let impulse_def = JsObj {
            fields: vec![
                ("base".into(), Js::Str("cannon".into())),
                ("TargetPerson".into(), Js::Str("1".into())),
                ("TargetDroid".into(), Js::Str("1".into())),
                ("AITargetPerson".into(), Js::Str("0".into())),
                ("AITargetDroid".into(), Js::Str("0".into())),
            ],
            def: None,
        };
        let impulse = table.weapon_ai(&impulse_def);
        assert_ne!(impulse.category_mask & 1, 0, "classic mask still permits persons");
        assert_eq!(impulse.ai_category_mask & 1, 0, "AI person flag disables person targeting");
        assert_eq!(impulse.ai_category_mask & 4, 0, "AI droid flag disables droid targeting");

        let thrown_blade = JsObj {
            fields: vec![
                ("base".into(), Js::Str("meleethrow".into())),
                ("MinRange".into(), Js::Str("4".into())),
                ("MaxRange".into(), Js::Str("40".into())),
            ],
            def: None,
        };
        let thrown = table.weapon_ai(&thrown_blade);
        assert_eq!(thrown.swbf2_action, action::MELEE);
        let blade_def = JsObj {
            fields: vec![("base".into(), Js::Str("melee".into()))],
            def: None,
        };
        assert_eq!(table.weapon_ai(&blade_def).swbf2_action, action::MELEE);
    }

    struct TestWeaponWorld {
        swbf2: bool,
        throw_reloading: bool,
        shooter: UnitView,
        target: UnitView,
        weapons: Vec<super::super::world::WeaponView>,
        selected: Option<usize>,
        ctl: super::super::world::Ctl,
    }

    impl super::super::world::Draws for TestWeaponWorld {
        fn uniform_a(&mut self) -> f64 { 0.5 }
        fn uniform_b(&mut self) -> f64 { 0.5 }
        fn uniform_c(&mut self) -> f64 { 0.5 }
    }

    impl World for TestWeaponWorld {
        fn uses_swbf2(&self) -> bool { self.swbf2 }
        fn melee_throw_reloading(&self, _unit: Obj) -> bool { self.throw_reloading }
        fn time(&self) -> f64 { 1.0 }
        fn difficulty(&self) -> super::super::world::DifficultyView { Default::default() }
        fn mission_stay_in_turrets(&self) -> bool { false }
        fn mission_no_jet_obstacle_test(&self) -> bool { false }
        fn ai_stay_in_turrets(&mut self) -> bool { false }
        fn unit(&self, u: Obj) -> UnitView {
            if u == 1 { self.shooter.clone() } else { self.target.clone() }
        }
        fn has_arsenal(&self, _u: Obj) -> bool { true }
        fn weapon_count(&self, _u: Obj) -> usize { self.weapons.len() }
        fn current_weapon(&self, _u: Obj, _channel: f64) -> Option<usize> { self.selected }
        fn weapon(&self, _u: Obj, index: usize) -> Option<super::super::world::WeaponView> { self.weapons.get(index).cloned() }
        fn body_lease_count(&self, _u: Obj) -> usize { 0 }
        fn body_lease(&self, _u: Obj, _index: usize) -> Option<super::super::world::LeaseView> { None }
        fn weapon_settings(&self, _u: Obj, _index: usize) -> super::super::world::WeaponSettings { unimplemented!() }
        fn edit_weapon<R>(&mut self, _u: Obj, _index: usize, _edit: impl FnOnce(&mut super::super::world::WeaponSettings) -> R) -> R { unimplemented!() }
        fn origin(&mut self, u: Obj) -> Vector3 { if u == 1 { self.shooter.position } else { self.target.position } }
        fn forward(&mut self, _u: Obj) -> Vector3 { Vector3 { x: 1.0, y: 0.0, z: 0.0 } }
        fn aim_point(&mut self, u: Obj) -> Vector3 { if u == 1 { self.shooter.position } else { self.target.position } }
        fn sphere(&mut self, _u: Obj) -> super::super::world::Sphere { super::super::world::Sphere { center: Vector3::default(), radius: 1.0 } }
        fn set_intent_buttons(&mut self, _u: Obj, _b0: f64, _b1: f64) {}
        fn set_intent_aim(&mut self, _u: Obj, _aim: Option<Vector3>) {}
        fn arsenal_select(&mut self, _u: Obj, index: usize) -> bool { self.selected = Some(index); true }
        fn use_classic_random(&mut self, _u: Obj) {}
        fn object_buffer(&self) -> crate::ai::owned::scratch::List<Obj> { unimplemented!() }
        fn query(&mut self, _center: &Vector3, _radius: f64, _filter: QueryFilter) -> crate::ai::owned::scratch::List<Obj> { unimplemented!() }
        fn relation(&mut self, a: f64, b: f64) -> f64 { if a == b { 1.0 } else { -1.0 } }
        fn living(&mut self) -> crate::ai::owned::scratch::List<Obj> { unimplemented!() }
        fn los(&mut self, _from: &Vector3, _to: &Vector3, _a: Option<Obj>, _b: Option<Obj>) -> bool { true }
        fn curves_for(&mut self, _team: f64) -> Option<super::super::world::Curves> { None }
        fn character_team(&self, _character: Obj) -> f64 { 1.0 }
        fn fresh_perception(&mut self, _c: Obj) -> super::super::perception::Perception { Default::default() }
        fn ctl(&self, _c: Obj) -> &super::super::world::Ctl { &self.ctl }
        fn ctl_mut(&mut self, _c: Obj) -> &mut super::super::world::Ctl { &mut self.ctl }
        fn agent_controller(&self, _agent: Obj) -> Obj { 10 }
        fn tracker(&mut self, _agent: Obj) -> Option<&mut super::super::perception::TargetTracker> { None }
        fn is_controller(&self, _c: Obj) -> bool { true }
        fn agent_accepts_alert(&mut self, _agent: Obj, _known: bool, _point: &Vector3) -> bool { false }
        fn agent_accepts_hazard(&mut self, _agent: Obj) -> bool { false }
        fn agent_accepts_acoustic(&mut self, _agent: Obj, _source: Obj) -> bool { false }
        fn shared_target(&mut self, _controller: Obj) -> Option<Obj> { None }
        fn has_vehicle_dispatch(&self) -> bool { false }
        fn vehicle_dispatch(&mut self, _shared: &mut Shared, _controller: Obj, _unit: Obj, _target: Obj, _switch_allowed: bool) -> super::super::world::Engagement { Default::default() }
        fn has_mount_reach(&self) -> bool { false }
        fn mount_reach(&mut self, _unit: Obj, _point: &Vector3) -> bool { false }
        fn current_controllable(&mut self, _character: Obj) -> (bool, Option<Obj>) { (false, None) }
        fn character_remote(&self, _character: Obj) -> Option<Obj> { None }
        fn character_unit(&self, _character: Obj) -> Option<Obj> { None }
        fn is_unit(&self, _o: Obj) -> bool { true }
        fn enter_pattern(&mut self, _agent: Obj, _state: f64) {}
        fn exit_pattern(&mut self, _agent: Obj, _state: f64) {}
        fn tick_pattern(&mut self, _agent: Obj, _state: f64, _dt: f64) {}
        fn sidestep(&mut self, _agent: Obj) {}
        fn cover_hints(&mut self, _center: &Vector3, _half: f64) -> crate::ai::owned::scratch::List<super::super::world::HintView> { unimplemented!() }
        fn has_jet_hints(&self) -> bool { false }
        fn jet_hints_excluded(&mut self, _controller: Obj) -> bool { false }
        fn jet_hints_find(&mut self, _controller: Obj) -> Option<Obj> { None }
        fn jet_hints_enter(&mut self, _controller: Obj, _hint: Option<Obj>) {}
        fn jet_hints_exit(&mut self, _controller: Obj, _hint: Option<Obj>) {}
        fn jet_hints_tick(&mut self, _controller: Obj, _hint: Option<Obj>, _dt: f64) -> bool { true }
        fn nav_point_clear(&mut self, _point: &Vector3, _unit: Obj) -> Option<bool> { None }
        fn nav_segment_blocked(&mut self, _from: &Vector3, _to: &Vector3, _unit: Obj) -> Option<bool> { None }
        fn nav_direct_clear(&mut self, _unit: Obj, _point: &Vector3) -> Option<bool> { None }
        fn nav_in_danger_region(&mut self, _unit: Obj) -> Option<bool> { None }
        fn agent_sync(&mut self, _agent: &super::super::agent::CombatAgent) {}
        fn agent_resync(&mut self, _agent: &mut super::super::agent::CombatAgent) {}
        fn interrupts_tick(&mut self, _agent: Obj, _combat: &mut super::super::binding::CombatAi) -> bool { false }
        fn agent_board_policy(&mut self, _agent: Obj, _candidate: Obj) -> bool { false }
        fn has_helper(&self, _agent: Obj) -> bool { false }
        fn helper_arrived(&mut self, _agent: Obj) -> bool { false }
        fn helper_flying(&mut self, _agent: Obj) -> bool { false }
        fn helper_has_follow(&self, _agent: Obj) -> bool { false }
        fn helper_goto(&mut self, _agent: Obj, _point: &Vector3, _mode: Option<super::super::world::GotoMode>) {}
        fn helper_stop(&mut self, _agent: Obj) {}
        fn helper_tick(&mut self, _agent: Obj, _dt: f64) {}
        fn helper_follow(&mut self, _agent: Obj, _target: Obj) {}
        fn helper_terminate(&mut self, _agent: Obj) {}
        fn request_jet(&mut self, _controller: Obj, _vector: &Vector3) {}
        fn emit_vo(&mut self, _unit: Obj, _event: &str, _property: &str) {}
    }

    #[test]
    fn ai_category_mask_defaults_independently_of_target_flags_and_allows_craft_or_buildings() {
        let table = WeaponAiTable::default();
        // Soldier weapon clears TargetVehicle and TargetBuilding without authoring AITarget* keys.
        let blaster = Rc::new(JsObj {
            fields: vec![
                ("base".into(), Js::Str("cannon".into())),
                ("TargetVehicle".into(), Js::Str("0".into())),
                ("TargetBuilding".into(), Js::Str("0".into())),
            ],
            def: None,
        });
        let ai = table.weapon_ai(&blaster);
        assert_eq!(ai.category_mask & (8 | 16), 0, "classic mask clears vehicle and building bits");
        assert_eq!(ai.ai_category_mask & (8 | 16), 8 | 16, "2005 AI mask defaults all standard categories on");

        let make_world = |swbf2: bool, def: Rc<JsObj>, target_kind: f64, target_category: f64| TestWeaponWorld {
            swbf2,
            throw_reloading: false,
            shooter: UnitView { obj: 1, alive: true, team: 1.0, kind: 0.0, generation: 1.0, ..Default::default() },
            target: UnitView {
                obj: 2,
                alive: true,
                team: 2.0,
                kind: target_kind,
                category: Some(target_category),
                generation: 1.0,
                position: Vector3 { x: 15.0, y: 0.0, z: 0.0 },
                ..Default::default()
            },
            weapons: vec![super::super::world::WeaponView { def, clip: 10.0, reserve: 10.0, rounds: Js::Num(10.0), ..Default::default() }],
            selected: Some(0),
            ctl: super::super::world::Ctl { perception: Some(Default::default()), ..Default::default() },
        };

        // 2005 bot can aim the blaster at a craft (category 3) and a building (category 4).
        for (kind, cat) in [(5.0, 3.0), (8.0, 4.0)] {
            let mut w2005 = make_world(true, blaster.clone(), kind, cat);
            set_target(&mut w2005, &table, 10, Some(1), Some(2));
            assert_eq!(w2005.ctl.perception.as_ref().unwrap().fire.weapon_target.map(|r| r.obj), Some(2));

            // 2004 bot uses category_mask and refuses both targets.
            let mut w2004 = make_world(false, blaster.clone(), kind, cat);
            set_target(&mut w2004, &table, 10, Some(1), Some(2));
            assert_eq!(w2004.ctl.perception.as_ref().unwrap().fire.weapon_target, None);
        }

        // Explicit AITargetVehicle=0 blocks craft on 2005 while keeping buildings allowed.
        let no_ai_veh = Rc::new(JsObj {
            fields: vec![
                ("base".into(), Js::Str("cannon".into())),
                ("TargetVehicle".into(), Js::Str("1".into())),
                ("AITargetVehicle".into(), Js::Str("0".into())),
            ],
            def: None,
        });
        let mut w_craft = make_world(true, no_ai_veh.clone(), 5.0, 3.0);
        set_target(&mut w_craft, &table, 10, Some(1), Some(2));
        assert_eq!(w_craft.ctl.perception.as_ref().unwrap().fire.weapon_target, None);

        let mut w_bldg = make_world(true, no_ai_veh, 8.0, 4.0);
        set_target(&mut w_bldg, &table, 10, Some(1), Some(2));
        assert_eq!(w_bldg.ctl.perception.as_ref().unwrap().fire.weapon_target.map(|r| r.obj), Some(2));
    }

    #[test]
    fn thrown_blade_is_skipped_only_when_reloading_and_switching_is_allowed() {
        let throw_ord = Js::Obj(Rc::new(JsObj {
            fields: vec![("base".into(), Js::Str("bolt".into())), ("Damage".into(), Js::Num(200.0))],
            def: None,
        }));
        let throw_def = Rc::new(JsObj {
            fields: vec![
                ("base".into(), Js::Str("meleethrow".into())),
                ("MinRange".into(), Js::Str("4".into())),
                ("MaxRange".into(), Js::Str("40".into())),
                ("ordnance".into(), throw_ord),
            ],
            def: None,
        });
        let rifle_ord = Js::Obj(Rc::new(JsObj {
            fields: vec![("base".into(), Js::Str("bolt".into())), ("Damage".into(), Js::Num(40.0))],
            def: None,
        }));
        let rifle_def = Rc::new(JsObj {
            fields: vec![
                ("base".into(), Js::Str("cannon".into())),
                ("MinRange".into(), Js::Str("0".into())),
                ("MaxRange".into(), Js::Str("60".into())),
                ("ordnance".into(), rifle_ord),
            ],
            def: None,
        });
        let mut w = TestWeaponWorld {
            swbf2: true,
            throw_reloading: false,
            shooter: UnitView { obj: 1, alive: true, team: 1.0, kind: 0.0, generation: 1.0, ..Default::default() },
            target: UnitView {
                obj: 2,
                alive: true,
                team: 2.0,
                kind: 0.0,
                category: Some(0.0),
                generation: 1.0,
                combat_max_health: Js::Num(100.0),
                position: Vector3 { x: 15.0, y: 0.0, z: 0.0 },
                ..Default::default()
            },
            weapons: vec![
                super::super::world::WeaponView { def: throw_def, bottomless: true, rounds: Js::Num(1.0), ..Default::default() },
                super::super::world::WeaponView { def: rifle_def, bottomless: true, rounds: Js::Num(30.0), ..Default::default() },
            ],
            selected: Some(1),
            ctl: super::super::world::Ctl { perception: Some(Default::default()), ..Default::default() },
        };
        let mut shared = Shared::default();
        // Ready thrown blade in band wins over the lower-damage rifle.
        let ready = select_weapon(&mut w, &mut shared, Some(10), 1, Some(2), true);
        assert_eq!((ready.ok, ready.index, ready.action), (true, Some(0), action::MELEE));
        // While reloading and switching is allowed, selection skips the thrown blade.
        w.throw_reloading = true;
        let switching = select_weapon(&mut w, &mut shared, Some(10), 1, Some(2), true);
        assert_eq!((switching.ok, switching.index), (true, Some(1)));
        // When switching is disallowed, a reloading thrown blade is not excluded.
        let no_switch = select_weapon(&mut w, &mut shared, Some(10), 1, Some(2), false);
        assert_eq!((no_switch.ok, no_switch.index), (true, Some(0)));
        // Out of MinRange..MaxRange, the thrown blade is excluded even when ready.
        w.throw_reloading = false;
        w.target.position.x = 2.0;
        let too_close = select_weapon(&mut w, &mut shared, Some(10), 1, Some(2), true);
        assert_eq!((too_close.ok, too_close.index), (true, Some(1)));
    }
}
