//! The pose sampler of src/anim.js for world and presentation hosts: the weighted blending
//! of animation clips into a pose (`Pose.add`), and the finals a pose
//! applies to its bones (`Pose.apply`: each touched bone's normalized
//! quaternion and its translation over its weight).
//!
//! Every answer must be the JavaScript's own to the last bit: poses are
//! also evaluated where they feed the simulation (a rider's hit shapes in
//! src/rider-pose.js), and on the page the drawn skeleton must not move.
//! Each function repeats the JavaScript it stands for operation by
//! operation, in the same order, in f64: clip samples are f32 in a
//! Float32Array there and here, each read widened exactly as JavaScript
//! widens it, and the pose's accumulators are f32 in both, so a store
//! rounds at the same point. Nothing here uses a function whose last bit
//! could differ from V8's (sqrt, floor and the f64 remainder are exact in
//! both; the sampler needs no others). Math.min and Math.max are `js_min`
//! and `js_max` below, which keep JavaScript's rules for NaN and signed
//! zeros. WebAssembly has no fused multiply-add unless asked for, and
//! Rust never contracts f64 operations by itself.
//!
//! Hosts load clips once and retain one pose per actor. The live simulation
//! ABI stays in rust/sim; browser and native presentation own their buffers
//! and call this sampler directly inside their batched frame update.

const STRIDE: usize = 7;
pub struct Clip {
    pub frames: usize,
    pub fps: f64,
    pub root_motion: bool,
    pub tracks: Vec<i32>,
    pub data: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Pose {
    pub root: i32,
    pub q: Vec<f32>,
    pub t: Vec<f32>,
    pub w: Vec<f32>,
    /// `apply`'s finals per bone (qx, qy, qz, qw, tx, ty, tz), f64 as
    /// three's Quaternion and Vector3 hold them.
    pub out: Vec<f64>,
}

/// Math.min(a, b): NaN if either is, and -0 below +0.
#[inline(always)]
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a < b {
        a
    } else if b < a {
        b
    } else if a == 0.0 && a.is_sign_negative() {
        a
    } else {
        b
    }
}

/// Math.max(a, b): NaN if either is, and +0 above -0.
#[inline(always)]
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a > b {
        a
    } else if b > a {
        b
    } else if a == 0.0 && a.is_sign_positive() {
        a
    } else {
        b
    }
}

impl Pose {
    pub fn new(count: usize, root: i32) -> Pose {
        Pose {
            root,
            q: vec![0.0; count * 4],
            t: vec![0.0; count * 3],
            w: vec![0.0; count],
            out: vec![0.0; count * STRIDE],
        }
    }

    pub fn clear(&mut self) {
        self.q.fill(0.0);
        self.t.fill(0.0);
        self.w.fill(0.0);
    }

    /// Pose.add(clip, time, weight, loop) of src/anim.js, the caller
    /// having turned away a missing clip and a weight at or under 1e-4.
    pub fn add(&mut self, clip: &Clip, time: f64, weight: f64, looping: bool) {
        let frames = clip.frames;
        let mut f = 0.0;
        if frames > 1 {
            let span = (frames - 1) as f64;
            f = time * clip.fps;
            f = if looping {
                ((f % span) + span) % span
            } else {
                js_min(js_max(f, 0.0), span)
            };
        }
        let i0 = f.floor();
        let i1 = js_min(i0 + 1.0, frames as f64 - 1.0);
        let a = f - i0;
        let (i0, i1) = (i0 as usize, i1 as usize);
        let d = &clip.data;
        for k in 0..clip.tracks.len() {
            let bone = clip.tracks[k];
            if bone < 0 {
                continue;
            }
            let bone = bone as usize;
            // Each frame's seven f32s, widened as JavaScript reads them.
            let f0 = &d[(k * frames + i0) * STRIDE..][..STRIDE];
            let f1 = &d[(k * frames + i1) * STRIDE..][..STRIDE];
            let at = |frame: &[f32], i: usize| frame[i] as f64;
            let (mut x1, mut y1, mut z1, mut w1) = (at(f1, 0), at(f1, 1), at(f1, 2), at(f1, 3));
            if at(f0, 0) * x1 + at(f0, 1) * y1 + at(f0, 2) * z1 + at(f0, 3) * w1 < 0.0 {
                x1 = -x1;
                y1 = -y1;
                z1 = -z1;
                w1 = -w1;
            }
            let mut qx = at(f0, 0) + (x1 - at(f0, 0)) * a;
            let mut qy = at(f0, 1) + (y1 - at(f0, 1)) * a;
            let mut qz = at(f0, 2) + (z1 - at(f0, 2)) * a;
            let mut qw = at(f0, 3) + (w1 - at(f0, 3)) * a;
            // The JavaScript's `|| 1`: a zero or NaN length becomes 1.
            let len = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
            let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
            qx /= len;
            qy /= len;
            qz /= len;
            qw /= len;
            let qi = bone * 4;
            if self.q[qi] as f64 * qx
                + self.q[qi + 1] as f64 * qy
                + self.q[qi + 2] as f64 * qz
                + self.q[qi + 3] as f64 * qw
                < 0.0
            {
                qx = -qx;
                qy = -qy;
                qz = -qz;
                qw = -qw;
            }
            self.q[qi] = (self.q[qi] as f64 + qx * weight) as f32;
            self.q[qi + 1] = (self.q[qi + 1] as f64 + qy * weight) as f32;
            self.q[qi + 2] = (self.q[qi + 2] as f64 + qz * weight) as f32;
            self.q[qi + 3] = (self.q[qi + 3] as f64 + qw * weight) as f32;
            let ti = bone * 3;
            if bone as i32 != self.root || !clip.root_motion {
                self.t[ti] =
                    (self.t[ti] as f64 + (at(f0, 4) + (at(f1, 4) - at(f0, 4)) * a) * weight) as f32;
                self.t[ti + 1] = (self.t[ti + 1] as f64
                    + (at(f0, 5) + (at(f1, 5) - at(f0, 5)) * a) * weight)
                    as f32;
                self.t[ti + 2] = (self.t[ti + 2] as f64
                    + (at(f0, 6) + (at(f1, 6) - at(f0, 6)) * a) * weight)
                    as f32;
            }
            self.w[bone] = (self.w[bone] as f64 + weight) as f32;
        }
    }

    /// The finals Pose.apply writes, per bone whose weight passes 1e-6:
    /// three's Quaternion.normalize of the accumulated quaternion (a zero
    /// length gives 0, 0, 0, 1; otherwise each part times 1 / length) and
    /// the accumulated translation over the weight. Bones at or under the
    /// weight keep whatever `out` held; the caller skips them by w, as
    /// the JavaScript does.
    pub fn apply_out(&mut self) {
        for i in 0..self.w.len() {
            let w = self.w[i] as f64;
            if w <= 1e-6 {
                continue;
            }
            let qi = i * 4;
            let ti = i * 3;
            let oi = i * STRIDE;
            let (x, y, z, wq) = (
                self.q[qi] as f64,
                self.q[qi + 1] as f64,
                self.q[qi + 2] as f64,
                self.q[qi + 3] as f64,
            );
            let l = (x * x + y * y + z * z + wq * wq).sqrt();
            if l == 0.0 {
                self.out[oi] = 0.0;
                self.out[oi + 1] = 0.0;
                self.out[oi + 2] = 0.0;
                self.out[oi + 3] = 1.0;
            } else {
                let l = 1.0 / l;
                self.out[oi] = x * l;
                self.out[oi + 1] = y * l;
                self.out[oi + 2] = z * l;
                self.out[oi + 3] = wq * l;
            }
            self.out[oi + 4] = self.t[ti] as f64 / w;
            self.out[oi + 5] = self.t[ti + 1] as f64 / w;
            self.out[oi + 6] = self.t[ti + 2] as f64 / w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(frames: usize, root_motion: bool, tracks: Vec<i32>, data: Vec<f32>) -> Clip {
        Clip {
            frames,
            fps: 30.0,
            root_motion,
            tracks,
            data,
        }
    }

    // One track on bone 1, two frames: identity to a quarter turn about y,
    // translation doubling. Halfway between the frames at weight 1 the
    // sample is the normalized lerp, as the JavaScript computes it.
    #[test]
    fn samples_between_frames() {
        let c = clip(
            2,
            false,
            vec![1],
            vec![
                0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0, 0.0, 1.0, 0.0, 0.0, 2.0, 4.0, 6.0,
            ],
        );
        let mut pose = Pose::new(3, 0);
        pose.add(&c, 0.5 / 30.0, 1.0, false);
        let half = (0.5f64).sqrt() as f32;
        assert_eq!(pose.q[4..8], [0.0, half, 0.0, half]);
        assert_eq!(pose.t[3..6], [1.5, 3.0, 4.5]);
        assert_eq!(pose.w[1], 1.0);
        pose.apply_out();
        let h = half as f64;
        let s = h * (1.0 / (h * h + h * h).sqrt());
        assert_eq!(pose.out[7..14], [0.0, s, 0.0, s, 1.5, 3.0, 4.5]);
    }

    // The root's travel is dropped only for a root-motion clip, and a
    // track for a bone the body lacks (-1) is skipped.
    #[test]
    fn root_motion_and_missing_tracks() {
        let c = clip(
            1,
            true,
            vec![0, -1],
            vec![
                0.0, 0.0, 0.0, 1.0, 5.0, 6.0, 7.0, 0.0, 0.0, 0.0, 1.0, 5.0, 6.0, 7.0,
            ],
        );
        let mut pose = Pose::new(2, 0);
        pose.add(&c, 0.0, 1.0, true);
        assert_eq!(pose.t[0..3], [0.0, 0.0, 0.0]);
        assert_eq!(pose.w, [1.0, 0.0]);
    }

    // Looping wraps on frames - 1, backwards too.
    #[test]
    fn looping_wraps() {
        let c = clip(
            3,
            false,
            vec![0],
            vec![
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 10.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 20.0, 0.0, 0.0,
            ],
        );
        let mut pose = Pose::new(1, -1);
        pose.add(&c, -0.5 / 30.0, 1.0, true);
        assert_eq!(pose.t[0], 15.0);
    }
}

pub mod prop;
pub mod rig;

pub mod display;

pub mod first_person;

pub mod timeline_layout;
pub mod timeline;

pub mod smoothing;
