//! Allocation-free convolution for the headphone decoders. The FFT plan,
//! impulse spectra, history and scratch belong to the decoder, never to a
//! render block. A partition is one worklet quantum, giving no added delay.

use crate::{host, FRAMES};

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}
// Keep complex products as four float multiplies and two float additions.
// The stored precision and accumulation order are part of the headphone
// response; fused or reassociated arithmetic need not produce the same PCM.
impl Complex {
    pub fn product(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
}

// The plan is immutable after construction and can be reused for multiple
// ear histories. Bit-reversal indices and twiddle roots are computed once,
// so no transform allocates or calls trigonometry during rendering.
pub struct Fft {
    len: usize,
    reverse: Vec<usize>,
    // Each stage's roots in a run of their own (stage of size 2^k: the
    // 2^(k-1) roots it takes from the table above, in the order it takes
    // them), and the same conjugated for the inverse transform.
    stages: Vec<Complex>,
    inverse_stages: Vec<Complex>,
}
// The transform's roots, computed in double precision and stored as floats.
fn roots(len: usize) -> Vec<Complex> {
    (0..len / 2)
        .map(|i| {
            let angle = -2.0 * std::f64::consts::PI * i as f64 / len as f64;
            Complex {
                re: host::cos(angle) as f32,
                im: host::sin(angle) as f32,
            }
        })
        .collect()
}
impl Fft {
    // Radix two requires a power-of-two size. Compute roots in double
    // precision, then store them as floats as the former FFTFrame did; later
    // butterflies operate entirely on those stored coefficients.
    pub fn new(len: usize) -> Self {
        assert!(len.is_power_of_two());
        let bits = len.trailing_zeros();
        let reverse = (0..len)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        let roots = roots(len);
        let mut stages: Vec<Complex> = Vec::with_capacity(len);
        let mut size = 2;
        while size <= len {
            stages.extend(roots.iter().step_by(len / size).take(size / 2));
            size *= 2;
        }
        // Conjugating the root changes transform direction; the butterflies
        // keep the same order in both directions.
        let inverse_stages = stages.iter().map(|w| Complex { re: w.re, im: -w.im }).collect();
        Self {
            len,
            reverse,
            stages,
            inverse_stages,
        }
    }
    // Transform the caller's scratch in place. Construction and validation
    // establish the size; every stage stays inside that same span. Inverse
    // normalization is a separate float multiply after the butterflies.
    pub fn process(&self, data: &mut [Complex], inverse: bool) {
        assert_eq!(data.len(), self.len);
        // Swap each bit-reversed pair only once. This permutation precedes the
        // iterative stages and needs no second transform-sized work buffer.
        for i in 0..self.len {
            let j = self.reverse[i];
            if j > i {
                data.swap(i, j);
            }
        }
        // Stages combine adjacent transforms of successively doubled size. A
        // stage of `size` takes every (len / size)th root of the full plan's
        // table, so each stage borrows the same coefficients. Every butterfly
        // keeps its f32 product and sums, in the same order, whatever the loop
        // shape: the transform's values do not change.
        let stages = if inverse { &self.inverse_stages } else { &self.stages };
        let mut size = 2;
        while size <= self.len {
            let half = size / 2;
            let roots = &stages[half - 1..size - 1];
            for block in data.chunks_exact_mut(size) {
                let (low, high) = block.split_at_mut(half);
                let (low, high) = (&mut low[..half], &mut high[..half]);
                for j in 0..half {
                    let x = low[j];
                    let y = high[j].product(roots[j]);
                    low[j] = Complex {
                        re: x.re + y.re,
                        im: x.im + y.im,
                    };
                    high[j] = Complex {
                        re: x.re - y.re,
                        im: x.im - y.im,
                    };
                }
            }
            size *= 2;
        }
        // Apply inverse normalization after all sums, matching the transform's
        // stored float boundaries. Folding the reciprocal into roots or kernels
        // would round products differently even though the algebra is equivalent.
        if inverse {
            let scale = 1.0 / self.len as f32;
            for c in data {
                c.re *= scale;
                c.im *= scale;
            }
        }
    }
}

// Pack two real output spectra into one complex inverse transform. The
// positive bins determine the conjugate negative bins of each ear. This
// keeps the ears independent while avoiding a second inverse FFT.
fn pack(left: Complex, right: Complex) -> (Complex, Complex) {
    (
        Complex {
            re: left.re - right.im,
            im: left.im + right.re,
        },
        Complex {
            re: left.re + right.im,
            im: -left.im + right.re,
        },
    )
}

/// One mono feed into two ears. Both filters share the input FFT/history;
/// their output spectra share a complex inverse FFT. All scratch is owned.
pub struct StereoConvolver {
    plan: Fft,
    kernels: Vec<[Vec<Complex>; 2]>,
    history: Vec<Vec<Complex>>,
    sums: [Vec<Complex>; 2],
    scratch: Vec<Complex>,
    overlap: [Complex; FRAMES],
    at: usize,
}
impl StereoConvolver {
    // SADIE impulse pairs can exceed one quantum. Split them into 128-frame
    // partitions, zero-pad each to 256, and retain their spectra for the lifetime
    // of this decoder. Construction happens in a configuration message.
    pub fn new(left: &[f32], right: &[f32]) -> Self {
        let len = 2 * FRAMES;
        let plan = Fft::new(len);
        // Allow different ear lengths by zero-extending the shorter filter.
        // Even an empty impulse keeps one silent partition so history and modulo
        // indexing remain valid without a special render-time allocation path.
        let count = left.len().max(right.len()).div_ceil(FRAMES).max(1);
        let mut kernels = Vec::with_capacity(count);
        for part in 0..count {
            kernels.push(std::array::from_fn(|ear| {
                let impulse = if ear == 0 { left } else { right };
                let mut data = vec![Complex::default(); len];
                for (i, slot) in data.iter_mut().enumerate().take(FRAMES) {
                    slot.re = impulse.get(part * FRAMES + i).copied().unwrap_or(0.0);
                }
                plan.process(&mut data, false);
                data
            }));
        }
        Self {
            plan,
            kernels,
            history: vec![vec![Complex::default(); len]; count],
            sums: std::array::from_fn(|_| vec![Complex::default(); FRAMES + 1]),
            scratch: vec![Complex::default(); len],
            overlap: [Complex::default(); FRAMES],
            at: 0,
        }
    }
    // One mono feed supplies both ears. The newest input spectrum replaces
    // one history slot, and each impulse partition selects the correspondingly
    // older quantum. History depth depends only on impulse length.
    pub fn process(&mut self, input: &[f32], output: &mut [[f32; FRAMES]; 2]) {
        let len = 2 * FRAMES;
        let current = &mut self.history[self.at];
        for i in 0..FRAMES {
            current[i] = Complex {
                re: input[i],
                im: 0.0,
            };
        }
        // Zero padding makes the frequency-domain product a linear convolution
        // for this partition. Its upper half is carried into the next output block,
        // rather than wrapping around into the current block as circular aliasing.
        current[FRAMES..].fill(Complex::default());
        self.plan.process(current, false);
        for sum in &mut self.sums {
            sum.fill(Complex::default());
        }
        // Accumulate partitions in increasing order, separately for each ear.
        // Only nonnegative bins are needed for real signals; pack reconstructs the
        // negative-frequency partners for the shared inverse transform.
        for (part, kernels) in self.kernels.iter().enumerate() {
            let history = &self.history[(self.at + self.history.len() - part) % self.history.len()];
            for (sum_ear, kernel_ear) in self.sums.iter_mut().zip(kernels) {
                for ((sum, &hist), &kern) in sum_ear[..=FRAMES].iter_mut().zip(&history[..=FRAMES]).zip(&kernel_ear[..=FRAMES]) {
                    let p = hist.product(kern);
                    sum.re += p.re;
                    sum.im += p.im;
                }
            }
        }
        // DC and Nyquist are self-conjugate endpoints. Write those bins once;
        // only interior bins have distinct negative-frequency destinations in the
        // packed transform, avoiding overwriting an endpoint with its partner.
        for i in 0..=FRAMES {
            let (positive, negative) = pack(self.sums[0][i], self.sums[1][i]);
            self.scratch[i] = positive;
            if i > 0 && i < FRAMES {
                self.scratch[len - i] = negative;
            }
        }
        // The inverse transform's real lane is left and imaginary lane is right.
        // Add the previous upper half before replacing overlap with this block's
        // tail. All spectra, sums and overlap storage were allocated in new.
        self.plan.process(&mut self.scratch, true);
        let [out_l, out_r] = output;
        for i in 0..FRAMES {
            out_l[i] = self.scratch[i].re + self.overlap[i].re;
            out_r[i] = self.scratch[i].im + self.overlap[i].im;
            self.overlap[i] = self.scratch[i + FRAMES];
        }
        // Advance the spectrum history after both ears consume the current block.
        // No sample history is shifted: partition age is resolved through the ring
        // index, keeping render work independent of memory movement.
        self.at = (self.at + 1) % self.history.len();
    }
}

/// HRTF FFTConvolver buffers a half FFT, adding that much latency. The
/// ear delays differ, so pack the two real time signals into one complex
/// forward transform and unpack its spectra before applying ear kernels.
pub struct BufferedStereoConvolver {
    input: Vec<Complex>,
    output: Vec<Complex>,
    scratch: Vec<Complex>,
    overlap: Vec<Complex>,
    at: usize,
}
impl BufferedStereoConvolver {
    // IRCAM HRTFPanner's FFTConvolver collects a half-transform before
    // publishing it. Keep that latency instead of substituting the SADIE
    // partitioner; source arrival time is part of Chromium's response.
    pub fn new(len: usize) -> Self {
        Self {
            input: vec![Complex::default(); len],
            output: vec![Complex::default(); len],
            scratch: vec![Complex::default(); len],
            overlap: vec![Complex::default(); len / 2],
            at: 0,
        }
    }
    /// `frames` of silent input after a silence longer than the response:
    /// only the partition's position moves on (every buffer already holds
    /// zeros, and a transform of zeros is zeros).
    pub fn idle(&mut self, frames: usize) {
        self.at = (self.at + frames) % self.overlap.len();
    }
    pub fn process(
        &mut self,
        plan: &Fft,
        left: &[Complex],
        right: &[Complex],
        input: &[[f32; FRAMES]; 2],
        output: &mut [[f32; FRAMES]; 2],
    ) {
        let half = self.overlap.len();
        let len = 2 * half;
        for i in 0..FRAMES {
            self.input[self.at] = Complex {
                re: input[0][i],
                im: input[1][i],
            };
            // Emit the previously completed half while collecting the next input.
            // The two ears can have different fractional delays before this stage, so
            // input is a packed pair of real signals, not duplicated mono PCM.
            output[0][i] = self.output[self.at].re;
            output[1][i] = self.output[self.at].im;
            self.at += 1;
            if self.at == half {
                self.output.copy_from_slice(&self.input);
                plan.process(&mut self.output, false);
                // Separate the two real spectra using conjugate partners from the packed
                // forward transform. Filter each ear independently, then pack them again
                // for one inverse transform; DC and Nyquist are included exactly once.
                for k in 0..=half {
                    let a = self.output[k];
                    let b = self.output[if k == 0 { 0 } else { len - k }];
                    let l = Complex {
                        re: (a.re + b.re) * 0.5,
                        im: (a.im - b.im) * 0.5,
                    }
                    .product(left[k]);
                    let r = Complex {
                        re: (a.im + b.im) * 0.5,
                        im: (b.re - a.re) * 0.5,
                    }
                    .product(right[k]);
                    let (positive, negative) = pack(l, r);
                    self.scratch[k] = positive;
                    if k > 0 && k < half {
                        self.scratch[len - k] = negative;
                    }
                }
                plan.process(&mut self.scratch, true);
                // Overlap-add retains the full linear response while publishing only the
                // next half-transform. Both banks have independent overlap during a panner
                // crossfade; sharing it would mix old and new kernel histories.
                for k in 0..half {
                    self.output[k] = Complex {
                        re: self.scratch[k].re + self.overlap[k].re,
                        im: self.scratch[k].im + self.overlap[k].im,
                    };
                    self.overlap[k] = self.scratch[k + half];
                }
                self.at = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The transform as first written (a root index and direction test per
    // butterfly): the loops' shape changed, not one value.
    fn reference(plan: &Fft, data: &mut [Complex], inverse: bool) {
        let roots = roots(plan.len);
        for i in 0..plan.len {
            let j = plan.reverse[i];
            if j > i {
                data.swap(i, j);
            }
        }
        let mut size = 2;
        while size <= plan.len {
            let half = size / 2;
            for start in (0..plan.len).step_by(size) {
                for j in 0..half {
                    let mut w = roots[j * plan.len / size];
                    if inverse {
                        w.im = -w.im;
                    }
                    let a = data[start + j];
                    let b = data[start + j + half].product(w);
                    data[start + j] = Complex { re: a.re + b.re, im: a.im + b.im };
                    data[start + j + half] = Complex { re: a.re - b.re, im: a.im - b.im };
                }
            }
            size *= 2;
        }
        if inverse {
            let scale = 1.0 / plan.len as f32;
            for c in data {
                c.re *= scale;
                c.im *= scale;
            }
        }
    }
    #[test]
    fn the_transform_gives_the_same_bits_as_first_written() {
        let mut x = 12345u32;
        let mut next = || {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 8) as f32 / (1 << 23) as f32 - 1.0
        };
        for bits in 1..=10 {
            let plan = Fft::new(1 << bits);
            for inverse in [false, true] {
                let input: Vec<Complex> = (0..plan.len).map(|_| Complex { re: next(), im: next() }).collect();
                let (mut a, mut b) = (input.clone(), input);
                plan.process(&mut a, inverse);
                reference(&plan, &mut b, inverse);
                for (a, b) in a.iter().zip(&b) {
                    assert_eq!((a.re.to_bits(), a.im.to_bits()), (b.re.to_bits(), b.im.to_bits()), "size {}", plan.len);
                }
            }
        }
    }
    #[test]
    // Unequal ear taps straddle multiple partition edges. Silent blocks after
    // the impulse must still emit the retained tail, with no left/right leakage
    // from packing both ears into one complex inverse transform.
    fn partition_boundaries_preserve_both_ear_impulses_and_tails() {
        let mut left = vec![0.0; 258];
        let mut right = left.clone();
        left[0] = 1.0;
        left[128] = 0.25;
        left[256] = -0.5;
        right[1] = 0.5;
        right[129] = -0.125;
        right[257] = 0.75;
        let mut convolver = StereoConvolver::new(&left, &right);
        let mut input = [0.0; FRAMES];
        input[0] = 1.0;
        let mut output = [[0.0; FRAMES]; 2];
        for block in 0..5 {
            convolver.process(&input, &mut output);
            input.fill(0.0);
            for (ear, ear_out) in output.iter().enumerate() {
                for (i, &sample) in ear_out.iter().enumerate() {
                    let expected = if ear == 0 {
                        left.get(block * FRAMES + i)
                    } else {
                        right.get(block * FRAMES + i)
                    }
                    .copied()
                    .unwrap_or(0.0);
                    assert!((sample - expected).abs() < 1e-6);
                }
            }
        }
    }
}
