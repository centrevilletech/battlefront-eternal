//! A BiquadFilterNode as Chromium computes one whose parameters hold still
//! (no automation): its coefficients from the float values of frequency,
//! Q and gain (Biquad::SetLowpassParams and the rest: fdlibm's sine and
//! cosine, 10^x as fdlibm's expf, normalized through a0's inverse), then
//! per sample in double precision from float input to float output, the
//! state flushed of denormals at each block's end. Operation for
//! operation, so a filter here takes the browser's values to the bit (on
//! Linux; its macOS build runs these through vDSP).

/// A filter's coefficients and its state (one channel).
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

/// The filter types the game's graphs use (BiquadFilterNode.type).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Lowpass,
    Highpass,
    Peaking,
    Highshelf,
}

// 10^x as the browser's biquad works it out: fdlibm's expf of x ln 10,
// which is fdlibm's exp of the argument rounded to single precision, its
// result rounded to single.
fn pow10(x: f64) -> f64 {
    v8math::exp((x * std::f64::consts::LN_10) as f32 as f64) as f32 as f64
}

// Below the smallest normal f32 the state is flushed to zero at a block's
// end (DenormalDisabler::FlushDenormalFloatToZero).
#[inline(always)]
fn flush(x: f64) -> f64 {
    if x.abs() < f32::MIN_POSITIVE as f64 { 0.0 } else { x }
}

impl Biquad {
    /// A filter of `kind` at `frequency` Hz (a float, as the param holds
    /// it) for a context at `rate`, with Q and gain (dB) as floats too.
    pub fn new(kind: Kind, frequency: f32, q: f32, gain: f32, rate: f64) -> Biquad {
        let mut b = Biquad::default();
        b.set(kind, frequency, q, gain, rate);
        b
    }

    /// New parameters (the state kept).
    pub fn set(&mut self, kind: Kind, frequency: f32, q: f32, gain: f32, rate: f64) {
        // NormalizeFrequency: the frequency over the Nyquist frequency (no
        // detune here).
        let f = frequency as f64 / (0.5 * rate);
        let (q, gain) = (q as f64, gain as f64);
        let [b0, b1, b2, a0, a1, a2] = match kind {
            Kind::Lowpass => lowpass(f, q),
            Kind::Highpass => highpass(f, q),
            Kind::Peaking => peaking(f, q, gain),
            Kind::Highshelf => highshelf(f, gain),
        };
        // SetNormalizedCoefficients.
        let inv = 1.0 / a0;
        self.b0 = b0 * inv;
        self.b1 = b1 * inv;
        self.b2 = b2 * inv;
        self.a1 = a1 * inv;
        self.a2 = a2 * inv;
    }

    /// One block from `src` into `dst` (the same length; they may not be
    /// the same slice here, as they may in the browser).
    pub fn process(&mut self, src: &[f32], dst: &mut [f32]) {
        let (b0, b1, b2, a1, a2) = (self.b0, self.b1, self.b2, self.a1, self.a2);
        let (mut x1, mut x2, mut y1, mut y2) = (self.x1, self.x2, self.y1, self.y2);
        for (x, out) in src.iter().zip(dst.iter_mut()) {
            let x = *x as f64;
            let y = (b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) as f32;
            *out = y;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y as f64;
        }
        self.x1 = flush(x1);
        self.x2 = flush(x2);
        self.y1 = flush(y1);
        self.y2 = flush(y2);
    }

    /// One block in place.
    pub fn process_in_place(&mut self, data: &mut [f32]) {
        let (b0, b1, b2, a1, a2) = (self.b0, self.b1, self.b2, self.a1, self.a2);
        let (mut x1, mut x2, mut y1, mut y2) = (self.x1, self.x2, self.y1, self.y2);
        for v in data.iter_mut() {
            let x = *v as f64;
            let y = (b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) as f32;
            *v = y;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y as f64;
        }
        self.x1 = flush(x1);
        self.x2 = flush(x2);
        self.y1 = flush(y1);
        self.y2 = flush(y2);
    }
}

// Each returns b0, b1, b2, a0, a1, a2 before normalizing, as Chromium's
// setters compute them.

fn lowpass(cutoff: f64, resonance_db: f64) -> [f64; 6] {
    let cutoff = cutoff.clamp(0.0, 1.0);
    if cutoff == 1.0 {
        return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    if cutoff > 0.0 {
        let resonance = pow10(resonance_db / 20.0);
        let theta = std::f64::consts::PI * cutoff;
        let alpha = v8math::sin(theta) / (2.0 * resonance);
        let cosw = v8math::cos(theta);
        let beta = (1.0 - cosw) / 2.0;
        return [beta, 2.0 * beta, beta, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha];
    }
    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

fn highpass(cutoff: f64, resonance_db: f64) -> [f64; 6] {
    let cutoff = cutoff.clamp(0.0, 1.0);
    if cutoff == 1.0 {
        return [0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    if cutoff > 0.0 {
        let resonance = pow10(resonance_db / 20.0);
        let theta = std::f64::consts::PI * cutoff;
        let alpha = v8math::sin(theta) / (2.0 * resonance);
        let cosw = v8math::cos(theta);
        let beta = (1.0 + cosw) / 2.0;
        return [beta, -2.0 * beta, beta, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha];
    }
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

fn peaking(frequency: f64, q: f64, db_gain: f64) -> [f64; 6] {
    let frequency = frequency.clamp(0.0, 1.0);
    let q = q.max(0.0);
    let a = pow10(db_gain / 40.0);
    if frequency > 0.0 && frequency < 1.0 {
        if q > 0.0 {
            let w0 = std::f64::consts::PI * frequency;
            let alpha = v8math::sin(w0) / (2.0 * q);
            let k = v8math::cos(w0);
            return [1.0 + alpha * a, -2.0 * k, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * k, 1.0 - alpha / a];
        }
        return [a * a, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

fn highshelf(frequency: f64, db_gain: f64) -> [f64; 6] {
    let frequency = frequency.clamp(0.0, 1.0);
    let a = pow10(db_gain / 40.0);
    if frequency == 1.0 {
        return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    if frequency > 0.0 {
        let w0 = std::f64::consts::PI * frequency;
        // The slope S is 1 (its largest).
        let s = 1.0;
        let alpha = 0.5 * v8math::sin(w0) * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
        let k = v8math::cos(w0);
        let k2 = 2.0 * a.sqrt() * alpha;
        let a_plus_one = a + 1.0;
        let a_minus_one = a - 1.0;
        return [
            a * (a_plus_one + a_minus_one * k + k2),
            -2.0 * a * (a_minus_one + a_plus_one * k),
            a * (a_plus_one + a_minus_one * k - k2),
            a_plus_one - a_minus_one * k + k2,
            2.0 * (a_minus_one - a_plus_one * k),
            a_plus_one - a_minus_one * k - k2,
        ];
    }
    [a * a, 0.0, 0.0, 1.0, 0.0, 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_lowpass_passes_a_signal_as_it_is_and_a_peak_of_0_db_too() {
        let mut lp = Biquad::new(Kind::Lowpass, 24000.0, -3.0103, 0.0, 48000.0);
        let mut pk = Biquad::new(Kind::Peaking, 1000.0, 0.9, 0.0, 48000.0);
        let src: Vec<f32> = (0..128).map(|i| ((i * 37) % 101) as f32 / 101.0 - 0.5).collect();
        let mut a = vec![0.0f32; 128];
        lp.process(&src, &mut a);
        assert_eq!(a, src);
        pk.process_in_place(&mut a);
        // 0 dB: a is 1, b and a match, the coefficients normalize to a
        // pass-through (to rounding).
        for (x, y) in a.iter().zip(&src) {
            assert!((x - y).abs() < 1e-6);
        }
    }

    #[test]
    fn a_highpass_takes_out_a_constant() {
        let mut hp = Biquad::new(Kind::Highpass, 180.0, -3.0103, 0.0, 48000.0);
        let mut v = vec![0.25f32; 128 * 40];
        hp.process_in_place(&mut v);
        assert!(v[v.len() - 1].abs() < 1e-4);
    }
}
