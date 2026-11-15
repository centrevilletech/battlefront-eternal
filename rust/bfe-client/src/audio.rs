//! Device quantum adaptation for audio/spatial.js::Renderer.build's Rust Master.
//! cpal buffers need not be multiples of the engine's 128-frame quantum. Keep
//! the unused samples, so a short callback never advances or loses DSP time.
use ::audio::{master::Master, FRAMES};
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, FromSample, SizedSample};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering}};

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
const AROUND: f64 = 4.0;

/// Pink noise on one speaker at a time (`AudioPanel.test` in `src/audio/panel.js`), stereo here.
pub struct SpeakerTest {
    // Per burst: the speaker (0 left, 1 right, 2 swept round the room) and where in the test it is.
    bursts: Vec<u8>,
    frame: u64,
    rng: u32,
    b: [f32; 3],
}

impl SpeakerTest {
    pub fn new(kind: &str) -> Option<Self> {
        let bursts = match kind {
            "left" => vec![0],
            "right" => vec![1],
            "all" => vec![0, 1],
            "around" => vec![2],
            _ => return None,
        };
        Some(Self { bursts, frame: 0, rng: 0x1234_5678, b: [0.0; 3] })
    }

    fn length(&self) -> u64 {
        (self.bursts.iter().map(|b| if *b == 2 { AROUND } else { BURST }).sum::<f64>() * TEST_RATE) as u64
    }

    // One stereo frame, or None once the test is over.
    fn next(&mut self) -> Option<[f32; 2]> {
        let t = self.frame as f64 / TEST_RATE;
        self.frame += 1;
        if self.frame > self.length() {
            return None;
        }
        let mut at = t;
        let mut speaker = 0;
        let mut local = 0.0;
        for b in &self.bursts {
            let length = if *b == 2 { AROUND } else { BURST };
            if at < length {
                speaker = *b;
                local = at / length;
                break;
            }
            at -= length;
        }
        // Paul Kellet's pink filter over white noise.
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let white = (self.rng >> 8) as f32 / 8_388_608.0 - 1.0;
        self.b[0] = 0.99765 * self.b[0] + white * 0.099_046;
        self.b[1] = 0.963 * self.b[1] + white * 0.296_516_4;
        self.b[2] = 0.57 * self.b[2] + white * 1.052_691_3;
        let pink = (self.b[0] + self.b[1] + self.b[2] + white * 0.184_8) * 0.11;
        // A short fade at each end of a burst keeps it from clicking.
        let within = if speaker == 2 { local * AROUND } else { at };
        let burst = if speaker == 2 { AROUND } else { BURST };
        let fade = (within.min(burst - within) / 0.02).clamp(0.0, 1.0) as f32;
        let level = pink * fade;
        Some(match speaker {
            0 => [level, 0.0],
            1 => [0.0, level],
            _ => {
                // Round the room and back: equal-power pan from the left speaker to the right and home.
                let angle = (1.0 - (local * 2.0 - 1.0).abs()) as f32 * std::f32::consts::FRAC_PI_2;
                [level * angle.cos(), level * angle.sin()]
            }
        })
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
                for frame in scratch.chunks_mut(2) {
                    let pair = if done { None } else { test.next() };
                    done |= pair.is_none();
                    let [left, right] = pair.unwrap_or([0.0, 0.0]);
                    frame[0] = left;
                    frame[1] = right;
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
                live.fill(scratch, status);
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
}

impl DynamicOutput {
    pub fn new(router: Arc<SharedAudioRouter>) -> Self {
        Self {
            router,
            shell_block: [0.0; FRAMES * 2],
            shell_pos: FRAMES * 2,
        }
    }

    pub fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T], status: &Status) {
        if let Ok(mut test_guard) = self.router.test.try_lock() {
            if let Some(test) = test_guard.as_mut() {
                let mut done = false;
                for frame in data.chunks_mut(2) {
                    let pair = if done { None } else { test.next() };
                    done |= pair.is_none();
                    let [left, right] = pair.unwrap_or([0.0, 0.0]);
                    for (i, sample) in frame.iter_mut().enumerate() {
                        *sample = T::from_sample(if i == 0 { left } else { right });
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
                live.fill(data, status);
                return;
            }
        }
        if let Ok(mut shell_guard) = self.router.shell.try_lock() {
            if let Some(shell) = shell_guard.as_mut() {
                let mut peak = 0.0f32;
                for sample in data {
                    if self.shell_pos >= self.shell_block.len() {
                        shell.render_interleaved(&mut self.shell_block);
                        status.blocks.fetch_add(1, Ordering::Relaxed);
                        self.shell_pos = 0;
                    }
                    let val = self.shell_block[self.shell_pos];
                    peak = peak.max(val.abs());
                    *sample = T::from_sample(val);
                    self.shell_pos += 1;
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
            Self::Live(s) => s.fill(data, status),
            Self::Dynamic(d) => d.fill(data, status),
        }
    }
}
// Keep the stream alive for the complete window lifetime. Dropping it stops
// callbacks before the last shared diagnostic reference is released.
pub struct AudioDevice {
    stream: cpal::Stream,
    pub status: Arc<Status>,
}
impl AudioDevice {
    // Startup negotiates one supported sample format. Runtime errors are
    // visible to the window host; they never choose another render chain.
    pub fn open(pcm: Vec<f32>, rate: u32) -> Result<Self, String> {
        Self::open_output(DeviceOutput::Pcm(MasterOutput::new(rate,pcm)?),rate)
    }
    pub fn open_sound(sound: crate::sound::SoundOutput) -> Result<Self,String> {
        Self::open_output(DeviceOutput::Sound(sound),48000)
    }
    pub fn open_live(output:crate::online::sound::LiveOutput)->Result<Self,String>{Self::open_output(DeviceOutput::Live(output),48000)}
    pub fn open_dynamic(router: Arc<SharedAudioRouter>) -> Result<Self, String> {
        Self::open_output(DeviceOutput::Dynamic(DynamicOutput::new(router)), 48000)
    }
    fn open_output(output: DeviceOutput, rate: u32) -> Result<Self,String> {
        let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        // Explicit rate and stereo prevent accidental speed or channel changes.
        // The loader will eventually decode directly to this admitted rate.
        let supported = device.supported_output_configs().map_err(|e| e.to_string())?
            .find(|c| c.channels() == 2 && c.min_sample_rate().0 <= rate && c.max_sample_rate().0 >= rate
                && matches!(c.sample_format(), cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16))
            .ok_or("audio device does not support stereo at the requested rate")?.with_sample_rate(cpal::SampleRate(rate));
        let format = supported.sample_format();
        let config = supported.config();
        let status = Arc::new(Status::default());
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, output, &status),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, output, &status),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, output, &status),
            _ => unreachable!(),
        }?;
        let result = Self { stream, status };
        result.play()?;
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
