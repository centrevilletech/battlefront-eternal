//! Unit ownership content from f3-sim.js. Generated records own the boundary
//! layout; the surrounding admission sequence supplies already allocated ids.
//! In particular a registry id is never interchangeable with a body slot.
use super::Call;
use crate::gen::*;
use bfe_content::{actor::Actor, content::Content, defs::ClassDef, Value};
use super::super::{AdmitError, Stage};

/// Droideka's numeric modes include holes. Keeping those holes is essential:
/// a compact list would play a different animation after folding.
pub const DROIDEKA_CLIPS: [Option<&str>; 14] = [
    Some("idle"), Some("turnl"), Some("turnr"), Some("death01"), None,
    Some("idle_to_leftfoot"), None, Some("walk_leftfoot_rightfoot"),
    Some("walk_rightfoot_leftfoot"), Some("leftfoot_to_idle"),
    Some("rightfoot_to_idle"), Some("fold"), Some("roll"), Some("unfold"),
];

/// Encode through the generated writer, including its indirect heap. This
/// also keeps optional null tags distinct from numeric NaNs and signed zero.
pub fn call(words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> Call {
    let mut out = Call { row: vec![0.0; words], heap: Vec::new(), ..Default::default() };
    put(&mut out.row, &mut out.heap);
    out
}

/// F3's constructor passes the collision world's handle, current water level
/// and collision body serial as scalars. An absent water owner writes NaN.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Init {
    pub collision_world: u32,
    pub water_level: f64,
    pub body_serial: u32,
}
impl Init {
    pub fn new(collision_world: u32, water_level: Option<f64>, body_serial: u32) -> Self {
        Self { collision_world, water_level: water_level.unwrap_or(f64::NAN), body_serial }
    }
    pub fn call(self) -> Call {
        Call { args: vec![self.collision_world as f64, self.water_level, self.body_serial as f64], ..Default::default() }
    }
}

/// f3_missile_classes: the empty spelling intentionally aliases Fighter.
/// Intern against the enclosing world's table, after earlier owners' strings.
pub fn missile_classes(mut intern: impl FnMut(&str) -> StrId) -> Vec<F3MissileClass> {
    [ ("", FlightClass::Fighter), ("fighter", FlightClass::Fighter),
      ("scout", FlightClass::Scout), ("bomber", FlightClass::Bomber),
      ("transport", FlightClass::Transport) ].into_iter()
        .map(|(name, class)| F3MissileClass { name: intern(name), class }).collect()
}

/// F3.bodiesIn names collision membership in collision-world order. Private
/// fleet variants may also call this with their single prepared body.
pub fn body_names(bodies: impl IntoIterator<Item = (Ref<Body>, u32)>) -> Vec<F3BodyName> {
    bodies.into_iter().map(|(body, kernel)| F3BodyName { body, kernel }).collect()
}

/// F3.prepareSkeleton reads decomposed bind transforms, not authored matrix
/// entries. buildActor reattaches bone zero to the mesh even when its record
/// names a parent; bones.indexOf(bone.parent) consequently reports -1 there.
pub fn body_skeleton(actor: &Actor, key: F3UnitKey, mut intern: impl FnMut(&str) -> StrId) -> Result<F3BodySkeleton, AdmitError> {
    // Names follow bone-array order, including duplicate names. Joint lookup
    // later uses those indices, so a name-based skeleton map would lose bones.
    let mut bones = Vec::with_capacity(actor.bones.len());
    for (i, bone) in actor.bones.iter().enumerate() {
        let parent = if i == 0 || !(bone.parent >= 0.0) { -1 } else {
            if bone.parent.fract() != 0.0 || bone.parent >= actor.bones.len() as f64 {
                return Err(error(format!("bone {i} has an invalid parent")));
            }
            bone.parent as i32
        };
        bones.push(F3BodyBone { name: intern(&bone.name), parent,
            position: [bone.position.x, bone.position.y, bone.position.z],
            quaternion: [bone.quaternion.x, bone.quaternion.y, bone.quaternion.z, bone.quaternion.w],
            scale: [bone.scale.x, bone.scale.y, bone.scale.z] });
    }
    Ok(F3BodySkeleton { key, bones })
}

/// Resolve the actor using Defs' class/body link, shared with UnitFactory's
/// cold-stage builder. Missing bodies are content errors before adoption.
pub fn class_skeleton(content: &Content, class: &ClassDef, key: F3UnitKey, intern: impl FnMut(&str) -> StrId) -> Result<F3BodySkeleton, AdmitError> {
    let actor = class.body.and_then(|i| content.actors.get(i)).map(|(_, a)| a)
        .ok_or_else(|| error("unit class has no prepared actor"))?;
    body_skeleton(actor, key, intern)
}

/// F3.prepareBody registers every optional tuning value as `?? null`.
/// Clip ids come from AnimKernel.clip in the same world as the body owner.
pub fn droideka_class(def: &Value, mut clip: impl FnMut(&str) -> Option<u32>) -> F3DroidekaClass {
    F3DroidekaClass {
        max_pitch_speed: optional_number(def, "MaxPitchSpeed"),
        ball_turn_speed: optional_number(def, "BallTurnSpeed"),
        ball_max_lean: optional_number(def, "BallMaxLean"),
        ball_acceleration: optional_number(def, "BallAcceleration"),
        ball_rolling_friction: optional_number(def, "BallRollingFriction"),
        clips: DROIDEKA_CLIPS.map(|name| name.and_then(&mut clip).map(f64::from).unwrap_or(-1.0)),
    }
}

// Nullable numeric fields test presence before Number conversion. A false or
// empty-string value is present and converts to zero, rather than becoming null.
pub(super) fn optional_number(def: &Value, key: &str) -> Option<f64> {
    def.get(key).filter(|v| !matches!(v, Value::Null)).map(|v| bfe_content::js::to_number(Some(v)))
}
pub(super) fn number_or(def: &Value, key: &str, fallback: f64) -> f64 {
    optional_number(def, key).unwrap_or(fallback)
}
pub(super) fn number_truthy(def: &Value, key: &str, fallback: f64) -> f64 {
    let number = bfe_content::js::to_number(def.get(key));
    if number == 0.0 || number.is_nan() { fallback } else { number }
}
pub(super) fn error(message: impl Into<String>) -> AdmitError { AdmitError::Content { stage: Stage::Units, message: message.into() } }

/// F3.prepareImpacts installs full damage scaling even for ordnance already
/// sent to F1 before unit ownership exists. Damage uses `||`, scales `??`.
pub fn hit_policy(ordnance: DefId, def: &Value) -> F3HitPolicy {
    let damage = def.get("Damage").filter(|v| v.truthy()).map(|v| bfe_content::js::to_number(Some(v))).unwrap_or(0.0);
    F3HitPolicy { ordnance, damage, person: number_or(def, "PersonScale", 1.0),
        animal: number_or(def, "AnimalScale", 1.0), droid: number_or(def, "DroidScale", 1.0),
        vehicle: number_or(def, "VehicleScale", 1.0), building: number_or(def, "BuildingScale", 1.0),
        shield: number_or(def, "ShieldScale", 1.0) }
}

/// Explosion ids belong to separate F1 and combat tables. Do not assume the
/// two tables assigned a shared definition the same number.
pub fn blast_binding(explosion: u32, combat: u32) -> F3BlastBinding { F3BlastBinding { explosion, combat } }

/// The host only disables friendly fire for the literal Boolean false.
pub fn impact_rules(setting: Option<&Value>) -> F3ImpactRules {
    F3ImpactRules { friendly_fire: !matches!(setting, Some(Value::Bool(false))) }
}

/// F3World's external explosion entry is also used by placed props. Preparing
/// the owner snapshot remains the caller's prerequisite to this pure row.
pub fn world_blast(explosion: u32, position: Vec3, owner: OrdnanceOwner) -> F3BlastCall {
    F3BlastCall { explosion, position, owner }
}

/// Weapon constructor's merged charge levels preserve object insertion order:
/// replacing a key changes its value, while a new key appends at the end.
pub fn weapon_levels(def: &Value) -> Result<Vec<Value>, AdmitError> {
    // Keep the base allocation itself: WeakSet service caches and the combat
    // definition table both distinguish equal-looking loadout objects.
    let base = def.as_object().ok_or_else(|| error("weapon definition is not an object"))?;
    let mut out = vec![def.clone()];
    if let Some(charges) = def.get("charges").filter(|v| v.truthy()) {
        let charges = charges.as_array().ok_or_else(|| error("weapon charges are not an array"))?;
        for charge in charges {
            let entries = charge.as_object().ok_or_else(|| error("weapon charge is not an object"))?;
            let written = base.entries().chain(entries.entries()).map(|(k,v)| (std::rc::Rc::from(k),v.clone())).collect();
            out.push(Value::Object(std::rc::Rc::new(bfe_content::Object::from_written(written))));
        }
    }
    Ok(out)
}

/// The early weaponAI view, before the cooked policy table publishes. Flag
/// coercion follows Number, including NaN being false; ai masks use >>> 0.
pub fn lock_rule(def: &Value) -> F3LockRule {
    let mut relation_mask = 1u32;
    let mut category_mask = 0xffff_ff9fu32;
    let set = |mask: &mut u32, key: &str, bit: u32| {
        if let Some(value) = def.get(key) {
            let n = bfe_content::js::to_number(Some(value));
            if n != 0.0 && !n.is_nan() { *mask |= bit; } else { *mask &= !bit; }
        }
    };
    // TargetAll is applied before individual bits even when its property
    // appeared later in the JSON object. This order is maskFromFlags policy.
    if let Some(all) = def.get("TargetAll") {
        let n = bfe_content::js::to_number(Some(all));
        category_mask = if n != 0.0 && !n.is_nan() { u32::MAX } else { 0 };
    }
    for (key, bit) in [("TargetEnemy",1),("TargetNeutral",2),("TargetFriendly",4)] { set(&mut relation_mask,key,bit); }
    for (key, bit) in [("TargetPerson",1),("TargetAnimal",2),("TargetDroid",4),("TargetVehicle",8),
        ("TargetBuilding",16),("TargetBuildingDead",32),("TargetBuildingUnbuilt",64)] { set(&mut category_mask,key,bit); }
    if let Some(ai) = def.get("ai").filter(|v| v.truthy()) {
        // A present ai object suppresses all flattened flags, even when it
        // leaves one mask out. The two defaults are independent.
        relation_mask = optional_number(ai,"relationMask").map(bfe_content::js::to_uint32).unwrap_or(1);
        category_mask = optional_number(ai,"categoryMask").map(bfe_content::js::to_uint32).unwrap_or(0xffff_ff9f);
    }
    // LockOffAngle zero falls back to the on angle. Explicit negative angles
    // and infinities remain truthy Numbers, matching the original || test.
    let on_angle = number_truthy(def,"LockOnAngle",0.0);
    F3LockRule { range: number_truthy(def,"LockOnRange",0.0), on_angle,
        off_angle: number_truthy(def,"LockOffAngle",on_angle), relation_mask, category_mask }
}

/// F3.prepareWeapons carries level-specific ordnance and item ids, but every
/// charge level retains the base weapon's channel and animation bank.
pub fn weapon_service(base: &Value, level: &Value, ordnance: Option<DefId>, item: Option<DefId>,
    mut intern: impl FnMut(&str) -> StrId, has_lightning: bool) -> F3WeaponService {
    let bank = text_truthy(base,"AnimationBank","rifle");
    let lightning = level.get("ordnance").map(|o| text_truthy(o,"LightningEffect","")).unwrap_or("");
    F3WeaponService { ordnance, item, channel: bfe_content::js::to_number(base.get("channel")),
        lock: lock_rule(level), bank: intern(bank), lightning: intern(lightning),
        has_lightning: !lightning.is_empty() && has_lightning }
}

pub(super) fn text_truthy<'a>(def: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    def.get(key).filter(|v| v.truthy()).and_then(Value::as_str).unwrap_or(fallback)
}

/// F3Hulls.prepare keeps the model's local sphere centre and the larger of
/// its radius and half the box diagonal. Vector3.length uses sqrt(x*x+...).
#[derive(Clone, Copy, Debug)]
pub struct HullIdentity {
    pub hull: Ref<Vehicle>, pub member: Option<Member>, pub category: u32,
    pub kind: f64, pub key: StrId, pub health: u32,
}
pub fn hull(identity: HullIdentity, size: Vec3, center: Vec3, sphere: Option<[f64;4]>,
    explosion: Option<u32>, final_explosion: Option<u32>) -> F3HullSetup {
    // Vector3.length evaluates the sum in this order. Math.hypot scales its
    // inputs differently and would change otherwise equivalent last bits.
    let radius = 0.5 * (size[0]*size[0] + size[1]*size[1] + size[2]*size[2]).sqrt();
    let (blast_center, blast_radius) = sphere.map(|s| ([s[0],s[1],s[2]], v8math::max(s[3],radius))).unwrap_or((center,radius));
    F3HullSetup { hull: identity.hull, member: identity.member, health: identity.health,
        category: identity.category, kind: identity.kind, key: identity.key,
        blast_center, blast_radius, explosion, final_explosion }
}

/// F3Items.prepare assigns base groups in first-use order, not alphabetically.
/// Repeated registration still emits a row, matching F3's initial sent scan.
#[derive(Default)]
pub struct ItemGroups { bases: Vec<String> }
impl ItemGroups {
    pub fn prepare(&mut self, def: &Value, id: DefId, pool: Option<f64>, model: ModelAnswer) -> F3ItemClass {
        let base = def.get("base").and_then(Value::as_str).unwrap_or("undefined");
        // Pools are shared by base, while definition ids distinguish classes.
        // A second mine class must not acquire a new mine-pool namespace.
        let group = match self.bases.iter().position(|b| b == base) {
            Some(i) => i, None => { self.bases.push(base.to_owned()); self.bases.len()-1 }
        };
        let health_keys = ["PersonHealth","AnimalHealth","DroidHealth","VehicleHealth","BuildingHealth"];
        let ammo_keys = ["PersonAmmo","AnimalAmmo","DroidAmmo","VehicleAmmo","BuildingAmmo"];
        let gain = |key: &str| def.get(key).filter(|v| v.truthy()).map(|v| bfe_content::js::to_number(Some(v))).unwrap_or(0.0);
        F3ItemClass { def: id, base: group as u32, pool: pool.unwrap_or(f64::INFINITY), mine: base == "mine",
            life: number_or(def,"LifeSpan",f64::INFINITY), health: number_or(def,"MaxHealth",f64::INFINITY), model,
            health_gain: health_keys.map(gain), ammo_gain: ammo_keys.map(gain) }
    }
}

/// prepareUnit keeps absent side-item definitions in the drop roll: dropping
/// those entries changes which later item is chosen with the same RNG word.
pub fn unit_items(def: &Value, class: DefId, explosion: Option<u32>,
    mut item: impl FnMut(&str) -> Result<Option<DefId>, AdmitError>,
    mut intern: impl FnMut(&str) -> StrId) -> Result<F3UnitItems, AdmitError> {
    let mut drops = Vec::new();
    if let Some(entries) = def.get("drops").filter(|v| v.truthy()) {
        for entry in entries.as_array().ok_or_else(|| error("unit drops are not an array"))? {
            let name = entry.get("class").and_then(Value::as_str).unwrap_or("undefined");
            // fieldWords keeps the original value kind. A probability string
            // is interned, not parsed into a number before the eventual roll.
            let probability = entry.get("probability");
            let (probability_tag, probability) = match probability {
                None => (0,0.0), Some(Value::Null) => (1,0.0),
                Some(Value::Bool(b)) => (if *b {3} else {2},0.0),
                Some(Value::Number(n)) => (4,*n),
                Some(Value::String(s)) => (5,intern(s).0 as f64),
                Some(_) => return Err(error("drop probability requires an explosion definition binding")),
            };
            drops.push(DropEntry { def: item(name)?, probability_tag, probability });
        }
    }
    Ok(F3UnitItems { class, explosion, drops, add_health: bfe_content::js::to_number(def.get("AddHealth")),
        health_effect: intern(text_truthy(def,"HealthGainEffect","Powerup_blue")),
        ammo_effect: intern(text_truthy(def,"AmmoGainEffect","Powerup_red")) })
}

/// F3Items.flush uses a body-qualified key independent of lazy owner numbering.
/// No call to ItemsWorld.ownerIds is allowed during cold construction.
pub fn dispenser_binding(key: F3UnitKey, weapon: DefId, level: u32, def: &Value,
    mut intern: impl FnMut(&str) -> StrId) -> F3DispenserBinding {
    let name = def.get("class").and_then(Value::as_str).unwrap_or("");
    // The life word is a JavaScript Number; use its string conversion for
    // the same key spelling, including signed zero and exponential notation.
    let owned_key = format!("{name}:body:{}:{}:{}", key.slot,key.generation,bfe_content::js::number_to_string(key.life));
    F3DispenserBinding { key, weapon, level, name: intern(name), owned_key: intern(&owned_key) }
}

/// Registry construction reserves the already-populated mixed Units list.
/// Future Units.add calls carry only the member kind and zero generation.
pub fn registry_prepare() -> Call { Call::default() }
pub fn registry_add(kind: KindCode) -> Result<Call, AdmitError> {
    // These are generated kind codes, not the discriminants of Member. The
    // native registry chooses its Member variant from this scalar vocabulary.
    if ![KIND_UNIT,KIND_VEHICLE_TARGET,KIND_TURRET_TARGET,KIND_OBJECTIVE_TARGET].contains(&kind) {
        return Err(error("unknown unit registry member kind"));
    }
    Ok(Call { args: vec![kind as f64,0.0], ..Default::default() })
}


/// A cold unit-owner call with its scalar export parameters kept separate
/// from the generated argument block. Call order belongs to construction:
/// skeletons and services follow each body's mixed-registry insertion.
#[derive(Clone, Debug)]
pub enum Entry {
    Init(Init),
    MissileClass(F3MissileClass),
    BodyNames(Vec<F3BodyName>),
    Skeleton(F3BodySkeleton),
    WeaponServices { weapon: DefId, levels: Vec<F3WeaponService> },
    HitPolicy(F3HitPolicy),
    BlastBinding(F3BlastBinding),
    ImpactRules(F3ImpactRules),
    WorldBlast(F3BlastCall),
    DroidekaClass(F3DroidekaClass),
    RegistryPrepare,
    RegistryAdd(KindCode),
    Relations(Vec<F3RelationFact>),
    Building(F3BuildingSetup),
    Hull(F3HullSetup),
    ItemClass(F3ItemClass),
    UnitItems(F3UnitItems),
    Dispenser(F3DispenserBinding),
    ReconOwners(u32),
    ReconClass(F3ReconClass),
    ReconLevel(F3ReconLevel),
    ReconStrike(F3ReconStrike),
    ReconWeapon(F3ReconWeapon),
    ReconCommand(F3ReconBinding),
}

/// The two independent inputs visible in the call recording. Neither scalar
/// arguments nor a heap is prefixed to the generated row's field offsets.
#[derive(Clone, Debug, Default)]
pub struct Export {
    pub name: &'static str,
    pub arguments: Vec<f64>,
    pub call: Call,
}
impl Entry {
    pub fn encode(&self) -> Result<Export, AdmitError> {
        macro_rules! record {
            ($name:literal,$row:ident,$type:ty) => {
                Export {name:$name,call:call(<$type>::WORDS,|r,h|$row.put(r,h)),..Default::default()}
            };
        }
        Ok(match self {
            Self::Init(row) => Export {name:"f3_init",arguments:row.call().args,..Default::default()},
            Self::MissileClass(row) => Export {arguments:vec![1.0],..record!("f3_missile_classes",row,F3MissileClass)},
            Self::BodyNames(rows) => {
                let mut value = Export {name:"f3_body_names",arguments:vec![rows.len() as f64],..Default::default()};
                value.call.row.resize(rows.len()*F3BodyName::WORDS,0.0);
                for (row,dest) in rows.iter().zip(value.call.row.chunks_exact_mut(F3BodyName::WORDS)) {row.put(dest,&mut value.call.heap);}
                value
            }
            Self::Skeleton(row) => record!("f3_body_skeleton",row,F3BodySkeleton),
            Self::WeaponServices {weapon,levels} => {
                let mut value = Export {name:"f3_weapon_services",arguments:vec![weapon.0 as f64,levels.len() as f64],..Default::default()};
                value.call.row.resize(levels.len()*F3WeaponService::WORDS,0.0);
                for (row,dest) in levels.iter().zip(value.call.row.chunks_exact_mut(F3WeaponService::WORDS)) {row.put(dest,&mut value.call.heap);}
                value
            }
            Self::HitPolicy(row) => record!("f3_hit_policy",row,F3HitPolicy),
            Self::BlastBinding(row) => record!("f3_blast_binding",row,F3BlastBinding),
            Self::ImpactRules(row) => record!("f3_impact_rules",row,F3ImpactRules),
            Self::WorldBlast(row) => record!("f3_world_blast",row,F3BlastCall),
            Self::DroidekaClass(row) => record!("units_droideka_class",row,F3DroidekaClass),
            Self::RegistryPrepare => Export {name:"f3_registry_prepare",..Default::default()},
            Self::RegistryAdd(kind) => Export {name:"f3_registry_add",arguments:registry_add(*kind)?.args,..Default::default()},
            Self::Relations(rows) => {
                let mut value = Export {name:"f3_relations",arguments:vec![rows.len() as f64],..Default::default()};
                value.call.row.resize(rows.len()*F3RelationFact::WORDS,0.0);
                for (row,dest) in rows.iter().zip(value.call.row.chunks_exact_mut(F3RelationFact::WORDS)) {row.put(dest,&mut value.call.heap);}
                value
            }
            Self::Building(row) => record!("f3_building",row,F3BuildingSetup),
            Self::Hull(row) => record!("f3_hull",row,F3HullSetup),
            Self::ItemClass(row) => record!("f3_item_class",row,F3ItemClass),
            Self::UnitItems(row) => record!("f3_unit_items",row,F3UnitItems),
            Self::Dispenser(row) => record!("f3_dispenser_binding",row,F3DispenserBinding),
            Self::ReconOwners(capacity) => Export {name:"f3_recon_owners",arguments:vec![*capacity as f64],..Default::default()},
            Self::ReconClass(row) => record!("f3_recon_class",row,F3ReconClass),
            Self::ReconLevel(row) => record!("f3_recon_level",row,F3ReconLevel),
            Self::ReconStrike(row) => record!("f3_recon_strike",row,F3ReconStrike),
            Self::ReconWeapon(row) => record!("f3_recon_weapon",row,F3ReconWeapon),
            Self::ReconCommand(row) => record!("f3_recon_command",row,F3ReconBinding),
        })
    }
}

/// F3Registry.relations expands at least six teams, row-major, after mission
/// teams and explicit alliance overrides have supplied Units.relation.
pub fn relations(team_count: usize, mut relation: impl FnMut(f64,f64)->f64) -> Result<Vec<F3RelationFact>,AdmitError> {
    let count = team_count.max(6);
    let total = count.checked_mul(count).filter(|&n|n<=u32::MAX as usize).ok_or_else(||error("unit relation table is too large"))?;
    let mut rows = Vec::new(); rows.try_reserve(total).map_err(|_|error("unit relation table cannot be reserved"))?;
    for a in 0..count { for b in 0..count { rows.push(F3RelationFact {a:a as f64,b:b as f64,value:relation(a as f64,b as f64)}); } }
    Ok(rows)
}

/// Shared lazy F1/combat tables used by F3.prepareWeapons. Implementations
/// retain any definition-registration calls so construction can emit those
/// before the enclosing service call, including nested explosion bindings.
pub trait Definitions: super::recon::Definitions {
    fn item(&mut self, def: &Value) -> Result<DefId,AdmitError>;
    fn has_lightning(&self, name: &str) -> bool;
}

/// F3.prepareWeapons uses a WeakSet of base definitions, and a separate recon
/// class Map. Equal-named weapons in different loadouts remain distinct.
#[derive(Default)]
pub struct Weapons { sent: Vec<Value>, recon: super::recon::Classes }
impl Weapons {
    pub fn prepare(&mut self, base: &Value, definitions: &mut impl Definitions,
        muzzle: Option<Vec3>) -> Result<Vec<Export>,AdmitError> {
        let id = bfe_content::defs::identity(base).ok_or_else(||error("weapon service has no object identity"))?;
        if self.sent.iter().any(|v|bfe_content::defs::identity(v)==Some(id)) {return Ok(Vec::new())}
        let mut calls = self.recon.prepare(base,definitions,muzzle)?.into_iter()
            .map(|(_,name,call)|Export {name,arguments:Vec::new(),call}).collect::<Vec<_>>();
        // Every level retains its base channel/bank. Only ordnance, item,
        // lightning and lock facts are read from the merged charge definition.
        let mut services = Vec::new();
        for level in weapon_levels(base)? {
            let ordnance = level.get("ordnance").filter(|v|!matches!(v,Value::Null)).map(|v|definitions.ordnance(v)).transpose()?;
            let item = level.get("item").filter(|v|!matches!(v,Value::Null)).map(|v|definitions.item(v)).transpose()?;
            let lightning = level.get("ordnance").map(|v|text_truthy(v,"LightningEffect","")).unwrap_or("");
            let has_lightning = !lightning.is_empty() && definitions.has_lightning(lightning);
            services.push(weapon_service(base,&level,ordnance,item,|s|definitions.intern(s),has_lightning));
        }
        let weapon = definitions.weapon(base)?;
        let service=Entry::WeaponServices {weapon,levels:services}.encode()?;
        let (_,encoded)=service.clone().into_call();
        definitions.emit(service.name,&encoded);
        calls.push(service);
        // Retain the source Rc as well as its identity: releasing it here
        // would let an allocator reuse that address for a different definition.
        self.sent.push(base.clone());
        Ok(calls)
    }
}

/// The shared sequence fills runtime identities before this owner is built.
pub fn build(cx: &mut super::Cx<'_>) -> Result<super::Calls, AdmitError> {
    // The bound entry point accepts receipts supplied by the sequence. This
    // fallback reads the same receipts from the shared F1 registration tables.
    let bindings=initial_bindings(cx)?;
    let mut groups=load_groups(cx)?;
    let mut calls=build_bound(cx,&bindings,&mut groups)?;
    calls.extend(super::props::build(cx)?);
    let hulls=std::mem::take(&mut cx.ids.hulls);
    // A topology change publishes the complete collision membership again;
    // an unchanged world keeps the individual hull names already sent.
    let names=cx.ids.bodies_changed.then(||cx.body_names());
    let suffix=finish_bound(cx,&hulls,names.as_deref());
    cx.ids.hulls=hulls;
    calls.extend(suffix?);
    calls.extend(super::items::placed(cx)?);
    cx.ids.units=Some(bindings);
    Ok(calls)
}

impl Export {
    /// Call keeps scalar parameters beside the generated argument block.
    /// The wrapper remains useful for code that selects an export by name.
    pub fn into_call(mut self) -> (&'static str,Call) {
        self.call.args = self.arguments;
        (self.name,self.call)
    }
}

/// Build a generated record against the shared string table, retaining the
/// precise interns that preceded this call. Content-derived fields are built
/// before encoding, so no row offsets or pointer relocations are duplicated.
pub fn in_context(cx: &mut super::Cx<'_>,
    make: impl FnOnce(&mut super::Cx<'_>,&mut Call)->Result<Entry,AdmitError>) -> Result<(Stage,&'static str,Call),AdmitError> {
    let mut names = Call::default();
    let entry = make(cx,&mut names)?;
    let (name,mut row) = entry.encode()?.into_call();
    row.strings = names.strings;
    Ok((Stage::Units,name,row))
}

/// F3's fixed missile vocabulary follows f3_init, after earlier owners have
/// interned their names. Each original export takes exactly one class row.
pub fn missile_calls(cx: &mut super::Cx<'_>) -> Result<super::Calls,AdmitError> {
    let mut calls = Vec::new();
    for (name,class) in [("",FlightClass::Fighter),("fighter",FlightClass::Fighter),
        ("scout",FlightClass::Scout),("bomber",FlightClass::Bomber),("transport",FlightClass::Transport)] {
        calls.push(in_context(cx,|cx,call| Ok(Entry::MissileClass(F3MissileClass {name:cx.intern(call,name),class})))?);
    }
    Ok(calls)
}

/// UnitFactory's body preparation shares Content's already-decomposed actor.
/// The qualified key is the registry/construction result of this same world.
pub fn skeleton_call(cx: &mut super::Cx<'_>, actor: &Actor, key: F3UnitKey) -> Result<(Stage,&'static str,Call),AdmitError> {
    in_context(cx,|cx,call|body_skeleton(actor,key,|name|cx.intern(call,name)).map(Entry::Skeleton))
}

/// Facts from the earlier collision, fleet, F1 and combat owners at attachF3.
/// These lists retain their existing registration order; they are not sorted
/// by class name, and no handle is inferred from a definition's contents.
#[derive(Clone,Debug)]
pub struct InitialBindings {
    pub body_serial: u32,
    pub ordnance: Vec<(DefId,Value)>,
    pub blasts: Vec<(u32,u32)>,
    pub items: Vec<ItemBinding>,
}
#[derive(Clone,Debug)]
pub struct ItemBinding {
    pub id: DefId, pub definition: Value, pub pool: Option<f64>, pub model: ModelAnswer,
}

/// Initial attachment ends at registry construction. F3Props runs next;
/// prepared hulls and final collision-body naming then finish the stage.
/// Keeping this split lets sequence::run preserve those owner dependencies.
pub fn build_bound(cx: &mut super::Cx<'_>, bindings: &InitialBindings,
    items: &mut ItemGroups) -> Result<super::Calls,AdmitError> {
    // Initial item rows contain no strings, so constructing their values
    // here cannot advance the interner past the intervening blast scan.
    let mut prefix=bindings.clone();prefix.blasts.clear();
    let mut calls=build_bound_with(cx,&prefix,items,|_,_,_|Ok(Vec::new()))?;
    let (pairs,blasts)=super::combat::with_bindings(cx,Stage::Units,|tables,strings| {
        let mut calls=Vec::new();let mut pairs=Vec::new();
        // F1 explosion ids are already allocated. Combat lazily registers
        // each same source object at this exact point in prepareImpacts.
        for (id,def) in tables.ordnance.explosions.values.clone().iter().enumerate() {
            let combat=tables.combat.explosion(def,strings)?;
            calls.append(&mut tables.combat.calls);
            calls.push(export(Entry::BlastBinding(blast_binding(id as u32,combat.0)),strings)?);
            pairs.push((id as u32,combat.0));
        }
        Ok((pairs,calls))
    })?;
    if !bindings.blasts.is_empty() && bindings.blasts!=pairs {return Err(error("initial blast bindings disagree with shared definition tables"));}
    // Insert after init, its five missile calls and all existing hit rows.
    // The rows following this point retain their original constructor order.
    calls.splice(6+bindings.ordnance.len()..6+bindings.ordnance.len(),blasts);
    save_groups(cx,items);
    Ok(calls)
}

/// F3.prepareImpacts can discover a combat explosion while walking F1's
/// existing explosion list. Emit that owner's lazy registration immediately
/// before its binding, preserving any strings interned between definitions.
pub fn build_bound_with(cx: &mut super::Cx<'_>, bindings: &InitialBindings,
    items: &mut ItemGroups,
    mut prepare_blast: impl FnMut(&mut super::Cx<'_>,u32,u32)->Result<super::Calls,AdmitError>) -> Result<super::Calls,AdmitError> {
    let world=cx.ids.kernel.as_ref().ok_or(AdmitError::Unavailable(Stage::Kernels))?.world;
    let water=cx.battle.world.get("water").filter(|v|v.truthy())
        .map(|v|bfe_content::js::to_number(v.get("height"))+0.0);
    let mut calls=Vec::new();
    // The original constructor asks for the empty name before preparing
    // private fleet variants; a sequence doing that preparation attaches this
    // intern to its first variant call, before entering build_bound.
    calls.push(in_context(cx,|cx,call| {cx.intern(call,"");Ok(Entry::Init(Init::new(world,water,bindings.body_serial)))} )?);
    calls.extend(missile_calls(cx)?);
    for (id,def) in &bindings.ordnance {calls.push(in_context(cx,|_,_|Ok(Entry::HitPolicy(hit_policy(*id,def))))?);}
    // Combat registration can intern strings while this scan is in progress.
    // Running the callback here prevents later bindings from moving those ids.
    for (explosion,combat) in &bindings.blasts {
        calls.extend(prepare_blast(cx,*explosion,*combat)?);
        calls.push(in_context(cx,|_,_|Ok(Entry::BlastBinding(blast_binding(*explosion,*combat))))?);
    }
    for binding in &bindings.items {
        calls.push(in_context(cx,|_,_|Ok(Entry::ItemClass(items.prepare(&binding.definition,binding.id,binding.pool,binding.model.clone()))))?);
    }
    calls.push(in_context(cx,|_,_|Ok(Entry::ImpactRules(impact_rules(None))))?);
    calls.push(in_context(cx,|_,_|Ok(Entry::RegistryPrepare))?);
    // Units.setTeams runs after AI admission. This first table is the original
    // six-team fallback, including neutral team zero on its own diagonal.
    let relation_rows=relations(0,|a,b|if a==0.0||b==0.0 {0.0}else if a==b {1.0}else{-1.0})?;
    calls.push(in_context(cx,|_,_|Ok(Entry::Relations(relation_rows)))?);
    Ok(calls)
}

/// A private hull already has its F2 ref and a stable kernel body. The full
/// collision list is present only when its topology version changed, exactly
/// as F3Hulls.prepare's trailing bodiesIn observes it. `setup.explosion` and
/// `final_explosion` are filled here: F3Hulls.prepare asks F1 for the ids of
/// the class's `explosion` and `finalExplosion || explosion` definitions at
/// that point, registering one F1 has not sent; `name` is the vehicle class.
#[derive(Clone,Debug)]
pub struct HullBinding {
    pub name: String,
    /// F3Hulls.prepare's registry key: `v<index among the live hulls>`,
    /// the empty string for a prepared variant; interned at the row.
    pub key: String,
    pub setup: F3HullSetup,
    pub body: F3BodyName,
    pub changed_bodies: Option<Vec<F3BodyName>>,
}
pub fn finish_bound(cx: &mut super::Cx<'_>, hulls: &[HullBinding],
    final_bodies: Option<&[F3BodyName]>) -> Result<super::Calls,AdmitError> {
    let mut calls=Vec::new();
    for hull in hulls {
        let class=cx.battle.vehicle_def.as_ref().and_then(|data|data.get("classes")).and_then(|classes|classes.get(&hull.name))
            .ok_or_else(||error(format!("hull class {} is not in the vehicle pack",hull.name)))?.clone();
        let (explosion,registered)=super::props::explosion_id(cx,class.get("explosion").filter(|v|v.truthy()))?;
        calls.extend(registered);
        let last=class.get("finalExplosion").filter(|v|v.truthy()).or_else(||class.get("explosion").filter(|v|v.truthy()));
        let (final_explosion,registered)=super::props::explosion_id(cx,last)?;
        calls.extend(registered);
        let mut setup=hull.setup.clone(); setup.explosion=explosion; setup.final_explosion=final_explosion;
        calls.push(in_context(cx,|cx,call| {setup.key=cx.intern(call,&hull.key); Ok(Entry::Hull(setup))})?);
        calls.push(in_context(cx,|_,_|Ok(Entry::BodyNames(vec![hull.body.clone()])))?);
        if let Some(bodies)=&hull.changed_bodies {calls.push(in_context(cx,|_,_|Ok(Entry::BodyNames(bodies.clone())))?);}
    }
    if let Some(bodies)=final_bodies {calls.push(in_context(cx,|_,_|Ok(Entry::BodyNames(bodies.to_vec())))?);}
    Ok(calls)
}

/// game.js presentationLibrary uses last-definition-wins spreads. A later
/// empty emitter list removes an earlier lightning definition of that name.
pub fn lightning_in<'a>(name: &str, libraries: impl IntoIterator<Item=&'a Value>) -> bool {
    let mut selected=None;
    for library in libraries {if let Some(value)=library.get(name) {selected=Some(value);}}
    selected.and_then(Value::as_array).is_some_and(|defs|defs.iter().any(|def|def.get("lightning").is_some_and(Value::truthy)))
}

/// HeadlessEffects consumes the same merged authored library as the page:
/// props, common, other sides, the played side, then the vehicle pack.
pub fn has_lightning(battle: &bfe_content::battle::Battle, name: &str) -> bool {
    let mut libraries=Vec::new();
    if let Some(library)=battle.props.as_ref().and_then(|v|v.get("effects")) {libraries.push(library);}
    if let Some(library)=battle.effects.get("effects") {libraries.push(library);}
    for side in battle.sides.iter().skip(1) {
        if let Some(library)=side.fx.as_ref().and_then(|v|v.get("effects")) {libraries.push(library);}
    }
    if let Some(library)=battle.side_fx.as_ref().and_then(|v|v.get("effects")) {libraries.push(library);}
    if let Some(library)=battle.vehicle_def.as_ref().and_then(|v|v.get("effects")) {libraries.push(library);}
    lightning_in(name,libraries)
}

impl Weapons {
    /// Publish after the shared lazy definition resolver has returned its
    /// calls and strings to Cx; no class-name or model-name lookup is needed.
    pub fn publish(&self, cx: &mut super::Cx<'_>) -> Result<(),AdmitError> {self.recon.publish(cx)}
}

// Item base groups belong to F3, independently of F1's item definition ids.
// Carry their first-use order across the initial scan and later unit seams.
pub(crate) fn load_groups(cx: &super::Cx<'_>) -> Result<ItemGroups,AdmitError> {
    let mut entries:Vec<_>=cx.ids.items.iter().filter_map(|(key,id)|key.strip_prefix("f3-base:").map(|base|(base,*id))).collect();
    // Metadata sits beside F1 paths, under a disjoint prefix. Its numeric
    // values are assigned group ids, so table storage order is immaterial.
    entries.sort_by(|a,b|a.1.total_cmp(&b.1));
    let mut groups=ItemGroups::default();
    for (base,id) in entries {
        if id!=groups.bases.len() as f64 {return Err(error("item base group order is not contiguous"));}
        groups.bases.push(base.to_owned());
    }
    Ok(groups)
}
pub(crate) fn save_groups(cx: &mut super::Cx<'_>,groups:&ItemGroups) {
    cx.ids.items.retain(|(key,_)|!key.starts_with("f3-base:"));
    cx.ids.items.extend(groups.bases.iter().enumerate().map(|(id,base)|(format!("f3-base:{base}"),id as f64)));
}

fn item_binding(content:&Content,def:&Value,id:DefId)->ItemBinding {
    // ItemsWorld.model uses the held-weapon library, including absent meshes.
    // item_radius preserves the fallback radius and optional pickup centre.
    let model=def.get("GeometryName").filter(|v|v.truthy()).and_then(Value::as_str).and_then(|name|content.weapon_models.get(name));
    let (radius,center)=bfe_content::derived::item_radius(model);
    let base=def.get("base").and_then(Value::as_str).unwrap_or("undefined");
    // An explicit null replaces the default pool in the mission spread,
    // then the prepare call turns that nullish entry into Infinity.
    let pool=match content.battle.world.get("mission").and_then(|v|v.get("pools")).and_then(|v|v.get(base)) {
        Some(Value::Null)=>None,
        Some(value)=>Some(bfe_content::js::to_number(Some(value))),
        None=>match base {"mine"=>Some(50.0),"powerupitem"=>Some(25.0),_=>None},
    };
    ItemBinding {id,definition:def.clone(),pool,model:ModelAnswer {mesh:model.is_some(),radius,center:center.map(|v|[v.x,v.y,v.z])}}
}

/// The shared F1 tables already contain the definitions constructed by fleet.
/// Read their sent prefixes, without allocating another id or reloading data.
fn initial_bindings(cx:&mut super::Cx<'_>)->Result<InitialBindings,AdmitError> {
    let content=cx.content;let body_serial=cx.ids.body_serial;
    let (bindings,_)=super::combat::with_bindings(cx,Stage::Units,|bindings,_| {
        // Reserved handles beyond sent are not yet native definitions. They
        // will drain later through definition_with and its service observer.
        let ordnance=bindings.ordnance.definitions.values.iter().take(bindings.ordnance.sent_count()).enumerate()
            .map(|(id,def)|(DefId(id as u32),def.clone())).collect();
        let items=bindings.items.definitions.values.iter().take(bindings.items.sent_count()).enumerate()
            .map(|(id,def)|item_binding(content,def,DefId(id as u32))).collect();
        Ok((InitialBindings {body_serial,ordnance,blasts:Vec::new(),items},Vec::new()))
    })?;
    Ok(bindings)
}

fn export(entry:Entry,strings:&crate::store::Strings)->Result<super::combat::ExportCall,AdmitError> {
    // Keep the interner cursor at encoding time. with_bindings distributes
    // each intervening UTF-16 prefix to the export which first observes it.
    let (name,call)=entry.encode()?.into_call();
    Ok(super::combat::ExportCall {name,call,string_count:Some(strings.list.len())})
}

// F1 notifies nativeServices immediately after each definition. A nested
// explosion therefore registers combat and its F3 binding before its parent.
pub(crate) fn prepared(content:&Content,groups:&mut ItemGroups,combat:&mut super::combat::CombatRows,
    kind:super::ordnance::Prepared,id:DefId,def:&Value,strings:&mut crate::store::Strings,
    calls:&mut Vec<super::combat::ExportCall>)->Result<(),AdmitError> {
    use super::ordnance::Prepared;
    let row=match kind {
        Prepared::Ordnance=>Entry::HitPolicy(hit_policy(id,def)),
        Prepared::Explosion=>{
            let blast=combat.explosion(def,strings)?;
            calls.append(&mut combat.calls);
            Entry::BlastBinding(blast_binding(id.0,blast.0))
        },
        Prepared::Item=>{
            let binding=item_binding(content,def,id);
            Entry::ItemClass(groups.prepare(def,id,binding.pool,binding.model))
        },
    };
    calls.push(export(row,strings)?);Ok(())
}

struct Services<'a> {
    content:&'a Content,
    bindings:&'a mut super::combat::Bindings,
    groups:&'a mut ItemGroups,
    strings:&'a mut crate::store::Strings,
    calls:Vec<super::combat::ExportCall>,
}
impl super::recon::Definitions for Services<'_> {
    fn intern(&mut self,text:&str)->StrId {self.strings.intern_str(text)}
    fn weapon(&mut self,def:&Value)->Result<DefId,AdmitError> {
        let id=self.bindings.combat.weapon(def,self.strings)?;
        self.calls.append(&mut self.bindings.combat.calls);Ok(id)
    }
    fn ordnance(&mut self,def:&Value)->Result<DefId,AdmitError> {
        let content=self.content;let groups=&mut *self.groups;let combat=&mut self.bindings.combat;
        // The hook borrows combat separately from F1, allowing an explosion
        // discovered inside a field to register before that field resumes.
        let id=self.bindings.ordnance.definition_with(def,self.strings,&mut |kind,id,def,strings,calls|prepared(content,groups,combat,kind,id,def,strings,calls))?;
        self.calls.append(&mut self.bindings.ordnance.calls);Ok(id)
    }
    fn explosion(&mut self,def:&Value)->Result<u32,AdmitError> {
        let content=self.content;let groups=&mut *self.groups;let combat=&mut self.bindings.combat;
        let id=self.bindings.ordnance.explosion_with(def,self.strings,&mut |kind,id,def,strings,calls|prepared(content,groups,combat,kind,id,def,strings,calls))?;
        self.calls.append(&mut self.bindings.ordnance.calls);Ok(id.0)
    }
    fn emit(&mut self,name:&'static str,call:&Call) {
        self.calls.push(super::combat::ExportCall {name,call:call.clone(),string_count:Some(self.strings.list.len())});
    }
}
impl Definitions for Services<'_> {
    fn item(&mut self,def:&Value)->Result<DefId,AdmitError> {
        let content=self.content;let groups=&mut *self.groups;let combat=&mut self.bindings.combat;
        let id=self.bindings.items.definition_with(def,&mut self.bindings.ordnance,self.strings,
            &mut |kind,id,def,strings,calls|prepared(content,groups,combat,kind,id,def,strings,calls))?;
        // ItemRows already drained its nested F1 calls into this queue.
        // Appending any rounds again would duplicate explosion definitions.
        self.calls.append(&mut self.bindings.items.calls);Ok(id)
    }
    fn has_lightning(&self,name:&str)->bool {has_lightning(&self.content.battle,name)}
}
impl Services<'_> {
    fn field(&mut self,value:Option<&Value>)->Result<[f64;2],AdmitError> {
        let content=self.content;let groups=&mut *self.groups;let combat=&mut self.bindings.combat;
        let words=self.bindings.ordnance.field_with(value,self.strings,
            &mut |kind,id,def,strings,calls|prepared(content,groups,combat,kind,id,def,strings,calls))?;
        self.calls.append(&mut self.bindings.ordnance.calls);Ok(words)
    }
}

/// F3.prepareWeapons then F3Items.prepareUnit, before adoption. The key is
/// accepted at this seam so spawn can use one API for each qualified body;
/// these class registrations themselves are shared across all of its lives.
pub fn prepare_unit_services(cx:&mut super::Cx<'_>,class:&ClassDef,_key:F3UnitKey)->Result<super::Calls,AdmitError> {
    use super::recon::Definitions as _;
    let content=cx.content;
    let side=cx.defs.names.text(class.side);
    let mut groups=load_groups(cx)?;
    // Classes retain their original Rc objects. Restoring the strike cache
    // also preserves shared base guns versus fresh merged charge objects.
    let recon_values=cx.definitions.recon.iter().map(|(_,v)|v.clone()).collect::<Vec<_>>();
    // Sent markers refer to already assigned combat ids, never wire class
    // names; equal named loadouts can have distinct source allocations.
    let sent=cx.ids.defs.iter().filter_map(|(key,_)|key.strip_prefix("f3-service:").and_then(|id|id.parse::<usize>().ok())).collect::<Vec<_>>();
    let classes=cx.ids.defs.iter().filter_map(|(key,_)|key.strip_prefix("f3-items:").and_then(|id|id.parse::<u32>().ok())).collect::<Vec<_>>();
    let (state,calls)=super::combat::with_bindings(cx,Stage::Player,|bindings,strings| {
        let mut weapons=Weapons {sent:Vec::new(),recon:super::recon::Classes::from_published(&recon_values)?};
        for id in &sent {weapons.sent.push(bindings.combat.weapons.values.get(*id).ok_or_else(||error("service marker has no combat weapon"))?.clone());}
        // Combat attachment normally sent this class before skeleton work.
        // Using its owner here also handles a caller with a reserved class.
        let class_id=bindings.combat.class(&class.record,strings)?;
        let mut services=Services {content,bindings,groups:&mut groups,strings,calls:Vec::new()};
        services.calls.append(&mut services.bindings.combat.calls);
        if let Some(guns)=class.record.get("weapons").and_then(Value::as_array) {
            for gun in guns {
                let muzzle=gun.get("droid").map(|def|super::recon::muzzle(&content.battle,def));
                weapons.prepare(gun,&mut services,muzzle)?;
            }
        }
        // F3Items.units is a class WeakSet, independent of weapon services.
        // A second class sharing a gun still gets its own drop registration.
        if !classes.contains(&class_id.0) {
            let explosion=class.record.get("explosion").filter(|v|v.truthy()).map(|def|services.explosion(def)).transpose()?;
            let side_items=content.battle.sides.iter().find(|entry|entry.name==side).map(|side|&side.items);
            let mut drops=Vec::new();
            if let Some(entries)=class.record.get("drops").filter(|v|!matches!(v,Value::Null)) {
                for drop in entries.as_array().ok_or_else(||error("unit drops are not an array"))? {
                    let name=drop.get("class").and_then(Value::as_str).unwrap_or("undefined");
                    let def=side_items.and_then(|items|items.get(name)).filter(|v|v.truthy()).map(|def|services.item(def)).transpose()?;
                    // fieldWords accepts every value kind, even an object
                    // probability which lazily registers an F1 explosion.
                    let words=services.field(drop.get("probability"))?;
                    drops.push(DropEntry {def,probability_tag:words[0] as u32,probability:words[1]});
                }
            }
            let row=F3UnitItems {class:class_id,explosion,drops,add_health:bfe_content::js::to_number(class.record.get("AddHealth")),
                health_effect:services.intern(text_truthy(&class.record,"HealthGainEffect","Powerup_blue")),
                ammo_effect:services.intern(text_truthy(&class.record,"AmmoGainEffect","Powerup_red"))};
            services.calls.push(export(Entry::UnitItems(row),services.strings)?);
        }
        let ids=weapons.sent.iter().map(|value|services.bindings.combat.weapons.get(value).ok_or_else(||error("prepared weapon lost its combat id"))).collect::<Result<Vec<_>,_>>()?;
        Ok(((weapons,ids,class_id),services.calls))
    })?;
    let (weapons,ids,class_id)=state;
    // Publish only after the resolver returned its strings and definitions.
    // Session then binds the same recon objects at their assigned indices.
    weapons.publish(cx)?;
    save_groups(cx,&groups);
    for id in ids {let key=format!("f3-service:{}",id.0);if !cx.ids.defs.iter().any(|(prior,_)|prior==&key) {cx.ids.defs.push((key,id.0 as f64));}}
    if !classes.contains(&class_id.0) {cx.ids.defs.push((format!("f3-items:{}",class_id.0),class_id.0 as f64));}
    Ok(calls)
}

/// Flush per-life dispenser names after adoption, never while preparing its
/// class. Resolving a base weapon here reads the earlier combat id only.
pub fn dispenser_calls(cx:&mut super::Cx<'_>,class:&ClassDef,key:F3UnitKey)->Result<super::Calls,AdmitError> {
    let (_,calls)=super::combat::with_bindings(cx,Stage::Player,|bindings,strings| {
        let mut calls=Vec::new();
        if let Some(guns)=class.record.get("weapons").and_then(Value::as_array) {
            for gun in guns {
                // Dispenser flushing must not create a new weapon handle.
                // The preparation call above is its explicit prerequisite.
                let weapon=bindings.combat.weapons.get(gun).ok_or_else(||error("dispenser weapon was not prepared"))?;
                for (level,def) in weapon_levels(gun)?.iter().enumerate() {
                    if def.get("item").is_some_and(Value::truthy) {
                        let row=dispenser_binding(key.clone(),weapon,level as u32,def,|name|strings.intern_str(name));
                        calls.push(export(Entry::Dispenser(row),strings)?);
                    }
                }
            }
        }
        Ok(((),calls))
    })?;Ok(calls)
}
