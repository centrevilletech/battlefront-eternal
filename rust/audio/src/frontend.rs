//! Menu, music and loading playback. Like the battle output, a whole
//! quantum is Rust-owned. The shell has linear bus/voice fades, no limiter.
// Bus numbers are the shell's two gain trees: effects and music.
// All storage, including completion events, exists before the first block.
use crate::{commands::*, master::gain_product, pcm::Source, voices::Sample, FRAMES};
pub const SLOTS: usize = FRONT_SLOTS;
// GainNode endpoints are float32, while time and interpolation stay f64.
// A replacement fade starts at the old curve's value on the same frame.
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
            from: self.value(start),
            to: value,
            start,
            end,
        };
    }
}
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
                FRONT_VOLUME => {
                    self.master_gain = c[0] as f32;
                    self.music_gain = c[1] as f32;
                    self.fx_gain = c[2] as f32;
                }
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
                for i in 0..FRAMES {
                    let level = v.gain.value((frame + i as u64) as f64);
                    // Mono is duplicated; stereo passes through. Surround
                    // assets use Web Audio's defined speakers downmix.
                    let s = &self.scratch;
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
            // Retire once, without freeing PCM on the render call. The
            // host copies this fixed ring only when something ended.
            if !v.source.on {
                v.active = false;
                self.ended[FRONT_END_STRIDE * ended + FRONT_END_SLOT] = slot as u32;
                self.ended[FRONT_END_STRIDE * ended + FRONT_END_GENERATION] = v.generation;
                ended += 1;
            }
        }
        for i in 0..FRAMES {
            let dip = self.dip.value((frame + i as u64) as f64);
            for c in 0..2 {
                let at = c * FRAMES + i;
                self.output[at] = gain(
                    gain(gain(self.music[at], dip), self.music_gain)
                        + gain(self.fx[at], self.fx_gain),
                    self.master_gain,
                );
            }
        }
        ended
    }
}
