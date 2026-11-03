//! The spatial mixer (src/audio/worklets/spatial-mixer.js): every 3D voice
//! is one mono input slot; each is panned to the output channels by the
//! layout's law and summed, gains ramping across each block, plus a mono
//! LFE send. The panning laws are the worklet's own: "panpot", "vbap",
//! "alsoft" (OpenAL Soft's decoder rows) and "ambi3". The main thread
//! sends each slot's direction, spread and LFE send (take), and a slot a
//! voice has just taken applies its first gains at once (start).

use crate::{host, js_max, js_min, BlockMut};

/// The panning laws, by the numbers the WebAssembly skin passes.
pub const PANPOT: u32 = 0;
pub const VBAP: u32 = 1;
pub const ALSOFT: u32 = 2;
pub const AMBI3: u32 = 3;

/// One speaker of a layout: azimuth and elevation in degrees, or none
/// (the LFE). Azimuth is positive to the left, as in src/audio/layouts.js.
pub type Speaker = Option<(f64, f64)>;

/// OpenAL Soft's decoder for the "alsoft" law: the front widening and one
/// row of ambisonic weights per channel (none where it feeds nothing).
pub struct Decoder {
    pub front: f64,
    pub rows: Vec<Option<Vec<f64>>>,
}

// The 16 harmonics at a unit vector (x front, y left, z up), stored as f32
// as the worklet's Float32Array stores them.
fn harmonics(x: f64, y: f64, z: f64, out: &mut [f32]) {
    out[0] = 0.28209479177387814f64 as f32;
    // Each of the worklet's values is an f64 product stored in a
    // Float32Array: the same product, rounded the same way.
    let mut s = |i: usize, v: f64| out[i] = v as f32;
    s(1, 0.4886025119029199 * y);
    s(2, 0.4886025119029199 * z);
    s(3, 0.4886025119029199 * x);
    s(4, 1.0925484305920792 * x * y);
    s(5, 1.0925484305920792 * y * z);
    s(6, 0.31539156525252005 * (3.0 * z * z - 1.0));
    s(7, 1.0925484305920792 * x * z);
    s(8, 0.5462742152960396 * (x * x - y * y));
    s(9, 0.5900435899266435 * y * (3.0 * x * x - y * y));
    s(10, 2.890611442640554 * x * y * z);
    s(11, 0.4570457994644658 * y * (5.0 * z * z - 1.0));
    s(12, 0.3731763325901154 * z * (5.0 * z * z - 3.0));
    s(13, 0.4570457994644658 * x * (5.0 * z * z - 1.0));
    s(14, 1.445305721320277 * z * (x * x - y * y));
    s(15, 0.5900435899266435 * x * (x * x - 3.0 * y * y));
}

pub struct Mixer {
    slots: usize,
    channels: usize,
    law: u32,
    decoder: Option<Decoder>,
    // Ear-level ring and height ring, sorted by azimuth: [channel, az].
    ring: Vec<(usize, f64)>,
    top: Vec<(usize, f64)>,
    all_main: Vec<usize>,
    // Per slot: direction (3), spread, lfe; the gains in use and aimed at.
    params: Vec<f32>,
    current: Vec<f32>,
    goal: Vec<f32>,
    lfe_now: Vec<f32>,
    lfe_target: Vec<f32>,
    fresh: Vec<u8>,
    dirty: Vec<u8>,
    next: Vec<f32>,
    scratch: Vec<f32>,
    tmp: Vec<f32>,
    h2d: [f64; 7],
}

impl Mixer {
    pub fn new(slots: usize, channels: usize, law: u32, decoder: Option<Decoder>, speakers: &[Speaker]) -> Mixer {
        let mut m = Mixer {
            slots,
            channels,
            law,
            decoder,
            ring: Vec::new(),
            top: Vec::new(),
            all_main: Vec::new(),
            params: vec![0.0; slots * 5],
            current: vec![0.0; slots * channels],
            goal: vec![0.0; slots * channels],
            lfe_now: vec![0.0; slots],
            lfe_target: vec![0.0; slots],
            fresh: vec![0; slots],
            dirty: vec![0; slots],
            next: vec![0.0; channels],
            scratch: vec![0.0; channels],
            tmp: vec![0.0; channels],
            h2d: [0.0; 7],
        };
        // The worklet's constructor lays out before its gain arrays exist,
        // so nothing is dirty yet.
        m.layout(speakers, false);
        m
    }

    /// A new layout (the worklet's "layout" message, and construction).
    pub fn set_layout(&mut self, law: u32, decoder: Option<Decoder>, speakers: &[Speaker]) {
        self.law = law;
        self.decoder = decoder;
        self.layout(speakers, true);
    }

    /// The layout's speakers alone; `dirty` marks every slot's gains
    /// stale, as a layout message does (construction does not).
    pub fn set_speakers(&mut self, speakers: &[Speaker], dirty: bool) {
        self.layout(speakers, dirty);
    }

    pub fn set_law(&mut self, law: u32) {
        self.law = law;
    }

    pub fn set_decoder(&mut self, decoder: Option<Decoder>) {
        self.decoder = decoder;
    }

    fn layout(&mut self, speakers: &[Speaker], dirty: bool) {
        let mut ring = Vec::new();
        let mut top = Vec::new();
        for (channel, s) in speakers.iter().enumerate() {
            if let Some((az, el)) = s {
                (if *el > 20.0 { &mut top } else { &mut ring }).push((channel, (az * std::f64::consts::PI) / 180.0));
            }
        }
        ring.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        top.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        self.ring = ring;
        self.top = top;
        self.all_main = speakers.iter().enumerate().filter_map(|(c, s)| s.map(|_| c)).collect();
        if dirty {
            self.dirty.fill(1);
        }
    }

    /// Slot parameters, [slot, x, y, z, spread, lfe] per entry (the
    /// worklet's "params" message; `fresh` at construction).
    pub fn take(&mut self, p: &[f32], fresh: bool) {
        let mut i = 0;
        while i + 5 < p.len() {
            let slot = p[i] as f64;
            // Out-of-range slots write nowhere, as a typed array's do.
            if slot >= 0.0 && (slot as usize) < self.slots && slot == (slot as usize) as f64 {
                let slot = slot as usize;
                let base = slot * 5;
                for k in 0..5 {
                    self.params[base + k] = p[i + 1 + k];
                }
                self.dirty[slot] = 1;
                if fresh {
                    self.fresh[slot] = 1;
                }
            }
            i += 6;
        }
    }

    /// A voice took this slot: its first gains apply at once.
    pub fn start(&mut self, slot: usize) {
        if slot < self.slots {
            self.fresh[slot] = 1;
        }
    }

    // Pairwise panning of azimuth `az` over `ring` into `out` (scaled by w).
    fn pair(ring: &[(usize, f64)], az: f64, out: &mut [f32], w: f64) {
        let n = ring.len();
        if n == 0 || w <= 0.0 {
            return;
        }
        if n == 1 {
            let c = ring[0].0;
            out[c] = (out[c] as f64 + w) as f32;
            return;
        }
        for i in 0..n {
            let (mut c1, mut a1) = ring[i];
            let (mut c2, mut a2) = ring[(i + 1) % n];
            if i == n - 1 {
                a2 += 2.0 * std::f64::consts::PI;
            }
            let mut a = az;
            if a < a1 {
                a += 2.0 * std::f64::consts::PI;
            }
            if a < a1 || a > a2 {
                continue;
            }
            if a2 - a1 > std::f64::consts::PI {
                let front = (a1 + a2) / 2.0 - std::f64::consts::PI;
                a = 2.0 * (front + std::f64::consts::PI / 2.0) - a;
                let t1 = a1;
                let t2 = a2;
                c1 = ring[(i + 1) % n].0;
                c2 = ring[i].0;
                a1 = t2 - 2.0 * std::f64::consts::PI;
                a2 = t1;
                while a > a2 {
                    a -= 2.0 * std::f64::consts::PI;
                }
                while a < a1 {
                    a += 2.0 * std::f64::consts::PI;
                }
            }
            let px = host::cos(a);
            let py = host::sin(a);
            let x1 = host::cos(a1);
            let y1 = host::sin(a1);
            let x2 = host::cos(a2);
            let y2 = host::sin(a2);
            let det = x1 * y2 - x2 * y1;
            let (mut g1, mut g2);
            if det.abs() < 1e-6 {
                g1 = 1.0;
                g2 = 0.0;
            } else {
                g1 = (px * y2 - x2 * py) / det;
                g2 = (x1 * py - px * y1) / det;
            }
            g1 = js_max(g1, 0.0);
            g2 = js_max(g2, 0.0);
            // The worklet's `Math.sqrt(...) || 1`: 0 and NaN both give 1.
            let norm = {
                let v = (g1 * g1 + g2 * g2).sqrt();
                if v == 0.0 || v.is_nan() { 1.0 } else { v }
            };
            out[c1] = (out[c1] as f64 + (w * g1) / norm) as f32;
            out[c2] = (out[c2] as f64 + (w * g2) / norm) as f32;
            return;
        }
    }

    fn gains(&mut self, slot: usize, out: &mut [f32]) {
        let base = slot * 5;
        let (mut x, mut y, mut z) = (self.params[base] as f64, self.params[base + 1] as f64, self.params[base + 2] as f64);
        let spread = js_min(js_max(self.params[base + 3] as f64, 0.0), 1.0);
        let len = (x * x + y * y + z * z).sqrt();
        out.fill(0.0);
        if len < 1e-6 {
            x = 0.0;
            y = 0.0;
            z = -1.0;
        } else {
            x /= len;
            y /= len;
            z /= len;
        }
        if self.law == ALSOFT {
            if let Some(decoder) = &self.decoder {
                let mut az = host::atan2(x, -z);
                let ev = host::asin(js_max(-1.0, js_min(1.0, y)));
                let front = decoder.front;
                if front > 0.0 && az.abs() < std::f64::consts::PI / 2.0 {
                    let sign = if az > 0.0 { 1.0 } else if az < 0.0 { -1.0 } else { 0.0 };
                    az = sign * js_min(az.abs() * front, std::f64::consts::PI / 2.0);
                }
                let ce = host::cos(ev);
                let bx = host::cos(az) * ce;
                let by = -host::sin(az) * ce;
                let h = &mut self.h2d;
                h[0] = 1.0;
                h[1] = 1.7320508075688772 * by;
                h[2] = 1.7320508075688772 * bx;
                h[3] = 3.872983346207417 * bx * by;
                h[4] = 1.9364916731037085 * (bx * bx - by * by);
                h[5] = 2.091650066335189 * by * (3.0 * bx * bx - by * by);
                h[6] = 2.091650066335189 * bx * (bx * bx - 3.0 * by * by);
                for c in 0..usize::min(out.len(), decoder.rows.len()) {
                    if let Some(row) = &decoder.rows[c] {
                        let mut g = 0.0;
                        for (k, v) in row.iter().enumerate() {
                            g += v * h[k];
                        }
                        out[c] = g as f32;
                    }
                }
                return;
            }
        }
        if self.law == AMBI3 {
            harmonics(-z, -x, y, out);
            if spread > 0.0 {
                let w = 1.0 - spread;
                let k = (16.0 / (1.0 + 3.0 * w * w + 5.0 * host::pow(w, 4.0) + 7.0 * host::pow(w, 6.0))).sqrt();
                out[0] = (out[0] as f64 * k) as f32;
                for c in 1..16 {
                    let e = (c as f64).sqrt().floor();
                    out[c] = (out[c] as f64 * (k * host::pow(w, e))) as f32;
                }
            }
            return;
        }
        if self.law == PANPOT {
            let pan = js_max(-1.0, js_min(1.0, x)) * (1.0 - spread);
            let t = ((pan + 1.0) * std::f64::consts::PI) / 4.0;
            if !out.is_empty() {
                out[0] = host::cos(t) as f32;
            }
            if out.len() > 1 {
                out[1] = host::sin(t) as f32;
            }
            return;
        }
        let az = host::atan2(-x, -z);
        let el = host::asin(js_max(-1.0, js_min(1.0, y)));
        self.scratch.fill(0.0);
        // The rings are read while the scratch is written; pair takes both.
        let (ring, top) = (std::mem::take(&mut self.ring), std::mem::take(&mut self.top));
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut tmp = std::mem::take(&mut self.tmp);
        if !top.is_empty() {
            let up = js_min(js_max(el / (std::f64::consts::PI / 4.0), 0.0), 1.0);
            Self::pair(&ring, az, &mut scratch, (1.0 - up).sqrt());
            tmp.fill(0.0);
            Self::pair(&top, az, &mut tmp, up.sqrt());
            for c in 0..self.channels {
                scratch[c] = (scratch[c] as f64 + tmp[c] as f64) as f32;
            }
        } else {
            Self::pair(&ring, az, &mut scratch, 1.0);
        }
        self.ring = ring;
        self.top = top;
        self.tmp = tmp;
        let n = if self.all_main.is_empty() { 1.0 } else { self.all_main.len() as f64 };
        for &c in &self.all_main.clone() {
            let s = scratch[c] as f64;
            out[c] = ((1.0 - spread) * s * s + spread / n).sqrt() as f32;
        }
        self.scratch = scratch;
    }

    /// One block: `inputs` is slots runs of `frames` mono samples with
    /// `present` marking the connected slots; `out` the speaker feeds (or
    /// ambisonic channels), `lfe` the optional mono LFE send.
    pub fn process(&mut self, inputs: &[f32], present: &[u8], out: &mut BlockMut, lfe: Option<&mut [f32]>) {
        let frames = out.frames;
        let n = self.channels;
        out.fill(0.0);
        let mut lfe = lfe;
        if let Some(l) = &mut lfe {
            l.fill(0.0);
        }
        let has_lfe = lfe.is_some();
        let c_max = usize::min(n, out.channels);
        for s in 0..self.slots {
            if present.get(s).copied().unwrap_or(0) == 0 {
                continue;
            }
            let src = &inputs[s * frames..(s + 1) * frames];
            let base = s * n;
            if self.dirty[s] != 0 {
                let mut next = std::mem::take(&mut self.next);
                self.gains(s, &mut next);
                for c in 0..n {
                    self.goal[base + c] = next[c];
                }
                self.next = next;
                self.lfe_target[s] = self.params[s * 5 + 4];
                self.dirty[s] = 0;
            }
            if self.fresh[s] != 0 {
                for c in 0..n {
                    self.current[base + c] = self.goal[base + c];
                }
                self.lfe_now[s] = self.lfe_target[s];
                self.fresh[s] = 0;
            }
            let silent = src.iter().all(|&v| v == 0.0);
            if silent {
                for c in 0..c_max {
                    self.current[base + c] = self.goal[base + c];
                }
                if has_lfe {
                    self.lfe_now[s] = self.lfe_target[s];
                }
                continue;
            }
            for c in 0..c_max {
                let g0 = self.current[base + c] as f64;
                let g1 = self.goal[base + c] as f64;
                if g0 == 0.0 && g1 == 0.0 {
                    continue;
                }
                // The worklet adds into a Float32Array: each sample's sum
                // is rounded as it is stored, as these are.
                let dst = &mut out.data[c * frames..(c + 1) * frames];
                if g0 == g1 {
                    for (d, &s) in dst.iter_mut().zip(src) {
                        *d = (*d as f64 + s as f64 * g1) as f32;
                    }
                } else {
                    let step = (g1 - g0) / frames as f64;
                    for (i, (d, &s)) in dst.iter_mut().zip(src).enumerate() {
                        *d = (*d as f64 + s as f64 * (g0 + step * (i + 1) as f64)) as f32;
                    }
                    self.current[base + c] = self.goal[base + c];
                }
            }
            if let Some(l) = &mut lfe {
                let l0 = self.lfe_now[s] as f64;
                let l1 = self.lfe_target[s] as f64;
                if l0 != 0.0 || l1 != 0.0 {
                    let step = (l1 - l0) / frames as f64;
                    let mut g = l0;
                    for i in 0..frames {
                        g += step;
                        l[i] = (l[i] as f64 + src[i] as f64 * g) as f32;
                    }
                    self.lfe_now[s] = self.lfe_target[s];
                }
            }
        }
    }
}
