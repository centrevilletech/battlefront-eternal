//! Input timing, correction smoothing and private replay window decisions.
//!
//! Each operation owns its input and result. Estimator methods, percentile
//! sampling, reset callbacks and exact checkpoint comparisons remain separate
//! calls so a host can publish state before invoking observable user code.

use jsrt::{js_hypot, js_max, js_min, js_round};

pub type Result<T> = std::result::Result<T, &'static str>;
const EXCHANGE: &str = "invalid reconciliation exchange";

fn flag(value: bool) -> f64 {
    u8::from(value) as f64
}

/// The sole changed cell of an existing ring. A host need not rewrite other
/// cells, whose contents may include signaling NaNs or externally held views.
#[derive(Clone, Copy, Debug)]
pub struct RingWrite {
    pub index: usize,
    pub value: f64,
}

fn write_ring(values: &mut [f64], at: f64, value: f64) -> Option<RingWrite> {
    if at >= 0.0 && at < values.len() as f64 && at.fract() == 0.0 {
        let index = at as usize;
        values[index] = value;
        Some(RingWrite { index, value })
    } else {
        None
    }
}

fn append_write(output: &mut Vec<f64>, write: Option<RingWrite>) {
    match write {
        Some(write) => output.extend([write.index as f64, write.value]),
        None => output.extend([-1.0, 0.0]),
    }
}

/// A copied estimator, including its retained ring contents. Null offset and
/// dropped values are distinct from every number, including NaN and zero.
#[derive(Clone, Debug)]
pub struct EstimatorState {
    pub hz: f64,
    pub window: f64,
    pub at: f64,
    pub count: f64,
    pub offset: Option<f64>,
    pub dropped: Option<f64>,
    pub offsets: Vec<f64>,
}

impl EstimatorState {
    pub fn reset(&mut self) {
        self.at = 0.0;
        self.count = 0.0;
        self.offset = None;
        self.dropped = None;
    }

    pub fn arrive(&mut self, tick: f64, now: f64, dropped: f64) -> Option<RingWrite> {
        if self.dropped.is_some_and(|previous| dropped != previous) {
            self.at = 0.0;
            self.count = 0.0;
        }
        self.dropped = Some(dropped);
        let write = write_ring(&mut self.offsets, self.at, tick - now * self.hz);
        self.at = (self.at + 1.0) % self.window;
        if self.count < self.window {
            self.count += 1.0;
        }
        let mut best = f64::NEG_INFINITY;
        for (i, &offset) in self.offsets.iter().enumerate() {
            // A strict comparison retains the first zero's sign and ignores
            // NaNs, unlike Math.max. Out-of-range JS reads cannot improve it.
            if (i as f64) < self.count && offset > best {
                best = offset;
            }
        }
        self.offset = Some(best);
        write
    }

    pub fn estimate(&self, now: f64) -> Option<f64> {
        estimate(self.hz, self.offset, now)
    }

    pub fn scalars(&self) -> [f64; 6] {
        [
            self.at,
            self.count,
            flag(self.offset.is_some()),
            self.offset.unwrap_or(0.0),
            flag(self.dropped.is_some()),
            self.dropped.unwrap_or(0.0),
        ]
    }
}

pub fn estimate(hz: f64, offset: Option<f64>, now: f64) -> Option<f64> {
    offset.map(|offset| offset + now * hz)
}

/// Scalar state after the host calls its current estimator.reset() and
/// gaps.fill(0). Those two calls can replace state or throw before this phase.
#[derive(Clone, Debug)]
pub struct LeadReset {
    pub gap_at: f64,
    pub gap_count: f64,
    pub arrived_at: Option<f64>,
    pub arrived_tick: f64,
    pub latest: f64,
    pub lead: f64,
    pub late: f64,
    pub worst_gap: f64,
    pub extra: f64,
    pub cooldown: f64,
    pub calm: f64,
    pub measured_key: f64,
    pub capped: bool,
}

impl LeadReset {
    pub fn new(margin: f64) -> Self {
        Self {
            gap_at: 0.0,
            gap_count: 0.0,
            arrived_at: None,
            arrived_tick: 0.0,
            latest: 0.0,
            lead: margin + 1.0,
            late: 0.0,
            worst_gap: 0.0,
            extra: 0.0,
            cooldown: 0.0,
            calm: 0.0,
            measured_key: -1.0,
            capped: false,
        }
    }

    pub fn words(&self) -> [f64; 14] {
        [
            self.gap_at,
            self.gap_count,
            flag(self.arrived_at.is_some()),
            self.arrived_at.unwrap_or(0.0),
            self.arrived_tick,
            self.latest,
            self.lead,
            self.late,
            self.worst_gap,
            self.extra,
            self.cooldown,
            self.calm,
            self.measured_key,
            flag(self.capped),
        ]
    }
}

/// Arrival state captured after the host's current estimator.arrive() returns.
#[derive(Clone, Debug)]
pub struct LeadArrival {
    pub hz: f64,
    pub latest: f64,
    pub arrived_at: Option<f64>,
    pub arrived_tick: f64,
    pub gap_at: f64,
    pub gap_count: f64,
    pub gaps: Vec<f64>,
}

impl LeadArrival {
    pub fn arrive(&mut self, tick: f64, now: f64) -> Option<RingWrite> {
        if tick > self.latest {
            self.latest = tick;
        }
        let mut write = None;
        if let Some(arrived_at) = self.arrived_at {
            if tick > self.arrived_tick {
                let gap = js_max(
                    0.0,
                    (now - arrived_at) * self.hz - (tick - self.arrived_tick),
                );
                write = write_ring(&mut self.gaps, self.gap_at, gap);
                self.gap_at = (self.gap_at + 1.0) % self.gaps.len() as f64;
                if self.gap_count < self.gaps.len() as f64 {
                    self.gap_count += 1.0;
                }
            }
        }
        self.arrived_at = Some(now);
        self.arrived_tick = js_max(self.arrived_tick, tick);
        write
    }

    pub fn scalars(&self) -> [f64; 6] {
        [
            self.latest,
            flag(self.arrived_at.is_some()),
            self.arrived_at.unwrap_or(0.0),
            self.arrived_tick,
            self.gap_at,
            self.gap_count,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AcknowledgementPolicy {
    pub max_lead: f64,
    pub cooldown_acks: f64,
    pub calm_acks: f64,
    pub recover: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AcknowledgementState {
    pub late: f64,
    pub extra: f64,
    pub cooldown: f64,
    pub calm: f64,
}

impl AcknowledgementState {
    pub fn acknowledge(
        &mut self,
        policy: AcknowledgementPolicy,
        late: f64,
        elapsed_ticks: f64,
        capped: bool,
    ) {
        let elapsed = js_max(1.0, js_min(60.0, elapsed_ticks));
        if self.cooldown > 0.0 {
            self.cooldown = js_max(0.0, self.cooldown - elapsed);
        }
        if late > 0.0 {
            self.late += 1.0;
            if capped {
                return;
            }
            self.calm = 0.0;
            if !jsrt::num_truthy(self.cooldown) {
                self.extra = js_min(policy.max_lead, self.extra + late);
                self.cooldown = policy.cooldown_acks;
            }
        } else {
            self.calm += elapsed;
            if self.calm >= policy.calm_acks {
                self.calm = if policy.recover {
                    policy.calm_acks
                } else {
                    0.0
                };
                self.extra = js_max(
                    0.0,
                    self.extra - if policy.recover { 0.1 * elapsed } else { 1.0 },
                );
            }
        }
    }

    pub fn words(self) -> [f64; 4] {
        [self.late, self.extra, self.cooldown, self.calm]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MeasurementKey {
    pub changed: bool,
    pub key: f64,
}

pub fn measurement_key(gap_count: f64, gap_at: f64, previous: f64) -> MeasurementKey {
    let key = gap_count * 4096.0 + gap_at;
    MeasurementKey {
        changed: key != previous,
        key,
    }
}

/// Called only after the new measuredKey has been published. The host keeps
/// custom slice, sort and species calls in its original continuation.
pub fn measure_gap(gaps: &[f64], gap_count: usize) -> Result<f64> {
    let mut sorted = gaps.get(..gap_count).ok_or(EXCHANGE)?.to_vec();
    if gap_count == 0 {
        return Ok(0.0);
    }
    jsrt::typed_sort_f64(&mut sorted);
    let at = js_min(gap_count as f64 - 1.0, (gap_count as f64 * 0.85).floor()) as usize;
    Ok(sorted[at])
}

/// Values captured after this.measure() and the current estimator.estimate().
/// The latest argument is the value captured when the original call began.
#[derive(Clone, Copy, Debug)]
pub struct LeadTargetInput {
    pub hz: f64,
    pub margin: f64,
    pub max_lead: f64,
    pub worst_gap: f64,
    pub extra: f64,
    pub extrapolate: bool,
    pub estimate: Option<f64>,
    pub latest: f64,
    pub round_trip_seconds: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct LeadTarget {
    pub bounded: f64,
    pub capped: bool,
    pub lead: f64,
}

pub fn target_lead(input: LeadTargetInput) -> LeadTarget {
    let transit = js_max(0.0, js_min(1.0, input.round_trip_seconds)) * input.hz;
    let slack = 0.5
        + input.margin
        + js_min(
            input.max_lead,
            transit + js_min(4.0, js_round(input.worst_gap)) + input.extra,
        );
    let target = input.estimate.unwrap_or(input.latest) + slack;
    let reference = if input.extrapolate {
        input
            .estimate
            .map_or(input.latest, |estimate| js_max(input.latest, estimate))
    } else {
        input.latest
    };
    let bounded = js_min(js_max(target, reference + 1.0), reference + input.max_lead);
    LeadTarget {
        bounded,
        capped: target > bounded,
        lead: bounded - input.latest,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SmootherState {
    pub offset: [f64; 3],
    pub count: f64,
    pub worst: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SmootherNote {
    pub size: f64,
    pub changed: bool,
    pub reset: bool,
}

impl SmootherState {
    pub fn reset(&mut self) {
        self.offset = [0.0; 3];
    }

    /// Returns before any reset callback. Publish offset/count/worst first,
    /// then call the current reset method and increment the current snaps.
    pub fn note(&mut self, old: [f64; 3], new: [f64; 3], snap: f64) -> SmootherNote {
        let delta = [old[0] - new[0], old[1] - new[1], old[2] - new[2]];
        let size = js_hypot(&delta);
        if size < 1e-6 {
            return SmootherNote {
                size: 0.0,
                changed: false,
                reset: false,
            };
        }
        self.count += 1.0;
        if size > self.worst {
            self.worst = size;
        }
        self.offset[0] += delta[0];
        self.offset[1] += delta[1];
        self.offset[2] += delta[2];
        SmootherNote {
            size,
            changed: true,
            reset: self.length() > snap,
        }
    }

    /// Returns whether the host must call its current reset method after the
    /// new offsets are visible. Reset statistics are never changed here.
    pub fn decay(&mut self, tau: f64, dt: f64) -> bool {
        let k = v8math::exp(-js_max(0.0, dt) / tau);
        self.offset[0] *= k;
        self.offset[1] *= k;
        self.offset[2] *= k;
        self.length() < 1e-4
    }

    pub fn length(&self) -> f64 {
        js_hypot(&self.offset)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WindowDecision {
    Reset = 0,
    Keep = 1,
    AskExact = 2,
}

/// The host retains the actual checkpoint object through AskExact. A true
/// result updates that old object's authorityTick even if the callback saved
/// or reset the current window; a false result calls the current reset method.
pub fn select_checkpoint(
    same_life: bool,
    authority_tick: f64,
    checkpoint_authority_tick: f64,
    checkpoint_tick: f64,
    limit: f64,
) -> WindowDecision {
    if same_life
        && (authority_tick == checkpoint_authority_tick || authority_tick < checkpoint_tick - limit)
    {
        WindowDecision::Keep
    } else if same_life
        && authority_tick > checkpoint_authority_tick
        && authority_tick <= checkpoint_tick
    {
        WindowDecision::AskExact
    } else {
        WindowDecision::Reset
    }
}

/// Source indices keep command object and nested input identities in the host.
pub fn filter_after_sequence(sequences: &[f64], checkpoint_sequence: f64) -> Vec<usize> {
    sequences
        .iter()
        .enumerate()
        .filter_map(|(i, &sequence)| (sequence > checkpoint_sequence).then_some(i))
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub struct PredictionCommand {
    pub generation: f64,
    pub epoch: f64,
    pub tick: f64,
}

/// The filter uses a strict lower tick bound, then Array.slice(-limit), whose
/// start is truncated independently of the subtraction used by the filter.
pub fn retain_commands(
    commands: &[PredictionCommand],
    tick: f64,
    generation: f64,
    epoch: f64,
    limit: f64,
) -> Vec<usize> {
    let mut kept: Vec<_> = commands
        .iter()
        .enumerate()
        .filter_map(|(i, command)| {
            (command.generation == generation
                && command.epoch == epoch
                && command.tick > tick - limit)
                .then_some(i)
        })
        .collect();
    let relative = jsrt::to_integer_or_infinity(-limit);
    let start = if relative < 0.0 {
        js_max(kept.len() as f64 + relative, 0.0)
    } else {
        js_min(relative, kept.len() as f64)
    } as usize;
    kept.drain(..start);
    kept
}

struct Words<'a> {
    input: &'a [f64],
    at: usize,
}

impl<'a> Words<'a> {
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

    fn count(&mut self) -> Result<usize> {
        count(self.number()?, self.input.len() - self.at)
    }

    fn list(&mut self) -> Result<Vec<f64>> {
        let n = self.count()?;
        let result = self
            .input
            .get(self.at..self.at + n)
            .ok_or(EXCHANGE)?
            .to_vec();
        self.at += n;
        Ok(result)
    }

    fn vector(&mut self) -> Result<[f64; 3]> {
        Ok([self.number()?, self.number()?, self.number()?])
    }

    fn finish(&self) -> Result<()> {
        if self.at == self.input.len() {
            Ok(())
        } else {
            Err(EXCHANGE)
        }
    }
}

fn count(value: f64, bound: usize) -> Result<usize> {
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= bound as f64 {
        Ok(value as usize)
    } else {
        Err(EXCHANGE)
    }
}

fn indices_words(indices: Vec<usize>) -> Vec<f64> {
    let mut output = Vec::with_capacity(indices.len() + 1);
    output.push(indices.len() as f64);
    output.extend(indices.into_iter().map(|i| i as f64));
    output
}

/// A stateless numeric exchange. Flags are 0/1, nullable numbers are
/// [present,value], and a list is [length,...values]. Outputs are owned before
/// any host callback; this same Session can therefore serve nested callers.
///
/// Opcodes and word layouts:
/// - 0: [] -> [at,count,offset?,offset,dropped?,dropped] after estimator reset.
/// - 1: [hz,window,at,count,offset?,offset,dropped?,dropped,tick,now,newDropped,
///   offsetsList] -> [at,count,offset?,offset,dropped?,dropped,writeIndex,value].
/// - 2: [hz,offset?,offset,now] -> [estimate?,estimate].
/// - 3: [margin] -> LeadReset::words(), after host estimator reset and gap fill.
/// - 4: [hz,latest,arrived?,arrived,arrivedTick,gapAt,gapCount,tick,now,gapsList]
///   -> [latest,arrived?,arrived,arrivedTick,gapAt,gapCount,writeIndex,value].
/// - 5: [maxLead,cooldownAcks,calmAcks,recover,lateCount,extra,cooldown,calm,
///   late,elapsedTicks,capped] -> [lateCount,extra,cooldown,calm].
/// - 6: [gapCount,gapAt,measuredKey] -> [changed,key]. Publish key before 7.
/// - 7: [gapCount,gapsList] -> [gap]. gapCount is an integer within the list.
/// - 8: [hz,margin,maxLead,worstGap,extra,extrapolate,estimate?,estimate,latest,
///   roundTrip] -> [bounded,capped,lead], after host measure and estimate calls.
/// - 9: [] -> [x,y,z] after smoother reset; statistics are retained by the host.
/// - 10: [x,y,z,count,worst,snap,oldXYZ,newXYZ]
///   -> [size,changed,x,y,z,count,worst,reset]. Publish before reset/snaps++.
/// - 11: [x,y,z,tau,dt] -> [x,y,z,reset]. Publish before the reset callback.
/// - 12: [x,y,z] -> [length].
/// - 13: [sameLife,authorityTick,cAuthorityTick,cTick,limit] -> [WindowDecision].
/// - 14: [checkpointSequence,sequencesList] -> [count,...sourceIndices].
/// - 15: [tick,generation,epoch,limit,count,...(generation,epoch,tick)]
///   -> [count,...sourceIndices].
///
/// Ring writeIndex is -1 if no valid element was written. Other ring bytes
/// must not be republished. Inputs with observable getters or coercions stay
/// in the host's original phase; no JavaScript object identity crosses here.
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    error: Option<&'static str>,
}

impl Session {
    pub fn run(&mut self, op: u32) -> Result<()> {
        self.output.clear();
        let mut r = Words {
            input: &self.input,
            at: 0,
        };
        let output = match op {
            0 => vec![0.0; 6],
            1 => {
                let mut state = EstimatorState {
                    hz: r.number()?,
                    window: r.number()?,
                    at: r.number()?,
                    count: r.number()?,
                    offset: r.nullable()?,
                    dropped: r.nullable()?,
                    offsets: Vec::new(),
                };
                let tick = r.number()?;
                let now = r.number()?;
                let dropped = r.number()?;
                state.offsets = r.list()?;
                let write = state.arrive(tick, now, dropped);
                let mut output = state.scalars().to_vec();
                append_write(&mut output, write);
                output
            }
            2 => {
                let hz = r.number()?;
                let offset = r.nullable()?;
                let value = estimate(hz, offset, r.number()?);
                vec![flag(value.is_some()), value.unwrap_or(0.0)]
            }
            3 => LeadReset::new(r.number()?).words().to_vec(),
            4 => {
                let mut state = LeadArrival {
                    hz: r.number()?,
                    latest: r.number()?,
                    arrived_at: r.nullable()?,
                    arrived_tick: r.number()?,
                    gap_at: r.number()?,
                    gap_count: r.number()?,
                    gaps: Vec::new(),
                };
                let tick = r.number()?;
                let now = r.number()?;
                state.gaps = r.list()?;
                let write = state.arrive(tick, now);
                let mut output = state.scalars().to_vec();
                append_write(&mut output, write);
                output
            }
            5 => {
                let policy = AcknowledgementPolicy {
                    max_lead: r.number()?,
                    cooldown_acks: r.number()?,
                    calm_acks: r.number()?,
                    recover: r.flag()?,
                };
                let mut state = AcknowledgementState {
                    late: r.number()?,
                    extra: r.number()?,
                    cooldown: r.number()?,
                    calm: r.number()?,
                };
                state.acknowledge(policy, r.number()?, r.number()?, r.flag()?);
                state.words().to_vec()
            }
            6 => {
                let key = measurement_key(r.number()?, r.number()?, r.number()?);
                vec![flag(key.changed), key.key]
            }
            7 => {
                let gap_count = r.number()?;
                let gaps = r.list()?;
                vec![measure_gap(&gaps, count(gap_count, gaps.len())?)?]
            }
            8 => {
                let input = LeadTargetInput {
                    hz: r.number()?,
                    margin: r.number()?,
                    max_lead: r.number()?,
                    worst_gap: r.number()?,
                    extra: r.number()?,
                    extrapolate: r.flag()?,
                    estimate: r.nullable()?,
                    latest: r.number()?,
                    round_trip_seconds: r.number()?,
                };
                let target = target_lead(input);
                vec![target.bounded, flag(target.capped), target.lead]
            }
            9 => vec![0.0; 3],
            10 => {
                let mut state = SmootherState {
                    offset: r.vector()?,
                    count: r.number()?,
                    worst: r.number()?,
                };
                let snap = r.number()?;
                let note = state.note(r.vector()?, r.vector()?, snap);
                vec![
                    note.size,
                    flag(note.changed),
                    state.offset[0],
                    state.offset[1],
                    state.offset[2],
                    state.count,
                    state.worst,
                    flag(note.reset),
                ]
            }
            11 => {
                let mut state = SmootherState {
                    offset: r.vector()?,
                    ..SmootherState::default()
                };
                let reset = state.decay(r.number()?, r.number()?);
                vec![
                    state.offset[0],
                    state.offset[1],
                    state.offset[2],
                    flag(reset),
                ]
            }
            12 => vec![js_hypot(&r.vector()?)],
            13 => vec![select_checkpoint(
                r.flag()?,
                r.number()?,
                r.number()?,
                r.number()?,
                r.number()?,
            ) as u8 as f64],
            14 => {
                let sequence = r.number()?;
                indices_words(filter_after_sequence(&r.list()?, sequence))
            }
            15 => {
                let tick = r.number()?;
                let generation = r.number()?;
                let epoch = r.number()?;
                let limit = r.number()?;
                let n = r.count()?;
                if n > (r.input.len() - r.at) / 3 {
                    return Err(EXCHANGE);
                }
                let mut commands = Vec::with_capacity(n);
                for _ in 0..n {
                    commands.push(PredictionCommand {
                        generation: r.number()?,
                        epoch: r.number()?,
                        tick: r.number()?,
                    });
                }
                indices_words(retain_commands(&commands, tick, generation, epoch, limit))
            }
            _ => return Err(EXCHANGE),
        };
        r.finish()?;
        self.output = output;
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn prediction_reconcile_create() -> *mut Session {
    Box::into_raw(Box::new(Session::default()))
}

#[no_mangle]
pub unsafe extern "C" fn prediction_reconcile_drop(p: *mut Session) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

#[no_mangle]
pub unsafe extern "C" fn prediction_reconcile_room(p: *mut Session, n: usize) -> *mut f64 {
    let s = &mut *p;
    s.input.resize(n, 0.0);
    s.input.as_mut_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_reconcile_run(p: *mut Session, op: u32) -> u32 {
    let s = &mut *p;
    s.error = s.run(op).err();
    u32::from(s.error.is_none())
}

#[no_mangle]
pub unsafe extern "C" fn prediction_reconcile_output(p: *const Session) -> *const f64 {
    (&*p).output.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_reconcile_output_len(p: *const Session) -> usize {
    (&*p).output.len()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_reconcile_error(p: *const Session) -> *const u8 {
    (&*p).error.unwrap_or("").as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_reconcile_error_len(p: *const Session) -> usize {
    (&*p).error.unwrap_or("").len()
}
