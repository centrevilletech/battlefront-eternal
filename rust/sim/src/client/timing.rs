//! runtime/reconcile.js, playout.js and frame-pacing.js. Each client retains
//! its arrival windows and clocks here; the browser supplies timestamps only.
//!
//! Where the world's clock is now. Each packet's arrival gives a lower bound
//! on the world's position (tick - arrival * hz, in ticks): the packet that
//! came with the least delay is the best estimate, so the largest offset over
//! a short window is kept. The world running slower than wall time (it drops
//! ticks) moves that bound down, so the page resets the window when a packet
//! reports dropped ticks.
//! The world's tick (fractional) at wall time `now`, or null before any packet.
//! The lowest stamp a command sent now may carry and still reach the world
//! before that tick runs: the world's estimated position, plus the packet
//! transit and the tick in progress, plus how many ticks the world has
//! recently run in one batch (a tick that ran long holds its mail until the
//! batch ends), plus an extra margin raised by whatever lateness the
//! acknowledgements still show (a transit the estimate cannot see) and
//! lowered again after on-time acknowledgements. With measured network RTT,
//! recovery can smoothly remove a transient stall's margin in seconds. The
//! target is continuous (a position for the input clock to track), half a
//! tick above that floor so each command is produced with that much to
//! spare; `lead` is how far above the newest arrived tick it is.
//! A packet for `tick` arrived at `now` (seconds); `dropped` is the world's
//! count of ticks it has given up.
//! Ticks the stream fell behind between this arrival and the last: a
//! batch of n ticks that arrive together after n ticks of silence
//! shows as a gap of about n - 1.
//! The world took a command `late` ticks after the tick it was stamped for.
//! A late one raises the extra margin by that much, once per `cooldown`
//! acknowledgements (the commands already in flight were stamped before
//! the raise). With `recover`, after `calm` on-time ones in a row, each
//! following ACK lowers it a tenth of a tick: the input clock can shed
//! the same six ticks per second at its 90% catch-down rate. A stall must not
//! leave minutes of queued-input latency, or drop the target so abruptly
//! that the clock hits its bound and stops sampling input. Without a
//! measured RTT, extra also covers steady transit, so retain the slower
//! one-tick reduction per calm interval for the local/unknown-RTT case.
//! `capped`: the command was stamped while maxLead held the target below
//! where the margin put it (target), because the page had not heard from
//! the world for longer than the cap covers. A page that stalls (a long
//! frame) does so on the world's packets too: it draws its next frame
//! before it has taken all that came, and stamps at the cap over a newest
//! tick some way behind the world. Those commands land late however wide
//! the margin (the cap holds them), so their lateness neither raises the
//! margin, which offline would then take two minutes to come down, nor
//! counts against its calm.
//! The 85th percentile gap, not the maximum: one stall must not keep the
//! lead wide for the whole window. The lead only borrows this as a
//! bounded hint (see target); real upstream lateness is measured
//! directly by acknowledge().
//! The position (in ticks, fractional) the input clock should be at for a
//! command sent at `now`, given the newest tick that has arrived. Bounded
//! above the newest tick so a world that stops answering cannot pull the
//! stamps away with it.
//! Arrival gaps mostly measure how bunched the DOWNSTREAM packets land;
//! stamping commands later does not un-bunch them, it only delays his
//! own actions. Borrow at most a few ticks of it and let the measured
//! lateness in extra (acknowledge) carry the rest.
//! A network world's clock keeps running when its downstream snapshots are
//! lost. Do not turn that loss into a stopped upstream input stream.
//! The cap, not the margin, set this position (acknowledge's `capped`).
//! Keeps a 60 Hz input clock tracking `target` (a fractional tick position)
//! without ever rewinding it (the world may still have the earlier stamps
//! queued): behind, it jumps to the target (ticks skipped that way are held
//! through by the world's mailbox and by ClientPrediction); more than a tick
//! ahead (the target came down after a rough patch), it runs at 90% so the
//! input latency shrinks back; and it never passes `bound`, so a world that
//! has stopped cannot be sent more than a few ticks of commands.
//! `step(dt, tick)` makes the command for each tick.
//!
//! `lock` (a phase, 0 to 1): where in the newest tick a frame is drawn, when
//! frames come about once a tick (a 60 Hz display). A body predicted from the
//! commands is drawn between the last two predicted ticks at this clock's
//! phase (its alpha). Frames at the tick rate keep whatever phase the clock
//! has, and the target leaves it anywhere: half a tick in, the command made
//! this frame shows half its effect, the rest a frame later. Locked, the
//! clock eases (a tenth of a tick a frame, as the trim does) to at least
//! `lock` of the way into the tick it is stamping, which never changes the
//! stamps, and eases back when that has left it stamping a whole tick later
//! than the target needs. A display slightly off 60 Hz drifts a tick against
//! the world every few hundred frames and costs about ten frames of easing
//! each time. Other frame rates draw every phase in turn and are left alone.
//! Where this frame will be drawn, and where the target will be then.
//!
//! move evenly however the packets bunch up.
//!
//! - The delay is a high percentile (95th) of recent arrival gaps, plus a
//!   margin, within [minDelay, maxDelay] ticks. One isolated stall must not
//!   widen the cushion for the whole window the way the maximum did. A page drawing at 20 fps
//!   sees packets three at a time, so its delay settles around 4 to 5. Packets that arrive in bursts
//!   (a tick that ran long, a garbage-collection pause) widen it; a calm
//!   stream lets it shrink again.
//! - The clock runs at wall speed, trimmed by at most `trim` (10%) toward
//!   its target. It never passes the newest tick: starved, it waits.
//! - Far from its target (a stall, a resume, the first packets), it jumps.
//! A packet for `tick` arrived at wall time `now` (seconds). `interval`
//! is the stream's snapshot spacing in ticks (1 for per-tick streams).
//! How late this arrival ran against one interval after the previous
//! one. A missing snapshot makes the next arrival a whole interval
//! late by this measure, so sustained loss widens the cushion and
//! holds a spare interval in the buffer. (The old formula subtracted
//! the ticks the packet jumped, so a lost snapshot measured on time.)
//! The delay to hold: the 95th percentile recent arrival gap plus a
//! margin, so sustained jitter widens the cushion and one stall does not.
//! Wall seconds passed: the clock moves on. Returns the tick to show
//! (fractional: the integer part is the newest tick applied, the fraction
//! the way to the next).
//! Trim toward the target: the whole error over about a second.
//! The newest tick the page should have applied for the clock's position.
//!
//! Presentation deadlines are independent of the world clock. No timer loop or
//! spin wait: the browser still schedules every opportunity with rAF.
//! rAF timestamps have small jitter even on a display matching the cap.
//! Missing a 60 Hz deadline by 0.1 ms must not throw away a whole refresh.
//! Retain the phase on an irregular display, but don't render catch-up
//! frames after a stall. Only simulation owes catch-up ticks.
//! Conservative dynamic resolution. Evaluate sustained frame pressure, not a
//! single shader compilation or OS hitch. Never repeatedly shrink the image to
//! try to fix a CPU-bound simulation. No localStorage writes while adapting.
//!
//! The GPU's share comes from its timer where the browser offers one (gpuMs),
//! else from the render profiler's fences (render-profiler.js): `behind`, the
//! last frame's GPU work still unfinished when this one began, and `latency`,
//! how soon after a frame was sent its fence was seen signaled (ms, an upper
//! bound of the GPU's time past the frame's last command).
//! Ignore resume gaps and large task stalls. They are not GPU evidence.
//! Without any GPU evidence, 30 Hz display pacing is indistinguishable
//! from a GPU taking 33 ms. Keep the chosen resolution rather than guessing.
//! A two-second startup/window prevents a low refresh display or one
//! expensive frame from thrashing render targets.

//!
//! Record and scratch lifetimes
//!
//! Each timing facade has one native owner for its whole lifetime. Native
//! arrival rings never borrow JavaScript arrays and never resize at an arrival
//! or ACK. Setup bounds diagnostic window sizes before allocating either ring.
//! The sort scratch has the same capacity as the gap ring and is reused for
//! every measurement. Reset clears validity without replacing that storage.
//!
//! The public record keeps null estimator observations as NaN sentinels.
//! Before the first packet, no estimated world time exists. That absence must
//! survive a reset instead of becoming an estimate based on a zero offset.
//! FixedClock uses the same generated fields in a distinct owner, where its
//! dropped count is an ordinary number rather than an arrival observation.
//!
//! A cached measurement is qualified by both ring count and write position.
//! An unchanged window does no sorting, while a wrapped full ring still
//! invalidates the prior percentile. Numeric typed-array ordering includes
//! signed zero; the native scratch uses that same ordering without allocating.
//! Recovery and extrapolation are configured once and remain client-local.

use crate::gen::ClientTiming as S;
use v8math::{floor, max, min};

// Resolve ring storage at construction. Even an excessive diagnostic window
// is bounded; no arrival, pause or percentile measurement can grow a store.
const CAPACITY: usize = 65536;
pub struct Timing {
    pub words: [f64; S::WORDS],
    offsets: Vec<f64>,
    gaps: Vec<f64>,
    sorted: Vec<f64>,
    // A tick callback may run this same clock again. Each active advance
    // retains its own target, budget and batch count while sharing elapsed
    // state. Excess recursion yields instead of allocating or trapping.
    pumps: [crate::world::clock::Pump; 16],
    depth: usize,
}
impl Timing {
    pub fn new() -> Self {
        Self {
            words: [0.0; S::WORDS],
            offsets: Vec::new(),
            gaps: Vec::new(),
            sorted: Vec::new(),
            pumps: [crate::world::clock::Pump::default(); 16],
            depth: 0,
        }
    }
    fn read(&self) -> S {
        S::take(&self.words, &[])
    }
    fn write(&mut self, s: &S) {
        s.put(&mut self.words, &mut Vec::new());
    }
    /// WorldTickEstimator / InputLead / PlayoutClock constructor reservations.
    pub fn setup(&mut self) {
        let mut s = self.read();
        s.window = (s.window as usize).clamp(1, CAPACITY) as f64;
        s.gap_window = (s.gap_window as usize).clamp(1, CAPACITY) as f64;
        self.offsets.resize(s.window as usize, 0.0);
        self.gaps.resize(s.gap_window as usize, 0.0);
        self.sorted.resize(s.gap_window as usize, 0.0);
        self.write(&s);
    }
    fn estimator_reset(s: &mut S) {
        s.at = 0.0;
        s.count = 0.0;
        s.offset = f64::NAN;
        s.dropped = f64::NAN;
    }
    fn estimator_arrive(&mut self, s: &mut S, tick: f64, now: f64, dropped: f64) {
        if !s.dropped.is_nan() && dropped != s.dropped {
            s.at = 0.0;
            s.count = 0.0;
        }
        s.dropped = dropped;
        let at = (s.at as usize).min(self.offsets.len() - 1);
        self.offsets[at] = tick - now * s.hz;
        s.at = ((at + 1) % self.offsets.len()) as f64;
        s.count = min(s.count + 1.0, self.offsets.len() as f64);
        let mut best = f64::NEG_INFINITY;
        for &n in &self.offsets[..(s.count as usize).min(self.offsets.len())] {
            if n > best {
                best = n;
            }
        }
        s.offset = best;
    }
    fn gap(&mut self, s: &mut S, gap: f64) {
        let at = (s.gap_at as usize).min(self.gaps.len() - 1);
        self.gaps[at] = gap;
        s.gap_at = ((at + 1) % self.gaps.len()) as f64;
        s.gap_count = min(s.gap_count + 1.0, self.gaps.len() as f64);
    }
    fn measure(&mut self, s: &mut S, percentile: f64) -> f64 {
        let count = (s.gap_count as usize).min(self.gaps.len());
        self.sorted[..count].copy_from_slice(&self.gaps[..count]);
        // Float64Array.sort orders numbers, unlike Array.sort. The retained
        // scratch uses an allocation-free unstable numeric sort.
        self.sorted[..count].sort_unstable_by(f64::total_cmp);
        if count == 0 {
            0.0
        } else {
            self.sorted[(floor(count as f64 * percentile) as usize).min(count - 1)]
        }
    }
    fn measured(&mut self, s: &mut S, playout: bool) {
        let key = s.gap_count * 4096.0 + s.gap_at;
        if key == s.measured_key {
            return;
        }
        s.measured_key = key;
        let gap = self.measure(s, if playout { 0.95 } else { 0.85 });
        if playout {
            s.delay = min(s.max_delay, max(s.min_delay, gap + s.margin));
        } else {
            s.worst_gap = gap;
        }
    }
    /// reset preserves immutable options and the already allocated rings.
    pub fn run(&mut self, op: u32, a: f64, b: f64, c: f64, d: f64, e: f64) -> f64 {
        let mut s = self.read();
        let mut result = 0.0;
        match op {
            0 => Self::estimator_reset(&mut s),
            1 => self.estimator_arrive(&mut s, a, b, c),
            2 => {
                result = if s.offset.is_nan() {
                    f64::NAN
                } else {
                    s.offset + a * s.hz
                };
            }
            3 => {
                Self::estimator_reset(&mut s);
                self.gaps.fill(0.0);
                s.gap_at = 0.0;
                s.gap_count = 0.0;
                s.arrived_at = f64::NAN;
                s.arrived_tick = 0.0;
                s.latest = 0.0;
                s.lead = s.margin + 1.0;
                s.late = 0.0;
                s.worst_gap = 0.0;
                s.extra = 0.0;
                s.cooldown = 0.0;
                s.calm = 0.0;
                s.measured_key = -1.0;
                s.capped = false;
            }
            4 => {
                self.estimator_arrive(&mut s, a, b, c);
                s.latest = max(s.latest, a);
                if !s.arrived_at.is_nan() && a > s.arrived_tick {
                    let gap = max(0.0, (b - s.arrived_at) * s.hz - (a - s.arrived_tick));
                    self.gap(&mut s, gap);
                }
                s.arrived_at = b;
                s.arrived_tick = max(s.arrived_tick, a);
            }
            5 => {
                let elapsed = max(1.0, min(60.0, b));
                if s.cooldown > 0.0 {
                    s.cooldown = max(0.0, s.cooldown - elapsed);
                }
                if a > 0.0 {
                    s.late += 1.0;
                    if c == 0.0 {
                        s.calm = 0.0;
                        if s.cooldown == 0.0 {
                            s.extra = min(s.max_lead, s.extra + a);
                            s.cooldown = s.cooldown_acks;
                        }
                    }
                } else {
                    s.calm += elapsed;
                    if s.calm >= s.calm_acks {
                        s.calm = if s.recover { s.calm_acks } else { 0.0 };
                        s.extra = max(0.0, s.extra - if s.recover { 0.1 * elapsed } else { 1.0 });
                    }
                }
            }
            6 => self.measured(&mut s, false),
            7 => {
                self.measured(&mut s, false);
                let estimate = if s.offset.is_nan() {
                    None
                } else {
                    Some(s.offset + a * s.hz)
                };
                let transit = max(0.0, min(1.0, c)) * s.hz;
                let slack = 0.5
                    + s.margin
                    + min(
                        s.max_lead,
                        transit + min(4.0, v8math::round(s.worst_gap)) + s.extra,
                    );
                let target = estimate.unwrap_or(b) + slack;
                let reference = if s.extrapolate {
                    max(b, estimate.unwrap_or(b))
                } else {
                    b
                };
                result = min(max(target, reference + 1.0), reference + s.max_lead);
                s.capped = target > result;
                s.lead = result - b;
            }
            8 => {
                s.clock = 0.0;
                s.latest = 0.0;
                s.delay = s.min_delay;
                s.rate = 1.0;
                self.gaps.fill(0.0);
                s.gap_at = 0.0;
                s.gap_count = 0.0;
                s.arrived_at = f64::NAN;
                s.arrived_tick = 0.0;
                s.started = false;
                s.waits = 0.0;
                s.measured_key = -1.0;
            }
            9 => {
                s.latest = max(s.latest, a);
                if !s.arrived_at.is_nan() && a > s.arrived_tick {
                    let gap = max(0.0, (b - s.arrived_at) * s.hz - c + 1.0);
                    self.gap(&mut s, gap);
                }
                s.arrived_at = b;
                s.arrived_tick = max(s.arrived_tick, a);
            }
            10 => {
                self.measured(&mut s, true);
                result = s.delay;
            }
            11 => {
                self.measured(&mut s, true);
                let target = s.latest - s.delay;
                if !s.started {
                    if s.latest > 0.0 {
                        s.started = true;
                        s.clock = max(0.0, target);
                        s.rate = 1.0;
                    }
                } else {
                    let error = target - s.clock;
                    if error.abs() > s.snap {
                        s.clock = max(0.0, target);
                        s.rate = 1.0;
                    } else {
                        s.rate = min(1.0 + s.trim, max(1.0 - s.trim, 1.0 + error / s.hz));
                        let mut next = s.clock + a * s.hz * s.rate;
                        if next > s.latest {
                            next = s.latest;
                            s.waits += 1.0;
                        }
                        s.clock = max(s.clock, next);
                    }
                }
                result = s.clock;
            }
            12 => {
                s.limit = if a.is_finite() && a > 0.0 { a } else { 0.0 };
                s.elapsed = 0.0;
                s.presented = 0.0;
                s.deadline = 0.0;
            }
            13 => {
                s.elapsed = 0.0;
                s.presented = 0.0;
                s.deadline = 0.0;
            }
            14 => {
                s.elapsed += max(0.0, a);
                let tolerance = if s.limit != 0.0 {
                    min(0.0005, 0.05 / s.limit)
                } else {
                    0.0
                };
                result = f64::NAN;
                if s.limit == 0.0 || s.elapsed + tolerance + 1e-8 >= s.deadline {
                    result = s.elapsed - s.presented;
                    s.presented = s.elapsed;
                    if s.limit != 0.0 {
                        s.deadline += max(
                            1.0,
                            floor((s.elapsed + tolerance + 1e-8 - s.deadline) * s.limit) + 1.0,
                        ) / s.limit;
                    }
                }
            }
            15 => {
                s.target = max(0.0, if a.is_nan() { 0.0 } else { a });
                s.scale = 1.0;
                Self::resolution_reset(&mut s);
            }
            16 => Self::resolution_reset(&mut s),
            17 => {
                self.resolution(&mut s, a, b, c, d, e);
                result = s.scale;
            }
            // advanceInputClock: phase locking changes elapsed time, never the
            // command sequence. The FixedClock host executes the returned span.
            18 => {
                let behind = a - s.elapsed * s.hz;
                if behind > 0.0 {
                    s.tick = max(s.tick, floor(a));
                    s.elapsed = a / s.hz;
                    s.compensation = 0.0;
                    s.debt = 0.0;
                    s.alpha = 0.0;
                }
                let rate = if behind < -1.0 { 0.9 } else { 1.0 };
                let frame = b * s.hz;
                let mut ticks = frame * rate;
                if d > 0.0 && rate == 1.0 && frame > 0.9 && frame < 1.1 {
                    let next = s.elapsed * s.hz + ticks;
                    let bound = a + frame;
                    let stamp = floor(next);
                    let phase = next - stamp;
                    if stamp == floor(bound) && phase < d {
                        ticks += min(0.1, d - phase);
                    } else if stamp > floor(bound + 0.05) {
                        ticks -= min(0.1, next - bound);
                    }
                }
                let bound = if c.is_nan() { a + 7.0 } else { c };
                result = min(ticks / s.hz, max(0.0, bound / s.hz - s.elapsed));
            }
            19 => {
                s.tick = 0.0;
                s.elapsed = 0.0;
                s.compensation = 0.0;
                s.debt = 0.0;
                s.alpha = 0.0;
                s.overloads = 0.0;
                s.dropped = 0.0;
                s.step_cost = 0.0;
            }
            20 => {
                s.elapsed = s.tick / s.hz;
                s.compensation = 0.0;
                s.debt = 0.0;
                s.alpha = 0.0;
            }
            21 => {
                s.dt = 1.0 / s.hz;
                result = f64::from(
                    s.hz > 0.0
                        && s.max_steps >= 1.0
                        && s.max_steps.fract() == 0.0
                        && s.budget > 0.0
                        && s.max_debt >= 0.0,
                );
            }
            22 => {
                result = s.tick / s.hz;
            }
            23 => {
                result = s.tick + 1.0;
            }
            24 => {
                result = floor(s.clock) + 1.0;
            }
            _ => {}
        }
        s.result = result;
        self.write(&s);
        result
    }
    fn resolution_reset(s: &mut S) {
        s.age = 0.0;
        s.time = 0.0;
        s.frames = 0.0;
        s.slow = 0.0;
        s.cpu_bound = 0.0;
        s.headroom = 0.0;
        s.recovery = 0.0;
    }
    /// ResolutionController.sample: a measured GPU timer outranks fences.
    fn resolution(
        &mut self,
        s: &mut S,
        interval: f64,
        cpu: f64,
        gpu: f64,
        behind: f64,
        latency: f64,
    ) {
        if s.target == 0.0 || !(interval > 0.0) || !interval.is_finite() {
            return;
        }
        if interval > 250.0 {
            Self::resolution_reset(s);
            return;
        }
        let budget = 1000.0 / s.target;
        let known = gpu.is_finite() && gpu > 0.0;
        let fenced = !known && !behind.is_nan();
        let slow = if known {
            gpu > budget * 0.85
        } else {
            fenced && behind != 0.0
        };
        let quick = if known {
            gpu < budget * 0.7
        } else {
            fenced && behind == 0.0 && latency.is_finite() && latency < budget * 0.7
        };
        s.time += interval;
        s.age += interval;
        s.frames += 1.0;
        s.slow += f64::from(interval > budget * 1.15 && slow);
        s.cpu_bound += f64::from(cpu > budget * 0.85 && (!known || cpu > gpu * 1.2));
        s.headroom += f64::from(interval < budget * 1.08 && cpu < budget * 0.65 && quick);
        if s.time < 2000.0 || s.frames < 20.0 {
            return;
        }
        if s.slow / s.frames > 0.35 && s.cpu_bound / s.frames < 0.25 {
            s.scale = max(0.5, s.scale - 0.125);
            s.recovery = 0.0;
        } else if s.headroom / s.frames > 0.9 {
            s.recovery += s.time;
            if s.recovery >= 8000.0 {
                s.scale = min(1.0, s.scale + 0.0625);
                s.recovery = 0.0;
            }
        } else {
            s.recovery = 0.0;
        }
        s.time = 0.0;
        s.frames = 0.0;
        s.slow = 0.0;
        s.cpu_bound = 0.0;
        s.headroom = 0.0;
    }
    // The public client timing record also carries a world-clock view. The
    // shared pump owns the arithmetic; these copies are fixed scalar records.
    fn clock(s: &S) -> crate::gen::WorldClockState {
        crate::gen::WorldClockState {
            hz: s.hz,
            dt: s.dt,
            max_steps: s.max_steps,
            budget: s.budget,
            max_debt: s.max_debt,
            tick: s.tick,
            elapsed: s.elapsed,
            compensation: s.compensation,
            debt: s.debt,
            alpha: s.alpha,
            overloads: s.overloads,
            dropped: s.dropped,
            step_cost: s.step_cost,
        }
    }
    fn publish_clock(&mut self, mut s: S, c: crate::gen::WorldClockState) {
        s.tick = c.tick;
        s.elapsed = c.elapsed;
        s.compensation = c.compensation;
        s.debt = c.debt;
        s.alpha = c.alpha;
        s.overloads = c.overloads;
        s.dropped = c.dropped;
        s.step_cost = c.step_cost;
        self.write(&s);
    }
    pub fn begin(&mut self, seconds: f64, limited: bool) -> f64 {
        if self.depth == self.pumps.len() {
            return -2.0;
        }
        let s = self.read();
        let mut c = Self::clock(&s);
        let result = self.pumps[self.depth].begin(&mut c, seconds, limited);
        if result.is_ok() {
            self.depth += 1;
        }
        self.publish_clock(s, c);
        result.unwrap_or(-1.0)
    }
    pub fn complete(&mut self) {
        if self.depth == 0 {
            return;
        }
        let s = self.read();
        let mut c = Self::clock(&s);
        self.pumps[self.depth - 1].complete(&mut c);
        self.publish_clock(s, c);
    }
    pub fn start(&mut self, now: f64) {
        if self.depth > 0 {
            self.pumps[self.depth - 1].start(now);
        }
    }
    pub fn stepped(&mut self, start: f64, end: f64) -> bool {
        if self.depth == 0 {
            return false;
        }
        let s = self.read();
        let mut c = Self::clock(&s);
        let result = self.pumps[self.depth - 1].stepped(&mut c, start, end);
        self.publish_clock(s, c);
        result
    }
    pub fn finish(&mut self, limited: bool) -> f64 {
        if self.depth == 0 {
            return 0.0;
        }
        let s = self.read();
        let mut c = Self::clock(&s);
        self.depth -= 1;
        let result = self.pumps[self.depth].finish(&mut c, limited);
        self.publish_clock(s, c);
        result
    }
    pub fn abort(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    // Handles are private to the thin client-clock view. A finalizer releases
    // the cold allocation; no clock references the authoritative world store.
    #[no_mangle]
    pub extern "C" fn client_timing_create() -> *mut Timing {
        Box::into_raw(Box::new(Timing::new()))
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_timing_drop(p: *mut Timing) {
        if !p.is_null() {
            drop(Box::from_raw(p));
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_timing_words(p: *mut Timing) -> *mut f64 {
        (*p).words.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_timing_offsets(p: *mut Timing) -> *const f64 {
        (*p).offsets.as_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_timing_gaps(p: *mut Timing) -> *const f64 {
        (*p).gaps.as_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_timing_setup(p: *mut Timing) {
        (*p).setup();
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_timing_run(
        p: *mut Timing,
        op: u32,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
    ) -> f64 {
        (*p).run(op, a, b, c, d, e)
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_clock_begin(p: *mut Timing, s: f64, l: u32) -> f64 {
        (*p).begin(s, l != 0)
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_clock_start(p: *mut Timing, t: f64) {
        (*p).start(t);
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_clock_complete(p: *mut Timing) {
        (*p).complete();
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_clock_stepped(p: *mut Timing, a: f64, b: f64) -> u32 {
        u32::from((*p).stepped(a, b))
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_clock_abort(p: *mut Timing) {
        (*p).abort();
    }
    #[no_mangle]
    pub unsafe extern "C" fn client_clock_finish(p: *mut Timing, l: u32) -> f64 {
        (*p).finish(l != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn clock() -> Timing {
        let mut t = Timing::new();
        let s = S {
            hz: 60.0,
            window: 60.0,
            gap_window: 120.0,
            margin: 1.0,
            max_lead: 12.0,
            cooldown_acks: 30.0,
            calm_acks: 600.0,
            min_delay: 2.0,
            max_delay: 10.0,
            trim: 0.1,
            snap: 12.0,
            ..Default::default()
        };
        t.write(&s);
        t.setup();
        t
    }
    #[test]
    fn nested_advance_retains_outer_target_through_reset_and_excess_depth() {
        let mut t = clock();
        let mut s = t.read();
        s.dt = 1.0 / 60.0;
        s.max_steps = 2.0;
        s.budget = f64::INFINITY;
        s.max_debt = f64::INFINITY;
        t.write(&s);
        t.run(19, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(t.begin(0.05, false), 2.0);
        assert_eq!(t.begin(0.0, false), 2.0);
        t.complete();
        assert!(t.stepped(0.0, 0.0));
        t.complete();
        assert!(!t.stepped(0.0, 0.0));
        assert_eq!(t.finish(false), 2.0);
        t.run(19, 0.0, 0.0, 0.0, 0.0, 0.0);
        t.complete();
        assert!(t.stepped(0.0, 0.0));
        t.complete();
        assert!(!t.stepped(0.0, 0.0));
        assert_eq!(t.finish(false), 2.0);
        assert_eq!(t.read().debt, 1.0);
        for _ in 0..16 {
            assert!(t.begin(0.0, false) >= 0.0);
        }
        assert_eq!(t.begin(0.0, false), -2.0);
        for _ in 0..16 {
            t.abort();
        }
        assert_eq!(t.depth, 0);
    }
    #[test]
    fn long_paused_arrival_burst_keeps_bounded_rings_and_recovers() {
        let mut t = clock();
        t.run(3, 0.0, 0.0, 0.0, 0.0, 0.0);
        let capacities = (t.offsets.capacity(), t.gaps.capacity(), t.sorted.capacity());
        for tick in 1..100000 {
            t.run(4, tick as f64, 10000.0, 0.0, 0.0, 0.0);
        }
        let stamp = t.run(7, 10000.0, 99999.0, 0.0, 0.0, 0.0);
        assert!(stamp <= 100011.0);
        t.run(5, 120.0, 60.0, 1.0, 0.0, 0.0);
        assert_eq!(t.read().extra, 0.0);
        for tick in 100000..100360 {
            t.run(
                4,
                tick as f64,
                10000.0 + (tick - 99999) as f64 / 60.0,
                0.0,
                0.0,
                0.0,
            );
        }
        assert_eq!(
            capacities,
            (t.offsets.capacity(), t.gaps.capacity(), t.sorted.capacity())
        );
    }
    #[test]
    fn matching_refresh_cap_preserves_every_jittered_deadline() {
        for hz in [30.0, 60.0, 120.0, 144.0, 240.0] {
            let mut t = clock();
            t.run(12, hz, 0.0, 0.0, 0.0, 0.0);
            let mut previous = 0.0;
            for i in 1..=2400 {
                let now = i as f64 / hz + if i % 2 == 1 { 0.00008 } else { -0.00008 };
                assert!(t.run(14, now - previous, 0.0, 0.0, 0.0, 0.0).is_finite());
                previous = now;
            }
        }
    }
    #[test]
    fn percentile_uses_numeric_order_and_retained_scratch() {
        let mut t = clock();
        let mut s = t.read();
        s.gap_count = 4.0;
        t.gaps[..4].copy_from_slice(&[2.0, 10.0, 1.0, 20.0]);
        assert_eq!(t.measure(&mut s, 0.85), 20.0);
    }
}
