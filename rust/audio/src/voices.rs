//! The enhanced mode's voices rendered in one place, for `?voices=rust`:
//! what each voice's chain of browser nodes does (src/audio/engine.js and
//! VoiceOut in src/audio/spatial.js), worked out here and written into the
//! spatial mixer's slot instead of reaching it through the nodes. Per voice,
//! as Chromium's nodes compute it:
//!
//! - the sample, as an AudioBufferSourceNode plays it: from its start frame
//!   (the start time rounded up to a frame) to its stop frame, a read
//!   position in double precision stepped by the sample's rate over the
//!   context's times the playbackRate (k-rate), linear interpolation
//!   between the two samples around it, looping or ending at the sample's
//!   end;
//! - its gain (a GainNode moved by setValueAtTime, setTargetAtTime and
//!   cancelScheduledValues), a value a sample;
//! - its reverb send, the gain's output times the send level, summed over
//!   the voices into the mixer's third output in the slots' order (the
//!   browser adds three or more sends in an order of its own that changes
//!   from graph to graph, so there its renders and these can differ in the
//!   last bits, as its own renders differ from each other);
//! - the air's lowpass (a BiquadFilterNode with a k-rate frequency), its
//!   coefficients from fdlibm's sine and cosine as the browser's, into the
//!   voice's slot.
//!
//! Every parameter runs on src/timeline.rs, the browser's own automation
//! arithmetic. Commands (start, the targets, release, stop) arrive once a
//! frame from the page, each with the context time its AudioParam call
//! would have had. Nothing allocates while rendering: samples and the
//! command buffer are allocated as they arrive.

use crate::timeline::Timeline;
use crate::FRAMES;

/// The command codes the page sends, and how many numbers each takes after
/// its code (slot first). `at` is the context time the page set it at (its
/// AudioParam call's).
pub const START: u32 = 1; // slot, sample, offset (s), loop, rate, cutoff (Hz), at
pub const GAIN_SET: u32 = 2; // slot, value, at
pub const GAIN: u32 = 3; // slot, target, time constant (s), at
pub const RATE: u32 = 4; // slot, target, time constant, at
pub const CUTOFF: u32 = 5; // slot, target (Hz), time constant, at
pub const SEND: u32 = 6; // slot, target, time constant, at
pub const RELEASE: u32 = 7; // slot, time constant, stop after (s), at
pub const STOP: u32 = 8; // slot

fn arity(op: u32) -> usize {
    match op {
        START => 7,
        GAIN_SET => 3,
        GAIN | RATE | CUTOFF | SEND | RELEASE => 4,
        STOP => 1,
        _ => 0,
    }
}

/// A sample the voices play: mono (a voice's chain reaches the mixer as its
/// first channel), at its own rate.
pub struct Sample {
    pub rate: f64,
    pub data: Vec<f32>,
}

// The browser's TimeToSampleFrame: the time in frames at 1024 times the
// rate, rounded, then rounded up (or to the nearest).
fn time_to_frame(time: f64, rate: f64, up: bool) -> f64 {
    let frame = (time * rate * 1024.0).round() / 1024.0;
    if up { frame.ceil() } else { frame.round() }
}

#[derive(Clone, Copy)]
struct Voice {
    on: bool,
    sample: usize,
    looping: bool,
    // Read position in the sample's frames; the source's start and stop
    // frames in the context's; whether it reached its sample's end.
    pos: f64,
    start: f64,
    stop: f64,
    ended: bool,
    rate: Timeline,
    gain: Timeline,
    send: Timeline,
    cutoff: Timeline,
    // The lowpass: its coefficients (for the cutoff `made_for`) and state.
    made_for: f32,
    b: [f64; 5],
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Default for Voice {
    fn default() -> Voice {
        Voice {
            on: false,
            sample: 0,
            looping: false,
            pos: 0.0,
            start: 0.0,
            stop: f64::INFINITY,
            ended: false,
            rate: Timeline::new(1.0),
            gain: Timeline::new(0.0),
            send: Timeline::new(0.0),
            cutoff: Timeline::new(0.0),
            made_for: f32::NAN,
            b: [0.0; 5],
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

pub struct Engine {
    rate: f64,
    nyquist: f64,
    // The lowpass's Q (in dB, as the browser takes a lowpass's) as its
    // linear resonance.
    resonance: f64,
    // Chromium's x86 build fills a setTargetAtTime's values four at a time.
    x86: bool,
    voices: Vec<Voice>,
    samples: Vec<Option<Sample>>,
    commands: Vec<f64>,
    /// The block's reverb send, summed over the voices (the mixer's third
    /// output).
    pub send: Vec<f32>,
    /// The context frame the next block starts at (the worklet's
    /// currentFrame).
    pub frame: f64,
    // A voice's gain and send values for the block.
    gains: [f32; FRAMES],
    sends: [f32; FRAMES],
}

// The browser's lowpass coefficients for a normalized cutoff (0..1 of the
// Nyquist frequency), normalized through a0's inverse as it does.
fn lowpass(cutoff: f64, resonance: f64) -> [f64; 5] {
    let cutoff = cutoff.clamp(0.0, 1.0);
    if cutoff == 1.0 {
        return [1.0, 0.0, 0.0, 0.0, 0.0];
    }
    if cutoff <= 0.0 {
        return [0.0; 5];
    }
    let theta = std::f64::consts::PI * cutoff;
    let alpha = v8math::sin(theta) / (2.0 * resonance);
    let cosw = v8math::cos(theta);
    let beta = (1.0 - cosw) / 2.0;
    let a0 = 1.0 / (1.0 + alpha);
    [beta * a0, 2.0 * beta * a0, beta * a0, -2.0 * cosw * a0, (1.0 - alpha) * a0]
}

// Below the smallest normal f32 the browser's biquad flushes its state to
// zero at each block's end.
#[inline(always)]
fn flush(x: f64) -> f64 {
    if x.abs() < f32::MIN_POSITIVE as f64 { 0.0 } else { x }
}

impl Engine {
    /// An engine for `slots` voices at the context's `rate`; `q_db` is the
    /// lowpass's Q in dB. Its resonance is 10^(Q / 20) as the browser works
    /// it out: fdlibm's expf, which is fdlibm's exp of the argument rounded
    /// to single precision, its result rounded to single.
    pub fn new(rate: f64, slots: usize, q_db: f64, x86: bool) -> Engine {
        let x = ((q_db / 20.0) * std::f64::consts::LN_10) as f32;
        Engine {
            rate,
            nyquist: rate / 2.0,
            resonance: v8math::exp(x as f64) as f32 as f64,
            x86,
            voices: vec![Voice::default(); slots],
            samples: Vec::new(),
            commands: Vec::new(),
            send: vec![0.0; FRAMES],
            frame: 0.0,
            gains: [0.0; FRAMES],
            sends: [0.0; FRAMES],
        }
    }

    /// Room for sample `id`'s `frames` at `rate`, to fill before a voice
    /// plays it (allocates: called as a sample arrives, never mid-block).
    pub fn sample_storage(&mut self, id: usize, frames: usize, rate: f64) -> &mut [f32] {
        if self.samples.len() <= id {
            self.samples.resize_with(id + 1, || None);
        }
        self.samples[id] = Some(Sample { rate, data: vec![0.0; frames] });
        &mut self.samples[id].as_mut().unwrap().data
    }

    /// Room for `len` command numbers (allocates; between blocks).
    pub fn command_storage(&mut self, len: usize) -> &mut [f64] {
        self.commands.clear();
        self.commands.resize(len, 0.0);
        &mut self.commands
    }

    /// Applies the commands in command_storage's first `len` numbers.
    pub fn apply_commands(&mut self, len: usize) {
        let commands = std::mem::take(&mut self.commands);
        let (rate, frame) = (self.rate, self.frame);
        let mut at = 0;
        while at < len.min(commands.len()) {
            let op = commands[at] as u32;
            let n = arity(op);
            if n == 0 || at + 1 + n > commands.len() {
                break;
            }
            let a = &commands[at + 1..at + 1 + n];
            let slot = a[0] as usize;
            at += 1 + n;
            if slot >= self.voices.len() {
                continue;
            }
            let v = &mut self.voices[slot];
            match op {
                START => {
                    let Some(Some(s)) = self.samples.get(a[1] as usize) else {
                        v.on = false;
                        continue;
                    };
                    // The offset within the sample, clipped to it; a whole
                    // frame when the source starts at rate 1, as the
                    // browser's (its interpolation then reads the samples
                    // as they are).
                    let offset = a[2].clamp(0.0, s.data.len() as f64 / s.rate);
                    let pos = if a[4] == 1.0 { time_to_frame(offset, s.rate, false) } else { offset * s.rate };
                    *v = Voice {
                        on: true,
                        sample: a[1] as usize,
                        looping: a[3] != 0.0,
                        pos,
                        start: time_to_frame(a[6], rate, true),
                        rate: Timeline::new(a[4] as f32),
                        cutoff: Timeline::new(a[5].min(self.nyquist) as f32),
                        ..Voice::default()
                    };
                }
                GAIN_SET => v.gain.insert(false, a[1] as f32, a[2], 0.0, frame, rate),
                GAIN => v.gain.insert(true, a[1] as f32, a[3], a[2], frame, rate),
                RATE => v.rate.insert(true, a[1] as f32, a[3], a[2], frame, rate),
                CUTOFF => v.cutoff.insert(true, a[1].min(self.nyquist) as f32, a[3], a[2], frame, rate),
                SEND => v.send.insert(true, a[1] as f32, a[3], a[2], frame, rate),
                RELEASE => {
                    // cancelScheduledValues(at), setTargetAtTime(0, at, tau),
                    // stop(at + after).
                    v.gain.cancel(a[3]);
                    v.gain.insert(true, 0.0, a[3], a[1], frame, rate);
                    v.stop = time_to_frame(a[3] + a[2], rate, true);
                }
                STOP => v.on = false,
                _ => {}
            }
        }
        self.commands = commands;
    }

    /// One block: each voice into its slot's run of `inputs` (and marked
    /// present), and the send summed into `self.send`. A voice past its
    /// stop or its sample's end plays silence into its lowpass until the
    /// page stops it (the browser's filter rings on until its nodes are
    /// disconnected).
    pub fn render(&mut self, inputs: &mut [f32], present: &mut [u8]) {
        self.send.fill(0.0);
        let (rate, x86, nyquist, resonance) = (self.rate, self.x86, self.nyquist, self.resonance);
        let frame = self.frame;
        self.frame += FRAMES as f64;
        let block = frame as u64;
        let send = &mut self.send[..FRAMES];
        let (gains, sends) = (&mut self.gains, &mut self.sends);
        for (slot, v) in self.voices.iter_mut().enumerate() {
            if !v.on {
                continue;
            }
            let Some(Some(sample)) = self.samples.get(v.sample) else {
                v.on = false;
                continue;
            };
            let data = &sample.data[..];
            let len = data.len();
            // The source's rate (k-rate), the lowpass's cutoff (k-rate).
            let step = (sample.rate / rate * v.rate.k_rate(block, rate) as f64).clamp(0.0, 1024.0);
            let cutoff = v.cutoff.k_rate(block, rate);
            if cutoff != v.made_for {
                v.b = lowpass(cutoff as f64 / nyquist, resonance);
                v.made_for = cutoff;
            }
            // The gain and the send: a value a sample under automation; a
            // constant one otherwise, where a 0 is silence outright (the
            // browser zeroes the output rather than multiplying).
            let gain_zero = !v.gain.has_events() && v.gain.value() == 0.0;
            if v.gain.has_events() {
                v.gain.a_rate(block, gains, rate, x86);
            } else {
                gains.fill(v.gain.value());
            }
            let send_zero = !v.send.has_events() && v.send.value() == 0.0;
            if v.send.has_events() {
                v.send.a_rate(block, sends, rate, x86);
            } else {
                sends.fill(v.send.value());
            }
            let [b0, b1, b2, a1, a2] = v.b;
            let (mut x1, mut x2, mut y1, mut y2) = (v.x1, v.x2, v.y1, v.y2);
            let out = &mut inputs[slot * FRAMES..(slot + 1) * FRAMES];
            let mut pos = v.pos;
            for i in 0..FRAMES {
                let at = frame + i as f64;
                let s = if v.ended || at < v.start || at >= v.stop {
                    0.0f32
                } else {
                    let k = pos as usize;
                    let f = pos - k as f64;
                    let next = if k + 1 < len { data[k + 1] } else if v.looping { data[0] } else { data[k] };
                    let s = ((1.0 - f) * data[k] as f64 + f * next as f64) as f32;
                    pos += step;
                    if pos >= len as f64 {
                        if v.looping {
                            pos = (pos - len as f64) % len as f64;
                        } else {
                            v.ended = true;
                        }
                    }
                    s
                };
                let g = if gain_zero { 0.0 } else { s * gains[i] };
                if !send_zero {
                    send[i] += g * sends[i];
                }
                let x = g as f64;
                let y = (b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) as f32;
                out[i] = y;
                x2 = x1;
                x1 = x;
                y2 = y1;
                y1 = y as f64;
            }
            v.pos = pos;
            v.x1 = flush(x1);
            v.x2 = flush(x2);
            v.y1 = flush(y1);
            v.y2 = flush(y2);
            present[slot] = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_voice_plays_its_sample_at_its_rate() {
        let mut e = Engine::new(48000.0, 4, -3.0103, true);
        let data = e.sample_storage(0, 1000, 24000.0);
        for (i, x) in data.iter_mut().enumerate() {
            *x = (i as f32) / 1000.0;
        }
        let c = e.command_storage(8 + 4);
        c[..8].copy_from_slice(&[START as f64, 2.0, 0.0, 0.0, 0.0, 0.5, 24000.0, 0.0]);
        c[8..12].copy_from_slice(&[GAIN_SET as f64, 2.0, 1.0, 0.0]);
        e.apply_commands(12);
        let mut inputs = vec![0.0f32; 4 * FRAMES];
        let mut present = vec![0u8; 4];
        e.render(&mut inputs, &mut present);
        assert_eq!(present, [0, 0, 1, 0]);
        // A quarter of the sample's rate: output sample 4 is the sample's
        // second (the lowpass at the Nyquist frequency passes it as it is).
        assert_eq!(inputs[2 * FRAMES + 4], 0.001);
        assert_eq!(inputs[2 * FRAMES + 2], ((0.5 * 0.0f32 as f64 + 0.5 * 0.001f32 as f64) as f32));
        // 1000 frames at 0.25 a frame: past 4000 frames, silence.
        for _ in 0..32 {
            e.render(&mut inputs, &mut present);
        }
        assert!(e.voices[2].ended);
        assert!(inputs[2 * FRAMES..3 * FRAMES].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn a_release_fades_then_stops_the_source() {
        let mut e = Engine::new(48000.0, 1, -3.0103, true);
        let data = e.sample_storage(0, 48000, 48000.0);
        data.fill(0.5);
        let c = e.command_storage(8 + 4);
        c[..8].copy_from_slice(&[START as f64, 0.0, 0.0, 0.0, 1.0, 1.0, 24000.0, 0.0]);
        c[8..12].copy_from_slice(&[GAIN_SET as f64, 0.0, 1.0, 0.0]);
        e.apply_commands(12);
        let mut inputs = vec![0.0f32; FRAMES];
        let mut present = vec![0u8; 1];
        e.render(&mut inputs, &mut present);
        assert!(inputs.iter().all(|&x| x == 0.5));
        // Released at the next block, 40 ms to its stop: frame 128 + 1920.
        let at = 128.0 / 48000.0;
        let c = e.command_storage(5);
        c.copy_from_slice(&[RELEASE as f64, 0.0, 0.01, 0.04, at]);
        e.apply_commands(5);
        for _ in 1..16 {
            e.render(&mut inputs, &mut present);
        }
        assert!(inputs[FRAMES - 1] > 0.0 && inputs[FRAMES - 1] < 0.05);
        e.render(&mut inputs, &mut present);
        assert!(inputs.iter().all(|&x| x == 0.0));
    }
}
