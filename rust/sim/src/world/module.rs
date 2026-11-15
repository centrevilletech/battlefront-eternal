use super::{clock, local};
use crate::gen::{WorldClockInput, WorldClockResult, WorldClockState, WorldClockOperation, WorldClockCall, WorldClockArgs, import_world_clock};
use crate::gen::{WorldEventState, WorldEventInput};
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

pub fn world_events_words(which: u32) -> u32 {
    (match which { 0 => WorldEventInput::WORDS, 1 => WorldEventState::WORDS, _ => 0 }) as u32
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

