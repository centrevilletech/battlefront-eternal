//! Rust owns the complete audio render phase: source playback, streams,
//! placement, convolution, room sends, mono and master processing. The page
//! supplies decoded PCM and context-timed commands. One worklet copies the
//! finished 128-frame quantum; the optional meter host copies readings.
//!
//! Recorded synthetic renders define the arithmetic contract: f64 state,
//! f32 stores at the former node boundaries, chronological automation and
//! deterministic transcendentals from v8math. Component entrypoints keep
//! independent golden coverage and serve native hosts without Web Audio.


pub mod commands;
pub mod adpcm;
mod signal;
mod biquad;
pub mod clarity;
pub mod convolution;
pub mod hrtf;
pub mod pcm;
pub mod streaming;
pub mod stereo;
pub mod output;
pub mod frontend;
pub mod limiter;
pub mod master;
pub mod meter;
pub mod mixer;
pub mod reverb;
pub mod stabilizer;
pub mod timeline;
pub mod voices;

/// A block of samples handed to a processor: `channels` runs of `frames`
/// f32 samples, one after another (channel-major). Inputs are read widened
/// to f64, as JavaScript reads a Float32Array.
pub struct Block<'a> {
    pub data: &'a [f32],
    pub channels: usize,
    pub frames: usize,
}

impl<'a> Block<'a> {
    #[inline(always)]
    pub fn ch(&self, c: usize) -> &'a [f32] {
        &self.data[c * self.frames..(c + 1) * self.frames]
    }
}

/// A block a processor fills. A store takes an f64 and keeps it as f32,
/// rounding as a Float32Array store in JavaScript does; `at` reads one
/// back, widened.
pub struct BlockMut<'a> {
    pub data: &'a mut [f32],
    pub channels: usize,
    pub frames: usize,
}

impl<'a> BlockMut<'a> {
    #[inline(always)]
    pub fn at(&self, c: usize, i: usize) -> f64 {
        self.data[c * self.frames + i] as f64
    }
    #[inline(always)]
    pub fn set(&mut self, c: usize, i: usize, v: f64) {
        self.data[c * self.frames + i] = v as f32;
    }
    #[inline(always)]
    pub fn add(&mut self, c: usize, i: usize, v: f64) {
        let at = c * self.frames + i;
        self.data[at] = (self.data[at] as f64 + v) as f32;
    }
    pub fn fill(&mut self, v: f32) {
        self.data.fill(v);
    }
}

/// Math.min(a, b): NaN if either is, and -0 below +0.
#[inline(always)]
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { return f64::NAN; }
    if a < b { a } else if b < a { b } else if a == 0.0 && a.is_sign_negative() { a } else { b }
}

/// Math.max(a, b): NaN if either is, and +0 above -0.
#[inline(always)]
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { return f64::NAN; }
    if a > b { a } else if b > a { b } else if a == 0.0 && a.is_sign_positive() { a } else { b }
}

/// Math.round(x): the nearest integer, halves toward positive infinity,
/// the sign of a zero kept.
#[inline(always)]
pub fn js_round(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 || !x.is_finite() { return x; }
    let r = (x + 0.5).floor();
    if r == 0.0 && x.is_sign_negative() { -0.0 } else { r }
}

/// Deterministic transcendentals, shared with the native Rust host. Audio
/// keeps the fdlibm power law used by the recorded processor fixtures.
pub mod host {
    pub use v8math::{sin, cos, tan, asin, atan2, exp, log10};
    pub use v8math::pow_legacy as pow;
    pub fn log10_each(values: &mut [f64]) { for value in values { *value = log10(*value); } }
    pub fn pow10_each(values: &mut [f64]) { for value in values { *value = pow(10.0, *value); } }
}

/// Frames per render quantum.
pub const FRAMES: usize = 128;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_max_round_keep_javascript_rules() {
        assert!(js_min(f64::NAN, 1.0).is_nan() && js_max(1.0, f64::NAN).is_nan());
        assert!(js_min(0.0, -0.0).is_sign_negative() && js_max(-0.0, 0.0).is_sign_positive());
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert!(js_round(-0.4).is_sign_negative());
        assert_eq!(js_round(4800.0), 4800.0);
    }

    fn block(data: &[f32], channels: usize, frames: usize) -> Block<'_> {
        Block { data, channels, frames }
    }

    #[test]
    fn panpot_centre_is_equal_power() {
        let mut m = mixer::Mixer::new(1, 2, mixer::PANPOT, None, &[Some((30.0, 0.0)), Some((-30.0, 0.0))]);
        m.take(&[0.0, 0.0, 0.0, -1.0, 0.0, 0.0], true);
        let input = vec![0.5f32; 128];
        let mut out_data = vec![0.0f32; 256];
        let mut out = BlockMut { data: &mut out_data, channels: 2, frames: 128 };
        m.process(&input, &[1], &mut out, None);
        let want = (0.5f64 * std::f64::consts::FRAC_1_SQRT_2) as f32;
        assert_eq!(out_data[0], want);
        assert_eq!(out_data[128], want);
    }

    #[test]
    fn limiter_keeps_a_hot_signal_at_the_ceiling() {
        let mut l = limiter::Limiter::new(48000.0, -1.0, 0.08, 0.0015, &[0.0; limiter::DYNAMICS_COUNT], 0);
        let data = vec![0.99f32; 2 * 128];
        let mut out_data = vec![0.0f32; 2 * 128];
        for _ in 0..200 {
            let input = block(&data, 2, 128);
            let mut out = BlockMut { data: &mut out_data, channels: 2, frames: 128 };
            l.process(&input, &mut out);
        }
        let ceiling = host::pow(10.0, -1.0 / 20.0) as f32;
        assert!(out_data.iter().all(|&v| v <= ceiling + 1e-6), "over the ceiling: {:?}", out_data.iter().fold(0.0f32, |a, &b| a.max(b)));
        assert!(out_data.iter().all(|&v| v > 0.8), "the gain should settle near the ceiling, not silence");
    }

    #[test]
    fn reverb_answers_an_impulse_then_goes_quiet() {
        let mut params = [0.0; reverb::PARAM_COUNT];
        params[0] = 0.2;
        params[2] = 0.3;
        let mut r = reverb::Reverb::new(48000.0, 2, &params, 0b101);
        let mut data = vec![0.0f32; 128];
        data[0] = 1.0;
        let mut heard = false;
        let mut tail = 0.0f32;
        for b in 0..4000 {
            let input = block(&data, 1, 128);
            let mut out_data = vec![0.0f32; 2 * 128];
            {
                let mut out = BlockMut { data: &mut out_data, channels: 2, frames: 128 };
                r.process(&input, &mut out);
            }
            let peak = out_data.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
            if b > 0 && peak > 0.0 {
                heard = true;
            }
            if b == 3999 {
                tail = peak;
            }
            data[0] = 0.0;
        }
        assert!(heard, "no tail after the impulse");
        assert_eq!(tail, 0.0, "the network must reach exact silence (its quiet shortcut)");
    }
}
