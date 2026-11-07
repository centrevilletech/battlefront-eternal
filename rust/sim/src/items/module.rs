//! The things lying about on the module: the exports JavaScript drives them
//! through once a world hands them over (src/items-sim.js), on the same
//! store, definitions and world as the rounds' (rust/sim/src/ordnance/
//! module.rs): one root borrow, short row loans around callbacks, and
//! generated call blocks for arguments and answers.

use crate::abi::Sim;
use crate::gen::*;
use crate::ordnance::defs::{Field, ItemDef};
use crate::ordnance::module::{call_fields, defs_in, field, JsWorld};
use crate::ordnance::Cx;
use crate::store::{word_u32, DefId, Ref, StrId};

/// An item class's fields the Rust reads, in the order `items_def` takes
/// them (src/items-sim.js ITEM_FIELDS).
const ITEM_FIELDS: [&str; 12] = ["base", "Gravity", "Rebound", "Friction", "LifeSpan", "MaxHealth", "HealthType", "ExplosionDeath", "ExplosionTrigger", "ExplosionExpire", "TriggerRadius", "SuppressRadius"];

pub(crate) fn items_def_in(w: &mut crate::world::owner::World, id: u32) {
    let s = &mut w.sim;
    let fields: [Field; 12] = call_fields(&s.calls, &s.strings);
    let def = ItemDef::read(|name| fields[ITEM_FIELDS.iter().position(|f| *f == name).expect("an item field")].clone());
    let d = defs_in(&mut w.ordnance);
    let i = id as usize;
    if d.item.len() <= i {
        d.item.resize(i + 1, def.clone());
    }
    d.item[i] = def;
}

pub(crate) fn items_pools_in(w: &mut crate::world::owner::World, n: u32) {
    let s = &mut w.sim;
    let (row, _) = s.calls.args();
    let pools = (0..n as usize)
        .map(|i| {
            let name = String::from_utf16_lossy(s.strings.units(StrId(word_u32(row[3 * i]))));
            (name, field(row[3 * i + 1], row[3 * i + 2], &s.strings))
        })
        .collect();
    defs_in(&mut w.ordnance).pools = pools;
}

/// The words of a command's record in the call block: 0 SpawnArgs, 1
/// DispenseArgs, 2 DropArgs, 3 ItemsBlastArgs, 4 HurtArgs.
pub fn items_args_words(which: u32) -> u32 {
    (match which {
        0 => SpawnArgs::WORDS,
        1 => DispenseArgs::WORDS,
        2 => DropArgs::WORDS,
        3 => ItemsBlastArgs::WORDS,
        4 => HurtArgs::WORDS,
        _ => 0,
    }) as u32
}

/// Runs `f` on a context over the instance's store, definitions and world.
fn with_cx<T>(w: &mut crate::world::owner::World, f: impl FnOnce(&mut Cx<'_, JsWorld<'_, '_>>) -> T) -> T {
    let boundary = crate::units::f3::external_in(w);
    let result = crate::units::f3::with_services_in(w,|w,recon| {
        let mut world = JsWorld::with_services(w, recon);
        let mut cx = Cx::new(&mut world);
        let result = f(&mut cx);
        world.finish();
        result
    });
    boundary.finish(w,true);
    result
}

pub(crate) fn items_update_in(w: &mut crate::world::owner::World, dt: f64, targets: u32) -> u32 {
    if crate::units::f3::active() { crate::units::f3_items::targets(targets); }
    with_cx(w, |cx| {
        cx.items_update(dt, targets as usize);
        let s = cx.s();
        u32::from(!s.items.items.is_empty() || !s.item.retired.is_empty() || !s.items.owned.is_empty())
    })
}

pub(crate) fn items_spawn_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let a = {
        let (row, heap) = w.sim.calls.args();
        SpawnArgs::take(row, heap)
    };
    let made = with_cx(w, |cx| cx.spawn(a.def, a.position, a.velocity, a.yaw, a.owner, a.team));
    answer(&mut w.sim, made)
}

pub(crate) fn items_dispense_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let a = {
        let s = &mut w.sim;
        let (row, heap) = s.calls.args();
        DispenseArgs::take(row, heap)
    };
    if crate::units::f3::active() {
        let ItemOwner::Unit(owner) = a.owner else { panic!("prepared owned dispenser source") };
        let life = w.sim.store.unit.get(owner).expect("dispenser owner").generation;
        let mut frame = crate::units::f3::items().take_name(owner.slot,life,a.weapon);
        let made = with_cx(w, |cx| cx.dispense_prepared(frame.key,&mut frame.work,&mut frame.spare,a.max_items,a.def,a.position,a.velocity,a.yaw,a.owner,a.team));
        crate::units::f3::items().restore(frame);
        return answer(&mut w.sim, made);
    }
    let weapon = String::from_utf16_lossy(w.sim.strings.units(a.weapon));
    let made = with_cx(w, |cx| cx.dispense(&weapon, a.max_items, a.def, a.position, a.velocity, a.yaw, a.owner, a.team));
    answer(&mut w.sim, made)
}

pub(crate) fn items_drop_for_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let (a, drops) = {
        let s = &mut w.sim;
        let a = {
            let (row, heap) = s.calls.args();
            DropArgs::take(row, heap)
        };
        let drops: Vec<super::Drop> = a.drops.iter().map(|d| super::Drop { def: d.def, probability: field(d.probability_tag as f64, d.probability, &s.strings) }).collect();
        (a, drops)
    };
    let made = with_cx(w, |cx| cx.drop_for(&drops, a.position, a.velocity, a.yaw));
    answer(&mut w.sim, made)
}

pub(crate) fn items_blast_in(w: &mut crate::world::owner::World) {
    let a = {
        let (row, heap) = w.sim.calls.args();
        ItemsBlastArgs::take(row, heap)
    };
    with_cx(w, |cx| cx.blast(DefId(a.explosion), a.position));
}

pub(crate) fn items_hurt_in(w: &mut crate::world::owner::World) {
    let a = {
        let (row, heap) = w.sim.calls.args();
        HurtArgs::take(row, heap)
    };
    with_cx(w, |cx| cx.hurt(a.item, a.amount));
}

/// An item made, or none: its kind and row into the call block.
fn answer(s: &mut Sim, made: Option<Ref<Item>>) -> Option<&[f64]> {
    let Sim { store, calls, .. } = s;
    match made {
        None => calls.result(1, |row, _| row[0] = 0.0),
        Some(r) => calls.result(1 + Item::EXPORT_STRIDE, |row, heap| {
            let v = store.item.get(r).or_else(|| store.item.retired.iter().rev().find(|(x, _)| *x == r).map(|(_, v)| v)).expect("an item the battle holds");
            row[0] = KIND_ITEM as f64;
            row[1] = r.slot as f64;
            row[2] = r.gen as f64;
            v.put_export(&mut row[1..], heap);
        }),
    }
    Some(&calls.block.words[..])
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    /// Registers item class `id` (DefTable order): its 12 fields (ITEM_FIELDS),
    /// two words each, in the call block.
    #[no_mangle]
    pub extern "C" fn items_def(id: u32) { items_def_in(crate::abi::facade::world(), id) }
    /// The battle's item pools (items.js `pools`): `n` entries in the call
    /// block, each the base's string, then its size as a field (two words).
    #[no_mangle]
    pub extern "C" fn items_pools(n: u32) { items_pools_in(crate::abi::facade::world(), n) }
    #[no_mangle]
    pub extern "C" fn items_args_words(which: u32) -> u32 { super::items_args_words(which) }
    /// items.js update: every item's tick, `targets` the number of targets the
    /// battle gave (JavaScript keeps them: ItemsWorld.targets).
    /// Whether the items' mirrors have anything to sync (1: items lie about or
    /// were retired; 0: none, and the lists are as they were).
    #[no_mangle]
    pub extern "C" fn items_update(dt: f64, targets: u32) -> u32 { items_update_in(crate::abi::facade::world(), dt, targets) }
    /// items.js spawn, its arguments (SpawnArgs) in the call block. The answer,
    /// in the call block: KIND_ITEM and the item's row, or 0 for no room.
    #[no_mangle]
    pub extern "C" fn items_spawn() -> *const f64 { items_spawn_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    /// items.js dispense (DispenseArgs); the answer as items_spawn's.
    #[no_mangle]
    pub extern "C" fn items_dispense() -> *const f64 { items_dispense_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    /// items.js dropFor (DropArgs); the answer as items_spawn's.
    #[no_mangle]
    pub extern "C" fn items_drop_for() -> *const f64 { items_drop_for_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    /// items.js blast (ItemsBlastArgs: the explosion's index).
    #[no_mangle]
    pub extern "C" fn items_blast() { items_blast_in(crate::abi::facade::world()) }
    /// items.js hurt (HurtArgs).
    #[no_mangle]
    pub extern "C" fn items_hurt() { items_hurt_in(crate::abi::facade::world()) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{items_def, items_pools, items_update, items_spawn, items_dispense, items_drop_for, items_blast, items_hurt};
