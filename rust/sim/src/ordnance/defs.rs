//! The content definitions the rounds and items read: an ordnance class
//! (a bolt's, a rocket's, a grenade's), an explosion, an item class. Each
//! field is held as the JavaScript value the definition holds there
//! ([`Field`]), and read with JavaScript's own rules (`??`, `||`, `typeof`,
//! ToNumber), so a field the content leaves out or gives an odd type reads
//! here as it reads in src/ordnance.js and src/items.js.
//!
//! A definition is known by its index in the battle's table of its sort
//! ([`DefId`], docs/sim-store.md "Definitions"); the battle registers its
//! definitions' fields as it meets them (or a replay reads them from a
//! capture's definition records).

use crate::store::DefId;

pub use bfe_content::js::string_to_number;

/// A definition's field as JavaScript reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// An explosion's definition: its index in the battle's explosions.
    Explosion(DefId),
    /// Any other object (an array, a record).
    Object,
}

impl Field {
    /// null or undefined (what `??` tests).
    pub fn is_nullish(&self) -> bool {
        matches!(self, Field::Undefined | Field::Null)
    }

    /// JavaScript's truthiness (what `if` and `||` test).
    pub fn truthy(&self) -> bool {
        match self {
            Field::Undefined | Field::Null => false,
            Field::Bool(b) => *b,
            Field::Num(x) => !(*x == 0.0 || x.is_nan()),
            Field::Str(s) => !s.is_empty(),
            Field::Explosion(_) | Field::Object => true,
        }
    }

    /// `typeof x === "object"` (null included).
    pub fn is_object(&self) -> bool {
        matches!(self, Field::Null | Field::Explosion(_) | Field::Object)
    }

    /// ToNumber, as arithmetic and comparisons with a number apply it.
    pub fn num(&self) -> f64 {
        match self {
            Field::Undefined => f64::NAN,
            Field::Null => 0.0,
            Field::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Field::Num(x) => *x,
            Field::Str(s) => string_to_number(s),
            // A plain object's text is "[object Object]".
            Field::Explosion(_) | Field::Object => f64::NAN,
        }
    }

    /// `x ?? fallback`, as a number.
    pub fn or_nullish(&self, fallback: f64) -> f64 {
        if self.is_nullish() {
            fallback
        } else {
            self.num()
        }
    }

    /// `x || fallback`, as a number.
    pub fn or(&self, fallback: f64) -> f64 {
        if self.truthy() {
            self.num()
        } else {
            fallback
        }
    }

    /// `x > 0`.
    pub fn positive(&self) -> bool {
        self.num() > 0.0
    }

    /// `x === name` for a string.
    pub fn is(&self, name: &str) -> bool {
        matches!(self, Field::Str(s) if s == name)
    }
}

/// An ordnance class (weapons.json's `ordnance`): what fire() makes of it
/// and how it flies.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceDef {
    pub base: Field,
    pub velocity: Field,
    pub life_span: Field,
    pub gravity: Field,
    pub laser_length: Field,
    pub glow_length: Field,
    pub blur_length: Field,
    pub damage: Field,
    pub scale_time: Field,
    pub turn_rate: Field,
    pub waver_turn: Field,
    pub waver_rate: Field,
    pub stick_terrain: Field,
    pub stick_building: Field,
    pub explosion_impact: Field,
    pub explosion_expire: Field,
}

impl OrdnanceDef {
    pub const FIELDS: [&'static str; 16] = [
        "base",
        "Velocity",
        "LifeSpan",
        "Gravity",
        "LaserLength",
        "GlowLength",
        "BlurLength",
        "Damage",
        "ScaleTime",
        "TurnRate",
        "WaverTurn",
        "WaverRate",
        "StickTerrain",
        "StickBuilding",
        "ExplosionImpact",
        "ExplosionExpire",
    ];

    /// From its fields by name.
    pub fn read(mut get: impl FnMut(&str) -> Field) -> OrdnanceDef {
        OrdnanceDef {
            base: get("base"),
            velocity: get("Velocity"),
            life_span: get("LifeSpan"),
            gravity: get("Gravity"),
            laser_length: get("LaserLength"),
            glow_length: get("GlowLength"),
            blur_length: get("BlurLength"),
            damage: get("Damage"),
            scale_time: get("ScaleTime"),
            turn_rate: get("TurnRate"),
            waver_turn: get("WaverTurn"),
            waver_rate: get("WaverRate"),
            stick_terrain: get("StickTerrain"),
            stick_building: get("StickBuilding"),
            explosion_impact: get("ExplosionImpact"),
            explosion_expire: get("ExplosionExpire"),
        }
    }

    /// ordnance.js glowLength: reading LaserLength sets GlowLength to three
    /// times it, and a GlowLength named after replaces that.
    pub fn glow(&self) -> f64 {
        if !self.glow_length.is_nullish() {
            self.glow_length.num()
        } else if self.laser_length.truthy() {
            self.laser_length.num() * 3.0
        } else {
            0.0
        }
    }
}

/// An explosion (an ordnance's ExplosionImpact or ExplosionExpire, an
/// item's ExplosionDeath and the rest): what its blast does to an item.
#[derive(Clone, Debug, PartialEq)]
pub struct ExplosionDef {
    pub damage: Field,
    pub damage_radius_inner: Field,
    pub damage_radius_outer: Field,
    /// PersonScale, AnimalScale, DroidScale, VehicleScale, BuildingScale.
    pub scales: [Field; 5],
}

/// ordnance.js HEALTH_SCALES's keys, in the order of `ExplosionDef::scales`.
pub const HEALTH_TYPES: [&str; 5] = ["person", "animal", "droid", "vehicle", "building"];
pub const SCALE_FIELDS: [&str; 5] = ["PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale"];

impl ExplosionDef {
    pub fn read(mut get: impl FnMut(&str) -> Field) -> ExplosionDef {
        ExplosionDef {
            damage: get("Damage"),
            damage_radius_inner: get("DamageRadiusInner"),
            damage_radius_outer: get("DamageRadiusOuter"),
            scales: SCALE_FIELDS.map(&mut get),
        }
    }

    /// ordnance.js healthScale(def, healthType): the scale its health type
    /// names (`String(healthType).toLowerCase()`; person when it names
    /// none), 1 when the explosion gives none.
    pub fn health_scale(&self, health_type: &Field) -> f64 {
        let text = match health_type {
            Field::Undefined => "undefined".to_string(),
            Field::Null => "null".to_string(),
            Field::Str(s) => s.to_lowercase(),
            // Numbers and booleans name no health type.
            _ => String::new(),
        };
        let at = HEALTH_TYPES.iter().position(|t| *t == text).unwrap_or(0);
        self.scales[at].or_nullish(1.0)
    }

    /// ordnance.js blastDamage: Damage, by the falloff over the distance
    /// and the scale for the health type.
    pub fn damage_at(&self, distance: f64, health_type: &Field) -> f64 {
        self.damage.or(0.0) * self.share(distance) * self.health_scale(health_type)
    }

    /// ordnance.js blastShare.
    pub fn share(&self, distance: f64) -> f64 {
        falloff(distance, self.damage_radius_inner.or(0.0), self.damage_radius_outer.or(0.0))
    }
}

/// ordnance.js falloff: full inside the inner radius, down to nothing at the
/// outer.
pub fn falloff(distance: f64, inner: f64, outer: f64) -> f64 {
    if distance < inner {
        return 1.0;
    }
    if distance < outer && outer > inner {
        return (outer - distance) / (outer - inner);
    }
    0.0
}

/// An item class (a side's `items`: a mine, a power-up).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemDef {
    pub base: Field,
    pub gravity: Field,
    pub rebound: Field,
    pub friction: Field,
    pub life_span: Field,
    pub max_health: Field,
    pub health_type: Field,
    pub explosion_death: Field,
    pub explosion_trigger: Field,
    pub explosion_expire: Field,
    pub trigger_radius: Field,
    pub suppress_radius: Field,
}

impl ItemDef {
    pub fn read(mut get: impl FnMut(&str) -> Field) -> ItemDef {
        ItemDef {
            base: get("base"),
            gravity: get("Gravity"),
            rebound: get("Rebound"),
            friction: get("Friction"),
            life_span: get("LifeSpan"),
            max_health: get("MaxHealth"),
            health_type: get("HealthType"),
            explosion_death: get("ExplosionDeath"),
            explosion_trigger: get("ExplosionTrigger"),
            explosion_expire: get("ExplosionExpire"),
            trigger_radius: get("TriggerRadius"),
            suppress_radius: get("SuppressRadius"),
        }
    }
}

/// The battle's definitions the rounds and items read, by index, and its
/// item pools (items.js `pools`: how many items of each base the world
/// holds at most, power-ups 25 and mines 50 unless the mission says).
#[derive(Clone, Debug, Default)]
pub struct Defs {
    pub ordnance: Vec<OrdnanceDef>,
    pub explosion: Vec<ExplosionDef>,
    pub item: Vec<ItemDef>,
    pub pools: Vec<(String, Field)>,
}

impl Defs {
    pub fn ordnance(&self, id: DefId) -> &OrdnanceDef {
        &self.ordnance[id.0 as usize]
    }
    pub fn explosion(&self, id: DefId) -> &ExplosionDef {
        &self.explosion[id.0 as usize]
    }
    pub fn item(&self, id: DefId) -> &ItemDef {
        &self.item[id.0 as usize]
    }

    /// `pools[base] ?? Infinity`.
    pub fn pool(&self, base: &Field) -> f64 {
        match base {
            Field::Str(name) => self.pools.iter().find(|(k, _)| k == name).map_or(f64::INFINITY, |(_, v)| v.or_nullish(f64::INFINITY)),
            _ => f64::INFINITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_read_as_javascript_reads_them() {
        assert_eq!(Field::Undefined.or_nullish(1.0), 1.0);
        assert_eq!(Field::Num(0.0).or_nullish(1.0), 0.0);
        assert_eq!(Field::Num(0.0).or(1.0), 1.0);
        assert_eq!(Field::Str("2.5".into()).or(1.0), 2.5);
        assert!(Field::Null.is_object() && !Field::Undefined.is_object());
        assert!(!Field::Undefined.positive() && Field::Num(0.5).positive());
        let e = ExplosionDef::read(|n| match n {
            "Damage" => Field::Num(100.0),
            "DamageRadiusInner" => Field::Num(2.0),
            "DamageRadiusOuter" => Field::Num(6.0),
            "DroidScale" => Field::Num(0.5),
            _ => Field::Undefined,
        });
        assert_eq!(e.damage_at(4.0, &Field::Str("Droid".into())), 100.0 * 0.5 * 0.5);
        assert_eq!(e.damage_at(1.0, &Field::Undefined), 100.0);
        assert_eq!(e.damage_at(7.0, &Field::Undefined), 0.0);
    }
}
