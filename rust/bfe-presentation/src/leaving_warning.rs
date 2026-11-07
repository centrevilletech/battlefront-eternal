//! Small page policies with their own retained state. Resource and device
//! hosts read numeric results without advancing a second JavaScript clock.
use crate::page_layout::warning as w;

pub struct Warning {
    pub row: [f64; w::STRIDE],
}
impl Default for Warning {
    fn default() -> Self {
        Self { row: [0.0; w::STRIDE] }
    }
}
impl Warning {
    // Ports LeavingWarning.update and alpha. A repeat display of the same
    // world tick takes no extra spring step; entering starts with one step.
    pub fn frame(&mut self) {
        let a = &mut self.row;
        a[w::STARTED] = 0.0;
        if a[w::OUTSIDE] == 0.0 {
            a[w::SHOWN] = 0.0;
            a[w::ALPHA] = 0.0;
            return;
        }
        // The warning retains its previous tick while hidden. Re-entering
        // ignores that gap and deliberately takes exactly one spring step.
        let started = a[w::SHOWN] == 0.0;
        let mut steps = (a[w::TICK] - a[w::LAST_TICK]).clamp(0.0, 60.0).ceil() as usize;
        if started {
            a[w::SHOWN] = 1.0;
            a[w::VALUE] = 254.0;
            a[w::RATE] = 0.0;
            steps = 1;
        }
        // The old loop uses i < steps, including fractional input ticks.
        // Clamp before conversion so a long pause never grows frame work.
        a[w::LAST_TICK] = a[w::TICK];
        for _ in 0..steps {
            a[w::VALUE] += a[w::RATE] / 60.0;
            a[w::RATE] += (128.0 - a[w::VALUE]) * 500.0 / 60.0;
        }
        // Publish both display alpha and the one-shot transition together;
        // a later property read must never advance time or replay the voice.
        a[w::STARTED] = started as u8 as f64;
        a[w::ALPHA] = a[w::VALUE].trunc().clamp(2.0, 255.0) / 255.0;
    }
}
