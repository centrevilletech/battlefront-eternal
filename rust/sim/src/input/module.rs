use super::{actions, fire};
use crate::abi::sim;
use crate::gen::{IaInput, IaResult, IfInput, IfResult};

#[no_mangle]
pub extern "C" fn input_actions_words(which: u32) -> u32 {
    (match which { 0 => IaInput::WORDS, 1 => IaResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_actions_run() -> *const f64 {
    let sim = sim();
    let (row, heap) = sim.calls.args();
    let input = IaInput::take(row, heap);
    let result = actions::run(input, &mut sim.strings);
    sim.calls.result(IaResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_fire_words(which: u32) -> u32 {
    (match which { 0 => IfInput::WORDS, 1 => IfResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_fire_run() -> *const f64 {
    let sim = sim();
    let (row, heap) = sim.calls.args();
    let input = IfInput::take(row, heap);
    let result = fire::run(input, &mut sim.strings);
    sim.calls.result(IfResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}
