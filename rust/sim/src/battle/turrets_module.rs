//! Turret aim/rig commands. The portable implementation is shared with
//! replay and the turret crews; only configuration storage is module-only.
use crate::abi::sim;
use crate::gen::{PtConfiguration, PtInput, PtResult, PtSettings};
use std::{ptr::addr_of_mut, rc::Rc};
static mut CONFIG: Vec<Rc<PtSettings>> = Vec::new();
fn config() -> &'static mut Vec<Rc<PtSettings>> { unsafe { &mut *addr_of_mut!(CONFIG) } }
#[no_mangle]
pub extern "C" fn turret_words(which: u32) -> u32 { (match which { 0 => PtConfiguration::WORDS, 1 => PtInput::WORDS, 2 => PtResult::WORDS, _ => 0 }) as u32 }
#[no_mangle]
pub extern "C" fn turret_configure() {
    let (row, heap) = sim().calls.args(); let input = PtConfiguration::take(row, heap);
    *config() = input.turrets.into_iter().map(Rc::new).collect();
}
#[no_mangle]
pub extern "C" fn turret_run() -> *const f64 {
    let (row, heap) = sim().calls.args(); let input = PtInput::take(row, heap);
    let settings = config()[input.state.index as usize].clone(); let result = super::turrets::run(input, &settings);
    let sim = sim(); sim.calls.result(PtResult::WORDS, |row, heap| result.put(row, heap)); sim.calls.block.ptr()
}
