//! Instance-local WASM cells. Pointer discovery borrows cold inline banks;
//! only the existing Sim entry admits the heap-backed exchange resources.
use crate::{store::calls::ImportExchange, world::owner::World};
use std::ptr::addr_of_mut;

static mut WORLD: World = World::COLD;
static mut EXCHANGE: ImportExchange = ImportExchange::new();

pub(crate) fn cold() -> &'static mut World {
    // One WASM instance runs on one thread; no shared native world uses this cell.
    unsafe { &mut *addr_of_mut!(WORLD) }
}
pub(crate) fn world() -> &'static mut World {
    let world = cold();
    world.warm();
    world
}
pub(crate) fn exchange() -> &'static mut ImportExchange {
    // The argument loan ends before dispatch, so recursive imports reuse the bank.
    unsafe { &mut *addr_of_mut!(EXCHANGE) }
}
