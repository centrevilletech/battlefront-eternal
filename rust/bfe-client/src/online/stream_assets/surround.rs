//! Render a missing surround music file from its stereo original using the
//! same direct/ambient split and speaker gains as sound preparation.
use super::Pcm;
use audio::convolution::{Complex, Fft};

const N: usize = 2048;
const HOP: usize = 512;
const RING: usize = 4096;
const RATE: f64 = 48_000.0;
const SIDE: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub(super) struct Surround {
    fft: Fft,
    window: Vec<f32>,
    input: [Vec<f32>; 2],
    spectra: [Vec<Complex>; 5],
    power: Vec<[f32; 4]>,
    mix: Vec<Vec<f32>>,
    center: i64,
    read: i64,
    next: usize,
}

impl Surround {
    pub(super) fn new(channels: usize) -> Self {
        Self {
            fft: Fft::new(N),
            window: (0..N)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
                .collect(),
            input: std::array::from_fn(|_| vec![0.; N]),
            spectra: std::array::from_fn(|_| vec![Complex::default(); N]),
            power: vec![[0.; 4]; N],
            mix: vec![vec![0.; RING]; channels],
            center: 0,
            read: 0,
            next: 0,
        }
    }

    fn window(&mut self, original: &mut Pcm, keep: usize) {
        let start = self.center - (N / 2) as i64;
        let ratio = original.rate / RATE;
        for frame in self.read..start + N as i64 {
            let at = frame as f64 * ratio;
            let index = at as usize;
            let ring = frame as usize % N;
            if index >= original.frames {
                for lane in &mut self.input {
                    lane[ring] = 0.;
                }
                continue;
            }
            let a = index.saturating_sub(1);
            let b = (index + 1).min(original.frames - 1);
            let c = (index + 2).min(original.frames - 1);
            if !original.sample.available(c) {
                original.sample.skip_to(a);
                original.sample.consumed(a);
                original.sample.feed();
            }
            let t = at.fract() as f32;
            for (ch, lane) in self.input.iter_mut().enumerate() {
                let p = original.sample.value(ch, a);
                let q = original.sample.value(ch, index);
                let r = original.sample.value(ch, b);
                let s = original.sample.value(ch, c);
                lane[ring] = q + 0.5
                    * t
                    * (r - p + t * (2. * p - 5. * q + 4. * r - s + t * (3. * (q - r) + s - p)));
            }
        }
        self.read = start + N as i64;
        for (ch, input) in self.input.iter().enumerate() {
            let bins = &mut self.spectra[ch];
            for (i, bin) in bins.iter_mut().enumerate() {
                let frame = start + i as i64;
                let x = if frame < 0 {
                    0.
                } else {
                    input[frame as usize % N]
                };
                *bin = Complex {
                    re: self.window[i] * x,
                    im: 0.,
                };
            }
            self.fft.process(bins, false);
        }
        for (i, p) in self.power.iter_mut().enumerate() {
            let l = self.spectra[0][i];
            let r = self.spectra[1][i];
            let ll = l.re * l.re + l.im * l.im;
            let rr = r.re * r.re + r.im * r.im;
            let cross = [ll, rr, l.re * r.re + l.im * r.im, l.im * r.re - l.re * r.im];
            for (value, obs) in p.iter_mut().zip(cross) {
                *value = 0.8 * *value + 0.2 * obs;
            }
            // The preparation transform normalizes each spectrum by the Hann sum.
            let coherence =
                ((p[2] * p[2] + p[3] * p[3]) / (p[0] * p[1] + 1e-20 * 1024f32.powi(4))).sqrt();
            let ambient = (1.0 - coherence).clamp(0.0, 1.0).sqrt();
            let direct = 1.0 - ambient;
            let (left, right) = (ll.sqrt(), rr.sqrt());
            let pan = (left - right).abs() / (left + right + 1.024e-9);
            let share = 0.25 * direct * (1.0 - pan).powi(2);
            let center = Complex {
                re: (l.re + r.re) * share,
                im: (l.im + r.im) * share,
            };
            self.spectra[0][i] = Complex {
                re: l.re * direct - SIDE * center.re,
                im: l.im * direct - SIDE * center.im,
            };
            self.spectra[1][i] = Complex {
                re: r.re * direct - SIDE * center.re,
                im: r.im * direct - SIDE * center.im,
            };
            self.spectra[2][i] = center;
            self.spectra[3][i] = Complex {
                re: l.re * ambient,
                im: l.im * ambient,
            };
            self.spectra[4][i] = Complex {
                re: r.re * ambient,
                im: r.im * ambient,
            };
        }
        for bins in &mut self.spectra {
            self.fft.process(bins, true);
        }
        // FL, FR, C, LFE, then back and side pairs. LFE is empty in these stems.
        let routes: &[(usize, usize, usize, f32)] = if self.mix.len() == 6 {
            &[
                (0, 0, 0, 1.),
                (1, 1, 0, 1.),
                (2, 2, 0, 0.5),
                (3, 4, 576, SIDE),
                (4, 5, 720, SIDE),
            ]
        } else {
            &[
                (0, 0, 0, 1.),
                (1, 1, 0, 1.),
                (2, 2, 0, 0.5),
                (3, 4, 1104, 0.5),
                (4, 5, 1296, 0.5),
                (3, 6, 480, SIDE),
                (4, 7, 624, SIDE),
            ]
        };
        for &(plane, channel, delay, gain) in routes {
            let lane = &mut self.mix[channel];
            let bins = &self.spectra[plane];
            for (i, bin) in bins.iter().enumerate() {
                let frame = start + i as i64;
                let output = frame + delay as i64;
                if frame >= 0 && output >= keep as i64 {
                    lane[output as usize % RING] += bin.re * self.window[i] * (gain / 1.5);
                }
            }
        }
        self.center += HOP as i64;
    }

    pub(super) fn render(
        &mut self,
        original: &mut Pcm,
        start: usize,
        frames: usize,
        out: &mut [f32],
    ) {
        if start.saturating_sub(self.next) > 12 * HOP + N {
            // Reconstruct the coherence history and overlap on a late switch or seek.
            self.center = (start.saturating_sub(12 * HOP) / HOP * HOP) as i64;
            self.read = (self.center - (N / 2) as i64).max(0);
            for buf in &mut self.input {
                buf.fill(0.);
            }
            for buf in &mut self.mix {
                buf.fill(0.);
            }
            self.power.fill([0.; 4]);
        }
        // For a short forward seek consume the intervening overlap in order.
        let mut at = self.next.min(start);
        if start - at > 12 * HOP + N {
            at = start;
        }
        while at < start + frames {
            let end = (at + HOP - at % HOP).min(start + frames);
            while self.center - ((N / 2) as i64) < end as i64 {
                self.window(original, at);
            }
            for (ch, lane) in self.mix.iter_mut().enumerate() {
                for i in at..end {
                    let sample = std::mem::take(&mut lane[i % RING]);
                    if i >= start {
                        out[ch * frames + i - start] = sample;
                    }
                }
            }
            at = end;
        }
        self.next = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_centered_stereo_original_uses_the_prepared_speaker_levels() {
        for channels in [6, 8] {
            let frames = 8192;
            let sample = audio::voices::Sample {
                data: vec![0.5; frames * 2],
                rate: RATE,
                channels: 2,
                frames,
                ring: None,
                feed: None,
            };
            let mut pcm = Pcm {
                sample,
                rate: RATE,
                channels: 2,
                frames,
            };
            let mut out = vec![0.; frames * channels];
            Surround::new(channels).render(&mut pcm, 0, frames, &mut out);
            for (c, lane) in out.chunks_exact(frames).enumerate() {
                let expected = match c {
                    0 | 1 => 0.5 - 0.25 * SIDE,
                    2 => 0.125,
                    _ => 0.,
                };
                assert!(
                    (lane[4096] - expected).abs() < 0.001,
                    "speaker {c}: {}",
                    lane[4096]
                );
            }
        }
    }
}
