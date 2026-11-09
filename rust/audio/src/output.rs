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

// The page's Renderer owns slot assignment; this module owns slot DSP.
// A stream reserves twelve virtual mono feeds so each recorded bed lane can
// be placed separately without borrowing a live effect voice's history.
pub const VOICES: usize = 64;
pub const STREAMS: usize = 32;
const SLOTS: usize = VOICES + STREAMS * CHANNELS;
const HRTF_LEVEL: f64 = 0.6382634861905486; // 10^(-3.9/20), stored by GainNode

// Control state for VoiceOut.update and StreamOut's placed sources.
// Positions and pan retain their AudioParam timelines independently of the
// source cursor. active denotes a bound placement, not nonzero PCM.
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

// Renderer.build's Classic stereo chain: original trim, optional mono
// fold, dynamics level, WaveShaper table, then user volume. This is a distinct
// mix design, so it does not pass through the surround Master limiter.
struct Light {
    volume: Timeline,
    original: f32,
    level: f32,
    curve: Vec<f32>,
    gains: [f32; FRAMES],
}
impl Light {
    fn new() -> Self {
        // Build the former soft-clip WaveShaper's 4097-point table once. Its x
        // range is doubled; light_level compensates by halving the preceding gain.
        // Evaluating tanh per sample would change interpolation and cost more.
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
        // Keep the original GainNode's f32 store before folding or shaping.
        // Combining original and level into one multiply changes rounding and the
        // sample's position within the nonlinear lookup table.
        for v in input.iter_mut() {
            *v *= self.original;
        }
        if fold {
            mono(input, 2);
        }
        // User volume automation is downstream of the shaper. Moving it upstream
        // would make a volume change alter distortion, rather than just output level.
        self.volume.a_rate(block, &mut self.gains, rate, x86);
        // A mono fold has identical lanes. Shape it once, then copy.
        for c in 0..if fold { 1 } else { 2 } {
            for i in 0..FRAMES {
                let value = input[c * FRAMES + i] * self.level;
                // WaveShaper clamps out-of-range input and linearly interpolates its f32
                // table. Preserve those boundaries instead of evaluating the analytic curve;
                // the stored table and interpolation define the audible transfer function.
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

// One instance belongs exclusively to the output AudioWorkletProcessor.
// Configuration, uploads and source binds run in message handlers between
// render calls. process must neither allocate nor release owned buffers.
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
    // io and cfg are synchronous ABI staging, not shared-memory mailboxes.
    // The host must finish writing before applying a command and must refresh
    // its typed views if an upload grows WASM memory.
    pub io: [f64; 64],
    pub cfg: Vec<f64>,
    pub commands: Vec<f64>,
    pub frame: u64,
    pub output: Vec<f32>,
    pub light_mode: bool,
    pub decoder_staging: Vec<f32>,
    // Reuse command storage across message batches. The PCM arrays below
    // are permanent render scratch; present marks initialized feeds so stopped
    // sources cannot expose samples left in scratch by a previous quantum.
    voice_commands: Vec<f64>,
    light: Option<Light>,
    feeds: usize,
    input: Vec<f32>,
    present: Vec<u8>,
    mixed: Vec<f32>,
    lfe: Vec<f32>,
    // direct carries already-routed speaker or ear samples into Master.
    // send is the mono room bus; hrtf_sum keeps headphone accumulation separate
    // until the former headphone GainNode's single f32 scale is applied.
    direct: Vec<f32>,
    send: Vec<f32>,
    hrtf_database: Option<Database>,
    panners: Vec<Panner>,
    hrtf_sum: Vec<f32>,
    // SADIE has sixteen harmonic feeds, each with a left/right impulse pair.
    // IRCAM instead shares one read-only database among independent panners;
    // their delay and convolution histories must never be shared between voices.
    decoder: Vec<StereoConvolver>,
    pair: [[f32; FRAMES]; 2],
    scratch: [f32; FRAMES],
    gains: [f32; FRAMES],
    cutoff: Vec<f32>,
    bed_gains: Vec<[[f32; 2]; CHANNELS]>,
    test: Vec<f32>,
}
impl Output {
    // Retains the ported JS/GL call-site signature without an extra wrapper struct.
    #[allow(clippy::too_many_arguments)]
    pub fn new(rate: f64, channels: usize, enhanced: bool, headphones: bool, binaural: bool, mono: bool, x86: bool) -> Self {
        // Match Renderer.build's quality/output combinations, including mono as
        // an output fold rather than a different source layout. Binaural encodes
        // sixteen 3OA feeds internally but still produces two device channels.
        let light_mode = !enhanced && !headphones && channels == 2;
        let feeds = if binaural { 16 } else { channels };
        let mut voices = Engine::new(rate, VOICES, -3.0103, x86);
        // Classic bypasses the enhanced source lowpass. Source samples, cursor
        // and gain processing still belong to Engine for both quality settings.
        // No native input bus exists for the host to mix in parallel.
        voices.filtered = enhanced;
        voices.set_inputs(0);
        // Classic stereo owns its WaveShaper chain; other layouts own Master.
        // The limiter attack differs between the two mix designs and is intentional,
        // including the shorter Classic surround/headphone value.
        let master = if light_mode {
            None
        } else {
            let mut m = Master::new(rate, channels, -1.0, if enhanced { 0.08 } else { 0.06 }, 0.0015, x86);
            m.set_mono(mono);
            Some(m)
        };
        // Database construction performs resource conversion and FFT preparation.
        // Allocate every possible panner here so a render quantum can select or
        // crossfade kernels using fixed storage, without building a filter.
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
            // NaN forces each stream's first enhanced cutoff update to install its
            // actual coefficients. Later blocks reuse them until the k-rate value changes;
            // filter histories survive those coefficient updates.
            cutoff: vec![f32::NAN; STREAMS],
            bed_gains: vec![[[0.0; 2]; CHANNELS]; STREAMS],
            test: vec![0.0; channels * FRAMES],
        }
    }
    // Renderer.options supplies speaker directions and Classic decoder rows.
    // Install routing and per-layout master configuration together. This runs
    // between blocks; rebuilding filters and layout tables may allocate here.
    pub fn layout(&mut self, law: u32, decoder: Option<Decoder>, speakers: &[Speaker], center: i32, lfe: i32, original: f32) {
        self.mixer.set_layout(law, decoder, speakers);
        if let Some(master) = &mut self.master {
            // A speaker-less lane such as LFE must not get a full-range room return.
            // Without a dedicated LFE layout, preserve the former all-channel room mask.
            let rooms = if lfe >= 0 {
                speakers
                    .iter()
                    .enumerate()
                    .fold(0, |a, (i, s)| if s.is_some() { a | (1 << i) } else { a })
            } else {
                u32::MAX
            };
            master.set_rooms(rooms);
            // Enhanced highpasses the speaker mix and lowpasses its LFE feed. Classic keeps
            // its original centre treatment rather than those crossover settings.
            // These are mix choices, not host device downmix rules.
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
    // Renderer.loadDecoder supplies channel-major pairs: harmonic zero left,
    // zero right, harmonic one left, and so on. Build spectra between blocks;
    // process only updates convolution history and accumulates into the ears.
    pub fn decoder(&mut self, filters: &[f32], frames: usize) {
        self.decoder.clear();
        for channel in 0..16 {
            let at = channel * 2 * frames;
            self.decoder
                .push(StereoConvolver::new(&filters[at..at + frames], &filters[at + frames..at + 2 * frames]));
        }
    }
    // VoiceOut construction binds a fresh logical source. Reset automation
    // and panner tails on slot reuse, matching creation of a new PannerNode;
    // a previous occupant's ear history must not leak into this source.
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
    // StreamOut.start can learn the channel width from its first segment.
    // Rebinding its routing must preserve already-scheduled segment cursors and
    #[cfg(target_arch = "wasm32")]
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
    // StreamOut's recorded mono/stereo/quad lanes are virtual speakers.
    // Resolve their fixed directions and Classic light gains on configuration,
    // so the render loop need only copy or multiply each lane.
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
            // Enhanced quad beds use side-rear angles; Classic retains the wider
            // rear pair. These positions reproduce the former stream speaker nodes,
            // not the physical output layout selected by the player.
            if self.headphones && self.enhanced && channels == 4 {
                angle = [30.0, -30.0, 110.0, -110.0][c];
            }
            // Classic light exaggerates front separation threefold. Preserve its
            // ninety-degree cap so a stereo bed reaches hard left/right rather than
            // acquiring the gentler surround/headphone speaker placement.
            if self.light_mode && angle.abs() < 90.0 {
                angle = angle.signum() * (angle.abs() * 3.0).min(90.0);
            }
            let a = angle * PI / 180.0;
            let (x, z) = (-host::sin(a), -host::cos(a));
            let mut gains = alsoft(x, 0.0, z);
            // Snap numerical near-zero and near-unity gains just as the JS graph did.
            // The exact endpoints matter for silence and unity-copy arithmetic, not only
            // for how far left or right a virtual speaker appears.
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
    // Renderer.setVolume has one context-timed meaning across mix designs.
    // Route it into the active master or light timeline; assigning a value also
    // sets the timeline's base before installing the scheduled event.
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
    // LIGHT_LEVELS are dB gains ahead of the Classic stereo shaper. Divide
    // by two because its table was authored over twice the nominal input range;
    // removing this factor would change both loudness and clipping onset.
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
            // VoiceOut and Engine share the source command prefix. Forward the
            // original words unchanged so rate/gain/stop ordering remains identical;
            // placement-only and stream commands are handled by this outer phase.
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
                // POSITION reproduces PannerNode position AudioParams. Store endpoint
                // values as f32 while event times and target constants remain f64; rounding
                // coordinates later would select different HRTF kernels near boundaries.
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
                // The AudioParam-shaped JS wrapper uses negative tau for cancellation.
                // Cancellation removes future automation without inventing a new gain jump;
                // set and exponential-target commands retain their separate meanings.
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
                // Three segment slots can overlap at scheduled boundaries. Starting one
                // replaces only that Source; the bed's shared gain and routing continue,
                // matching multiple BufferSourceNodes feeding one stream GainNode.
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
                // Disable routing immediately, but leave sample deallocation to the
                // separate host release message. process never drops a PCM allocation.
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
        // Return the taken vector rather than allocating a new message buffer.
        // This ownership swap happens only in apply, never inside render work.
        self.commands = commands;
    }

    // Render one fixed 128-frame Web Audio quantum. All arrays and filter
    // state already exist; loops may write them but must not grow containers,
    // construct a panner, decode an asset, or free a finished sample.
    pub fn process(&mut self) -> Option<f64> {
        // Clear accumulation buses, including bypass test audio, every quantum.
        // Input scratch is gated by present instead of clearing all reserved slots;
        // this avoids scanning hundreds of inactive virtual speaker lanes.
        self.direct.fill(0.0);
        self.test.fill(0.0);
        self.send.fill(0.0);
        self.hrtf_sum.fill(0.0);
        self.present.fill(0);
        self.voices.frame = self.frame as f64;
        // Classic stereo voices already produce their two-channel light mix.
        // Other modes render mono voice feeds for placement, plus a separate room
        // send; none of those feeds has yet entered the output master chain.
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
            // The speaker-test panel addresses a physical lane after mix processing.
            // Keep these tones outside user volume, mono fold and dynamics so selecting
            // a speaker still identifies that speaker under any listening preset.
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
                // The room tap is before the enhanced occlusion lowpass, as in the
                // former node graph. Filtering the tap as well would darken reverberation
                // and change the balance between direct and reflected sound.
                stream.send.a_rate(self.frame, &mut self.gains, self.rate, self.x86);
                for i in 0..FRAMES {
                    self.send[i] += stream.output[i] * self.gains[i];
                }
                if self.enhanced {
                    // Biquad cutoff is sampled once per quantum. Update coefficients only
                    // when it changes; recreating filters would erase their history and click.
                    // A placed stream contributes its mono first lane to the spatial path.
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
                // LightPan owns the two separately automated side gains. Applying a
                // single pan value after source summation would lose their target curves
                // and alter release behavior when the source moves or fades.
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
        // Beds enter the same Classic stereo shaper as effects. Fold to mono
        // inside Light before shaping, then append physical-speaker tests afterward.
        // There is no second Master pass on this mix.
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
        // IRCAM replaces the former native HRTF PannerNodes. Each source keeps
        // its own convolution history, including silent tail quanta while bound;
        // collect both ears before applying the common headphone trim.
        if self.headphones && !self.binaural {
            for slot in 0..VOICES {
                if !self.placements[slot].active {
                    continue;
                }
                let meta = &mut self.placements[slot];
                let src = &self.input[slot * FRAMES..(slot + 1) * FRAMES];
                // Enhanced nonspatial voices retain the direct mixer route. Classic
                // headphones spatialize those voices too, preserving its original virtual
                // speaker presentation instead of silently changing the quality preset.
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
                // This source has already reached the ear bus. Remove its mono feed
                // from the speaker mixer so it cannot be heard twice.
                self.present[slot] = 0;
            }
            for slot in 0..STREAMS {
                let stream = &self.streams[slot];
                if !stream.on || stream.kind >= 3 {
                    continue;
                }
                // Recorded beds place each lane at the same virtual speaker angles as
                // route_stream. A placed bed uses one moving mono panner instead; its other
                // recorded lanes do not become additional positioned sources.
                let angles: &[f64] = if stream.spatial {
                    &[0.0]
                } else {
                    match stream.channels {
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
                    }
                };
                if !stream.spatial && ((self.enhanced && stream.channels != 4) || angles.is_empty()) {
                    continue;
                }
                for (c, &angle_deg) in angles.iter().enumerate() {
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
                        let a = angle_deg * PI / 180.0;
                        // The former positionX/Z AudioParams stored floats before PannerHandler
                        // normalized the vector. Keep that round trip even for fixed bed angles;
                        // using full-precision trigonometry changes kernel selection at boundaries.
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
        // Enhanced UI/nonspatial voices use equal-power stereo outside the 3OA
        // encoder. Their direct ear signal joins the decoded field before Master,
        // so they retain dynamics processing without acquiring spatial HRIR color.
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

        // Mixer consumes only present mono feeds. Its output lanes are either
        // physical speakers or sixteen harmonic components; direct ear/bed buses
        // and the room tap remain separate until the master stage.
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
        // Decode each 3OA harmonic through its own two-ear SADIE impulse pair.
        // Sum in harmonic order, retaining f32 additions. The raw harmonic lanes
        // must then be cleared so Master cannot interpret them as speaker PCM.
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
        // Master combines the speaker mix, optional LFE, room send and already
        // routed direct bus in its defined gain/filter order. A limiter report is
        // control data; only the final planar output is copied to the device.
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
    // StreamOut's nonspatial routing replaces its former split/gain/delay
    // node graph. Preserve each format's coefficients rather than delegating
    // downmix or upmix to the output device's speaker interpretation.
    fn route_bed(&mut self, slot: usize) {
        let stream = &mut self.streams[slot];
        let count = stream.channels;
        let n = self.channels;
        let spatial_bank = (!self.enhanced && !self.headphones && !self.light_mode && matches!(count, 1 | 2 | 4)) || (self.binaural && count == 4);
        // Classic surround beds and Enhanced binaural quad beds travel through
        // virtual mono speakers. Their cached placement belongs to Mixer, so direct
        // copying here would bypass the selected pan law or harmonic encoder.
        if spatial_bank {
            for c in 0..count {
                let id = VOICES + slot * CHANNELS + c;
                self.input[id * FRAMES..(id + 1) * FRAMES].copy_from_slice(&stream.output[c * FRAMES..(c + 1) * FRAMES]);
                self.present[id] = 1;
            }
            return;
        }
        // These lanes are handled by the later IRCAM loop. Leave them out of the
        // direct bus to avoid a second unfiltered copy of the same bed.
        if self.headphones && !self.binaural && ((!self.enhanced && matches!(count, 1 | 2 | 4)) || (self.enhanced && count == 4)) {
            return;
        }
        // Classic light uses the precomputed OpenAL side gains. It still sums
        // beds before the common shaper; the cached gains only replace fixed speaker
        // placement and do not include user volume or dynamics level.
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
        // This closure borrows existing arrays and is called synchronously; it
        // creates no heap object. Every contribution rounds into the destination
        // f32 bus before the next source or routing coefficient is applied.
        let add = |target: &mut [f32], from: usize, to: usize, gain: f32| {
            for i in 0..FRAMES {
                target[to * FRAMES + i] += stream.output[from * FRAMES + i] * gain;
            }
        };
        // Enhanced stereo keeps recorded fronts unchanged and folds quad rears
        // at 0.7. A mono bed uses equal-power centre. These deliberate coefficients
        // differ from Web Audio's generic speakers downmix.
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
        // A matching multichannel recording already names physical speaker lanes.
        // Pass it discretely, including its recorded LFE, rather than spatializing
        // or deriving another bass signal from each lane.
        if count == n && n > 4 {
            for c in 0..n {
                add(&mut self.direct, c, c, 1.0);
            }
            return;
        }
        // A 5.1 recording spreads each surround across 7.1 back and side speakers.
        // The retained 0.5/0.87 pair is the authored routing, not a renormalized
        // trigonometric pan; changing it changes the bed's energy.
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
        // Enhanced dialogue uses a real centre speaker when available. Other
        // mono beds sit equally in front left/right, preserving the former stream
        // kind distinction rather than assigning every mono recording to centre.
        if count == 1 {
            if stream.kind == 1 && self.enhanced && n > 2 {
                add(&mut self.direct, 0, 2, 1.0);
            } else {
                add(&mut self.direct, 0, 0, std::f32::consts::FRAC_1_SQRT_2);
                add(&mut self.direct, 0, 1, std::f32::consts::FRAC_1_SQRT_2);
            }
            return;
        }
        // Fronts remain direct. Only enhanced stereo music on surround gets the
        // side-derived ambience below; plain stereo effects do not gain invented
        // rear content merely because more output channels are available.
        add(&mut self.direct, 0, 0, 1.0);
        add(&mut self.direct, 1, 1, 1.0);
        if count == 2 && !(stream.kind == 2 && self.enhanced && n > 2) {
            return;
        }
        let rear_l = 4;
        let rear_r = 5;
        let side_l = if n >= 8 { 6 } else { 4 };
        let side_r = if n >= 8 { 7 } else { 5 };
        // Stereo ambience derives the difference signal L-R, lowpasses it at
        // 7 kHz, then decorrelates it with four fractional DelayNode equivalents.
        // Delay rings were allocated at stream creation; render only advances them.
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
                // Different delays and opposite signs reduce coherent rear localization.
                // The 7.1 back pair is attenuated by 0.7 in addition to its half-level tap;
                // side taps retain the original half level.
                let gain = (0.5 * sign * if n >= 8 && j < 2 { 0.7 } else { 1.0 }) as f32;
                let delay = seconds * self.rate;
                let len = stream.delays[j].len();
                for i in 0..FRAMES {
                    let at = stream.delay_at[j];
                    stream.delays[j][at] = stream.side[i];
                    // DelayNode interpolates between neighboring samples for noninteger
                    // sample-rate delays. Rounding milliseconds to whole frames would change
                    // both the phase of the difference signal and its cancellation pattern.
                    let read = (at as f64 + len as f64 - delay).rem_euclid(len as f64);
                    let a = read as usize;
                    let f = (read - a as f64) as f32;
                    let sample = stream.delays[j][a] + f * (stream.delays[j][(a + 1) % len] - stream.delays[j][a]);
                    self.direct[c * FRAMES + i] += sample * gain;
                    stream.delay_at[j] = (at + 1) % len;
                }
            }
        // Quad recordings retain their front/rear identity through layout changes.
        // Fold rears into stereo at the quality-specific gain, split them for 7.1,
        // or send them directly to the two 5.1 surround lanes.
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

// VoiceOut.update's position targets advance at audio rate, but the browser
// HRTFPanner chooses one direction per quantum from the first sample. Advance
// all three timelines fully before selecting that direction; later automation
// must not stall just because spatial selection is block based.
// Block-level HRTF panner step across 3D placement and automation state.
#[allow(clippy::too_many_arguments)]
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
    for (pos, axis) in position.iter_mut().zip(&mut meta.axes) {
        axis.a_rate(block, scratch, rate, x86);
        *pos = scratch[0] as f64;
    }
    let (azimuth, elevation) = crate::hrtf::angles(position.map(|v| v as f32));
    panner.process(database.as_ref().unwrap(), rate, azimuth, elevation, input, pair);
}

// VoiceOut's nonspatial binaural pan reproduces StereoPannerNode's mono
// equal-power law. Pan is a float AudioParam sampled per frame; trigonometry
// is evaluated in double precision and its gains stored as floats.
fn stereo_pan(meta: &mut Placement, input: &[f32], mix: &mut [f32], block: u64, rate: f64, x86: bool, scratch: &mut [f32; FRAMES]) {
    meta.pan.a_rate(block, scratch, rate, x86);
    for i in 0..FRAMES {
        let a = (scratch[i].clamp(-1.0, 1.0) as f64 + 1.0) * PI / 4.0;
        mix[i] += input[i] * host::cos(a) as f32;
        mix[FRAMES + i] += input[i] * host::sin(a) as f32;
    }
}

// Device callbacks bind between quanta. Keep their storage policy separate
// from the worklet's cold message handler, with the same routing and cursors.
#[cfg(not(target_arch = "wasm32"))]
impl Output {
    /// StreamOut.start's native bind reuses every admitted bed buffer.
    pub fn stream(&mut self, slot: usize, channels: usize, kind: u32, spatial: bool) {
        let on=self.streams[slot].on;
        let gain=self.streams[slot].gain;
        let sources=self.streams[slot].sources;
        self.streams[slot].reset(self.rate);
        let channels=channels.min(CHANNELS);
        self.streams[slot].configure(channels,kind,spatial);
        if on {self.streams[slot].gain=gain;self.streams[slot].sources=sources;}
        let rate=self.rate;
        self.streams[slot].filter.resize_with(channels,||Biquad::new(Kind::Lowpass,(rate/2.0) as f32,-3.0103,0.0,rate));
        self.placements[VOICES+slot]=Placement::default();
        self.placements[VOICES+slot].active=true;
        self.placements[VOICES+slot].spatial=spatial;
        self.route_stream(slot);
    }
    /// AudioOutput::controls reserves both forwarding banks before playback.
    /// Source binds have the same allocation bound from the first callback.
    pub fn reserve_controls(&mut self, words: usize) {
        let words=words.min(65536);
        self.commands.resize(words,0.0);
        self.voice_commands.reserve(words);
        self.voices.command_storage(words);
        for stream in &mut self.streams {stream.filter.reserve(CHANNELS);}
    }
}
