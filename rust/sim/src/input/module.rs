use super::{actions, fire, mailbox, controls};
use crate::abi::sim;
use crate::gen::{IaInput, IaResult, IfInput, IfResult, ImInput, ImResult, IqInput, IqResult, IcInput, IcResult, IkInput};

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

#[no_mangle]
pub extern "C" fn input_mailbox_words(which: u32) -> u32 {
    (match which { 0 => ImInput::WORDS, 1 => ImResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_mailbox_run() -> *const f64 {
    let sim = sim();
    let (row, heap) = sim.calls.args();
    let input = ImInput::take(row, heap);
    let result = mailbox::run(input, &mut sim.strings);
    sim.calls.result(ImResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_queue_words(which: u32) -> u32 {
    (match which { 0 => IqInput::WORDS, 1 => IqResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_queue_run() -> *const f64 {
    let sim = sim(); let (row, heap) = sim.calls.args();
    let result = controls::queue(IqInput::take(row, heap), &mut sim.strings);
    sim.calls.result(IqResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_controls_words(which: u32) -> u32 {
    (match which { 0 => IcInput::WORDS, 1 => IcResult::WORDS, 2 => IkInput::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_controls_run() -> *const f64 {
    let sim = sim(); let (row, heap) = sim.calls.args();
    let result = controls::controls(IcInput::take(row, heap));
    sim.calls.result(IcResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_controls_axis() -> f64 {
    let (row, heap) = sim().calls.args();
    let input = IkInput::take(row, heap);
    controls::axis(&input.down, &input.plus, &input.minus)
}
