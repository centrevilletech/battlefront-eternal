//! How far below its origin a flyer stands on its landing gear: src/flight/
//! gear.js gearFoot, from the vehicle template's drawn batches (src/
//! vehicle-rig.js vehicleBatches: every part in the rig's bind frame, on its
//! joints) posed at the takeoff clip's first frame (src/anim.js Pose, its
//! Float32Array accumulators and all). A load-time number of the class:
//! flight/model.js lands and lifts the craft by it.

use crate::clips::Clip;
use crate::derived::{rig_bones, RigBone};
use crate::js::{js_max, js_min, json_stringify, to_uint16};
use crate::models::Part;
use crate::three::{Matrix4, Quaternion, Vector3};
use crate::value::Value;

/// A batch's skinning inputs (vehicleBatches' position, skinIndex and
/// skinWeight attributes), in its vertices' order.
#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub positions: Vec<f32>,
    pub joints: Vec<u16>,
    pub weights: Vec<f32>,
}

/// vehicle-rig.js vehicleBatches(rig, parts) as a Vehicles template makes
/// them (no separate bones): parts grouped by [texture, flags, scroll] in
/// first-seen order, a rigid part's vertices carried to the rig's bind
/// frame by its bone's world matrix (a root's world is its local itself),
/// a skinned part's kept, each vertex on its joint (a soft skin's three).
pub fn vehicle_batches(rig: &Value, parts: &[Part]) -> Vec<Batch> {
    let bones = rig_bones(rig);
    let mut world: Vec<Option<Matrix4>> = vec![None; bones.len()];
    fn resolve(i: usize, bones: &[RigBone], world: &mut Vec<Option<Matrix4>>) -> Matrix4 {
        if let Some(m) = world[i] {
            return m;
        }
        let p = bones[i].parent;
        let m = if p >= 0.0 { Matrix4::multiply_matrices(&resolve(p as usize, bones, world), &bones[i].local) } else { bones[i].local };
        world[i] = Some(m);
        m
    }
    let mut worlds: Vec<Matrix4> = (0..bones.len()).map(|i| resolve(i, &bones, &mut world)).collect();
    worlds.push(Matrix4::identity());
    // names: lower-case bone name -> index (a later bone of the same name wins).
    let joint_of = |part: &Part| -> usize {
        let name = part.bone().unwrap_or("").to_lowercase();
        bones.iter().rposition(|b| b.name.to_lowercase() == name).unwrap_or(bones.len())
    };
    let mut groups: Vec<(String, Vec<&Part>)> = Vec::new();
    for part in parts {
        let key = format!(
            "[{},{},{}]",
            json_stringify(part.field("texture")),
            json_stringify(part.field("flags")),
            json_stringify(part.field("scroll"))
        );
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some(g) => g.1.push(part),
            None => groups.push((key, vec![part])),
        }
    }
    let mut out = Vec::with_capacity(groups.len());
    for (_, group) in groups {
        let count: usize = group.iter().map(|p| p.count()).sum();
        let mut batch = Batch { positions: vec![0.0; count * 3], joints: vec![0; count * 4], weights: vec![0.0; count * 4] };
        let mut vertex = 0;
        for part in group {
            let joint = joint_of(part);
            let matrix = worlds[joint];
            let joints = part.field("joints").filter(|j| j.truthy());
            let skin_joints = part.field("skinJoints").filter(|j| j.truthy());
            let skin_weights = part.field("skinWeights");
            for i in 0..part.count() {
                let mut point = part.position(i);
                if joints.is_none() {
                    point.apply_matrix4(&matrix);
                }
                batch.positions[vertex * 3] = point.x as f32;
                batch.positions[vertex * 3 + 1] = point.y as f32;
                batch.positions[vertex * 3 + 2] = point.z as f32;
                // part.joints?.[i] ?? joint
                let own = match joints.and_then(|j| j.at(i)) {
                    Some(Value::Null) | None => joint as f64,
                    Some(v) => crate::js::to_number(Some(v)),
                };
                batch.joints[vertex * 4] = to_uint16(own);
                batch.weights[vertex * 4] = 1.0;
                if let Some(sj) = skin_joints {
                    for k in 0..3 {
                        let j = sj.at(i * 3 + k).map(|v| crate::js::to_number(Some(v))).unwrap_or(f64::NAN);
                        let w = skin_weights.and_then(|sw| sw.at(i * 3 + k)).map(|v| crate::js::to_number(Some(v))).unwrap_or(f64::NAN);
                        batch.joints[vertex * 4 + k] = to_uint16(j);
                        batch.weights[vertex * 4 + k] = w as f32;
                    }
                }
                vertex += 1;
            }
        }
        out.push(batch);
    }
    out
}

/// anim.js Pose: Float32Array accumulators (every `+=` rounds to f32).
struct Pose {
    q: Vec<f32>,
    t: Vec<f32>,
    w: Vec<f32>,
    root: f64,
}

impl Pose {
    fn new(count: usize, root: f64) -> Pose {
        Pose { q: vec![0.0; count * 4], t: vec![0.0; count * 3], w: vec![0.0; count], root }
    }

    /// Pose.add(clip, time, weight, loop).
    fn add(&mut self, clip: &Clip, time: f64, weight: f64, looping: bool) {
        if weight <= 1e-4 {
            return;
        }
        let frames = clip.frames_f64();
        let mut f = 0.0;
        if frames > 1.0 {
            let span = frames - 1.0;
            f = time * clip.fps;
            f = if looping { ((f % span) + span) % span } else { js_min(js_max(f, 0.0), span) };
        }
        let i0 = f.floor();
        let i1 = js_min(i0 + 1.0, frames - 1.0);
        let a = f - i0;
        let d = clip.data();
        let at = |i: f64| -> f64 {
            if i >= 0.0 && i.fract() == 0.0 && (i as usize) < d.len() {
                d[i as usize] as f64
            } else {
                f64::NAN
            }
        };
        let stride = crate::clips::STRIDE;
        for (k, &bone) in clip.tracks.iter().enumerate() {
            if bone < 0.0 {
                continue;
            }
            let k = k as f64;
            let p0 = (k * frames + i0) * stride;
            let p1 = (k * frames + i1) * stride;
            let (mut x1, mut y1, mut z1, mut w1) = (at(p1), at(p1 + 1.0), at(p1 + 2.0), at(p1 + 3.0));
            if at(p0) * x1 + at(p0 + 1.0) * y1 + at(p0 + 2.0) * z1 + at(p0 + 3.0) * w1 < 0.0 {
                x1 = -x1;
                y1 = -y1;
                z1 = -z1;
                w1 = -w1;
            }
            let mut qx = at(p0) + (x1 - at(p0)) * a;
            let mut qy = at(p0 + 1.0) + (y1 - at(p0 + 1.0)) * a;
            let mut qz = at(p0 + 2.0) + (z1 - at(p0 + 2.0)) * a;
            let mut qw = at(p0 + 3.0) + (w1 - at(p0 + 3.0)) * a;
            let len = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
            let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
            qx /= len;
            qy /= len;
            qz /= len;
            qw /= len;
            let b = bone as usize;
            if b >= self.w.len() {
                continue;
            }
            let qi = b * 4;
            let acc = &mut self.q;
            if acc[qi] as f64 * qx + acc[qi + 1] as f64 * qy + acc[qi + 2] as f64 * qz + acc[qi + 3] as f64 * qw < 0.0 {
                qx = -qx;
                qy = -qy;
                qz = -qz;
                qw = -qw;
            }
            acc[qi] = (acc[qi] as f64 + qx * weight) as f32;
            acc[qi + 1] = (acc[qi + 1] as f64 + qy * weight) as f32;
            acc[qi + 2] = (acc[qi + 2] as f64 + qz * weight) as f32;
            acc[qi + 3] = (acc[qi + 3] as f64 + qw * weight) as f32;
            let ti = b * 3;
            if bone != self.root || !clip.root_motion {
                for c in 0..3 {
                    let c0 = at(p0 + 4.0 + c as f64);
                    let c1 = at(p1 + 4.0 + c as f64);
                    self.t[ti + c] = (self.t[ti + c] as f64 + (c0 + (c1 - c0) * a) * weight) as f32;
                }
            }
            self.w[b] = (self.w[b] as f64 + weight) as f32;
        }
    }

    /// Pose.apply(bones): each touched bone's quaternion (normalized, as
    /// three's Quaternion.normalize) and position (over its weight).
    fn apply(&self, posed: &mut [(Vector3, Quaternion)]) {
        for (i, bone) in posed.iter_mut().enumerate() {
            let w = self.w[i] as f64;
            if w <= 1e-6 {
                continue;
            }
            let qi = i * 4;
            let mut q = Quaternion { x: self.q[qi] as f64, y: self.q[qi + 1] as f64, z: self.q[qi + 2] as f64, w: self.q[qi + 3] as f64 };
            let l = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
            if l == 0.0 {
                q = Quaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
            } else {
                let l = 1.0 / l;
                q = Quaternion { x: q.x * l, y: q.y * l, z: q.z * l, w: q.w * l };
            }
            bone.1 = q;
            let ti = i * 3;
            bone.0 = Vector3::new(self.t[ti] as f64 / w, self.t[ti + 1] as f64 / w, self.t[ti + 2] as f64 / w);
        }
    }
}

/// gear.js gearFoot(rig, batches, clip): the lowest point of the model at
/// the clip's first frame, negated; None without frames, bones or a finite
/// point.
pub fn gear_foot(rig: &Value, batches: &[Batch], clip: &Clip) -> Option<f64> {
    let frames = clip.frames_f64();
    if frames == 0.0 || frames.is_nan() {
        return None;
    }
    let bones = rig_bones(rig);
    if bones.is_empty() {
        return None;
    }
    let count = bones.len();
    let bind_trs: Vec<(Vector3, Quaternion, Vector3)> = bones.iter().map(|b| b.local.decompose()).collect();
    let bind: Vec<Matrix4> = bind_trs.iter().map(|(p, q, s)| Matrix4::compose(p, q, s)).collect();
    let mut pose = Pose::new(count, -1.0);
    pose.add(clip, 0.0, 1.0, false);
    let mut posed: Vec<(Vector3, Quaternion)> = bind_trs.iter().map(|(p, q, _)| (*p, *q)).collect();
    pose.apply(&mut posed);
    // The root stays at its bind (/^dummyroot$/i).
    if let Some(root) = bones.iter().position(|b| b.name.len() == 9 && b.name.eq_ignore_ascii_case("dummyroot")) {
        posed[root] = (bind_trs[root].0, bind_trs[root].1);
    }
    let local: Vec<Matrix4> = posed.iter().enumerate().map(|(i, (p, q))| Matrix4::compose(p, q, &bind_trs[i].2)).collect();
    let chain = |locals: &[Matrix4]| -> Vec<Matrix4> {
        fn resolve(i: usize, bones: &[RigBone], locals: &[Matrix4], world: &mut Vec<Option<Matrix4>>) -> Matrix4 {
            if let Some(m) = world[i] {
                return m;
            }
            let p = bones[i].parent;
            let m = if p >= 0.0 { Matrix4::multiply_matrices(&resolve(p as usize, bones, locals, world), &locals[i]) } else { locals[i] };
            world[i] = Some(m);
            m
        }
        let mut world = vec![None; count];
        (0..count).map(|i| resolve(i, &bones, locals, &mut world)).collect()
    };
    let bind_world = chain(&bind);
    let posed_world = chain(&local);
    let mut skin: Vec<[f64; 16]> = (0..count).map(|i| Matrix4::multiply_matrices(&posed_world[i], &bind_world[i].inverted()).elements).collect();
    skin.push(Matrix4::identity().elements);
    let mut low = f64::INFINITY;
    for batch in batches {
        let n = batch.positions.len() / 3;
        for v in 0..n {
            let (x, y, z) = (batch.positions[v * 3] as f64, batch.positions[v * 3 + 1] as f64, batch.positions[v * 3 + 2] as f64);
            let mut height = 0.0;
            for k in 0..4 {
                let w = batch.weights[v * 4 + k] as f64;
                if w == 0.0 || w.is_nan() {
                    continue;
                }
                let joint = batch.joints[v * 4 + k] as usize;
                let e = skin.get(joint).unwrap_or(&skin[count]);
                height += w * (e[1] * x + e[5] * y + e[9] * z + e[13]);
            }
            if height < low {
                low = height;
            }
        }
    }
    if low.is_finite() {
        Some(-low)
    } else {
        None
    }
}
