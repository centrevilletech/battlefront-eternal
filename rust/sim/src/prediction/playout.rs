//! Owned packet playout and sample bracket calculations.
//!
//! Allocation, observable slice/sort calls and overridden methods stay between
//! these pure phases. No model reference or borrowed result crosses a callback.

pub use super::reconcile::{MeasurementKey, RingWrite};
use jsrt::{js_max, js_min};

pub type Result<T> = std::result::Result<T, &'static str>;
const EXCHANGE: &str = "invalid playout exchange";
const ARITHMETIC: &str = "unsupported playout arithmetic";
const MAX_INTEGER: f64 = 9007199254740991.0;

fn flag(value: bool) -> f64 {
    u8::from(value) as f64
}
fn finite(value: f64) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ARITHMETIC)
    }
}
fn integer(value: f64) -> bool {
    value.is_finite() && value >= 0.0 && value <= MAX_INTEGER && value.fract() == 0.0
}

/// Publish these fields before the host allocates its replacement gap ring.
#[derive(Clone, Copy, Debug)]
pub struct ResetHead {
    pub clock: f64,
    pub latest: f64,
    pub delay: f64,
    pub rate: f64,
}
impl ResetHead {
    pub fn new(min_delay: f64) -> Result<Self> {
        Ok(Self {
            clock: 0.0,
            latest: 0.0,
            delay: finite(min_delay)?,
            rate: 1.0,
        })
    }
    pub fn words(self) -> [f64; 4] {
        [self.clock, self.latest, self.delay, self.rate]
    }
}

/// Publish only after the replacement Float64Array has been assigned. The
/// constructor may have mutated unrelated fields or thrown before this phase.
#[derive(Clone, Copy, Debug)]
pub struct ResetTail {
    pub gap_at: f64,
    pub gap_count: f64,
    pub arrived_at: Option<f64>,
    pub arrived_tick: f64,
    pub started: bool,
    pub waits: f64,
    pub measured_key: f64,
}
impl Default for ResetTail {
    fn default() -> Self {
        Self {
            gap_at: 0.0,
            gap_count: 0.0,
            arrived_at: None,
            arrived_tick: 0.0,
            started: false,
            waits: 0.0,
            measured_key: -1.0,
        }
    }
}
impl ResetTail {
    pub fn words(self) -> [f64; 8] {
        [
            self.gap_at,
            self.gap_count,
            flag(self.arrived_at.is_some()),
            self.arrived_at.unwrap_or(0.0),
            self.arrived_tick,
            flag(self.started),
            self.waits,
            self.measured_key,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArrivalInput {
    pub hz: f64,
    pub window: f64,
    pub latest: f64,
    pub arrived_at: Option<f64>,
    pub arrived_tick: f64,
    pub gap_at: f64,
    pub gap_count: f64,
    pub ring_length: usize,
    pub tick: f64,
    pub now: f64,
    pub interval: f64,
}

/// A single optional ring write preserves every untouched byte and the host's
/// ring identity. Write flags also retain the original conditional assignments.
#[derive(Clone, Copy, Debug)]
pub struct Arrival {
    pub write_latest: bool,
    pub latest: f64,
    pub advance_gap: bool,
    pub ring_write: Option<RingWrite>,
    pub gap_at: f64,
    pub write_count: bool,
    pub gap_count: f64,
    pub arrived_at: f64,
    pub arrived_tick: f64,
}
impl Arrival {
    pub fn words(self) -> [f64; 10] {
        [
            flag(self.write_latest),
            self.latest,
            flag(self.advance_gap),
            self.ring_write.map_or(-1.0, |v| v.index as f64),
            self.ring_write.map_or(0.0, |v| v.value),
            self.gap_at,
            flag(self.write_count),
            self.gap_count,
            self.arrived_at,
            self.arrived_tick,
        ]
    }
}

pub fn arrive(input: ArrivalInput) -> Result<Arrival> {
    finite(input.latest)?;
    finite(input.arrived_tick)?;
    finite(input.tick)?;
    finite(input.now)?;
    if !integer(input.window)
        || input.window == 0.0
        || !integer(input.gap_at)
        || input.gap_at >= input.window
        || !integer(input.gap_count)
        || input.gap_count > input.window
    {
        return Err(ARITHMETIC);
    }
    let mut result = Arrival {
        write_latest: input.tick > input.latest,
        latest: if input.tick > input.latest {
            input.tick
        } else {
            input.latest
        },
        advance_gap: false,
        ring_write: None,
        gap_at: input.gap_at,
        write_count: false,
        gap_count: input.gap_count,
        arrived_at: input.now,
        arrived_tick: js_max(input.arrived_tick, input.tick),
    };
    if let Some(previous) = input.arrived_at {
        finite(previous)?;
        if input.tick > input.arrived_tick {
            finite(input.hz)?;
            finite(input.interval)?;
            // Keep every arithmetic operation in the original evaluation order.
            let elapsed = finite(input.now - previous)?;
            let ticks = finite(elapsed * input.hz)?;
            let late = finite(ticks - input.interval)?;
            let gap = js_max(0.0, finite(late + 1.0)?);
            result.advance_gap = true;
            if input.gap_at < input.ring_length as f64 {
                result.ring_write = Some(RingWrite {
                    index: input.gap_at as usize,
                    value: gap,
                });
            }
            result.gap_at = (input.gap_at + 1.0) % input.window;
            if input.gap_count < input.window {
                result.write_count = true;
                result.gap_count += 1.0;
            }
        }
    }
    Ok(result)
}

/// Publish a changed key before slice, sort, species or percentile callbacks.
pub fn measurement_key(gap_count: f64, gap_at: f64, previous: f64) -> Result<MeasurementKey> {
    finite(gap_count)?;
    finite(gap_at)?;
    finite(previous)?;
    finite(gap_count * 4096.0)?;
    let result = super::reconcile::measurement_key(gap_count, gap_at, previous);
    finite(result.key)?;
    Ok(result)
}

/// The intrinsic slice path supplies exactly its sampled prefix. NaNs and
/// exceptional samples stay in JavaScript, including V8's NaN payload order.
pub fn measure_gap(samples: &[f64]) -> Result<f64> {
    for &sample in samples {
        finite(sample)?;
    }
    if samples.is_empty() {
        return Ok(0.0);
    }
    let mut sorted = samples.to_vec();
    measure_gap_in_place(&mut sorted)

}

/// Native hosts lend retained scratch instead of allocating a sorted copy.
/// Finite numeric order, including signed zero, matches TypedArray.sort;
/// equal finite values have identical bits, so unstable tie order is inert.
pub fn measure_gap_in_place(samples:&mut[f64])->Result<f64>{
    for &sample in samples.iter(){finite(sample)?;}
    if samples.is_empty(){return Ok(0.0);}
    samples.sort_unstable_by(f64::total_cmp);
    let count=samples.len() as f64;
    let at=js_min(count-1.0,(count*0.95).floor())as usize;
    Ok(samples[at])
}

/// Capture these values after the host's original custom sort and percentile
/// lookup, if any. Inverted bounds remain meaningful and are not normalized.
pub fn measure_delay(gap: f64, margin: f64, min_delay: f64, max_delay: f64) -> Result<f64> {
    finite(gap)?;
    finite(margin)?;
    finite(min_delay)?;
    finite(max_delay)?;
    Ok(js_min(max_delay, js_max(min_delay, finite(gap + margin)?)))
}

#[derive(Clone, Copy, Debug)]
pub struct AdvanceInput {
    pub latest: f64,
    pub delay: f64,
    pub clock: f64,
    pub started: bool,
    pub hz: f64,
    pub trim: f64,
    pub snap: f64,
    pub dt: f64,
    pub waits: f64,
}

/// The mode supplies the host's exact assignment order before its final fresh
/// clock read. Unwritten rate/waits words are zero placeholders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AdvanceMode {
    Idle = 0,
    Start = 1,
    Snap = 2,
    Run = 3,
    Wait = 4,
}

#[derive(Clone, Copy, Debug)]
pub struct Advance {
    pub mode: AdvanceMode,
    pub clock: f64,
    pub rate: f64,
    pub waits: f64,
}
impl Advance {
    pub fn words(self) -> [f64; 4] {
        [self.mode as u8 as f64, self.clock, self.rate, self.waits]
    }
}

/// Invoke only after the current measure() method has returned. All results
/// own their values; the host publishes them before any diagnostic callback.
pub fn advance(input: AdvanceInput) -> Result<Advance> {
    finite(input.latest)?;
    finite(input.delay)?;
    let target = finite(input.latest - input.delay)?;
    if !input.started {
        return if input.latest <= 0.0 {
            Ok(Advance {
                mode: AdvanceMode::Idle,
                clock: finite(input.clock)?,
                rate: 0.0,
                waits: 0.0,
            })
        } else {
            Ok(Advance {
                mode: AdvanceMode::Start,
                clock: js_max(0.0, target),
                rate: 1.0,
                waits: 0.0,
            })
        };
    }
    finite(input.clock)?;
    finite(input.snap)?;
    let error = finite(target - input.clock)?;
    if error.abs() > input.snap {
        return Ok(Advance {
            mode: AdvanceMode::Snap,
            clock: js_max(0.0, target),
            rate: 1.0,
            waits: 0.0,
        });
    }
    finite(input.hz)?;
    finite(input.trim)?;
    finite(input.dt)?;
    let upper = finite(1.0 + input.trim)?;
    let lower = finite(1.0 - input.trim)?;
    let ratio = finite(error / input.hz)?;
    let rate = js_min(upper, js_max(lower, finite(1.0 + ratio)?));
    let ticks = finite(input.dt * input.hz)?;
    let travel = finite(ticks * rate)?;
    let mut next = finite(input.clock + travel)?;
    let (mode, waits) = if next > input.latest {
        next = input.latest;
        (AdvanceMode::Wait, finite(finite(input.waits)? + 1.0)?)
    } else {
        (AdvanceMode::Run, 0.0)
    };
    Ok(Advance {
        mode,
        clock: js_max(input.clock, next),
        rate,
        waits,
    })
}

pub fn shown_tick(clock: f64) -> Result<f64> {
    finite(finite(clock)?.floor() + 1.0)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Interval {
    pub previous: Option<f64>,
    pub current: Option<f64>,
}
impl Interval {
    pub fn reset(&mut self) {
        self.previous = None;
        self.current = None;
    }
    pub fn words(self) -> [f64; 4] {
        [
            flag(self.previous.is_some()),
            self.previous.unwrap_or(0.0),
            flag(self.current.is_some()),
            self.current.unwrap_or(0.0),
        ]
    }
    /// None represents a nullish old current for this operation. It can mean
    /// either null or undefined; both produce tick - 1 before current is set.
    pub fn apply(&mut self, tick: f64) -> Result<()> {
        finite(tick)?;
        let previous = match self.current {
            Some(value) => finite(value)?,
            None => finite(tick - 1.0)?,
        };
        self.previous = Some(previous);
        self.current = Some(tick);
        Ok(())
    }
    /// For sampling and alpha, None represents exact null. Undefined fields
    /// use the host's original path, retaining its coercions and NaNs.
    pub fn needs_sample(&self, clock: f64) -> Result<bool> {
        match self.current {
            None => Ok(true),
            Some(current) => Ok(finite(current)? <= finite(clock)?),
        }
    }
    pub fn alpha(&self, clock: f64) -> Result<f64> {
        let Some(current) = self.current else {
            return Ok(1.0);
        };
        finite(current)?;
        finite(clock)?;
        let previous = finite(self.previous.unwrap_or(0.0))?;
        let numerator = finite(clock - previous)?;
        let span = finite(current - previous)?;
        let fraction = finite(numerator / js_max(1.0, span))?;
        Ok(js_max(0.0, js_min(1.0, fraction)))
    }
}

/// None marks a Number.isFinite rejection, without coercing the original JS
/// argument. Its short circuit skips every later arithmetic operation.
pub fn rendered_view_tick(clock: Option<f64>, latest: Option<f64>) -> Result<f64> {
    let Some(clock) = clock else {
        return Ok(0.0);
    };
    finite(clock)?;
    let Some(latest) = latest else {
        return Ok(0.0);
    };
    finite(latest)?;
    Ok(js_max(0.0, js_min(latest.floor(), clock.floor())))
}

struct Words<'a> {
    input: &'a [f64],
    at: usize,
}
impl Words<'_> {
    fn number(&mut self) -> Result<f64> {
        let value = *self.input.get(self.at).ok_or(EXCHANGE)?;
        self.at += 1;
        Ok(value)
    }
    fn flag(&mut self) -> Result<bool> {
        match self.number()? {
            0.0 => Ok(false),
            1.0 => Ok(true),
            _ => Err(EXCHANGE),
        }
    }
    fn nullable(&mut self) -> Result<Option<f64>> {
        let present = self.flag()?;
        let value = self.number()?;
        Ok(present.then_some(value))
    }
    fn count(&mut self, bound: usize) -> Result<usize> {
        let value = self.number()?;
        if integer(value) && value <= bound as f64 {
            Ok(value as usize)
        } else {
            Err(EXCHANGE)
        }
    }
    fn samples(&mut self) -> Result<&[f64]> {
        let count = self.count((self.input.len() - self.at).saturating_sub(1))?;
        let samples = &self.input[self.at..self.at + count];
        self.at += count;
        Ok(samples)
    }
    fn finish(&self) -> Result<()> {
        if self.at == self.input.len() {
            Ok(())
        } else {
            Err(EXCHANGE)
        }
    }
}

/// Reusable owned scalar exchange with no pending phase or foreign calls.
/// Every operation validates the whole input before publishing its output.
/// Booleans are 0/1 and nullable values are [present,value].
///
/// - 0 reset head: [minDelay] -> [clock,latest,delay,rate].
/// - 1 reset tail: [] -> [gapAt,gapCount,arrivedAt?,arrivedTick,started,waits,key].
/// - 2 arrival: [hz,window,latest,arrivedAt?,arrivedTick,gapAt,gapCount,ringLength,
///   tick,now,interval] -> [writeLatest,latest,advanceGap,writeIndex,writeValue,
///   gapAt,writeCount,gapCount,arrivedAt,arrivedTick]. No ring write uses index -1.
/// - 3 key: [gapCount,gapAt,key] -> [changed,key]. Publish before slice/sort.
/// - 4 gap: [count,...samples] -> [95thPercentile]. Only an intrinsic prefix.
/// - 5 delay: [gap,margin,minDelay,maxDelay] -> [delay].
/// - 6 advance: [latest,delay,clock,started,hz,trim,snap,dt,waits]
///   -> [mode,clock,rate,waits]. Modes: Idle=0, Start=1, Snap=2, Run=3, Wait=4.
/// - 7 shown: [clock] -> [shownTick].
/// - 8 interval reset: [] -> [previous?,current?].
/// - 9 interval apply: [current?,tick] -> [previous,current].
/// - 10 needs sample: [current?,clock] -> [needsSample]. Null skips clock.
/// - 11 alpha: [current?,previous?,clock] -> [alpha]. Null current skips all.
/// - 12 rendered: [finiteClock?,finiteLatest?] -> [renderedViewTick].
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
}
impl Session {
    pub fn run(&mut self, op: u32) -> Result<()> {
        self.output.clear();
        let mut r = Words {
            input: &self.input,
            at: 0,
        };
        let output = match op {
            0 => ResetHead::new(r.number()?)?.words().to_vec(),
            1 => ResetTail::default().words().to_vec(),
            2 => arrive(ArrivalInput {
                hz: r.number()?,
                window: r.number()?,
                latest: r.number()?,
                arrived_at: r.nullable()?,
                arrived_tick: r.number()?,
                gap_at: r.number()?,
                gap_count: r.number()?,
                ring_length: r.count(u32::MAX as usize)?,
                tick: r.number()?,
                now: r.number()?,
                interval: r.number()?,
            })?
            .words()
            .to_vec(),
            3 => {
                let key = measurement_key(r.number()?, r.number()?, r.number()?)?;
                vec![flag(key.changed), key.key]
            }
            4 => vec![measure_gap(r.samples()?)?],
            5 => vec![measure_delay(
                r.number()?,
                r.number()?,
                r.number()?,
                r.number()?,
            )?],
            6 => advance(AdvanceInput {
                latest: r.number()?,
                delay: r.number()?,
                clock: r.number()?,
                started: r.flag()?,
                hz: r.number()?,
                trim: r.number()?,
                snap: r.number()?,
                dt: r.number()?,
                waits: r.number()?,
            })?
            .words()
            .to_vec(),
            7 => vec![shown_tick(r.number()?)?],
            8 => Interval::default().words().to_vec(),
            9 => {
                let mut interval = Interval {
                    previous: None,
                    current: r.nullable()?,
                };
                interval.apply(r.number()?)?;
                vec![interval.previous.unwrap(), interval.current.unwrap()]
            }
            10 => {
                let interval = Interval {
                    previous: None,
                    current: r.nullable()?,
                };
                vec![flag(interval.needs_sample(r.number()?)?)]
            }
            11 => {
                let interval = Interval {
                    current: r.nullable()?,
                    previous: r.nullable()?,
                };
                vec![interval.alpha(r.number()?)?]
            }
            12 => vec![rendered_view_tick(r.nullable()?, r.nullable()?)?],
            _ => return Err(EXCHANGE),
        };
        r.finish()?;
        self.output = output;
        Ok(())
    }
}
