//! The content combat reads: a weapon's class at each of its levels (the
//! class, then each charge level laid over it as `{ ...def, ...level }`,
//! src/weapons.js:122), the round it fires (an ordnance class) and a unit's
//! class. Fields are held as the JavaScript values the definitions hold
//! ([`Js`]) and read with JavaScript's own rules (`x || d`, `x ?? d`,
//! comparisons through ToNumber), so a field the content leaves out reads
//! here as it reads and src/combat.js.

use crate::store::DefId;
use std::rc::Rc;

/// A JavaScript value as a definition holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Js {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Arr(Rc<Vec<Js>>),
    Obj(Rc<JsObj>),
}

/// A plain object's own properties, in JavaScript's order; `def` names a
/// content definition's place in its table (a weapon's ordnance class).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsObj {
    pub fields: Vec<(Rc<str>, Js)>,
    pub def: Option<(DefTable, DefId)>,
}

/// Which table a definition is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefTable {
    Weapon,
    Ordnance,
    Item,
    UnitClass,
    Other,
}

const UNDEFINED: Js = Js::Undefined;

impl JsObj {
    pub fn get(&self, key: &str) -> &Js {
        self.fields
            .iter()
            .find(|(k, _)| &**k == key)
            .map(|(_, v)| v)
            .unwrap_or(&UNDEFINED)
    }

    /// `{ ...self, ...over }`: self's properties in order, each that `over`
    /// also has taking its value in place, then `over`'s others in order.
    pub fn spread(&self, over: &JsObj) -> JsObj {
        let mut fields = self.fields.clone();
        for (k, v) in &over.fields {
            match fields.iter_mut().find(|(key, _)| key == k) {
                Some(slot) => slot.1 = v.clone(),
                None => fields.push((k.clone(), v.clone())),
            }
        }
        JsObj { fields, def: None }
    }
}

impl Js {
    /// A property (undefined on anything but an object).
    pub fn get(&self, key: &str) -> &Js {
        match self {
            Js::Obj(o) => o.get(key),
            _ => &UNDEFINED,
        }
    }

    /// An array's item (undefined past its end or on anything else).
    pub fn at(&self, i: usize) -> &Js {
        match self {
            Js::Arr(a) => a.get(i).unwrap_or(&UNDEFINED),
            _ => &UNDEFINED,
        }
    }

    /// An array's length (undefined's is read nowhere: 0).
    pub fn len(&self) -> usize {
        match self {
            Js::Arr(a) => a.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_nullish(&self) -> bool {
        matches!(self, Js::Undefined | Js::Null)
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Js::Undefined)
    }

    pub fn truthy(&self) -> bool {
        match self {
            Js::Undefined | Js::Null => false,
            Js::Bool(b) => *b,
            Js::Num(x) => !(*x == 0.0 || x.is_nan()),
            Js::Str(s) => !s.is_empty(),
            Js::Arr(_) | Js::Obj(_) => true,
        }
    }

    /// ToNumber.
    pub fn num(&self) -> f64 {
        match self {
            Js::Undefined => f64::NAN,
            Js::Null => 0.0,
            Js::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Js::Num(x) => *x,
            Js::Str(s) => bfe_content::js::string_to_number(s),
            // ToPrimitive of an array is its join: "" is 0, one item is
            // that item as text; an object is NaN.
            Js::Arr(a) => match a.len() {
                0 => 0.0,
                1 => match &a[0] {
                    Js::Undefined | Js::Null => 0.0,
                    Js::Num(x) => *x,
                    Js::Str(s) => bfe_content::js::string_to_number(s),
                    _ => f64::NAN,
                },
                _ => f64::NAN,
            },
            Js::Obj(_) => f64::NAN,
        }
    }

    /// `x || d` where a number is wanted.
    pub fn or(&self, d: f64) -> f64 {
        if self.truthy() {
            self.num()
        } else {
            d
        }
    }

    /// `x ?? d` where a number is wanted.
    pub fn nn(&self, d: f64) -> f64 {
        if self.is_nullish() {
            d
        } else {
            self.num()
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Js::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `a === "text"`.
    pub fn is_str(&self, s: &str) -> bool {
        matches!(self, Js::Str(x) if &**x == s)
    }

    pub fn obj(&self) -> Option<&Rc<JsObj>> {
        match self {
            Js::Obj(o) => Some(o),
            _ => None,
        }
    }
}

/// The weapon class fields combat reads, by name: each level keeps them in
/// this order ([`Level::get`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum K {
    Class,
    Base,
    Channel,
    AnimationBank,
    RoundsPerClip,
    Ammo,
    SpreadThreshold,
    MaxPressedTime,
    LockTime,
    ReloadTime,
    StandStillSpread,
    CrouchStillSpread,
    ProneStillSpread,
    StandAimSpread,
    CrouchAimSpread,
    ProneAimSpread,
    StandMoveSpread,
    CrouchMoveSpread,
    ProneMoveSpread,
    YawSpread,
    PitchSpread,
    HeatRecoverRate,
    HeatThreshold,
    SpreadRecoverRate,
    ShotDelay,
    TriggerSingle,
    SalvoCount,
    InitialSalvoDelay,
    SalvoDelay,
    ClearLockOnFire,
    Ordnance,
    HideOnFire,
    ShotsPerSalvo,
    ShotPatternPitchYaw,
    ShotPatternCount,
    HeatPerShot,
    SpreadPerShot,
    SpreadLimit,
    At,
    Item,
    Droid,
    Velocity,
    MaxStrength,
    MinStrength,
    KickStrength,
    KickBuildup,
    ShotElevate,
    ZoomMin,
    ZoomRate,
    ZoomMax,
    MaxItems,
    AllowDestruct,
    Swings,
    Strike,
    LockOnRange,
    ZoomFirstPerson,
    TriggerAll,
}

const KEYS: [&str; 57] = [
    "class",
    "base",
    "channel",
    "AnimationBank",
    "RoundsPerClip",
    "ammo",
    "SpreadThreshold",
    "MaxPressedTime",
    "LockTime",
    "ReloadTime",
    "StandStillSpread",
    "CrouchStillSpread",
    "ProneStillSpread",
    "StandAimSpread",
    "CrouchAimSpread",
    "ProneAimSpread",
    "StandMoveSpread",
    "CrouchMoveSpread",
    "ProneMoveSpread",
    "YawSpread",
    "PitchSpread",
    "HeatRecoverRate",
    "HeatThreshold",
    "SpreadRecoverRate",
    "ShotDelay",
    "TriggerSingle",
    "SalvoCount",
    "InitialSalvoDelay",
    "SalvoDelay",
    "ClearLockOnFire",
    "ordnance",
    "HideOnFire",
    "ShotsPerSalvo",
    "ShotPatternPitchYaw",
    "ShotPatternCount",
    "HeatPerShot",
    "SpreadPerShot",
    "SpreadLimit",
    "at",
    "item",
    "droid",
    "Velocity",
    "MaxStrength",
    "MinStrength",
    "KickStrength",
    "KickBuildup",
    "ShotElevate",
    "ZoomMin",
    "ZoomRate",
    "ZoomMax",
    "MaxItems",
    "AllowDestruct",
    "swings",
    "strike",
    "LockOnRange",
    "ZoomFirstPerson",
    "TriggerAll",
];

/// PlayerCombat.arc's immutable emitter fields. Resolve names and numeric
/// defaults during content loading, rather than a keyed lookup for every link.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcDef {
    pub length: f64,
    pub sine: f64,
    pub name: Option<Rc<str>>,
    pub damage: f64,
    pub max_damage: f64,
    pub first_radius: f64,
    pub max_targets: f64,
    pub threshold: f64,
    pub jump: f64,
    pub drop: f64,
    pub no_chaining: bool,
    pub affect_friends: bool,
    pub affect_enemies: bool,
    pub smolder_damage_rate: f64,
    pub smolder_timer: f64,
    pub smolder_effect: Option<Rc<str>>,
    pub aimer_effect: Option<Rc<str>>,
}
impl ArcDef {
    fn read(def: &Js) -> Self {
        let flag = |key: &str, default: bool| {
            let v = def.get(key);
            if v.is_nullish() {
                default
            } else {
                let n = v.num();
                n != 0.0 && !n.is_nan()
            }
        };
        let opt_str = |key: &str| match def.get(key) {
            Js::Str(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        Self {
            length: def.get("ConeLength").nn(10.0),
            sine: v8math::sin(((def.get("ConeAngle").nn(60.0) / 2.0) * std::f64::consts::PI) / 180.0),
            name: match def.get("LightningEffect") {
                Js::Str(s) => Some(s.clone()),
                _ => None,
            },
            damage: def.get("Damage").nn(0.0),
            max_damage: def.get("MaxDamage").nn(0.0),
            first_radius: def.get("FirstRadius").num(),
            max_targets: def.get("MaxTargets").nn(5.0),
            threshold: def.get("DamageThreshold").nn(0.0),
            jump: def.get("MaxJumpDistance").nn(10.0),
            drop: def.get("DamageDropPerTarget").nn(0.0),
            no_chaining: flag("NoChaining", false),
            affect_friends: flag("AffectFriends", true),
            affect_enemies: flag("AffectEnemies", true),
            smolder_damage_rate: jsrt::js_max(def.get("SmolderDamageRate").nn(0.0), 0.0),
            smolder_timer: jsrt::js_max(def.get("SmolderEffectTimer").nn(0.0), 0.0),
            smolder_effect: opt_str("SmolderEffect"),
            aimer_effect: opt_str("PlayEffectOnOwnerAimer"),
        }
    }
}

/// One prepared swing's length and hit time (meleeSwings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swing {
    pub duration: f64,
    pub hit: f64,
}

/// meleeSwings takes at most three authored swings. Convert the
/// two numeric fields once: updating a held blade never rebuilds a class table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeleeSwings {
    entries: [Swing; 3],
    count: usize,
}
impl MeleeSwings {
    fn read(def: &Js) -> Self {
        let mut result = Self {
            entries: [Swing { duration: 1.0, hit: 0.5 }; 3],
            count: def.len().min(3).max(1),
        };
        for i in 0..def.len().min(3) {
            result.entries[i] = Swing {
                duration: def.at(i).get("duration").num(),
                hit: def.at(i).get("hit").num(),
            };
        }
        result
    }
    pub fn entries(&self) -> &[Swing] {
        &self.entries[..self.count]
    }
}

/// A weapon class at one level: its object and prepared fields combat reads.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponBase { Other, Shield, Launcher, Grenade, Dispenser, Detonator, Remote, Melee, Repair }
impl WeaponBase {
    fn read(value:&Js) -> Self {
        match value.str() {
            Some("shield")=>Self::Shield,Some("launcher")=>Self::Launcher,
            Some("grenade")=>Self::Grenade,Some("dispenser")=>Self::Dispenser,
            Some("remote")=>Self::Remote,Some("melee")=>Self::Melee,
            Some("repair")=>Self::Repair,_=>Self::Other,
        }
    }
    fn read_for_family(family: bfe_family::GameFamily, value: &Js) -> Self {
        if family == bfe_family::GameFamily::Swbf2 && value.str() == Some("detonator") {
            Self::Detonator
        } else {
            Self::read(value)
        }
    }
    pub fn thrown(&self) -> bool {matches!(self,Self::Grenade|Self::Dispenser|Self::Detonator|Self::Remote)}
}

#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub obj: Rc<JsObj>,
    pub kind: WeaponBase,
    pub missile: bool,
    pub emitter: bool,
    pub arc: ArcDef,
    pub bank: Rc<str>,
    pub swings: MeleeSwings,
    pub can_deflect: bool,
    // LiveObjects locks use authored flags, including undefined's distinction
    // from numeric zero. Prepare them here rather than reading keys per unit.
    pub repair: crate::battle::live::Definition,
    vals: Vec<Js>,
    pub cosmetic_round: bool,
    // givePowerup excludes dispensers which themselves make power-ups.
    // Resolve this nested content property once, outside the pickup loop.
    pub powerup_dispenser: bool,
    /// A shield weapon's bubble, which rounds meet while the shield is up.
    pub bubble: Option<Bubble>,
}

/// A droideka's shield as a body rounds can meet: an ellipsoid whose three
/// radii are the shield class's `ShieldRadius` (one number fills all three),
/// about `ShieldOffset` in the body's frame (x to its left, y up, z ahead).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bubble {
    pub radii: [f64; 3],
    pub offset: [f64; 3],
}

impl Bubble {
    /// From a shield class; None without a positive radius on every axis.
    pub fn read(obj: &JsObj) -> Option<Bubble> {
        let radius = numbers(obj.get("ShieldRadius"));
        let first = *radius.first()?;
        let radii = [first, radius.get(1).copied().unwrap_or(first), radius.get(2).copied().unwrap_or(first)];
        if !radii.iter().all(|r| *r > 0.0 && r.is_finite()) {
            return None;
        }
        // A missing offset component is zero, as the class reads it.
        let offset = numbers(obj.get("ShieldOffset"));
        let at = |i: usize| offset.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
        Some(Bubble { radii, offset: [at(0), at(1), at(2)] })
    }
}

/// The numbers of a key given as one number, a list or a string "x y z".
fn numbers(value: &Js) -> Vec<f64> {
    match value {
        Js::Num(n) => vec![*n],
        Js::Arr(list) => list.iter().map(Js::num).collect(),
        Js::Str(text) => text.split_whitespace().map(|part| part.parse().unwrap_or(f64::NAN)).collect(),
        _ => Vec::new(),
    }
}

impl Level {
    pub fn new(obj: Rc<JsObj>) -> Level {
        Self::new_for_family(bfe_family::GameFamily::Swbf1, obj)
    }

    pub fn new_for_family(family: bfe_family::GameFamily, obj: Rc<JsObj>) -> Level {
        let kind = WeaponBase::read_for_family(family, obj.get("base"));
        let bubble = if matches!(kind, WeaponBase::Shield) { Bubble::read(&obj) } else { None };
        let vals = KEYS.iter().map(|k| obj.get(k).clone()).collect();
        let ordnance = obj.get("ordnance");
        // PredictedShots.fire admits these classes before choosing a barrel.
        let cosmetic_round = ordnance.truthy() && !matches!(ordnance.get("base").str(), Some("sticky" | "grenade" | "emitterordnance"));
        let powerup_dispenser = obj.get("base").is_str("dispenser") && obj.get("item").get("base").is_str("powerupitem");
        let missile = ordnance.get("base").is_str("missile");
        let emitter = ordnance.get("base").is_str("emitterordnance");
        let arc = ArcDef::read(ordnance);
        let swings = MeleeSwings::read(obj.get("swings"));
        let deflect = obj.get("CanDeflect").num();
        let repair = repair_definition(&obj);
        let value = obj.get("AnimationBank");
        let bank = if !value.truthy() {
            Rc::from("rifle")
        } else {
            match value {
                Js::Str(s) => s.clone(),
                other => Rc::from(format!("{}", other.num())),
            }
        };
        Level {
            obj,
            kind,
            vals,
            missile,
            emitter,
            arc,
            bank,
            swings,
            can_deflect: deflect != 0.0 && !deflect.is_nan(),
            repair,
            cosmetic_round,
            powerup_dispenser,
            bubble,
        }
    }

    #[inline]
    pub fn get(&self, k: K) -> &Js {
        &self.vals[k as usize]
    }

    /// Any other field, by name.
    pub fn field(&self, key: &str) -> &Js {
        self.obj.get(key)
    }
}

// LiveObjects.repairs reads four amounts and rejects only explicit zero target
// flags. This runs at content load, preserving primitives without cloning or
// searching definition strings while a repair tool acquires its next target.
fn repair_definition(obj: &JsObj) -> crate::battle::live::Definition {
    use crate::ordnance::defs::{ExplosionDef, Field};
    fn field(value: &Js) -> Field {
        match value {
            Js::Undefined => Field::Undefined,
            Js::Null => Field::Null,
            Js::Bool(b) => Field::Bool(*b),
            Js::Num(n) => Field::Num(*n),
            Js::Str(s) => Field::Str(s.to_string()),
            _ => Field::Object,
        }
    }
    crate::battle::live::Definition {
        repair: ["BuildingHealth", "VehicleHealth", "BuildingRebuild", "BuildingBuild"]
            .map(|name| field(obj.get(name))),
        target_vehicle: field(obj.get("TargetVehicle")),
        target_neutral: field(obj.get("TargetNeutral")),
        target_enemy: field(obj.get("TargetEnemy")),
        target_friendly: field(obj.get("TargetFriendly")),
        range: field(obj.get("LockOnRange")),
        angle: field(obj.get("LockOnAngle")),
        shield_scale: field(obj.get("ShieldScale")),
        explosion: ExplosionDef::read(|name| field(obj.get(name))),
    }
}

/// A weapon class: its levels (`levels`).
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponClass {
    pub levels: Vec<Level>,
}

impl WeaponClass {
    /// `[def, ...(def.charges || []).map((level) => ({ ...def, ...level }))]`.
    pub fn new(def: Rc<JsObj>) -> WeaponClass {
        Self::new_for_family(bfe_family::GameFamily::Swbf1, def)
    }

    pub fn new_for_family(family: bfe_family::GameFamily, def: Rc<JsObj>) -> WeaponClass {
        let mut levels = vec![Level::new_for_family(family, def.clone())];
        let charges = def.get("charges");
        if charges.truthy() {
            if let Js::Arr(list) = charges {
                for level in list.iter() {
                    let over = level.obj().map(|o| (**o).clone()).unwrap_or_default();
                    levels.push(Level::new_for_family(family, Rc::new(def.spread(&over))));
                }
            }
        }
        WeaponClass { levels }
    }

    pub fn base(&self) -> &Level {
        &self.levels[0]
    }
}

/// The battle's weapon classes, by their DefId in the `weapon` table.
#[derive(Clone, Debug, Default)]
pub struct WeaponDefs {
    pub classes: Vec<Option<WeaponClass>>,
}

impl WeaponDefs {
    pub fn set(&mut self, id: DefId, class: WeaponClass) {
        let i = id.0 as usize;
        if self.classes.len() <= i {
            self.classes.resize(i + 1, None);
        }
        self.classes[i] = Some(class);
    }

    pub fn has(&self, id: DefId) -> bool {
        self.classes.get(id.0 as usize).is_some_and(|c| c.is_some())
    }

    pub fn class(&self, id: DefId) -> &WeaponClass {
        self.classes[id.0 as usize]
            .as_ref()
            .expect("a weapon class the battle registered")
    }
}
