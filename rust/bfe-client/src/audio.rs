//! Device quantum adaptation for audio/spatial.js::Renderer.build's Rust Master.
//! cpal buffers need not be multiples of the engine's 128-frame quantum. Keep
//! the unused samples, so a short callback never advances or loses DSP time.
use ::audio::{master::Master, FRAMES};
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, FromSample, SizedSample};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering}};

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
        let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        // Explicit rate and stereo prevent accidental speed or channel changes.
        // The loader will eventually decode directly to this admitted rate.
        let supported = device.supported_output_configs().map_err(|e| e.to_string())?
            .find(|c| c.channels() == 2 && c.min_sample_rate().0 <= rate && c.max_sample_rate().0 >= rate
                && matches!(c.sample_format(), cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16))
            .ok_or("audio device does not support stereo at the requested rate")?.with_sample_rate(cpal::SampleRate(rate));
        let format = supported.sample_format();
        let config = supported.config();
        let output = MasterOutput::new(rate, pcm)?;
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
    mut output: MasterOutput, status: &Arc<Status>) -> Result<cpal::Stream, String> {
    let rendered = status.clone();
    let failed = status.clone();
    device.build_output_stream(config, move |data: &mut [T], _| output.fill(data, &rendered),
        move |_| { failed.failed.store(true, Ordering::Relaxed); }, None).map_err(|e| e.to_string())
}
