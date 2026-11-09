//! UnitFactory.prepare's links into the admission's one decoded catalog.
//! The same class objects reach combat, F3 services, body construction and
//! the network registry; a spawn never reloads or re-decodes its assets.
use super::{error, from_context, Body};
use crate::{gen::F3UnitKey, world::admit::{rows::{Call, Cx}, AdmitError}};
use bfe_content::{actor::Actor, clips::Clips, defs::ClassDef, Value};

pub struct Parts<'a> {
    pub actor: &'a Actor,
    pub shared: &'a Clips,
    pub walker: Option<&'a Clips>,
    pub def: &'a Value,
    pub side: &'a str,
    pub class: &'a ClassDef,
}

/// UnitFactory.sideOf/prepared: the first side carrying the class wins.
/// A class without an infantry body is refused before mutating an owner.
pub fn parts<'a>(cx: &Cx<'a>, name: &str) -> Result<Parts<'a>, AdmitError> {
    let (side,def)=cx.battle.sides.iter().find_map(|side| side.units.get(name).map(|def|(side,def)))
        .ok_or_else(||error(format!("no side has the class {name}")))?;
    if !matches!(def.get("base").and_then(Value::as_str),Some("soldier" | "walkerdroid")) {
        return Err(error("unit class is not a soldier or droideka"));
    }
    let class=cx.defs.class(&side.name,name).filter(|class|class.record.same(def))
        .ok_or_else(||error("unit class is outside the admission catalog"))?;
    let actor=class.body.and_then(|i|cx.content.actors.get(i)).map(|(_,actor)|actor)
        .ok_or_else(||error("unit class has no prepared body"))?;
    let shared=class.clips.and_then(|i|cx.content.body_clips.get(i)).map(|(_,clips)|clips.as_ref())
        .ok_or_else(||error("unit class has no prepared shared clips"))?;
    let walker=class.walker_clips.and_then(|i|cx.content.walker_clips.get(i)).map(|(_,clips)|clips);
    if def.get("base").and_then(Value::as_str)==Some("walkerdroid") && walker.is_none() {
        return Err(error("droideka has no prepared clip bank"));
    }
    Ok(Parts {actor,shared,walker,def,side:&side.name,class})
}

/// Combat bindings and interned names come from the same admission context.
pub fn body(cx: &mut Cx, parts: &Parts, _name: &str, key: F3UnitKey, passive: bool, call: &mut Call) -> Result<Body, AdmitError> {
    let content=cx.content;
    from_context(cx,content,parts.class,key,passive,call)
}
