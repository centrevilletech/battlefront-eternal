//! 2005 (`GameFamily::Swbf2`) space building attachments, indestructible
//! proximity interior doors, self-piloted auto-turrets, and remote turret
//! chairs.
//!
//! Canonical health, alive state, team, `[built, destroyed, unbuilt]` collision
//! looks, and `Event::ObjectKilled` for every destructible building, turret,
//! powerup station, command post, prop, and remote terminal live exclusively in
//! `f3_props` / `f4` (ONE owner, zero parallel health copy). This module stores
//! extra 2005-only state (visibility, lock-on eligibility,
//! AI damage floor, auto/remote turrets, remote chair rings, animated hull
//! links, and death explosions) in `SpaceBuildingAttachment` keyed by canonical
//! `building_index`, and owns indestructible interior proximity doors
//! (`SpaceDoor`, no health) separately.

use bfe_wide::Wide;
use super::SpaceOwner;
use super::digest::Fnv;
use super::combat::{
    apply_weapon_spread, SpaceDamageCategory, SpaceExplosionSpec, SpaceOrdnancePool, SpaceProjectile,
    SpaceRng, PROVISIONAL_WARMUP_DECAY_RATE,
};
use super::flight::{
    parse_explosion, parse_space_weapon, SpaceLockTargetKind, SpacePilotId, SpaceWeaponSpec, EXPIRE_BLAST, IMPACT_BLASTS,
};
use crate::battle::props;
use crate::collision::{CollisionOwner, Ray, MISS};
use crate::gen::F4Handle;
use bfe_content::map::PlacedObject;
use bfe_content::vehicle_pack::Swbf2VehiclePack;
use bfe_content::{Object, Value};
use std::cmp::Ordering;
use std::rc::Rc;

/// Admits 2005 (`GameFamily::Swbf2`) placed prop instances into canonical
/// `f3_props` (`live_sources`), covering all destructible ship systems,
/// turrets, powerup stations, command posts, props, and remote chairs while
/// leaving doors (`base == "door"`) as indestructible space proximity colliders.
pub fn swbf2_live_prop_sources(battle: &bfe_content::battle::Battle) -> Vec<usize> {
    let Some(data) = battle.props.as_ref() else {
        return Vec::new();
    };
    let Some(instances) = data.get("instances").and_then(Value::as_array) else {
        return Vec::new();
    };
    instances
        .iter()
        .enumerate()
        .filter_map(|(index, desc)| {
            let base = desc.get("base")?.as_str()?;
            let model = desc.get("model")?.as_str()?;
            let live = matches!(
                base,
                "trap"
                    | "powerupstation"
                    | "armedbuilding"
                    | "commandpost"
                    | "destructablebuilding"
                    | "prop"
                    | "remoteterminal"
            );
            (live && data.get("models")?.get(model)?.truthy()).then_some(index)
        })
        .collect()
}

const PI: f64 = std::f64::consts::PI;

/// Provisional fallback proximity trigger radius for interior doors when
/// `AnimationTrigger` does not specify a numeric distance (metres).
pub const PROVISIONAL_DOOR_TRIGGER_RADIUS: f64 = 4.0;

/// Provisional fallback bounding radius for placed objects that lack both a
/// collision body AABB and an authored `Radius` property (metres).
pub const PROVISIONAL_OBJECT_DEFAULT_RADIUS: f64 = 4.0;

/// Very large mission-assigned maximum health disables repair.
pub const UNREPAIRABLE_MAX_HEALTH_THRESHOLD: f64 = 9.0e10;

/// A long respawn delay disables automatic turret regeneration.
pub const NO_AUTO_RESPAWN_THRESHOLD: f64 = 1e5;

/// Provisional rate-unit split for 2005 turret `TurnRate` / `PitchRate`:
/// authored values strictly greater than this threshold (e.g. `90.0` on
/// auto-turrets) are interpreted as degrees/second and converted to rad/s;
/// smaller values (e.g. `1.5` on remote turrets or `0.2` on heavy turrets) are
/// interpreted as rad/s directly. Tune once measured against the 2005 game.
pub const PROVISIONAL_TURRET_DEG_THRESHOLD: f64 = 10.0;

/// Provisional fallback turret slew rate when neither `TurnRate` nor
/// `MaxTurnSpeed` is authored (rad/s).
pub const PROVISIONAL_TURRET_DEFAULT_RATE_RAD: f64 = 1.5;

/// Provisional barrel alignment cosine threshold required before a self-piloted
/// auto-turret opens fire.
pub const PROVISIONAL_TURRET_ALIGN_COS: f64 = 0.96;

/// Provisional fallback muzzle standoff past turret bounding radius when
/// `vehicles.json` does not supply fire points (metres).
pub const PROVISIONAL_TURRET_MUZZLE_PAD: f64 = 1.0;

/// Provisional target-center distance tolerance during turret LOS checks (m).
pub const PROVISIONAL_TURRET_LOS_TOL: f64 = 3.0;

/// Lightweight snapshot of a live soldier fed into the space layer each tick
/// for proximity doors, interior auto-turrets, and boarding blast damage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceSoldierTarget {
    pub unit: crate::store::Ref<crate::gen::Unit>,
    pub life: f64,
    pub center: [f64; 3],
    pub radius: f64,
    pub team: i32,
    pub is_human: bool,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

/// Turret control mode authored via `PilotType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SpaceTurretPilotType {
    #[default]
    Player = 0,
    SelfPiloted = 1,
    Remote = 2,
}

/// Target filter parsed from a turret weapon's `AITarget*` / `Target*` flags.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurretTargetFilter {
    pub target_person: bool,
    pub target_vehicle: bool,
    pub target_building: bool,
    pub min_range: f64,
    pub max_range: f64,
}

/// Targeting defaults when a turret class supplies no filters.
pub const PROVISIONAL_TURRET_TARGETS: TurretTargetFilter = TurretTargetFilter {
            target_person: false,
            target_vehicle: true,
            target_building: false,
            min_range: 10.0,
            max_range: 1000.0,
};

impl Default for TurretTargetFilter {
    fn default() -> Self { PROVISIONAL_TURRET_TARGETS }
}

/// Live weapon and aimer state for an `armedbuilding` or `defensegridturret`
/// with `PilotType = self` or `PilotType = remote`.
#[derive(Clone, Debug)]
pub struct SpaceTurretState {
    pub pilot_type: SpaceTurretPilotType,
    pub weapon: SpaceWeaponSpec,
    pub filter: TurretTargetFilter,
    pub yaw: f64,
    pub pitch: f64,
    pub prev_yaw: f64,
    pub prev_pitch: f64,
    pub yaw_min: f64,
    pub yaw_max: f64,
    pub pitch_min: f64,
    pub pitch_max: f64,
    pub turn_rate: f64,
    pub pitch_rate: f64,
    pub clock: super::combat::SpaceShotClock,
    pub barrage_remaining: u32,
    pub barrage_timer: f64,
    pub trigger_was_held: bool,
    pub warm_up: f64,
    pub heat: f64,
    pub overheated: bool,
    pub shots_fired: u32,
    /// Model-space turret muzzle offsets resolved from `vehicles.json`.
    pub muzzle_offsets: Vec<[f64; 3]>,
    /// Active soldier controlling this turret through a linked `remoteterminal`.
    pub remote_operator: Option<SpacePilotId>,
    pub pending_remote_aim: Option<[f64; 3]>,
    pub pending_remote_fire: bool,
    /// The spread scale the chair's bot hands the gun (`None` is 1; a player's
    /// gun takes no scale).
    pub pending_remote_spread_scale: Option<f64>,
    pub kick: super::combat::SpaceKick,
}

/// Linked remote turret control chair (`base == "remoteterminal"`).
#[derive(Clone, Debug, Default)]
pub struct SpaceTerminalState {
    pub remote_object_name: String,
    pub next_terminal_name: String,
    pub prev_terminal_name: String,
    /// Canonical `building_index` of the linked exterior turret.
    pub remote_turret: Option<usize>,
    /// Canonical `building_index` of the next linked chair in the ring.
    pub next_terminal: Option<usize>,
    /// Canonical `building_index` of the previous linked chair in the ring.
    pub prev_terminal: Option<usize>,
    pub occupant: Option<SpacePilotId>,
    /// Where the chair seats its operator, in its own frame (its
    /// `PilotPosition` node), when it shows him: a `PilotAnimation` and
    /// `IsPilotExposed`. The 2005 game puts the soldier there; shown, he can be
    /// hit there.
    pub seat: Option<[f64; 3]>,
    /// Where the operator stood when he took the chair: he leaves it there.
    pub feet: Option<[f64; 3]>,
}

/// A chair's `PilotPosition` node in its model's frame, from the vehicle pack.
fn chair_seat(pack: &Swbf2VehiclePack, class: &str) -> Option<[f64; 3]> {
    let class = pack.class(class)?;
    let point = class.points.iter().find(|p| p.property.eq_ignore_ascii_case("PilotPosition"))?;
    point.model_position().or_else(|| {
        let joint = pack.model(class.model.as_deref()?)?.skeleton.get(point.joint?)?;
        Some([joint.matrix[12], joint.matrix[13], joint.matrix[14]])
    })
}

use crate::units::f3_props::Shields;

/// Core canonical scalar state read from `f3_props` / `f4` for one building.
#[derive(Clone, Copy, Debug)]
pub struct BuildingCoreFacts {
    pub alive: bool,
    pub state: props::State,
    pub health: f64,
    pub max_health: f64,
    pub team: i32,
    pub collidable: bool,
    pub target_generation: u32,
}

impl From<(bool, props::State, f64, f64, i32, bool, u32)> for BuildingCoreFacts {
    fn from(t: (bool, props::State, f64, f64, i32, bool, u32)) -> Self {
        Self {
            alive: t.0,
            state: t.1,
            health: t.2,
            max_health: t.3,
            team: t.4,
            collidable: t.5,
            target_generation: t.6,
        }
    }
}

/// Space-layer attachment for one canonical `f3_props` building (aligned 1-to-1
/// with `f3_props` building index). Holds NO parallel health, alive, or team
/// copy (those live exclusively in canonical `f3_props` / `f4`) while owning
/// the extra 2005-only state (lock-on flag, visibility, AI damage
/// floor, damage category, turrets, remote chair rings, and animation links).
#[derive(Clone, Debug)]
pub struct SpaceBuildingAttachment {
    /// Canonical index in `f3_props` (`0..f3_props::count_in`).
    pub building_index: usize,
    /// Source instance index in `props.json["instances"]` / `placed`.
    pub instance: usize,
    pub name: String,
    pub class: String,
    pub base: String,
    /// Canonical `f4` handle for `ObjectHandle::Prop(owner)`.
    /// Collision kernel bodies for `[built, destroyed, unbuilt]`.
    pub bodies: [Option<u32>; 3],
    pub animated_index: Option<usize>,
    pub matrix: [f64; 16],
    pub position: [f64; 3],
    pub center: [f64; 3],
    pub local_center_offset: [f64; 3],
    pub radius: f64,
    /// Its built model's bounding sphere (local centre and radius), which
    /// blasts are measured to (`None`: the sphere around its body).
    pub sphere: Option<[f64; 4]>,
    pub visible: bool,
    pub lock_on_enabled: bool,
    pub ai_damage_threshold: f64,
    pub damage_category: SpaceDamageCategory,
    pub death_explosion: Option<SpaceExplosionSpec>,
    /// Previous tick's canonical alive state for respawn edge detection.
    pub last_life: Option<f64>,
    pub turret: Option<SpaceTurretState>,
    pub terminal: Option<SpaceTerminalState>,
}

impl SpaceBuildingAttachment {
    /// Updates live transform from a world-animated prop matrix (such as an
    /// orbiting frigate or a frigate-mounted turret).
    pub fn update_from_matrix(&mut self, matrix: [f64; 16]) {
        self.matrix = matrix;
        self.position = [matrix[12], matrix[13], matrix[14]];
        let o = self.local_center_offset;
        self.center = [
            matrix[0] * o[0] + matrix[4] * o[1] + matrix[8] * o[2] + matrix[12],
            matrix[1] * o[0] + matrix[5] * o[1] + matrix[9] * o[2] + matrix[13],
            matrix[2] * o[0] + matrix[6] * o[1] + matrix[10] * o[2] + matrix[14],
        ];
    }

    /// The centre and radius blasts are measured to.
    pub fn blast_sphere(&self) -> ([f64; 3], f64) {
        let Some([x, y, z, radius]) = self.sphere else {
            return (self.center, self.radius);
        };
        let m = &self.matrix;
        (std::array::from_fn(|k| m[k] * x + m[4 + k] * y + m[8 + k] * z + m[12 + k]), radius)
    }

    pub fn active_body(&self, facts: &BuildingCoreFacts) -> Option<u32> {
        if !facts.collidable {
            return None;
        }
        self.bodies[facts.state.index()]
    }

    /// Mixes attachment + canonical building state into an FNV-1a digest.
    pub fn mix_digest(&self, facts: Option<BuildingCoreFacts>, shields: Shields, hash: Wide) -> Wide {
        let mut m = Fnv::new(hash);
        if let Some(f) = facts {
            m.small(f.target_generation);
            m.small(f.alive);
            m.bits(f.health);
            m.signed(f.team);
            m.small(f.collidable);
        }
        m.bits(shields.current);
        m.bits(shields.maximum);
        m.bits(shields.recharge);
        m.small(self.visible);
        m.small(self.lock_on_enabled);
        m.bits(self.ai_damage_threshold);
        for who in [self.turret.as_ref().and_then(|t|t.remote_operator),self.terminal.as_ref().and_then(|t|t.occupant)] {
            m.small(who.is_some());
            if let Some(who) = who { m.small(who.owner.slot); m.small(who.owner.gen); }
        }
        m.option_bits(self.last_life);
        for &c in &self.position {
            m.bits(c);
        }
        if let Some(ref t) = self.turret {
            m.bits(t.yaw);
            m.bits(t.pitch);
            m.bits(t.prev_yaw);
            m.bits(t.prev_pitch);
            m.small(t.shots_fired);
            for n in [t.clock.cooldown,t.clock.wait,t.barrage_timer,t.heat,t.warm_up] { m.bits(n); }
            for flag in [t.overheated,t.pending_remote_fire] { m.small(flag); }
            m.small(t.barrage_remaining); m.small(t.clock.salvos);
            m.small(t.pending_remote_aim.is_some());
            if let Some(aim) = t.pending_remote_aim { for n in aim { m.bits(n); } }
            for n in [t.kick.value, t.kick.cool_down, t.pending_remote_spread_scale.unwrap_or(-1.0)] { m.bits(n); }
            m.small(t.kick.spun);
        }
        m.finish()
    }
}

/// Read-only snapshot of one canonical `f3_props` building joined with its
/// space attachment transform.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceBuildingSnapshot {
    pub building_index: usize,
    pub owner: F4Handle,
    pub generation: u32,
    pub alive: bool,
    pub health: f64,
    pub max_health: f64,
    pub cur_shield: f64,
    pub max_shield: f64,
    pub add_shield: f64,
    pub team: i32,
    pub collidable: bool,
    pub visible: bool,
    pub lock_on_enabled: bool,
    pub ai_damage_threshold: f64,
    pub position: [f64; 3],
}

/// Indestructible interior proximity door (`base == "door"`).
///
/// Doors carry NO health pool and never enter `f3_props`; they open when an
/// unlocked proximity check detects a nearby on-foot unit and toggle their
/// collision kernel body cleanly.
#[derive(Clone, Debug)]
pub struct SpaceDoor {
    pub id: usize,
    pub instance: usize,
    pub name: String,
    pub class: String,
    pub body: Option<u32>,
    pub matrix: [f64; 16],
    pub position: [f64; 3],
    pub center: [f64; 3],
    pub door_trigger_radius: f64,
    pub trigger_position: [f64; 3],
    pub opening_clip: Option<std::rc::Rc<bfe_content::clips::Clip>>,
    pub door_open: bool,
    pub prev_door_open: bool,
    pub previous_fraction: f64,
    pub open_fraction: f64,
    pub locked: bool,
    pub collidable: bool,
    pub visible: bool,
}

/// Read-only snapshot of an indestructible interior proximity door.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceDoorSnapshot {
    pub id: usize,
    pub instance: usize,
    pub door_open: bool,
    pub prev_door_open: bool,
    pub previous_fraction: f64,
    pub open_fraction: f64,
    pub locked: bool,
    pub collidable: bool,
    pub visible: bool,
    pub position: [f64; 3],
}

/// Provisional transition duration for a proximity door without a timing clip.
pub const PROVISIONAL_DOOR_TRAVEL_SECONDS: f64 = 0.4;

impl SpaceDoor {
    /// Shared door collision rule used by both the server and online clients:
    /// a collidable door blocks passage while locked or less than halfway open.
    #[inline]
    pub const fn is_blocking(collidable: bool, locked: bool, open_fraction: f64) -> bool {
        collidable && (locked || open_fraction < 0.5)
    }

    /// The moving shutter clears the opening halfway through its clip.
    pub fn passage_blocked(&self) -> bool { self.locked || self.open_fraction < 0.5 }

    pub fn snapshot(&self) -> SpaceDoorSnapshot {
        SpaceDoorSnapshot {
            id: self.id,
            instance: self.instance,
            door_open: self.door_open,
            prev_door_open: self.prev_door_open,
            previous_fraction: self.previous_fraction,
            open_fraction: self.open_fraction,
            locked: self.locked,
            collidable: Self::is_blocking(self.collidable, self.locked, self.open_fraction),
            visible: self.visible,
            position: self.position,
        }
    }

    /// Synchronizes this door's collision body enabled state across BOTH
    /// `collision` (raycasts) and `flags` (soldier movement).
    pub fn sync_collision(
        &self,
        collision: &mut CollisionOwner,
        collision_world: usize,
        flags: &mut [u8],
    ) {
        let Some(body) = self.body else {
            return;
        };
        let enabled = Self::is_blocking(self.collidable, self.locked, self.open_fraction);
        collision.enable_body(body, enabled);
        if let Some(order) = collision
            .state
            .worlds
            .get(collision_world)
            .and_then(|w| w.order.iter().position(|&s| s == body))
        {
            if let Some(slot) = flags.get_mut(order) {
                *slot = u8::from(enabled);
            }
        } else if let Some(slot) = flags.get_mut(body as usize) {
            *slot = u8::from(enabled);
        }
    }

    pub fn mix_digest(&self, hash: Wide) -> Wide {
        let mut m = Fnv::new(hash);
        m.bits(self.previous_fraction);
        m.bits(self.open_fraction);
        m.small(self.visible);
        m.small(self.door_open);
        m.small(self.prev_door_open);
        m.small(self.locked);
        m.small(self.collidable);
        m.finish()
    }
}

/// Zero-allocation case-insensitive comparison between a pre-lowercased ASCII
/// index key and a query slice.
#[inline]
pub fn cmp_lower_ascii(lower_key: &str, query: &str) -> Ordering {
    let mut a = lower_key.bytes();
    let mut b = query.bytes().map(|c| c.to_ascii_lowercase());
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) => match x.cmp(&y) {
                Ordering::Equal => continue,
                ord => return ord,
            },
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}

/// Builds a sorted lowercase script-name index for O(log N) zero-allocation
/// lookup.
pub fn build_name_index_from<'a>(
    items: impl Iterator<Item = (usize, &'a str)>,
) -> Vec<(String, usize)> {
    let mut index: Vec<(String, usize)> = items
        .filter(|(_, name)| !name.is_empty())
        .map(|(idx, name)| (name.to_ascii_lowercase(), idx))
        .collect();
    index.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    index
}

/// Zero-allocation script-name lookup in a sorted lowercase index.
pub fn lookup_by_name(index: &[(String, usize)], name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    index
        .binary_search_by(|(k, _)| cmp_lower_ascii(k, name))
        .ok()
        .map(|at| index[at].1)
}

pub(crate) fn is_door(object: &PlacedObject, classes: Option<&Value>) -> bool {
    if !object.base.is_empty() { return object.base.eq_ignore_ascii_case("door"); }
    class_entry_ci(classes, "entc", &object.class).and_then(|v| v.get("base"))
        .and_then(Value::as_str).is_some_and(|base| base.eq_ignore_ascii_case("door"))
}

fn class_entry_ci<'a>(map_classes: Option<&'a Value>, section: &str, name: &str) -> Option<&'a Value> {
    let table = map_classes?.get(section)?.as_object()?;
    if let Some(exact) = table.get(name) {
        return Some(exact);
    }
    for (k, v) in table.entries() {
        if k.eq_ignore_ascii_case(name) {
            return Some(v);
        }
    }
    None
}

fn class_prop_ci<'a>(entry: Option<&'a Value>, plain: &str, hex: &str) -> Option<&'a str> {
    let pairs = entry?.get("properties")?.as_array()?;
    pairs.iter().rev().find_map(|pair| {
        let arr = pair.as_array()?;
        let k = arr.first()?.as_str()?;
        let v = arr.get(1)?.as_str()?;
        (k.eq_ignore_ascii_case(plain) || (!hex.is_empty() && k.eq_ignore_ascii_case(hex)))
            .then_some(v)
    })
}

/// A map class and the classes it inherits through `base`, root first: the
/// 2005 game reads a class as its parent's values with its own keys after
/// them. Empty for a class the map does not carry.
fn class_chain<'a>(map_classes: Option<&'a Value>, section: &str, name: &str) -> Vec<&'a Value> {
    let mut chain: Vec<&Value> = Vec::new();
    let mut next = class_entry_ci(map_classes, section, name);
    while let Some(class) = next.filter(|c| chain.len() < 16 && !chain.iter().any(|seen| std::ptr::eq(*seen, *c))) {
        chain.push(class);
        next = class.get("base").and_then(Value::as_str).and_then(|base| class_entry_ci(map_classes, section, base));
    }
    chain.reverse();
    chain
}

/// A property of a class chain: the class's own last value, else its parent's.
fn chain_prop<'a>(chain: &[&'a Value], plain: &str, hex: &str) -> Option<&'a str> {
    chain.iter().rev().find_map(|class| class_prop_ci(Some(class), plain, hex))
}

/// A class chain as the preparation writes a unit's weapon or ordnance: the
/// engine class at its root as `base`, and the whole chain's properties in
/// order.
fn chain_record(name: &str, chain: &[&Value]) -> Option<Object> {
    let mut record = Object::new();
    record.set("class".into(), Value::str(name));
    record.set("base".into(), chain.first()?.get("base").cloned().unwrap_or(Value::Null));
    let properties = chain.iter().filter_map(|class| class.get("properties")?.as_array()).flatten().cloned().collect();
    record.set("properties".into(), Value::Array(Rc::new(properties)));
    Some(record)
}

fn placed_or_class_prop<'a>(
    placed: &'a PlacedObject,
    class: &[&'a Value],
    plain: &str,
    hex: &str,
) -> Option<&'a str> {
    placed
        .properties
        .iter()
        .rev()
        .find(|(k, _)| {
            k.eq_ignore_ascii_case(plain) || (!hex.is_empty() && k.eq_ignore_ascii_case(hex))
        })
        .map(|(_, v)| v.as_str())
        .or_else(|| chain_prop(class, plain, hex))
}

fn parse_f64(text: Option<&str>, fallback: f64) -> f64 {
    text.and_then(|s| s.split_whitespace().next())
        .and_then(|tok| tok.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(fallback)
}

fn parse_pair_f64(text: Option<&str>, fallback: (f64, f64)) -> (f64, f64) {
    let Some(s) = text else { return fallback };
    let mut it = s.split_whitespace();
    let a = it.next().and_then(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite());
    let b = it.next().and_then(|t| t.parse::<f64>().ok()).filter(|v| v.is_finite());
    match (a, b) {
        (Some(lo), Some(hi)) => (lo, hi),
        _ => fallback,
    }
}

fn parse_bool_flag(text: Option<&str>, fallback: bool) -> bool {
    let Some(s) = text else { return fallback };
    let t = s.trim();
    if t.eq_ignore_ascii_case("true") {
        return true;
    }
    if t.eq_ignore_ascii_case("false") {
        return false;
    }
    t.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map_or(fallback, |v| v != 0.0)
}

/// Converts a turret rate using `PROVISIONAL_TURRET_DEG_THRESHOLD`:
/// values strictly greater than the threshold are converted from deg/s to
/// rad/s; smaller values are kept as rad/s directly.
#[inline]
pub fn normalize_turret_rate_rad(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return PROVISIONAL_TURRET_DEFAULT_RATE_RAD;
    }
    if raw > PROVISIONAL_TURRET_DEG_THRESHOLD {
        raw * PI / 180.0
    } else {
        raw
    }
}

/// Parses an `expc` explosion class from `maps/<map>/classes.json`.
pub fn parse_map_class_explosion(
    map_classes: Option<&Value>,
    exp_name: &str,
) -> Option<SpaceExplosionSpec> {
    let record = chain_record(exp_name, &class_chain(map_classes, "expc", exp_name))?;
    Some(parse_explosion(&Value::Object(Rc::new(record))))
}

/// A guided rocket is a craft to the game: it goes up by its class's
/// `ExplosionDestruct`, or by its `ExplosionCritical` past its boost threshold.
/// A unit's prepared record names the two classes; the map carries them.
pub fn attach_rocket_blasts(weapons: &mut [SpaceWeaponSpec], def: &Value, map_classes: Option<&Value>) {
    let records = def.get("weapons").and_then(Value::as_array).unwrap_or(&[]);
    for (spec, record) in weapons.iter_mut().zip(records).filter(|(spec, _)| spec.guided_missile) {
        let blast = |plain: &str, hex: &str| {
            class_prop_ci(record.get("ordnance"), plain, hex).and_then(|name| parse_map_class_explosion(map_classes, name))
        };
        spec.blast = blast("ExplosionDestruct", "8a323f32").unwrap_or_default();
        spec.guided_blast_boost = blast("ExplosionCritical", "e04f08c1").unwrap_or(spec.blast);
    }
}

pub const PROVISIONAL_TURRET_YAW_LIMITS: (f64,f64) = (-180.0,180.0);
pub const PROVISIONAL_TURRET_PITCH_LIMITS: (f64,f64) = (-30.0,60.0);

/// Resolves a placed turret's weapon, ordnance and blast through the classes
/// they inherit, read as a craft's weapon is, and its target filter.
pub fn parse_map_class_weapon(
    map_classes: Option<&Value>,
    weapon_name: &str,
) -> Option<(SpaceWeaponSpec, TurretTargetFilter)> {
    let weapon = class_chain(map_classes, "wpnc", weapon_name);
    let mut record = chain_record(weapon_name, &weapon)?;
    let ordnance_name = chain_prop(&weapon, "OrdnanceName", "").unwrap_or("");
    let mut ordnance = class_chain(map_classes, "ordc", ordnance_name);
    if ordnance.is_empty() {
        ordnance = class_chain(map_classes, "entc", ordnance_name);
    }
    if let Some(mut shot) = chain_record(ordnance_name, &ordnance) {
        // The blasts it names are nested as the preparation nests a craft's.
        for key in IMPACT_BLASTS.into_iter().chain([EXPIRE_BLAST]) {
            let blast = chain_prop(&ordnance, key, "").and_then(|name| chain_record(name, &class_chain(map_classes, "expc", name)));
            if let Some(blast) = blast {
                shot.set(key.into(), Value::Object(Rc::new(blast)));
            }
        }
        record.set("ordnance".into(), Value::Object(Rc::new(shot)));
    }
    let spec = parse_space_weapon(&Value::Object(Rc::new(record)), 0);
    let flag = |ai: &str, plain: &str, fallback: bool| {
        parse_bool_flag(chain_prop(&weapon, ai, "").or_else(|| chain_prop(&weapon, plain, "")), fallback)
    };
    let fallback = PROVISIONAL_TURRET_TARGETS;
    let filter = TurretTargetFilter {
        target_person: flag("AITargetPerson", "TargetPerson", fallback.target_person),
        target_vehicle: flag("AITargetVehicle", "TargetVehicle", fallback.target_vehicle),
        target_building: flag("AITargetBuilding", "TargetBuilding", fallback.target_building),
        min_range: parse_f64(chain_prop(&weapon, "MinRange", ""), fallback.min_range),
        max_range: parse_f64(
            chain_prop(&weapon, "MaxRange", "").or_else(|| chain_prop(&ordnance, "Range", "")),
            fallback.max_range,
        ),
    };
    Some((spec, filter))
}

/// Built 2005 space building attachments and indestructible interior doors
/// returned by [`build_space_attachments_and_doors`].
#[derive(Default)]
pub struct BuiltSpaceObjects {
    pub buildings: Vec<SpaceBuildingAttachment>,
    pub initial_shields: Vec<(usize, Shields)>,
    pub building_name_index: Vec<(String, usize)>,
    pub doors: Vec<SpaceDoor>,
    pub door_name_index: Vec<(String, usize)>,
}

/// Constructs 1-to-1 space attachments for every canonical `f3_props` building
/// (owning 2005 shields, lock-on, AI threshold, visibility, and turrets) and
/// indestructible interior proximity doors (`base == "door"`).
#[allow(clippy::too_many_arguments)]
pub fn build_space_attachments_and_doors(
    placed: &[PlacedObject],
    building_sources: &[(usize, usize, [Option<u32>; 3])],
    map_classes: Option<&Value>,
    vehicle_pack: Option<&Swbf2VehiclePack>,
    prop_kernels: &[(usize, u8, u32)],
    animated_by_instance: &[(usize, usize)],
    collision: &CollisionOwner,
) -> BuiltSpaceObjects {
    let mut initial_shields = Vec::with_capacity(building_sources.len());
    let mut attachments = Vec::with_capacity(building_sources.len());

    for &(building_index, source_idx, bodies) in building_sources {
        let Some(obj) = placed.get(source_idx) else {
            continue;
        };
        let class = class_chain(map_classes, "entc", &obj.class);
        let eff_base = if !obj.base.is_empty() {
            obj.base.as_str()
        } else {
            class
                .first()
                .and_then(|root| root.get("base"))
                .and_then(Value::as_str)
                .unwrap_or("")
        };
        let is_terminal = eff_base.eq_ignore_ascii_case("remoteterminal");
        let animated_index = animated_by_instance
            .iter()
            .find(|&&(inst, _)| inst == obj.index)
            .map(|&(_, aidx)| aidx);

        let max_shield = v8math::max(
            0.0,
            parse_f64(placed_or_class_prop(obj, &class, "MaxShield", ""), 0.0),
        );
        let cur_shield = v8math::min(
            max_shield,
            v8math::max(
                0.0,
                parse_f64(placed_or_class_prop(obj, &class, "CurShield", ""), max_shield),
            ),
        );
        let add_shield = parse_f64(placed_or_class_prop(obj, &class, "AddShield", ""), 0.0);
        let ai_damage_threshold = parse_f64(
            placed_or_class_prop(obj, &class, "AIDamageThreshold", ""),
            0.0,
        );
        // Default lock_on_enabled = false unless explicitly authored in ODF
        // (`EnableBuildingLockOn = 1`); mission scripts enable lock-on via the
        // rules seam (`ObjectProperty::LockOn(true)`). No HP heuristic.
        let lock_on_enabled = parse_bool_flag(
            placed_or_class_prop(obj, &class, "EnableBuildingLockOn", ""),
            false,
        );
        let health_type = placed_or_class_prop(obj, &class, "HealthType", "");
        let damage_category = if health_type.is_some_and(|h| h.eq_ignore_ascii_case("vehicle")) {
            SpaceDamageCategory::Vehicle
        } else if health_type.is_some_and(|h| h.eq_ignore_ascii_case("droid")) {
            SpaceDamageCategory::Droid
        } else {
            SpaceDamageCategory::Building
        };
        initial_shields.push((building_index, Shields {
            current: cur_shield,
            maximum: max_shield,
            recharge: add_shield,
        }));

        let death_explosion = placed_or_class_prop(obj, &class, "ExplosionName", "")
            .and_then(|exp_name| parse_map_class_explosion(map_classes, exp_name));

        let turret = placed_or_class_prop(obj, &class, "PilotType", "").and_then(|pilot_str| {
            let pilot_type = if pilot_str.eq_ignore_ascii_case("self") {
                SpaceTurretPilotType::SelfPiloted
            } else if pilot_str.eq_ignore_ascii_case("remote") {
                SpaceTurretPilotType::Remote
            } else {
                return None;
            };
            let weapon_name =
                placed_or_class_prop(obj, &class, "WeaponName", "").unwrap_or("");
            let (weapon, filter) = parse_map_class_weapon(map_classes, weapon_name)?;
            let (yaw_min_deg, yaw_max_deg) = parse_pair_f64(
                placed_or_class_prop(obj, &class, "YawLimits", ""),
                PROVISIONAL_TURRET_YAW_LIMITS,
            );
            let (pitch_min_deg, pitch_max_deg) = parse_pair_f64(
                placed_or_class_prop(obj, &class, "PitchLimits", ""),
                PROVISIONAL_TURRET_PITCH_LIMITS,
            );
            let turn_raw = parse_f64(
                placed_or_class_prop(obj, &class, "TurnRate", "")
                    .or_else(|| placed_or_class_prop(obj, &class, "MaxTurnSpeed", "")),
                PROVISIONAL_TURRET_DEFAULT_RATE_RAD,
            );
            let pitch_raw = parse_f64(
                placed_or_class_prop(obj, &class, "PitchRate", "")
                    .or_else(|| placed_or_class_prop(obj, &class, "MaxPitchSpeed", ""))
                    .or_else(|| placed_or_class_prop(obj, &class, "MaxTurnSpeed", "")),
                PROVISIONAL_TURRET_DEFAULT_RATE_RAD,
            );
            let muzzle_offsets = vehicle_pack
                .and_then(|vp| vp.class(&obj.class))
                .map(|cls| cls.weapon_muzzles(weapon_name, None))
                .unwrap_or_default();
            Some(SpaceTurretState {
                pilot_type,
                weapon,
                filter,
                yaw: 0.0,
                pitch: 0.0,
                prev_yaw: 0.0,
                prev_pitch: 0.0,
                yaw_min: yaw_min_deg * PI / 180.0,
                yaw_max: yaw_max_deg * PI / 180.0,
                pitch_min: pitch_min_deg * PI / 180.0,
                pitch_max: pitch_max_deg * PI / 180.0,
                turn_rate: normalize_turret_rate_rad(turn_raw),
                pitch_rate: normalize_turret_rate_rad(pitch_raw),
                clock: Default::default(),
                barrage_remaining: 0,
                barrage_timer: 0.0,
                trigger_was_held: false,
                warm_up: 0.0,
                heat: 0.0,
                overheated: false,
                shots_fired: 0,
                muzzle_offsets,
                remote_operator: None,
                pending_remote_aim: None,
                pending_remote_fire: false,
                pending_remote_spread_scale: None,
                kick: Default::default(),
            })
        });

        let terminal = if is_terminal {
            let remote_object_name = placed_or_class_prop(
                obj,
                &class,
                "RemoteGameObject",
                "d50db89e",
            )
            .unwrap_or("")
            .to_string();
            let next_terminal_name = placed_or_class_prop(
                obj,
                &class,
                "NextLinkedTerminal",
                "b7bc02ad",
            )
            .unwrap_or("")
            .to_string();
            let prev_terminal_name = placed_or_class_prop(
                obj,
                &class,
                "PrevLinkedTerminal",
                "5b36f215",
            )
            .unwrap_or("")
            .to_string();
            let shown = placed_or_class_prop(obj, &class, "PilotAnimation", "6e4fc069").is_some_and(|a| !a.trim().is_empty())
                && parse_bool_flag(placed_or_class_prop(obj, &class, "IsPilotExposed", "2b60e217"), false);
            Some(SpaceTerminalState {
                remote_object_name,
                next_terminal_name,
                prev_terminal_name,
                remote_turret: None,
                next_terminal: None,
                prev_terminal: None,
                occupant: None,
                seat: vehicle_pack.filter(|_| shown).and_then(|pack| chair_seat(pack, &obj.class)),
                feet: None,
            })
        } else {
            None
        };

        let (center, radius) = bodies[0]
            .and_then(|b| collision.state.bodies.get(b as usize))
            .map(|body| {
                let bb = body.bounds;
                let c = [
                    0.5 * (bb[0] + bb[3]),
                    0.5 * (bb[1] + bb[4]),
                    0.5 * (bb[2] + bb[5]),
                ];
                let r = 0.5 * v8math::hypot3(bb[3] - bb[0], bb[4] - bb[1], bb[5] - bb[2]);
                (c, v8math::max(1.0, r))
            })
            .unwrap_or_else(|| {
                (
                    obj.position,
                    obj.radius.unwrap_or(PROVISIONAL_OBJECT_DEFAULT_RADIUS),
                )
            });

        let mut inv = three::Matrix4 { elements: obj.matrix };
        inv.invert();
        let e = &inv.elements;
        let local_center_offset = [
            e[0] * center[0] + e[4] * center[1] + e[8] * center[2] + e[12],
            e[1] * center[0] + e[5] * center[1] + e[9] * center[2] + e[13],
            e[2] * center[0] + e[6] * center[1] + e[10] * center[2] + e[14],
        ];

        attachments.push(SpaceBuildingAttachment {
            building_index,
            instance: obj.index,
            name: obj.name.clone(),
            class: obj.class.clone(),
            base: eff_base.to_string(),
            bodies,
            animated_index,
            matrix: obj.matrix,
            position: obj.position,
            center,
            local_center_offset,
            radius,
            sphere: None,
            visible: obj.visible,
            lock_on_enabled,
            ai_damage_threshold,
            damage_category,
            death_explosion,
            last_life: None,
            turret,
            terminal,
        });
    }

    let building_name_index = build_name_index_from(
        attachments
            .iter()
            .map(|a| (a.building_index, a.name.as_str())),
    );
    for att in &mut attachments {
        if let Some(term) = att.terminal.as_mut() {
            term.remote_turret = lookup_by_name(&building_name_index, &term.remote_object_name);
            term.next_terminal = lookup_by_name(&building_name_index, &term.next_terminal_name);
            term.prev_terminal = lookup_by_name(&building_name_index, &term.prev_terminal_name);
        }
    }

    // Indestructible proximity doors (`base == "door"`, NO health pool).
    let mut doors = Vec::new();
    for obj in placed {
        if !is_door(obj, map_classes) {
            continue;
        }
        let class = class_chain(map_classes, "entc", &obj.class);
        let body = prop_kernels
            .iter()
            .find(|&&(p_inst, look, _)| p_inst == obj.index && look == 0)
            .map(|&(_, _, k)| k);
        let center = body
            .and_then(|b| collision.state.bodies.get(b as usize))
            .map(|b| {
                let bb = b.bounds;
                [
                    0.5 * (bb[0] + bb[3]),
                    0.5 * (bb[1] + bb[4]),
                    0.5 * (bb[2] + bb[5]),
                ]
            })
            .unwrap_or(obj.position);
        let locked = parse_bool_flag(
            placed_or_class_prop(obj, &class, "IsLocked", ""),
            false,
        );
        let door_trigger_radius = obj.door_animation.as_ref()
            .map_or(PROVISIONAL_DOOR_TRIGGER_RADIUS, |binding| binding.trigger_radius);
        let id = doors.len();
        doors.push(SpaceDoor {
            id,
            instance: obj.index,
            name: obj.name.clone(),
            class: obj.class.clone(),
            body,
            matrix: obj.matrix,
            position: obj.position,
            center,
            trigger_position: obj.position, opening_clip: None,
            door_trigger_radius,
            door_open: false,
            prev_door_open: false,
            open_fraction: 0.0, previous_fraction: 0.0,
            locked,
            collidable: true,
            visible: obj.visible,
        });
    }
    let door_name_index = build_name_index_from(
        doors.iter().enumerate().map(|(i, d)| (i, d.name.as_str())),
    );

    BuiltSpaceObjects {
        buildings: attachments,
        initial_shields,
        building_name_index,
        doors,
        door_name_index,
    }
}

/// Computes a first-order velocity-leading unit aim direction from
/// `shooter_pos` to a moving target `(target_pos, target_vel)` for projectile
/// speed `projectile_speed`. Falls back to direct bearing for instant beams or
/// unreachable intercepts. Pure `v8math`.
pub fn lead_target_direction(
    shooter_pos: [f64; 3],
    target_pos: [f64; 3],
    target_vel: [f64; 3],
    projectile_speed: f64,
) -> Option<[f64; 3]> {
    let (rx, ry, rz, t) = intercept(shooter_pos, target_pos, target_vel, projectile_speed)?;
    let aim_x = rx + target_vel[0] * t;
    let aim_y = ry + target_vel[1] * t;
    let aim_z = rz + target_vel[2] * t;
    let len = v8math::hypot3(aim_x, aim_y, aim_z);
    (len > 1e-6).then_some([aim_x / len, aim_y / len, aim_z / len])
}

/// The point [`lead_target_direction`] aims at: where the target will be when
/// a round leaving now at `projectile_speed` meets it.
pub fn lead_target_point(
    shooter_pos: [f64; 3],
    target_pos: [f64; 3],
    target_vel: [f64; 3],
    projectile_speed: f64,
) -> Option<[f64; 3]> {
    let (.., t) = intercept(shooter_pos, target_pos, target_vel, projectile_speed)?;
    Some([target_pos[0] + target_vel[0] * t, target_pos[1] + target_vel[1] * t, target_pos[2] + target_vel[2] * t])
}

/// The line from the shooter to the target and the time, to first order, a round
/// takes to meet it (0 for a beam or a target at rest).
fn intercept(
    shooter_pos: [f64; 3],
    target_pos: [f64; 3],
    target_vel: [f64; 3],
    projectile_speed: f64,
) -> Option<(f64, f64, f64, f64)> {
    let rx = target_pos[0] - shooter_pos[0];
    let ry = target_pos[1] - shooter_pos[1];
    let rz = target_pos[2] - shooter_pos[2];
    let dist = v8math::hypot3(rx, ry, rz);
    if dist < 1e-6 {
        return None;
    }
    let speed = projectile_speed;
    let v_sq = target_vel[0] * target_vel[0]
        + target_vel[1] * target_vel[1]
        + target_vel[2] * target_vel[2];
    let t = if speed > 1.0 && v_sq > 1e-4 {
        let a = v_sq - speed * speed;
        let b = 2.0 * (rx * target_vel[0] + ry * target_vel[1] + rz * target_vel[2]);
        let c = dist * dist;
        if a.abs() < 1e-6 {
            if b < -1e-6 { -c / b } else { dist / speed }
        } else {
            let disc = b * b - 4.0 * a * c;
            if disc >= 0.0 {
                let s = disc.sqrt();
                let t1 = (-b - s) / (2.0 * a);
                let t2 = (-b + s) / (2.0 * a);
                if t1 > 0.0 && t2 > 0.0 {
                    v8math::min(t1, t2)
                } else if t1 > 0.0 {
                    t1
                } else if t2 > 0.0 {
                    t2
                } else {
                    dist / speed
                }
            } else {
                dist / speed
            }
        }
    } else {
        0.0
    };
    Some((rx, ry, rz, t))
}

/// Checks unobstructed line of sight from a turret to a candidate target.
pub fn has_turret_los(
    shooter_pos: [f64; 3],
    target_pos: [f64; 3],
    shooter_body: Option<u32>,
    target_body: Option<u32>,
    collision: &mut CollisionOwner,
    collision_world: usize,
) -> bool {
    let dx = target_pos[0] - shooter_pos[0];
    let dy = target_pos[1] - shooter_pos[1];
    let dz = target_pos[2] - shooter_pos[2];
    let dist = v8math::hypot3(dx, dy, dz);
    if dist <= PROVISIONAL_TURRET_LOS_TOL {
        return true;
    }
    let dir = [dx / dist, dy / dist, dz / dist];
    let ignore = shooter_body.map_or(-1, |b| b as i32);
    let hit = collision.ray_hit(
        collision_world as u32,
        Ray {
            ox: shooter_pos[0],
            oy: shooter_pos[1],
            oz: shooter_pos[2],
            dx: dir[0],
            dy: dir[1],
            dz: dir[2],
        },
        dist - 1.0,
        ignore,
    );
    match hit {
        None => true,
        Some((code, h)) => {
            if code == MISS || target_body == Some(code as u32) {
                true
            } else {
                h.distance >= dist - PROVISIONAL_TURRET_LOS_TOL
            }
        }
    }
}

/// Slews a turret's `(yaw, pitch)` toward world direction `world_dir` relative
/// to the turret's base matrix `matrix`, respecting yaw/pitch limits and turn
/// rates. Saves `prev_yaw`/`prev_pitch` for interpolation. Returns
/// `(aligned_within_cone, fire_world_dir)`.
pub fn aim_turret_toward(
    turret: &mut SpaceTurretState,
    matrix: &[f64; 16],
    world_dir: [f64; 3],
    dt: f64,
) -> (bool, [f64; 3]) {
    turret.prev_yaw = turret.yaw;
    turret.prev_pitch = turret.pitch;

    // Transform desired world direction into turret-local axes.
    let rx = matrix[0] * world_dir[0] + matrix[1] * world_dir[1] + matrix[2] * world_dir[2];
    let ry = matrix[4] * world_dir[0] + matrix[5] * world_dir[1] + matrix[6] * world_dir[2];
    let rz = matrix[8] * world_dir[0] + matrix[9] * world_dir[1] + matrix[10] * world_dir[2];
    let horiz = v8math::hypot(rx, rz);
    let desired_yaw = v8math::min(
        turret.yaw_max,
        v8math::max(turret.yaw_min, v8math::atan2(rx, rz)),
    );
    let desired_pitch = v8math::min(
        turret.pitch_max,
        v8math::max(turret.pitch_min, v8math::atan2(ry, v8math::max(1e-6, horiz))),
    );

    let mut dyaw = desired_yaw - turret.yaw;
    while dyaw > PI {
        dyaw -= 2.0 * PI;
    }
    while dyaw < -PI {
        dyaw += 2.0 * PI;
    }
    let max_yaw = turret.turn_rate * dt;
    let step_yaw = v8math::min(max_yaw, v8math::max(-max_yaw, dyaw));
    turret.yaw = v8math::min(
        turret.yaw_max,
        v8math::max(turret.yaw_min, turret.yaw + step_yaw),
    );

    let dpitch = desired_pitch - turret.pitch;
    let max_pitch = turret.pitch_rate * dt;
    let step_pitch = v8math::min(max_pitch, v8math::max(-max_pitch, dpitch));
    turret.pitch = v8math::min(
        turret.pitch_max,
        v8math::max(turret.pitch_min, turret.pitch + step_pitch),
    );

    let fire_dir = barrel_direction(turret, matrix);
    let dot = fire_dir[0] * world_dir[0] + fire_dir[1] * world_dir[1] + fire_dir[2] * world_dir[2];
    (dot >= PROVISIONAL_TURRET_ALIGN_COS, fire_dir)
}

/// The way a turret mounted at `matrix` points now.
pub fn barrel_direction(turret: &SpaceTurretState, matrix: &[f64; 16]) -> [f64; 3] {
    let cp = v8math::cos(turret.pitch);
    let sp = v8math::sin(turret.pitch);
    let sy = v8math::sin(turret.yaw);
    let cy = v8math::cos(turret.yaw);
    let lx = sy * cp;
    let ly = sp;
    let lz = cy * cp;
    let wx = matrix[0] * lx + matrix[4] * ly + matrix[8] * lz;
    let wy = matrix[1] * lx + matrix[5] * ly + matrix[9] * lz;
    let wz = matrix[2] * lx + matrix[6] * ly + matrix[10] * lz;
    let len = v8math::hypot3(wx, wy, wz);
    if len > 1e-6 {
        [wx / len, wy / len, wz / len]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Fires one turret tick into `pool` honoring `WarmUpTime`, its shot clock,
/// `BarrageMin`/`BarrageMax`/`BarrageDelay`, `ShotsPerSalvo`,
/// heat/overheat, the spread and its kick, and vehicle-pack turret fire points.
/// Zero allocations.
#[allow(clippy::too_many_arguments)]
pub fn step_turret_fire(
    turret: &mut SpaceTurretState,
    matrix: &[f64; 16],
    fallback_origin: [f64; 3],
    fire_dir: [f64; 3],
    turret_team: i32,
    turret_body: Option<u32>,
    emitter: Option<super::combat::EmittingBody>,
    owner_unit: Option<SpaceOwner>,
    owner_is_ai: bool,
    trigger: bool,
    dt: f64,
    rng: &mut SpaceRng,
    pool: &mut SpaceOrdnancePool,
) -> (u32, [f64; 3], [f64; 3]) {
    turret.barrage_timer = v8math::max(0.0, turret.barrage_timer - dt);
    if turret.heat > 0.0 {
        turret.heat = v8math::max(0.0, turret.heat - turret.weapon.heat_recover_rate * dt);
        if turret.overheated && turret.heat <= turret.weapon.heat_threshold {
            turret.overheated = false;
        }
    }
    turret.kick.tick(&turret.weapon, trigger, turret.warm_up, turret.clock.done(), turret.overheated, !owner_is_ai, dt);
    turret.trigger_was_held = trigger;

    if !trigger {
        turret.warm_up = v8math::max(0.0, turret.warm_up - dt * PROVISIONAL_WARMUP_DECAY_RATE);
    } else if turret.warm_up < turret.weapon.warm_up_time {
        turret.warm_up = v8math::min(turret.weapon.warm_up_time, turret.warm_up + dt);
    }

    let spec = &turret.weapon;
    // A barrage is how a bot works the trigger: a player's gun has none.
    let barrage = owner_is_ai && spec.barrage_max > 0;
    // Only a bot's skill scales its kick: a player's is always 1.
    let scale = if owner_is_ai { turret.pending_remote_spread_scale.filter(|s| s.is_finite()).unwrap_or(1.0) } else { 1.0 };
    let mut emitted = 0u32;
    let mut last_origin = fallback_origin;
    let mut last_dir = fire_dir;
    let mut left = dt;
    // The rounds still to go of the salvo in hand, which fires its
    // `shots_per_salvo` at once.
    let mut rounds = None;
    loop {
        if rounds == Some(0) {
            rounds = None;
            turret.kick.salvo(spec, scale);
            turret.clock.fired(spec, &mut left, rng);
            if barrage && turret.clock.salvos == 0 {
                turret.barrage_remaining = turret.barrage_remaining.saturating_sub(1);
                if turret.barrage_remaining == 0 && spec.barrage_delay > 0.0 {
                    turret.barrage_timer = spec.barrage_delay;
                }
            }
        }
        if rounds.is_none() {
            let may_start = trigger
                && turret.warm_up >= spec.warm_up_time
                && !turret.overheated
                && !(barrage && turret.barrage_timer > 0.0);
            if !turret.clock.due(spec, may_start, &mut left) {
                break;
            }
            if barrage && turret.barrage_remaining == 0 {
                turret.barrage_remaining = rng.next_range_u32(spec.barrage_min.max(1), spec.barrage_max);
            }
            rounds = Some(spec.shots_per_salvo.max(1));
        }
        // An overheated gun cuts the shot short; the salvo it cut kicks.
        if turret.overheated && spec.heat_per_shot > 0.0 {
            turret.clock.salvos = 1;
            rounds = Some(0);
            continue;
        }
        let barrel = turret.shots_fired as usize;
        turret.shots_fired = turret.shots_fired.wrapping_add(1);
        if spec.heat_per_shot > 0.0 {
            turret.heat = v8math::min(1.0, turret.heat + spec.heat_per_shot);
            if turret.heat >= 1.0 {
                turret.overheated = true;
            }
        }

        let origin = if !turret.muzzle_offsets.is_empty() {
            let local = turret.muzzle_offsets[barrel % turret.muzzle_offsets.len()];
            [
                matrix[0] * local[0] + matrix[4] * local[1] + matrix[8] * local[2] + matrix[12],
                matrix[1] * local[0] + matrix[5] * local[1] + matrix[9] * local[2] + matrix[13],
                matrix[2] * local[0] + matrix[6] * local[1] + matrix[10] * local[2] + matrix[14],
            ]
        } else {
            fallback_origin
        };
        // The spread, the gun's own and its kick, is drawn in the gun's frame,
        // square to its mounting's up.
        let (pitch_spread, yaw_spread) = turret.kick.spread(spec);
        let shot_dir = apply_weapon_spread(fire_dir, [matrix[4], matrix[5], matrix[6]], pitch_spread, yaw_spread, rng);
        last_origin = origin;
        last_dir = shot_dir;

        let proj = SpaceProjectile {
            generation: 0, previous_position: [0.0; 3],
            active: true,
            kind: spec.ordnance_kind,
            owner_unit,
            owner_team: turret_team,
            owner_is_ai,
            from_infantry: false,
            ignore_body: turret_body,
            pass_through: spec.pass_through,
            emitter,
            weapon_index: 0,
            position: origin,
            velocity: shot_dir.map(|v| v * spec.velocity),
            age: 0.0,
            life_span: spec.life_span,
            gravity: spec.gravity,
            turn_rate: spec.turn_rate,
            lock_kind: SpaceLockTargetKind::None,
            lock_target: 0,
            lock_generation: 0,
            is_tracer: spec.tracer(barrel as u32),
            base_damage: spec.max_damage,
            final_damage: spec.final_damage,
            damage_transition_delay: spec.damage_transition_delay,
            damage_transition_period: spec.damage_transition_period,
            scales: [
                spec.person_scale,
                spec.animal_scale,
                spec.droid_scale,
                spec.vehicle_scale,
                spec.building_scale,
            ],
            shield_scale: spec.shield_scale,
            guided_pilot_fighter: None,
            guided_damage: 0.0,
            guided_damage_boost: 0.0,
            guided_boost_threshold: 0.8,
            guided_cruise_speed: 0.0,
            guided_boost_speed: 0.0,
            guided_out_of_bounds: -1.0,
            blast: spec.blast,
            expire_blast: spec.expire_blast,
            guided_blast_boost: spec.guided_blast_boost,
        };
        if pool.spawn_weapon(proj, &spec.name, barrel.checked_rem(turret.muzzle_offsets.len())).is_some() {
            emitted += 1;
        }
        rounds = rounds.map(|n| n - 1);
    }

    (emitted, last_origin, last_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{Ray, MISS};
    use crate::space::flight::{install_bounds_box_tree, SpaceOrdnanceKind};
    use bfe_content::bvh::{build as build_bvh, Positions};
    use bfe_content::install::CollisionKernel;

    fn make_empty_world(coll: &mut CollisionOwner) -> usize {
        let empty_bvh = build_bvh(Positions::F32(&[]), &[]);
        let empty_pos = coll.positions(&[]);
        let empty_tree = coll.tree(empty_pos, &[], &empty_bvh);
        coll.world_new(empty_tree, -1, -1) as usize
    }

    fn make_box_body(
        coll: &mut CollisionOwner,
        world: usize,
        min: [f64; 3],
        max: [f64; 3],
        flags: &mut [u8],
        enabled: bool,
    ) -> u32 {
        let tree = install_bounds_box_tree(coll, min, max);
        let body = coll.body_new(tree, 0) as u32;
        coll.body_offset(body, (body as f64 + 1.0) * 16777216.0);
        let io = coll.io_mut();
        io[20..23].copy_from_slice(&min);
        io[23..26].copy_from_slice(&max);
        coll.body_set(body);
        coll.state.worlds[world].order.push(body);
        coll.state.worlds[world].version += 1;
        let order = coll.state.worlds[world].order.len() - 1;
        flags[order] = u8::from(enabled);
        coll.enable_body(body, enabled);
        body
    }

    #[test]
    fn indestructible_proximity_door_toggles_collision_and_respects_lock() {
        let mut coll = CollisionOwner::new();
        let world = make_empty_world(&mut coll);
        let mut flags = vec![0u8; 64];
        let body = make_box_body(&mut coll, world, [-2.0, 0.0, -0.5], [2.0, 3.0, 0.5], &mut flags, true);
        let mut door = SpaceDoor {
            id: 0,
            instance: 0,
            name: "bulkhead_door_1".into(),
            class: "synth_door".into(),
            body: Some(body),
            matrix: three::Matrix4::default().elements,
            position: [0.0; 3],
            center: [0.0, 1.5, 0.0],
            trigger_position: [0.;3], opening_clip: None,
            door_trigger_radius: 4.0,
            door_open: false,
            prev_door_open: false,
            open_fraction: 0.0, previous_fraction: 0.0,
            locked: false,
            collidable: true,
            visible: true,
        };
        let ray = Ray { ox: 0.0, oy: 1.5, oz: -5.0, dx: 0.0, dy: 0.0, dz: 1.0 };
        assert_eq!(coll.ray_hit(world as u32, ray, 10.0, -1).map(|(c, _)| c), Some(body as i32));

        door.door_open = true;
        door.open_fraction = 0.49;
        door.sync_collision(&mut coll, world, &mut flags);
        assert_eq!(flags[0], 1);
        door.open_fraction = 0.5;
        door.sync_collision(&mut coll, world, &mut flags);
        assert!(coll.ray_hit(world as u32, ray, 10.0, -1).is_none_or(|(c, _)| c == MISS));
        assert_eq!(flags[0], 0);

        door.locked = true;
        door.door_open = false;
        door.sync_collision(&mut coll, world, &mut flags);
        assert_eq!(coll.ray_hit(world as u32, ray, 10.0, -1).map(|(c, _)| c), Some(body as i32));
        assert_eq!(flags[0], 1);
    }

    /// An idle self-piloted gun firing the invented weapon `name` from `classes`.
    fn test_turret(classes: &Value, name: &str) -> SpaceTurretState {
        let (weapon, filter) = parse_map_class_weapon(Some(classes), name).expect("the invented gun parses");
        SpaceTurretState {
            pilot_type: SpaceTurretPilotType::SelfPiloted,
            weapon,
            filter,
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            yaw_min: -PI,
            yaw_max: PI,
            pitch_min: -0.5,
            pitch_max: 0.8,
            turn_rate: 2.0,
            pitch_rate: 2.0,
            clock: Default::default(),
            barrage_remaining: 0,
            barrage_timer: 0.0,
            trigger_was_held: false,
            warm_up: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
            muzzle_offsets: Vec::new(),
            remote_operator: None,
            pending_remote_aim: None,
            pending_remote_fire: false,
            pending_remote_spread_scale: None,
            kick: Default::default(),
        }
    }

    #[test]
    fn a_ship_gun_reads_every_field_through_its_parent_classes_as_a_craft_weapon_does() {
        // One invented weapon twice: as a craft's, whose record lists its
        // parents' keys before its own, and as a ship gun, whose class and
        // ordnance each hold their own keys and name their parent.
        let arm = r#"["ShotDelay","0.41"],["SalvoCount","7"],["ShotsPerSalvo","2"],["SalvoDelay","0.06 0.13"],["InitialSalvoDelay","0.17"],["BarrageMin","7"],["BarrageMax","9"],["BarrageDelay","1.3"],["AutoAimSize","0.7"],["RoundsPerClip","23"],["ReloadTime","2.6"],["HeatPerShot","0.07"],["HeatRecoverRate","0.31"],["HeatThreshold","0.44"],["WarmUpTime","0.9"],["CoolDownTime","1.7"],["PitchSpread","0.6"],["SpreadPerShot","0.33"],["SpreadLimit","1.9"],["SpreadRecoverRate","0.8"],["SpreadThreshold","0.35"],["SecondaryOrdnancePeriod","5"],["LockOnRange","310"],["LockTime","0.85"],["MinRange","13.5"],["OptimalRange","215"],["MaxRange","735"],["AITargetPerson","1"],["AITargetVehicle","0"],["AITargetBuilding","1"],["OrdnanceName","test_shot_root"]"#;
        let own = r#"["ShotDelay","0.29"],["YawSpread","1.3"],["OrdnanceName","test_shot"]"#;
        let root = r#"["Velocity","415"],["LifeSpan","2.7"],["Gravity","0.35"],["TurnRate","0.85"],["PassThrough","1"],["MaxDamage","61"],["DamageFinalDamage","24"],["DamageTransitionDelay","0.45"],["DamageTransitionPeriod","0.65"],["PersonScale","0.35"],["AnimalScale","0.45"],["DroidScale","0.55"],["VehicleScale","1.65"],["BuildingScale","0.15"],["ShieldScale","0.65"]"#;
        let mid = r#"["MaxDamage","67"],["Gravity","0.21"]"#;
        let shot = r#"["MaxDamage","83"]"#;
        let parse = |text: String| bfe_content::json::parse(&text).unwrap();
        let classes = parse(format!(
            r#"{{"wpnc":{{"test_arm":{{"base":"cannon","properties":[{arm}]}},"test_gun":{{"base":"Test_Arm","properties":[{own}]}}}},
                "ordc":{{"test_shot_root":{{"base":"missile","properties":[{root}]}},"test_shot_mid":{{"base":"test_shot_root","properties":[{mid}]}},"test_shot":{{"base":"test_shot_mid","properties":[{shot}]}}}}}}"#
        ));
        let craft = parse_space_weapon(
            &parse(format!(
                r#"{{"class":"test_gun","base":"cannon","properties":[{arm},{own}],"ordnance":{{"class":"test_shot","base":"missile","properties":[{root},{mid},{shot}]}}}}"#
            )),
            0,
        );
        let (ship, filter) = parse_map_class_weapon(Some(&classes), "test_gun").expect("the invented gun parses");
        assert_eq!(format!("{ship:?}"), format!("{craft:?}"));
        assert_eq!((ship.shot_delay, ship.max_damage, ship.gravity, ship.velocity), (0.29, 83.0, 0.21, 415.0));
        assert_eq!(
            filter,
            TurretTargetFilter { target_person: true, target_vehicle: false, target_building: true, min_range: 13.5, max_range: 735.0 }
        );
    }

    #[test]
    fn only_a_beam_passes_through_what_it_strikes() {
        let passes = |base: &str| {
            let json = format!(r#"{{"wpnc":{{"test_gun":{{"base":"cannon","properties":[["OrdnanceName","test_shot"]]}}}},"ordc":{{"test_shot":{{"base":"{base}","properties":[["PassThrough","1"]]}}}}}}"#);
            parse_map_class_weapon(Some(&bfe_content::json::parse(&json).unwrap()), "test_gun").unwrap().0.pass_through
        };
        assert_eq!(["bolt", "missile", "shell", "beam"].map(passes), [false, false, false, true]);
    }

    #[test]
    fn a_ship_beam_is_a_beam_through_its_parents_and_reaches_its_own_range() {
        // An invented beam two classes below the engine's: it reaches the
        // ordnance's range, and its gun looks for targets within the weapon's.
        let json = r#"{"wpnc":{"test_ray_arm":{"base":"cannon","properties":[["MaxRange","735"]]},"test_ray_gun":{"base":"test_ray_arm","properties":[["OrdnanceName","test_ray"]]}},
            "ordc":{"test_ray_root":{"base":"beam","properties":[["PassThrough","1"]]},"test_ray_mid":{"base":"test_ray_root","properties":[["Range","1130"]]},"test_ray":{"base":"test_ray_mid","properties":[]}}}"#;
        let (spec, filter) = parse_map_class_weapon(Some(&bfe_content::json::parse(json).unwrap()), "test_ray_gun").unwrap();
        assert_eq!((spec.ordnance_kind, spec.round_reach(), spec.pass_through), (SpaceOrdnanceKind::Beam, 1130.0, true));
        assert_eq!((spec.max_range, filter.max_range), (735.0, 735.0));
        assert_eq!(filter.target_vehicle, TurretTargetFilter::default().target_vehicle);
    }

    #[test]
    fn a_ship_guns_blast_reads_its_fields_through_its_parent_class() {
        // An invented blast whose parent gives its damage, inner radius and
        // building scale; the shot names it in its own parent.
        let json = r#"{"wpnc":{"test_gun":{"base":"cannon","properties":[["OrdnanceName","test_shot"]]}},
            "ordc":{"test_shot_root":{"base":"bolt","properties":[["ExplosionName","test_burst"]]},"test_shot":{"base":"test_shot_root","properties":[]}},
            "expc":{"test_burst_root":{"base":"explosion","properties":[["Damage","37"],["DamageRadiusInner","3.4"],["BuildingScale","0.15"]]},"test_burst":{"base":"test_burst_root","properties":[["DamageRadiusOuter","8.6"],["VehicleScale","1.65"]]}}}"#;
        let (spec, _) = parse_map_class_weapon(Some(&bfe_content::json::parse(json).unwrap()), "test_gun").unwrap();
        assert_eq!((spec.blast.damage, spec.blast.inner_radius, spec.blast.outer_radius), (37.0, 3.4, 8.6));
        assert_eq!((spec.blast.scales, spec.blast.shield_scale), ([1.0, 1.0, 1.0, 1.65, 0.15], 1.0));
    }

    #[test]
    fn a_ship_guns_blast_is_read_as_a_craft_weapons_is() {
        // Invented: a gun whose round inherits the name of its blast, which
        // inherits half its keys; the same records as a craft carries them.
        let parse = |text: String| bfe_content::json::parse(&text).unwrap();
        let root = r#"["Damage","370"],["DamageRadiusInner","3.4"],["ArmorScale","0.35"],["HealthScale","0.85"]"#;
        let own = r#"["DamageRadius","16.35"],["BuildingScale","0.55"]"#;
        let blast = |base: &str| {
            let classes = parse(format!(
                r#"{{"wpnc":{{"test_gun":{{"base":"cannon","properties":[["OrdnanceName","test_shot"]]}}}},
                    "expc":{{"test_burst_root":{{"base":"explosion","properties":[{root}]}},"test_burst":{{"base":"test_burst_root","properties":[{own}]}}}},
                    "ordc":{{"test_shot_root":{{"base":"{base}","properties":[["ExplosionName","test_burst"]]}},"test_shot":{{"base":"test_shot_root","properties":[["MaxDamage","83"]]}}}}}}"#
            ));
            let craft = parse(format!(
                r#"{{"class":"test_gun","base":"cannon","properties":[["OrdnanceName","test_shot"]],"ordnance":{{"class":"test_shot","base":"{base}","properties":[["ExplosionName","test_burst"],["MaxDamage","83"]],
                    "ExplosionName":{{"class":"test_burst","base":"explosion","properties":[{root},{own}]}}}}}}"#
            ));
            let ship = parse_map_class_weapon(Some(&classes), "test_gun").unwrap().0.blast;
            assert_eq!(ship, parse_space_weapon(&craft, 0).blast, "{base}");
            (ship.damage, ship.inner_radius, ship.outer_radius, ship.scales, ship.shield_scale)
        };
        assert_eq!(blast("missile"), (370.0, 3.4, 16.35, [0.85, 0.85, 0.85, 0.35, 0.55], 1.0));
        assert_eq!(blast("beam"), (0.0, 0.0, 0.0, [0.0; 5], 0.0));
    }

    #[test]
    fn a_guided_rocket_goes_up_by_its_crafts_blasts_the_critical_one_past_its_boost_threshold() {
        // An invented rocket whose class names its two blasts, one of them by
        // the hashed key the preparation leaves unnamed.
        let classes = bfe_content::json::parse(
            r#"{"expc":{"test_pop":{"base":"explosion","properties":[["Damage","23"],["DamageRadius","4.5"]]},"test_bang":{"base":"explosion","properties":[["Damage","410"],["DamageRadius","19.5"]]}}}"#,
        ).unwrap();
        let def = bfe_content::json::parse(
            r#"{"weapons":[{"class":"test_gun"},{"class":"test_remote","ordnance":{"class":"test_rocket","base":"flyer","properties":[["GuidedMissile","1"],["8a323f32","test_pop"],["e04f08c1","test_bang"]]}}]}"#,
        ).unwrap();
        let mut weapons: Vec<SpaceWeaponSpec> = def.get("weapons").and_then(Value::as_array).unwrap().iter().enumerate().map(|(i, w)| parse_space_weapon(w, i)).collect();
        attach_rocket_blasts(&mut weapons, &def, Some(&classes));
        assert_eq!((weapons[0].blast.damage, weapons[1].blast.damage, weapons[1].guided_blast_boost.damage), (0.0, 23.0, 410.0));
        let at = |speed: f64| {
            let rocket = SpaceProjectile {
                kind: weapons[1].ordnance_kind, velocity: [0.0, 0.0, speed], guided_boost_speed: 200.0, guided_boost_threshold: 0.75,
                blast: weapons[1].blast, guided_blast_boost: weapons[1].guided_blast_boost, ..Default::default()
            };
            rocket.impact_blast().damage
        };
        assert_eq!((at(140.0), at(160.0)), (23.0, 410.0));
    }

    #[test]
    fn a_placed_gun_reads_its_own_class_through_its_parents() {
        // An invented gun post whose parent class makes it self-piloted and
        // names its weapon and limits; its own class narrows its yaw and the
        // placed object its shield.
        let json = r#"{"entc":{"test_post_root":{"base":"armedbuilding","properties":[["PilotType","self"],["WeaponName","test_gun"],["YawLimits","-67 71"],["PitchLimits","-12 47"],["MaxShield","310"],["AddShield","9"]]},"test_post":{"base":"test_post_root","properties":[["YawLimits","-33 41"]]}},
            "wpnc":{"test_gun":{"base":"cannon","properties":[["ShotDelay","0.29"]]}}}"#;
        let placed = PlacedObject {
            index: 0,
            layer: None,
            name: "post".into(),
            class: "Test_Post".into(),
            base: "armedbuilding".into(),
            model: None,
            door_animation: None,
            destroyed_model: None,
            unbuilt_model: None,
            matrix: three::Matrix4::default().elements,
            position: [0.0; 3],
            team: 1,
            radius: None,
            collision: None,
            destroyed_collision: None,
            unbuilt_collision: None,
            visible: true,
            properties: vec![("MaxShield".into(), "260".into())],
            settings: Value::Null,
            raw: Value::Null,
        };
        let classes = bfe_content::json::parse(json).unwrap();
        let built = build_space_attachments_and_doors(&[placed], &[(0, 0, [None; 3])], Some(&classes), None, &[], &[], &CollisionOwner::new());
        let turret = built.buildings[0].turret.as_ref().expect("its parent's PilotType makes it a gun");
        let rad = |degrees: f64| degrees * PI / 180.0;
        assert_eq!(turret.weapon.shot_delay, 0.29);
        assert_eq!((turret.yaw_min, turret.yaw_max), (rad(-33.0), rad(41.0)));
        assert_eq!((turret.pitch_min, turret.pitch_max), (rad(-12.0), rad(47.0)));
        let shield = built.initial_shields[0].1;
        assert_eq!((shield.maximum, shield.recharge), (260.0, 9.0));
    }

    #[test]
    fn a_ship_guns_kick_comes_through_its_parent_rises_with_each_salvo_and_falls_at_rest() {
        // An invented gun whose parent gives the limit (1.4 degrees) and the
        // threshold; its own class gives the kick per salvo and the recovery.
        let json = r#"{"wpnc":{"test_kick_base":{"base":"cannon","properties":[["SpreadLimit","1.4"],["SpreadThreshold","0.8"]]},"test_kick_gun":{"base":"test_kick_base","properties":[["SpreadPerShot","0.5"],["SpreadRecoverRate","0.7"],["ShotDelay","0.13"]]}}}"#;
        let mut turret = test_turret(&bfe_content::json::parse(json).unwrap(), "test_kick_gun");
        let matrix = three::Matrix4::default().elements;
        let aim = [0.0, 0.0, 1.0];
        let mut rng = SpaceRng::new(Wide::from_u32(31));
        let mut run = |turret: &mut SpaceTurretState, ticks: u32, trigger: bool| {
            let mut shots = Vec::new();
            for _ in 0..ticks {
                let mut pool = SpaceOrdnancePool::new(4);
                let (fired, _, d) = step_turret_fire(turret, &matrix, [0.0, 2.0, 0.0], aim, 1, None, None, None, true, trigger, 1.0 / 60.0, &mut rng, &mut pool);
                if fired > 0 {
                    shots.push(d);
                }
            }
            shots
        };
        let deg = PI / 180.0;
        let burst = run(&mut turret, 120, true);
        assert!(burst.len() > 10);
        assert_eq!(burst[0], aim);
        assert_ne!(burst[1], aim);
        assert!(burst.iter().all(|d| v8math::acos(v8math::min(1.0, d[2])) <= 1.4 * deg * std::f64::consts::SQRT_2 + 1e-12));
        assert!(burst[4..].iter().any(|d| v8math::acos(v8math::min(1.0, d[2])) > 0.9 * deg), "the kick reaches its limit");
        // Two seconds at rest bring the bot's kick back to zero.
        run(&mut turret, 120, false);
        assert_eq!(run(&mut turret, 1, true), vec![aim]);
    }

    #[test]
    fn a_ship_guns_kick_falls_through_an_overheat_with_the_trigger_held() {
        // An invented ship gun that overheats after five shots and cools for
        // 1.3 s after it stops: its kick falls the same whether its trigger is
        // held through the overheat or let go.
        let json = r#"{"wpnc":{"test_hot_gun":{"base":"cannon","properties":[["SpreadPerShot","0.6"],["SpreadLimit","2.1"],["SpreadRecoverRate","0.9"],["ShotDelay","0.13"],["CoolDownTime","1.3"],["HeatPerShot","0.23"],["HeatRecoverRate","0.17"],["HeatThreshold","0.29"]]}}}"#;
        let classes = bfe_content::json::parse(json).unwrap();
        let matrix = three::Matrix4::default().elements;
        let mut rng = SpaceRng::new(Wide::from_u32(37));
        let mut step = |turret: &mut SpaceTurretState, trigger: bool| {
            let mut pool = SpaceOrdnancePool::new(4);
            step_turret_fire(turret, &matrix, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1, None, None, None, true, trigger, 1.0 / 60.0, &mut rng, &mut pool);
        };
        let (mut held, mut let_go) = (test_turret(&classes, "test_hot_gun"), test_turret(&classes, "test_hot_gun"));
        while !held.overheated {
            step(&mut held, true);
            step(&mut let_go, true);
        }
        let start = held.kick.value;
        for tick in 0..180 {
            step(&mut held, true);
            step(&mut let_go, false);
            assert_eq!(held.kick.value, let_go.kick.value, "tick {tick}");
        }
        assert!(held.overheated);
        assert!(held.kick.value < start - PI / 180.0, "{start}");
    }

    #[test]
    fn a_ship_guns_rounds_end_by_the_blast_its_ordnance_inherits_the_name_of() {
        let json = r#"{"wpnc":{"test_gun":{"base":"launcher","properties":[["OrdnanceName","test_shot"]]}},
            "ordc":{"test_shot_root":{"base":"bolt","properties":[["ExplosionExpire","test_flak"]]},"test_shot":{"base":"test_shot_root","properties":[]}},
            "expc":{"test_flak_root":{"base":"explosion","properties":[["Damage","37"],["DamageRadiusInner","3.4"]]},"test_flak":{"base":"test_flak_root","properties":[["DamageRadiusOuter","8.6"],["VehicleScale","1.65"]]}}}"#;
        let (spec, _) = parse_map_class_weapon(Some(&bfe_content::json::parse(json).unwrap()), "test_gun").unwrap();
        assert_eq!((spec.expire_blast.damage, spec.expire_blast.inner_radius, spec.expire_blast.outer_radius), (37.0, 3.4, 8.6));
        assert_eq!(spec.expire_blast.scales, [1.0, 1.0, 1.0, 1.65, 1.0]);
        assert!(!spec.blast.hurts());
    }

    #[test]
    fn a_ship_guns_barrage_paces_a_bot_and_never_a_player() {
        // An invented gun whose bots let go for 0.9 s after every fifth salvo,
        // its trigger held for 3 s; a player takes a bot's seat in the pause.
        let json = r#"{"wpnc":{"test_barrage_gun":{"base":"cannon","properties":[["ShotDelay","0.11"],["BarrageMin","5"],["BarrageMax","5"],["BarrageDelay","0.9"]]}}}"#;
        let classes = bfe_content::json::parse(json).unwrap();
        let matrix = three::Matrix4::default().elements;
        let mut rng = SpaceRng::new(Wide::from_u32(41));
        let mut salvos = |turret: &mut SpaceTurretState, bot: bool, ticks: u32| {
            let mut fired = 0;
            for _ in 0..ticks {
                let mut pool = SpaceOrdnancePool::new(4);
                fired += step_turret_fire(turret, &matrix, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1, None, None, None, bot, true, 1.0 / 60.0, &mut rng, &mut pool).0;
            }
            fired
        };
        let gun = || test_turret(&classes, "test_barrage_gun");
        assert_eq!((salvos(&mut gun(), false, 180), salvos(&mut gun(), true, 180)), (28, 14));
        let mut seat = gun();
        assert_eq!(salvos(&mut seat, true, 40), 5);
        assert!(seat.barrage_timer > 0.5);
        assert_eq!(salvos(&mut seat, false, 1), 1);
    }

    #[test]
    fn a_ship_guns_shot_delay_counts_from_the_tick_after_its_last_salvo() {
        // The invented gun of the craft's test, the same ticks.
        let json = r#"{"wpnc":{"test_salvo_gun":{"base":"cannon","properties":[["SalvoCount","5"],["SalvoDelay","0.057"],["InitialSalvoDelay","0.13"],["ShotDelay","0.31"]]}}}"#;
        let mut turret = test_turret(&bfe_content::json::parse(json).unwrap(), "test_salvo_gun");
        let matrix = three::Matrix4::default().elements;
        let mut rng = SpaceRng::new(Wide::from_u32(47));
        let ticks: Vec<usize> = (0..95)
            .filter(|_| {
                let mut pool = SpaceOrdnancePool::new(4);
                step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1, None, None, None, true, true, 1.0 / 60.0, &mut rng, &mut pool).0 > 0
            })
            .collect();
        assert_eq!(ticks, [7, 11, 14, 18, 21, 48, 51, 55, 58, 62, 89, 92]);
    }

    #[test]
    fn only_a_bots_chair_gun_takes_a_spread_scale() {
        // An invented gun whose kick never falls back, fired by a bot with no
        // scale, by a player and by a bot, the last two with a scale of 2.5.
        let json = r#"{"wpnc":{"test_kick_gun":{"base":"cannon","properties":[["SpreadPerShot","0.2"],["SpreadLimit","30"],["ShotDelay","0.11"]]}}}"#;
        let classes = bfe_content::json::parse(json).unwrap();
        let matrix = three::Matrix4::default().elements;
        let fire = |bot: bool, scale: Option<f64>| {
            let mut turret = test_turret(&classes, "test_kick_gun");
            turret.pending_remote_spread_scale = scale;
            let mut rng = SpaceRng::new(Wide::from_u32(31));
            let mut shots = Vec::new();
            for _ in 0..60 {
                let mut pool = SpaceOrdnancePool::new(4);
                let (fired, _, d) = step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1, None, None, None, bot, true, 1.0 / 60.0, &mut rng, &mut pool);
                if fired > 0 {
                    shots.push(d);
                }
            }
            shots
        };
        let plain = fire(true, None);
        assert!(plain.len() > 5);
        assert_eq!(fire(false, Some(2.5)), plain);
        assert_ne!(fire(true, Some(2.5)), plain);
    }

    #[test]
    fn a_ship_gun_reads_its_pitch_and_yaw_spread_through_its_parent_class() {
        // An invented gun and its parent: the parent's pitch spread stays, the
        // gun's own yaw spread replaces the parent's.
        let json = r#"{"wpnc":{"test_gun_base":{"base":"cannon","properties":[["PitchSpread","0.6"],["YawSpread","0.9"]]},"test_gun":{"base":"test_gun_base","properties":[["YawSpread","1.3"]]}}}"#;
        let (spec, _) = parse_map_class_weapon(Some(&bfe_content::json::parse(json).unwrap()), "test_gun").unwrap();
        let deg = PI / 180.0;
        assert_eq!((spec.pitch_spread, spec.yaw_spread), (0.6 * deg, 1.3 * deg));
    }

    #[test]
    fn a_ship_gun_fires_every_round_of_each_salvo() {
        // An invented gun: one salvo a shot of four rounds.
        let json = r#"{"wpnc":{"test_salvo_gun":{"base":"cannon","properties":[["SalvoCount","1"],["ShotsPerSalvo","4"],["ShotDelay","0.37"]]}}}"#;
        let mut turret = test_turret(&bfe_content::json::parse(json).unwrap(), "test_salvo_gun");
        let matrix = three::Matrix4::default().elements;
        let mut rng = SpaceRng::new(Wide::from_u32(29));
        let mut pool = SpaceOrdnancePool::new(16);
        let (fired, _, _) = step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1, None, None, None, true, true, 1.0 / 60.0, &mut rng, &mut pool);
        assert_eq!(fired, 4);
        assert_eq!(turret.shots_fired, 4);
    }

    #[test]
    fn a_tracer_goes_out_with_a_ship_guns_first_round_and_every_period_after() {
        // An invented gun of one salvo of fourteen rounds that inherits a tracer
        // to every four.
        let json = r#"{"wpnc":{"test_arm":{"base":"cannon","properties":[["SecondaryOrdnancePeriod","4"]]},"test_tracer_gun":{"base":"test_arm","properties":[["SalvoCount","1"],["ShotsPerSalvo","14"]]}}}"#;
        let mut turret = test_turret(&bfe_content::json::parse(json).unwrap(), "test_tracer_gun");
        let matrix = three::Matrix4::default().elements;
        let mut rng = SpaceRng::new(Wide::from_u32(29));
        let mut pool = SpaceOrdnancePool::new(16);
        step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1, None, None, None, true, true, 1.0 / 60.0, &mut rng, &mut pool);
        let tracers: Vec<usize> = (0..14).filter(|&i| pool.slots[i].is_tracer).collect();
        assert_eq!((pool.active_count(), tracers), (14, vec![0, 4, 8, 12]));
    }

    #[test]
    fn a_ship_gun_marks_periodic_tracer_rounds() {
        let json = r#"{"wpnc":{"test_tracer_gun":{"base":"cannon","properties":[["SalvoCount","1"],["ShotsPerSalvo","4"],["ShotDelay","0.2"],["SecondaryOrdnancePeriod","3"]]}}}"#;
        let mut turret = test_turret(&bfe_content::json::parse(json).unwrap(), "test_tracer_gun");
        let matrix = three::Matrix4::default().elements;
        let mut rng = SpaceRng::new(Wide::from_u32(13));
        let mut pool = SpaceOrdnancePool::new(8);
        let (fired, _, _) = step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1, None, None, None, true, true, 1.0 / 60.0, &mut rng, &mut pool);
        assert_eq!(fired, 4);
        let flags: Vec<bool> = pool.slots[..4].iter().map(|p| p.is_tracer).collect();
        assert_eq!(flags, vec![true, false, false, true]);
    }

    #[test]
    fn a_ship_gun_spreads_its_shots_by_max_spread_on_each_axis_whatever_its_heat() {
        // An invented gun: each shot within 0.025 of MaxSpread in pitch and
        // 0.05 of it in yaw, in the gun's frame; no MinSpread, and no wider
        // when hot.
        let json = r#"{"wpnc":{"test_ship_gun":{"base":"launcher","properties":[["MinSpread","0.14"],["MaxSpread","0.29"],["ShotDelay","0.43"]]}}}"#;
        let classes = bfe_content::json::parse(json).unwrap();
        let (weapon, filter) = parse_map_class_weapon(Some(&classes), "test_ship_gun").expect("the invented gun parses");
        let mut turret = SpaceTurretState {
            pilot_type: SpaceTurretPilotType::SelfPiloted,
            weapon,
            filter,
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            yaw_min: -PI,
            yaw_max: PI,
            pitch_min: -0.5,
            pitch_max: 0.8,
            turn_rate: 2.0,
            pitch_rate: 2.0,
            clock: Default::default(),
            barrage_remaining: 0,
            barrage_timer: 0.0,
            trigger_was_held: false,
            warm_up: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
            muzzle_offsets: Vec::new(),
            remote_operator: None,
            pending_remote_aim: None,
            pending_remote_fire: false,
            pending_remote_spread_scale: None,
            kick: Default::default(),
        };
        let matrix = three::Matrix4::default().elements;
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let n = v8math::hypot3(0.31, 0.22, 0.92);
        let dir = [0.31 / n, 0.22 / n, 0.92 / n];
        let r = [dir[2], 0.0, -dir[0]];
        let rn = v8math::hypot3(r[0], r[1], r[2]);
        let right = [r[0] / rn, r[1] / rn, r[2] / rn];
        let up = [dir[1] * right[2] - dir[2] * right[1], dir[2] * right[0] - dir[0] * right[2], dir[0] * right[1] - dir[1] * right[0]];
        let (pitch_max, yaw_max) = (0.025 * 0.29, 0.05 * 0.29);
        let mut rng = SpaceRng::new(Wide::from_u32(23));
        for heat in [0.0, 0.95] {
            let (mut widest_pitch, mut widest_yaw) = (0.0f64, 0.0f64);
            for _ in 0..300 {
                (turret.clock, turret.heat, turret.overheated) = (Default::default(), heat, false);
                let mut pool = SpaceOrdnancePool::new(2);
                let (fired, _, d) = step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], dir, 1, None, None, None, true, true, 1.0 / 60.0, &mut rng, &mut pool);
                assert_eq!(fired, 1);
                let pitch = v8math::asin(dot(d, up)).abs();
                let yaw = v8math::atan2(dot(d, right), dot(d, dir)).abs();
                assert!(pitch <= pitch_max + 1e-12 && yaw <= yaw_max + 1e-12, "heat {heat}: pitch {pitch} yaw {yaw}");
                widest_pitch = v8math::max(widest_pitch, pitch);
                widest_yaw = v8math::max(widest_yaw, yaw);
            }
            assert!(widest_pitch > 0.7 * pitch_max && widest_yaw > 0.7 * yaw_max, "heat {heat}: {widest_pitch} {widest_yaw}");
        }
    }

    #[test]
    fn turret_rate_units_slew_off_axis_warmup_lead_and_los_occlusion() {
        // Values > PROVISIONAL_TURRET_DEG_THRESHOLD convert deg/s -> rad/s;
        // values <= threshold stay rad/s.
        assert!((normalize_turret_rate_rad(90.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!((normalize_turret_rate_rad(1.5) - 1.5).abs() < 1e-9);

        // Velocity lead tilts aim forward along moving target's motion.
        let lead = lead_target_direction([0.0, 0.0, 0.0], [0.0, 0.0, 200.0], [100.0, 0.0, 0.0], 400.0).unwrap();
        assert!(lead[0] > 0.2, "lead must aim ahead in +X, got {:?}", lead);

        // LOS check detects intervening hull wall.
        let mut coll = CollisionOwner::new();
        let world = make_empty_world(&mut coll);
        let mut flags = vec![0u8; 64];
        assert!(has_turret_los([0.0, 2.0, 0.0], [0.0, 2.0, 50.0], None, None, &mut coll, world));
        let _wall = make_box_body(&mut coll, world, [-10.0, -5.0, 20.0], [10.0, 10.0, 22.0], &mut flags, true);
        assert!(!has_turret_los([0.0, 2.0, 0.0], [0.0, 2.0, 50.0], None, None, &mut coll, world));

        // Off-axis slew + WarmUpTime holds fire until aligned and spun up.
        let mut turret = SpaceTurretState {
            pilot_type: SpaceTurretPilotType::SelfPiloted,
            weapon: SpaceWeaponSpec {
                shot_delay: 0.2,
                warm_up_time: 0.25,
                velocity: 800.0,
                life_span: 1.0,
                max_damage: 100.0,
                ..Default::default()
            },
            filter: TurretTargetFilter::default(),
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            yaw_min: -PI,
            yaw_max: PI,
            pitch_min: -0.5,
            pitch_max: 0.8,
            turn_rate: 2.0,
            pitch_rate: 2.0,
            clock: Default::default(),
            barrage_remaining: 0,
            barrage_timer: 0.0,
            trigger_was_held: false,
            warm_up: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
            muzzle_offsets: Vec::new(),
            remote_operator: None,
            pending_remote_aim: None,
            pending_remote_fire: false,
            pending_remote_spread_scale: None,
            kick: Default::default(),
        };
        let matrix = three::Matrix4::default().elements;
        let mut pool = SpaceOrdnancePool::new(16);
        let mut rng = SpaceRng::new(Wide::from_u32(7));
        // Target 90 degrees right (+X): turret starts at yaw = 0, cannot fire on tick 1.
        let (ok0, dir0) = aim_turret_toward(&mut turret, &matrix, [1.0, 0.0, 0.0], 1.0 / 60.0);
        assert!(!ok0, "off-axis target must not be aligned on tick 1");
        let (f0, _, _) = step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], dir0, 1, None, None, None, true, ok0, 1.0 / 60.0, &mut rng, &mut pool);
        assert_eq!(f0, 0);

        // Step 90 ticks (1.5 s): turret finishes slewing (~0.78 s) + finishes warm-up (0.25 s) -> fires.
        let mut total_fired = 0u32;
        for _ in 0..90 {
            let (ok, dir) = aim_turret_toward(&mut turret, &matrix, [1.0, 0.0, 0.0], 1.0 / 60.0);
            let (n, _, _) = step_turret_fire(&mut turret, &matrix, [0.0, 2.0, 0.0], dir, 1, None, None, None, true, ok, 1.0 / 60.0, &mut rng, &mut pool);
            total_fired += n;
        }
        assert!(total_fired >= 2, "turret must fire after slewing and warming up, got {total_fired}");
    }
}
