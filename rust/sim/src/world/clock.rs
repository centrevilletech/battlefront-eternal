//! runtime/fixed-clock.js. Host pacing supplies time and executes each
//! requested tick; compensated elapsed time, debt and tick order are native.
//!
//! One gameplay clock. Rendering only supplies elapsed time; integer ticks
//! determine gameplay time. A task has bounded work and retains catch-up debt.
//!
//! Two optional limits keep a machine that cannot keep up from spiralling
//! (each catch-up batch making the next frame later still):
//! - budget: wall-clock seconds of ticks per advance. The batch shrinks to
//!   what the measured cost of a tick fits in, never below one tick.
//! - maxDebt: ticks of debt kept. Beyond it the excess is dropped: offline,
//!   the world runs slower than real time instead of falling further behind.
//!   `dropped` counts every tick given up this way.
//! Neither changes the tick sequence or a tick's length, only how many run per
//! advance, so gameplay results are the same at any speed. An advance with
//! `limited` false ignores both (tools and replays that must run every tick).

//!
//! FixedClock callback ordering
//!
//! Elapsed time is a compensated sum in seconds. Tick targets are derived
//! from that sum with the original small rounding allowance. They do not
//! accumulate a rounded number of ticks per display frame, which would
//! drift on refresh rates that do not divide the simulation frequency.
//!
//! Each advance captures its target and budget before invoking host code.
//! A callback may reset the public clock, change its rate or enter another
//! clock. Those edits affect the shared state while the current batch keeps
//! the target and work limit it already captured. A local Pump preserves
//! those values across imports without holding any host resource reference.
//!
//! The host observes the state before its tick. Only a completed callback
//! increments the tick number; a failed tick leaves the advanced elapsed
//! time and any callback edits visible for the next advance. Budget timing
//! samples after that increment, so time callbacks see the completed tick.
//! The measured cost is in milliseconds while the configured budget is in
//! seconds. Smoothing keeps the original tenth-share arithmetic order.
//!
//! Dropped debt reduces elapsed wall time and clears its compensation. It
//! never skips a simulation tick number or changes the tick duration. A
//! machine that cannot keep up therefore runs slower rather than accumulating
//! unbounded catch-up work. Offline tools can ignore that limit explicitly
//! while retaining the same bounded batch size and consecutive tick sequence.
//! A nonzero debt publishes full interpolation alpha until it is repaid.

use crate::gen::WorldClockState;

pub trait Host {
    fn now(&mut self, state: &mut WorldClockState) -> f64;
    fn step(&mut self, state: &mut WorldClockState, dt: f64, tick: f64);
}

pub fn reset(s: &mut WorldClockState) {
    s.tick = 0.0;
    s.elapsed = 0.0;
    s.compensation = 0.0;
    s.debt = 0.0;
    s.alpha = 0.0;
    s.overloads = 0.0;
    s.dropped = 0.0;
    s.step_cost = 0.0;
}

pub fn rebase(s: &mut WorldClockState) {
    s.elapsed = s.tick / s.hz;
    s.compensation = 0.0;
    s.debt = 0.0;
    s.alpha = 0.0;
}

/// FixedClock's callback pump keeps all numeric state in the world clock.
/// Client input clocks use the same owner with browser tick callbacks.
#[derive(Clone, Copy, Default)]
pub struct Pump {
    count: f64,
    ran: f64,
    target: f64,
    batch: f64,
    budget: f64,
}
impl Pump {
    pub fn begin(
        &mut self,
        s: &mut WorldClockState,
        seconds: f64,
        limited: bool,
    ) -> Result<f64, &'static str> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("invalid elapsed time");
        }
        let value = seconds - s.compensation;
        let sum = s.elapsed + value;
        s.compensation = (sum - s.elapsed) - value;
        s.elapsed = sum;
        self.target = v8math::floor(s.elapsed * s.hz + 1e-8);
        self.budget = if limited { s.budget } else { f64::INFINITY };
        let mut limit = s.max_steps;
        if self.budget != f64::INFINITY && s.step_cost > 0.0 {
            limit = v8math::max(
                1.0,
                v8math::min(limit, v8math::floor(self.budget * 1000.0 / s.step_cost)),
            );
        }
        self.count = v8math::min(v8math::max(0.0, self.target - s.tick), limit);
        self.ran = 0.0;
        Ok(self.count)
    }
    pub fn start(&mut self, now: f64) {
        self.batch = now;
    }
    /// Called after a completed host tick. A full budget still permits the
    /// first tick, then yields while preserving consecutive tick numbers.
    pub fn complete(&mut self, s: &mut WorldClockState) {
        s.tick += 1.0;
        self.ran += 1.0;
    }
    pub fn stepped(&mut self, s: &mut WorldClockState, start: f64, end: f64) -> bool {
        let mut stop = false;
        if self.budget != f64::INFINITY {
            let cost = end - start;
            s.step_cost = if s.step_cost != 0.0 && !s.step_cost.is_nan() {
                s.step_cost + (cost - s.step_cost) * 0.1
            } else {
                cost
            };
            stop = end - self.batch >= self.budget * 1000.0;
        }
        self.ran < self.count && !stop
    }
    /// Catch-up debt drops wall time, never simulation tick identities.
    /// Unlimited replay retains every debt tick regardless of live limits.
    pub fn finish(&mut self, s: &mut WorldClockState, limited: bool) -> f64 {
        s.debt = v8math::max(0.0, self.target - s.tick);
        if limited && s.debt > s.max_debt {
            let drop = s.debt - s.max_debt;
            s.elapsed -= drop / s.hz;
            s.compensation = 0.0;
            s.dropped += drop;
            s.debt = s.max_debt;
        }
        s.alpha = if s.debt != 0.0 && !s.debt.is_nan() {
            1.0
        } else {
            v8math::max(0.0, v8math::min(1.0, s.elapsed * s.hz - s.tick))
        };
        if s.debt != 0.0 && !s.debt.is_nan() {
            s.overloads += 1.0;
        }
        self.ran
    }
}

pub fn advance(
    s: &mut WorldClockState,
    seconds: f64,
    limited: bool,
    host: &mut impl Host,
) -> Result<f64, &'static str> {
    let mut pump = Pump::default();
    let count = pump.begin(s, seconds, limited)?;
    let timed = limited && s.budget != f64::INFINITY;
    pump.start(if timed { host.now(s) } else { 0.0 });
    if count > 0.0 {
        loop {
            let start = if timed { host.now(s) } else { 0.0 };
            host.step(s, s.dt, s.tick + 1.0);
            pump.complete(s);
            let end = if timed { host.now(s) } else { 0.0 };
            if !pump.stepped(s, start, end) {
                break;
            }
        }
    }
    Ok(pump.finish(s, limited))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Ticks {
        time: f64,
        values: Vec<f64>,
        cost: f64,
    }
    impl Host for Ticks {
        fn now(&mut self, _: &mut WorldClockState) -> f64 {
            self.time
        }
        fn step(&mut self, state: &mut WorldClockState, dt: f64, tick: f64) {
            assert_eq!(dt, state.dt);
            assert_eq!(tick, state.tick + 1.0);
            self.values.push(tick);
            self.time += self.cost;
        }
    }
    fn state() -> WorldClockState {
        WorldClockState {
            hz: 60.0,
            dt: 1.0 / 60.0,
            max_steps: 8.0,
            budget: f64::INFINITY,
            max_debt: f64::INFINITY,
            ..Default::default()
        }
    }

    #[test]
    fn compensated_clock_keeps_consecutive_ticks_through_jitter() {
        let mut s = state();
        let mut host = Ticks::default();
        for _ in 0..720 {
            advance(&mut s, 1.0 / 144.0, true, &mut host).unwrap();
        }
        assert_eq!(s.tick, 300.0);
        assert_eq!(s.debt, 0.0);
        assert_eq!(host.values, (1..=300).map(f64::from).collect::<Vec<_>>());
    }

    #[test]
    fn slow_ticks_drop_elapsed_debt_without_skipping_tick_numbers() {
        let mut s = state();
        s.budget = 0.035;
        s.max_debt = 8.0;
        let mut host = Ticks {
            cost: 50.0,
            ..Default::default()
        };
        for _ in 0..6 {
            assert_eq!(advance(&mut s, 0.1, true, &mut host), Ok(1.0));
        }
        assert_eq!(host.values, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(s.debt, 8.0);
        assert_eq!(s.tick + s.debt + s.dropped, 36.0);
        rebase(&mut s);
        assert_eq!(s.elapsed, s.tick / s.hz);
        assert_eq!(s.debt, 0.0);
    }
}
