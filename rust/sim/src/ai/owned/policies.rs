//! weaponAI previews read the same cooked content as native bot targeting.
//! This is a cold publication; it adds no controller or per-tick crossing.
use crate::{world::owner::World,gen::{AiWeaponPolicy,AiWeaponPolicies,AiWeaponPolicyContent,AiWeaponPolicySource},store::DefId};
use crate::ai::combat::weapons_ai::WeaponAiTable;

/// A page has no AI battle. Cook its preview masks through the same content
/// parser and table as bots, without creating controllers or a second world.
pub fn ai_weapon_policies_in(w:&mut World)->*const f64 {
    let source={let(row,heap)=w.sim.calls.args();AiWeaponPolicySource::take(row,heap)};
    let definitions=&crate::combat::module::defs_in(&mut w.combat).weapons;
    if source.weapons.iter().any(|id|!definitions.has(*id)) {return std::ptr::null();}
    let mut table=WeaponAiTable::default();
    for data in source.data {
        let Ok(records)=super::load::weapon_records(&data.json) else{return std::ptr::null()};
        table.register(&records);
    }
    if crate::units::f3::active_in(&w.units) {complete_bindings(w,&table);}
    publish(w,&table,source.weapons.into_iter())
}

pub fn ai_world_weapon_policies_in(w:&mut World)->*const f64 {
    let Some(battle)=w.ai.battle() else{return std::ptr::null()};
    if battle.ready() {return std::ptr::null();}
    let table=battle.combat.shared.weapons.clone();
    // Initial physical owners can precede AI content loading. Complete their
    // immutable lock bindings now, without resetting weapons or RNG streams.
    complete_bindings(w,&table);
    let definitions=crate::combat::module::defs_in(&mut w.combat);
    let ids=definitions.weapons.classes.iter().enumerate()
        .filter_map(|(index,class)|class.as_ref().map(|_|DefId(index as u32)));
    publish(w,&table,ids)
}

fn complete_bindings(w:&mut World,table:&WeaponAiTable) {
    crate::units::f3::refresh_weapon_masks_in(w,table);
    crate::vehicles::module::f2::firing::refresh_weapon_masks_in(w,table);
    crate::vehicles::module::f2::live_seats::refresh_weapon_masks_in(w,table);
}

fn publish(w:&mut World,table:&WeaponAiTable,ids:impl Iterator<Item=DefId>)->*const f64 {
    let definitions=crate::combat::module::defs_in(&mut w.combat);
    let weapons=ids.map(|id| {
        let class=definitions.weapons.class(id);
        let levels=class.levels.iter().map(|level| {
            let ai=table.weapon_ai(&level.obj);
            AiWeaponPolicy {min_range:ai.min_range,optimal_range:ai.optimal_range,max_range:ai.max_range,implicit_range:ai.implicit_range,
                extreme_range:ai.extreme_range,relation_mask:ai.relation_mask,category_mask:ai.category_mask,
                barrage_min:ai.barrage_min,barrage_max:ai.barrage_max,barrage_delay:ai.barrage_delay,trigger_single:ai.trigger_single,
                zoom_max:ai.zoom_max,action:ai.action}
        }).collect();
        AiWeaponPolicies {weapon:id,levels}
    }).collect();
    let out=AiWeaponPolicyContent {weapons};
    w.sim.calls.result(AiWeaponPolicyContent::WORDS,|row,heap|out.put(row,heap));
    w.sim.calls.block.ptr()
}

// Facade shims for the cold policy boundary.
#[no_mangle]
pub extern "C" fn ai_weapon_policies()->*const f64 {ai_weapon_policies_in(crate::abi::world())}
#[no_mangle]
pub extern "C" fn ai_world_weapon_policies()->*const f64 {ai_world_weapon_policies_in(crate::abi::world())}
