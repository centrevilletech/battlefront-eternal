//! Chromium's IRCAM composite headphone decoder, translated from its
//! HRTFDatabase, HRTFKernel and HRTFPanner (BSD-3-Clause; see
//! THIRD-PARTY-NOTICES.md). The database and every voice's convolution and
//! delay buffers are built before rendering. No browser panner is involved.

use crate::convolution::{BufferedConvolver, Complex, Fft};
use crate::{host, FRAMES};
use std::f64::consts::PI;

const AZIMUTHS: usize = 192;
const ELEVATIONS: usize = 10;
const RAW: &[u8] = include_bytes!("../data/irc-composite.f32");
const MAX_ELEVATIONS: [i32; 24] = [
    90, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45,
];
const ELEVATION_ORDER: [i32; 10] = [0, 15, 30, 45, 60, 75, 90, -45, -30, -15];

pub struct Kernel {
    pub spectrum: Vec<Complex>,
    pub delay: f32,
}
pub struct Database {
    pub plan: Fft,
    pub size: usize,
    kernels: Vec<Kernel>,
}

// The resource loader uses the 32-tap, 32-offset sinc kernel. This is
// loading-time conversion, not a render-quantum operation. Zero extension
// and the Blackman cutoff match AudioBus's resource sample-rate conversion.
fn resample(input: &[f32], rate: f64) -> Vec<f32> {
    if rate == 44100.0 {
        return input.to_vec();
    }
    let ratio = 44100.0 / rate;
    let scale = if ratio > 1.0 { 0.9 / ratio } else { 0.9 };
    let mut kernels = [[0.0f32; 32]; 33];
    for offset in 0..=32 {
        let fraction = offset as f32 / 32.0;
        for i in 0..32 {
            let pre = std::f32::consts::PI * (i as f32 - 16.0 - fraction);
            let x = (i as f32 - fraction) / 32.0;
            let window = (0.42 - 0.5 * host::cos(2.0 * PI * x as f64) + 0.08 * host::cos(4.0 * PI * x as f64)) as f32;
            kernels[offset][i] = (window as f64 * if pre == 0.0 { scale } else { host::sin(scale * pre as f64) / pre as f64 }) as f32;
        }
    }
    let length = (input.len() as f64 / ratio) as usize;
    let mut output = vec![0.0; length];
    let mut position = 0.0f64;
    for out in &mut output {
        let index = position as isize;
        let blend = (position - index as f64) * 32.0;
        let kernel = (blend as usize).min(31);
        let x = (blend - kernel as f64) as f32;
        let (mut a, mut b) = ([0.0f32; 4], [0.0f32; 4]);
        for i in 0..32 {
            let at = index + i as isize - 16;
            let sample = if at < 0 { 0.0 } else { input.get(at as usize).copied().unwrap_or(0.0) };
            a[i % 4] += sample * kernels[kernel][i];
            b[i % 4] += sample * kernels[kernel + 1][i];
        }
        let mut lanes = [0.0; 4];
        for i in 0..4 {
            lanes[i] = a[i] * (1.0 - x) + b[i] * x;
        }
        *out = (lanes[0] + lanes[2]) + (lanes[1] + lanes[3]);
        position += ratio;
    }
    output
}

fn spectrum_parts(c: Complex) -> (f64, f64) {
    let (re, im) = (c.re as f64, c.im as f64);
    ((re * re + im * im).sqrt(), host::atan2(im, re))
}
fn polar(magnitude: f64, phase: f64) -> Complex {
    Complex {
        re: (magnitude * host::cos(phase)) as f32,
        im: (magnitude * host::sin(phase)) as f32,
    }
}
fn unwrap(mut phase: f64) -> f64 {
    if phase < -PI {
        phase += 2.0 * PI;
    }
    if phase > PI {
        phase -= 2.0 * PI;
    }
    phase
}
fn mirror(data: &mut [Complex]) {
    let n = data.len();
    for i in 1..n / 2 {
        data[n - i] = Complex {
            re: data[i].re,
            im: -data[i].im,
        };
    }
}

impl Database {
    pub fn new(rate: f64) -> Self {
        let analysis_size = (1usize << ((256.0 * rate / 44100.0).log2() as u32)).max(32);
        let size = analysis_size * 2;
        let analysis = Fft::new(analysis_size);
        let plan = Fft::new(size);
        let mut kernels = Vec::with_capacity(ELEVATIONS * AZIMUTHS * 2);
        for elevation in 0..ELEVATIONS {
            let angle = -45 + elevation as i32 * 15;
            let mut raw = Vec::with_capacity(48);
            for azimuth in 0..24 {
                let actual = angle.min(MAX_ELEVATIONS[azimuth]);
                let e = ELEVATION_ORDER.iter().position(|v| *v == actual).unwrap();
                for ear in 0..2 {
                    let mut input = [0.0; 256];
                    for (i, sample) in input.iter_mut().enumerate() {
                        let at = (((azimuth * 10 + e) * 256 + i) * 2 + ear) * 4;
                        *sample = f32::from_le_bytes(RAW[at..at + 4].try_into().unwrap());
                    }
                    let mut response = resample(&input, rate);
                    response.resize(analysis_size, 0.0);
                    let mut spectrum: Vec<Complex> = response[..analysis_size].iter().map(|v| Complex { re: *v, im: 0.0 }).collect();
                    analysis.process(&mut spectrum, false);
                    // FFTFrame packs Nyquist in DC's imaginary component.
                    let (mut sum, mut weight, mut last) = (0.0, 0.0, 0.0);
                    for i in 0..analysis_size / 2 {
                        let value = if i == 0 {
                            Complex {
                                re: spectrum[0].re,
                                im: spectrum[analysis_size / 2].re,
                            }
                        } else {
                            spectrum[i]
                        };
                        let (magnitude, phase) = spectrum_parts(value);
                        sum += magnitude * unwrap(phase - last);
                        weight += magnitude;
                        last = phase;
                    }
                    let mut delay = -(sum / weight) / (2.0 * PI / analysis_size as f64);
                    if delay > 20.0 {
                        delay -= 20.0;
                    }
                    let adjust = delay * 2.0 * PI / analysis_size as f64;
                    for i in 1..analysis_size / 2 {
                        let (m, p) = spectrum_parts(spectrum[i]);
                        spectrum[i] = polar(m, p + i as f64 * adjust);
                    }
                    spectrum[0].re = 0.0;
                    mirror(&mut spectrum);
                    analysis.process(&mut spectrum, true);
                    let fade = (rate / 4410.0) as usize;
                    let mut result = vec![Complex::default(); size];
                    for i in 0..analysis_size {
                        let level = if i >= analysis_size - fade {
                            1.0 - (i - (analysis_size - fade)) as f32 / fade as f32
                        } else {
                            1.0
                        };
                        result[i].re = spectrum[i].re * level;
                    }
                    plan.process(&mut result, false);
                    raw.push(Kernel {
                        spectrum: result,
                        delay: delay as f32,
                    });
                }
            }
            for azimuth in 0..AZIMUTHS {
                let a = azimuth / 8;
                for ear in 0..2 {
                    let first = &raw[a * 2 + ear];
                    if azimuth % 8 == 0 {
                        kernels.push(Kernel {
                            spectrum: first.spectrum.clone(),
                            delay: first.delay,
                        });
                    } else {
                        let second = &raw[((a + 1) % 24) * 2 + ear];
                        kernels.push(Self::interpolate(&plan, first, second, (azimuth % 8) as f64 / 8.0));
                    }
                }
            }
        }
        Self { plan, size, kernels }
    }
    fn interpolate(plan: &Fft, a: &Kernel, b: &Kernel, x: f64) -> Kernel {
        let n = a.spectrum.len();
        let mut spectrum = vec![Complex::default(); n];
        spectrum[0].re = ((1.0 - x) * a.spectrum[0].re as f64 + x * b.spectrum[0].re as f64) as f32;
        spectrum[n / 2].re = ((1.0 - x) * a.spectrum[n / 2].re as f64 + x * b.spectrum[n / 2].re as f64) as f32;
        let (mut phase, mut last_a, mut last_b) = (0.0, 0.0, 0.0);
        for i in 1..n / 2 {
            let (ma, pa) = spectrum_parts(a.spectrum[i]);
            let (mb, pb) = spectrum_parts(b.spectrum[i]);
            let (da, db) = (20.0 * host::log10(ma), 20.0 * host::log10(mb));
            let (mut sa, mut sb) = (1.0 - x, x);
            let threshold = if i > 16 { 5.0 } else { 2.0 };
            if da - db < -threshold && da < 0.0 {
                sa = host::pow(sa, 0.75);
                sb = 1.0 - sa;
            } else if da - db > threshold && db < 0.0 {
                sb = host::pow(sb, 0.75);
                sa = 1.0 - sb;
            }
            let (dpa, dpb) = (unwrap(pa - last_a), unwrap(pb - last_b));
            last_a = pa;
            last_b = pb;
            let change = if dpa - dpb > PI {
                sa * dpa + sb * (2.0 * PI + dpb)
            } else if dpb - dpa > PI {
                sa * (2.0 * PI + dpa) + sb * dpb
            } else {
                sa * dpa + sb * dpb
            };
            phase = unwrap(phase + change);
            spectrum[i] = polar(host::pow(10.0, 0.05 * (sa * da + sb * db)), phase);
        }
        mirror(&mut spectrum);
        plan.process(&mut spectrum, true);
        spectrum[n / 2..].fill(Complex::default());
        for c in &mut spectrum {
            c.im = 0.0;
        }
        plan.process(&mut spectrum, false);
        Kernel {
            spectrum,
            delay: ((1.0 - x) * a.delay as f64 + x * b.delay as f64) as f32,
        }
    }
    fn kernel(&self, azimuth: usize, elevation: f64, ear: usize) -> &Kernel {
        let elevation = ((elevation.clamp(-45.0, 90.0) + 45.0) / 15.0) as usize;
        &self.kernels[(elevation * AZIMUTHS + azimuth) * 2 + ear]
    }
}

struct Delay {
    data: Vec<f32>,
    at: usize,
}
impl Delay {
    fn new(rate: f64) -> Self {
        Self {
            data: vec![0.0; (rate * 0.002).ceil() as usize + FRAMES + 1],
            at: 0,
        }
    }
    fn process(&mut self, frames: f64, input: &[f32], output: &mut [f32]) {
        let len = self.data.len();
        let read = (self.at as f64 + len as f64 - frames).rem_euclid(len as f64);
        let index = read as usize;
        let fraction = (read - index as f64) as f32;
        for i in 0..FRAMES {
            self.data[(self.at + i) % len] = input[i];
        }
        for i in 0..FRAMES {
            let a = self.data[(index + i) % len];
            let b = self.data[(index + i + 1) % len];
            output[i] = a + fraction * (b - a);
        }
        self.at = (self.at + FRAMES) % len;
    }
}

pub struct Panner {
    azimuth: [usize; 2],
    elevation: [f64; 2],
    initial: bool,
    selection: usize,
    blend: f32,
    increment: f32,
    convolvers: [[BufferedConvolver; 2]; 2],
    delays: [Delay; 2],
    delayed: [[f32; FRAMES]; 2],
    scratch: [[[f32; FRAMES]; 2]; 2],
}
impl Panner {
    pub fn new(rate: f64, size: usize) -> Self {
        Self {
            azimuth: [0; 2],
            elevation: [0.0; 2],
            initial: true,
            selection: 0,
            blend: 0.0,
            increment: 0.0,
            convolvers: std::array::from_fn(|_| std::array::from_fn(|_| BufferedConvolver::new(size))),
            delays: [Delay::new(rate), Delay::new(rate)],
            delayed: [[0.0; FRAMES]; 2],
            scratch: [[[0.0; FRAMES]; 2]; 2],
        }
    }
    pub fn process(&mut self, database: &Database, rate: f64, azimuth: f64, elevation: f64, input: &[f32], output: &mut [[f32; FRAMES]; 2]) {
        let azimuth = (-azimuth).rem_euclid(360.0) / 1.875;
        let index = (azimuth as usize).min(AZIMUTHS - 1);
        let fraction = azimuth - index as f64;
        if self.initial {
            self.azimuth = [index; 2];
            self.elevation = [elevation; 2];
            self.initial = false;
        }
        let fade = if rate <= 48000.0 { 2048.0 } else { 4096.0 };
        if self.blend == 0.0 && self.selection == 0 && (index != self.azimuth[0] || elevation != self.elevation[0]) {
            self.increment = 1.0 / fade;
            self.azimuth[1] = index;
            self.elevation[1] = elevation;
        }
        if self.blend == 1.0 && self.selection == 1 && (index != self.azimuth[1] || elevation != self.elevation[1]) {
            self.increment = -1.0 / fade;
            self.azimuth[0] = index;
            self.elevation[0] = elevation;
        }
        for ear in 0..2 {
            let k0 = database.kernel(self.azimuth[0], self.elevation[0], ear);
            let k1 = database.kernel(self.azimuth[1], self.elevation[1], ear);
            let next0 = database.kernel((self.azimuth[0] + 1) % AZIMUTHS, self.elevation[0], ear);
            let next1 = database.kernel((self.azimuth[1] + 1) % AZIMUTHS, self.elevation[1], ear);
            let d0 = (1.0 - fraction) * k0.delay as f64 + fraction * next0.delay as f64;
            let d1 = (1.0 - fraction) * k1.delay as f64 + fraction * next1.delay as f64;
            let delay = (1.0 - self.blend as f64) * d0 + self.blend as f64 * d1;
            self.delays[ear].process(delay, input, &mut self.delayed[ear]);
            if self.selection == 0 || self.increment != 0.0 {
                self.convolvers[0][ear].process(&database.plan, &k0.spectrum, &self.delayed[ear], &mut self.scratch[0][ear]);
            }
            if self.selection == 1 || self.increment != 0.0 {
                self.convolvers[1][ear].process(&database.plan, &k1.spectrum, &self.delayed[ear], &mut self.scratch[1][ear]);
            }
        }
        if self.increment == 0.0 {
            for ear in 0..2 {
                output[ear].copy_from_slice(&self.scratch[self.selection][ear]);
            }
        } else {
            let mut x = self.blend;
            for i in 0..FRAMES {
                for ear in 0..2 {
                    output[ear][i] = (1.0 - x) * self.scratch[0][ear][i] + x * self.scratch[1][ear][i];
                }
                x += self.increment;
            }
            self.blend = x;
            if self.increment > 0.0 && (x - 1.0).abs() < self.increment {
                self.selection = 1;
                self.blend = 1.0;
                self.increment = 0.0;
            } else if self.increment < 0.0 && x.abs() < -self.increment {
                self.selection = 0;
                self.blend = 0.0;
                self.increment = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_decoder_matches_recorded_browser_impulses() {
        let expected = include_bytes!("../../../tests/fixtures/audio/hrtf.f32");
        let cases: [(f64, [f64; 3]); 12] = [
            (44100.0, [0.0, 0.0, -1.0]),
            (44100.0, [0.28700000047683716, 0.1908089965581894, -0.938734769821167]),
            (44100.0, [-0.6645187735557556, -0.22495105862617493, -0.7126091718673706]),
            (44100.0, [0.8658934831619263, 0.5, -0.015114227309823036]),
            (44100.0, [-0.5775319933891296, 0.6691306233406067, 0.46767619252204895]),
            (44100.0, [0.13682065904140472, -0.48480960726737976, 0.8638516664505005]),
            (48000.0, [0.0, 0.0, -1.0]),
            (48000.0, [0.28700000047683716, 0.1908089965581894, -0.938734769821167]),
            (48000.0, [-0.6645187735557556, -0.22495105862617493, -0.7126091718673706]),
            (48000.0, [0.8658934831619263, 0.5, -0.015114227309823036]),
            (48000.0, [-0.5775319933891296, 0.6691306233406067, 0.46767619252204895]),
            (48000.0, [0.13682065904140472, -0.48480960726737976, 0.8638516664505005]),
        ];
        let mut database = None;
        let mut made_for = 0.0;
        for (case, (rate, position)) in cases.iter().enumerate() {
            if *rate != made_for {
                database = Some(Database::new(*rate));
                made_for = *rate;
            }
            let db = database.as_ref().unwrap();
            let mut panner = Panner::new(*rate, db.size);
            let [x, y, z] = *position;
            let azimuth = host::atan2(x, -z) * 180.0 / PI;
            let elevation = host::atan2(y, (x * x + z * z).sqrt()) * 180.0 / PI;
            let mut output = [[0.0; FRAMES]; 2];
            let mut worst = 0.0f32;
            let mut example = (0usize, 0.0f32, 0.0f32);
            for block in 0..32 {
                let mut input = [0.0; FRAMES];
                if block == 4 {
                    input[0] = 1.0;
                }
                panner.process(db, *rate, azimuth, elevation, &input, &mut output);
                for ear in 0..2 {
                    for i in 0..FRAMES {
                        let at = (case * 8192 + ear * 4096 + block * FRAMES + i) * 4;
                        let value = f32::from_le_bytes(expected[at..at + 4].try_into().unwrap());
                        if (output[ear][i] - value).abs() > worst {
                            worst = (output[ear][i] - value).abs();
                            example = (block * FRAMES + i, output[ear][i], value);
                        }
                    }
                }
            }
            eprintln!(
                "HRTF case {case} rate {rate}: {worst}, {example:?}, front delay {}",
                db.kernel(0, 0.0, 0).delay
            );
            assert!(worst < 0.000015848932, "case {case}: {worst}");
        }
    }
}

/// PannerHandler's listener-relative geometry. Its vector normalization
/// and elevation result round to float, which matters at kernel boundaries.
pub fn angles(position: [f32; 3]) -> (f64, f64) {
    let [mut x, mut y, mut z] = position;
    let length = ((x as f64 * x as f64 + y as f64 * y as f64) + z as f64 * z as f64).sqrt() as f32;
    if length < 1.0e-10 {
        return (0.0, 0.0);
    }
    x /= length;
    y /= length;
    z /= length;
    let horizontal = (x as f64 * x as f64 + z as f64 * z as f64).sqrt() as f32;
    let (px, pz) = if horizontal > 1.0e-10 {
        (x / horizontal, z / horizontal)
    } else {
        (x, z)
    };
    let mut azimuth = (px.clamp(-1.0, 1.0) as f64).acos() * 180.0 / PI;
    if -pz < 0.0 {
        azimuth = 360.0 - azimuth;
    }
    azimuth = if azimuth <= 270.0 { 90.0 - azimuth } else { 450.0 - azimuth };
    let normalized_length = ((x as f64 * x as f64 + y as f64 * y as f64) + z as f64 * z as f64).sqrt() as f32;
    let elevation = 90.0 - (((y / normalized_length).clamp(-1.0, 1.0) as f64).acos() * 180.0 / PI) as f32 as f64;
    (azimuth, elevation)
}
