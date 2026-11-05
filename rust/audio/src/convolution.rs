//! Allocation-free convolution for the headphone decoders. The FFT plan,
//! impulse spectra, history and scratch belong to the decoder, never to a
//! render block. A partition is one worklet quantum, giving no added delay.

use crate::{host, FRAMES};

#[derive(Clone, Copy, Default)]
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
        let reverse = (0..len).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        let roots = (0..len / 2)
            .map(|i| {
                let angle = -2.0 * std::f64::consts::PI * i as f64 / len as f64;
                Complex {
                    re: host::cos(angle) as f32,
                    im: host::sin(angle) as f32,
                }
            })
            .collect();
        Self { len, reverse, roots }
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
}

/// Uniform overlap-add partitions, one input channel into one output.
/// The current block participates immediately, as ConvolverNode's direct
/// first partition does. Numerical FFT rounding is covered by the render gate.
pub struct Convolver {
    plan: Fft,
    kernels: Vec<Vec<Complex>>,
    history: Vec<Vec<Complex>>,
    scratch: Vec<Complex>,
    overlap: [f32; FRAMES],
    at: usize,
}
impl Convolver {
    pub fn new(impulse: &[f32]) -> Self {
        let len = FRAMES * 2;
        let plan = Fft::new(len);
        let count = impulse.len().div_ceil(FRAMES).max(1);
        let mut kernels = Vec::with_capacity(count);
        for part in 0..count {
            let mut spectrum = vec![Complex::default(); len];
            for (i, c) in spectrum[..FRAMES].iter_mut().enumerate() {
                c.re = impulse.get(part * FRAMES + i).copied().unwrap_or(0.0);
            }
            plan.process(&mut spectrum, false);
            kernels.push(spectrum);
        }
        Self {
            plan,
            kernels,
            history: vec![vec![Complex::default(); len]; count],
            scratch: vec![Complex::default(); len],
            overlap: [0.0; FRAMES],
            at: 0,
        }
    }
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let len = FRAMES * 2;
        let current = &mut self.history[self.at];
        for i in 0..FRAMES {
            current[i] = Complex { re: input[i], im: 0.0 };
        }
        current[FRAMES..].fill(Complex::default());
        self.plan.process(current, false);
        self.scratch.fill(Complex::default());
        for (part, kernel) in self.kernels.iter().enumerate() {
            let history = &self.history[(self.at + self.history.len() - part) % self.history.len()];
            for i in 0..len {
                let p = history[i].product(kernel[i]);
                self.scratch[i].re += p.re;
                self.scratch[i].im += p.im;
            }
        }
        self.plan.process(&mut self.scratch, true);
        for i in 0..FRAMES {
            output[i] = self.scratch[i].re + self.overlap[i];
            self.overlap[i] = self.scratch[i + FRAMES].re;
        }
        self.at = (self.at + 1) % self.history.len();
    }
}

/// Chromium's HRTF FFTConvolver differs from ConvolverNode: a whole half
/// FFT is buffered before processing, so that half FFT is also its latency.
pub struct BufferedConvolver {
    input: Vec<Complex>,
    output: Vec<Complex>,
    overlap: Vec<f32>,
    at: usize,
}
impl BufferedConvolver {
    pub fn new(len: usize) -> Self {
        Self {
            input: vec![Complex::default(); len],
            output: vec![Complex::default(); len],
            overlap: vec![0.0; len / 2],
            at: 0,
        }
    }
    pub fn process(&mut self, plan: &Fft, kernel: &[Complex], input: &[f32], output: &mut [f32]) {
        let half = self.overlap.len();
        for i in 0..input.len() {
            self.input[self.at].re = input[i];
            output[i] = self.output[self.at].re;
            self.at += 1;
            if self.at == half {
                self.output.copy_from_slice(&self.input);
                plan.process(&mut self.output, false);
                for (c, k) in self.output.iter_mut().zip(kernel) {
                    *c = c.product(*k);
                }
                plan.process(&mut self.output, true);
                for j in 0..half {
                    self.output[j].re += self.overlap[j];
                    self.overlap[j] = self.output[j + half].re;
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
    fn partition_boundaries_preserve_the_impulse_and_its_tail() {
        let mut impulse = vec![0.0; 300];
        impulse[0] = 0.5;
        impulse[127] = -0.25;
        impulse[299] = 0.125;
        let mut c = Convolver::new(&impulse);
        let mut input = [0.0; FRAMES];
        input[0] = 1.0;
        let mut output = [0.0; FRAMES];
        for block in 0..4 {
            c.process(&input, &mut output);
            for (i, sample) in output.iter().enumerate() {
                assert!((*sample - impulse.get(block * FRAMES + i).copied().unwrap_or(0.0)).abs() < 1e-6);
            }
            input.fill(0.0);
        }
    }
}
