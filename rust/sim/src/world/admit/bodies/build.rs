use super::{error, walker, Muzzles};
use crate::gen::*;
use crate::store::{DefId, Nullable, StrId};
use crate::world::admit::{rows::units, AdmitError};
use bfe_content::{actor::Actor, content::Content, defs::ClassDef, Value};

/// Content identities have already been registered in the enclosing world.
/// They are supplied, never derived from a separate per-body name table.
#[derive(Clone, Debug)]
pub struct Bindings {
    pub key: F3UnitKey,
    pub side: StrId,
    pub class_name: StrId,
    pub class: DefId,
    pub health_scale: StrId,
    /// Shield definition id, when this droideka carries a shield weapon.
    pub shield: Option<DefId>,
}

/// CombatFacade.attach's scalar arguments. The existing cmb_new body builds
/// the arsenal from its registered class, keeping weapon defaults in one place.
#[derive(Clone, Debug)]
pub struct CombatInit {
    pub class: DefId,
    pub max_health: f64,
    pub chunks: f64,
    pub scale: StrId,
}

/// Complete construction facts, before adoption/staging. F3 keeps the private
/// collision and pose buffers with this row when it transfers a cold cookie.
#[derive(Clone, Debug)]
pub struct Body {
    pub unit: Unit,
    pub adoption: F3Adoption,
    pub skeleton: F3BodySkeleton,
    pub combat: CombatInit,
    pub muzzles: Option<Muzzles>,
}
impl Body {
    /// F3Factory.prepare and resident use the same retained adoption facts.
    pub fn stage(&self, cookie: u32) -> F3ColdStage {
        F3ColdStage { cookie, adoption: self.adoption.clone() }
    }
}

/// UnitFactory.build for every prepared soldier class, including a crew's
/// ordinary soldier body. ClassDef shares body/clip links with F3's class rows.
/// A probe droid or other unsupported roster class fails before any owner runs.
pub fn build(content: &Content, class: &ClassDef, ids: Bindings, max_fly_height: f64,
    passive: bool, intern: impl FnMut(&str) -> StrId) -> Result<Body, AdmitError>
{
    let actor = class.body.and_then(|i| content.actors.get(i)).map(|(_, a)| a)
        .ok_or_else(|| error("unit class has no prepared actor"))?;
    let base = class.record.get("base").and_then(Value::as_str);
    if !matches!(base, Some("soldier" | "walkerdroid")) {
        return Err(error("unit class is not a soldier or droideka"));
    }
    let shared = class.clips.and_then(|i| content.body_clips.get(i)).map(|(_, c)| c.as_ref())
        .ok_or_else(|| error("unit class has no prepared shared clips"))?;
    let walker_clips = class.walker_clips.and_then(|i| content.walker_clips.get(i)).map(|(_, c)| c);
    from_parts(&class.record, actor, shared, walker_clips, ids, max_fly_height, passive, intern)
}

/// buildBody over already decoded actor/clip content, also used by small
/// synthetic fixtures. The common bank sets get-up/dive durations even for
/// a droideka; its private bank supplies the walking and gun animation.
pub fn from_parts(def: &Value, actor: &Actor, shared: &bfe_content::clips::Clips,
    walker_clips: Option<&bfe_content::clips::Clips>, ids: Bindings, ceiling: f64,
    passive: bool, intern: impl FnMut(&str) -> StrId) -> Result<Body, AdmitError>
{
    if !matches!(def.get("base").and_then(Value::as_str), Some("soldier" | "walkerdroid")) {
        return Err(error("unit class is not a soldier or droideka"));
    }
    if def.get("base").and_then(Value::as_str) == Some("soldier") && !actor.bones.iter().any(|b| b.name == "DummyRoot") {
        return Err(error("soldier animator has no DummyRoot bone"));
    }
    if def.get("HealthType").filter(|v| v.truthy()).is_some_and(|v| v.as_str().is_none()) {
        return Err(error("unit health type is not a string"));
    }
    let skeleton = units::body_skeleton(actor, ids.key.clone(), intern)?;
    let getup = shared.get("human_rifle_prone_getup_stand").map_or(0.0, |c| c.duration);
    let dive = 0.75 * shared.get("human_rifle_diveforward").map_or(0.0, |c| c.duration);
    let mut out = from_actor(def, actor, ids.clone(), ceiling, getup, dive, passive, skeleton)?;
    if out.unit.walker {
        let clips = walker_clips.ok_or_else(|| error("droideka class has no prepared clips"))?;
        let guns = bfe_content::derived::droideka_guns(actor, def.get("weapons"));
        let (muzzles, animator) = walker::build(actor, clips, &guns, def)?;
        let UnitBody::Droideka(body) = &mut out.unit.soldier else { unreachable!() };
        body.def = ids.class;
        body.muzzle_animator = Some(animator);
        let shield = def.get("weapons").and_then(Value::as_array).and_then(|ws| ws.iter()
            .find(|w| w.get("base").and_then(Value::as_str) == Some("shield")));
        if let Some(shield) = shield {
            let id = ids.shield.ok_or_else(|| error("droideka shield has no definition binding"))?;
            let max = truthy_number(shield, "MaxShield", 0.0);
            body.shield = Some(Shield { def: id, max, value: max, on: false,
                drain: number(shield, "AddShield", 0.0), refill: number(shield, "AddShieldOff", 0.0) });
        }
        out.muzzles = Some(muzzles);
    }
    Ok(out)
}

// Unit and CombatFacade's constructors. Keep absent flags as undefined: a
// cold body has not spawned, observed water or been given a network owner.
fn from_actor(def: &Value, actor: &Actor, ids: Bindings, ceiling: f64, getup: f64,
    dive: f64, passive: bool, skeleton: F3BodySkeleton) -> Result<Body, AdmitError>
{
    let walker = def.get("base").and_then(Value::as_str) == Some("walkerdroid");
    let soldier = if walker {
        let root = actor.bone_by_lower_name("dummyroot").map_or(-1.0, |i| i as f64);
        UnitBody::Droideka(crate::units::class::droideka_body(def, root, ceiling))
    } else {
        UnitBody::Soldier(crate::units::class::soldier_body(def, ceiling, getup, dive))
    };
    let health_type = def.get("HealthType").filter(|v| v.truthy()).and_then(Value::as_str).unwrap_or("person").to_lowercase();
    let category = match health_type.as_str() {
        "animal" => 1, "droid" => 2, "vehicle" => 3, "building" => 4, _ => 0,
    };
    let no_enter = bfe_content::js::parse_float_value(def.get("NoEnterVehicles"));
    let adoption = F3Adoption { key: ids.key.clone(), passive, category, has_actor: true,
        no_enter: if matches!(def.get("NoEnterVehicles"), None | Some(Value::Null)) { 0.0 } else { no_enter },
        droideka_water: walker.then(|| water(def)).flatten() };
    let max_health = truthy_number(def, "MaxHealth", 300.0);
    let chunks = if def.get("chunks").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
        && number(def, "ChunkFrequency", -1.0) > 0.0 { number(def, "ChunkFrequency", -1.0) } else { 0.0 };
    let mut bounds = Bounds::default();
    bounds.radius = 1.0; // Unit's coarse bound before the first shape query.
    let shapes = vec![HitShape { r#type: ShapeType::Sphere, part: ShapePart::Body, ..Default::default() }; 2];
    let unit = Unit { generation: ids.key.life, side: ids.side, class_name: ids.class_name, class_def: ids.class,
        walker, kind: if walker { 3.0 } else { 0.0 },
        hero: def.get("hero").filter(|v| v.truthy()).or_else(|| def.get("IsHero")).is_some_and(Value::truthy),
        removed: true, root_scale: number(def, "CollisionRootScale", 1.0),
        shapes, bounds, spheres_tick: -1.0, nav_radius: Some(nav_radius(actor)?),
        controls: Controls { was: [0.0; 2], pressed: vec![false; 2] },
        intent: Intent { aim: None, jet_direction: None, ..Default::default() },
        soldier, held: Nullable::Undefined, ..Default::default() };
    Ok(Body { unit, adoption, skeleton,
        combat: CombatInit { class: ids.class, max_health, chunks, scale: ids.health_scale }, muzzles: None })
}

/// UnitFactory.build uses the native actor's bindBounds (Float32 positions)
/// and Math.fround after Math.hypot. Preserve this rounding before AI reads it.
fn nav_radius(actor: &Actor) -> Result<f64, AdmitError> {
    if actor.positions.len() % 3 != 0 || actor.positions.is_empty() {
        return Err(error("unit actor has no packed positions"));
    }
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in actor.positions.chunks_exact(3) {
        for (i, axis) in [0, 2].into_iter().enumerate() {
            let v = p[axis] as f64;
            if !v.is_finite() { return Err(error("unit actor has an invalid position")); }
            lo[i] = lo[i].min(v); hi[i] = hi[i].max(v);
        }
    }
    Ok((0.25 * v8math::hypot(hi[0] - lo[0], hi[1] - lo[1])) as f32 as f64)
}

pub(super) fn number(def: &Value, key: &str, fallback: f64) -> f64 {
    match def.get(key) { None | Some(Value::Null) => fallback, Some(v) => bfe_content::js::to_number(Some(v)) }
}
pub(super) fn truthy_number(def: &Value, key: &str, fallback: f64) -> f64 {
    def.get(key).filter(|v| v.truthy()).map_or(fallback, |v| bfe_content::js::to_number(Some(v)))
}
// vehicle-water.js reads the first array entry before Number and keeps only
// finite numbers. Ball depth's absent default is 0, unlike upright's 1.5.
fn water(def: &Value) -> Option<DroidekaWater> {
    let num = |key, fallback| {
        let value = def.get(key);
        let value = match value { Some(Value::Array(a)) => a.first(), _ => value };
        let n = bfe_content::js::to_number(value);
        if n.is_finite() { n } else { fallback }
    };
    let amount = num("WaterDamageAmount", 0.0);
    (amount > 0.0).then(|| DroidekaWater { interval: num("WaterDamageInterval", 1.0), amount,
        upright: num("UprightWaterDamageHeight", 1.5), ball: num("BallWaterDamageHeight", 0.0) })
}
