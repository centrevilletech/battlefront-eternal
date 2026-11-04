use super::{clock, events, separation::Separation};
use crate::abi::sim;
use crate::gen::{WsSeparation, WsSeparationResult, WorldClockInput, WorldClockResult, WorldClockState, WorldClockOperation, WorldClockCall, WorldClockArgs, import_world_clock};
use crate::gen::{WorldEventState, WorldEventInput, WorldEventOperation, WorldEventCall, WorldEventArgs, import_world_events};
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

struct ClockHost;
impl ClockHost {
    fn call(&mut self, state: &mut WorldClockState, op: WorldClockCall, dt: f64, tick: f64) -> f64 {
        // Only this stack-owned value spans a callback. The callback can
        // run another clock or any other phase on the same module.
        let reply = import_world_clock(&WorldClockArgs { op, state: state.clone(), dt, tick });
        *state = reply.state;
        reply.time
    }
}
impl clock::Host for ClockHost {
    fn now(&mut self, state: &mut WorldClockState) -> f64 { self.call(state, WorldClockCall::Now, 0.0, 0.0) }
    fn step(&mut self, state: &mut WorldClockState, dt: f64, tick: f64) { self.call(state, WorldClockCall::Step, dt, tick); }
}

#[no_mangle]
pub extern "C" fn world_clock_words(which: u32) -> u32 {
    (match which { 0 => WorldClockInput::WORDS, 1 => WorldClockResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn world_clock_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); WorldClockInput::take(row, heap) };
    let mut state = input.state;
    let result = match input.op {
        WorldClockOperation::Advance => clock::advance(&mut state, input.seconds, input.limited, &mut ClockHost),
        WorldClockOperation::Reset => { clock::reset(&mut state); Ok(0.0) }
        WorldClockOperation::Rebase => { clock::rebase(&mut state); Ok(0.0) }
    };
    let sim = sim();
    let (ran, error) = match result { Ok(ran) => (ran, None), Err(error) => (0.0, Some(sim.strings.intern_str(error))) };
    let out = WorldClockResult { state, ran, error };
    sim.calls.result(WorldClockResult::WORDS, |row, heap| out.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn world_clock_time(tick: f64, hz: f64) -> f64 { tick / hz }

struct EventHost;
impl events::Host for EventHost {
    fn call(&mut self, state: &mut WorldEventState, op: WorldEventCall, index: f64) -> bool {
        let reply = import_world_events(&WorldEventArgs { op, state: state.clone(), index });
        *state = reply.state;
        reply.present
    }
}

#[no_mangle]
pub extern "C" fn world_events_words(which: u32) -> u32 {
    (match which { 0 => WorldEventInput::WORDS, 1 => WorldEventState::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn world_events_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); WorldEventInput::take(row, heap) };
    let mut state = input.state;
    match input.op {
        WorldEventOperation::Emit => events::emit(&mut state, &mut EventHost),
        WorldEventOperation::Read => events::read(&mut state, &mut EventHost),
        WorldEventOperation::Reset => events::reset(&mut state, &mut EventHost),
    }
    let sim = sim();
    sim.calls.result(WorldEventState::WORDS, |row, heap| state.put(row, heap));
    sim.calls.block.ptr()
}
