//! Chromium's IRCAM composite headphone decoder, translated from its
//! HRTFDatabase, HRTFKernel and HRTFPanner (BSD-3-Clause; see
//! THIRD-PARTY-NOTICES.md). The database and every voice's convolution and
//! delay buffers are built before rendering. No browser panner is involved.

use crate::convolution::{BufferedStereoConvolver, Complex, Fft};
use crate::{host, FRAMES};
use std::f64::consts::PI;

// The recorded IRCAM grid has 24 azimuths at 15-degree intervals.
// HRTFDatabase expands it eightfold to 1.875-degree selections. Elevation
// coverage varies by azimuth and uses the resource's original ordering.
const AZIMUTHS: usize = 192;
const ELEVATIONS: usize = 10;
const RAW: &[u8] = include_bytes!("../data/irc-composite.f32");
const MAX_ELEVATIONS: [i32; 24] = [
    90, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45, 75, 45, 60, 45,
];
const ELEVATION_ORDER: [i32; 10] = [0, 15, 30, 45, 60, 75, 90, -45, -30, -15];

// Kernel spectra have bulk delay removed; delay is restored separately
// per ear before convolution. This split lets adjacent direction changes
// interpolate arrival time without smearing the whole impulse spectrum.
pub struct Kernel {
    pub spectrum: Vec<Complex>,
    pub delay: f32,
}
// One output instance shares this read-only database across its panners.
// The worklet constructs it between render calls; kernel interpolation and
// resource conversion may allocate here but never in Panner.process.
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
    // This is AudioBus's embedded-resource conversion, distinct from decoded
    // asset conversion in audio-decode. Its coefficient intermediates round to
    // f32 at different points; merging the two would change the IRCAM database.
    let ratio = 44100.0 / rate;
    let scale = if ratio > 1.0 { 0.9 / ratio } else { 0.9 };
    let mut kernels = [[0.0f32; 32]; 33];
    for (offset, kernel) in kernels.iter_mut().enumerate() {
        let fraction = offset as f32 / 32.0;
        for (i, slot) in kernel.iter_mut().enumerate() {
            let pre = std::f32::consts::PI * (i as f32 - 16.0 - fraction);
            let x = (i as f32 - fraction) / 32.0;
            let window = (0.42 - 0.5 * host::cos(2.0 * PI * x as f64) + 0.08 * host::cos(4.0 * PI * x as f64)) as f32;
            *slot = (window as f64 * if pre == 0.0 { scale } else { host::sin(scale * pre as f64) / pre as f64 }) as f32;
        }
    }
    // Truncate the converted resource length and zero-extend the sinc window.
    // The resource is short enough that no 512-frame phase wrap is needed.
    // Its four-lane accumulation still follows the host's float summation order.
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

// HRTFKernel analyzes magnitude and phase in double precision even though
// FFT bins are floats. Preserve that widening before magnitude weighting;
// rounding the phase early changes the estimated interaural delay.
fn spectrum_parts(c: Complex) -> (f64, f64) {
    let (re, im) = (c.re as f64, c.im as f64);
    ((re * re + im * im).sqrt(), host::atan2(im, re))
}
// Return to float storage at the FFT boundary. Trigonometric evaluation
// stays in double precision, matching the host's kernel construction.
fn polar(magnitude: f64, phase: f64) -> Complex {
    Complex {
        re: (magnitude * host::cos(phase)) as f32,
        im: (magnitude * host::sin(phase)) as f32,
    }
}
// Adjacent wrapped phase values differ by at most one full turn here.
// Choose the nearest branch around +/-pi; absolute phase is less useful
// than these continuous differences for delay removal and interpolation.
fn unwrap(mut phase: f64) -> f64 {
    if phase < -PI {
        phase += 2.0 * PI;
    }
    if phase > PI {
        phase -= 2.0 * PI;
    }
    phase
}
// Real impulses require conjugate negative-frequency bins. DC and Nyquist
// are their own partners and are left intact; only interior positive bins
// are mirrored before an inverse transform.
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
        // HRTFKernel chooses the lower power of two near 256 source-rate taps,
        // not the next larger size. Keep the minimum and size rounding because
        // FFTConvolver latency and its tail window both depend on this choice.
        let analysis_size = (1usize << ((256.0 * rate / 44100.0).log2() as u32)).max(32);
        let size = analysis_size * 2;
        let analysis = Fft::new(analysis_size);
        let plan = Fft::new(size);
        let mut kernels = Vec::with_capacity(ELEVATIONS * AZIMUTHS * 2);
        for elevation in 0..ELEVATIONS {
            let angle = -45 + elevation as i32 * 15;
            let mut raw = Vec::with_capacity(48);
            for (azimuth, &max_elevation) in MAX_ELEVATIONS.iter().enumerate() {
                // The database repeats the nearest available elevation above each raw
                // measurement's coverage. ELEVATION_ORDER reflects file order, not ascending
                // angle, so use a lookup rather than indexing directly by elevation.
                let actual = angle.min(max_elevation);
                let e = ELEVATION_ORDER.iter().position(|v| *v == actual).unwrap();
                for ear in 0..2 {
                    let mut input = [0.0; 256];
                    for (i, sample) in input.iter_mut().enumerate() {
                        let at = (((azimuth * 10 + e) * 256 + i) * 2 + ear) * 4;
                        *sample = f32::from_le_bytes(RAW[at..at + 4].try_into().unwrap());
                    }
                    // Convert the resource before spectral analysis. Resize may truncate the
                    // converted impulse to the analysis span, as HRTFKernel did, before the
                    // later zero padding used for linear convolution.
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
                    // The magnitude-weighted phase slope estimates bulk group delay.
                    // HRTFKernel retains a twenty-frame lead-in when possible; removing the
                    // entire delay would shift the browser's impulse onset.
                    let mut delay = -(sum / weight) / (2.0 * PI / analysis_size as f64);
                    if delay > 20.0 {
                        delay -= 20.0;
                    }
                    // Rotate phase to remove only that bulk delay, then clear DC. The final
                    // per-ear Delay restores the removed part during playback while the kernel
                    // retains its short lead-in and direction-dependent spectral detail.
                    let adjust = delay * 2.0 * PI / analysis_size as f64;
                    for (i, bin) in spectrum.iter_mut().enumerate().take(analysis_size / 2).skip(1) {
                        let (m, p) = spectrum_parts(*bin);
                        *bin = polar(m, p + i as f64 * adjust);
                    }
                    spectrum[0].re = 0.0;
                    mirror(&mut spectrum);
                    analysis.process(&mut spectrum, true);
                    // Fade roughly the last ten source-rate samples before zero padding.
                    // This softens the truncated impulse edge; fading the padded tail instead
                    // would leave the discontinuity in the actual response unchanged.
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
            // Exact measured directions keep their original spectra. Only the seven
            // intermediate directions are reconstructed, with wrap from the last raw
            // azimuth to zero; no interpolation allocation occurs during rendering.
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
    // FFTFrame.createInterpolatedFrame blends magnitudes in dB and continuous
    // phase differences, rather than linearly blending complex bins. This
    // preserves the shape of the ear's directional notches between measurements.
    fn interpolate(plan: &Fft, a: &Kernel, b: &Kernel, x: f64) -> Kernel {
        let n = a.spectrum.len();
        let mut spectrum = vec![Complex::default(); n];
        spectrum[0].re = ((1.0 - x) * a.spectrum[0].re as f64 + x * b.spectrum[0].re as f64) as f32;
        spectrum[n / 2].re = ((1.0 - x) * a.spectrum[n / 2].re as f64 + x * b.spectrum[n / 2].re as f64) as f32;
        let (mut phase, mut last_a, mut last_b) = (0.0, 0.0, 0.0);
        for (i, bin) in spectrum.iter_mut().enumerate().take(n / 2).skip(1) {
            let (ma, pa) = spectrum_parts(a.spectrum[i]);
            let (mb, pb) = spectrum_parts(b.spectrum[i]);
            let (da, db) = (20.0 * host::log10(ma), 20.0 * host::log10(mb));
            let (mut sa, mut sb) = (1.0 - x, x);
            // Favor a deeper below-zero notch when the two magnitudes differ enough.
            // The two/five dB threshold and 0.75 weight exponent are intentional browser
            // constants; ordinary linear magnitude interpolation fills those notches.
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
            // Blend neighboring phase increments on a common branch before accumulating.
            // Interpolating wrapped absolute phases would jump by a turn near +/-pi
            // and introduce a different time-domain impulse.
            let change = if dpa - dpb > PI {
                sa * dpa + sb * (2.0 * PI + dpb)
            } else if dpb - dpa > PI {
                sa * (2.0 * PI + dpa) + sb * dpb
            } else {
                sa * dpa + sb * dpb
            };
            phase = unwrap(phase + change);
            *bin = polar(host::pow(10.0, 0.05 * (sa * da + sb * db)), phase);
        }
        mirror(&mut spectrum);
        plan.process(&mut spectrum, true);
        // Projection back to a real, half-length impulse removes interpolation
        // energy in the padded half. Transform again only after clearing that tail;
        // otherwise circular energy would leak into later audio blocks.
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
    // HRTFDatabase quantizes elevation downward into fifteen-degree slices.
    // It does not interpolate between slices at render time. Preserve clamp
    // and truncation at the grid boundaries to match native kernel selection.
    fn kernel(&self, azimuth: usize, elevation: f64, ear: usize) -> &Kernel {
        let elevation = ((elevation.clamp(-45.0, 90.0) + 45.0) / 15.0) as usize;
        &self.kernels[(elevation * AZIMUTHS + azimuth) * 2 + ear]
    }
}

// Each ear owns an independent fractional DelayDSPKernel-style ring.
// The ring includes a quantum plus a neighbor beyond the maximum two ms;
// Panner.process can change delay without allocating or shifting history.
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
        // Compute the read origin before advancing the write cursor. Write the
        // whole input quantum before reading so a short delay can reference current
        // samples; interpolate neighboring floats at one block-constant fraction.
        let read = (self.at as f64 + len as f64 - frames).rem_euclid(len as f64);
        let index = read as usize;
        let fraction = (read - index as f64) as f32;
        for (i, &sample) in input.iter().enumerate() {
            self.data[(self.at + i) % len] = sample;
        }
        for (i, out) in output.iter_mut().enumerate() {
            let a = self.data[(index + i) % len];
            let b = self.data[(index + i + 1) % len];
            *out = a + fraction * (b - a);
        }
        self.at = (self.at + FRAMES) % len;
    }
}

// HRTFPanner holds two persistent convolution banks. One is audible while
// the other receives a new direction, then both run during a crossfade.
// All ear delays and scratch are private to this bound source.
pub struct Panner {
    azimuth: [usize; 2],
    elevation: [f64; 2],
    initial: bool,
    selection: usize,
    blend: f32,
    increment: f32,
    convolvers: [BufferedStereoConvolver; 2],
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
            convolvers: std::array::from_fn(|_| BufferedStereoConvolver::new(size)),
            delays: [Delay::new(rate), Delay::new(rate)],
            delayed: [[0.0; FRAMES]; 2],
            scratch: [[[0.0; FRAMES]; 2]; 2],
        }
    }
    pub fn process(&mut self, database: &Database, rate: f64, azimuth: f64, elevation: f64, input: &[f32], output: &mut [[f32; FRAMES]; 2]) {
        // PannerHandler's azimuth sign is opposite the database's clockwise grid.
        // Wrap before dividing so directions crossing zero use adjacent kernels,
        // not a discontinuity from a negative index.
        let azimuth = (-azimuth).rem_euclid(360.0) / 1.875;
        let index = (azimuth as usize).min(AZIMUTHS - 1);
        let fraction = azimuth - index as f64;
        // A newly created PannerNode starts at the requested direction without a
        // fade from an arbitrary default. Seed both banks together on its first
        // quantum so their initial histories and selection agree.
        if self.initial {
            self.azimuth = [index; 2];
            self.elevation = [elevation; 2];
            self.initial = false;
        }
        // Native HRTFPanner uses 2048 samples through 48 kHz, 4096 above it.
        // Update only the inactive bank at a fade endpoint; movement during a fade
        // does not restart or retarget the active convolution halfway through.
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
            // Arrival delay interpolates between neighboring azimuths and then across
            // the two bank selections. Spectral kernels remain on the dense grid; delay
            // has a continuous fraction even while the selected spectrum is discrete.
            let d0 = (1.0 - fraction) * k0.delay as f64 + fraction * next0.delay as f64;
            let d1 = (1.0 - fraction) * k1.delay as f64 + fraction * next1.delay as f64;
            let delay = (1.0 - self.blend as f64) * d0 + self.blend as f64 * d1;
            self.delays[ear].process(delay, input, &mut self.delayed[ear]);
        }
        // Run both histories during a fade, but only the selected history while
        // stationary. Borrow immutable kernels from the shared database; no cloning,
        // FFT planning or buffer construction is allowed in this render path.
        for selection in 0..2 {
            if self.selection == selection || self.increment != 0.0 {
                let left = database.kernel(self.azimuth[selection], self.elevation[selection], 0);
                let right = database.kernel(self.azimuth[selection], self.elevation[selection], 1);
                self.convolvers[selection].process(
                    &database.plan,
                    &left.spectrum,
                    &right.spectrum,
                    &self.delayed,
                    &mut self.scratch[selection],
                );
            }
        }
        if self.increment == 0.0 {
            for (out_ear, src_ear) in output.iter_mut().zip(&self.scratch[self.selection]) {
                out_ear.copy_from_slice(src_ear);
            }
        } else {
            // Crossfade gains advance as f32 once per sample, shared by both ears.
            // The accumulation and endpoint snap mirror HRTFPanner's transition, avoiding
            // a different envelope from recomputing a double-precision ramp each block.
            let mut x = self.blend;
            let [out_l, out_r] = output;
            for ((l, r), ((&l0, &r0), (&l1, &r1))) in out_l
                .iter_mut()
                .zip(out_r.iter_mut())
                .zip(
                    self.scratch[0][0]
                        .iter()
                        .zip(&self.scratch[0][1])
                        .zip(self.scratch[1][0].iter().zip(&self.scratch[1][1])),
                )
            {
                *l = (1.0 - x) * l0 + x * l1;
                *r = (1.0 - x) * r0 + x * r1;
                x += self.increment;
            }
            self.blend = x;
            // Snap within one increment so float accumulation cannot strand the fade
            // just below its endpoint. Only at that endpoint may the other bank become
            // the next inactive destination for a moving source.
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

/// PannerHandler's listener-relative geometry. Its vector normalization
/// and elevation result round to float, which matters at kernel boundaries.
pub fn angles(position: [f32; 3]) -> (f64, f64) {
    let [mut x, mut y, mut z] = position;
    let length = ((x as f64 * x as f64 + y as f64 * y as f64) + z as f64 * z as f64).sqrt() as f32;
    // PannerHandler treats a coincident source/listener as straight ahead.
    // The threshold avoids division by an effectively zero float length, keeping
    // subsequent azimuth and elevation selection finite.
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
    // Resolve quadrants in the host's listener convention: forward is -Z,
    // right is +X. Keep its acos/quadrant formulation rather than replacing it
    // with atan2, whose boundary rounding can pick a neighboring grid cell.
    let mut azimuth = (px.clamp(-1.0, 1.0) as f64).acos() * 180.0 / PI;
    if -pz < 0.0 {
        azimuth = 360.0 - azimuth;
    }
    azimuth = if azimuth <= 270.0 { 90.0 - azimuth } else { 450.0 - azimuth };
    // Normalization itself rounded each component to f32. Recompute the
    // length of that rounded vector before elevation, then round the degree
    // result to float as PannerHandler does before returning its double value.
    let normalized_length = ((x as f64 * x as f64 + y as f64 * y as f64) + z as f64 * z as f64).sqrt() as f32;
    let elevation = 90.0 - (((y / normalized_length).clamp(-1.0, 1.0) as f64).acos() * 180.0 / PI) as f32 as f64;
    (azimuth, elevation)
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
                for (ear, ear_out) in output.iter().enumerate() {
                    for (i, &sample) in ear_out.iter().enumerate() {
                        let at = (case * 8192 + ear * 4096 + block * FRAMES + i) * 4;
                        let value = f32::from_le_bytes(expected[at..at + 4].try_into().unwrap());
                        if (sample - value).abs() > worst {
                            worst = (sample - value).abs();
                            example = (block * FRAMES + i, sample, value);
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
