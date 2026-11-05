//! The complete audio render quantum: Rust owns source playback, beds,
//! placement, headphone decoding, room sends, mono and the master chain.
//! The host supplies decoded PCM and timed commands, then copies one output.

use crate::biquad::{Biquad, Kind};
use crate::commands::*;
use crate::convolution::StereoConvolver;
use crate::hrtf::{Database, Panner};
use crate::master::Master;
use crate::mixer::{Decoder, Mixer, Speaker};
use crate::pcm::{Stream, CHANNELS};
use crate::stereo::{alsoft, mono, LightPan};
use crate::timeline::Timeline;
use crate::voices::Engine;
use crate::{host, BlockMut, FRAMES};
use std::f64::consts::PI;

pub const VOICES: usize = 64;
pub const STREAMS: usize = 32;
const SLOTS: usize = VOICES + STREAMS * CHANNELS;
const HRTF_LEVEL: f64 = 0.6382634861905486; // 10^(-3.9/20), stored by GainNode

pub struct Placement {
    pub active: bool,
    pub spatial: bool,
    pub axes: [Timeline; 3],
    pub pan: Timeline,
    pub light: LightPan,
}
impl Default for Placement {
    fn default() -> Self {
        Self {
            active: false,
            spatial: true,
            axes: [Timeline::new(0.0); 3],
            pan: Timeline::new(0.0),
            light: LightPan::default(),
        }
    }
}

struct Light {
    volume: Timeline,
    original: f32,
    level: f32,
    curve: Vec<f32>,
    gains: [f32; FRAMES],
}
impl Light {
    fn new() -> Self {
        let knee = host::pow(10.0, -1.0 / 20.0);
        let curve = (0..4097)
            .map(|i| {
                let x = 2.0 * (2.0 * i as f64 / 4096.0 - 1.0);
                let a = x.abs();
                (if a <= knee {
                    x
                } else {
                    x.signum() * (knee + (1.0 - knee) * v8math::tanh((a - knee) / (1.0 - knee)))
                }) as f32
            })
            .collect();
        Self {
            volume: Timeline::new(1.0),
            original: host::pow(10.0, 1.5 / 20.0) as f32,
            level: (host::pow(10.0, -6.1 / 20.0) / 2.0) as f32,
            curve,
            gains: [0.0; FRAMES],
        }
    }
    fn process(&mut self, input: &mut [f32], output: &mut [f32], block: u64, rate: f64, x86: bool, fold: bool) {
        for v in input.iter_mut() {
            *v *= self.original;
        }
        if fold {
            mono(input, 2);
        }
        self.volume.a_rate(block, &mut self.gains, rate, x86);
        // A mono fold has identical lanes. Shape it once, then copy.
        for c in 0..if fold { 1 } else { 2 } {
            for i in 0..FRAMES {
                let value = input[c * FRAMES + i] * self.level;
                let index = ((value + 1.0) * 2048.0).clamp(0.0, 4096.0);
                let a = index as usize;
                let b = (a + 1).min(4096);
                let fraction = index - a as f32;
                let shaped = self.curve[a] + fraction * (self.curve[b] - self.curve[a]);
                output[c * FRAMES + i] = if self.gains[i] == 1.0 { shaped } else { shaped * self.gains[i] };
            }
        }
        if fold {
            let (left, right) = output.split_at_mut(FRAMES);
            right[..FRAMES].copy_from_slice(left);
        }
    }
}

pub struct Output {
    pub rate: f64,
    pub channels: usize,
    pub enhanced: bool,
    pub headphones: bool,
    pub binaural: bool,
    pub mono: bool,
    pub x86: bool,
    pub voices: Engine,
    pub streams: Vec<Stream>,
    pub placements: Vec<Placement>,
    pub mixer: Mixer,
    pub master: Option<Master>,
    pub io: [f64; 64],
    pub cfg: Vec<f64>,
    pub commands: Vec<f64>,
    pub frame: u64,
    pub output: Vec<f32>,
    pub light_mode: bool,
    pub decoder_staging: Vec<f32>,
    voice_commands: Vec<f64>,
    light: Option<Light>,
    feeds: usize,
    input: Vec<f32>,
    present: Vec<u8>,
    mixed: Vec<f32>,
    lfe: Vec<f32>,
    direct: Vec<f32>,
    send: Vec<f32>,
    hrtf_database: Option<Database>,
    panners: Vec<Panner>,
    hrtf_sum: Vec<f32>,
    decoder: Vec<StereoConvolver>,
    pair: [[f32; FRAMES]; 2],
    scratch: [f32; FRAMES],
    gains: [f32; FRAMES],
    cutoff: Vec<f32>,
    bed_gains: Vec<[[f32; 2]; CHANNELS]>,
    test: Vec<f32>,
}
impl Output {
    #[allow(clippy::too_many_arguments)]
    pub fn new(rate: f64, channels: usize, enhanced: bool, headphones: bool, binaural: bool, mono: bool, x86: bool) -> Self {
        let light_mode = !enhanced && !headphones && channels == 2;
        let feeds = if binaural { 16 } else { channels };
        let mut voices = Engine::new(rate, VOICES, -3.0103, x86);
        voices.filtered = enhanced;
        voices.set_inputs(0);
        let master = if light_mode {
            None
        } else {
            let mut m = Master::new(rate, channels, -1.0, if enhanced { 0.08 } else { 0.06 }, 0.0015, x86);
            m.set_mono(mono);
            Some(m)
        };
        let hrtf_database = if headphones && !binaural { Some(Database::new(rate)) } else { None };
        let panners = if let Some(db) = &hrtf_database {
            (0..SLOTS).map(|_| Panner::new(rate, db.size)).collect()
        } else {
            Vec::new()
        };
        Self {
            rate,
            channels,
            enhanced,
            headphones,
            binaural,
            mono,
            x86,
            voices,
            streams: (0..STREAMS).map(|_| Stream::new(rate)).collect(),
            placements: (0..VOICES + STREAMS).map(|_| Placement::default()).collect(),
            mixer: Mixer::new(SLOTS, feeds, crate::mixer::PANPOT, None, &[]),
            master,
            io: [0.0; 64],
            cfg: Vec::new(),
            commands: Vec::new(),
            frame: 0,
            output: vec![0.0; channels * FRAMES],
            light_mode,
            decoder_staging: Vec::new(),
            voice_commands: Vec::new(),
            light: if light_mode { Some(Light::new()) } else { None },
            feeds,
            input: vec![0.0; SLOTS * FRAMES],
            present: vec![0; SLOTS],
            mixed: vec![0.0; feeds * FRAMES],
            lfe: vec![0.0; FRAMES],
            direct: vec![0.0; channels * FRAMES],
            send: vec![0.0; FRAMES],
            hrtf_database,
            panners,
            hrtf_sum: vec![0.0; 2 * FRAMES],
            decoder: Vec::new(),
            pair: [[0.0; FRAMES]; 2],
            scratch: [0.0; FRAMES],
            gains: [0.0; FRAMES],
            cutoff: vec![f32::NAN; STREAMS],
            bed_gains: vec![[[0.0; 2]; CHANNELS]; STREAMS],
            test: vec![0.0; channels * FRAMES],
        }
    }
    pub fn layout(&mut self, law: u32, decoder: Option<Decoder>, speakers: &[Speaker], center: i32, lfe: i32, original: f32) {
        self.mixer.set_layout(law, decoder, speakers);
        if let Some(master) = &mut self.master {
            let rooms = if lfe >= 0 {
                speakers
                    .iter()
                    .enumerate()
                    .fold(0, |a, (i, s)| if s.is_some() { a | (1 << i) } else { a })
            } else {
                u32::MAX
            };
            master.set_rooms(rooms);
            if self.enhanced {
                master.set_highpass(180.0, -3.0103);
                if lfe >= 0 {
                    master.set_lfe(lfe as usize, 120.0, -3.0103);
                }
            } else if center >= 0 {
                master.set_original(center as usize, original);
            }
        }
    }
    pub fn decoder(&mut self, filters: &[f32], frames: usize) {
        self.decoder.clear();
        for channel in 0..16 {
            let at = channel * 2 * frames;
            self.decoder
                .push(StereoConvolver::new(&filters[at..at + frames], &filters[at + frames..at + 2 * frames]));
        }
    }
    pub fn voice(&mut self, slot: usize, spatial: bool) {
        self.placements[slot] = Placement::default();
        self.placements[slot].active = true;
        self.placements[slot].spatial = spatial;
        self.mixer.start(slot);
        // A recycled source has a new native panner in the reference graph.
        if let Some(db) = &self.hrtf_database {
            self.panners[slot] = Panner::new(self.rate, db.size);
        }
    }
    pub fn stream(&mut self, slot: usize, channels: usize, kind: u32, spatial: bool) {
        let on = self.streams[slot].on;
        let gain = self.streams[slot].gain;
        let sources = self.streams[slot].sources;
        self.streams[slot] = Stream::new(self.rate);
        self.streams[slot].configure(channels, kind, spatial);
        if on {
            self.streams[slot].gain = gain;
            self.streams[slot].sources = sources;
        }
        self.streams[slot].filter = (0..channels)
            .map(|_| Biquad::new(Kind::Lowpass, (self.rate / 2.0) as f32, -3.0103, 0.0, self.rate))
            .collect();
        self.placements[VOICES + slot] = Placement::default();
        self.placements[VOICES + slot].active = true;
        self.placements[VOICES + slot].spatial = spatial;
        self.route_stream(slot);
    }
    fn route_stream(&mut self, slot: usize) {
        let channels = self.streams[slot].channels;
        let angles: &[f64] = match channels {
            1 => &[0.0],
            2 => &[30.0, -30.0],
            4 => &[30.0, -30.0, 150.0, -150.0],
            _ => &[],
        };
        for c in 0..channels.min(CHANNELS) {
            let id = VOICES + slot * CHANNELS + c;
            let mut angle = angles.get(c).copied().unwrap_or(0.0);
            if self.headphones && self.enhanced && channels == 4 {
                angle = [30.0, -30.0, 110.0, -110.0][c];
            }
            if self.light_mode && angle.abs() < 90.0 {
                angle = angle.signum() * (angle.abs() * 3.0).min(90.0);
            }
            let a = angle * PI / 180.0;
            let (x, z) = (-host::sin(a), -host::cos(a));
            let mut gains = alsoft(x, 0.0, z);
            for value in &mut gains {
                if value.abs() < 1e-6 {
                    *value = 0.0;
                } else if (*value - 1.0).abs() < 1e-6 {
                    *value = 1.0;
                }
            }
            self.bed_gains[slot][c] = [gains[0] as f32, gains[1] as f32];
            // Placed stream directions arrive in the page parameter batch.
            if !self.streams[slot].spatial {
                self.mixer
                    .take(&[id as f32, x as f32, 0.0, z as f32, if self.binaural { 0.2 } else { 0.0 }, 0.0], true);
            }
            self.mixer.start(id);
            if let Some(db) = &self.hrtf_database {
                self.panners[id] = Panner::new(self.rate, db.size);
            }
        }
    }
    /// Reattach live lanes after an output-layout change. The new
    /// placement DSP resets, while PCM cursors and gain automation survive.
    pub fn reattach(&mut self) {
        for slot in 0..VOICES {
            if self.placements[slot].active {
                self.mixer.start(slot);
            }
        }
        for slot in 0..STREAMS {
            if self.streams[slot].on {
                self.route_stream(slot);
            }
        }
    }
    pub fn volume(&mut self, value: f32, tau: f64, at: f64, set: bool) {
        if let Some(m) = &mut self.master {
            m.volume(value, tau, at, set, self.frame as f64);
        }
        if let Some(light) = &mut self.light {
            if set {
                light.volume.set_value(value);
            }
            light.volume.insert(!set, value, at, tau, self.frame as f64, self.rate);
        }
    }
    pub fn light_level(&mut self, db: f64) {
        if let Some(l) = &mut self.light {
            l.level = (host::pow(10.0, db / 20.0) / 2.0) as f32;
        }
    }
    /// One page frame of timed commands. Buffer growth occurs here, never
    /// in process(). Source commands retain the voice engine's wire format.
    pub fn apply(&mut self, len: usize) {
        let commands = std::mem::take(&mut self.commands);
        self.voice_commands.clear();
        let mut at = 0;
        while at < len.min(commands.len()) {
            let op = commands[at] as u32;
            let size = arity(op);
            if size == 0 || at + 1 + size > commands.len() {
                break;
            }
            let a = &commands[at + 1..at + 1 + size];
            at += 1 + size;
            let slot = a[0] as usize;
            if op <= PLACE {
                if slot < VOICES {
                    self.voice_commands.extend_from_slice(&commands[at - size - 1..at]);
                    if op == crate::voices::STOP {
                        self.placements[slot].active = false;
                    }
                }
                continue;
            }
            match op {
                POSITION => {
                    let &[_, axis, value, tau, time, set] = a else { unreachable!() };
                    if let Some(m) = self.placements.get_mut(slot) {
                        if let Some(axis) = m.axes.get_mut(axis as usize) {
                            if set != 0.0 {
                                axis.set_value(value as f32);
                            }
                            axis.insert(set == 0.0, value as f32, time, tau, self.frame as f64, self.rate);
                        }
                    }
                }
                PAN => {
                    let &[_, value, tau, time] = a else { unreachable!() };
                    if let Some(m) = self.placements.get_mut(slot) {
                        m.pan.insert(true, value as f32, time, tau, self.frame as f64, self.rate);
                    }
                }
                STREAM_PLACE => {
                    let &[_, x, y, z, level, time] = a else { unreachable!() };
                    if let Some(m) = self.placements.get_mut(VOICES + slot) {
                        m.light.place([x, y, z], level, time, self.frame as f64, self.rate);
                    }
                }
                STREAM_GAIN => {
                    let &[_, value, tau, time, set] = a else { unreachable!() };
                    if let Some(s) = self.streams.get_mut(slot) {
                        if set != 0.0 {
                            s.gain.set_value(value as f32);
                        }
                        if tau < 0.0 {
                            s.gain.cancel(time);
                        } else {
                            s.gain.insert(set == 0.0, value as f32, time, tau, self.frame as f64, self.rate);
                        }
                    }
                }
                STREAM_START => {
                    let &[_, part, sample_id, when, offset, pitch, looping] = a else {
                        unreachable!()
                    };
                    if let (Some(s), Some(Some(sample))) = (self.streams.get_mut(slot), self.voices.samples.get(sample_id as usize)) {
                        if let Some(source) = s.sources.get_mut(part as usize) {
                            source.start(sample_id as usize, sample, when, offset, pitch, looping != 0.0, self.rate);
                        }
                    }
                }
                STREAM_STOP => {
                    let &[_, part, time] = a else { unreachable!() };
                    if let Some(s) = self.streams.get_mut(slot) {
                        if let Some(source) = s.sources.get_mut(part as usize) {
                            source.stop(time, self.rate);
                        }
                    }
                }
                STREAM_RELEASE => {
                    if let Some(s) = self.streams.get_mut(slot) {
                        s.on = false;
                        self.placements[VOICES + slot].active = false;
                    }
                }
                STREAM_BIND => {
                    let &[_, channels, kind, spatial] = a else { unreachable!() };
                    if slot < STREAMS {
                        self.stream(slot, channels as usize, kind as u32, spatial != 0.0);
                    }
                }
                BIND => {
                    let &[_, spatial] = a else { unreachable!() };
                    if slot < VOICES {
                        self.voice(slot, spatial != 0.0);
                    }
                }
                STREAM_PARAM => {
                    let &[_, kind, value, tau, time] = a else { unreachable!() };
                    if let Some(s) = self.streams.get_mut(slot) {
                        let timeline = if kind == 0.0 { &mut s.cutoff } else { &mut s.send };
                        timeline.insert(true, value as f32, time, tau, self.frame as f64, self.rate);
                    }
                }
                _ => {}
            }
        }
        let n = self.voice_commands.len();
        self.voices.command_storage(n).copy_from_slice(&self.voice_commands);
        self.voices.apply_commands(n);
        self.commands = commands;
    }

    pub fn process(&mut self) -> Option<f64> {
        self.direct.fill(0.0);
        self.test.fill(0.0);
        self.send.fill(0.0);
        self.hrtf_sum.fill(0.0);
        self.present.fill(0);
        self.voices.frame = self.frame as f64;
        if self.light_mode {
            self.voices.render_light(&mut self.mixed[..2 * FRAMES]);
        } else {
            self.voices.render(&mut self.input[..VOICES * FRAMES], &mut self.present);
            self.send.copy_from_slice(&self.voices.send);
        }
        for slot in 0..STREAMS {
            if !self.streams[slot].on {
                continue;
            }
            self.streams[slot].process(&self.voices.samples, self.frame, self.rate, self.x86);
            if self.streams[slot].kind >= 3 {
                let channel = (self.streams[slot].kind - 3) as usize;
                if channel < self.channels {
                    for i in 0..FRAMES {
                        self.test[channel * FRAMES + i] += self.streams[slot].output[i];
                    }
                }
                continue;
            }
            if self.streams[slot].spatial {
                let stream = &mut self.streams[slot];
                stream.send.a_rate(self.frame, &mut self.gains, self.rate, self.x86);
                for i in 0..FRAMES {
                    self.send[i] += stream.output[i] * self.gains[i];
                }
                if self.enhanced {
                    let cutoff = stream.cutoff.k_rate(self.frame, self.rate);
                    if cutoff != self.cutoff[slot] {
                        for filter in &mut stream.filter {
                            filter.set(Kind::Lowpass, cutoff, -3.0103, 0.0, self.rate);
                        }
                        self.cutoff[slot] = cutoff;
                    }
                    stream.filter[0].process_in_place(&mut stream.output[..FRAMES]);
                }
                let id = VOICES + slot * CHANNELS;
                if self.light_mode {
                    let meta = &mut self.placements[VOICES + slot];
                    for c in 0..2 {
                        meta.light.sides[c].a_rate(self.frame, &mut self.gains, self.rate, self.x86);
                        for i in 0..FRAMES {
                            self.mixed[c * FRAMES + i] += stream.output[i] * self.gains[i];
                        }
                    }
                } else {
                    self.input[id * FRAMES..(id + 1) * FRAMES].copy_from_slice(&stream.output[..FRAMES]);
                    self.present[id] = 1;
                }
            } else {
                self.route_bed(slot);
            }
        }
        if self.light_mode {
            for i in 0..2 * FRAMES {
                self.mixed[i] += self.direct[i];
            }
            self.light.as_mut().unwrap().process(
                &mut self.mixed[..2 * FRAMES],
                &mut self.output,
                self.frame,
                self.rate,
                self.x86,
                self.mono,
            );
            for i in 0..self.output.len() {
                if self.test[i] != 0.0 {
                    self.output[i] += self.test[i];
                }
            }
            self.frame += FRAMES as u64;
            return None;
        }
        if self.headphones && !self.binaural {
            for slot in 0..VOICES {
                if !self.placements[slot].active {
                    continue;
                }
                let meta = &mut self.placements[slot];
                let src = &self.input[slot * FRAMES..(slot + 1) * FRAMES];
                if self.enhanced && !meta.spatial {
                    continue;
                }
                {
                    pan_position(
                        meta,
                        &self.hrtf_database,
                        &mut self.panners[slot],
                        src,
                        &mut self.pair,
                        self.frame,
                        self.rate,
                        self.x86,
                        &mut self.scratch,
                    );
                    for c in 0..2 {
                        for i in 0..FRAMES {
                            self.hrtf_sum[c * FRAMES + i] += self.pair[c][i];
                        }
                    }
                }
                self.present[slot] = 0;
            }
            for slot in 0..STREAMS {
                let stream = &self.streams[slot];
                if !stream.on || stream.kind >= 3 {
                    continue;
                }
                let angles: &[f64] = match stream.channels {
                    1 => &[0.0],
                    2 => &[30.0, -30.0],
                    4 => {
                        if self.enhanced {
                            &[30.0, -30.0, 110.0, -110.0]
                        } else {
                            &[30.0, -30.0, 150.0, -150.0]
                        }
                    }
                    _ => &[],
                };
                if !stream.spatial && ((self.enhanced && stream.channels != 4) || angles.is_empty()) {
                    continue;
                }
                for c in 0..if stream.spatial { 1 } else { stream.channels } {
                    let id = VOICES + slot * CHANNELS + c;
                    if stream.spatial {
                        pan_position(
                            &mut self.placements[VOICES + slot],
                            &self.hrtf_database,
                            &mut self.panners[id],
                            &stream.output[..FRAMES],
                            &mut self.pair,
                            self.frame,
                            self.rate,
                            self.x86,
                            &mut self.scratch,
                        );
                    } else {
                        let a = angles[c] * PI / 180.0;
                        let x = (-host::sin(a)) as f32 as f64;
                        let z = (-host::cos(a)) as f32 as f64;
                        let (azimuth, elevation) = crate::hrtf::angles([x as f32, 0.0, z as f32]);
                        self.panners[id].process(
                            self.hrtf_database.as_ref().unwrap(),
                            self.rate,
                            azimuth,
                            elevation,
                            &stream.output[c * FRAMES..(c + 1) * FRAMES],
                            &mut self.pair,
                        );
                    }
                    for ear in 0..2 {
                        for i in 0..FRAMES {
                            self.hrtf_sum[ear * FRAMES + i] += self.pair[ear][i];
                        }
                    }
                    self.present[id] = 0;
                }
            }
            let level = HRTF_LEVEL as f32;
            for i in 0..2 * FRAMES {
                self.direct[i] += self.hrtf_sum[i] * level;
            }
        }
        if self.binaural {
            for slot in 0..VOICES {
                if self.placements[slot].active && !self.placements[slot].spatial {
                    stereo_pan(
                        &mut self.placements[slot],
                        &self.input[slot * FRAMES..(slot + 1) * FRAMES],
                        &mut self.direct,
                        self.frame,
                        self.rate,
                        self.x86,
                        &mut self.scratch,
                    );
                    self.present[slot] = 0;
                }
            }
        }

        {
            let mut feeds = BlockMut {
                data: &mut self.mixed,
                channels: self.feeds,
                frames: FRAMES,
            };
            self.mixer.process(
                &self.input,
                &self.present,
                &mut feeds,
                if self.enhanced && self.channels > 2 { Some(&mut self.lfe) } else { None },
            );
        }
        if self.binaural {
            for c in 0..16 {
                self.decoder[c].process(&self.mixed[c * FRAMES..(c + 1) * FRAMES], &mut self.pair);
                for ear in 0..2 {
                    for i in 0..FRAMES {
                        self.direct[ear * FRAMES + i] += self.pair[ear][i];
                    }
                }
            }
            self.mixed[..self.channels * FRAMES].fill(0.0);
        }
        let result = self.master.as_mut().unwrap().process(
            &self.mixed[..self.channels * FRAMES],
            if self.enhanced && self.channels > 2 { Some(&self.lfe) } else { None },
            &self.send,
            None,
            Some(&self.direct),
            self.frame,
            &mut self.output,
        );
        for i in 0..self.output.len() {
            if self.test[i] != 0.0 {
                self.output[i] += self.test[i];
            }
        }
        self.frame += FRAMES as u64;
        result
    }
    fn route_bed(&mut self, slot: usize) {
        let stream = &mut self.streams[slot];
        let count = stream.channels;
        let n = self.channels;
        let spatial_bank = (!self.enhanced && !self.headphones && !self.light_mode && matches!(count, 1 | 2 | 4)) || (self.binaural && count == 4);
        if spatial_bank {
            for c in 0..count {
                let id = VOICES + slot * CHANNELS + c;
                self.input[id * FRAMES..(id + 1) * FRAMES].copy_from_slice(&stream.output[c * FRAMES..(c + 1) * FRAMES]);
                self.present[id] = 1;
            }
            return;
        }
        if self.headphones && !self.binaural && ((!self.enhanced && matches!(count, 1 | 2 | 4)) || (self.enhanced && count == 4)) {
            return;
        }
        if self.light_mode && matches!(count, 1 | 2 | 4) {
            for c in 0..count {
                for ear in 0..2 {
                    let gain = self.bed_gains[slot][c][ear];
                    for i in 0..FRAMES {
                        self.direct[ear * FRAMES + i] += stream.output[c * FRAMES + i] * gain;
                    }
                }
            }
            return;
        }
        let add = |target: &mut [f32], from: usize, to: usize, gain: f32| {
            for i in 0..FRAMES {
                target[to * FRAMES + i] += stream.output[from * FRAMES + i] * gain;
            }
        };
        if self.enhanced && !self.headphones && n == 2 && matches!(count, 1 | 2 | 4) {
            if count == 1 {
                add(&mut self.direct, 0, 0, std::f32::consts::FRAC_1_SQRT_2);
                add(&mut self.direct, 0, 1, std::f32::consts::FRAC_1_SQRT_2);
            } else {
                add(&mut self.direct, 0, 0, 1.0);
                add(&mut self.direct, 1, 1, 1.0);
                if count == 4 {
                    add(&mut self.direct, 2, 0, 0.7);
                    add(&mut self.direct, 3, 1, 0.7);
                }
            }
            return;
        }
        if count == n && n > 4 {
            for c in 0..n {
                add(&mut self.direct, c, c, 1.0);
            }
            return;
        }
        if count == 6 && n == 8 {
            for c in 0..4 {
                add(&mut self.direct, c, c, 1.0);
            }
            add(&mut self.direct, 4, 6, 0.87);
            add(&mut self.direct, 4, 4, 0.5);
            add(&mut self.direct, 5, 7, 0.87);
            add(&mut self.direct, 5, 5, 0.5);
            return;
        }
        if count == 1 {
            if stream.kind == 1 && self.enhanced && n > 2 {
                add(&mut self.direct, 0, 2, 1.0);
            } else {
                add(&mut self.direct, 0, 0, std::f32::consts::FRAC_1_SQRT_2);
                add(&mut self.direct, 0, 1, std::f32::consts::FRAC_1_SQRT_2);
            }
            return;
        }
        add(&mut self.direct, 0, 0, 1.0);
        add(&mut self.direct, 1, 1, 1.0);
        if count == 2 && !(stream.kind == 2 && self.enhanced && n > 2) {
            return;
        }
        let rear_l = 4;
        let rear_r = 5;
        let side_l = if n >= 8 { 6 } else { 4 };
        let side_r = if n >= 8 { 7 } else { 5 };
        if count == 2 {
            for i in 0..FRAMES {
                stream.side[i] = stream.output[i] - stream.output[FRAMES + i];
            }
            stream.upmix_filter.process_in_place(&mut stream.side);
            for (j, c, seconds, sign) in [
                (0, rear_l, 0.012, 1.0),
                (1, rear_r, 0.017, -1.0),
                (2, side_l, 0.008, 1.0),
                (3, side_r, 0.011, -1.0),
            ] {
                if j >= 2 && n < 8 {
                    continue;
                }
                let gain = (0.5 * sign * if n >= 8 && j < 2 { 0.7 } else { 1.0 }) as f32;
                let delay = seconds * self.rate;
                let len = stream.delays[j].len();
                for i in 0..FRAMES {
                    let at = stream.delay_at[j];
                    stream.delays[j][at] = stream.side[i];
                    let read = (at as f64 + len as f64 - delay).rem_euclid(len as f64);
                    let a = read as usize;
                    let f = (read - a as f64) as f32;
                    let sample = stream.delays[j][a] + f * (stream.delays[j][(a + 1) % len] - stream.delays[j][a]);
                    self.direct[c * FRAMES + i] += sample * gain;
                    stream.delay_at[j] = (at + 1) % len;
                }
            }
        } else if n == 2 {
            add(&mut self.direct, 2, 0, if self.enhanced { 0.7 } else { 1.0 });
            add(&mut self.direct, 3, 1, if self.enhanced { 0.7 } else { 1.0 });
        } else if n >= 8 {
            add(&mut self.direct, 2, side_l, 0.87);
            add(&mut self.direct, 2, rear_l, 0.5);
            add(&mut self.direct, 3, side_r, 0.87);
            add(&mut self.direct, 3, rear_r, 0.5);
        } else {
            add(&mut self.direct, 2, rear_l, 1.0);
            add(&mut self.direct, 3, rear_r, 1.0);
        }
    }
}

fn pan_position(
    meta: &mut Placement,
    database: &Option<Database>,
    panner: &mut Panner,
    input: &[f32],
    pair: &mut [[f32; FRAMES]; 2],
    block: u64,
    rate: f64,
    x86: bool,
    scratch: &mut [f32; FRAMES],
) {
    let mut position = [0.0; 3];
    for c in 0..3 {
        meta.axes[c].a_rate(block, scratch, rate, x86);
        position[c] = scratch[0] as f64;
    }
    let (azimuth, elevation) = crate::hrtf::angles(position.map(|v| v as f32));
    panner.process(database.as_ref().unwrap(), rate, azimuth, elevation, input, pair);
}

fn stereo_pan(meta: &mut Placement, input: &[f32], mix: &mut [f32], block: u64, rate: f64, x86: bool, scratch: &mut [f32; FRAMES]) {
    meta.pan.a_rate(block, scratch, rate, x86);
    for i in 0..FRAMES {
        let a = (scratch[i].clamp(-1.0, 1.0) as f64 + 1.0) * PI / 4.0;
        mix[i] += input[i] * host::cos(a) as f32;
        mix[FRAMES + i] += input[i] * host::sin(a) as f32;
    }
}
