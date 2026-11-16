//! Device quantum adaptation for audio/spatial.js::Renderer.build's Rust Master.
//! cpal buffers need not be multiples of the engine's 128-frame quantum. Keep
//! the unused samples, so a short callback never advances or loses DSP time.
use ::audio::{master::Master, FRAMES};
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, FromSample, SizedSample};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, AtomicU32, AtomicUsize, Ordering}};

// What the output device can take, learnt when it opens: its own channel
// count (what Automatic follows) and the most it accepts at 48 kHz; and the
// channel count the battle's output layout wants, which reopens the device.
// The widths it takes are a mask (bit n: n channels); a width it would not
// open is struck off. Each open (or failure) moves the generation on, so a
// battle re-resolves its layout against what the device really plays.
static DEVICE_DEFAULT: AtomicUsize = AtomicUsize::new(2);
static DEVICE_WIDTHS: AtomicU32 = AtomicU32::new(1 << 2);
static STRUCK: AtomicU32 = AtomicU32::new(0);
static DEVICE_GENERATION: AtomicU64 = AtomicU64::new(0);
static WANTED: AtomicUsize = AtomicUsize::new(2);
/// The output device's own channel count and the widths it plays (bit n: n channels; stereo always).
pub fn device_channels() -> (usize, u32) {
    (DEVICE_DEFAULT.load(Ordering::Relaxed), (DEVICE_WIDTHS.load(Ordering::Relaxed) & !STRUCK.load(Ordering::Relaxed)) | 1 << 2)
}
/// Moves on whenever the device is opened again or a width is struck off.
pub fn device_generation() -> u64 {
    DEVICE_GENERATION.load(Ordering::Relaxed)
}
/// The device would not play `channels` wide: no layout asks for that again.
pub fn strike_width(channels: usize) {
    if channels != 2 && channels < 32 {
        STRUCK.fetch_or(1u32 << channels, Ordering::Relaxed);
        DEVICE_GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}
/// The battle's layout asks the device for this many channels.
pub fn want_channels(channels: usize) {
    WANTED.store(channels, Ordering::Relaxed);
}
pub fn wanted_channels() -> usize {
    WANTED.load(Ordering::Relaxed)
}
/// Which of an output layout's channels (L R C LFE SL SR; FL FR FC LFE BL BR SL
/// SR TFL TFR TBL TBR) device channel `device` plays. Windows and macOS take
/// that order as it is. ALSA's (Linux) puts the rear pair before the centre
/// and LFE: FL FR RL RR FC LFE [SL SR]. ALSA has no 12-channel order; the
/// PipeWire plugin's is FL FR FC LFE SL SR RL RR and the four heights.
pub fn order(channels: usize, device: usize) -> usize {
    if !cfg!(target_os = "linux") {
        return device;
    }
    match (channels, device) {
        (6 | 8, 2) => 4,
        (6 | 8, 3) => 5,
        (6 | 8, 4) => 2,
        (6 | 8, 5) => 3,
        (12, 4) => 6,
        (12, 5) => 7,
        (12, 6) => 4,
        (12, 7) => 5,
        (_, d) => d,
    }
}

#[cfg(test)]
mod order_tests {
    #[test]
    fn every_layout_channel_reaches_one_device_channel() {
        for channels in [2, 6, 8, 12] {
            let mut seen: Vec<usize> = (0..channels).map(|d| super::order(channels, d)).collect();
            seen.sort();
            assert_eq!(seen, (0..channels).collect::<Vec<_>>());
        }
    }
}

// Device diagnostics never take the render thread lock. Relaxed atomics are
// sufficient because none of these counters publishes audio sample ownership.
#[derive(Default)]
pub struct Status {
    pub blocks: AtomicU64,
    pub failed: AtomicBool,
    pub peak: AtomicU32,
}

// One callback closure exclusively owns this processor and its PCM. The
// window never reaches into DSP state while the device thread is rendering.
// Loading and validation finish before the stream can invoke its callback.
pub struct MasterOutput {
    master: Master,
    pcm: Vec<f32>,
    source: usize,
    feeds: [f32; FRAMES * 2],
    output: [f32; FRAMES * 2],
    next: usize,
    block: u64,
}
impl MasterOutput {
    // Master::new reserves reverb and limiter scratch for the fixed quantum.
    // Reject incomplete stereo frames before moving PCM to the callback.
    pub fn new(rate: u32, pcm: Vec<f32>) -> Result<Self, String> {
        if !(8000..=192000).contains(&rate) || pcm.len() % 2 != 0 || pcm.iter().any(|v| !v.is_finite()) {
            return Err("PCM must be finite interleaved stereo at a supported sample rate".into());
        }
        Ok(Self { master: Master::new(rate as f64, 2, -1.0, 0.1, 0.005, cfg!(target_arch = "x86_64")),
            pcm, source: 0, feeds: [0.0; FRAMES * 2], output: [0.0; FRAMES * 2], next: FRAMES * 2, block: 0 })
    }
    // Output remains planar inside Master, matching the browser DSP stores.
    // Only this final copy interleaves it into the device buffer. An odd-sized
    // callback preserves both its channel position and remaining quantum.
    pub fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T], status: &Status) {
        let mut peak = 0.0f32;
        for sample in data {
            if self.next == FRAMES * 2 {
                // Decoded PCM is admitted once. A capture with no audio supplies
                // silence through the same chain; it never selects another DSP.
                for frame in 0..FRAMES {
                    for channel in 0..2 {
                        self.feeds[channel * FRAMES + frame] = self.pcm.get(self.source).copied().unwrap_or(0.0);
                        if !self.pcm.is_empty() { self.source = (self.source + 1) % self.pcm.len(); }
                    }
                }
                self.master.process(&self.feeds, None, &[0.0; FRAMES], None, None, self.block, &mut self.output);
                self.block += 1;
                status.blocks.fetch_add(1, Ordering::Relaxed);
                self.next = 0;
            }
            let value = self.output[(self.next % 2) * FRAMES + self.next / 2];
            peak = peak.max(value.abs());
            *sample = T::from_sample(value);
            self.next += 1;
        }
        // Aggregate metering once per callback, rather than one atomic update
        // per sample. This counter is diagnostic, not part of the audio graph.
        status.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }
}

/// Shared audio router keeping one `cpal` stream open across the standalone
/// application lifecycle (startup -> shell -> battle -> shell).
#[derive(Default)]
pub struct SharedAudioRouter {
    pub shell: Mutex<Option<bfe_shell::audio::ShellAudioMixer>>,
    pub live: Mutex<Option<crate::online::sound::LiveOutput>>,
    /// The speaker test of the battle's AUDIO DETAILS: while one plays it is all the device hears.
    test: Mutex<Option<SpeakerTest>>,
}

const TEST_RATE: f64 = 48_000.0;
const BURST: f64 = 1.2;
const GAP: f64 = 0.25;
const AROUND: f64 = 6.0;

/// The speaker test of the battle's AUDIO DETAILS (`AudioPanel.test` and `around`): pink
/// noise on one speaker at a time, the LFE a 50 Hz tone, or a burst carried once round the room from ahead to the
/// left through the layout's panning. It plays in the output layout's channels; while it plays it is all the device
/// hears.
pub struct SpeakerTest {
    // The layout's channels and the speakers to sound in turn (or the sweep).
    channels: usize,
    lfe: i32,
    turn: Vec<usize>,
    around: Option<audio::mixer::Mixer>,
    frame: u64,
    rng: u32,
    b: [f32; 7],
    gains: Vec<f32>,
    scratch: Vec<f32>,
}

impl SpeakerTest {
    /// `kind`: a channel index of the battle's layout, `all`, or `around`.
    pub fn new(kind: &str) -> Option<Self> {
        let layout = crate::online::sound_output::current_layout();
        let channels = layout.speakers.len();
        let (turn, around) = match kind {
            "all" => ((0..channels).collect(), None),
            "around" => {
                let law = if channels == 2 { audio::mixer::PANPOT } else { audio::mixer::VBAP };
                (Vec::new(), Some(audio::mixer::Mixer::new(1, channels, law, None, layout.speakers)))
            }
            // The stereo panel's names, kept for old saved actions.
            "left" => (vec![0], None),
            "right" => (vec![1], None),
            n => (vec![n.parse::<usize>().ok().filter(|c| *c < channels)?], None),
        };
        Some(Self { channels, lfe: layout.lfe, turn, around, frame: 0, rng: 0x1234_5678, b: [0.0; 7], gains: vec![0.0; channels],
            scratch: vec![0.0; channels * audio::FRAMES] })
    }

    fn length(&self) -> f64 {
        if self.around.is_some() { AROUND } else { self.turn.len() as f64 * (BURST + GAP) }
    }

    fn pink(&mut self) -> f32 {
        // Paul Kellet's pink filter over white noise, as the panel's.
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let w = (self.rng >> 8) as f32 / 8_388_608.0 - 1.0;
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + w * 0.055_517_9;
        b[1] = 0.99332 * b[1] + w * 0.075_075_9;
        b[2] = 0.969 * b[2] + w * 0.153_852;
        b[3] = 0.8665 * b[3] + w * 0.310_485_6;
        b[4] = 0.55 * b[4] + w * 0.532_952_2;
        b[5] = -0.7616 * b[5] - w * 0.016_898;
        let v = (b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362) * 0.11;
        b[6] = w * 0.115_926;
        v
    }

    /// The next frame in the layout's channels, or false once the test is over.
    fn next(&mut self, out: &mut [f32]) -> bool {
        out.fill(0.0);
        let t = self.frame as f64 / TEST_RATE;
        self.frame += 1;
        if t >= self.length() {
            return false;
        }
        let pink = self.pink();
        if let Some(mixer) = &mut self.around {
            // Ahead, then round to the left, once (x right, z back).
            if self.frame % audio::FRAMES as u64 == 1 {
                let a = t / AROUND * std::f64::consts::TAU;
                mixer.take(&[0.0, -a.sin() as f32, 0.0, -a.cos() as f32, 0.0, 0.0], self.frame == 1);
                let input = [1.0f32; audio::FRAMES];
                let mut block = audio::BlockMut { data: &mut self.scratch, channels: self.channels, frames: audio::FRAMES };
                mixer.process(&input, &[1], &mut block, None);
                for c in 0..self.channels {
                    self.gains[c] = self.scratch[c * audio::FRAMES + audio::FRAMES - 1];
                }
            }
            let fade = (t.min(AROUND - t) / 0.02).clamp(0.0, 1.0) as f32;
            for (o, g) in out.iter_mut().zip(&self.gains) {
                *o = pink * 0.3 * fade * g;
            }
            return true;
        }
        let slot = (t / (BURST + GAP)) as usize;
        let at = t - slot as f64 * (BURST + GAP);
        let Some(&channel) = self.turn.get(slot) else { return false };
        if at >= BURST || channel >= out.len() {
            return true;
        }
        out[channel] = if channel as i32 == self.lfe {
            // A 50 Hz tone for the subwoofer, 50 ms ramps.
            let ramp = (at.min(BURST - at) / 0.05).clamp(0.0, 1.0);
            (0.25 * ramp * (std::f64::consts::TAU * 50.0 * at).sin()) as f32
        } else {
            let fade = (at.min(BURST - at) / 0.02).clamp(0.0, 1.0) as f32;
            pink * 0.35 * fade
        };
        true
    }
}

impl SharedAudioRouter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set_shell(&self, mixer: Option<bfe_shell::audio::ShellAudioMixer>) {
        if let Ok(mut guard) = self.shell.lock() {
            *guard = mixer;
        }
    }

    /// Starts the named speaker test (`left`, `right`, `all`, `around`), or stops the one playing (`None`).
    pub fn speaker_test(&self, kind: Option<&str>) {
        if let Ok(mut guard) = self.test.lock() {
            *guard = kind.and_then(SpeakerTest::new);
            if std::env::var_os("BFE_MENU_INPUT_TRACE").is_some() {
                eprintln!("[speaker test] {kind:?}");
            }
        }
    }

    pub fn testing(&self) -> bool {
        self.test.lock().is_ok_and(|test| test.is_some())
    }

    pub fn set_live(&self, live: Option<crate::online::sound::LiveOutput>) {
        if let Ok(mut guard) = self.live.lock() {
            *guard = live;
        }
    }

    pub fn pump_headless(&self, status: &Status, scratch: &mut [f32; FRAMES * 2]) {
        if let Ok(mut test_guard) = self.test.try_lock() {
            if let Some(test) = test_guard.as_mut() {
                let mut done = false;
                let mut layout = [0.0f32; 12];
                for frame in scratch.chunks_mut(2) {
                    done = done || !test.next(&mut layout[..test.channels]);
                    if done { layout.fill(0.0); }
                    frame[0] = layout[0];
                    frame[1] = layout[1];
                }
                if done {
                    *test_guard = None;
                    if std::env::var_os("BFE_MENU_INPUT_TRACE").is_some() {
                        eprintln!("[speaker test] done");
                    }
                }
                return;
            }
        }
        if let Ok(mut live_guard) = self.live.try_lock() {
            if let Some(live) = live_guard.as_mut() {
                // No device: the battle's sounds move on at the device's pace, unmixed.
                live.headless = true;
                live.fill(scratch, 2, status);
                return;
            }
        }
        if let Ok(mut shell_guard) = self.shell.try_lock() {
            if let Some(shell) = shell_guard.as_mut() {
                shell.render_interleaved(scratch);
                status.blocks.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

pub struct DynamicOutput {
    router: Arc<SharedAudioRouter>,
    shell_block: [f32; FRAMES * 2],
    shell_pos: usize,
    // The device's channels per frame; the menus and the test play on the
    // first two.
    channels: usize,
}

impl DynamicOutput {
    pub fn new(router: Arc<SharedAudioRouter>, channels: usize) -> Self {
        Self {
            router,
            shell_block: [0.0; FRAMES * 2],
            shell_pos: FRAMES * 2,
            channels: channels.max(1),
        }
    }

    pub fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T], status: &Status) {
        let channels = self.channels;
        if let Ok(mut test_guard) = self.router.test.try_lock() {
            if let Some(test) = test_guard.as_mut() {
                let mut done = false;
                let mut layout = [0.0f32; 12];
                let have = test.channels;
                for frame in data.chunks_mut(channels) {
                    done = done || !test.next(&mut layout[..have]);
                    if done { layout.fill(0.0); }
                    for (device, sample) in frame.iter_mut().enumerate() {
                        let source = order(have, device);
                        *sample = T::from_sample(if source < have { layout[source] } else { 0.0 });
                    }
                }
                if done {
                    *test_guard = None;
                }
                return;
            }
        }
        if let Ok(mut live_guard) = self.router.live.try_lock() {
            if let Some(live) = live_guard.as_mut() {
                live.headless = false;
                live.fill(data, channels, status);
                return;
            }
        }
        if let Ok(mut shell_guard) = self.router.shell.try_lock() {
            if let Some(shell) = shell_guard.as_mut() {
                let mut peak = 0.0f32;
                for frame in data.chunks_mut(channels) {
                    if self.shell_pos >= self.shell_block.len() {
                        shell.render_interleaved(&mut self.shell_block);
                        status.blocks.fetch_add(1, Ordering::Relaxed);
                        self.shell_pos = 0;
                    }
                    for (i, sample) in frame.iter_mut().enumerate() {
                        let val = if i < 2 { self.shell_block[self.shell_pos + i] } else { 0.0 };
                        peak = peak.max(val.abs());
                        *sample = T::from_sample(val);
                    }
                    self.shell_pos += 2;
                }
                status.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
                return;
            }
        }
        for sample in data {
            *sample = T::from_sample(0.0);
        }
    }
}

// Both sources end in the existing Rust output chain. Selecting a source is
// cold admission; each callback retains its processor and partial quantum.
enum DeviceOutput {
    Pcm(MasterOutput),
    Sound(crate::sound::SoundOutput),
    Live(crate::online::sound::LiveOutput),
    Dynamic(DynamicOutput),
}
impl DeviceOutput {
    fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T], status: &Status) {
        match self {
            Self::Pcm(p) => p.fill(data, status),
            Self::Sound(s) => s.fill(data, status),
            Self::Live(s) => s.fill(data, 2, status),
            Self::Dynamic(d) => d.fill(data, status),
        }
    }
}
// Keep the stream alive for the complete window lifetime. Dropping it stops
// callbacks before the last shared diagnostic reference is released.
pub struct AudioDevice {
    stream: cpal::Stream,
    pub status: Arc<Status>,
    /// The channels the stream was opened with.
    pub channels: usize,
}
impl AudioDevice {
    // Startup negotiates one supported sample format. Runtime errors are
    // visible to the window host; they never choose another render chain.
    pub fn open(pcm: Vec<f32>, rate: u32) -> Result<Self, String> {
        Self::open_output(DeviceOutput::Pcm(MasterOutput::new(rate,pcm)?),rate,2)
    }
    pub fn open_sound(sound: crate::sound::SoundOutput) -> Result<Self,String> {
        Self::open_output(DeviceOutput::Sound(sound),48000,2)
    }
    pub fn open_live(output:crate::online::sound::LiveOutput)->Result<Self,String>{Self::open_output(DeviceOutput::Live(output),48000,2)}
    /// The application's one stream, `channels` wide (stereo, or the battle's
    /// surround layout when the device takes it).
    pub fn open_dynamic(router: Arc<SharedAudioRouter>, channels: usize) -> Result<Self, String> {
        let wanted = channels.max(2) as u16;
        Self::open_output_with(48000, wanted, |channels| DeviceOutput::Dynamic(DynamicOutput::new(router, channels)))
    }
    fn open_output(output: DeviceOutput, rate: u32, channels: u16) -> Result<Self,String> {
        let mut output = Some(output);
        Self::open_output_with(rate, channels, move |_| output.take().unwrap())
    }
    fn open_output_with(rate: u32, channels: u16, output: impl FnOnce(usize) -> DeviceOutput) -> Result<Self,String> {
        let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        let usable = |c: &cpal::SupportedStreamConfigRange| c.min_sample_rate().0 <= rate && c.max_sample_rate().0 >= rate
            && matches!(c.sample_format(), cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16);
        let configs: Vec<_> = device.supported_output_configs().map_err(|e| e.to_string())?.filter(usable).collect();
        let max = configs.iter().map(|c| c.channels() as usize).max().unwrap_or(2);
        let widths = configs.iter().filter(|c| c.channels() < 32).fold(0u32, |m, c| m | 1 << c.channels());
        let default = device.default_output_config().map(|c| c.channels() as usize).unwrap_or(2);
        DEVICE_DEFAULT.store(default.max(2), Ordering::Relaxed);
        DEVICE_WIDTHS.store(widths, Ordering::Relaxed);
        // Explicit rate and channel count prevent accidental speed or channel
        // changes. A width the device lacks falls back to stereo.
        let supported = configs.iter().find(|c| c.channels() == channels).or_else(|| configs.iter().find(|c| c.channels() == 2))
            .ok_or("audio device does not support stereo at the requested rate")?.with_sample_rate(cpal::SampleRate(rate));
        let output = output(supported.channels() as usize);
        let format = supported.sample_format();
        let config = supported.config();
        let status = Arc::new(Status::default());
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, output, &status),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, output, &status),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, output, &status),
            _ => unreachable!(),
        }?;
        let result = Self { stream, status, channels: config.channels as usize };
        eprintln!("audio device: {} channels at {rate} Hz (device default {default}, up to {max})", result.channels);
        let played = result.play();
        DEVICE_GENERATION.fetch_add(1, Ordering::Relaxed);
        if result.channels != channels as usize {
            strike_width(channels as usize);
        }
        played?;
        Ok(result)
    }
    pub fn play(&self) -> Result<(), String> { self.stream.play().map_err(|e| e.to_string()) }
    pub fn pause(&self) -> Result<(), String> { self.stream.pause().map_err(|e| e.to_string()) }
    // cpal invokes its error closure on a backend thread. The window observes
    // that fault on its next redraw and performs ordinary shutdown there.
    pub fn check(&self) -> Result<(), String> {
        if self.status.failed.load(Ordering::Relaxed) { Err("audio output stream failed".into()) } else { Ok(()) }
    }
}
// Sample conversion is selected once through the generic callback. There is
// no format switch, allocation, message queue or mutex in the sample loop.
fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig,
    mut output: DeviceOutput, status: &Arc<Status>) -> Result<cpal::Stream, String> {
    let rendered = status.clone();
    let failed = status.clone();
    device.build_output_stream(config, move |data: &mut [T], _| output.fill(data, &rendered),
        move |_| { failed.failed.store(true, Ordering::Relaxed); }, None).map_err(|e| e.to_string())
}
