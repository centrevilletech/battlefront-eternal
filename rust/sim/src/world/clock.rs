//! runtime/fixed-clock.js. Host pacing supplies time and executes each
//! requested tick; compensated elapsed time, debt and tick order are native.
use crate::gen::WorldClockState;

pub trait Host {
    fn now(&mut self, state: &mut WorldClockState) -> f64;
    fn step(&mut self, state: &mut WorldClockState, dt: f64, tick: f64);
}

pub fn reset(s: &mut WorldClockState) {
    s.tick = 0.0; s.elapsed = 0.0; s.compensation = 0.0; s.debt = 0.0;
    s.alpha = 0.0; s.overloads = 0.0; s.dropped = 0.0; s.step_cost = 0.0;
}

pub fn rebase(s: &mut WorldClockState) {
    s.elapsed = s.tick / s.hz; s.compensation = 0.0; s.debt = 0.0; s.alpha = 0.0;
}

pub fn advance(s: &mut WorldClockState, seconds: f64, limited: bool, host: &mut impl Host) -> Result<f64, &'static str> {
    if !seconds.is_finite() || seconds < 0.0 { return Err("invalid elapsed time"); }
    let value = seconds - s.compensation;
    let sum = s.elapsed + value;
    s.compensation = (sum - s.elapsed) - value;
    s.elapsed = sum;
    let target = v8math::floor(s.elapsed * s.hz + 1e-8);
    let budget = if limited { s.budget } else { f64::INFINITY };
    let mut limit = s.max_steps;
    if budget != f64::INFINITY && s.step_cost > 0.0 {
        limit = v8math::max(1.0, v8math::min(limit, v8math::floor(budget * 1000.0 / s.step_cost)));
    }
    let count = v8math::min(v8math::max(0.0, target - s.tick), limit);
    let batch_start = if budget != f64::INFINITY { host.now(s) } else { 0.0 };
    let mut ran = 0.0;
    while ran < count {
        let start = if budget != f64::INFINITY { host.now(s) } else { 0.0 };
        let (dt, tick) = (s.dt, s.tick + 1.0);
        host.step(s, dt, tick);
        s.tick += 1.0; ran += 1.0;
        if budget != f64::INFINITY {
            let end = host.now(s);
            let cost = end - start;
            s.step_cost = if s.step_cost != 0.0 && !s.step_cost.is_nan() { s.step_cost + (cost - s.step_cost) * 0.1 } else { cost };
            if end - batch_start >= budget * 1000.0 { break; }
        }
    }
    s.debt = v8math::max(0.0, target - s.tick);
    if limited && s.debt > s.max_debt {
        let drop = s.debt - s.max_debt;
        s.elapsed -= drop / s.hz;
        s.compensation = 0.0;
        s.dropped += drop;
        s.debt = s.max_debt;
    }
    s.alpha = if s.debt != 0.0 && !s.debt.is_nan() { 1.0 } else { v8math::max(0.0, v8math::min(1.0, s.elapsed * s.hz - s.tick)) };
    if s.debt != 0.0 && !s.debt.is_nan() { s.overloads += 1.0; }
    Ok(ran)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Ticks { time: f64, values: Vec<f64>, cost: f64 }
    impl Host for Ticks {
        fn now(&mut self, _: &mut WorldClockState) -> f64 { self.time }
        fn step(&mut self, state: &mut WorldClockState, dt: f64, tick: f64) {
            assert_eq!(dt, state.dt); assert_eq!(tick, state.tick + 1.0);
            self.values.push(tick); self.time += self.cost;
        }
    }
    fn state() -> WorldClockState { WorldClockState { hz: 60.0, dt: 1.0 / 60.0, max_steps: 8.0, budget: f64::INFINITY, max_debt: f64::INFINITY, ..Default::default() } }

    #[test]
    fn compensated_clock_keeps_consecutive_ticks_through_jitter() {
        let mut s = state(); let mut host = Ticks::default();
        for _ in 0..720 { advance(&mut s, 1.0 / 144.0, true, &mut host).unwrap(); }
        assert_eq!(s.tick, 300.0); assert_eq!(s.debt, 0.0);
        assert_eq!(host.values, (1..=300).map(f64::from).collect::<Vec<_>>());
    }

    #[test]
    fn slow_ticks_drop_elapsed_debt_without_skipping_tick_numbers() {
        let mut s = state(); s.budget = 0.035; s.max_debt = 8.0;
        let mut host = Ticks { cost: 50.0, ..Default::default() };
        for _ in 0..6 { assert_eq!(advance(&mut s, 0.1, true, &mut host), Ok(1.0)); }
        assert_eq!(host.values, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(s.debt, 8.0); assert_eq!(s.tick + s.debt + s.dropped, 36.0);
        rebase(&mut s); assert_eq!(s.elapsed, s.tick / s.hz); assert_eq!(s.debt, 0.0);
    }
}
