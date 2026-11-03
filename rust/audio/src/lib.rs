//! The sound stage's worklet DSP (src/audio/worklets) in Rust: the spatial
//! mixer, the room reverb, the front stabilizer, the master dynamics and
//! true-peak limiter, the loudness meter and the enhanced mode's clarity
//! stage, for `?audio=wasm`. The JavaScript stays the default; with the
//! flag, each AudioWorkletProcessor hands its blocks to this module
//! instead (src/audio/wasm-dsp.js and the worklets' hand-off points).
//!
//! Every answer must be the JavaScript's own to the last bit, so the game
//! sounds exactly as it does without this module. Each processor here
//! repeats the worklet it stands for operation by operation, in the same
//! order: f64 where JavaScript holds a number or a Float64Array, f32
//! storage where it holds a Float32Array (a store rounds, a read widens,
//! exactly as JavaScript's do), and the same accumulation order in every
//! sum. Rust never contracts f64 operations into a fused multiply-add by
//! itself, and WebAssembly has none unless asked for. What is left is
//! sin, cos, tan, asin, atan2, exp, pow and log10, whose last bit is the
//! math library's: this module imports them from its host, so in the
//! browser they are V8's own, the ones the JavaScript calls. A native
//! host gives its own (here, the standard library's), whose last bit may
//! differ; nothing else about a native build changes. Math.min and
//! Math.max are `js_min` and `js_max` below, which keep JavaScript's
//! rules for NaN and signed zeros; `js_round` is Math.round.
//!
//! The processors themselves (mixer, reverb, stabilizer, limiter, meter,
//! clarity) know nothing of the web: each takes blocks of f32 samples and
//! fills blocks, which is all a native host needs. The WebAssembly calls
//! at the bottom are a thin skin over them: one handle per processor,
//! flat sample buffers and a few f64 slots exchanged through the module's
//! memory, as rust/rays does. Blocks are the worklet's 128 frames (the
//! render quantum); the core takes any frame count, the skin stages 128.

use std::ptr::addr_of_mut;

pub mod clarity;
pub mod limiter;
pub mod meter;
pub mod mixer;
pub mod reverb;
pub mod stabilizer;

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

/// The host's transcendentals. In WebAssembly they are imports the host
/// fills from its own Math (V8's in the browser and in Node, so the web
/// build's last bits are the JavaScript worklets' own); a native build
/// uses the standard library's.
pub mod host {
    // The host's own Math functions, imported under js_ names: plain
    // sin, cos and so on would link against the math library bundled
    // with the standard library instead of staying imports.
    #[cfg(target_arch = "wasm32")]
    mod ffi {
        extern "C" {
            pub fn js_sin(x: f64) -> f64;
            pub fn js_cos(x: f64) -> f64;
            pub fn js_tan(x: f64) -> f64;
            pub fn js_asin(x: f64) -> f64;
            pub fn js_atan2(y: f64, x: f64) -> f64;
            pub fn js_exp(x: f64) -> f64;
            pub fn js_pow(x: f64, y: f64) -> f64;
            pub fn js_log10(x: f64) -> f64;
        }
    }

    #[inline(always)]
    pub fn sin(x: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_sin(x) }
        #[cfg(not(target_arch = "wasm32"))]
        x.sin()
    }
    #[inline(always)]
    pub fn cos(x: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_cos(x) }
        #[cfg(not(target_arch = "wasm32"))]
        x.cos()
    }
    #[inline(always)]
    pub fn tan(x: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_tan(x) }
        #[cfg(not(target_arch = "wasm32"))]
        x.tan()
    }
    #[inline(always)]
    pub fn asin(x: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_asin(x) }
        #[cfg(not(target_arch = "wasm32"))]
        x.asin()
    }
    #[inline(always)]
    pub fn atan2(y: f64, x: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_atan2(y, x) }
        #[cfg(not(target_arch = "wasm32"))]
        y.atan2(x)
    }
    #[inline(always)]
    pub fn exp(x: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_exp(x) }
        #[cfg(not(target_arch = "wasm32"))]
        x.exp()
    }
    /// JavaScript's `x ** y`.
    #[inline(always)]
    pub fn pow(x: f64, y: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_pow(x, y) }
        #[cfg(not(target_arch = "wasm32"))]
        x.powf(y)
    }
    #[inline(always)]
    pub fn log10(x: f64) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe { ffi::js_log10(x) }
        #[cfg(not(target_arch = "wasm32"))]
        x.log10()
    }
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

#[derive(Default)]
struct State {
    mixers: Vec<Staged<mixer::Mixer>>,
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
    (s.mixers.len() - 1) as u32
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
    let st = &mut state().mixers[h as usize];
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
    s.meters.push(Staged::new(meter::Meter::new(rate, None), 16 * FRAMES, 0, 0));
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
    for r in &readings {
        st.cfg.extend([r.momentary, r.short_term, r.integrated, r.peak, r.true_peak, r.channels.len() as f64]);
        st.cfg.extend(r.channels.iter().copied());
    }
    readings.len() as u32
}

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
