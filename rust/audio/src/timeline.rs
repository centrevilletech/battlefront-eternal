//! An AudioParam's automation as Chromium works it out (its
//! AudioParamHandler, the timeline of one parameter), for the events the
//! voice engine sets: setValueAtTime, setTargetAtTime and
//! cancelScheduledValues. Operation for operation, so a parameter here takes
//! the browser's values to the bit: the value in single precision, the
//! times in double, the exponentials from fdlibm (rust/v8math's port of the
//! same library), an event's segment ending at ceil(next event's time *
//! rate), events in the past clamped to the block they are first seen in,
//! and, on x86, a setTargetAtTime's run of a-rate values filled four at a
//! time from closed-form coefficients (the browser's SSE path), which rounds
//! otherwise than stepping one value at a time. Nothing here allocates: a
//! parameter holds at most CAP events (one a frame, the past ones dropped
//! as the browser drops them).

const CAP: usize = 16;
// exp(-10): a setTargetAtTime has converged once this close (relatively),
// or ten time constants on.
const SET_TARGET_THRESHOLD: f32 = 4.539_993e-5;
const TIME_CONSTANTS_TO_CONVERGE: f32 = 10.0;
const RENDER_QUANTUM: f64 = 128.0;

#[derive(Clone, Copy, Default)]
struct Event {
    target: bool, // setTargetAtTime (else setValueAtTime)
    value: f32,
    time: f64,
    tau: f64,
    fresh: bool, // not yet seen by a render (the browser's new_events_)
}

#[derive(Clone, Copy)]
pub struct Timeline {
    intrinsic: f32,
    ev: [Event; CAP],
    n: usize,
}

fn converged(value: f32, target: f32, current_time: f64, start_time: f64, tau: f64) -> bool {
    if current_time > start_time + TIME_CONSTANTS_TO_CONVERGE as f64 * tau {
        return true;
    }
    if target == 0.0 && value.abs() < SET_TARGET_THRESHOLD {
        return true;
    }
    target != 0.0 && (target - value).abs() < SET_TARGET_THRESHOLD * value.abs()
}

impl Timeline {
    pub fn new(value: f32) -> Timeline {
        Timeline { intrinsic: value, ev: [Event::default(); CAP], n: 0 }
    }

    /// The value with no automation left (AudioParam.value).
    pub fn value(&self) -> f32 {
        self.intrinsic
    }

    /// Whether any event is left (a param with none is its value).
    pub fn has_events(&self) -> bool {
        self.n > 0
    }

    fn remove_old(&mut self, count: usize) {
        if self.n > 1 {
            let k = count.min(self.n - 1);
            self.ev.copy_within(k..self.n, 0);
            self.n -= k;
        }
    }

    // Whether event i still counts at `frame` (its next has not started).
    fn current(&self, i: usize, frame: f64, rate: f64) -> bool {
        if i + 1 >= self.n {
            return true;
        }
        if self.ev[i + 1].time * rate >= frame {
            return true;
        }
        let e = self.ev[i];
        let at = e.time * rate;
        !e.target && at <= frame && frame < at + 1.0
    }

    /// setValueAtTime (target false) or setTargetAtTime at context time
    /// `time`, with the destination at `frame` (the next block's).
    pub fn insert(&mut self, target: bool, value: f32, time: f64, tau: f64, frame: f64, rate: f64) {
        // A time constant of 0 jumps at once: a setValueAtTime.
        let target = target && tau != 0.0;
        if self.n > 0 {
            let mut skipped = 0;
            for i in 0..self.n {
                if self.current(i, frame, rate) {
                    break;
                }
                skipped += 1;
            }
            if skipped > 0 {
                self.remove_old(skipped);
            }
        }
        if self.n == CAP {
            self.remove_old(1);
        }
        // After the events of the same time (upper bound).
        let mut at = self.n;
        if self.n > 0 && self.ev[self.n - 1].time > time {
            at = (0..self.n).find(|&i| self.ev[i].time > time).unwrap_or(self.n);
        }
        self.ev.copy_within(at..self.n, at + 1);
        self.ev[at] = Event { target, value, time, tau, fresh: true };
        self.n += 1;
    }

    /// cancelScheduledValues(time): the events from `time` on go.
    pub fn cancel(&mut self, time: f64) {
        if let Some(i) = (0..self.n).find(|&i| self.ev[i].time >= time) {
            self.n = i;
        }
    }

    // New events before the block's time take it (a stable re-sort after).
    fn clamp_new(&mut self, time: f64) {
        let mut clamped = false;
        for e in &mut self.ev[..self.n] {
            if e.fresh {
                if e.time < time {
                    e.time = time;
                    clamped = true;
                }
                e.fresh = false;
            }
        }
        if clamped {
            // Insertion sort: stable, and the list is short.
            for i in 1..self.n {
                let mut j = i;
                while j > 0 && self.ev[j - 1].time > self.ev[j].time {
                    self.ev.swap(j - 1, j);
                    j -= 1;
                }
            }
        }
    }

    /// A k-rate parameter's value for the block starting at `frame`.
    pub fn k_rate(&mut self, frame: u64, rate: f64) -> f32 {
        let value = self.intrinsic;
        if self.n == 0 || (frame as f64 / rate) < self.ev[0].time {
            return value;
        }
        let mut out = [0.0f32; 1];
        let v = self.values(frame, &mut out, rate, rate / RENDER_QUANTUM, value, false);
        self.intrinsic = v;
        v
    }

    /// An a-rate parameter's values for the block starting at `frame`.
    pub fn a_rate(&mut self, frame: u64, out: &mut [f32], rate: f64, x86: bool) {
        let v = self.values(frame, out, rate, rate, self.intrinsic, x86);
        self.intrinsic = v;
    }

    // The browser's ValuesForFrameRangeImpl for these events: `out` from
    // `start`, `control` the rate the time constants count in.
    fn values(&mut self, start: u64, out: &mut [f32], rate: f64, control: f64, default: f32, x86: bool) -> f32 {
        let len = out.len();
        let end = start + len as u64;
        if self.n == 0 || end as f64 / rate <= self.ev[0].time {
            out.fill(default);
            return default;
        }
        self.clamp_new(start as f64 / rate);
        let mut default = default;
        // All events in the past: their value, once a setTarget has
        // converged (the events then go).
        let current_time = start as f64 / rate;
        let last = self.ev[self.n - 1];
        if last.time + 1.5 * RENDER_QUANTUM / rate < current_time {
            let mut done = true;
            if last.target {
                if converged(default, last.value, current_time, last.time, last.tau) {
                    default = last.value;
                } else {
                    done = false;
                }
            }
            if done {
                out.fill(default);
                self.remove_old(self.n);
                return default;
            }
        }
        // Before the first event: the default.
        let mut frame = start;
        let mut write = 0usize;
        let first = self.ev[0].time;
        if first > start as f64 / rate {
            let first_frame = (first * rate).ceil();
            let fill_end = if (end as f64) > first_frame { first_frame as u64 } else { end };
            let fill = ((fill_end - start) as usize).min(len);
            out[write..fill].fill(default);
            write = fill;
            frame += fill as u64;
        }
        let mut value = default;
        let mut skipped = 0;
        let mut i = 0;
        while i < self.n && write < len {
            if !self.current(i, frame as f64, rate) {
                skipped += 1;
                i += 1;
                continue;
            }
            let e = self.ev[i];
            let time2 = if i + 1 < self.n { self.ev[i + 1].time } else { end as f64 / rate + 1.0 };
            let fill_end = if (end as f64) > time2 * rate { (time2 * rate).ceil() as u64 } else { end };
            let fill = ((fill_end - start) as usize).min(len);
            if !e.target {
                frame = fill_end;
                value = e.value;
                out[write..fill].fill(value);
                write = fill;
            } else {
                (frame, value, write) = set_target(fill, e, rate, control, fill_end, out, frame, value, write, x86);
            }
            i += 1;
        }
        if skipped > 0 {
            self.remove_old(skipped);
        }
        out[write..].fill(value);
        out[len - 1]
    }
}

// The browser's ProcessSetTarget.
#[allow(clippy::too_many_arguments)]
fn set_target(fill: usize, e: Event, rate: f64, control: f64, fill_end: u64, out: &mut [f32], frame: u64, value: f32, write: usize, x86: bool) -> (u64, f32, usize) {
    let (mut frame, mut value, mut write) = (frame, value, write);
    let target = e.value;
    let tau = e.tau as f32;
    let c = (1.0 - v8math::exp(-1.0 / (control * tau as f64))) as f32;
    let ramp_start = e.time * rate;
    if ramp_start <= frame as f64 && (frame as f64) < ramp_start + 1.0 {
        value = (target as f64 + (value - target) as f64 * v8math::exp(-(frame as f64 / rate - e.time) / tau as f64)) as f32;
    } else {
        value += (target - value) * c;
    }
    if converged(value, target, frame as f64 / rate, e.time, tau as f64) {
        frame += (fill - write) as u64;
        out[write..fill].fill(target);
        write = fill;
    } else {
        if x86 && fill > write {
            // Four values a step: v + (t - v) * [0, c0, c1, c2], then
            // v += (t - v) * c3, with c1 = c0 (2 - c0) and so on, in f32.
            let c0 = c;
            let c1 = c0 * (2.0 - c0);
            let c2 = c0 * ((c0 - 3.0) * c0 + 3.0);
            let c3 = c0 * (c0 * ((4.0 - c0) * c0 - 6.0) + 4.0);
            let trunc = write + ((fill - write) / 4) * 4;
            while write < trunc {
                let delta = target - value;
                out[write] = value + delta * 0.0;
                out[write + 1] = value + delta * c0;
                out[write + 2] = value + delta * c1;
                out[write + 3] = value + delta * c2;
                value += delta * c3;
                write += 4;
            }
        }
        while write < fill {
            out[write] = value;
            value += (target - value) * c;
            write += 1;
        }
        if write >= 1 {
            value = out[write - 1];
        }
        frame = fill_end;
    }
    (frame, value, write)
}
