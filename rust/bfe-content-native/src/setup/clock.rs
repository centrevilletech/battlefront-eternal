//! The host's world clock as WorldTick (src/runtime/world-tick.js) stamps it
//! into every WorldTickInput and copies it back from every WorldTickOutput.
//! The world itself advances the clock inside its step (World::step runs
//! FixedClock's compensated advance once a tick); the host only keeps the
//! completed state between ticks and hands it back with the next row.
use bfe_sim::gen::{WorldClockState, WorldTickInput, WorldTickOutput};
use bfe_sim::world::clock;

/// game.js: `new FixedClock({ budget: 0.035, maxDebt: 8 })`, 60 Hz and
/// maxSteps 8 by default, every running field zero from `reset()`.
pub fn battle_clock() -> WorldClockState {
    WorldClockState { hz: 60.0, dt: 1.0 / 60.0, max_steps: 8.0, budget: 0.035, max_debt: 8.0, ..Default::default() }
}

/// One battle's tick rows: the clock between ticks and the row for the next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickRows {
    pub clock: WorldClockState,
}
impl TickRows {
    pub fn new() -> Self {
        Self { clock: battle_clock() }
    }
    /// The tick the next row steps: game.step requires `command.tick ===
    /// worldClock.tick + 1`, and FixedClock.advance steps `this.tick + 1`.
    pub fn next_tick(&self) -> f64 {
        self.clock.tick + 1.0
    }
    /// WorldTick.run: dt is the clock's, tick the next, the clock the one
    /// completed by the previous tick; the rest are the host's facts for
    /// this tick (the foreign unit rows written, whether the AI and live
    /// owners attached, the aim camera handle, whether the AI has no
    /// cameras of its own, the squad orders pressed, the host's wall time
    /// for the tick in milliseconds and the world's mouse setting when a
    /// player's command names none; WorldTick.run's defaults for the last
    /// two are 0 and 1: a harness keeps no wall clock for the players'
    /// stale-input rule, and its players name their own mouse scale).
    pub fn input(&self, units: u32, ai: bool, live: bool, camera: u32, offline_camera: bool, squads: u32, at: f64, mouse_scale: f64) -> WorldTickInput {
        WorldTickInput {
            dt: self.clock.dt,
            tick: self.next_tick(),
            clock: self.clock.clone(),
            units,
            ai,
            live,
            camera,
            offline_camera,
            squads,
            at,
            mouse_scale,
        }
    }
    /// WorldTick.complete: the eight running fields (CLOCK_FIELDS) copied
    /// from the output's clock; hz, dt and the limits stay the host's.
    pub fn complete(&mut self, output: &WorldTickOutput) {
        let from = &output.clock;
        let to = &mut self.clock;
        to.tick = from.tick;
        to.elapsed = from.elapsed;
        to.compensation = from.compensation;
        to.debt = from.debt;
        to.alpha = from.alpha;
        to.overloads = from.overloads;
        to.dropped = from.dropped;
        to.step_cost = from.step_cost;
    }
    /// The clock completion World::step publishes for a tick it ran: one
    /// compensated advance by dt with the budget and debt limits off, as
    /// the step calls `clock::advance(&mut output.clock, dt, false, ..)`.
    /// Pure arithmetic, so a host can check its rows against a recording
    /// without a world, and keep a clock running before one exists.
    pub fn completed(&self) -> Result<WorldClockState, &'static str> {
        let mut state = self.clock.clone();
        let dt = state.dt;
        let ran = clock::advance(&mut state, dt, false, &mut Completed)?;
        if ran != 1.0 { return Err("the clock did not run one tick"); }
        Ok(state)
    }
}

// World::step's CompletedClock: Game.step consumes one stamped command, so
// pacing and the host's now() are outside the interval.
struct Completed;
impl clock::Host for Completed {
    fn now(&mut self, _: &mut WorldClockState) -> f64 { 0.0 }
    fn step(&mut self, _: &mut WorldClockState, _: f64, _: f64) {}
}
