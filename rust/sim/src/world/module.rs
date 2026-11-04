use super::separation::Separation;
use crate::abi::sim;
use crate::gen::{WsSeparation, WsSeparationResult};
use std::ptr::addr_of_mut;

static mut SEPARATION: Option<Separation> = None;

#[no_mangle]
pub extern "C" fn world_separation_words(which: u32) -> u32 {
    (match which { 0 => WsSeparation::WORDS, 1 => WsSeparationResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn world_separate() -> *const f64 {
    let sim = sim(); let (row, heap) = sim.calls.args();
    let input = WsSeparation::take(row, heap);
    // One module instance, one thread, no host callbacks in the crowd pass.
    let phase = unsafe { (*addr_of_mut!(SEPARATION)).get_or_insert_with(Separation::default) };
    let out = phase.run(input);
    sim.calls.result(WsSeparationResult::WORDS, |row, heap| out.put(row, heap));
    sim.calls.block.ptr()
}
