//! The master chain in the mixer's own module: what follows the spatial mixer in src/audio/spatial.js
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
use crate::stabilizer::Stabilizer;
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
    original: Option<(Stabilizer, f32)>,
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

// Native GainNodes flush subnormal products, while a unity gain copies
// its input verbatim (including subnormals produced by a worklet).
fn gain_product(sample: f32, gain: f32) -> f32 {
    let value = sample * gain;
    if value.abs() < f32::MIN_POSITIVE { 0.0f32.copysign(value) } else { value }
}

impl Master {
    /// A chain for `channels` at the context's `rate`: the reverb as its
    /// worklet starts (its parameters placed after, `place_room`), the
    /// limiter at `ceiling` dB with `release` and `lookahead` s, the volume
    /// at 1; no high-pass, LFE, EQ or clarity until they are set.
    pub fn new(rate: f64, channels: usize, ceiling: f64, release: f64, lookahead: f64, x86: bool) -> Master {
        let n = channels * FRAMES;
        let mut master = Master {
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
            original: None,
            limiter: Limiter::new(rate, ceiling, release, lookahead, &[0.0; DYNAMICS_COUNT], 0),
            volume: Timeline::new(1.0),
            send: vec![0.0; FRAMES],
            wet: vec![0.0; n],
            low: vec![0.0; FRAMES],
            mix: vec![0.0; n],
            stage: vec![0.0; n],
            gains: [0.0; FRAMES],
        };
        // Reserve fixed render-quantum scratch before the first block.
        master.reverb.scratch(FRAMES);
        master.limiter.setup(channels);
        master.limiter.scratch(FRAMES);
        master
    }

    /// Classic surround's front stabilizer followed by its output level.
    pub fn set_original(&mut self, centre: usize, level: f32) {
        self.original = Some((Stabilizer::new(self.rate, self.channels, 0, 1, centre), level));
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
        if let Some((stabilizer, level)) = &mut self.original {
            let input = Block { data: &self.mix[..len], channels: n, frames: FRAMES };
            let mut output = BlockMut { data: &mut self.stage[..len], channels: n, frames: FRAMES };
            stabilizer.process(&input, &mut output);
            for i in 0..len {
                self.mix[i] = gain_product(self.stage[i], *level);
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
                self.mix[i] = if level == 1.0 { self.stage[i] } else if level == 0.0 { 0.0 } else { gain_product(self.stage[i], level) };
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
                    out[at + i] = gain_product(self.stage[at + i], self.gains[i]);
                }
            }
        } else {
            let g = self.volume.k_rate(block, rate);
            for i in 0..len {
                out[i] = if g == 0.0 { 0.0 } else if g == 1.0 { self.stage[i] } else { gain_product(self.stage[i], g) };
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48000.0;

    // A chain over `channels`, its room long and the send's high-pass in.
    fn chain(channels: usize) -> Master {
        let mut m = Master::new(RATE, channels, -1.0, 0.08, 0.0015, true);
        let mut room = [0.0; PARAM_COUNT];
        room[0] = 0.5;
        room[2] = 1.2;
        m.room(&room, 0b101, true);
        m.set_highpass(180.0, -3.0103);
        m
    }

    // Each channel's peak over `blocks` blocks of the chain, as given.
    #[allow(clippy::too_many_arguments)]
    fn peaks(m: &mut Master, blocks: usize, feeds: &[f32], lfe: Option<&[f32]>, send: &[f32], send_in: Option<&[f32]>, mix_in: Option<&[f32]>) -> Vec<f32> {
        let n = m.channels;
        let mut out = vec![0.0; n * FRAMES];
        let mut peak = vec![0.0f32; n];
        for b in 0..blocks {
            m.process(feeds, lfe, send, send_in, mix_in, (b * FRAMES) as u64, &mut out);
            for c in 0..n {
                for x in &out[c * FRAMES..(c + 1) * FRAMES] {
                    peak[c] = peak[c].max(x.abs());
                }
            }
        }
        peak
    }

    #[test]
    fn the_reverb_reaches_the_speakers_it_should_and_the_lfe_its_channel() {
        // Left, right, LFE: a send and an LFE send, no feeds.
        let mut m = chain(3);
        m.set_rooms(0b011);
        m.set_lfe(2, 120.0, -3.0103);
        let silence = vec![0.0f32; 3 * FRAMES];
        let tone: Vec<f32> = (0..FRAMES).map(|i| 0.5 * (i as f32 * 0.3).sin()).collect();
        let low: Vec<f32> = (0..FRAMES).map(|i| 0.5 * (i as f32 * 0.01).sin()).collect();
        let none = vec![0.0f32; FRAMES];
        let peak = peaks(&mut m, 400, &silence, Some(&none), &tone, None, None);
        assert!(peak[0] > 1e-3 && peak[1] > 1e-3, "the room's on the speakers: {peak:?}");
        assert_eq!(peak[2], 0.0, "and not on the LFE");
        let peak = peaks(&mut m, 200, &silence, Some(&low), &none, None, None);
        assert!(peak[2] > 1e-3, "the LFE's send on its channel: {peak:?}");
    }

    #[test]
    fn what_comes_through_the_browser_joins_the_send_and_the_mix() {
        let silence = vec![0.0f32; 2 * FRAMES];
        let none = vec![0.0f32; FRAMES];
        let tone: Vec<f32> = (0..FRAMES).map(|i| 0.5 * (i as f32 * 0.3).sin()).collect();
        // An input to the send sounds as the engine's own send does.
        let (mut a, mut b) = (chain(2), chain(2));
        let by_engine = peaks(&mut a, 200, &silence, None, &tone, None, None);
        let by_input = peaks(&mut b, 200, &silence, None, &none, Some(&tone), None);
        assert_eq!(by_engine, by_input);
        // The mix's input sounds as the speaker feeds do.
        let feeds: Vec<f32> = tone.iter().chain(tone.iter()).copied().collect();
        let (mut a, mut b) = (chain(2), chain(2));
        let by_feeds = peaks(&mut a, 50, &feeds, None, &none, None, None);
        let by_mix = peaks(&mut b, 50, &silence, None, &none, None, Some(&feeds));
        assert_eq!(by_feeds, by_mix);
        assert!(by_mix[0] > 0.1);
    }

    #[test]
    fn the_volume_follows_its_value_and_its_automation() {
        let mut m = Master::new(RATE, 1, 0.0, 0.08, 0.0015, true);
        let feeds = vec![0.25f32; FRAMES];
        let none = vec![0.0f32; FRAMES];
        m.volume(0.5, 0.0, 0.0, true, 0.0);
        let at = |m: &mut Master, b: usize| {
            let mut out = vec![0.0; FRAMES];
            m.process(&feeds, None, &none, None, None, (b * FRAMES) as u64, &mut out);
            out[FRAMES - 1]
        };
        // Past the limiter's lookahead: the set value on the signal.
        let mut last = 0.0;
        for b in 0..8 {
            last = at(&mut m, b);
        }
        assert_eq!(last, 0.125);
        // A fade to 0, 50 ms a time constant: e^-1 of the way left at 50 ms.
        let frame = (8 * FRAMES) as f64;
        m.volume(0.0, 0.05, frame / RATE, false, frame);
        let blocks = (0.05 * RATE / FRAMES as f64).round() as usize;
        for b in 8..8 + blocks {
            last = at(&mut m, b);
        }
        assert!((last - 0.125 * 0.3679).abs() < 0.002, "{last}");
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Counting;
    thread_local! { static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) }; }
    #[global_allocator]
    static ALLOCATOR: Counting = Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.with(|n| { if let Some(v) = n.get() { n.set(Some(v + 1)); } });
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout); }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            ALLOCS.with(|n| { if let Some(v) = n.get() { n.set(Some(v + 1)); } });
            System.realloc(ptr, layout, size)
        }
    }

    #[test]
    fn output_paths_allocate_nothing_from_the_first_render_block() {
        use crate::voices::{Engine, START, GAIN_SET};
        for (channels, classic) in [(6, true), (8, true), (12, true), (2, false)] {
            let mut master = Master::new(48000.0, channels, -1.0, 0.06, 0.0015, true);
            if classic { master.set_original(2, 1.2); }
            else { master.set_clarity(-1, 0.94); }
            let mut voices = Engine::new(48000.0, 2, -3.0103, true);
            voices.filtered = !classic;
            voices.sample_storage(0, 1024, 48000.0).fill(0.25);
            voices.command_storage(12).copy_from_slice(&[START as f64, 0.0, 0.0, 0.0, 1.0, 1.0, 6000.0, 0.0, GAIN_SET as f64, 0.0, 0.5, 0.0]);
            voices.apply_commands(12);
            let mut slots = [0.0; 2 * FRAMES];
            let mut present = [0; 2];
            let feeds = vec![0.1; channels * FRAMES];
            let mut out = vec![0.0; channels * FRAMES];
            ALLOCS.with(|n| n.set(Some(0)));
            for b in 0..64 {
                voices.render(&mut slots, &mut present);
                master.process(&feeds, None, &voices.send, None, None, b * FRAMES as u64, &mut out);
            }
            let count = ALLOCS.with(|n| n.replace(None).unwrap());
            assert_eq!(count, 0, "{channels} channels, classic={classic}");
        }
    }
}
