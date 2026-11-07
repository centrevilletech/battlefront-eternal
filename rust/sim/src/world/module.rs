use super::{clock, events, local};
use crate::gen::{WorldClockInput, WorldClockResult, WorldClockState, WorldClockOperation, WorldClockCall, WorldClockArgs, import_world_clock};
use crate::gen::{WorldEventState, WorldEventInput, WorldEventOperation, WorldEventCall, WorldEventArgs, import_world_events};
use crate::gen::{WorldLocalState, WorldLocalInput, WorldLocalOperation, WorldLocalCall, WorldLocalArgs, import_world_local};

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
pub fn world_clock_words(which: u32) -> u32 {
    (match which { 0 => WorldClockInput::WORDS, 1 => WorldClockResult::WORDS, _ => 0 }) as u32
}

pub(crate) fn world_clock_run_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let input = { let (row, heap) = w.sim.calls.args(); WorldClockInput::take(row, heap) };
    let mut state = input.state;
    let result = match input.op {
        WorldClockOperation::Advance => clock::advance(&mut state, input.seconds, input.limited, &mut ClockHost),
        WorldClockOperation::Reset => { clock::reset(&mut state); Ok(0.0) }
        WorldClockOperation::Rebase => { clock::rebase(&mut state); Ok(0.0) }
    };
    let sim = &mut w.sim;
    let (ran, error) = match result { Ok(ran) => (ran, None), Err(error) => (0.0, Some(sim.strings.intern_str(error))) };
    let out = WorldClockResult { state, ran, error };
    sim.calls.result(WorldClockResult::WORDS, |row, heap| out.put(row, heap));
    Some(&sim.calls.block.words[..])
}
pub fn world_clock_time(tick: f64, hz: f64) -> f64 { tick / hz }

struct EventHost;
impl events::Host for EventHost {
    fn call(&mut self, state: &mut WorldEventState, op: WorldEventCall, index: f64) -> bool {
        let reply = import_world_events(&WorldEventArgs { op, state: state.clone(), index });
        *state = reply.state;
        reply.present
    }
}
pub fn world_events_words(which: u32) -> u32 {
    (match which { 0 => WorldEventInput::WORDS, 1 => WorldEventState::WORDS, _ => 0 }) as u32
}

pub(crate) fn world_events_run_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let input = { let (row, heap) = w.sim.calls.args(); WorldEventInput::take(row, heap) };
    let mut state = input.state;
    match input.op {
        WorldEventOperation::Emit => events::emit(&mut state, &mut EventHost),
        WorldEventOperation::Read => events::read(&mut state, &mut EventHost),
        WorldEventOperation::Reset => events::reset(&mut state, &mut EventHost),
    }
    let sim = &mut w.sim;
    sim.calls.result(WorldEventState::WORDS, |row, heap| state.put(row, heap));
    Some(&sim.calls.block.words[..])
}

struct LocalHost;
impl local::Host for LocalHost {
    fn call(&mut self, state: &mut WorldLocalState, op: WorldLocalCall, dt: f64) -> bool {
        let reply = import_world_local(&WorldLocalArgs { op, state: state.clone(), dt });
        *state = reply.state;
        reply.value
    }
}
pub fn world_local_words(which: u32) -> u32 {
    (match which { 0 => WorldLocalInput::WORDS, 1 => WorldLocalState::WORDS, _ => 0 }) as u32
}

pub(crate) fn world_local_run_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let input = { let (row, heap) = w.sim.calls.args(); WorldLocalInput::take(row, heap) };
    let mut state = input.state;
    match input.op {
        WorldLocalOperation::Tail => local::tail(&mut state, input.dt, &mut LocalHost),
    }
    let sim = &mut w.sim;
    sim.calls.result(WorldLocalState::WORDS, |row, heap| state.put(row, heap));
    Some(&sim.calls.block.words[..])
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn world_clock_words(which: u32) -> u32 { super::world_clock_words(which) }
    #[no_mangle]
    pub extern "C" fn world_clock_time(tick: f64, hz: f64) -> f64 { super::world_clock_time(tick, hz) }
    #[no_mangle]
    pub extern "C" fn world_events_words(which: u32) -> u32 { super::world_events_words(which) }
    #[no_mangle]
    pub extern "C" fn world_local_words(which: u32) -> u32 { super::world_local_words(which) }
    #[no_mangle]
    pub extern "C" fn world_clock_run() -> *const f64 { world_clock_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn world_events_run() -> *const f64 { world_events_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn world_local_run() -> *const f64 { world_local_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
