//! Hero power definitions read from one prepared weapon record.
//!
//! A record is the flattened weapon class with its `ordnance` object, as the
//! 2005 preparation writes it into `sides/<side>/units.json`. Values arrive as
//! text or numbers and are read once at admission; the fixed step only sees
//! the validated numbers below.

use crate::combat::combo::zero_fnv1a_lower;
use crate::combat::impacts::HealthScale;
use bfe_content::Value;
use std::fmt;

/// Fixed step rate of every power timer.
pub const TICKS_PER_SECOND: f64 = 60.0;

/// The 2005 game accepts any target within this distance whose bearing is
/// less than about 84 degrees off the cone axis, whatever the authored cone.
pub const NEAR_CONE_RADIUS: f64 = 2.5;
/// Cosine bound for targets inside [`NEAR_CONE_RADIUS`].
pub const NEAR_CONE_COS: f64 = 0.1;
/// An area weapon holds at most four targets.
pub const MAX_AREA_TARGETS: u8 = 4;

/// Why a weapon record could not become a power.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerDefError {
    pub weapon: String,
    pub message: &'static str,
}

impl fmt::Display for PowerDefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "power {}: {}", self.weapon, self.message)
    }
}

impl std::error::Error for PowerDefError {}

/// When a press becomes a cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trigger {
    /// `TriggerSingle`: one cast per press. Otherwise casts repeat while held.
    pub single: bool,
    /// `InitialSalvoDelay` in ticks between the press and the cast.
    pub windup: u32,
    /// `ShotDelay` in ticks after a cast before the next one can begin.
    pub recovery: u32,
}

/// What a cast spends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cost {
    /// `EnergyDrain`, charged through the soldier's stamina latch.
    pub energy: f64,
    /// An area weapon pays on the press; an emitter pays when the cast fires.
    pub on_press: bool,
    /// `EnergyDrainRate` per second while an area weapon holds a target.
    pub rate: f64,
    /// Casts available in one life when the weapon spends rounds, not energy.
    pub rounds: Option<u16>,
}

/// Which relations an effect accepts. Neutral targets count as friends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relations {
    pub friends: bool,
    pub enemies: bool,
}

impl Relations {
    pub fn accepts(self, enemy: bool) -> bool {
        if enemy {
            self.enemies
        } else {
            self.friends
        }
    }
}

/// An emitter that strikes along a cone from the caster (push, pull, lightning).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    pub length: f64,
    /// Cosine of half of `ConeAngle`.
    pub cos_half: f64,
    pub max_targets: u8,
    /// `Damage`, at no charge.
    pub damage: f64,
    /// `MaxDamage`, at full charge.
    pub max_damage: f64,
    /// `DamageThreshold`: the emission stops below this damage per target.
    pub threshold: f64,
    /// Signed `Push`: positive throws away from the caster, negative pulls in.
    pub push: f64,
    pub relations: Relations,
    /// `PersonScale`, `AnimalScale`, `DroidScale`, `VehicleScale`, `BuildingScale`.
    pub scales: [f64; 5],
}

impl Cone {
    /// Damage dealt to each struck target out of `count`: the charged amount
    /// is shared among them but never falls below the uncharged `Damage`.
    pub fn damage_per_target(&self, charge: f64, count: usize) -> f64 {
        let charged = self.damage + (self.max_damage - self.damage) * charge;
        if count == 0 {
            return charged;
        }
        v8math::max(charged / count as f64, self.damage)
    }
}

/// A timed modifier given to every accepted soldier in range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Buff {
    pub seconds: f64,
    pub value: f64,
}

/// An emitter that affects soldiers around the caster (the aura abilities).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aura {
    pub radius: f64,
    pub relations: Relations,
    /// `ApplyOnOwnerIfOnOthers`: the caster shares the effect when it reached anyone.
    pub owner_with_others: bool,
    /// `BuffOffenseTimer` and `BuffOffenseMult`: damage dealt is multiplied.
    pub offense: Option<Buff>,
    /// `BuffDefenseTimer` and `BuffDefenseMult`: damage taken is multiplied.
    pub defense: Option<Buff>,
    /// `BuffHealthTimer` and `BuffHealthRate`: health restored per second.
    pub regen: Option<Buff>,
}

/// What an area weapon does to a held target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaEffect {
    /// The target is held in place, choking, and hurt for as long as it is held.
    Choke,
}

/// An area weapon that acquires targets along the caster's aim and holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub effect: AreaEffect,
    /// `AreaRange`: reach along the aim.
    pub range: f64,
    /// `AreaRadius` is a half angle, in degrees, for an aimed ray.
    pub cos_half: f64,
    pub sin_half: f64,
    /// `AreaHeight`: thickness of the slab around the aim plane.
    pub height: f64,
    pub max_targets: u8,
    /// `EffectDamage` per second while held.
    pub damage: f64,
    /// `EffectStrength`; a choke needs a positive strength to hold its target.
    pub strength: f64,
    /// `ThrustFactor`: the caster's movement scale while holding.
    pub thrust: f64,
}

/// An explosion a combo state sets off where the character stands
/// (`PlayExplosion` with the blade's `ExplosionName`), such as a landing slam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub damage: f64,
    pub inner: f64,
    pub outer: f64,
    pub push: f64,
    pub push_inner: f64,
    pub push_outer: f64,
    /// `HurtOwner`: whether the character's own blast reaches it.
    pub hurt_owner: bool,
    /// `PersonScale`, `AnimalScale`, `DroidScale`, `VehicleScale`, `BuildingScale`.
    pub scales: [f64; 5],
}

impl Blast {
    /// Read a prepared explosion record.
    pub fn from_record(name: &str, record: &Value) -> Result<Self, PowerDefError> {
        let reader = Reader { name };
        let radius = reader.nonnegative(reader.number(record, "DamageRadius", Some(0.0))?)?;
        let push_radius = reader.nonnegative(reader.number(record, "PushRadius", Some(0.0))?)?;
        Ok(Blast {
            damage: reader.nonnegative(reader.number(record, "Damage", Some(0.0))?)?,
            inner: reader.nonnegative(reader.number(record, "DamageRadiusInner", Some(radius))?)?,
            outer: reader.nonnegative(reader.number(record, "DamageRadiusOuter", Some(radius))?)?,
            push: reader.nonnegative(reader.number(record, "Push", Some(0.0))?)?,
            push_inner: reader.nonnegative(reader.number(record, "PushRadiusInner", Some(push_radius))?)?,
            push_outer: reader.nonnegative(reader.number(record, "PushRadiusOuter", Some(push_radius))?)?,
            hurt_owner: reader.flag(record, "HurtOwner", true)?,
            scales: scales(&reader, record)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Cone(Cone),
    Aura(Aura),
    Area(Area),
}

/// One equipped power, ready for the fixed step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerDef {
    /// Case-folded FNV-1a of the weapon class, as combo bindings name weapons.
    pub weapon: u32,
    pub trigger: Trigger,
    pub cost: Cost,
    pub effect: Effect,
    /// `LockOnRange`: reach of an on-foot lock while this power is selected.
    pub lock_range: f64,
    /// `FireAnim`: offhand animation slot (0 = FIRE, 1 = FIRE2).
    pub fire_anim: u8,
    /// `InstantPlayFireAnim`: start the caster animation on press instead of at emission.
    pub instant_anim: bool,
    /// CRC of `human_{SoldierAnimation}` for area victims (0 when unset).
    pub soldier_anim: u32,
}

impl PowerDef {
    /// Health category scale for damage this power deals.
    pub fn scale(&self, health: HealthScale) -> f64 {
        match self.effect {
            Effect::Cone(cone) => cone.scales[health as usize],
            Effect::Aura(_) | Effect::Area(_) => 1.0,
        }
    }
}

struct Reader<'a> {
    name: &'a str,
}

impl Reader<'_> {
    fn error(&self, message: &'static str) -> PowerDefError {
        PowerDefError { weapon: self.name.to_owned(), message }
    }

    fn number(&self, record: &Value, key: &str, default: Option<f64>) -> Result<f64, PowerDefError> {
        let Some(value) = record.get(key) else {
            return default.ok_or_else(|| self.error("missing a required number"));
        };
        let parsed = match value {
            Value::Number(n) => Some(*n),
            Value::String(text) => text.split_ascii_whitespace().next().and_then(|t| t.parse::<f64>().ok()),
            _ => None,
        };
        parsed.filter(|n| n.is_finite()).ok_or_else(|| self.error("an unreadable number"))
    }

    fn flag(&self, record: &Value, key: &str, default: bool) -> Result<bool, PowerDefError> {
        Ok(self.number(record, key, Some(if default { 1.0 } else { 0.0 }))? != 0.0)
    }

    fn word<'v>(&self, record: &'v Value, key: &str) -> Option<&'v str> {
        record.get(key).and_then(Value::as_str).map(str::trim)
    }

    fn ticks(&self, record: &Value, key: &str) -> Result<u32, PowerDefError> {
        let seconds = self.number(record, key, Some(0.0))?;
        if seconds < 0.0 || seconds > 600.0 {
            return Err(self.error("a timer outside 0 to 600 seconds"));
        }
        Ok(v8math::ceil(seconds * TICKS_PER_SECOND - 1e-9) as u32)
    }

    fn nonnegative(&self, value: f64) -> Result<f64, PowerDefError> {
        if value < 0.0 {
            return Err(self.error("a negative amount"));
        }
        Ok(value)
    }
}

fn cos_sin(degrees: f64) -> (f64, f64) {
    let radians = degrees * std::f64::consts::PI / 180.0;
    (v8math::cos(radians), v8math::sin(radians))
}

fn scales(reader: &Reader<'_>, ord: &Value) -> Result<[f64; 5], PowerDefError> {
    let mut out = [1.0; 5];
    for (slot, key) in out.iter_mut().zip(["PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale"]) {
        *slot = reader.nonnegative(reader.number(ord, key, Some(1.0))?)?;
    }
    Ok(out)
}

fn buff(reader: &Reader<'_>, ord: &Value, timer: &str, value: &str) -> Result<Option<Buff>, PowerDefError> {
    let seconds = reader.nonnegative(reader.number(ord, timer, Some(0.0))?)?;
    if seconds <= 0.0 {
        return Ok(None);
    }
    let value = reader.nonnegative(reader.number(ord, value, Some(1.0))?)?;
    Ok(Some(Buff { seconds, value }))
}

fn emitter(reader: &Reader<'_>, ord: &Value) -> Result<Effect, PowerDefError> {
    // An emitter affects friends and enemies unless a key turns either off.
    let relations = Relations {
        friends: reader.flag(ord, "AffectFriends", true)?,
        enemies: reader.flag(ord, "AffectEnemies", true)?,
    };
    let length = reader.nonnegative(reader.number(ord, "ConeLength", Some(0.0))?)?;
    let radius = reader.nonnegative(reader.number(ord, "Radius", Some(0.0))?)?;
    if length > 0.0 {
        if !reader.flag(ord, "NoChaining", false)? {
            return Err(reader.error("a chaining emitter is not a hero power"));
        }
        let angle = reader.number(ord, "ConeAngle", Some(0.0))?;
        if !(angle > 0.0 && angle <= 360.0) {
            return Err(reader.error("a cone angle outside 0 to 360 degrees"));
        }
        let max_targets = reader.number(ord, "MaxTargets", Some(1.0))?;
        if !(1.0..=255.0).contains(&max_targets) {
            return Err(reader.error("a target count outside 1 to 255"));
        }
        let damage = reader.nonnegative(reader.number(ord, "Damage", Some(0.0))?)?;
        let max_damage = reader.nonnegative(reader.number(ord, "MaxDamage", Some(damage))?)?;
        return Ok(Effect::Cone(Cone {
            length,
            cos_half: cos_sin(0.5 * angle).0,
            max_targets: max_targets as u8,
            damage,
            max_damage,
            threshold: reader.number(ord, "DamageThreshold", Some(0.0))?,
            push: reader.number(ord, "Push", Some(0.0))?,
            relations,
            scales: scales(reader, ord)?,
        }));
    }
    if radius > 0.0 {
        let aura = Aura {
            radius,
            relations,
            owner_with_others: reader.flag(ord, "ApplyOnOwnerIfOnOthers", false)?,
            offense: buff(reader, ord, "BuffOffenseTimer", "BuffOffenseMult")?,
            defense: buff(reader, ord, "BuffDefenseTimer", "BuffDefenseMult")?,
            regen: buff(reader, ord, "BuffHealthTimer", "BuffHealthRate")?,
        };
        if aura.offense.is_none() && aura.defense.is_none() && aura.regen.is_none() {
            return Err(reader.error("an area emitter without a timed effect"));
        }
        return Ok(Effect::Aura(aura));
    }
    Err(reader.error("an emitter with neither a cone nor a radius"))
}

fn area(reader: &Reader<'_>, weapon: &Value) -> Result<Effect, PowerDefError> {
    match reader.word(weapon, "EffectType") {
        Some(t) if t.eq_ignore_ascii_case("Choke") => {}
        _ => return Err(reader.error("an area effect other than a choke")),
    }
    match reader.word(weapon, "AreaType") {
        Some(t) if t.eq_ignore_ascii_case("Ray") => {}
        _ => return Err(reader.error("an area shape other than an aimed ray")),
    }
    match reader.word(weapon, "FireType") {
        Some(t) if t.eq_ignore_ascii_case("Hold") => {}
        _ => return Err(reader.error("an area weapon that is not held")),
    }
    let degrees = reader.number(weapon, "AreaRadius", Some(0.0))?;
    if !(0.0..90.0).contains(&degrees) {
        return Err(reader.error("an aimed ray half angle outside 0 to 90 degrees"));
    }
    let (cos_half, sin_half) = cos_sin(degrees);
    let max_targets = reader.number(weapon, "MaxTargets", Some(1.0))?;
    if max_targets < 1.0 {
        return Err(reader.error("an area weapon without targets"));
    }
    Ok(Effect::Area(Area {
        effect: AreaEffect::Choke,
        range: reader.nonnegative(reader.number(weapon, "AreaRange", None)?)?,
        cos_half,
        sin_half,
        height: reader.nonnegative(reader.number(weapon, "AreaHeight", Some(0.0))?)?,
        // The 2005 game keeps at most four held targets whatever the record says.
        max_targets: v8math::min(max_targets, f64::from(MAX_AREA_TARGETS)) as u8,
        damage: reader.nonnegative(reader.number(weapon, "EffectDamage", Some(0.0))?)?,
        strength: reader.nonnegative(reader.number(weapon, "EffectStrength", Some(0.0))?)?,
        thrust: reader.nonnegative(reader.number(weapon, "ThrustFactor", Some(1.0))?)?,
    }))
}

/// `LockOnRange` of any weapon record, 0 when it has none.
pub fn lock_range(weapon: &Value) -> Result<f64, PowerDefError> {
    let reader = Reader { name: weapon.get("class").and_then(Value::as_str).unwrap_or("") };
    reader.nonnegative(reader.number(weapon, "LockOnRange", Some(0.0))?)
}

/// Read one weapon record. `Ok(None)` means the weapon is not a hero power
/// this module runs (a gun, a blade, a thrown blade, a dispenser, or any
/// weapon on the primary trigger).
pub fn from_weapon(weapon: &Value) -> Result<Option<PowerDef>, PowerDefError> {
    let name = weapon.get("class").and_then(Value::as_str).unwrap_or("");
    let reader = Reader { name };
    // Powers are cast with the offhand trigger. An emitter on the primary
    // trigger, such as a flamethrower, stays with the gun code.
    if reader.number(weapon, "channel", Some(0.0))? != 1.0 {
        return Ok(None);
    }
    let base = weapon.get("base").and_then(Value::as_str).unwrap_or("");
    let ord = weapon.get("ordnance");
    let ord_base = ord.and_then(|o| o.get("base")).and_then(Value::as_str).unwrap_or("");
    let (effect, on_press) = if base.eq_ignore_ascii_case("areaeffectweapon") {
        (area(&reader, weapon)?, true)
    } else if base.eq_ignore_ascii_case("cannon") && ord_base.eq_ignore_ascii_case("emitterordnance") {
        let Some(ord) = ord else { return Ok(None) };
        (emitter(&reader, ord)?, false)
    } else {
        return Ok(None);
    };
    if name.is_empty() {
        return Err(reader.error("a weapon record without a class"));
    }
    let energy = reader.nonnegative(reader.number(weapon, "EnergyDrain", Some(0.0))?)?;
    let rate = reader.nonnegative(reader.number(weapon, "EnergyDrainRate", Some(0.0))?)?;
    // A weapon that spends rounds instead of energy carries its prepared
    // allowance; an energy weapon reloads on its own.
    let rounds = if energy == 0.0 && reader.number(weapon, "RoundsPerClip", Some(0.0))? > 0.0 {
        let ammo = reader.number(weapon, "ammo", Some(0.0))?;
        if !(0.0..=1000.0).contains(&ammo) {
            return Err(reader.error("a round allowance outside 0 to 1000"));
        }
        Some(ammo as u16)
    } else {
        None
    };
    let trigger = Trigger {
        single: reader.flag(weapon, "TriggerSingle", true)?,
        windup: reader.ticks(weapon, "InitialSalvoDelay")?,
        recovery: reader.ticks(weapon, "ShotDelay")?,
    };
    let fire_anim = v8math::min(
        v8math::max(reader.number(weapon, "FireAnim", Some(0.0))?, 0.0),
        3.0,
    ) as u8;
    let instant_anim = reader.flag(weapon, "InstantPlayFireAnim", false)?;
    let soldier_anim = reader
        .word(weapon, "SoldierAnimation")
        .filter(|s| !s.is_empty())
        .map_or(0, |s| bfe_content::zero_hash::crc(&format!("human_{s}")));
    Ok(Some(PowerDef {
        weapon: zero_fnv1a_lower(name),
        trigger,
        cost: Cost { energy, on_press, rate, rounds },
        effect,
        lock_range: reader.nonnegative(reader.number(weapon, "LockOnRange", Some(0.0))?)?,
        fire_anim,
        instant_anim,
        soldier_anim,
    }))
}
