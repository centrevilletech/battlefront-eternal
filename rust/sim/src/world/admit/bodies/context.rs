//! Spawn construction through the enclosing admission context. Definition
//! ids and strings come from preceding owners; a body never creates a new
//! independent class table or reloads assets to derive another identity.
use super::{build, error, Bindings, Body};
use crate::{gen::F3UnitKey, world::admit::{rows::{Call, Cx}, AdmitError}};
use bfe_content::{content::Content, defs::ClassDef, Value};

/// Build one body's facts through Cx. `key` is the registry/slab identity
/// allocated at this construction point, including the temporary stock slot.
/// The enclosing call retains every new intern before installing the body.
pub fn from_context(cx: &mut Cx, content: &Content, class: &ClassDef,
    key: F3UnitKey, passive: bool, call: &mut Call) -> Result<Body, AdmitError>
{
    if !std::ptr::eq(cx.battle, &content.battle) {
        return Err(error("spawn content must share the admission battle"));
    }
    // ClassDef's Name belongs to Defs, not the simulation's string table.
    // Aliases share the combat definition but retain their own factory pool
    // and Unit.className; selecting the first matching object loses that name.
    let defs=cx.defs;
    let class=defs.classes.iter().find(|candidate|candidate.name==class.name &&
        candidate.side==class.side && candidate.record.same(&class.record))
        .ok_or_else(||error("spawn class is outside the admission catalog"))?;
    let side=defs.names.text(class.side);
    let name=defs.names.text(class.name);
    let shield = class.record.get("weapons").and_then(Value::as_array).and_then(|weapons| weapons.iter()
        .find(|weapon| weapon.get("base").and_then(Value::as_str) == Some("shield")));
    let ((class_id, shield), _) = crate::world::admit::rows::combat::with_bindings(cx, crate::world::admit::Stage::Player, |bindings, _| {
        let class_id = bindings.combat.classes.get(&class.record).ok_or_else(|| error("spawn class has no earlier combat binding"))?;
        let shield = shield.map(|def| bindings.combat.weapons.get(def).ok_or_else(|| error("spawn shield has no earlier weapon binding"))).transpose()?;
        Ok(((class_id, shield), Vec::new()))
    })?;
    let scale = health_scale(&class.record);
    let ids = Bindings { key, side: cx.intern(call, side), class_name: cx.intern(call, name),
        class: class_id, health_scale: cx.intern(call, scale), shield };
    let ceiling = content.battle.world.get("mission").map_or(200., |mission| match mission.get("maxFlyHeight") {
        None | Some(Value::Null) => 200., value => bfe_content::js::to_number(value),
    });
    build(content, class, ids, ceiling, passive, |name| cx.intern(call, name))
}

/// CombatFacade's HEALTH_SCALES lookup. Unknown health types retain the
/// PersonScale default, independently of targeting's category fallback.
pub fn health_scale(def: &Value) -> &'static str {
    match def.get("HealthType").filter(|v| v.truthy()).and_then(Value::as_str).unwrap_or("person").to_lowercase().as_str() {
        "animal" => "AnimalScale", "droid" => "DroidScale", "vehicle" => "VehicleScale",
        "building" => "BuildingScale", "shield" => "ShieldScale", _ => "PersonScale",
    }
}
