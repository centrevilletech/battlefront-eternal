//! Opt-in 2005 (`GameFamily::Swbf2`) movie audio conditioning helper.
//!
//! This module is a standalone decode-thread helper and is not yet wired into
//! live front-end or battle movie playback; 2005 movies remain silent until
//! playback connects it, and 2004 (`GameFamily::Swbf1`) movies remain
//! unconditionally silent (`open` refuses `GameFamily::Swbf1` before opening a
//! decoder). Never call `Swbf2MovieAudio` on the real-time audio device callback.
//!
//! Conditioning covers:
//! - Rejecting unsupported channel counts (`channels == 0 || channels > 2`) at open time.
//! - Resampling tracks whose native rate differs from the 48 kHz mixer (such as 44.1 kHz).
//! - Soft-limiting source peaks above `LIMITER_KNEE` (`0.95`) before bus scaling so
//!   over-full-scale tracks (peaks around `1.11`) engage the soft knee cleanly.
//! - Applying the active movie bus gain (default `SHELL_MOVIES_BUS_GAIN = 0.725`,
//!   adjustable via `set_bus_gain` for `ScriptCB_SetMovieAudioBus`) and clamping
//!   to `[-PEAK_CEILING, PEAK_CEILING]`.

use crate::Source;
use bfe_family::GameFamily;
use bink_decoder::Decoder;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

/// Target sample rate of the engine's audio mixer.
pub const MIXER_RATE_HZ: u32 = 48_000;
/// Authored 2005 movie bus gain relative to full scale.
pub const SHELL_MOVIES_BUS_GAIN: f32 = 0.725;
/// Soft-limiter knee threshold applied to source samples prior to bus scaling.
pub const LIMITER_KNEE: f32 = 0.95;
/// Output peak ceiling after soft limiting and bus scaling.
pub const PEAK_CEILING: f32 = 0.99;

/// Bounded archive slice for reading a movie with audio track 0 enabled.
struct Entry {
    file: File,
    start: u64,
    length: u64,
    at: u64,
}

impl Read for Entry {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.length.saturating_sub(self.at);
        let n = (buf.len() as u64).min(left) as usize;
        if n == 0 {
            return Ok(0);
        }
        let n = self.file.read(&mut buf[..n])?;
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Entry {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(d) => self.at.checked_add_signed(d),
            SeekFrom::End(d) => self.length.checked_add_signed(d),
        };
        let at = target
            .and_then(|t| self.start.checked_add(t).map(|abs| (t, abs)))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek outside the movie"))?;
        self.file.seek(SeekFrom::Start(at.1))?;
        self.at = at.0;
        Ok(self.at)
    }
}

/// Stateful linear resampler + pre-bus soft peak limiter + bus gain targeting 48 kHz stereo PCM.
pub struct AudioConditioner {
    source_rate: u32,
    channels: usize,
    bus_gain: f32,
    phase: f64,
    prev: [f32; 2],
    has_prev: bool,
}

impl AudioConditioner {
    pub fn new(source_rate: u32, channels: usize) -> Self {
        Self {
            source_rate: source_rate.max(8_000),
            // Keep the real interleaved input stride so callers passing >2 channels
            // never desynchronize frame boundaries.
            channels: channels.max(1),
            bus_gain: SHELL_MOVIES_BUS_GAIN,
            phase: 0.0,
            prev: [0.0; 2],
            has_prev: false,
        }
    }

    /// Sets the active movie bus gain (for `ScriptCB_SetMovieAudioBus`), clamped to `[0.0, 1.0]`.
    pub fn set_bus_gain(&mut self, gain: f32) {
        if gain.is_finite() {
            self.bus_gain = gain.clamp(0.0, 1.0);
        }
    }

    /// Returns the active movie bus gain.
    pub fn bus_gain(&self) -> f32 {
        self.bus_gain
    }

    /// Conditions interleaved float PCM into 48 kHz stereo frames appended to `out`.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        let ch = self.channels;
        let frames = interleaved.len() / ch;
        if frames == 0 {
            return;
        }
        let sample_pair = |idx: usize| -> [f32; 2] {
            let l = interleaved[idx * ch];
            let r = if ch >= 2 { interleaved[idx * ch + 1] } else { l };
            [l, r]
        };

        let condition = |x: f32, gain: f32| -> f32 {
            (limit_sample(x) * gain).clamp(-PEAK_CEILING, PEAK_CEILING)
        };

        if self.source_rate == MIXER_RATE_HZ {
            out.reserve(frames * 2);
            for i in 0..frames {
                let pair = sample_pair(i);
                out.push(condition(pair[0], self.bus_gain));
                out.push(condition(pair[1], self.bus_gain));
            }
            self.prev = sample_pair(frames - 1);
            self.has_prev = true;
            return;
        }

        let step = self.source_rate as f64 / MIXER_RATE_HZ as f64;
        if !self.has_prev {
            self.prev = sample_pair(0);
            self.has_prev = true;
        }
        let est = ((frames as f64 / step).ceil() as usize + 2) * 2;
        out.reserve(est);

        while self.phase < frames as f64 {
            let idx = self.phase.floor() as isize;
            let frac = (self.phase - idx as f64) as f32;
            let a = if idx <= 0 { self.prev } else { sample_pair((idx - 1) as usize) };
            let b = sample_pair((idx as usize).min(frames - 1));
            let l = a[0] + (b[0] - a[0]) * frac;
            let r = a[1] + (b[1] - a[1]) * frac;
            out.push(condition(l, self.bus_gain));
            out.push(condition(r, self.bus_gain));
            self.phase += step;
        }
        self.phase -= frames as f64;
        self.prev = sample_pair(frames - 1);
    }
}

/// Soft knee limiter above `LIMITER_KNEE` (`0.95`) with hard ceiling at `PEAK_CEILING`.
fn limit_sample(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let ax = x.abs();
    if ax <= LIMITER_KNEE {
        return x;
    }
    let over = ax - LIMITER_KNEE;
    let compressed = LIMITER_KNEE + (PEAK_CEILING - LIMITER_KNEE) * (over / (over + 0.04));
    compressed.min(PEAK_CEILING).copysign(x)
}

/// Decoded audio track metadata and streaming reader for 2005 movies only.
/// Intended for a background decode thread, never an audio device callback.
pub struct Swbf2MovieAudio {
    decoder: Decoder<Entry>,
    conditioner: AudioConditioner,
    pub source_rate: u32,
    pub source_channels: usize,
}

impl Swbf2MovieAudio {
    /// Opens a movie's first audio track strictly for `GameFamily::Swbf2`.
    /// Returns an error for `GameFamily::Swbf1` so 2004 movies stay unconditionally silent,
    /// and rejects channel counts outside `1..=2`.
    pub fn open(family: GameFamily, source: &Source) -> Result<Self, String> {
        if family != GameFamily::Swbf2 {
            return Err("movie audio is disabled for this game family".into());
        }
        let fail = |e: io::Error| format!("{}: {e}", source.path.display());
        let mut file = File::open(&source.path).map_err(fail)?;
        file.seek(SeekFrom::Start(source.offset)).map_err(fail)?;
        let mut entry = Entry { file, start: source.offset, length: source.length, at: 0 };
        let mut head = [0u8; 8];
        entry.read_exact(&mut head).map_err(fail)?;
        let size = u64::from(u32::from_le_bytes(head[4..8].try_into().unwrap())) + 8;
        if size > source.length {
            return Err(format!("{}: movie exceeds archive entry", source.path.display()));
        }
        entry.seek(SeekFrom::Start(0)).map_err(fail)?;
        let decoder = Decoder::new(entry).map_err(|e| e.to_string())?;
        let track = decoder
            .header()
            .audio_tracks
            .first()
            .ok_or_else(|| "movie has no audio track".to_string())?;
        let source_rate = track.sample_rate;
        let source_channels = track.channels as usize;
        if source_channels == 0 || source_channels > 2 {
            return Err(format!("unsupported movie audio channel count: {source_channels}"));
        }
        Ok(Self {
            decoder,
            conditioner: AudioConditioner::new(source_rate, source_channels),
            source_rate,
            source_channels,
        })
    }

    /// Updates the movie bus gain (for `ScriptCB_SetMovieAudioBus`).
    pub fn set_bus_gain(&mut self, gain: f32) {
        self.conditioner.set_bus_gain(gain);
    }

    /// Decodes the next video frame and appends its conditioned 48 kHz stereo PCM into `out`.
    /// Returns `Ok(false)` at end of movie.
    pub fn next_frame_pcm(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        match self.decoder.next_frame().map_err(|e| e.to_string())? {
            Some(frame) => {
                if let Some(samples) = frame.audio.first() {
                    self.conditioner.process(samples, out);
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn swbf1_family_is_strictly_refused() {
        let dummy = Source { path: PathBuf::from("nonexistent.bik"), offset: 0, length: 0 };
        let err = Swbf2MovieAudio::open(GameFamily::Swbf1, &dummy).err().unwrap();
        assert!(err.contains("disabled"));
    }

    #[test]
    fn conditioner_limits_over_full_scale_peaks_before_bus_scaling() {
        let mut cond = AudioConditioner::new(48_000, 2);
        // A 1.11 peak must engage the 0.95 knee BEFORE applying 0.725 bus scaling.
        let src = [1.11f32, -1.11, 0.5, -0.5];
        let mut out = Vec::new();
        cond.process(&src, &mut out);
        assert_eq!(out.len(), 4);
        let raw_scaled = 1.11 * SHELL_MOVIES_BUS_GAIN;
        let knee_scaled = LIMITER_KNEE * SHELL_MOVIES_BUS_GAIN;
        let ceiling_scaled = PEAK_CEILING * SHELL_MOVIES_BUS_GAIN;
        assert!(
            out[0] > knee_scaled && out[0] <= ceiling_scaled && out[0] < raw_scaled - 0.05,
            "1.11 peak must engage soft limiter before bus gain: got {} (raw scaled {})",
            out[0],
            raw_scaled
        );
        assert!((out[1] + out[0]).abs() < 1e-6);
        assert!((out[2] - 0.5 * SHELL_MOVIES_BUS_GAIN).abs() < 1e-6);
    }

    #[test]
    fn conditioner_resamples_44100_to_48000_respects_bus_gain_and_limits_overs() {
        let mut cond = AudioConditioner::new(44_100, 2);
        cond.set_bus_gain(1.0);
        assert_eq!(cond.bus_gain(), 1.0);
        let frames_in = 4_410; // 0.1 s at 44.1 kHz
        let mut src = Vec::with_capacity(frames_in * 2);
        for i in 0..frames_in {
            let t = i as f32 / 44_100.0;
            let v = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 1.11;
            src.push(v);
            src.push(-v);
        }
        let mut out = Vec::new();
        cond.process(&src, &mut out);
        let frames_out = out.len() / 2;
        assert!((frames_out as isize - 4_800).abs() <= 4, "expected ~4800 frames, got {frames_out}");
        let peak = out.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        assert!(peak <= PEAK_CEILING && peak > LIMITER_KNEE, "peak {peak} must sit between knee and ceiling");
    }

    #[test]
    fn conditioner_preserves_multichannel_input_stride_without_desync() {
        let mut cond = AudioConditioner::new(48_000, 6);
        cond.set_bus_gain(1.0);
        // Two 6-channel frames where L=0.2, R=-0.4 and surround channels are 0.9.
        let src = [0.2f32, -0.4, 0.9, 0.9, 0.9, 0.9, 0.3, -0.5, 0.9, 0.9, 0.9, 0.9];
        let mut out = Vec::new();
        cond.process(&src, &mut out);
        assert_eq!(out.len(), 4);
        assert!((out[0] - 0.2).abs() < 1e-6);
        assert!((out[1] + 0.4).abs() < 1e-6);
        assert!((out[2] - 0.3).abs() < 1e-6);
        assert!((out[3] + 0.5).abs() < 1e-6);
    }
}
