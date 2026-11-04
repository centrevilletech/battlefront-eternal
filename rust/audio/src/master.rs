//! The enhanced mode's master chain in the mixer's own module, for
//! `?master=rust`: what follows the spatial mixer in src/audio/spatial.js
//! (Renderer.build), worked out here instead of in the browser's nodes and
//! three more worklets. Per block, as the graph computes it:
//!
//! - the reverb's send (the voice engine's, plus the browser's voices'
//!   sends that reach it as an input) through its high-pass (a
//!   BiquadFilterNode) into the room reverb (src/reverb.rs);
//! - the LFE send (surround) through two low-passes into the LFE channel;
//! - the mix: the speaker feeds, the reverb on each channel with a speaker,
//!   the LFE, and what else reached the mix (streams, as an input); a node
//!   input adds its connections in an order of its own, which matters only
//!   from three of them on (docs/audio.md, "The voice engine");
//! - the clarity EQ (three BiquadFilterNodes on every channel), the clarity
//!   stage (src/clarity.rs) and its level (a GainNode), when clarity is on;
//! - the limiter (src/limiter.rs) and the master volume (a GainNode moved
//!   by setTargetAtTime).
//!
//! The filters are src/biquad.rs (Chromium's), the volume's automation
//! src/timeline.rs; the reverb, clarity and limiter are the worklets' own
//! processors. Nothing allocates while rendering, past each processor's
//! own first block.

use crate::biquad::{Biquad, Kind};
use crate::clarity::Clarity;
use crate::limiter::{Limiter, DYNAMICS_COUNT};
use crate::reverb::{Reverb, PARAM_COUNT};
use crate::timeline::Timeline;
use crate::{Block, BlockMut, FRAMES};

pub struct Master {
    rate: f64,
    channels: usize,
    x86: bool,
    // The send's high-pass (none in the original mode).
    hp: Option<Biquad>,
    reverb: Reverb,
    // The channels the reverb reaches (a bit each; the LFE's left out).
    rooms: u32,
    // The LFE: its channel and its two low-passes.
    lfe: Option<(usize, [Biquad; 2])>,
    // The clarity EQ: its stages, each a filter a channel.
    eq: Vec<Vec<Biquad>>,
    clarity: Option<Clarity>,
    level: f32,
    limiter: Limiter,
    volume: Timeline,
    // Scratch: the send, the reverb's output, the LFE, the mix, a stage's
    // output, the volume's values.
    send: Vec<f32>,
    wet: Vec<f32>,
    low: Vec<f32>,
    mix: Vec<f32>,
    stage: Vec<f32>,
    gains: [f32; FRAMES],
}

/// A filter's kind by the number the page sends (0 lowpass, 1 highpass,
/// 2 peaking, 3 highshelf).
pub fn kind(n: u32) -> Kind {
    match n {
        0 => Kind::Lowpass,
        1 => Kind::Highpass,
        2 => Kind::Peaking,
        _ => Kind::Highshelf,
    }
}

impl Master {
    /// A chain for `channels` at the context's `rate`: the reverb as its
    /// worklet starts (its parameters placed after, `place_room`), the
    /// limiter at `ceiling` dB with `release` and `lookahead` s, the volume
    /// at 1; no high-pass, LFE, EQ or clarity until they are set.
    pub fn new(rate: f64, channels: usize, ceiling: f64, release: f64, lookahead: f64, x86: bool) -> Master {
        let n = channels * FRAMES;
        Master {
            rate,
            channels,
            x86,
            hp: None,
            reverb: Reverb::new(rate, channels, &[0.0; PARAM_COUNT], 0),
            rooms: u32::MAX,
            lfe: None,
            eq: Vec::new(),
            clarity: None,
            level: 1.0,
            limiter: Limiter::new(rate, ceiling, release, lookahead, &[0.0; DYNAMICS_COUNT], 0),
            volume: Timeline::new(1.0),
            send: vec![0.0; FRAMES],
            wet: vec![0.0; n],
            low: vec![0.0; FRAMES],
            mix: vec![0.0; n],
            stage: vec![0.0; n],
            gains: [0.0; FRAMES],
        }
    }

    pub fn set_highpass(&mut self, frequency: f32, q: f32) {
        self.hp = Some(Biquad::new(Kind::Highpass, frequency, q, 0.0, self.rate));
    }

    pub fn set_rooms(&mut self, mask: u32) {
        self.rooms = mask;
    }

    pub fn set_lfe(&mut self, channel: usize, cutoff: f32, q: f32) {
        let f = Biquad::new(Kind::Lowpass, cutoff, q, 0.0, self.rate);
        self.lfe = Some((channel, [f, f]));
    }

    /// One more EQ stage, on every channel.
    pub fn add_eq(&mut self, kind: Kind, frequency: f32, gain: f32, q: f32) {
        let f = Biquad::new(kind, frequency, q, gain, self.rate);
        self.eq.push(vec![f; self.channels]);
    }

    /// The clarity stage (`skip`: the LFE's index, or -1) and its level.
    pub fn set_clarity(&mut self, skip: i64, level: f32) {
        self.clarity = Some(Clarity::new(self.rate, self.channels, skip));
        self.level = level;
    }

    /// The reverb's parameters (`initial`: in force at once, as its
    /// worklet's construction puts them; else glided to, as a message).
    pub fn room(&mut self, params: &[f64; PARAM_COUNT], mask: u32, initial: bool) {
        if initial {
            self.reverb.place(params, mask);
        } else {
            self.reverb.set(params, mask);
        }
    }

    pub fn dynamics(&mut self, d: &[f64; DYNAMICS_COUNT], mask: u32) {
        self.limiter.set_dynamics(d, mask);
    }

    /// The volume: its value set at context time `at` (`set`, as
    /// AudioParam.value), or a setTargetAtTime; `frame` is the next
    /// block's.
    pub fn volume(&mut self, value: f32, tau: f64, at: f64, set: bool, frame: f64) {
        if set {
            self.volume.set_value(value);
        }
        self.volume.insert(!set, value, at, tau, frame, self.rate);
    }

    /// One block at context frame `block`: `feeds` the mixer's speaker
    /// feeds (channels runs), `lfe_send` its LFE send (surround), `send`
    /// the engine's reverb send, `send_in` and `mix_in` the inputs that
    /// carry the browser's own voices' sends and everything else bound for
    /// the mix (None when silent). Writes `out` (channels runs); returns
    /// the limiter's gain reduction (dB) when a report is due.
    #[allow(clippy::too_many_arguments)]
    pub fn process(&mut self, feeds: &[f32], lfe_send: Option<&[f32]>, send: &[f32], send_in: Option<&[f32]>, mix_in: Option<&[f32]>, block: u64, out: &mut [f32]) -> Option<f64> {
        let n = self.channels;
        let len = n * FRAMES;
        // The send into the reverb: the engine's, and the input's.
        match send_in {
            Some(s) => {
                for i in 0..FRAMES {
                    self.send[i] = send[i] + s[i];
                }
            }
            None => self.send.copy_from_slice(&send[..FRAMES]),
        }
        if let Some(hp) = &mut self.hp {
            hp.process_in_place(&mut self.send);
        }
        {
            let input = Block { data: &self.send, channels: 1, frames: FRAMES };
            let mut wet = BlockMut { data: &mut self.wet[..len], channels: n, frames: FRAMES };
            self.reverb.process(&input, &mut wet);
        }
        // The LFE's low-passes.
        let lfe_channel = match (&mut self.lfe, lfe_send) {
            (Some((c, [a, b])), Some(l)) => {
                a.process(&l[..FRAMES], &mut self.low);
                b.process_in_place(&mut self.low);
                Some(*c)
            }
            _ => None,
        };
        // The mix.
        for c in 0..n {
            let at = c * FRAMES;
            let m = &mut self.mix[at..at + FRAMES];
            m.copy_from_slice(&feeds[at..at + FRAMES]);
            if self.rooms & (1 << c) != 0 {
                let w = &self.wet[at..at + FRAMES];
                for i in 0..FRAMES {
                    m[i] += w[i];
                }
            }
            if lfe_channel == Some(c) {
                for i in 0..FRAMES {
                    m[i] += self.low[i];
                }
            }
            if let Some(b) = mix_in {
                let b = &b[at..at + FRAMES];
                for i in 0..FRAMES {
                    m[i] += b[i];
                }
            }
        }
        // The clarity EQ, then the clarity stage at its level.
        for stage in &mut self.eq {
            for (c, f) in stage.iter_mut().enumerate() {
                f.process_in_place(&mut self.mix[c * FRAMES..(c + 1) * FRAMES]);
            }
        }
        if let Some(clarity) = &mut self.clarity {
            {
                let input = Block { data: &self.mix[..len], channels: n, frames: FRAMES };
                let mut output = BlockMut { data: &mut self.stage[..len], channels: n, frames: FRAMES };
                clarity.process(&input, &mut output);
            }
            // A GainNode: 1 copies, 0 silences, else a product a sample.
            let level = self.level;
            for i in 0..len {
                self.mix[i] = if level == 1.0 { self.stage[i] } else if level == 0.0 { 0.0 } else { self.stage[i] * level };
            }
        }
        // The limiter.
        let report = {
            let input = Block { data: &self.mix[..len], channels: n, frames: FRAMES };
            let mut output = BlockMut { data: &mut self.stage[..len], channels: n, frames: FRAMES };
            self.limiter.process(&input, &mut output)
        };
        // The volume, a GainNode: a value a sample while its automation
        // runs, else its value for the block (1 copies, 0 silences).
        let rate = self.rate;
        if self.volume.sample_accurate(block, rate) {
            self.volume.a_rate(block, &mut self.gains, rate, self.x86);
            for c in 0..n {
                let at = c * FRAMES;
                for i in 0..FRAMES {
                    out[at + i] = self.stage[at + i] * self.gains[i];
                }
            }
        } else {
            let g = self.volume.k_rate(block, rate);
            for i in 0..len {
                out[i] = if g == 0.0 { 0.0 } else if g == 1.0 { self.stage[i] } else { self.stage[i] * g };
            }
        }
        report
    }
}
