//! Menu, music and loading playback. Like the battle output, a whole
//! quantum is Rust-owned. The shell has linear bus/voice fades, no limiter.
// Bus numbers are the shell's two gain trees: effects and music.
// All storage, including completion events, exists before the first block.
use crate::{commands::*, master::gain_product, pcm::Source, voices::Sample, FRAMES};
// FrontendAudio sends assets and timed controls from the page. This module
// reproduces ShellAudio and LoadingSound's BufferSource/GainNode tree; its
// worklet owns all mutable sources, bus sums and fade state.
pub const SLOTS: usize = FRONT_SLOTS;
// GainNode endpoints are float32, while time and interpolation stay f64.
// A replacement fade starts at the host's last completed gain sample.
#[derive(Clone, Copy)]
struct Linear {
    from: f32,
    to: f32,
    start: f64,
    end: f64,
}
impl Linear {
    fn constant(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: 0.0,
            end: 0.0,
        }
    }
    // A completed or not-yet-started ramp returns an exact endpoint. Inside
    // the ramp, compute in f64 and store once to f32, matching GainNode's linear
    // automation rather than accumulating a float increment over time.
    fn value(&self, frame: f64) -> f32 {
        if frame < self.start {
            self.from
        } else if frame >= self.end {
            self.to
        } else {
            (self.from as f64
                + (self.to as f64 - self.from as f64) * (frame - self.start)
                    / (self.end - self.start)) as f32
        }
    }
    // ShellAudio.fadeBus and LoadingSound.dispose cancel their old curve
    // before replacing it. Scheduled pulse fades use the initial level.
    fn ramp(&mut self, value: f32, start: f64, end: f64) {
        *self = Self {
            from: self.value(start - 1.0),
            to: value,
            start,
            end,
        };
    }
}
// generation identifies the page's logical playback, independently of its
// reusable slot. Completion messages carry both so a late onended cannot
// release a new track that has already taken the old slot.
struct Voice {
    source: Source,
    gain: Linear,
    bus: usize,
    generation: u32,
    active: bool,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            source: Source::default(),
            gain: Linear::constant(1.0),
            bus: 0,
            generation: 0,
            active: false,
        }
    }
}
// All render scratch and the worst-case completion record are fixed-size.
// Message handlers may install/release samples and grow command storage;
// process only mutates existing buffers and never frees finished PCM.
pub struct Frontend {
    rate: f64,
    voices: [Voice; SLOTS],
    pub samples: [Option<Sample>; SLOTS],
    pub commands: Vec<f64>,
    scratch: [f32; 12 * FRAMES],
    music: [f32; 2 * FRAMES],
    fx: [f32; 2 * FRAMES],
    pub output: [f32; 2 * FRAMES],
    pub ended: [u32; FRONT_END_STRIDE * SLOTS],
    master_gain: f32,
    music_gain: f32,
    fx_gain: f32,
    dip: Linear,
}
// Preserve GainNode's zero and unity shortcuts and its f32 multiply
// boundary. Do not fold voice, bus and master levels into a single gain:
// rounding at each former node contributes to the recorded output.
fn gain(value: f32, level: f32) -> f32 {
    if level == 1.0 {
        value
    } else if level == 0.0 {
        0.0
    } else {
        gain_product(value, level)
    }
}
impl Frontend {
    // Match ShellAudio's initial music/effect levels. A source can begin
    // before the page sends a volume update, so defaults are audible behavior,
    // not merely placeholder state for configuration.
    pub fn new(rate: f64) -> Self {
        Self {
            rate,
            voices: std::array::from_fn(|_| Voice::default()),
            samples: std::array::from_fn(|_| None),
            commands: Vec::new(),
            scratch: [0.0; 12 * FRAMES],
            music: [0.0; 2 * FRAMES],
            fx: [0.0; 2 * FRAMES],
            output: [0.0; 2 * FRAMES],
            ended: [0; FRONT_END_STRIDE * SLOTS],
            master_gain: 1.0,
            music_gain: 0.6,
            fx_gain: 0.3,
            dip: Linear::constant(1.0),
        }
    }
    // PCM is installed by a worklet message, between render calls. The
    // main thread releases a finished slot before reusing its generation.
    pub fn sample(&mut self, slot: usize, frames: usize, channels: usize, rate: f64) -> *mut f32 {
        let data = vec![0.0; frames * channels];
        self.samples[slot] = Some(Sample {
            frames,
            channels,
            rate,
            data,
            ring: None,
        });
        self.samples[slot].as_mut().unwrap().data.as_mut_ptr()
    }
    // Apply one message batch. Times remain in the context clock; source
    // starts and linear fades are resolved per sample inside process.
    pub fn apply(&mut self) {
        let mut at = 0;
        while at < self.commands.len() {
            let op = self.commands[at] as u32;
            let n = arity(op);
            if n == 0 || at + n >= self.commands.len() {
                break;
            }
            let c = &self.commands[at + 1..at + n + 1];
            match op {
                // Starting replaces one scheduled source after PCM is installed. The
                // page supplies a generation and chooses the effects/music bus; restarting
                // a slot resets its voice fade without resetting either bus's gain.
                FRONT_START => {
                    let slot = c[0] as usize;
                    if slot < SLOTS {
                        if let Some(sample) = &self.samples[slot] {
                            let v = &mut self.voices[slot];
                            v.source
                                .start(slot, sample, c[2], c[3], c[4], c[5] != 0.0, self.rate);
                            v.generation = c[1] as u32;
                            v.active = true;
                            v.gain = Linear::constant(c[6] as f32);
                            v.bus = usize::from(c[7] != 0.0);
                        }
                    }
                }
                // LoadingSound.dispose fades then stops at the same context time.
                // Retiring at fade start would lose the release; leaving the source alive
                // after fade end would delay onended and retain its PCM unnecessarily.
                FRONT_FADE => {
                    if let Some(v) = self.voices.get_mut(c[0] as usize) {
                        v.gain.ramp(0.0, c[1] * self.rate, c[2] * self.rate);
                        v.source.stop(c[2], self.rate);
                    }
                }
                FRONT_STOP => {
                    if let Some(v) = self.voices.get_mut(c[0] as usize) {
                        v.source.stop(c[1], self.rate);
                    }
                }
                // These are GainNode value assignments, not ramps. Store float endpoints
                // separately even when a subsequent master multiplication could be combined.
                FRONT_VOLUME => {
                    self.master_gain = c[0] as f32;
                    self.music_gain = c[1] as f32;
                    self.fx_gain = c[2] as f32;
                }
                // ShellAudio.fadeBus ducks music without changing the configured music
                // volume. Effects stay outside that dip, so loading or UI pulses retain
                // their own level while a track fades under another sound.
                FRONT_DIP => {
                    self.dip
                        .ramp(c[0] as f32, c[2] * self.rate, (c[2] + c[1]) * self.rate);
                }
                _ => {}
            }
            at += n + 1;
        }
    }
    // Source -> individual level -> bus sum -> dip/music or effects level
    // -> master. Keep each former gain boundary's f32 store separate.
    pub fn process(&mut self, frame: u64) -> usize {
        self.music.fill(0.0);
        self.fx.fill(0.0);
        // Loading pulses are mono. Keep that bus and its gain arithmetic
        // mono until the final device copy, as the former graph did. Music
        // or a stereo effect widens the whole quantum before mixing.
        let mut mono = true;
        for (voice, sample) in self.voices.iter().zip(&self.samples) {
            if voice.active && sample.as_ref().is_some_and(|s| s.channels != 1) {
                mono = false;
                break;
            }
        }
        // Each active voice can end once in this quantum, so the fixed record has
        // room for every slot. Report after rendering and let a later message free
        // its PCM; no completion callback or allocation runs inside this loop.
        let mut ended = 0;
        for (slot, v) in self.voices.iter_mut().enumerate() {
            if !v.active {
                continue;
            }
            if let Some(sample) = &self.samples[slot] {
                self.scratch.fill(0.0);
                v.source
                    .process(sample, frame as f64, &mut self.scratch, false);
                let bus = if v.bus == 1 {
                    &mut self.music
                } else {
                    &mut self.fx
                };
                if mono {
                    // Most loading quanta precede a scheduled fade. Use
                    // the same constant-gain shortcut as the voice engine.
                    let constant = v.gain.from == v.gain.to
                        || ((frame + FRAMES as u64 - 1) as f64) < v.gain.start
                        || frame as f64 >= v.gain.end;
                    if constant {
                        let level = v.gain.value(frame as f64);
                        for (dst, &sample) in bus.iter_mut().zip(&self.scratch) {
                            *dst += gain(sample, level);
                        }
                    } else {
                        for (i, (dst, &sample)) in bus.iter_mut().zip(&self.scratch).enumerate() {
                            let level = v.gain.value((frame + i as u64) as f64);
                            *dst += gain(sample, level);
                        }
                    }
                } else {
                    for i in 0..FRAMES {
                        let level = v.gain.value((frame + i as u64) as f64);
                        // Mono is duplicated; stereo passes through. Surround
                        // assets use Web Audio's defined speakers downmix.
                        let s = &self.scratch;
                        // The former shell output used speaker interpretation for quad and 5.1.
                        // Keep its half-level quad sum and sqrt(1/2) centre/surround contribution.
                        // LFE is omitted from stereo, rather than folded into both channels.
                        let (l, r) = match sample.channels {
                            1 => (s[i], s[i]),
                            4 => (
                                0.5 * (s[i] + s[2 * FRAMES + i]),
                                0.5 * (s[FRAMES + i] + s[3 * FRAMES + i]),
                            ),
                            6 => (
                                s[i] + std::f32::consts::FRAC_1_SQRT_2
                                    * (s[2 * FRAMES + i] + s[4 * FRAMES + i]),
                                s[FRAMES + i]
                                    + std::f32::consts::FRAC_1_SQRT_2
                                        * (s[2 * FRAMES + i] + s[5 * FRAMES + i]),
                            ),
                            _ => (s[i], s[FRAMES + i]),
                        };
                        bus[i] += gain(l, level);
                        bus[FRAMES + i] += gain(r, level);
                    }
                }
            }
            // Retire once, without freeing PCM on the render call. The
            // host copies this fixed ring only when something ended.
            if !v.source.on {
                v.active = false;
                self.ended[FRONT_END_STRIDE * ended + FRONT_END_SLOT] = slot as u32;
                self.ended[FRONT_END_STRIDE * ended + FRONT_END_GENERATION] = v.generation;
                ended += 1;
            }
        }
        // Apply voice gain before summing, music dip before music level, then
        // combine the two buses before master level. Each gain call retains its
        // former f32 store, including the mono path's final duplication.
        for i in 0..FRAMES {
            let dip = self.dip.value((frame + i as u64) as f64);
            for c in 0..if mono { 1 } else { 2 } {
                let at = c * FRAMES + i;
                self.output[at] = gain(
                    gain(gain(self.music[at], dip), self.music_gain)
                        + gain(self.fx[at], self.fx_gain),
                    self.master_gain,
                );
            }
        }
        // The former mono bus widens only at the destination. Duplicate the final
        // stored sample rather than recomputing right-channel gain arithmetic.
        if mono {
            let (left, right) = self.output.split_at_mut(FRAMES);
            right.copy_from_slice(left);
        }
        ended
    }
}
