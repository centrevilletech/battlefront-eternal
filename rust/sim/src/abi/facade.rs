//! Instance-local WASM cells. Pointer discovery borrows cold inline banks;
//! only the existing Sim entry admits the heap-backed exchange resources.
use crate::{store::calls::ImportExchange, world::owner::World};
use std::ptr::addr_of_mut;

static mut WORLD: World = World::COLD;
static mut EXCHANGE: ImportExchange = ImportExchange::new();

pub(crate) fn cold() -> &'static mut World {
    // SAFETY: each cell belongs to one single-threaded WASM instance.
    // Callers end Rust loans before callbacks or another export can reenter.
    unsafe { &mut *addr_of_mut!(WORLD) }
}
pub(crate) fn world() -> &'static mut World {
    let world = cold();
    world.warm();
    world
}
pub(crate) fn exchange() -> &'static mut ImportExchange {
    // SAFETY: the argument loan ends before dispatch. Recursive imports
    // reuse this instance-local bank only after that loan has ended.
    unsafe { &mut *addr_of_mut!(EXCHANGE) }
}
