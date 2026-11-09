//! ItemsSim's item definitions and pool rows (items-sim.js).
//!
//! Side drops, dispensers and placed items all enter the same identity table.
//! A caller visits these at its construction seam, so shared definitions keep
//! one handle while equal but independently authored records remain distinct.
use super::combat::{Definitions, ExportCall};
use super::ordnance::{OrdnanceRows, Prepared};
use crate::store::{DefId, Strings};
use crate::world::admit::{AdmitError, Stage};
use bfe_content::battle::Battle;
use bfe_content::value::{Object, Value};
use std::rc::Rc;

pub const ITEM_FIELDS: &[&str] = &["base", "Gravity", "Rebound", "Friction", "LifeSpan", "MaxHealth", "HealthType", "ExplosionDeath", "ExplosionTrigger", "ExplosionExpire", "TriggerRadius", "SuppressRadius"];

#[derive(Default)]
pub struct ItemRows {
    pub definitions: Definitions,
    pub calls: Vec<ExportCall>,
    sent: usize,
}
impl ItemRows {
    // AI content may have reserved more ids than ItemsSim has published.
    // Retain the sent prefix explicitly so those classes still reach F1.
    pub fn from_definitions(definitions:Definitions,sent:usize)->Result<Self,AdmitError> {
        if sent>definitions.values.len() {return Err(AdmitError::Content {stage:Stage::Store,message:"sent item count exceeds its table".into()});}
        Ok(Self {definitions,calls:Vec::new(),sent})
    }
    // This count tracks native definitions, independently of the id table.
    // Cx carries it between AI fact gathering and later body construction.
    pub fn sent_count(&self)->usize {self.sent}
    /// ItemsSim.defId drains reserved handles just as OrdnanceSim does.
    /// Explosion calls made while encoding the fields precede items_def.
    pub fn definition(&mut self, def: &Value, rounds: &mut OrdnanceRows, strings: &mut Strings) -> Result<DefId, AdmitError> {
        self.definition_with(def, rounds, strings, &mut |_,_,_,_,_| Ok(()))
    }
    pub fn definition_with(&mut self, def: &Value, rounds: &mut OrdnanceRows, strings: &mut Strings, prepared: &mut impl FnMut(Prepared, DefId, &Value, &mut Strings, &mut Vec<ExportCall>) -> Result<(),AdmitError>) -> Result<DefId, AdmitError> {
        // The requested item can already be reserved. The host still sends
        // all pending predecessors before using that item's handle.
        let id = self.definitions.id(def, Stage::Store)?;
        while self.sent < self.definitions.values.len() {
            let next = self.sent;
            self.sent += 1;
            let def = self.definitions.values[next].clone();
            let row = rounds.fields_with(&def, ITEM_FIELDS, strings, prepared)?;
            // Nested explosion and service calls belong before this item,
            // including when several reserved item handles drain together.
            self.calls.append(&mut rounds.calls);
            self.calls.push(ExportCall::new("items_def", vec![next as f64], row).with_strings(strings));
            prepared(Prepared::Item, DefId(next as u32), &def, strings, &mut self.calls)?;
        }
        Ok(id)
    }
}

/// Items constructor's `{...POOLS, ...pools}` followed by ItemsSim.sendPools.
/// Existing defaults keep their place when a mission overrides them; numeric
/// keys still sort before text keys according to Object.entries semantics.
pub fn pools(mission: Option<&Value>, rounds: &mut OrdnanceRows, strings: &mut Strings) -> Result<ExportCall, AdmitError> {
    let mut object = Object::from_written(vec![
        (Rc::from("powerupitem"), Value::Number(25.0)),
        (Rc::from("mine"), Value::Number(50.0)),
    ]);
    if let Some(authored) = mission.and_then(|v| v.get("pools")).and_then(Value::as_object) {
        for (key, value) in authored.entries() { object.set(Rc::from(key), value.clone()); }
    }
    let mut row = Vec::with_capacity(object.len() * 3);
    for (base, size) in object.entries() {
        row.push(strings.intern_str(base).0 as f64);
        row.extend(rounds.field(Some(size), strings)?);
    }
    Ok(ExportCall::new("items_pools", vec![object.len() as f64], row).with_strings(strings))
}

/// The side and placed item definitions are kept as references to the loaded
/// Battle; no JSON round trip can accidentally merge or duplicate identities.
/// This catalog does not allocate handles: each owner requests those lazily.
// The caller's actual use order assigns handles. In particular, a placed
// item and a same-named side item can be independent source allocations.
pub fn definitions(battle: &Battle) -> Vec<&Value> {
    let mut out = Vec::new();
    for side in &battle.sides {
        if let Some(items) = side.items.as_object() { out.extend(items.values()); }
    }
    if let Some(placed) = battle.props.as_ref().and_then(|v| v.get("items")).and_then(Value::as_array) {
        for entry in placed {
            if let Some(item) = entry.get("item").filter(|v| v.truthy()) { out.push(item); }
        }
    }
    out
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::ordnance::defs::Field;
    use crate::world::owner::World;

    #[test]
    fn tagged_item_rows_are_read_by_the_native_owner() {
        let definition = bfe_content::json::parse(r#"{"base":"mine","Gravity":-0,"LifeSpan":null,"MaxHealth":300,"ExplosionExpire":{"Damage":50}}"#).unwrap();
        let mut items=ItemRows::default();
        let mut rounds=OrdnanceRows::default();
        let mut strings=Strings::new();
        let pool_call=pools(None,&mut rounds,&mut strings).unwrap();
        items.definition(&definition,&mut rounds,&mut strings).unwrap();
        let mut world=World::new();
        world.sim.strings=strings;
        for call in rounds.calls.into_iter().chain(items.calls).chain(std::iter::once(pool_call)) {
            world.sim.calls.result(call.call.row.len(),|row,_|row.copy_from_slice(&call.call.row));
            match call.name {
                "ord_explosion" => crate::ordnance::module::ord_explosion_in(&mut world,call.call.args[0] as u32),
                "items_def" => crate::items::module::items_def_in(&mut world,call.call.args[0] as u32),
                "items_pools" => crate::items::module::items_pools_in(&mut world,call.call.args[0] as u32),
                _ => panic!("unexpected item construction call"),
            }
        }
        let defs=crate::ordnance::module::defs_in(&mut world.ordnance);
        assert_eq!(defs.item.len(),1);
        assert_eq!(defs.pools.len(),2);
        assert_eq!(defs.pools[0],("powerupitem".into(),Field::Num(25.0)));
        assert_eq!(defs.item[0].base,Field::Str("mine".into()));
        assert_eq!(defs.item[0].life_span,Field::Null);
        assert_eq!(defs.item[0].explosion_expire,Field::Explosion(DefId(0)));
        let Field::Num(gravity)=defs.item[0].gravity else {panic!("numeric gravity");};
        assert_eq!(gravity.to_bits(),(-0.0f64).to_bits());
    }
}

/// attachItemsSim sends the constructor's pools before any placed item exists.
pub fn build(cx:&mut super::Cx)->Result<super::Calls,AdmitError> {
    let mission=cx.battle.world.get("mission").cloned();
    let (_,calls)=super::combat::with_bindings(cx,Stage::Store,|bindings,strings| {
        // Authored object-valued pools follow fieldWords too, so preserve
        // any nested definition calls ahead of the final pool record.
        let pool=pools(mission.as_ref(),&mut bindings.ordnance,strings)?;
        let mut calls=std::mem::take(&mut bindings.ordnance.calls);calls.push(pool);
        Ok(((),calls))
    })?;
    Ok(calls)
}

/// spawnPlacedItems (src/placed-items.js), run by the headless host right
/// after attachF3: every power-up the map places (props.json `items`, an
/// entry with an item definition and a 16-element matrix) is spawned as an
/// ordinary item at the matrix's translation, heading along its forward
/// axis, at rest, with the entry's team. ItemsWorld.spawn first asks F1 for
/// the definition's id (items_def, and F3's item class through F1's service
/// observer), then the command runs: nativeServices.beforeStep publishes
/// the registry's foreign rows (f3_foreign, when units or buildings exist)
/// before items_spawn reads the SpawnArgs block.
pub fn placed(cx: &mut super::Cx<'_>) -> Result<super::Calls, AdmitError> {
    use crate::gen::{ItemOwner, SpawnArgs};
    let Some(placed) = cx.battle.props.as_ref().and_then(|props| props.get("items")).and_then(Value::as_array).map(<[Value]>::to_vec) else { return Ok(Vec::new()); };
    let content = cx.content;
    let buildings = cx.ids.props.len();
    let mut calls = Vec::new();
    for entry in &placed {
        let Some(def) = entry.get("item").filter(|v| v.truthy()) else { continue; };
        let Some(matrix) = entry.get("matrix").and_then(Value::as_array).filter(|m| m.len() == 16) else { continue; };
        let m: Vec<f64> = matrix.iter().map(|v| bfe_content::js::to_number(Some(v))).collect();
        let mut groups = super::units::load_groups(cx)?;
        let (id, registered) = super::combat::with_bindings(cx, Stage::Units, |bindings, strings| {
            let groups = &mut groups; let combat = &mut bindings.combat;
            let id = bindings.items.definition_with(def, &mut bindings.ordnance, strings, &mut |kind, id, def, strings, calls| super::units::prepared(content, groups, combat, kind, id, def, strings, calls))?;
            Ok((id, std::mem::take(&mut bindings.items.calls)))
        })?;
        super::units::save_groups(cx, &groups);
        calls.extend(registered);
        let team = entry.get("team").filter(|v| v.truthy()).map_or(0.0, |v| bfe_content::js::to_number(Some(v)));
        let args = SpawnArgs { def: id, position: [m[12], m[13], m[14]], velocity: [0.0; 3],
            yaw: v8math::atan2(m[8], m[10]), owner: ItemOwner::Null, team };
        let block = super::units::call(SpawnArgs::WORDS, |row, heap| args.put(row, heap));
        // ItemsWorld.command writes the SpawnArgs block, then beforeStep
        // publishes the registry's foreign rows (F3Registry.foreign: nothing
        // without a unit row or a building; no unit has a row before the
        // roster), then items_spawn reads the block. The block therefore
        // travels with f3_foreign when that call is made: it stays in the
        // exchange until items_spawn reads it.
        if buildings != 0 {
            calls.push((Stage::Units, "f3_foreign", super::Call { args: vec![0.0], ..block }));
            calls.push((Stage::Units, "items_spawn", super::Call::default()));
        } else {
            calls.push((Stage::Units, "items_spawn", block));
        }
    }
    Ok(calls)
}
