//! Speaker-panel test signals, prepared on request, outside rendering.
use crate::host;
use std::f64::consts::PI;
pub fn make(rate: f64, frames: usize, tone: bool) -> Vec<f32> {
    let mut out = vec![0.0; frames];
    let mut state = [0.0; 7];
    let mut seed = 0x6a09e667u32;
    let fade = (rate * 0.02).round() as usize;
    for (i, value) in out.iter_mut().enumerate() {
        let sample = if tone {
            host::sin(2.0 * PI * 50.0 * i as f64 / rate) * 0.25
        } else {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let w = seed as f64 / 2147483648.0 - 1.0;
            state[0] = 0.99886 * state[0] + w * 0.0555179;
            state[1] = 0.99332 * state[1] + w * 0.0750759;
            state[2] = 0.969 * state[2] + w * 0.153852;
            state[3] = 0.8665 * state[3] + w * 0.3104856;
            state[4] = 0.55 * state[4] + w * 0.5329522;
            state[5] = -0.7616 * state[5] - w * 0.016898;
            let value = (state.iter().sum::<f64>() + w * 0.5362) * 0.11;
            state[6] = w * 0.115926;
            value
        };
        let envelope = (i.min(frames - 1 - i) as f64 / fade.max(1) as f64).min(1.0);
        *value = (sample * envelope) as f32;
    }
    out
}
