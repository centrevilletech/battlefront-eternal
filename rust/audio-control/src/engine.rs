//! The sound engine: one AudioContext, the game's buses, and its voices,
//! following the 2004 game's sound library (docs/formats.md, "Sound"):
//!
//! - Voices: a pool of 64 virtual voices, of which 32 are heard. Every frame
//!   each voice scores (1 - Bias) x its level here + Bias x Priority, and the
//!   top 32 play; the rest keep time silently and come back where they would
//!   be. A sound asked for with the pool full gets in only by stealing the
//!   lowest-scoring voice, and only if (1 - Bias) x its bus gain + Bias x
//!   Priority beats that score.
//! - Out of earshot: a one-shot whose level where it is asked for is 0 (3D,
//!   at or past its MuteDistance) doesn't start at all (the game's entity
//!   sounds all ask it to), so a battle far off takes no voices and can't
//!   steal the ones near.
//! - Level: clamp(bus gain x (Gain +- GainDev), 0, 1) x the distance gain. A
//!   bus's gain is its Gain times its parents' (core.lvl: root -> ingame ->
//!   main -> soundfx and music; ambience and voiceover under ingame).
//! - Ducking: while anything plays on a bus with AttenuateBus (voiceover), the
//!   bus it names (main) ramps to 1 - Attenuation over AttenuationLeadIn, and
//!   back over AttenuationLeadOut once the last voice is done.
//! - Doppler (on unless ModeDoppler 0): the pitch times 1 + (the speed toward
//!   the listener) / 344.
//! - The listener: the camera, 2 m out in front of it for distance.
//!
//! The original mode adds what the Steam build's OpenAL output does: an extra
//! falloff past half of MaxDistance, pitch changes gliding 1/256 of the way
//! twice a frame, and 2D sounds always centred (its Pan never arrives).
//!
use crate::{cross, dot, length, normalize, sub, Point, Random, render};
use crate::layout::{bus as b, entry as en, sample as sa, sound as s};

pub const EFFECTS: usize = 0;
pub const WEAPONS: usize = 1;
pub const EXPLOSIONS: usize = 2;
pub const FOLEY: usize = 3;
pub const AMBIENCE: usize = 4;
pub const MUSIC: usize = 5;
pub const VOICE: usize = 6;
pub const INTERFACE: usize = 7;
pub const NONE: usize = usize::MAX;
pub const SILENT: u64 = u64::MAX;
pub const CAPACITY: usize = 512;

#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub render_id: usize,
    pub duration: f64,
    pub rate: f64,
    pub scale: f64,
    pub available: bool,
}
impl Sample {
    pub fn load(row: &[f64]) -> Self { Self { render_id: row[sa::RENDER_ID] as usize, duration: row[sa::DURATION], rate: row[sa::RATE], scale: row[sa::SCALE], available: row[sa::AVAILABLE] != 0.0 } }
}

#[derive(Clone, Copy)]
pub struct Curve { pub roll_in: f64, pub min: f64, pub mute: f64, pub max: f64, pub roll: f64 }
impl Curve {
    /// ports props.js::distanceGain. The mute test deliberately follows the
    /// minimum test: overlapping distances keep the original near plateau.
    pub fn gain(self, d: f64) -> f64 {
        if d < self.roll_in { return (d / (self.roll_in + 10.0 * self.roll * (self.roll_in - d))).max(0.0); }
        if d <= self.min { return 1.0; }
        if d >= self.mute { return 0.0; }
        ((self.max - d) / (self.max - self.min + 10.0 * self.roll * (d - self.min))).clamp(0.0, 1.0)
    }
    /// ports props.js::openAlFalloff, on top of the library's own curve.
    pub fn openal(self, d: f64) -> f64 { let half = 0.5 * self.max; if d > half && half > 0.0 { half / d } else { 1.0 } }
}

pub struct Entry { pub id: usize, pub weight: f64, pub interval: f64, pub dev: f64, pub next: f64 }
impl Entry {
    pub fn load(row: &[f64]) -> Self { Self { id: row[en::ID] as usize, weight: row[en::WEIGHT], interval: row[en::INTERVAL], dev: row[en::INTERVAL_DEV], next: f64::NEG_INFINITY } }
}

pub struct Sound {
    pub kind: usize, pub bus: usize,
    pub pitch: f64, pub pitch_dev: f64, pub gain: f64, pub gain_dev: f64,
    pub reverb: f64, pub looping: bool, pub pan: f64, pub mode3d: bool,
    pub doppler: bool, pub bias: f64, pub priority: f64, pub curve: Curve,
    pub probability: f64, pub interval: f64, pub interval_dev: f64,
    pub random_position: bool, pub cycle: i32, pub next: f64,
    pub stream: usize, pub entries: Vec<Entry>,
}
impl Sound {
    pub fn load(row: &[f64], entries: Vec<Entry>) -> Self {
        // ports props.js::prop defaults and clamps. NaN in the cold schema
        // means an absent property; live frame data never resolves defaults.
        let mut values = [0.0; s::STRIDE]; values.copy_from_slice(row);
        for key in [s::PITCH, s::GAIN, s::DOPPLER, s::PRIORITY, s::MUTE_DISTANCE, s::MAX_DISTANCE, s::PROBABILITY] { if values[key].is_nan() { values[key] = 1.0; } }
        if values[s::BIAS].is_nan() { values[s::BIAS] = 0.5; }
        for v in &mut values { if v.is_nan() { *v = 0.0; } }
        for key in [s::GAIN, s::GAIN_DEV, s::REVERB, s::PROBABILITY] { values[key] = values[key].clamp(0.0, 1.0); }
        values[s::PAN] = values[s::PAN].clamp(-1.0, 1.0); let row = &values;
        Self { kind: row[s::KIND] as usize, bus: crate::phase::index(row[s::BUS]),
            pitch: row[s::PITCH], pitch_dev: row[s::PITCH_DEV], gain: row[s::GAIN], gain_dev: row[s::GAIN_DEV],
            reverb: row[s::REVERB], looping: row[s::LOOP] > 0.0, pan: row[s::PAN], mode3d: row[s::MODE3D] != 0.0,
            doppler: row[s::DOPPLER] > 0.0, bias: row[s::BIAS], priority: row[s::PRIORITY],
            curve: Curve { roll_in: row[s::ROLL_IN_DISTANCE], min: row[s::MIN_DISTANCE], mute: row[s::MUTE_DISTANCE], max: row[s::MAX_DISTANCE], roll: row[s::ROLL_OFF] },
            probability: row[s::PROBABILITY], interval: row[s::INTERVAL], interval_dev: row[s::INTERVAL_DEV],
            random_position: row[s::RANDOM_POSITION] == 1.0, cycle: if row[s::CYCLE] > 0.0 { -1 } else { -2 }, next: f64::NEG_INFINITY,
            stream: crate::phase::index(row[s::STREAM]), entries }
    }
    /// ports props.js::gate. Even a failed probability roll consumes the
    /// interval, but an interval rejection does not draw randomness.
    pub fn gate(&mut self, now: f64, random: &mut Random) -> bool {
        if now < self.next { return false; }
        if self.interval != 0.0 || self.interval_dev != 0.0 { self.next = now + self.interval + self.interval_dev * random.spread(); }
        random.next() <= self.probability
    }
    /// ports props.js::pickSample. Availability is resolved at load so the
    /// retained list needs no filtering or temporary allocation on a play.
    pub fn pick(&mut self, now: f64, random: &mut Random) -> Option<usize> {
        let n = self.entries.len(); if n == 0 { return None; }
        let index;
        if self.cycle >= 0 { self.cycle = (self.cycle + 1) % n as i32; index = self.cycle as usize; }
        else {
            let total: f64 = self.entries.iter().map(|e| e.weight.max(0.0)).sum();
            let mut r = random.next() * if total == 0.0 { n as f64 } else { total };
            let mut chosen = n - 1;
            for (i, e) in self.entries.iter().enumerate() { r -= if total != 0.0 { e.weight.max(0.0) } else { 1.0 }; if r < 0.0 { chosen = i; break; } }
            index = chosen;
            if self.cycle != -2 { self.cycle = index as i32; }
        }
        let id = self.entries[index].id; let interval = self.entries[index].interval; let dev = self.entries[index].dev;
        if interval != 0.0 || dev != 0.0 {
            // The JS state map keys this gate by sample, including lists
            // that repeat an ID with different weights or intervals.
            let gate = self.entries.iter().position(|e| e.id == id).unwrap();
            if now < self.entries[gate].next { return None; }
            self.entries[gate].next = now + interval + dev * random.spread();
        }
        Some(id)
    }
}

pub struct Bus {
    gain: f64, parent: usize, pub duck: f64, pub fade_level: f64,
    fade_from: f64, fade_to: f64, fade_length: f64, fade_elapsed: f64,
    target: usize, amount: f64, lead_in: f64, lead_out: f64,
    busy: Option<bool>, level: f64, from: f64, to: f64, length: f64, elapsed: f64,
}
impl Bus {
    pub fn load(row: &[f64]) -> Self {
        Self { gain: row[b::GAIN], parent: crate::phase::index(row[b::PARENT]), duck: 1.0, fade_level: 1.0,
            fade_from: 1.0, fade_to: 1.0, fade_length: 0.0, fade_elapsed: 0.0,
            target: crate::phase::index(row[b::TARGET]), amount: row[b::AMOUNT], lead_in: row[b::LEAD_IN], lead_out: row[b::LEAD_OUT],
            busy: None, level: 1.0, from: 1.0, to: 1.0, length: 0.0, elapsed: 0.0 }
    }
    /// ports AudioEngine.fadeBus; replacing a fade begins at its current
    /// frame value rather than restarting from the bus's base gain.
    pub fn fade(&mut self, level: f64, seconds: f64) { self.fade_from = self.fade_level; self.fade_to = level; self.fade_length = seconds.max(0.001); self.fade_elapsed = 0.0; }
    pub fn resume_level(&self) -> f64 { if self.fade_length > 0.0 { self.fade_to } else { self.fade_level } }
}

#[derive(Clone, Copy)]
pub struct Listener { pub position: Point, pub ear: Point, pub forward: Point, pub up: Point, pub right: Point }
impl Default for Listener {
    fn default() -> Self { Self { position: [0.0; 3], ear: [0.0; 3], forward: [0.0, 0.0, -1.0], up: [0.0, 1.0, 0.0], right: [1.0, 0.0, 0.0] } }
}
impl Listener {
    /// ports AudioEngine.setListener: distance uses a point two metres
    /// ahead while direction is measured from the camera itself.
    pub fn set(&mut self, position: Point, forward: Point, up: Point) {
        self.position = position; self.forward = normalize(forward); self.right = normalize(cross(self.forward, up)); self.up = cross(self.right, self.forward);
        for (ear, (&pos, &fwd)) in self.ear.iter_mut().zip(position.iter().zip(&self.forward)) { *ear = pos + fwd * 2.0; }
    }
    pub fn place(&self, position: Point) -> (f64, f64, Point) {
        let distance = length(sub(position, self.ear)); let delta = sub(position, self.position); let span = length(delta);
        let inv = if span > 0.0001 { 1.0 / span } else { 0.0 };
        (distance, span, [dot(delta, self.right) * inv, dot(delta, self.up) * inv, -dot(delta, self.forward) * inv])
    }
}

#[derive(Clone, Copy)]
pub struct Target { value: f64, since: f64 }
impl Default for Target { fn default() -> Self { Self { value: f64::NAN, since: f64::NEG_INFINITY } } }
impl Target {
    /// ports automation.js::Target.set. An unchanged target keeps being
    /// sent until forty time constants have elapsed, then settles exactly.
    pub fn set(&mut self, value: f64, now: f64, tau: f64) -> bool {
        if !value.is_finite() { return false; }
        if value == self.value { if now - self.since >= 40.0 * tau { return false; } }
        else { self.value = value; self.since = now; }
        true
    }
    pub fn hold(&mut self, value: f64) { self.value = value; self.since = f64::NEG_INFINITY; }
}

#[derive(Clone, Copy)]
pub struct Ask {
    pub sound: usize, pub emitter: usize, pub position: Option<Point>, pub category: usize,
    pub looping: Option<bool>, pub gain: f64, pub pitch: f64, pub curve: Option<Curve>,
}
impl Ask {
    pub fn new(sound: usize, category: usize) -> Self { Self { sound, category, emitter: NONE, position: None, looping: None, gain: 1.0, pitch: 1.0, curve: None } }
}

#[derive(Clone, Copy)]
pub struct Voice {
    pub serial: u64, pub sound: usize, pub sample: usize, pub category: usize, pub bus: usize,
    pub sample_info: Sample,
    pub emitter: usize, pub position: Option<Point>, pub mode3d: bool, pub looping: bool,
    pub curve: Curve, pub gain: f64, pub pitch: f64, pub gain_scale: f64, pub pitch_scale: f64,
    pub elapsed: f64, pub begun: bool, pub glide: Option<f64>, pub done: bool,
    pub real: bool, pub slot: usize, pub distance: f64, pub span: f64, pub local: Point,
    pub radial: f64, pub last: Option<f64>, pub audible: f64, pub score: f64,
    pub occlusion_rays: [f64; 3], pub occlusion_aim: i32, pub occluded_target: f64, pub occluded: Option<f64>,
    pub level_target: Target, pub rate_target: Target, pub position_target: [Target; 3],
    pub pan_target: Target, pub send_target: Target, pub cutoff: f64, pub sent: [f32; 5],
    pub fresh: bool,
}
impl Default for Voice {
    fn default() -> Self {
        Self { serial: 0, sound: NONE, sample: NONE, category: EFFECTS, bus: NONE,
            sample_info: Sample::default(),
            emitter: NONE, position: None, mode3d: false, looping: false,
            curve: Curve { roll_in: 0.0, min: 0.0, mute: 1.0, max: 1.0, roll: 0.0 },
            gain: 1.0, pitch: 1.0, gain_scale: 1.0, pitch_scale: 1.0, elapsed: 0.0, begun: false,
            glide: None, done: true, real: false, slot: NONE, distance: 0.0, span: 0.0, local: [0.0, 0.0, -1.0],
            radial: 0.0, last: None, audible: 0.0, score: 0.0,
            occlusion_rays: [0.0; 3], occlusion_aim: -1, occluded_target: 0.0, occluded: None,
            level_target: Target::default(), rate_target: Target::default(), position_target: [Target::default(); 3],
            pan_target: Target::default(), send_target: Target::default(), cutoff: 20000.0, sent: [f32::NAN; 5], fresh: true }
    }
}

pub struct Engine {
    pub channels: usize, pub sounds: Vec<Sound>, pub samples: Vec<Sample>, pub buses: Vec<Bus>,
    pub enhanced_samples: Vec<Option<Sample>>,
    pub voices: Vec<Voice>, pub listener: Listener, pub time: f64, pub random: Random,
    pub enhanced: bool, pub running: bool, pub volumes: [f64; 8], pub max_voices: usize, pub virtual_voices: usize,
    pub stats: [u64; 6], pub commands: Vec<f64>, pub placements: Vec<f64>,
    pub now: f64, pub wall_now: f64, pub sample_rate: f64, pub light: bool, pub hrtf: bool, pub binaural: bool, pub lfe: bool,
    order: [usize; CAPACITY], order_len: usize, serial: u64,
    slots: [usize; 64], free_slots: usize, releases: [(usize, f64); 64], release_len: usize,
}
impl Engine {
    pub fn reserve_slot(&mut self) -> usize { assert!(self.free_slots > 0, "sound render capacity exhausted"); self.free_slots -= 1; self.slots[self.free_slots] }
    pub fn return_slot(&mut self, slot: usize) { self.slots[self.free_slots] = slot; self.free_slots += 1; }
    pub fn detach_emitter(&mut self, emitter: usize, position: Point) { for v in &mut self.voices { if v.emitter == emitter { v.emitter = NONE; v.position = Some(position); } } }
    pub fn next_serial(&mut self) -> u64 { self.serial += 1; self.serial }
    pub fn order_len(&self) -> usize { self.order_len }
    pub fn order_at(&self, at: usize) -> usize { self.order[at] }
    pub fn new(seed: u32) -> Self {
        Self { channels: 2, sounds: Vec::new(), samples: Vec::new(), buses: Vec::new(), voices: vec![Voice::default(); CAPACITY],
            enhanced_samples: Vec::new(),
            listener: Listener::default(), time: 0.0, random: Random(seed), enhanced: false, running: false,
            volumes: [1.0; 8], max_voices: 32, virtual_voices: 64, stats: [0; 6],
            commands: Vec::with_capacity(65536), placements: Vec::with_capacity(8192),
            now: 0.0, wall_now: 0.0, sample_rate: 48000.0, light: false, hrtf: false, binaural: false, lfe: false,
            order: [0; CAPACITY], order_len: 0, serial: 0,
            slots: std::array::from_fn(|i| 63 - i), free_slots: 64, releases: [(NONE, 0.0); 64], release_len: 0 }
    }
    pub fn bus_gain(&self, mut key: usize) -> f64 {
        let mut gain = 1.0;
        for _ in 0..16 {
            let Some(bus) = self.buses.get(key) else { break; };
            gain *= bus.gain * bus.duck * bus.fade_level; key = bus.parent;
        }
        gain
    }
    /// ports AudioEngine.play for plain effects. Gate and sample selection
    /// precede culling and stealing; rejected requests still consume them.
    pub fn play(&mut self, ask: Ask, followed: Option<Point>) -> u64 {
        let Some(sound) = self.sounds.get_mut(ask.sound) else { return SILENT; };
        let looping = ask.looping.unwrap_or(sound.looping);
        if !self.running && !looping { return SILENT; }
        if !sound.gate(self.time, &mut self.random) { return SILENT; }
        let Some(sample_id) = sound.pick(self.time, &mut self.random) else { return SILENT; };
        let curve = ask.curve.unwrap_or(sound.curve);
        let at = ask.position.or(followed);
        if !looping && sound.mode3d && at.is_some_and(|p| curve.gain(length(sub(p, self.listener.ear))) <= 0.0) { self.stats[5] += 1; return SILENT; }
        let bias = sound.bias; let priority = sound.priority; let bus = sound.bus;
        let mut pool = 0; let mut lowest = NONE;
        for i in 0..self.order_len {
            let id = self.order[i]; let v = &self.voices[id]; if v.done { continue; }
            pool += 1;
            if lowest == NONE || v.score < self.voices[lowest].score { lowest = id; }
        }
        if pool >= self.virtual_voices {
            let estimate = (1.0 - bias) * self.bus_gain(bus) + bias * priority;
            if lowest == NONE || estimate <= self.voices[lowest].score { self.stats[4] += 1; return SILENT; }
            self.stop_index(lowest, 0.03, self.now); self.stats[3] += 1;
        }
        self.compact();
        let Some(id) = self.voices.iter().position(|v| v.done && v.slot == NONE) else { return SILENT; };
        let sound = &self.sounds[ask.sound];
        let sample = if self.enhanced { self.enhanced_samples.get(sample_id).copied().flatten().unwrap_or(self.samples[sample_id]) } else { self.samples[sample_id] };
        let gain = (sound.gain + sound.gain_dev * self.random.spread()).clamp(0.0, 1.0) * ask.gain;
        let pitch = (sound.pitch + sound.pitch_dev * self.random.spread()).max(0.01) * ask.pitch;
        let elapsed = if sound.random_position { self.random.next() * sample.duration } else { 0.0 };
        self.serial += 1;
        let mut voice = Voice { serial: self.serial, sound: ask.sound, sample: sample_id, sample_info: sample, category: ask.category, bus,
            emitter: ask.emitter, position: ask.position, mode3d: sound.mode3d && at.is_some(), looping,
            curve, gain, pitch, elapsed, done: !sample.available, ..Voice::default() };
        for target in &mut voice.position_target { target.hold(0.0); }
        voice.pan_target.hold(0.0); voice.send_target.hold(0.0);
        self.voices[id] = voice;
        // A stop may have left an old occurrence in the ordered list. The
        // frame sweep removes it before a slot is reused, preserving ties.
        if self.order_len < CAPACITY { self.order[self.order_len] = id; self.order_len += 1; }
        self.stats[2] += 1; self.serial * CAPACITY as u64 + id as u64
    }
    /// ports AudioEngine.updateDucks. Streams supply busy flags from their
    /// retained state, so asynchronous loading does not duck prematurely.
    pub fn ducks(&mut self, dt: f64, streams_busy: &[bool]) {
        for bus in &mut self.buses {
            if bus.fade_length <= 0.0 { continue; }
            bus.fade_elapsed += dt;
            bus.fade_level = bus.fade_from + (bus.fade_to - bus.fade_from) * (bus.fade_elapsed / bus.fade_length).min(1.0);
            if bus.fade_elapsed >= bus.fade_length { bus.fade_length = 0.0; }
        }
        for i in 0..self.buses.len() {
            if self.buses[i].target == NONE { continue; }
            let busy = self.voices.iter().any(|v| !v.done && v.bus == i) || streams_busy.get(i).copied().unwrap_or(false);
            let bus = &mut self.buses[i];
            if bus.busy != Some(busy) {
                bus.busy = Some(busy); bus.from = bus.level; bus.to = if busy { 1.0 - bus.amount } else { 1.0 };
                bus.length = if busy { bus.lead_in } else { bus.lead_out }.max(0.001); bus.elapsed = 0.0;
            }
            if bus.level != bus.to { bus.elapsed += dt; bus.level = bus.from + (bus.to - bus.from) * (bus.elapsed / bus.length).min(1.0); }
            let target = bus.target; let level = bus.level;
            if let Some(target) = self.buses.get_mut(target) { target.duck = level; }
        }
    }
    /// ports Voice.measure. Its silent clock deliberately ignores Doppler
    /// and the Classic glide, just as the page's virtual clock did.
    pub fn measure(&mut self, id: usize, dt: f64, followed: Option<Point>, now: f64) {
        let bus_gain = self.bus_gain(self.voices[id].bus); let v = &mut self.voices[id]; if v.done { return; }
        if let Some(p) = followed { v.position = Some(p); }
        let mut gain = (v.gain * bus_gain).min(1.0) * v.gain_scale;
        if v.mode3d {
            if let Some(p) = v.position {
                let (d, span, local) = self.listener.place(p); v.distance = d; v.span = span; v.local = local;
                if let Some(last) = v.last { if dt > 0.0 { v.radial = (last - d) / dt; } }
                v.last = Some(d); gain *= v.curve.gain(d); if !self.enhanced { gain *= v.curve.openal(d); }
            }
        } else { v.distance = 0.0; v.span = 0.0; }
        v.audible = gain; let sound = &self.sounds[v.sound];
        v.score = if gain > 0.000001 { (1.0 - sound.bias) * gain + sound.bias * sound.priority } else { 0.0 };
        if v.begun { v.elapsed += dt * v.pitch * v.pitch_scale; } v.begun = true;
        if !v.looping && v.elapsed >= v.sample_info.duration { self.stop_index(id, 0.005, now); }
    }
    /// ports AudioEngine.update's stable descending score sort. Fixed-size
    /// insertion sort avoids the allocator used by general stable sorting.
    pub fn rank(&mut self, ranked: &mut [usize; CAPACITY]) -> usize {
        let mut count = 0;
        for k in 0..self.order_len {
            let id = self.order[k]; if self.voices[id].done { continue; }
            let mut at = count;
            while at > 0 && self.voices[id].score > self.voices[ranked[at - 1]].score { ranked[at] = ranked[at - 1]; at -= 1; }
            ranked[at] = id; count += 1;
        }
        self.stats[0] = ranked[..count].iter().take(self.max_voices).filter(|&&id| self.voices[id].score > 0.0).count() as u64;
        self.stats[1] = count as u64 - self.stats[0]; count
    }
    pub fn compact(&mut self) {
        let mut kept = 0;
        for i in 0..self.order_len { let id = self.order[i]; if !self.voices[id].done { self.order[kept] = id; kept += 1; } }
        self.order_len = kept;
    }
    /// ports Voice.rate. Classic calls this both at start and apply, so a
    /// newly heard voice performs the second glide in its first frame too.
    pub fn rate(&mut self, id: usize) -> f64 {
        let v = &mut self.voices[id]; let sound = &self.sounds[v.sound]; let sample = v.sample_info;
        let mut rate = v.pitch * v.pitch_scale;
        if sound.doppler && v.mode3d && v.radial.abs() < 172.0 { rate *= 1.0 + v.radial / 344.0; }
        let hz = sample.rate * sample.scale; rate = (rate * hz).clamp(100.0, 200000.0) / hz;
        if !self.enhanced { let glide = v.glide.get_or_insert(rate); *glide += (rate - *glide) * (1.0 - (255.0_f64 / 256.0).powi(2)); rate = *glide; }
        rate *= sample.scale; if rate.is_finite() && rate > 0.0 { rate } else { 1.0 }
    }
    fn stop_index(&mut self, id: usize, fade: f64, now: f64) {
        if id == NONE || self.voices[id].done { return; }
        self.release(id, fade, now); self.voices[id].real = false; self.voices[id].done = true;
    }
    fn release(&mut self, id: usize, fade: f64, now: f64) {
        let v = &mut self.voices[id]; if v.slot == NONE { return; }
        let slot = v.slot; v.slot = NONE;
        if self.light {
            for side in 0..2 {
                self.commands.extend_from_slice(&[render::CANCEL as f64, slot as f64, side as f64, now]);
                self.commands.extend_from_slice(&[render::SIDE as f64, slot as f64, side as f64, 0.0, fade.max(0.001) / 3.0, now]);
            }
            self.commands.extend_from_slice(&[render::STOP_AT as f64, slot as f64, now + (fade + 0.01)]);
        } else { self.commands.extend_from_slice(&[render::RELEASE as f64, slot as f64, fade.max(0.001) / 3.0, fade + 0.01, now]); }
        if self.release_len < self.releases.len() { self.releases[self.release_len] = (slot, self.wall_now + ((fade + 0.1) * 1000.0) / 1000.0); self.release_len += 1; }
    }
    /// ports Voice.setReal and start. The render slot and the virtual voice
    /// have different lifetimes: release tails retain their slot briefly.
    pub fn set_real(&mut self, id: usize, want: bool) {
        let v = &self.voices[id]; if v.done || want == v.real { return; }
        if !want {
            self.release(id, 0.03, self.now); self.voices[id].real = false; self.stats[3] += 1; return;
        }
        // Release tails still own their slots when a slow or suspended device
        // has not reached their deadlines. Keep this voice virtual until a
        // later frame reclaims a slot; audio pressure must not stop the game.
        if self.free_slots == 0 {
            return;
        }
        self.free_slots -= 1; let slot = self.slots[self.free_slots];
        let v = &mut self.voices[id]; let sample = v.sample_info;
        let offset = if v.looping { v.elapsed % sample.duration } else { v.elapsed } * sample.scale;
        if !v.looping && offset >= sample.duration * sample.scale {
            self.slots[self.free_slots] = slot; self.free_slots += 1; self.stop_index(id, 0.005, self.now); return;
        }
        v.slot = slot; v.fresh = true;
        v.level_target = Target::default(); v.rate_target = Target::default(); v.position_target = [Target::default(); 3];
        for target in &mut v.position_target { target.hold(0.0); }
        v.pan_target.hold(0.0); v.send_target.hold(0.0); v.cutoff = 20000.0; v.sent = [f32::NAN; 5];
        self.commands.extend_from_slice(&[render::BIND as f64, slot as f64, v.mode3d as u8 as f64]);
        let looping = v.looping;
        let rate = self.rate(id);
        self.commands.extend_from_slice(&[render::START as f64, slot as f64, sample.render_id as f64, offset.max(0.0), looping as u8 as f64, rate, 20000.0_f64.min(self.sample_rate / 2.0), self.now]);
        if self.light { for side in 0..2 { self.commands.extend_from_slice(&[render::SIDE_SET as f64, slot as f64, side as f64, 0.0, self.now]); } }
        self.voices[id].rate_target.hold(rate); self.voices[id].real = true;
    }
    /// ports VoiceOut.dispose's wall timer, which runs with a suspended
    /// device. STOP precedes a later BIND even after a long hidden-tab gap.
    pub fn reclaim(&mut self) {
        let mut kept = 0;
        for i in 0..self.release_len {
            let (slot, at) = self.releases[i];
            if self.wall_now >= at {
                self.commands.extend_from_slice(&[render::STOP as f64, slot as f64]);
                self.slots[self.free_slots] = slot; self.free_slots += 1;
            } else { self.releases[kept] = (slot, at); kept += 1; }
        }
        self.release_len = kept;
    }
    /// ports Voice.apply and VoiceOut.update. A shape is computed once by
    /// the control phase; the host forwards the resulting numbers verbatim.
    pub fn apply(&mut self, id: usize, shape: Shape) {
        if self.voices[id].done || !self.voices[id].real { return; }
        let rate = self.rate(id); let v = &mut self.voices[id]; let sound = &self.sounds[v.sound]; let slot = v.slot as f64;
        let raw = v.audible * self.volumes[v.category] * shape.gain; let level = if raw.is_finite() { raw } else { 0.0 };
        if !self.light {
            if v.fresh { self.commands.extend_from_slice(&[render::GAIN_SET as f64, slot, level, self.now]); v.level_target.hold(level); v.fresh = false; }
            else if v.level_target.set(level, self.now, 0.015) { self.commands.extend_from_slice(&[render::GAIN as f64, slot, level, 0.015, self.now]); }
        }
        if v.rate_target.set(rate, self.now, 0.015) { self.commands.extend_from_slice(&[render::RATE as f64, slot, rate, 0.015, self.now]); }
        let pan = if self.enhanced { sound.pan } else { 0.0 };
        let direction = if v.mode3d { v.local } else { let a = pan.clamp(-1.0, 1.0) * std::f64::consts::FRAC_PI_2; [v8math::sin(a), 0.0, -v8math::cos(a)] };
        if self.light { self.commands.extend_from_slice(&[render::PLACE as f64, slot, direction[0], direction[1], direction[2], level, self.now]); return; }
        if self.hrtf && (v.mode3d || !self.enhanced) {
            let distance = v.span.max(1.0);
            for (axis, (&dir, target)) in direction.iter().zip(&mut v.position_target).enumerate() { let value = dir * distance; if target.set(value, self.now, 0.01) { self.commands.extend_from_slice(&[render::POSITION as f64, slot, axis as f64, value, 0.01, self.now, 0.0]); } }
        } else if self.binaural && !v.mode3d {
            let pan = direction[0].clamp(-1.0, 1.0);
            if v.pan_target.set(pan, self.now, 0.015) { self.commands.extend_from_slice(&[render::PAN as f64, slot, pan, 0.015, self.now]); }
        } else {
            let spread = if v.mode3d && self.enhanced { shape.spread.max((1.0 - v.span / 1.5).clamp(0.0, 1.0) * 0.6) } else { shape.spread };
            let lfe = if self.enhanced && self.lfe { shape.lfe * 0.5 } else { 0.0 };
            let sent = [direction[0] as f32, direction[1] as f32, direction[2] as f32, spread as f32, lfe as f32];
            if v.sent != sent { v.sent = sent; self.placements.extend_from_slice(&[slot, direction[0], direction[1], direction[2], spread, lfe]); }
        }
        if self.enhanced && (shape.cutoff - v.cutoff).abs() > 10.0 { v.cutoff = shape.cutoff; self.commands.extend_from_slice(&[render::CUTOFF as f64, slot, v.cutoff.min(self.sample_rate / 2.0), 0.03, self.now]); }
        let send = shape.send * if self.enhanced { sound.reverb } else { 1.0 };
        if v.send_target.set(send, self.now, 0.05) { self.commands.extend_from_slice(&[render::SEND as f64, slot, send, 0.05, self.now]); }
    }
    pub(crate) fn resolve(&self, handle: u64) -> Option<usize> {
        if handle == SILENT { return None; }
        let id = (handle % CAPACITY as u64) as usize;
        (self.voices[id].serial == handle / CAPACITY as u64).then_some(id)
    }
    pub fn stop(&mut self, handle: u64, fade: f64) { if let Some(id) = self.resolve(handle) { self.stop_index(id, fade, self.now); } }
    pub fn gain(&mut self, handle: u64, value: f64) { if value.is_finite() { if let Some(id) = self.resolve(handle) { self.voices[id].gain_scale = value; } } }
    pub fn pitch(&mut self, handle: u64, value: f64) { if value.is_finite() { if let Some(id) = self.resolve(handle) { self.voices[id].pitch_scale = value; } } }
    pub fn absolute_pitch(&mut self, handle: u64, value: f64) { if value.is_finite() { if let Some(id) = self.resolve(handle) { self.voices[id].pitch = value; self.voices[id].pitch_scale = 1.0; } } }
    pub fn playing(&self, handle: u64) -> bool { self.resolve(handle).is_some_and(|id| !self.voices[id].done) }
    pub fn reach(&self, handle: u64) -> f64 { self.resolve(handle).map_or(0.0, |id| self.voices[id].curve.mute) }
}

#[derive(Clone, Copy)]
pub struct Shape { pub cutoff: f64, pub gain: f64, pub send: f64, pub lfe: f64, pub spread: f64 }
impl Default for Shape { fn default() -> Self { Self { cutoff: 20000.0, gain: 1.0, send: 0.0, lfe: 0.0, spread: 0.0 } } }
