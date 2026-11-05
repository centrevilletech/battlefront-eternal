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
impl Complex {
    pub fn product(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
}

pub struct Fft {
    len: usize,
    reverse: Vec<usize>,
    roots: Vec<Complex>,
}
impl Fft {
    pub fn new(len: usize) -> Self {
        assert!(len.is_power_of_two());
        let bits = len.trailing_zeros();
        let reverse = (0..len)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        let roots = (0..len / 2)
            .map(|i| {
                let angle = -2.0 * std::f64::consts::PI * i as f64 / len as f64;
                Complex {
                    re: host::cos(angle) as f32,
                    im: host::sin(angle) as f32,
                }
            })
            .collect();
        Self {
            len,
            reverse,
            roots,
        }
    }
    pub fn process(&self, data: &mut [Complex], inverse: bool) {
        assert_eq!(data.len(), self.len);
        for i in 0..self.len {
            let j = self.reverse[i];
            if j > i {
                data.swap(i, j);
            }
        }
        let mut size = 2;
        while size <= self.len {
            let half = size / 2;
            // Two adjacent complex butterflies fit one SIMD register. The
            // first stage has only one butterfly and stays scalar. The
            // browser target requires SIMD; native hosts use this same
            // arithmetic below, with no alternate audio implementation.
            #[cfg(target_arch = "wasm32")]
            if half >= 2 {
                unsafe {
                    self.vector_stage(data, size, inverse);
                }
                size *= 2;
                continue;
            }
            for start in (0..self.len).step_by(size) {
                for j in 0..half {
                    let mut w = self.roots[j * self.len / size];
                    if inverse {
                        w.im = -w.im;
                    }
                    let a = data[start + j];
                    let b = data[start + j + half].product(w);
                    data[start + j] = Complex {
                        re: a.re + b.re,
                        im: a.im + b.im,
                    };
                    data[start + j + half] = Complex {
                        re: a.re - b.re,
                        im: a.im - b.im,
                    };
                }
            }
            size *= 2;
        }
        if inverse {
            let scale = 1.0 / self.len as f32;
            for c in data {
                c.re *= scale;
                c.im *= scale;
            }
        }
    }

    // Work on pairs, retaining each complex product's f32 multiply/add
    // boundaries. A stage is one internal call, rather than a call per
    // butterfly. All loads/stores are inside the validated FFT span.
    #[cfg(target_arch = "wasm32")]
    #[target_feature(enable = "simd128")]
    unsafe fn vector_stage(&self, data: &mut [Complex], size: usize, inverse: bool) {
        use core::arch::wasm32::*;
        let half = size / 2;
        let stride = self.len / size;
        let ptr = data.as_mut_ptr();
        for start in (0..self.len).step_by(size) {
            for j in (0..half).step_by(2) {
                let mut first = self.roots[j * stride];
                let mut second = self.roots[(j + 1) * stride];
                if inverse {
                    first.im = -first.im;
                    second.im = -second.im;
                }
                let real = f32x4(first.re, first.re, second.re, second.re);
                let imaginary = f32x4(-first.im, first.im, -second.im, second.im);
                let low = v128_load(ptr.add(start + j).cast());
                let high = v128_load(ptr.add(start + j + half).cast());
                let swapped = i32x4_shuffle::<1, 0, 3, 2>(high, high);
                let product = f32x4_add(f32x4_mul(high, real), f32x4_mul(swapped, imaginary));
                v128_store(ptr.add(start + j).cast(), f32x4_add(low, product));
                v128_store(ptr.add(start + j + half).cast(), f32x4_sub(low, product));
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
    pub fn new(left: &[f32], right: &[f32]) -> Self {
        let len = 2 * FRAMES;
        let plan = Fft::new(len);
        let count = left.len().max(right.len()).div_ceil(FRAMES).max(1);
        let mut kernels = Vec::with_capacity(count);
        for part in 0..count {
            kernels.push(std::array::from_fn(|ear| {
                let impulse = if ear == 0 { left } else { right };
                let mut data = vec![Complex::default(); len];
                for i in 0..FRAMES {
                    data[i].re = impulse.get(part * FRAMES + i).copied().unwrap_or(0.0);
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
    pub fn process(&mut self, input: &[f32], output: &mut [[f32; FRAMES]; 2]) {
        let len = 2 * FRAMES;
        let current = &mut self.history[self.at];
        for i in 0..FRAMES {
            current[i] = Complex {
                re: input[i],
                im: 0.0,
            };
        }
        current[FRAMES..].fill(Complex::default());
        self.plan.process(current, false);
        for sum in &mut self.sums {
            sum.fill(Complex::default());
        }
        for (part, kernels) in self.kernels.iter().enumerate() {
            let history = &self.history[(self.at + self.history.len() - part) % self.history.len()];
            for ear in 0..2 {
                for i in 0..=FRAMES {
                    let p = history[i].product(kernels[ear][i]);
                    self.sums[ear][i].re += p.re;
                    self.sums[ear][i].im += p.im;
                }
            }
        }
        for i in 0..=FRAMES {
            let (positive, negative) = pack(self.sums[0][i], self.sums[1][i]);
            self.scratch[i] = positive;
            if i > 0 && i < FRAMES {
                self.scratch[len - i] = negative;
            }
        }
        self.plan.process(&mut self.scratch, true);
        for i in 0..FRAMES {
            output[0][i] = self.scratch[i].re + self.overlap[i].re;
            output[1][i] = self.scratch[i].im + self.overlap[i].im;
            self.overlap[i] = self.scratch[i + FRAMES];
        }
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
    pub fn new(len: usize) -> Self {
        Self {
            input: vec![Complex::default(); len],
            output: vec![Complex::default(); len],
            scratch: vec![Complex::default(); len],
            overlap: vec![Complex::default(); len / 2],
            at: 0,
        }
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
            output[0][i] = self.output[self.at].re;
            output[1][i] = self.output[self.at].im;
            self.at += 1;
            if self.at == half {
                self.output.copy_from_slice(&self.input);
                plan.process(&mut self.output, false);
                for k in 0..=half {
                    let a = self.output[k];
                    let b = self.output[(len - k) % len];
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
    #[test]
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
            for ear in 0..2 {
                for i in 0..FRAMES {
                    let expected = if ear == 0 {
                        left.get(block * FRAMES + i)
                    } else {
                        right.get(block * FRAMES + i)
                    }
                    .copied()
                    .unwrap_or(0.0);
                    assert!((output[ear][i] - expected).abs() < 1e-6);
                }
            }
        }
    }
}
