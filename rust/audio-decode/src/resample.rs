//! Explicit asset-rate conversion. The 32-tap kernels and 512-frame phase
//! wrap match Blink SincResampler (BSD-3-Clause; THIRD-PARTY-NOTICES.md).
//! It keeps only the current window and one decoded frame, not a track.
use crate::Pcm;
const BLOCK: f64 = 512.0;
// The loading worker owns this state and can allocate between PCM windows.
// This is asset conversion, distinct from playback-rate interpolation on the
// audio thread. Keeping the stages separate preserves the former decoded
// buffer's length, loop offsets and playback-rate scale.
pub struct Resampler {
    channels: usize,
    ratio: f64,
    kernels: [[f32; 32]; 33],
    input: Vec<Vec<f32>>,
    // first is the absolute source index at input[at]. block and position track
    // phase independently of transfer boundaries, so changing network chunk size
    // cannot reset interpolation or accumulate a different rounding error.
    first: u64,
    at: usize,
    received: u64,
    block: u64,
    position: f64,
    pub produced: u64,
    total: Option<u64>,
    eof: bool,
    gain: f64,
}
impl Resampler {
    // Reproduce SincResampler's window and arithmetic, not just a generic
    // bandlimited converter. Thirty-three kernels bracket 32 fractional phases;
    // neighboring kernel outputs are blended after their f32 accumulation.
    pub fn new(source: u32, target: u32, channels: usize, total: Option<u64>, gain: f64) -> Self {
        let ratio = source as f64 / target as f64;
        let mut kernels = [[0.0; 32]; 33];
        if source != target {
            // Downsampling narrows the passband to prevent aliasing; upsampling keeps
            // the same 90% guard band. Blackman endpoints and coefficient f32 stores
            // match the host asset converter's impulse response.
            let cutoff = 0.9 * if ratio > 1.0 { 1.0 / ratio } else { 1.0 };
            for (offset, kernel) in kernels.iter_mut().enumerate() {
                let fraction = offset as f64 / 32.0;
                for (i, value) in kernel.iter_mut().enumerate() {
                    let x = (i as f64 - fraction) / 32.0;
                    let s = cutoff * std::f64::consts::PI * (i as f64 - 16.0 - fraction);
                    let sinc = if s == 0.0 { 1.0 } else { v8math::sin(s) / s } * cutoff;
                    let window = 0.42 - 0.5 * v8math::cos(2.0 * std::f64::consts::PI * x)
                        + 0.08 * v8math::cos(4.0 * std::f64::consts::PI * x);
                    *value = (sinc * window) as f32;
                }
            }
        }
        Self {
            channels,
            ratio,
            kernels,
            input: vec![Vec::new(); channels],
            first: 0,
            at: 0,
            received: 0,
            block: 0,
            position: 0.0,
            produced: 0,
            // Truncate the converted length just as the former AudioBuffer did. The
            // filter tail is not appended to the track; it only fills samples inside
            // this declared interval using zero extension beyond source EOF.
            total: total.map(|n| (n as f64 / ratio) as u64),
            eof: false,
            gain,
        }
    }
    // Retain only the decoded window not yet consumed by the filter. PCM
    // arrives planar and every lane shares one phase and source-frame count.
    pub fn push(&mut self, pcm: Pcm) {
        debug_assert_eq!(pcm.channels, self.channels);
        for c in 0..self.channels {
            self.input[c].extend_from_slice(&pcm.data[c * pcm.frames..(c + 1) * pcm.frames]);
        }
        self.received += pcm.frames as u64;
    }
    // Unknown-length containers derive their count only after the final frame.
    // Known lengths remain authoritative, including an early compressed-tail
    // probe for Opus. EOF permits the trailing zero-padded half-kernel.
    pub fn finish(&mut self) {
        self.eof = true;
        if self.total.is_none() {
            self.total = Some((self.received as f64 / self.ratio) as u64);
        }
    }
    pub fn finished(&self) -> bool {
        self.total.is_some_and(|total| self.produced >= total)
    }
    // Negative indices and reads beyond EOF are silence. Interior filter calls
    // bypass these checks; only boundary windows build a padded stack array.
    fn sample(&self, c: usize, index: i64) -> f32 {
        if index < 0 || (index as u64) < self.first || (index as u64) >= self.received {
            return 0.0;
        }
        self.input[c][self.at + (index as u64 - self.first) as usize]
    }
    fn filter(&self, c: usize, index: u64, kernel: usize, x: f32) -> f32 {
        // Interior windows are contiguous. Resolve history bounds once,
        // instead of checking every tap of every output sample.
        let start = index as i64 - 16;
        if start >= self.first as i64 && start as u64 + 32 <= self.received {
            let at = self.at + (start as u64 - self.first) as usize;
            let samples: &[f32; 32] = self.input[c][at..at + 32].try_into().unwrap();
            convolve(samples, &self.kernels[kernel], &self.kernels[kernel + 1], x)
        } else {
            // Only the leading/trailing half-kernels require zero padding.
            let samples = std::array::from_fn(|i| self.sample(c, start + i as i64));
            convolve(
                &samples,
                &self.kernels[kernel],
                &self.kernels[kernel + 1],
                x,
            )
        }
    }
    // Produce a bounded planar transfer. The provisional lane stride is maximum;
    // a short final transfer is compacted afterward without allocating a second
    // PCM buffer. The caller copies output before asking for another window.
    pub fn process(&mut self, maximum: usize, out: &mut Vec<f32>) -> usize {
        out.resize(maximum * self.channels, 0.0);
        let mut count = 0;
        while count < maximum && !self.finished() {
            let index = self.block + self.position as u64;
            // A sinc sample needs future taps, so pause until enough decoded input is
            // available. Unity-rate copies need no lookahead and bypass the filter to
            // preserve source-rate FLAC and WAV PCM bit for bit.
            let needed = index + if self.ratio == 1.0 { 0 } else { 15 };
            if !self.eof && needed >= self.received {
                break;
            }
            if self.ratio == 1.0 && index >= self.received {
                break;
            }
            let blend = (self.position - self.position.floor()) * 32.0;
            let kernel = (blend as usize).min(31);
            let x = (blend - kernel as f64) as f32;
            for c in 0..self.channels {
                let mut value = if self.ratio == 1.0 {
                    self.sample(c, index as i64)
                } else {
                    self.filter(c, index, kernel, x)
                };
                // Asset gain was a JS double multiply followed by a Float32Array store.
                // Keep it after conversion, and bypass unity rather than forcing another
                // multiply into the otherwise exact source-rate PCM path.
                if self.gain != 1.0 {
                    value = (value as f64 * self.gain) as f32;
                }
                out[c * maximum + count] = value;
            }
            self.position += self.ratio;
            self.produced += 1;
            count += 1;
            // Blink wraps phase every 512 source frames. Subtracting this fixed block
            // rather than letting an absolute f64 phase grow preserves its rounding
            // sequence across long tracks and unrelated output transfer sizes.
            while self.position >= BLOCK {
                self.position -= BLOCK;
                self.block += BLOCK as u64;
            }
        }
        // Preserve the left half-kernel at a block boundary, and recycle
        // input storage in batches rather than moving it every sample.
        let keep = self.block.saturating_sub(16).min(self.received);
        if keep > self.first {
            self.at += (keep - self.first) as usize;
            self.first = keep;
        }
        if self.at >= 4096 {
            for data in &mut self.input {
                data.drain(..self.at);
            }
            self.at = 0;
        }
        // A short window still has contiguous lanes at the public ABI. Later lane
        // copies move downward, so overlap is safe and no sample data is lost.
        if count < maximum {
            for c in 1..self.channels {
                out.copy_within(c * maximum..c * maximum + count, c * count);
            }
            out.truncate(count * self.channels);
        }
        count
    }
}
impl Resampler {
    // A compressed-tail probe supplies an output-rate count, already converted
    // by Decoder. Do not apply the source/target ratio a second time here.
    pub fn set_total(&mut self, total: u64) {
        self.total = Some(total);
    }
}

impl Resampler {
    // Batch several short codec frames into a transfer window. Waiting
    // here keeps packet cadence out of the audio thread's message queue.
    pub fn ready(&self, maximum: usize) -> bool {
        let last =
            self.block as f64 + self.position + (maximum.saturating_sub(1) as f64) * self.ratio;
        let needed = last.floor() as u64 + if self.ratio == 1.0 { 0 } else { 16 };
        needed < self.received
    }
}

// The browser module requires the same SIMD128 support as the output DSP.
// Four independent accumulators preserve the recorded SSE operation order;
// there is no fused multiply-add or reassociation of the horizontal sum.
#[cfg(target_arch = "wasm32")]
fn convolve(samples: &[f32; 32], ka: &[f32; 32], kb: &[f32; 32], x: f32) -> f32 {
    unsafe { convolve_simd(samples, ka, kb, x) }
}
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn convolve_simd(samples: &[f32; 32], ka: &[f32; 32], kb: &[f32; 32], x: f32) -> f32 {
    use std::arch::wasm32::*;
    let (mut a, mut b) = (f32x4_splat(0.0), f32x4_splat(0.0));
    for i in (0..32).step_by(4) {
        let sample = v128_load(samples.as_ptr().add(i).cast());
        a = f32x4_add(a, f32x4_mul(sample, v128_load(ka.as_ptr().add(i).cast())));
        b = f32x4_add(b, f32x4_mul(sample, v128_load(kb.as_ptr().add(i).cast())));
    }
    let lanes = f32x4_add(
        f32x4_mul(a, f32x4_splat(1.0 - x)),
        f32x4_mul(b, f32x4_splat(x)),
    );
    (f32x4_extract_lane::<0>(lanes) + f32x4_extract_lane::<2>(lanes))
        + (f32x4_extract_lane::<1>(lanes) + f32x4_extract_lane::<3>(lanes))
}
// Native correctness tools use the same four-lane arithmetic. The worker
// build above always executes SIMD; codec decoding stays off the audio thread.
#[cfg(not(target_arch = "wasm32"))]
fn convolve(samples: &[f32; 32], ka: &[f32; 32], kb: &[f32; 32], x: f32) -> f32 {
    let (mut a, mut b) = ([0.0f32; 4], [0.0f32; 4]);
    for i in 0..32 {
        a[i % 4] += samples[i] * ka[i];
        b[i % 4] += samples[i] * kb[i];
    }
    let lanes = std::array::from_fn::<_, 4, _>(|i| a[i] * (1.0 - x) + b[i] * x);
    (lanes[0] + lanes[2]) + (lanes[1] + lanes[3])
}
