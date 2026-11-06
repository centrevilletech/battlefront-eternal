//! Streamed sound: music, voice-over, and ambience. Each OpenAudioStream call
//! in the mission opens one slot on a stream file (Theed opens nab2 twice),
//! and a slot plays one segment at a time. A SoundStreamProperties (or
//! SoundStream) object names its Stream and a SegmentList to pick from: by
//! weight, or in turn with CyclePlayback, each segment gated by its own
//! interval (Segment(id, weight, interval, intervalDev)). A looping one picks
//! again when a segment ends and carries straight on.
//!
//! Voice-over and the soldiers' chatter are queued, as the 2004 game queues
//! them: each stream's slot keeps up to four lines, best score first (the
//! voice score, (1 - Bias) x level + Bias x Priority, the level with its bus
//! and, for a line said somewhere, its distance; ties behind earlier ones),
//! worked out again for every line queued whenever one is added, and a slot
//! that falls idle plays the first.
//! So lines on one stream never overlap or cut each other off. A fifth line
//! drops the last; one said somewhere that would itself be last isn't
//! queued, while a 2D one drops the line ahead of it instead. A line is
//! checked as it is asked for: its segment is picked (with each segment's
//! own interval), then its PlayInterval and PlayProbability. A line said out
//! of earshot still scores Bias x Priority: it is queued, and holds the
//! slot unheard.
//!
//! A soldier's line (a mono Mode3D stream, at the place he said it, which it
//! keeps) plays from there through one of the output's 3D voices, as his
//! other sounds do; every other stream is a bed (music, ambience, the
//! commander's voice-over).
//!
//! Segments are fetched on demand (by byte range when the file is large) and
//! decoded off the main thread; only recently used ones stay decoded.
//!
//! Stream selection and scheduling, porting streams.js. Byte-range reads and
//! decoder transport remain host I/O; only readiness facts return here.
use crate::engine::{Ask, Engine, Target, Voice, NONE, SILENT, VOICE, MUSIC};
use crate::{Point, length, sub, render};
use crate::acoustics::Acoustics;

pub const STREAM_VOICES: usize = 32;
#[derive(Clone, Copy)]
pub struct Stream { pub slots: usize, pub channels: usize }
#[derive(Clone, Copy)]
pub struct Segment { pub stream: usize, pub duration: f64, pub channels: usize, pub variants: [usize; 4] }
#[derive(Clone, Copy)]
pub struct Options { pub ask: Ask, pub spatial: bool, pub segment: usize, pub fade: f64 }
impl Options { pub fn new(sound: usize, category: usize) -> Self { Self { ask: Ask::new(sound, category), spatial: false, segment: NONE, fade: 0.0 } } }
#[derive(Clone, Copy)]
pub struct Part { pub part: usize, pub ends: f64 }
#[derive(Clone, Copy)]
pub struct Request { pub voice: usize, pub generation: u64, pub segment: usize, pub part: usize, pub sample: usize, pub operation: usize, pub variant: usize }
#[derive(Clone, Copy)]
struct Playing {
    generation: u64, active: bool, release_at: f64, sound: usize, stream: usize, category: usize,
    spatial: bool, position: Option<Point>, gain: f64, pitch: f64, looping: bool, sounding: bool,
    pending: bool, current: Option<Part>, queued: Option<Part>,
    parts: [f64; 3], channels: usize, bound: bool, level: f64, target: Option<f64>, tau: f64, until: f64,
    gain_target: Target, placement: Voice,
}
impl Default for Playing {
    fn default() -> Self { Self { generation: 0, active: false, release_at: 0.0, sound: NONE, stream: NONE, category: MUSIC,
        spatial: false, position: None, gain: 1.0, pitch: 1.0, looping: false, sounding: false,
        pending: false, current: None, queued: None, parts: [0.0; 3], channels: 1, bound: false,
        level: 1.0, target: None, tau: 0.02, until: 0.0, gain_target: Target::default(), placement: Voice::default() } }
}
#[derive(Clone, Copy)]
struct Queued { options: Options, score: f64 }
pub struct Streams {
    pub definitions: Vec<Stream>, pub segments: Vec<Segment>, pub requests: Vec<Request>,
    reserved: [bool; STREAM_VOICES], voices: Vec<Playing>, queues: Vec<[Option<Queued>; 4]>, serial: u64, pub sample_base: usize, pub cache_seconds: f64, cache_order: Vec<usize>,
}
impl Default for Streams {
    fn default() -> Self { Self { reserved: [false; STREAM_VOICES], definitions: Vec::new(), segments: Vec::new(), requests: Vec::with_capacity(128), voices: vec![Playing::default(); STREAM_VOICES], queues: Vec::new(), serial: 0, sample_base: 0, cache_seconds: 600.0, cache_order: Vec::new() } }
}
impl Streams {
    pub fn invalidate_placement(&mut self) { for v in &mut self.voices { v.placement.sent = [f32::NAN;5]; } }
    pub fn reserve_slot(&mut self) -> usize { let id = self.voices.iter().enumerate().position(|(id,v)| !self.reserved[id] && !v.active && v.release_at == 0.0).expect("sound stream capacity exhausted"); self.reserved[id] = true; id }
    pub fn return_slot(&mut self, id: usize) { assert!(self.reserved[id]); self.reserved[id] = false; }
    pub fn configure(&mut self) { self.cache_order.reserve(self.segments.len()); self.queues.resize(self.definitions.len(), [None; 4]); }
    fn id(&self, handle: u64) -> Option<usize> { let id = (handle % STREAM_VOICES as u64) as usize; (handle != SILENT && self.voices[id].generation == handle / STREAM_VOICES as u64).then_some(id) }
    pub fn playing(&self, handle: u64) -> bool { self.id(handle).is_some_and(|id| self.voices[id].active) }
    pub fn busy(&self, bus: usize, e: &Engine) -> bool { self.voices.iter().any(|v| v.active && v.sounding && e.sounds[v.sound].bus == bus) }
    pub fn free(&self, stream: usize) -> bool { self.definitions.get(stream).is_some_and(|s| self.voices.iter().filter(|v| v.active && v.stream == stream).count() < s.slots) }
    /// ports Streams.score: a far line still has its bias/priority score and
    /// therefore holds an unheard stream slot instead of being culled.
    fn score(&self, e: &Engine, sound: usize, position: Option<Point>) -> f64 {
        let s = &e.sounds[sound]; let mut level = (s.gain * e.bus_gain(s.bus)).min(1.0);
        if let Some(p) = position { level *= s.curve.gain(length(sub(p, e.listener.ear))); }
        (1.0 - s.bias) * level + s.bias * s.priority
    }
    fn ask(&mut self, e: &mut Engine, mut options: Options) -> Option<(Options, f64)> {
        let sound = e.sounds.get_mut(options.ask.sound)?; if sound.kind != 4 || sound.stream == NONE { return None; }
        options.segment = sound.pick(e.time, &mut e.random)?;
        if !sound.gate(e.time, &mut e.random) { return None; }
        let position = if sound.mode3d { options.ask.position } else { None };
        let score = self.score(e, options.ask.sound, position); (score > 0.0).then_some((options, score))
    }
    /// ports Streams.queue, including stable equal-score order and the 2D
    /// fifth line replacing the preceding line when it would rank last.
    pub fn queue(&mut self, e: &mut Engine, options: Options) -> bool {
        let Some((options, score)) = self.ask(e, options) else { return false; };
        let stream = e.sounds[options.ask.sound].stream; let old = self.queues[stream]; let mut list = [None; 5]; let mut n = 0;
        for request in old.into_iter().flatten() {
            let s = &e.sounds[request.options.ask.sound]; let p = if s.mode3d { request.options.ask.position } else { None };
            let score = self.score(e, request.options.ask.sound, p); if score > 0.0 { list[n] = Some(Queued { score, ..request }); n += 1; }
        }
        let at = (0..n).find(|&i| list[i].unwrap().score < score).unwrap_or(n);
        for i in (at..n).rev() { list[i + 1] = list[i]; } list[at] = Some(Queued { options, score }); n += 1;
        let mut accepted = true;
        if n > 4 && at == 4 {
            if options.ask.position.is_some() && e.sounds[options.ask.sound].mode3d { accepted = false; }
            else { list[3] = list[4]; }
        }
        self.queues[stream].copy_from_slice(&list[..4]); accepted
    }
    pub fn say(&mut self, e: &mut Engine, options: Options) -> u64 {
        let Some((mut options, _)) = self.ask(e, options) else { return SILENT; };
        let stream = e.sounds[options.ask.sound].stream;
        if !self.free(stream) { for id in 0..STREAM_VOICES { if self.voices[id].active && self.voices[id].stream == stream { self.stop_id(e, id, 0.05); } } }
        options.ask.position = None; self.play(e, options)
    }
    /// ports Streams.play and StreamVoice.constructor. The first segment is
    /// picked before the gain/pitch dice; ordinary play does not run gate().
    pub fn play(&mut self, e: &mut Engine, options: Options) -> u64 {
        let Some(sound) = e.sounds.get(options.ask.sound) else { return SILENT; };
        let stream = sound.stream; if sound.kind != 4 || !self.free(stream) { return SILENT; }
        let first = if options.segment != NONE { options.segment } else { let Some(id) = e.sounds[options.ask.sound].pick(e.time, &mut e.random) else { return SILENT; }; id };
        let Some(id) = self.voices.iter().enumerate().position(|(id,v)| !self.reserved[id] && !v.active && v.release_at == 0.0) else { panic!("sound stream capacity exhausted"); };
        let sound = &e.sounds[options.ask.sound]; let definition = self.definitions[stream]; self.serial += 1;
        let gain = (sound.gain + sound.gain_dev * e.random.spread()).clamp(0.0, 1.0) * options.ask.gain;
        let pitch = (sound.pitch + sound.pitch_dev * e.random.spread()).max(0.01);
        let spatial = options.spatial && options.ask.position.is_some() && sound.mode3d && definition.channels == 1;
        let mut placement = Voice { mode3d: spatial, category: options.ask.category, position: options.ask.position, curve: options.ask.curve.unwrap_or(sound.curve), ..Voice::default() };
        for target in &mut placement.position_target { target.hold(0.0); } placement.send_target.hold(0.0); placement.pan_target.hold(0.0);
        self.voices[id] = Playing { generation: self.serial, active: true, sound: options.ask.sound, stream, category: options.ask.category, spatial,
            position: options.ask.position, gain, pitch, looping: options.ask.looping.unwrap_or(sound.looping), channels: definition.channels, placement, ..Playing::default() };
        let kind = match options.ask.category { VOICE => 1.0, MUSIC => 2.0, _ => 0.0 };
        e.commands.extend_from_slice(&[render::STREAM_BIND as f64, id as f64, definition.channels as f64, kind, spatial as u8 as f64]);
        e.commands.extend_from_slice(&[render::STREAM_GAIN as f64, id as f64, 0.0, 0.0, e.now, 1.0]);
        self.fade_id(e, id, 1.0, options.fade); self.request(e, id, first);
        self.serial * STREAM_VOICES as u64 + id as u64
    }
    fn level(&self, e: &Engine, id: usize) -> f64 { let v = &self.voices[id]; v.gain * e.bus_gain(e.sounds[v.sound].bus) * e.volumes[v.category] }
    fn request(&mut self, e: &mut Engine, id: usize, segment: usize) {
        if segment == NONE { self.finish(e, id); return; }
        let v = &mut self.voices[id]; let part = (0..3).find(|&part| v.parts[part] <= e.now).expect("stream segment capacity exhausted");
        v.parts[part] = f64::INFINITY; v.pending = true;
        assert!(self.requests.len() < self.requests.capacity(), "stream request capacity exhausted");
        let generation = v.generation;
        let variants = self.segments[segment].variants;
        let variant = if !e.enhanced { NONE } else if variants[0] == NONE { variants[3] } else if e.channels >= 8 { [variants[2], variants[1], variants[0]].into_iter().find(|v| *v != NONE).unwrap() } else if e.channels >= 6 { if variants[1] != NONE { variants[1] } else { variants[0] } } else { variants[0] };
        self.requests.push(Request { voice: id, generation, segment, part, sample: self.sample_base + id * 3 + part, operation: crate::layout::request_kind::LOAD, variant });
        if variant == NONE { self.cache_touch(segment); }
    }
    /// ports Streams.trim: retain the two newest decoded originals even
    /// when they exceed the device budget. Byte and PCM ownership stays I/O.
    fn cache_touch(&mut self, segment: usize) {
        if let Some(at) = self.cache_order.iter().position(|id| *id == segment) { self.cache_order.remove(at); }
        assert!(self.cache_order.len() < self.cache_order.capacity()); self.cache_order.insert(0,segment);
        let mut seconds = 0.0; let mut at = 0;
        while at < self.cache_order.len() {
            let id = self.cache_order[at]; seconds += self.segments[id].duration;
            if seconds > self.cache_seconds && at >= 2 {
                assert!(self.requests.len() < self.requests.capacity()); self.requests.push(Request { voice: NONE, generation: 0, segment: id, part: 0, sample: 0, operation: crate::layout::request_kind::EVICT, variant: NONE }); self.cache_order.remove(at);
            } else { at += 1; }
        }
    }
    /// ports StreamVoice.next's asynchronous continuation. This is an asset
    /// readiness boundary, never another gameplay or per-emitter update.
    pub fn ready(&mut self, e: &mut Engine, id: usize, generation: u64, part: usize, sample: usize, duration: f64, channels: usize) -> bool {
        let v = &mut self.voices[id]; if !v.active || v.generation != generation { return false; }
        if !v.bound { v.bound = true; if v.channels != channels { v.channels = channels; let kind = match v.category { VOICE => 1.0, MUSIC => 2.0, _ => 0.0 }; e.commands.extend_from_slice(&[render::STREAM_BIND as f64, id as f64, channels as f64, kind, v.spatial as u8 as f64]); } }
        let when = v.current.map_or(e.now, |p| p.ends.max(e.now)); let ends = when + duration / v.pitch;
        let playing = Part { part, ends }; if v.current.is_some() { v.queued = Some(playing); } else { v.current = Some(playing); }
        v.parts[part] = ends; v.pending = false; v.sounding = true;
        e.commands.extend_from_slice(&[render::STREAM_START as f64, id as f64, part as f64, sample as f64, when, 0.0, v.pitch, 0.0]); true
    }
    fn fade_id(&mut self, e: &mut Engine, id: usize, level: f64, seconds: f64) {
        let base = self.voices[id].target.unwrap_or_else(|| self.level(e, id)); let v = &mut self.voices[id];
        v.level = level; v.tau = if seconds > 0.0 { seconds / 3.0 } else { 0.02 }; v.until = e.now + seconds.max(0.0); v.gain_target = Target::default();
        e.commands.extend_from_slice(&[render::STREAM_GAIN as f64, id as f64, 0.0, -1.0, e.now, 0.0]);
        e.commands.extend_from_slice(&[render::STREAM_GAIN as f64, id as f64, base * level, if seconds > 0.0 { v.tau } else { 0.0 }, e.now, if seconds > 0.0 { 0.0 } else { 1.0 }]);
    }
    fn finish(&mut self, e: &Engine, id: usize) { let v = &mut self.voices[id]; v.active = false; v.sounding = false; v.release_at = e.wall_now + 3.0; }
    fn stop_id(&mut self, e: &mut Engine, id: usize, fade: f64) {
        if !self.voices[id].active { return; } self.fade_id(e, id, 0.0, fade);
        let v = &self.voices[id]; for p in [v.current, v.queued].into_iter().flatten() { e.commands.extend_from_slice(&[render::STREAM_STOP as f64, id as f64, p.part as f64, e.now + 2.0 * fade + 0.05]); }
        self.finish(e, id);
    }
    pub fn stop(&mut self, e: &mut Engine, handle: u64, fade: f64) { if let Some(id) = self.id(handle) { self.stop_id(e, id, fade); } }
    pub fn failed(&mut self, e: &Engine, id: usize, generation: u64) -> bool { if id >= self.voices.len() || !self.voices[id].active || self.voices[id].generation != generation { return false; } self.finish(e, id); true }
    pub fn stop_all(&mut self, e: &mut Engine, fade: f64) { for id in 0..STREAM_VOICES { self.stop_id(e, id, fade); } for q in &mut self.queues { *q = [None; 4]; } }
    /// ports StreamVoice.update, including its five-millisecond queued handoff
    /// and eight-second lookahead. No segment choice or timing remains in JS.
    pub fn update(&mut self, e: &mut Engine, acoustics: &Acoustics, space_send: f64) {
        for id in 0..STREAM_VOICES {
            if !self.voices[id].active {
                if self.voices[id].release_at > 0.0 && e.wall_now >= self.voices[id].release_at {
                    e.commands.extend_from_slice(&[render::STREAM_RELEASE as f64, id as f64]);
                    assert!(self.requests.len() < self.requests.capacity());
                    self.requests.push(Request { voice: id, generation: self.voices[id].generation, segment: NONE, part: 0, sample: self.sample_base + id * 3, operation: crate::layout::request_kind::RELEASE, variant: NONE }); self.voices[id].release_at = 0.0;
                } continue;
            }
            let v = &mut self.voices[id];
            if v.queued.is_some() && v.current.is_some_and(|p| e.now >= p.ends - 0.005) { v.current = v.queued; v.queued = None; }
            if v.current.is_some_and(|p| e.now >= p.ends) && v.queued.is_none() && !v.pending { self.finish(e, id); continue; }
            if v.looping && v.current.is_some_and(|p| p.ends - e.now < 8.0) && v.queued.is_none() && !v.pending {
                let sound = v.sound; let segment = e.sounds[sound].pick(e.time, &mut e.random).unwrap_or(NONE); self.request(e, id, segment);
            }
            let mut gain = self.level(e, id); let v = &mut self.voices[id]; let reverb = e.sounds[v.sound].reverb; let mode3d = e.sounds[v.sound].mode3d;
            if let Some(position) = v.position {
                if v.spatial {
                    let (distance, span, local) = e.listener.place(position); let shape = if e.enhanced { acoustics.shape(v.category, true, distance, 0.0) } else { crate::engine::Shape { send: space_send, ..Default::default() } };
                    gain *= v.placement.curve.gain(distance); if !e.enhanced { gain *= v.placement.curve.openal(distance); } gain *= shape.gain;
                    Self::place(e, id, &mut v.placement, local, span, shape, reverb);
                } else if mode3d { gain *= v.placement.curve.gain(length(sub(position, e.listener.ear))); }
            }
            v.target = Some(gain); let tau = if e.now < v.until { v.tau } else { 0.02 };
            if v.gain_target.set(gain * v.level, e.now, tau) { e.commands.extend_from_slice(&[render::STREAM_GAIN as f64, id as f64, gain * v.level, tau, e.now, 0.0]); }
        }
        for stream in 0..self.queues.len() {
            while self.queues[stream][0].is_some() && self.free(stream) {
                let request = self.queues[stream][0].unwrap(); for i in 0..3 { self.queues[stream][i] = self.queues[stream][i + 1]; } self.queues[stream][3] = None; self.play(e, request.options);
            }
        }
    }
    fn place(e: &mut Engine, id: usize, v: &mut Voice, p: Point, span: f64, shape: crate::engine::Shape, reverb: f64) {
        if e.light { e.commands.extend_from_slice(&[render::STREAM_PLACE as f64, id as f64, p[0], p[1], p[2], 1.0, e.now]); return; }
        if e.hrtf {
            for axis in 0..3 { let value = p[axis] * span.max(1.0); if v.position_target[axis].set(value, e.now, 0.01) { e.commands.extend_from_slice(&[render::POSITION as f64, (64 + id) as f64, axis as f64, value, 0.01, e.now, 0.0]); } }
        } else {
            let spread = if e.enhanced { shape.spread.max((1.0 - span / 1.5).clamp(0.0, 1.0) * 0.6) } else { shape.spread };
            let lfe = if e.enhanced && e.lfe { shape.lfe * 0.5 } else { 0.0 }; let sent = [p[0] as f32, p[1] as f32, p[2] as f32, spread as f32, lfe as f32];
            if sent != v.sent { v.sent = sent; e.placements.extend_from_slice(&[(64 + id * 12) as f64, p[0], p[1], p[2], spread, lfe]); }
        }
        if e.enhanced && (shape.cutoff - v.cutoff).abs() > 10.0 { v.cutoff = shape.cutoff; e.commands.extend_from_slice(&[render::STREAM_PARAM as f64, id as f64, 0.0, shape.cutoff.min(e.sample_rate / 2.0), 0.03, e.now]); }
        let send = shape.send * if e.enhanced { reverb } else { 1.0 };
        if v.send_target.set(send, e.now, 0.05) { e.commands.extend_from_slice(&[render::STREAM_PARAM as f64, id as f64, 1.0, send, 0.05, e.now]); }
    }
}
