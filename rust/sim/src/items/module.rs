//! The things lying about on the module: the exports JavaScript drives them
//! through once a world hands them over (src/items-sim.js), on the same
//! store, definitions and world as the rounds' (rust/sim/src/ordnance/
//! module.rs, whose note holds here: pointers, never a reference held
//! across an import, the call block for arguments and answers).

use crate::abi::{sim, Sim};
use crate::gen::*;
use crate::ordnance::defs::{Field, ItemDef};
use crate::ordnance::module::{call_fields, defs, field, parts, JsWorld};
use crate::ordnance::Cx;
use crate::store::{word_u32, DefId, Ref, StrId};

/// An item class's fields the Rust reads, in the order `items_def` takes
/// them (src/items-sim.js ITEM_FIELDS).
const ITEM_FIELDS: [&str; 12] = ["base", "Gravity", "Rebound", "Friction", "LifeSpan", "MaxHealth", "HealthType", "ExplosionDeath", "ExplosionTrigger", "ExplosionExpire", "TriggerRadius", "SuppressRadius"];

/// Registers item class `id` (DefTable order): its 12 fields (ITEM_FIELDS),
/// two words each, in the call block.
#[no_mangle]
pub extern "C" fn items_def(id: u32) {
    let s = sim();
    let fields: [Field; 12] = call_fields(&s.calls, &s.strings);
    let def = ItemDef::read(|name| fields[ITEM_FIELDS.iter().position(|f| *f == name).expect("an item field")].clone());
    // SAFETY: the ordnance module's note on its definitions.
    let d = unsafe { &mut *defs() };
    let i = id as usize;
    if d.item.len() <= i {
        d.item.resize(i + 1, def.clone());
    }
    d.item[i] = def;
}

/// The battle's item pools (items.js `pools`): `n` entries in the call
/// block, each the base's string, then its size as a field (two words).
#[no_mangle]
pub extern "C" fn items_pools(n: u32) {
    let s = sim();
    let (row, _) = s.calls.args();
    let pools = (0..n as usize)
        .map(|i| {
            let name = String::from_utf16_lossy(s.strings.units(StrId(word_u32(row[3 * i]))));
            (name, field(row[3 * i + 1], row[3 * i + 2], &s.strings))
        })
        .collect();
    // SAFETY: as items_def.
    unsafe { &mut *defs() }.pools = pools;
}

/// The words of a command's record in the call block: 0 SpawnArgs, 1
/// DispenseArgs, 2 DropArgs, 3 ItemsBlastArgs, 4 HurtArgs.
#[no_mangle]
pub extern "C" fn items_args_words(which: u32) -> u32 {
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
fn with_cx<T>(f: impl FnOnce(&mut Cx<'_, JsWorld<'_>>) -> T) -> T {
    let _owned_boundary = crate::units::f3::external();
    crate::units::f3::with_services(|recon| {
        let (store, strings) = parts();
        let mut world = JsWorld::with_services(store, strings,recon);
        // SAFETY: the ordnance module's note.
        let mut cx = unsafe { Cx::new(store, strings, defs(), &mut world) };
        f(&mut cx)
    })
}

/// items.js update: every item's tick, `targets` the number of targets the
/// battle gave (JavaScript keeps them: ItemsWorld.targets).
/// Whether the items' mirrors have anything to sync (1: items lie about or
/// were retired; 0: none, and the lists are as they were).
#[no_mangle]
pub extern "C" fn items_update(dt: f64, targets: u32) -> u32 {
    if crate::units::f3::active() { crate::units::f3_items::targets(targets); }
    with_cx(|cx| {
        cx.items_update(dt, targets as usize);
        let s = cx.s();
        u32::from(!s.items.items.is_empty() || !s.item.retired.is_empty() || !s.items.owned.is_empty())
    })
}

/// items.js spawn, its arguments (SpawnArgs) in the call block. The answer,
/// in the call block: KIND_ITEM and the item's row, or 0 for no room.
#[no_mangle]
pub extern "C" fn items_spawn() -> *const f64 {
    let a = {
        let (row, heap) = sim().calls.args();
        SpawnArgs::take(row, heap)
    };
    let made = with_cx(|cx| cx.spawn(a.def, a.position, a.velocity, a.yaw, a.owner, a.team));
    answer(made)
}

/// items.js dispense (DispenseArgs); the answer as items_spawn's.
#[no_mangle]
pub extern "C" fn items_dispense() -> *const f64 {
    let a = {
        let s = sim();
        let (row, heap) = s.calls.args();
        DispenseArgs::take(row, heap)
    };
    if crate::units::f3::active() {
        let ItemOwner::Unit(owner) = a.owner else { panic!("prepared owned dispenser source") };
        let life = sim().store.unit.get(owner).expect("dispenser owner").generation;
        let mut frame = crate::units::f3::items().take_name(owner.slot,life,a.weapon);
        let made = with_cx(|cx| cx.dispense_prepared(frame.key,&mut frame.work,&mut frame.spare,a.max_items,a.def,a.position,a.velocity,a.yaw,a.owner,a.team));
        crate::units::f3::items().restore(frame);
        return answer(made);
    }
    let weapon = String::from_utf16_lossy(sim().strings.units(a.weapon));
    let made = with_cx(|cx| cx.dispense(&weapon, a.max_items, a.def, a.position, a.velocity, a.yaw, a.owner, a.team));
    answer(made)
}

/// items.js dropFor (DropArgs); the answer as items_spawn's.
#[no_mangle]
pub extern "C" fn items_drop_for() -> *const f64 {
    let (a, drops) = {
        let s = sim();
        let a = {
            let (row, heap) = s.calls.args();
            DropArgs::take(row, heap)
        };
        let drops: Vec<super::Drop> = a.drops.iter().map(|d| super::Drop { def: d.def, probability: field(d.probability_tag as f64, d.probability, &s.strings) }).collect();
        (a, drops)
    };
    let made = with_cx(|cx| cx.drop_for(&drops, a.position, a.velocity, a.yaw));
    answer(made)
}

/// items.js blast (ItemsBlastArgs: the explosion's index).
#[no_mangle]
pub extern "C" fn items_blast() {
    let a = {
        let (row, heap) = sim().calls.args();
        ItemsBlastArgs::take(row, heap)
    };
    with_cx(|cx| cx.blast(DefId(a.explosion), a.position));
}

/// items.js hurt (HurtArgs).
#[no_mangle]
pub extern "C" fn items_hurt() {
    let a = {
        let (row, heap) = sim().calls.args();
        HurtArgs::take(row, heap)
    };
    with_cx(|cx| cx.hurt(a.item, a.amount));
}

/// An item made, or none: its kind and row into the call block.
fn answer(made: Option<Ref<Item>>) -> *const f64 {
    let s = sim();
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
    calls.block.ptr()
}
