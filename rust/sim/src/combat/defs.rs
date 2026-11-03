//! The content combat reads: a weapon's class at each of its levels (the
//! class, then each charge level laid over it as `{ ...def, ...level }`,
//! src/weapons.js:122), the round it fires (an ordnance class) and a unit's
//! class. Fields are held as the JavaScript values the definitions hold
//! ([`Js`]) and read with JavaScript's own rules (`x || d`, `x ?? d`,
//! comparisons through ToNumber), so a field the content leaves out reads
//! here as it reads in src/weapons.js and src/combat.js.
//!
//! A weapon class is known by its definition's index in the battle's
//! `weapon` table ([`DefId`], docs/sim-store.md "Definitions"); the battle
//! registers each as it meets it, and a replay builds them from a capture's
//! definition records.

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
        self.fields.iter().find(|(k, _)| &**k == key).map(|(_, v)| v).unwrap_or(&UNDEFINED)
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
}

const KEYS: [&str; 55] = [
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
];

/// A weapon class at one level: its object and the fields combat reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub obj: Rc<JsObj>,
    vals: Vec<Js>,
}

impl Level {
    pub fn new(obj: Rc<JsObj>) -> Level {
        let vals = KEYS.iter().map(|k| obj.get(k).clone()).collect();
        Level { obj, vals }
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

/// A weapon class: its levels (src/weapons.js:122 `levels`).
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponClass {
    pub levels: Vec<Level>,
}

impl WeaponClass {
    /// `[def, ...(def.charges || []).map((level) => ({ ...def, ...level }))]`.
    pub fn new(def: Rc<JsObj>) -> WeaponClass {
        let mut levels = vec![Level::new(def.clone())];
        let charges = def.get("charges");
        if charges.truthy() {
            if let Js::Arr(list) = charges {
                for level in list.iter() {
                    let over = level.obj().map(|o| (**o).clone()).unwrap_or_default();
                    levels.push(Level::new(Rc::new(def.spread(&over))));
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
        self.classes[id.0 as usize].as_ref().expect("a weapon class the battle registered")
    }
}
