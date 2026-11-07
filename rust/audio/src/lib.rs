//! Rust owns the complete audio render phase: source playback, streams,
//! placement, convolution, room sends, mono and master processing. The page
//! supplies decoded PCM and context-timed commands. One worklet copies the
//! finished 128-frame quantum; the optional meter host copies readings.
//!
//! Recorded synthetic renders define the arithmetic contract: f64 state,
//! f32 stores at the former node boundaries, chronological automation and
//! deterministic transcendentals from v8math. Component entrypoints keep
//! independent golden coverage and serve native hosts without Web Audio.

use std::ptr::addr_of_mut;

pub mod commands;
pub mod adpcm;
mod signal;
mod biquad;
pub mod clarity;
pub mod convolution;
pub mod hrtf;
pub mod pcm;
mod streaming;
pub mod stereo;
pub mod output;
pub mod frontend;
pub mod limiter;
pub mod master;
pub mod meter;
pub mod mixer;
pub mod reverb;
pub mod stabilizer;
pub mod timeline;
pub mod voices;

/// A block of samples handed to a processor: `channels` runs of `frames`
/// f32 samples, one after another (channel-major). Inputs are read widened
/// to f64, as JavaScript reads a Float32Array.
pub struct Block<'a> {
    pub data: &'a [f32],
    pub channels: usize,
    pub frames: usize,
}

impl<'a> Block<'a> {
    #[inline(always)]
    pub fn ch(&self, c: usize) -> &'a [f32] {
        &self.data[c * self.frames..(c + 1) * self.frames]
    }
}

/// A block a processor fills. A store takes an f64 and keeps it as f32,
/// rounding as a Float32Array store in JavaScript does; `at` reads one
/// back, widened.
pub struct BlockMut<'a> {
    pub data: &'a mut [f32],
    pub channels: usize,
    pub frames: usize,
}

impl<'a> BlockMut<'a> {
    #[inline(always)]
    pub fn at(&self, c: usize, i: usize) -> f64 {
        self.data[c * self.frames + i] as f64
    }
    #[inline(always)]
    pub fn set(&mut self, c: usize, i: usize, v: f64) {
        self.data[c * self.frames + i] = v as f32;
    }
    #[inline(always)]
    pub fn add(&mut self, c: usize, i: usize, v: f64) {
        let at = c * self.frames + i;
        self.data[at] = (self.data[at] as f64 + v) as f32;
    }
    pub fn fill(&mut self, v: f32) {
        self.data.fill(v);
    }
}

/// Math.min(a, b): NaN if either is, and -0 below +0.
#[inline(always)]
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { return f64::NAN; }
    if a < b { a } else if b < a { b } else if a == 0.0 && a.is_sign_negative() { a } else { b }
}

/// Math.max(a, b): NaN if either is, and +0 above -0.
#[inline(always)]
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { return f64::NAN; }
    if a > b { a } else if b > a { b } else if a == 0.0 && a.is_sign_positive() { a } else { b }
}

/// Math.round(x): the nearest integer, halves toward positive infinity,
/// the sign of a zero kept.
#[inline(always)]
pub fn js_round(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 || !x.is_finite() { return x; }
    let r = (x + 0.5).floor();
    if r == 0.0 && x.is_sign_negative() { -0.0 } else { r }
}

/// Deterministic transcendentals, shared with the native Rust host. Audio
/// keeps the fdlibm power law used by the recorded processor fixtures.
pub mod host {
    pub use v8math::{sin, cos, tan, asin, atan2, exp, log10};
    pub use v8math::pow_legacy as pow;
    pub fn log10_each(values: &mut [f64]) { for value in values { *value = log10(*value); } }
    pub fn pow10_each(values: &mut [f64]) { for value in values { *value = pow(10.0, *value); } }
}

// The WebAssembly skin: handles, staged buffers, and one process call per
// processor. Frames staged per call (the worklet's render quantum).
pub const FRAMES: usize = 128;
// f64 slots exchanged per handle (small parameters and results).
const IO_LEN: usize = 64;

struct Staged<T> {
    proc: T,
    input: Vec<f32>,
    output: Vec<f32>,
    io: [f64; IO_LEN],
    cfg: Vec<f64>,
    present: Vec<u8>,
}

impl<T> Staged<T> {
    fn new(proc: T, input: usize, output: usize, present: usize) -> Staged<T> {
        Staged { proc, input: vec![0.0; input], output: vec![0.0; output], io: [0.0; IO_LEN], cfg: Vec::new(), present: vec![0; present] }
    }
}

// A master chain and its staging: the inputs bound for it (the send's, a
// block; the mix's, a block a channel) and whether each came, its output,
// and f64 slots for its parameters.
struct MasterStaged {
    chain: master::Master,
    channels: usize,
    bus: Vec<f32>,
    flags: [u8; 2],
    out: Vec<f32>,
    io: [f64; 16],
}

#[derive(Default)]
struct State {
    outputs: Vec<output::Output>,
    frontends: Vec<frontend::Frontend>,
    adpcm: adpcm::Decoder,
    signal: Vec<f32>,
    asset_pcm: Vec<f32>,
    asset_i16: Vec<i16>,
    mixers: Vec<Staged<mixer::Mixer>>,
    // A mixer's voice engine, by the mixer's handle.
    engines: Vec<Option<voices::Engine>>,
    // The light mix's voice engines and their output (left, then right).
    lights: Vec<(voices::Engine, Vec<f32>)>,
    // A mixer's master chain, by the
    // mixer's handle.
    masters: Vec<Option<MasterStaged>>,
    reverbs: Vec<Staged<reverb::Reverb>>,
    stabilizers: Vec<Staged<stabilizer::Stabilizer>>,
    limiters: Vec<Staged<limiter::Limiter>>,
    meters: Vec<Staged<meter::Meter>>,
    clarities: Vec<Staged<clarity::Clarity>>,
}

static mut STATE: Option<State> = None;

fn state() -> &'static mut State {
    unsafe { (*addr_of_mut!(STATE)).get_or_insert_with(State::default) }
}

// The staged buffers' addresses. `cfg` is f64 scratch a setter reads at
// once (layouts, rows, weights); `io` is the handle's f64 slots.
macro_rules! buffers {
    ($list:ident, $inp:ident, $outp:ident, $iop:ident, $cfgp:ident) => {
        #[no_mangle]
        pub extern "C" fn $inp(h: u32) -> *mut f32 { state().$list[h as usize].input.as_mut_ptr() }
        #[no_mangle]
        pub extern "C" fn $outp(h: u32) -> *mut f32 { state().$list[h as usize].output.as_mut_ptr() }
        #[no_mangle]
        pub extern "C" fn $iop(h: u32) -> *mut f64 { state().$list[h as usize].io.as_mut_ptr() }
        /// Room for `len` f64s of configuration, to fill at the pointer
        /// before the setter that reads them.
        #[no_mangle]
        pub extern "C" fn $cfgp(h: u32, len: u32) -> *mut f64 {
            let st = &mut state().$list[h as usize];
            st.cfg.clear();
            st.cfg.resize(len as usize, 0.0);
            st.cfg.as_mut_ptr()
        }
    };
}

buffers!(mixers, mixer_in, mixer_out, mixer_io, mixer_cfg);
buffers!(reverbs, reverb_in, reverb_out, reverb_io, reverb_cfg);
buffers!(stabilizers, stabilizer_in, stabilizer_out, stabilizer_io, stabilizer_cfg);
buffers!(limiters, limiter_in, limiter_out, limiter_io, limiter_cfg);
buffers!(meters, meter_in, meter_out, meter_io, meter_cfg);
buffers!(clarities, clarity_in, clarity_out, clarity_io, clarity_cfg);

/// A spatial mixer over `slots` mono inputs and `channels` outputs
/// (panpot law, no layout yet). Its input staging is slots runs of 128
/// samples, its output channels+1 runs (the last the LFE send).
#[no_mangle]
pub extern "C" fn mixer_new(slots: u32, channels: u32) -> u32 {
    let s = state();
    s.mixers.push(Staged::new(
        mixer::Mixer::new(slots as usize, channels as usize, mixer::PANPOT, None, &[]),
        slots as usize * FRAMES,
        (channels as usize + 1) * FRAMES,
        slots as usize,
    ));
    s.engines.push(None);
    s.masters.push(None);
    (s.mixers.len() - 1) as u32
}

/// Gives the mixer a voice engine (src/voices.rs): voices it renders into
/// its slots itself, at the context's `rate`, with the lowpass's Q in dB;
/// x86=1 fills automation as Chromium's x86 build does.
#[no_mangle]
pub extern "C" fn mixer_engine(h: u32, rate: f64, q_db: f64, x86: u32) {
    let s = state();
    let slots = s.mixers[h as usize].present.len();
    s.engines[h as usize] = Some(voices::Engine::new(rate, slots, q_db, x86 != 0));
}

/// How many of the mixer's slots, from the first, the browser's inputs feed
/// (the worklet stages only those); the rest are the engine's alone.
#[no_mangle]
pub extern "C" fn mixer_inputs(h: u32, inputs: u32) {
    if let Some(e) = &mut state().engines[h as usize] {
        e.set_inputs(inputs as usize);
    }
}

/// Room for sample `id` (`frames` f32 at `rate`) in the mixer's engine, to
/// fill at the pointer.
#[no_mangle]
pub extern "C" fn mixer_sample(h: u32, id: u32, frames: u32, rate: f64) -> *mut f32 {
    match &mut state().engines[h as usize] {
        Some(e) => e.sample_storage(id as usize, frames as usize, rate).as_mut_ptr(),
        None => std::ptr::null_mut(),
    }
}

/// Room for `len` command numbers, to fill before mixer_commands.
#[no_mangle]
pub extern "C" fn mixer_commands_ptr(h: u32, len: u32) -> *mut f64 {
    match &mut state().engines[h as usize] {
        Some(e) => e.command_storage(len as usize).as_mut_ptr(),
        None => std::ptr::null_mut(),
    }
}

/// The voices' commands (voices.rs: START, GAIN, ...), applied at once.
#[no_mangle]
pub extern "C" fn mixer_commands(h: u32, len: u32) {
    if let Some(e) = &mut state().engines[h as usize] {
        e.apply_commands(len as usize);
    }
}

/// The context frame the next block starts at (the worklet's currentFrame),
/// before mixer_process.
#[no_mangle]
pub extern "C" fn mixer_frame(h: u32, frame: f64) {
    if let Some(e) = &mut state().engines[h as usize] {
        e.frame = frame;
    }
}

/// The engine's reverb send for the last block (128 f32).
#[no_mangle]
pub extern "C" fn mixer_send(h: u32) -> *mut f32 {
    match &mut state().engines[h as usize] {
        Some(e) => e.send.as_mut_ptr(),
        None => std::ptr::null_mut(),
    }
}

/// The mixer's per-slot present flags (1 where an input is connected).
#[no_mangle]
pub extern "C" fn mixer_present(h: u32) -> *mut u8 { state().mixers[h as usize].present.as_mut_ptr() }

/// Room for `len` f32s (slot parameters), to fill before mixer_params.
/// The mixer's input staging doubles as it: parameters arrive between
/// blocks, never during one.
#[no_mangle]
pub extern "C" fn mixer_params_ptr(h: u32) -> *mut f32 { state().mixers[h as usize].input.as_mut_ptr() }

/// The speakers from cfg, `count` of them as (azimuth, elevation,
/// present) triples; dirty=1 marks every slot's gains stale (a layout
/// message), 0 at construction.
#[no_mangle]
pub extern "C" fn mixer_speakers(h: u32, count: u32, dirty: u32) {
    let st = &mut state().mixers[h as usize];
    let speakers: Vec<mixer::Speaker> = (0..count as usize)
        .map(|i| {
            let (az, el, present) = (st.cfg[i * 3], st.cfg[i * 3 + 1], st.cfg[i * 3 + 2]);
            if present != 0.0 { Some((az, el)) } else { None }
        })
        .collect();
    st.proc.set_speakers(&speakers, dirty != 0);
}

/// The alsoft decoder from cfg: [row count, each row's length (-1 for
/// none), the rows' values], its front widening in io 0; present=0 clears it.
#[no_mangle]
pub extern "C" fn mixer_decoder(h: u32, present: u32) {
    let st = &mut state().mixers[h as usize];
    if present == 0 {
        st.proc.set_decoder(None);
        return;
    }
    let rows = st.cfg[0] as usize;
    let mut at = 1 + rows;
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        let len = st.cfg[1 + r];
        if len < 0.0 {
            out.push(None);
        } else {
            let len = len as usize;
            out.push(Some(st.cfg[at..at + len].to_vec()));
            at += len;
        }
    }
    let front = st.io[0];
    st.proc.set_decoder(Some(mixer::Decoder { front, rows: out }));
}

/// The panning law (mixer::PANPOT, VBAP, ALSOFT, AMBI3).
#[no_mangle]
pub extern "C" fn mixer_law(h: u32, law: u32) { state().mixers[h as usize].proc.set_law(law); }

/// Slot parameters (len f32s at mixer_params_ptr, [slot, x, y, z, spread,
/// lfe] per entry); fresh=1 at construction.
#[no_mangle]
pub extern "C" fn mixer_params(h: u32, len: u32, fresh: u32) {
    let st = &mut state().mixers[h as usize];
    // The parameters are in the input staging; take them out before the
    // borrow of the processor.
    let p = st.input[..len as usize].to_vec();
    st.proc.take(&p, fresh != 0);
}

/// A voice took this slot.
#[no_mangle]
pub extern "C" fn mixer_start(h: u32, slot: u32) { state().mixers[h as usize].proc.start(slot as usize); }

/// One block: the slots from the input staging and present flags into
/// out_channels of speaker feeds, and the LFE send after them when
/// has_lfe is 1.
#[no_mangle]
pub extern "C" fn mixer_process(h: u32, out_channels: u32, has_lfe: u32) {
    let s = state();
    let st = &mut s.mixers[h as usize];
    // The engine's voices into their slots first, as if they came in.
    if let Some(e) = &mut s.engines[h as usize] {
        e.render(&mut st.input, &mut st.present);
    }
    let oc = out_channels as usize;
    let (main, rest) = st.output.split_at_mut(oc * FRAMES);
    let mut out = BlockMut { data: main, channels: oc, frames: FRAMES };
    let input = std::mem::take(&mut st.input);
    let present = std::mem::take(&mut st.present);
    if has_lfe != 0 {
        st.proc.process(&input, &present, &mut out, Some(&mut rest[..FRAMES]));
    } else {
        st.proc.process(&input, &present, &mut out, None);
    }
    st.input = input;
    st.present = present;
}

/// Gives the mixer the enhanced mode's master chain (src/master.rs): its
/// `channels`, the limiter's ceiling (dB), release and lookahead (s), and
/// x86=1 as for the engine. The rest is set by the master_ calls below.
#[no_mangle]
pub extern "C" fn master_new(h: u32, rate: f64, channels: u32, ceiling: f64, release: f64, lookahead: f64, x86: u32) {
    let n = channels as usize;
    state().masters[h as usize] = Some(MasterStaged {
        chain: master::Master::new(rate, n, ceiling, release, lookahead, x86 != 0),
        channels: n,
        bus: vec![0.0; (1 + n) * FRAMES],
        flags: [0; 2],
        out: vec![0.0; n * FRAMES],
        io: [0.0; 16],
    });
}

fn master(h: u32) -> &'static mut MasterStaged {
    state().masters[h as usize].as_mut().expect("no master chain")
}

/// The master chain's f64 slots: the reverb's parameters (0-10, the mask
/// in 11) before master_room, the limiter's dynamics (0-6, the mask in 7)
/// before master_dynamics; the limiter's gain reduction in 0 after a
/// master_process that reports one.
#[no_mangle]
pub extern "C" fn master_io(h: u32) -> *mut f64 { master(h).io.as_mut_ptr() }

/// The inputs' staging: the send's block, then the mix's channels.
#[no_mangle]
pub extern "C" fn master_bus(h: u32) -> *mut f32 { master(h).bus.as_mut_ptr() }

/// Whether the send's input and the mix's came this block (1 each).
#[no_mangle]
pub extern "C" fn master_flags(h: u32) -> *mut u8 { master(h).flags.as_mut_ptr() }

/// The chain's output: a block a channel.
#[no_mangle]
pub extern "C" fn master_out(h: u32) -> *mut f32 { master(h).out.as_mut_ptr() }

/// The reverb's parameters from io (initial=1: in force at once).
#[no_mangle]
pub extern "C" fn master_room(h: u32, initial: u32) {
    let m = master(h);
    let mut p = [0.0; reverb::PARAM_COUNT];
    p.copy_from_slice(&m.io[..reverb::PARAM_COUNT]);
    let mask = m.io[reverb::PARAM_COUNT] as u32;
    m.chain.room(&p, mask, initial != 0);
}

/// The limiter's dynamics profile from io.
#[no_mangle]
pub extern "C" fn master_dynamics(h: u32) {
    let m = master(h);
    let mut d = [0.0; limiter::DYNAMICS_COUNT];
    d.copy_from_slice(&m.io[..limiter::DYNAMICS_COUNT]);
    let mask = m.io[limiter::DYNAMICS_COUNT] as u32;
    m.chain.dynamics(&d, mask);
}

#[no_mangle]
pub extern "C" fn master_highpass(h: u32, frequency: f32, q: f32) { master(h).chain.set_highpass(frequency, q); }

/// The channels the reverb reaches, a bit each.
#[no_mangle]
pub extern "C" fn master_rooms(h: u32, mask: u32) { master(h).chain.set_rooms(mask); }

#[no_mangle]
pub extern "C" fn master_lfe(h: u32, channel: u32, cutoff: f32, q: f32) { master(h).chain.set_lfe(channel as usize, cutoff, q); }

/// One more clarity EQ stage (kind: 0 lowpass, 1 highpass, 2 peaking, 3
/// highshelf).
#[no_mangle]
pub extern "C" fn master_eq(h: u32, kind: u32, frequency: f32, gain: f32, q: f32) { master(h).chain.add_eq(master::kind(kind), frequency, gain, q); }

/// The clarity stage (skip: the LFE's index, or -1) at its level.
#[no_mangle]
pub extern "C" fn master_clarity(h: u32, skip: i32, level: f32) { master(h).chain.set_clarity(skip as i64, level); }

/// The master volume, at the next block (the engine's frame): set=1 sets
/// its value at context time `at`, as AudioParam.value, else a
/// setTargetAtTime with time constant `tau`.
#[no_mangle]
pub extern "C" fn master_volume(h: u32, value: f32, tau: f64, at: f64, set: u32) {
    let s = state();
    let frame = s.engines[h as usize].as_ref().map_or(0.0, |e| e.frame);
    s.masters[h as usize].as_mut().expect("no master chain").chain.volume(value, tau, at, set != 0, frame);
}

/// The chain over the block mixer_process just rendered (its speaker feeds,
/// its LFE send when has_lfe is 1, its engine's reverb send), into
/// master_out; 1 when the limiter has a report (io 0), else 0.
#[no_mangle]
pub extern "C" fn master_process(h: u32, has_lfe: u32) -> u32 {
    let s = state();
    let mixer = &s.mixers[h as usize];
    let e = s.engines[h as usize].as_ref().expect("no voice engine");
    let m = s.masters[h as usize].as_mut().expect("no master chain");
    let n = m.channels;
    let feeds = &mixer.output[..n * FRAMES];
    let lfe = if has_lfe != 0 { Some(&mixer.output[n * FRAMES..(n + 1) * FRAMES]) } else { None };
    let (send_in, mix_in) = m.bus.split_at(FRAMES);
    let send_in = if m.flags[0] != 0 { Some(send_in) } else { None };
    let mix_in = if m.flags[1] != 0 { Some(mix_in) } else { None };
    match m.chain.process(feeds, lfe, &e.send, send_in, mix_in, e.frame as u64 - FRAMES as u64, &mut m.out) {
        Some(reduction) => {
            m.io[0] = reduction;
            1
        }
        None => 0,
    }
}

/// The light mix's voice engine: `slots` voices at the
/// context's `rate`, each through two speaker gains into a stereo output;
/// x86=1 fills automation as Chromium's x86 build does.
#[no_mangle]
pub extern "C" fn light_new(slots: u32, rate: f64, x86: u32) -> u32 {
    let s = state();
    // (No lowpass in the light mix: its Q is unused.)
    s.lights.push((voices::Engine::new(rate, slots as usize, 0.0, x86 != 0), vec![0.0; 2 * FRAMES]));
    (s.lights.len() - 1) as u32
}

/// Room for sample `id` (`frames` f32 at `rate`), to fill at the pointer.
#[no_mangle]
pub extern "C" fn light_sample(h: u32, id: u32, frames: u32, rate: f64) -> *mut f32 {
    state().lights[h as usize].0.sample_storage(id as usize, frames as usize, rate).as_mut_ptr()
}

/// Room for `len` command numbers, to fill before light_commands.
#[no_mangle]
pub extern "C" fn light_commands_ptr(h: u32, len: u32) -> *mut f64 {
    state().lights[h as usize].0.command_storage(len as usize).as_mut_ptr()
}

/// The voices' commands (voices.rs: START, SIDE_SET, ...), applied at once.
#[no_mangle]
pub extern "C" fn light_commands(h: u32, len: u32) {
    state().lights[h as usize].0.apply_commands(len as usize);
}

/// The context frame the next block starts at (the worklet's currentFrame).
#[no_mangle]
pub extern "C" fn light_frame(h: u32, frame: f64) {
    state().lights[h as usize].0.frame = frame;
}

/// One block into the output (light_out).
#[no_mangle]
pub extern "C" fn light_process(h: u32) {
    let (engine, out) = &mut state().lights[h as usize];
    engine.render_light(out);
}

/// The output: 128 f32 of the left speaker, then 128 of the right.
#[no_mangle]
pub extern "C" fn light_out(h: u32) -> *mut f32 {
    state().lights[h as usize].1.as_mut_ptr()
}

/// A room reverb at `rate` with `channels` of output signs. Its input
/// staging holds in_channels runs, its output out_channels (both given
/// per block; room for 16 of each).
#[no_mangle]
pub extern "C" fn reverb_new(rate: f64, channels: u32) -> u32 {
    let s = state();
    s.reverbs.push(Staged::new(
        reverb::Reverb::new(rate, channels as usize, &[0.0; reverb::PARAM_COUNT], 0),
        16 * FRAMES,
        16 * FRAMES,
        0,
    ));
    (s.reverbs.len() - 1) as u32
}

/// Parameters from io 0-10 with the mask in io 11; initial=1 also puts
/// them in force at once, as construction does.
#[no_mangle]
pub extern "C" fn reverb_params(h: u32, initial: u32) {
    let st = &mut state().reverbs[h as usize];
    let mut params = [0.0; reverb::PARAM_COUNT];
    params.copy_from_slice(&st.io[..reverb::PARAM_COUNT]);
    let mask = st.io[reverb::PARAM_COUNT] as u32;
    if initial != 0 {
        st.proc.place(&params, mask);
    } else {
        st.proc.set(&params, mask);
    }
}

/// One block of the reverb.
#[no_mangle]
pub extern "C" fn reverb_process(h: u32, in_channels: u32, out_channels: u32) {
    let st = &mut state().reverbs[h as usize];
    let input = Block { data: &st.input[..in_channels as usize * FRAMES], channels: in_channels as usize, frames: FRAMES };
    let mut out = BlockMut { data: &mut st.output[..out_channels as usize * FRAMES], channels: out_channels as usize, frames: FRAMES };
    st.proc.process(&input, &mut out);
}

/// The front stabilizer for a layout's channels.
#[no_mangle]
pub extern "C" fn stabilizer_new(rate: f64, channels: u32, left: u32, right: u32, centre: u32) -> u32 {
    let s = state();
    s.stabilizers.push(Staged::new(
        stabilizer::Stabilizer::new(rate, channels as usize, left as usize, right as usize, centre as usize),
        channels as usize * FRAMES,
        channels as usize * FRAMES,
        0,
    ));
    (s.stabilizers.len() - 1) as u32
}

/// One block of the stabilizer.
#[no_mangle]
pub extern "C" fn stabilizer_process(h: u32, in_channels: u32, out_channels: u32) {
    let st = &mut state().stabilizers[h as usize];
    let input = Block { data: &st.input[..in_channels as usize * FRAMES], channels: in_channels as usize, frames: FRAMES };
    let mut out = BlockMut { data: &mut st.output[..out_channels as usize * FRAMES], channels: out_channels as usize, frames: FRAMES };
    st.proc.process(&input, &mut out);
}

/// The master dynamics and limiter (ceiling in dBTP, release and
/// lookahead in seconds). Room for 16 channels of staging.
#[no_mangle]
pub extern "C" fn limiter_new(rate: f64, ceiling: f64, release: f64, lookahead: f64) -> u32 {
    let s = state();
    s.limiters.push(Staged::new(
        limiter::Limiter::new(rate, ceiling, release, lookahead, &[0.0; limiter::DYNAMICS_COUNT], 0),
        16 * FRAMES,
        16 * FRAMES,
        0,
    ));
    (s.limiters.len() - 1) as u32
}

/// The dynamics profile from io 0-6 with the mask in io 7.
#[no_mangle]
pub extern "C" fn limiter_dynamics(h: u32) {
    let st = &mut state().limiters[h as usize];
    let mut d = [0.0; limiter::DYNAMICS_COUNT];
    d.copy_from_slice(&st.io[..limiter::DYNAMICS_COUNT]);
    let mask = st.io[limiter::DYNAMICS_COUNT] as u32;
    st.proc.set_dynamics(&d, mask);
}

#[no_mangle]
pub extern "C" fn limiter_ceiling(h: u32, db: f64) { state().limiters[h as usize].proc.set_ceiling(db); }

#[no_mangle]
pub extern "C" fn limiter_release(h: u32, seconds: f64) { state().limiters[h as usize].proc.set_release(seconds); }

/// One block of the limiter: 1 and the gain reduction (dB) in io 0 when a
/// report is due, else 0.
#[no_mangle]
pub extern "C" fn limiter_process(h: u32, in_channels: u32, out_channels: u32) -> u32 {
    let st = &mut state().limiters[h as usize];
    let input = Block { data: &st.input[..in_channels as usize * FRAMES], channels: in_channels as usize, frames: FRAMES };
    let mut out = BlockMut { data: &mut st.output[..out_channels as usize * FRAMES], channels: out_channels as usize, frames: FRAMES };
    match st.proc.process(&input, &mut out) {
        Some(reduction) => {
            st.io[0] = reduction;
            1
        }
        None => 0,
    }
}

/// The loudness meter (its weights follow by meter_weights when it has
/// any). Room for 16 channels of staging.
#[no_mangle]
pub extern "C" fn meter_new(rate: f64) -> u32 {
    let s = state();
    let mut staged = Staged::new(meter::Meter::new(rate, None), 16 * FRAMES, 0, 0);
    staged.cfg.reserve(22);
    s.meters.push(staged);
    (s.meters.len() - 1) as u32
}

/// This meter's weights from its cfg (count of them).
#[no_mangle]
pub extern "C" fn meter_weights(h: u32, count: u32) {
    let st = &mut state().meters[h as usize];
    let w = st.cfg[..count as usize].to_vec();
    st.proc.set_weights(w);
}

#[no_mangle]
pub extern "C" fn meter_reset(h: u32) { state().meters[h as usize].proc.reset(); }

/// One block of the meter: how many readings it completed, laid out in
/// cfg per reading as [momentary, short-term, integrated, peak,
/// true-peak, channel count, channel peaks...].
#[no_mangle]
pub extern "C" fn meter_process(h: u32, in_channels: u32) -> u32 {
    let st = &mut state().meters[h as usize];
    let input = Block { data: &st.input[..in_channels as usize * FRAMES], channels: in_channels as usize, frames: FRAMES };
    let readings = st.proc.process(&input);
    st.cfg.clear();
    if let Some(r) = &readings {
        st.cfg.extend([r.momentary, r.short_term, r.integrated, r.peak, r.true_peak, r.count as f64]);
        st.cfg.extend(r.channels[..r.count].iter().copied());
    }
    readings.is_some() as u32
}

#[no_mangle]
pub extern "C" fn meter_reserve(h: u32) { state().meters[h as usize].proc.reserve(); }
#[no_mangle]
pub extern "C" fn meter_remaining(h: u32) -> u32 { state().meters[h as usize].proc.remaining() as u32 }

/// The cfg laid out by the last meter_process, and its length in io 0.
#[no_mangle]
pub extern "C" fn meter_readings(h: u32) -> *mut f64 {
    let st = &mut state().meters[h as usize];
    st.io[0] = st.cfg.len() as f64;
    st.cfg.as_mut_ptr()
}

/// The clarity stage for `channels`, `skip` the LFE's index or -1.
#[no_mangle]
pub extern "C" fn clarity_new(rate: f64, channels: u32, skip: i32) -> u32 {
    let s = state();
    s.clarities.push(Staged::new(
        clarity::Clarity::new(rate, channels as usize, skip as i64),
        channels as usize * FRAMES,
        channels as usize * FRAMES,
        0,
    ));
    (s.clarities.len() - 1) as u32
}

/// One block of the clarity stage.
#[no_mangle]
pub extern "C" fn clarity_process(h: u32, in_channels: u32, out_channels: u32) {
    let st = &mut state().clarities[h as usize];
    let input = Block { data: &st.input[..in_channels as usize * FRAMES], channels: in_channels as usize, frames: FRAMES };
    let mut out = BlockMut { data: &mut st.output[..out_channels as usize * FRAMES], channels: out_channels as usize, frames: FRAMES };
    st.proc.process(&input, &mut out);
}

/// Nothing: what a call into the module costs by itself.
#[no_mangle]
pub extern "C" fn nothing() -> u32 { 0 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_max_round_keep_javascript_rules() {
        assert!(js_min(f64::NAN, 1.0).is_nan() && js_max(1.0, f64::NAN).is_nan());
        assert!(js_min(0.0, -0.0).is_sign_negative() && js_max(-0.0, 0.0).is_sign_positive());
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert!(js_round(-0.4).is_sign_negative());
        assert_eq!(js_round(4800.0), 4800.0);
    }

    fn block(data: &[f32], channels: usize, frames: usize) -> Block<'_> {
        Block { data, channels, frames }
    }

    #[test]
    fn panpot_centre_is_equal_power() {
        let mut m = mixer::Mixer::new(1, 2, mixer::PANPOT, None, &[Some((30.0, 0.0)), Some((-30.0, 0.0))]);
        m.take(&[0.0, 0.0, 0.0, -1.0, 0.0, 0.0], true);
        let input = vec![0.5f32; 128];
        let mut out_data = vec![0.0f32; 256];
        let mut out = BlockMut { data: &mut out_data, channels: 2, frames: 128 };
        m.process(&input, &[1], &mut out, None);
        let want = (0.5f64 * std::f64::consts::FRAC_1_SQRT_2) as f32;
        assert_eq!(out_data[0], want);
        assert_eq!(out_data[128], want);
    }

    #[test]
    fn limiter_keeps_a_hot_signal_at_the_ceiling() {
        let mut l = limiter::Limiter::new(48000.0, -1.0, 0.08, 0.0015, &[0.0; limiter::DYNAMICS_COUNT], 0);
        let data = vec![0.99f32; 2 * 128];
        let mut out_data = vec![0.0f32; 2 * 128];
        for _ in 0..200 {
            let input = block(&data, 2, 128);
            let mut out = BlockMut { data: &mut out_data, channels: 2, frames: 128 };
            l.process(&input, &mut out);
        }
        let ceiling = host::pow(10.0, -1.0 / 20.0) as f32;
        assert!(out_data.iter().all(|&v| v <= ceiling + 1e-6), "over the ceiling: {:?}", out_data.iter().fold(0.0f32, |a, &b| a.max(b)));
        assert!(out_data.iter().all(|&v| v > 0.8), "the gain should settle near the ceiling, not silence");
    }

    #[test]
    fn reverb_answers_an_impulse_then_goes_quiet() {
        let mut params = [0.0; reverb::PARAM_COUNT];
        params[0] = 0.2;
        params[2] = 0.3;
        let mut r = reverb::Reverb::new(48000.0, 2, &params, 0b101);
        let mut data = vec![0.0f32; 128];
        data[0] = 1.0;
        let mut heard = false;
        let mut tail = 0.0f32;
        for b in 0..4000 {
            let input = block(&data, 1, 128);
            let mut out_data = vec![0.0f32; 2 * 128];
            {
                let mut out = BlockMut { data: &mut out_data, channels: 2, frames: 128 };
                r.process(&input, &mut out);
            }
            let peak = out_data.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
            if b > 0 && peak > 0.0 {
                heard = true;
            }
            if b == 3999 {
                tail = peak;
            }
            data[0] = 0.0;
        }
        assert!(heard, "no tail after the impulse");
        assert_eq!(tail, 0.0, "the network must reach exact silence (its quiet shortcut)");
    }
}

/// Classic has no per-voice lowpass; select it for isolated voice fixtures.
#[no_mangle]
pub extern "C" fn mixer_filtered(h: u32, filtered: u32) {
    if let Some(e) = &mut state().engines[h as usize] { e.filtered = filtered != 0; }
}

/// Render isolated voice slots before the Rust placement and HRTF phase.
#[no_mangle]
pub extern "C" fn mixer_voices(h: u32) {
    let s = state();
    let st = &mut s.mixers[h as usize];
    st.input.fill(0.0);
    if let Some(e) = &mut s.engines[h as usize] { e.render(&mut st.input, &mut st.present); }
}

/// Classic surround's front stabilizer and output level, ahead of dynamics.
#[no_mangle]
pub extern "C" fn master_original(h: u32, centre: u32, level: f32) {
    master(h).chain.set_original(centre as usize, level);
}

/// The complete render phase. The host keeps no voice, bed or master DSP.
#[no_mangle]
pub extern "C" fn output_new(rate:f64,channels:u32,enhanced:u32,headphones:u32,binaural:u32,mono:u32,x86:u32)->u32 {
    let s=state();s.outputs.push(output::Output::new(rate,channels as usize,enhanced!=0,headphones!=0,binaural!=0,mono!=0,x86!=0));(s.outputs.len()-1) as u32
}
fn output(h:u32)->&'static mut output::Output{&mut state().outputs[h as usize]}
#[no_mangle]
pub extern "C" fn output_io(h:u32)->*mut f64{output(h).io.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn output_cfg(h:u32,len:u32)->*mut f64{let o=output(h);o.cfg.resize(len as usize,0.0);o.cfg.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn output_out(h:u32)->*mut f32{output(h).output.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn output_sample(h:u32,id:u32,frames:u32,channels:u32,rate:f64)->*mut f32{output(h).voices.sample_channels(id as usize,frames as usize,channels as usize,rate).as_mut_ptr()}
#[no_mangle]
pub extern "C" fn output_commands_ptr(h:u32,len:u32)->*mut f64{let o=output(h);o.commands.resize(len as usize,0.0);o.commands.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn output_commands(h:u32,len:u32){output(h).apply(len as usize)}
#[no_mangle]
pub extern "C" fn output_params(h:u32,len:u32){let o=output(h);let n=len as usize;let mut data=std::mem::take(&mut o.cfg);for i in 0..n {data[i]=data[i] as f32 as f64;}let mut at=0;while at+6<=n{let p=[data[at] as f32,data[at+1] as f32,data[at+2] as f32,data[at+3] as f32,data[at+4] as f32,data[at+5] as f32];o.mixer.take(&p,false);at+=6;}o.cfg=data;}
/// cfg: speaker triples (azimuth, elevation, exists), then decoder row
/// counts and rows, as the original mixer's layout interface uses.
#[no_mangle]
pub extern "C" fn output_layout(h:u32,law:u32,speakers:u32,decoder:u32,front:f64,center:i32,lfe:i32,original:f32){
    let o=output(h);let n=speakers as usize;let cfg=&o.cfg;
    let list:Vec<mixer::Speaker>=(0..n).map(|i|if cfg[i*3+2]!=0.0{Some((cfg[i*3],cfg[i*3+1]))}else{None}).collect();
    let decode=if decoder!=0{let count=cfg[n*3] as usize;let mut at=n*3+1+count;let mut rows=Vec::with_capacity(count);for i in 0..count{let len=cfg[n*3+1+i] as i32;if len<0{rows.push(None);}else{rows.push(Some(cfg[at..at+len as usize].to_vec()));at+=len as usize;}}Some(mixer::Decoder{front,rows})}else{None};
    o.layout(law,decode,&list,center,lfe,original);
}
#[no_mangle]
pub extern "C" fn output_eq(h:u32,kind:u32,frequency:f32,gain:f32,q:f32){if let Some(m)=&mut output(h).master{m.add_eq(master::kind(kind),frequency,gain,q);}}
#[no_mangle]
pub extern "C" fn output_clarity(h:u32,skip:i32,level:f32){if let Some(m)=&mut output(h).master{m.set_clarity(skip as i64,level);}}
#[no_mangle]
pub extern "C" fn output_room(h:u32,initial:u32){let o=output(h);let mut values=[0.0;reverb::PARAM_COUNT];values.copy_from_slice(&o.io[..reverb::PARAM_COUNT]);if let Some(m)=&mut o.master{m.room(&values,o.io[reverb::PARAM_COUNT] as u32,initial!=0);}}
#[no_mangle]
pub extern "C" fn output_dynamics(h:u32){let o=output(h);let mut values=[0.0;limiter::DYNAMICS_COUNT];values.copy_from_slice(&o.io[..limiter::DYNAMICS_COUNT]);if let Some(m)=&mut o.master{m.dynamics(&values,o.io[limiter::DYNAMICS_COUNT] as u32);}}
#[no_mangle]
pub extern "C" fn output_volume(h:u32,value:f32,tau:f64,at:f64,set:u32){output(h).volume(value,tau,at,set!=0);}
#[no_mangle]
pub extern "C" fn output_light_level(h:u32,db:f64){output(h).light_level(db);}
#[no_mangle]
pub extern "C" fn output_decoder(h:u32,frames:u32)->*mut f32{let o=output(h);o.decoder_staging.resize(frames as usize*32,0.0);o.decoder_staging.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn output_decoder_ready(h:u32,frames:u32){let o=output(h);let values=std::mem::take(&mut o.decoder_staging);o.decoder(&values,frames as usize);o.decoder_staging=values;}
#[no_mangle]
pub extern "C" fn output_process(h:u32,frame:f64)->u32{let o=output(h);o.frame=frame as u64;match o.process(){Some(v)=>{o.io[0]=v;1},None=>0}}

/// Rebuild output-only DSP while retaining decoded PCM, source positions
/// and automation. The host's one output node remains attached to the device.
#[no_mangle]
pub extern "C" fn output_rebuild(h:u32,channels:u32,enhanced:u32,headphones:u32,binaural:u32,mono:u32) {
    let old=output(h);
    let mut next=output::Output::new(old.rate,channels as usize,enhanced!=0,headphones!=0,binaural!=0,mono!=0,old.x86);
    std::mem::swap(&mut old.voices,&mut next.voices);
    std::mem::swap(&mut old.streams,&mut next.streams);
    std::mem::swap(&mut old.placements,&mut next.placements);
    next.voices.filtered=enhanced!=0;next.frame=old.frame;
    next.reattach();
    *old=next;
}

/// Asset decoding happens off the audio thread, using separate module
/// instances in the loader worker. Compressed assets never use a host codec.
#[no_mangle]
pub extern "C" fn adpcm_input(length: u32) -> *mut u8 { let d=&mut state().adpcm; d.bytes.resize(length as usize,0); d.bytes.as_mut_ptr() }
#[no_mangle]
pub extern "C" fn adpcm_decode(channels: u32, count: u32, interleave: u32) -> u32 { state().adpcm.decode(channels as usize,count as usize,interleave as usize) as u32 }
#[no_mangle]
pub extern "C" fn adpcm_output(channel: u32) -> *mut f32 { state().adpcm.output[channel as usize].as_mut_ptr() }

#[no_mangle]
pub extern "C" fn signal_pcm(rate:f64, frames:u32, tone:u32) -> *mut f32 { let s=state();s.signal=signal::make(rate,frames as usize,tone!=0);s.signal.as_mut_ptr() }

#[no_mangle]
pub extern "C" fn output_free_sample(h:u32,id:u32) { if let Some(sample)=output(h).voices.samples.get_mut(id as usize){*sample=None;} }

/// Whole decoded assets are prepared between page tasks, never in process.
#[no_mangle]
pub extern "C" fn asset_i16(frames:u32)->*mut i16 {let s=state();s.asset_i16.resize(frames as usize,0);s.asset_pcm.resize(frames as usize,0.0);s.asset_i16.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn asset_normalize()->*mut f32 {let s=state();for i in 0..s.asset_i16.len(){s.asset_pcm[i]=(s.asset_i16[i] as f64/32768.0) as f32;}s.asset_pcm.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn asset_pcm(frames:u32)->*mut f32 {let s=state();s.asset_pcm.resize(frames as usize,0.0);s.asset_pcm.as_mut_ptr()}
// Front-end output: PCM and commands are staged between render quanta.
#[no_mangle]
pub extern "C" fn front_new(rate: f64) -> u32 { let s=state(); s.frontends.push(frontend::Frontend::new(rate)); (s.frontends.len()-1) as u32 }
#[no_mangle]
pub extern "C" fn front_out(h:u32)->*mut f32 {state().frontends[h as usize].output.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn front_ended_len()->u32 {(commands::FRONT_END_STRIDE*frontend::SLOTS) as u32}
#[no_mangle]
pub extern "C" fn front_ended(h:u32)->*mut u32 {state().frontends[h as usize].ended.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn front_sample(h:u32,slot:u32,frames:u32,channels:u32,rate:f64)->*mut f32 {state().frontends[h as usize].sample(slot as usize,frames as usize,channels as usize,rate)}
#[no_mangle]
pub extern "C" fn front_release(h:u32,slot:u32) {state().frontends[h as usize].samples[slot as usize]=None;}
#[no_mangle]
pub extern "C" fn front_commands_ptr(h:u32,len:u32)->*mut f64 {let f=&mut state().frontends[h as usize];f.commands.resize(len as usize,0.0);f.commands.as_mut_ptr()}
#[no_mangle]
pub extern "C" fn front_commands(h:u32) {state().frontends[h as usize].apply();}
#[no_mangle]
pub extern "C" fn front_process(h:u32,frame:f64)->u32 {state().frontends[h as usize].process(frame as u64) as u32}

// Streaming sample storage is sized once. Incoming chunks use the shared
// loading scratch between quanta; render calls never grow the ring.
#[no_mangle]
pub extern "C" fn output_stream_new(h:u32,id:u32,frames:u32,channels:u32,rate:f64,capacity:u32) {
    let samples=&mut output(h).voices.samples;
    if samples.len()<=id as usize {samples.resize_with(id as usize+1,||None);}
    samples[id as usize]=Some(voices::Sample::streaming(frames as usize,channels as usize,rate,capacity as usize));
}
#[no_mangle]
pub extern "C" fn front_stream_new(h:u32,id:u32,frames:u32,channels:u32,rate:f64,capacity:u32) {
    state().frontends[h as usize].samples[id as usize]=Some(voices::Sample::streaming(frames as usize,channels as usize,rate,capacity as usize));
}
#[no_mangle]
pub extern "C" fn output_stream_push(h:u32,id:u32,frames:u32,eof:u32)->u32 {
    let s=state();let pcm=&s.asset_pcm;
    s.outputs[h as usize].voices.samples[id as usize].as_mut().unwrap().append(pcm,frames as usize,eof!=0) as u32
}
#[no_mangle]
pub extern "C" fn front_stream_push(h:u32,id:u32,frames:u32,eof:u32)->u32 {
    let s=state();let pcm=&s.asset_pcm;
    s.frontends[h as usize].samples[id as usize].as_mut().unwrap().append(pcm,frames as usize,eof!=0) as u32
}
#[no_mangle]
pub extern "C" fn output_stream_free(h:u32,id:u32)->u32 {output(h).voices.samples.get(id as usize).and_then(|s|s.as_ref()).map_or(0,|s|s.writable()) as u32}
#[no_mangle]
pub extern "C" fn front_stream_free(h:u32,id:u32)->u32 {state().frontends[h as usize].samples.get(id as usize).and_then(|s|s.as_ref()).map_or(0,|s|s.writable()) as u32}
#[no_mangle]
pub extern "C" fn output_stream_error(h:u32,id:u32)->u32 {output(h).voices.samples.get(id as usize).and_then(|s|s.as_ref()).and_then(|s|s.ring.as_ref()).is_some_and(|r|r.underrun.get()) as u32}
#[no_mangle]
pub extern "C" fn front_stream_error(h:u32,id:u32)->u32 {state().frontends[h as usize].samples.get(id as usize).and_then(|s|s.as_ref()).and_then(|s|s.ring.as_ref()).is_some_and(|r|r.underrun.get()) as u32}
