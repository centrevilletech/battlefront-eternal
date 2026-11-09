//! Stereo placement and mono folding used by the Rust output graph.
use crate::timeline::Timeline;
use crate::{host, FRAMES};

pub fn alsoft(mut x: f64, mut y: f64, mut z: f64) -> [f64; 2] {
    let length = (x * x + y * y + z * z).sqrt();
    if length < 1e-6 {
        x = 0.0;
        y = 0.0;
        z = -1.0;
    } else {
        x /= length;
        y /= length;
        z /= length;
    }
    let mut azimuth = host::atan2(x, -z);
    if azimuth.abs() < std::f64::consts::FRAC_PI_2 {
        azimuth = azimuth.signum() * (azimuth.abs() * 1.5).min(std::f64::consts::FRAC_PI_2);
    }
    let ce = host::cos(host::asin(y.clamp(-1.0, 1.0)));
    let xx = 3.0f64.sqrt() * host::cos(azimuth) * ce;
    let yy = -3.0f64.sqrt() * host::sin(azimuth) * ce;
    [0.5 + 0.288675135 * yy + 0.0552305643 * xx, 0.5 - 0.288675135 * yy + 0.0552305643 * xx]
}

pub struct LightPan {
    pub sides: [Timeline; 2],
    levels: [f64; 2],
    placed: bool,
}
impl Default for LightPan {
    fn default() -> Self {
        Self {
            sides: [Timeline::new(0.0); 2],
            levels: [0.0; 2],
            placed: false,
        }
    }
}
impl LightPan {
    pub fn place(&mut self, position: [f64; 3], level: f64, at: f64, frame: f64, rate: f64) {
        let gains = alsoft(position[0], position[1], position[2]);
        for ((&gain, last_slot), side) in gains.iter().zip(&mut self.levels).zip(&mut self.sides) {
            let value = gain * level;
            if !value.is_finite() {
                continue;
            }
            let last = *last_slot;
            if !self.placed {
                side.set_value(value as f32);
                side.insert(false, value as f32, at, 0.0, frame, rate);
            } else if (value - last).abs() > 1e-3 * value.abs().max(last.abs()) || (value == 0.0) != (last == 0.0) {
                side.insert(true, value as f32, at, 0.015, frame, rate);
            } else {
                continue;
            }
            *last_slot = value;
        }
        self.placed = true;
    }
}

/// Native speaker interpretation only defines 2- and 6-channel mono folds.
/// Larger layouts copy channel zero. Keep that observed browser behavior.
pub fn mono(data: &mut [f32], channels: usize) {
    for i in 0..FRAMES {
        let v = match channels {
            2 => (data[i] + data[FRAMES + i]) * 0.5,
            6 => {
                std::f32::consts::FRAC_1_SQRT_2 * (data[i] + data[FRAMES + i])
                    + data[2 * FRAMES + i]
                    + 0.5 * (data[4 * FRAMES + i] + data[5 * FRAMES + i])
            }
            _ => data[i],
        };
        data[i] = v;
        data[FRAMES + i] = v;
    }
    if channels > 2 {
        data[2 * FRAMES..channels * FRAMES].fill(0.0);
    }
}
