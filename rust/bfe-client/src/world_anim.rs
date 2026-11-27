//! Authored `worldAnimations` evaluator for placed props (looping frigates,
//! spinning fans/sensors, and rigid parent-child hierarchies).
//! All clip path integrals and record bindings are resolved at admission so
//! per-frame evaluation performs zero heap allocations.
use bfe_render::instances as I;
use serde_json::Value;
use std::collections::HashMap;
use three::{Euler, EulerOrder, Matrix4, Quaternion, Vector3};

const LOCAL_PATH_SAMPLES: usize = 256;

#[derive(Clone)]
struct Key {
    time: f64,
    value: [f64; 3],
    kind: u8,
    tangents: [f64; 6],
}

struct Clip {
    duration: f64,
    looping: bool,
    local_translation: bool,
    positions: Vec<Key>,
    rotations: Vec<Key>,
    /// Cumulative integrated displacement in object base frame for `local_translation` clips.
    local_path: Vec<[f64; 3]>,
}

struct Binding {
    clip: usize,
    /// `(record_index, base_world_matrix)` for every part belonging to this animated instance.
    records: Vec<(usize, [f64; 16])>,
    /// Hierarchy children: `(record_index, inv(parent_base) * child_base)`.
    children: Vec<(usize, [f64; 16])>,
}

#[derive(Default)]
pub struct WorldAnimations {
    clips: Vec<Clip>,
    bindings: Vec<Binding>,
}

fn parse_keys(arr: Option<&Vec<Value>>) -> Vec<Key> {
    let Some(arr) = arr else {
        return Vec::new();
    };
    let mut keys: Vec<Key> = arr
        .iter()
        .filter_map(|k| {
            let time = k.get("time")?.as_f64()?;
            let val = k.get("value")?.as_array()?;
            if val.len() < 3 {
                return None;
            }
            let value = [
                val[0].as_f64().unwrap_or(0.0),
                val[1].as_f64().unwrap_or(0.0),
                val[2].as_f64().unwrap_or(0.0),
            ];
            let kind = k.get("type").and_then(Value::as_u64).unwrap_or(1) as u8;
            let mut tangents = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            if let Some(t) = k.get("tangents").and_then(Value::as_array) {
                for (i, v) in t.iter().take(6).enumerate() {
                    tangents[i] = v.as_f64().unwrap_or(0.0);
                }
            }
            Some(Key {
                time,
                value,
                kind,
                tangents,
            })
        })
        .collect();
    keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    keys
}

fn sample_keys(keys: &[Key], t: f64) -> [f64; 3] {
    if keys.is_empty() {
        return [0.0; 3];
    }
    if keys.len() == 1 || t <= keys[0].time {
        return keys[0].value;
    }
    let last = keys.len() - 1;
    if t >= keys[last].time {
        return keys[last].value;
    }
    // Binary search for segment [i, i + 1] containing t.
    let idx = keys.partition_point(|k| k.time <= t).saturating_sub(1).min(last - 1);
    let a = &keys[idx];
    let b = &keys[idx + 1];
    let dt = b.time - a.time;
    if dt <= 1e-12 {
        return a.value;
    }
    let u = ((t - a.time) / dt).clamp(0.0, 1.0);
    if a.kind == 2 || a.kind == 3 {
        let u2 = u * u;
        let u3 = u2 * u;
        let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
        let h10 = u3 - 2.0 * u2 + u;
        let h01 = -2.0 * u3 + 3.0 * u2;
        let h11 = u3 - u2;
        std::array::from_fn(|k| {
            let slope_a = a.tangents[3 + k];
            let slope_b = b.tangents[k];
            h00 * a.value[k] + h10 * dt * slope_a + h01 * b.value[k] + h11 * dt * slope_b
        })
    } else {
        std::array::from_fn(|k| a.value[k] + (b.value[k] - a.value[k]) * u)
    }
}

fn euler_quat(rot: [f64; 3]) -> Quaternion {
    let mut q = Quaternion::default();
    let e = Euler::new(rot[0], rot[1], rot[2], EulerOrder::YXZ);
    q.set_from_euler(&e);
    q
}

fn rotate_vec(q: &Quaternion, v: [f64; 3]) -> [f64; 3] {
    let mut p = Vector3::new(v[0], v[1], v[2]);
    p.apply_quaternion(q);
    [p.x, p.y, p.z]
}

fn mul_mat4(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    std::array::from_fn(|i| {
        let (c, r) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum()
    })
}

fn invert_mat4(m: &[f64; 16]) -> [f64; 16] {
    let mut mat = Matrix4 { elements: *m };
    mat.invert();
    mat.elements
}

impl Clip {
    fn from_json(v: &Value) -> Option<Self> {
        let duration = v.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
        let looping = v.get("loop").and_then(Value::as_bool).unwrap_or(false);
        let local_translation = v
            .get("localTranslation")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let positions = parse_keys(v.get("positions").and_then(Value::as_array));
        let rotations = parse_keys(v.get("rotations").and_then(Value::as_array));
        if duration <= 0.0 || (positions.is_empty() && rotations.is_empty()) {
            return None;
        }
        let mut local_path = Vec::new();
        if local_translation && positions.len() >= 2 {
            local_path.reserve(LOCAL_PATH_SAMPLES + 1);
            let mut acc = [0.0; 3];
            local_path.push(acc);
            let step = duration / LOCAL_PATH_SAMPLES as f64;
            let mut prev = sample_keys(&positions, 0.0);
            for i in 0..LOCAL_PATH_SAMPLES {
                let t1 = (i + 1) as f64 * step;
                let t_mid = (i as f64 + 0.5) * step;
                let next = sample_keys(&positions, t1);
                let dp = [next[0] - prev[0], next[1] - prev[1], next[2] - prev[2]];
                let q = euler_quat(sample_keys(&rotations, t_mid));
                let world_dp = rotate_vec(&q, dp);
                for k in 0..3 {
                    acc[k] += world_dp[k];
                }
                local_path.push(acc);
                prev = next;
            }
        }
        Some(Self {
            duration,
            looping,
            local_translation,
            positions,
            rotations,
            local_path,
        })
    }

    fn evaluate(&self, time: f64) -> [f64; 16] {
        let t = if !time.is_finite() || self.duration <= 0.0 {
            0.0
        } else if self.looping {
            time.rem_euclid(self.duration)
        } else {
            time.clamp(0.0, self.duration)
        };
        let rot = sample_keys(&self.rotations, t);
        let q = euler_quat(rot);
        let pos = if self.local_translation && self.local_path.len() == LOCAL_PATH_SAMPLES + 1 {
            let s = (t / self.duration) * LOCAL_PATH_SAMPLES as f64;
            let i0 = (s.floor() as usize).min(LOCAL_PATH_SAMPLES - 1);
            let frac = (s - i0 as f64).clamp(0.0, 1.0);
            let p0 = self.local_path[i0];
            let p1 = self.local_path[i0 + 1];
            [
                p0[0] + (p1[0] - p0[0]) * frac,
                p0[1] + (p1[1] - p0[1]) * frac,
                p0[2] + (p1[2] - p0[2]) * frac,
            ]
        } else {
            sample_keys(&self.positions, t)
        };
        let mut m = Matrix4::default();
        m.compose(
            &Vector3::new(pos[0], pos[1], pos[2]),
            &q,
            &Vector3::new(1.0, 1.0, 1.0),
        );
        m.elements
    }
}

impl WorldAnimations {
    /// Build cold bindings from `map["worldAnimations"]`, matching authored object names
    /// to admitted static prop records.
    pub fn new(
        world_anim: &Value,
        record_names: &[Option<String>],
        records: &[[f64; I::REC_STRIDE]],
    ) -> Self {
        let Some(clips_obj) = world_anim.get("clips").and_then(Value::as_object) else {
            return Self::default();
        };
        let Some(groups_obj) = world_anim.get("groups").and_then(Value::as_object) else {
            return Self::default();
        };

        let mut clip_map: HashMap<String, usize> = HashMap::new();
        let mut clips = Vec::new();
        for (name, val) in clips_obj {
            if let Some(clip) = Clip::from_json(val) {
                clip_map.insert(name.to_ascii_lowercase(), clips.len());
                clips.push(clip);
            }
        }
        if clips.is_empty() {
            return Self::default();
        }

        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, name) in record_names.iter().enumerate() {
            if let Some(n) = name {
                by_name.entry(n.to_ascii_lowercase()).or_default().push(idx);
            }
        }

        let mut hierarchy_children: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(hierarchies) = world_anim.get("hierarchies").and_then(Value::as_array) {
            for h in hierarchies {
                let Some(parent) = h.get("parent").and_then(Value::as_str) else {
                    continue;
                };
                let Some(children) = h.get("children").and_then(Value::as_array) else {
                    continue;
                };
                let entry = hierarchy_children
                    .entry(parent.to_ascii_lowercase())
                    .or_default();
                for c in children.iter().filter_map(Value::as_str) {
                    entry.push(c.to_ascii_lowercase());
                }
            }
        }

        let mut bindings = Vec::new();
        for group in groups_obj.values() {
            if !group.get("defaultOn").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let disable_hier = group
                .get("disableHierarchies")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let Some(group_bindings) = group.get("bindings").and_then(Value::as_array) else {
                continue;
            };
            for b in group_bindings {
                let Some(clip_name) = b.get("clip").and_then(Value::as_str) else {
                    continue;
                };
                let Some(obj_name) = b.get("object").and_then(Value::as_str) else {
                    continue;
                };
                let Some(&clip_idx) = clip_map.get(&clip_name.to_ascii_lowercase()) else {
                    continue;
                };
                let obj_lower = obj_name.to_ascii_lowercase();
                let Some(rec_indices) = by_name.get(&obj_lower) else {
                    continue;
                };
                let mut parent_records = Vec::with_capacity(rec_indices.len());
                for &r in rec_indices {
                    let mut base = [0.0; 16];
                    base.copy_from_slice(&records[r][I::REC_WORLD..I::REC_WORLD + 16]);
                    parent_records.push((r, base));
                }
                let mut children = Vec::new();
                if !disable_hier {
                    if let (Some(&(_, parent_base)), Some(child_names)) =
                        (parent_records.first(), hierarchy_children.get(&obj_lower))
                    {
                        let parent_inv = invert_mat4(&parent_base);
                        for cname in child_names {
                            if let Some(child_recs) = by_name.get(cname) {
                                for &cr in child_recs {
                                    let mut child_base = [0.0; 16];
                                    child_base.copy_from_slice(
                                        &records[cr][I::REC_WORLD..I::REC_WORLD + 16],
                                    );
                                    let local = mul_mat4(&parent_inv, &child_base);
                                    children.push((cr, local));
                                }
                            }
                        }
                    }
                }
                bindings.push(Binding {
                    clip: clip_idx,
                    records: parent_records,
                    children,
                });
            }
        }

        Self { clips, bindings }
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Fixed simulation tick rate used to synchronize animated world geometry
    /// with simulation collision.
    pub const SIM_HZ: f64 = 60.0;

    /// Convert a simulation tick and interpolation fraction `[0, 1]` into
    /// animation time in seconds.
    pub fn sim_time(tick: u32, alpha: f64) -> f64 {
        let frac = if alpha.is_finite() { alpha.clamp(0.0, 1.0) } else { 0.0 };
        (f64::from(tick) + frac) / Self::SIM_HZ
    }

    /// Update animated prop matrices from a fixed 60 Hz simulation tick and
    /// frame interpolation fraction. Zero allocations.
    pub fn apply_sim(&self, tick: u32, alpha: f64, records: &mut [[f64; I::REC_STRIDE]]) {
        self.apply(Self::sim_time(tick, alpha), records);
    }

    /// Update animated prop matrices in-place at `time` seconds. Zero allocations.
    pub fn apply(&self, time: f64, records: &mut [[f64; I::REC_STRIDE]]) {
        if self.bindings.is_empty() {
            return;
        }
        for binding in &self.bindings {
            let delta = self.clips[binding.clip].evaluate(time);
            let mut primary_world = None;
            for &(r, ref base) in &binding.records {
                let world = mul_mat4(base, &delta);
                records[r][I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&world);
                if primary_world.is_none() {
                    primary_world = Some(world);
                }
            }
            if let Some(parent_world) = primary_world {
                for &(cr, ref child_local) in &binding.children {
                    let child_world = mul_mat4(&parent_world, child_local);
                    records[cr][I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&child_world);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity_record(pos: [f64; 3]) -> [f64; I::REC_STRIDE] {
        let mut r = [0.0; I::REC_STRIDE];
        let mut m = Matrix4::default();
        m.set_position(pos[0], pos[1], pos[2]);
        r[I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&m.elements);
        r
    }

    #[test]
    fn non_looping_hermite_lift_clamps_and_honors_sim_tick_interpolation() {
        let wa = json!({
            "clips": {
                "lift_rise": {
                    "duration": 2.5,
                    "loop": false,
                    "localTranslation": false,
                    "positions": [
                        {"time": 0.0, "value": [0.0, 0.0, 0.0], "type": 2, "tangents": [0.0, 0.0, 0.0, 2.0, 0.0, 0.0]},
                        {"time": 2.5, "value": [4.0, 10.0, 0.0], "type": 2, "tangents": [0.0, 6.0, 0.0, 0.0, 0.0, 0.0]}
                    ],
                    "rotations": [
                        {"time": 0.0, "value": [0.0, 0.0, 0.0], "type": 1},
                        {"time": 2.5, "value": [std::f64::consts::FRAC_PI_2, 0.0, 0.0], "type": 1}
                    ]
                }
            },
            "groups": {
                "active_lift": {"defaultOn": true, "bindings": [{"clip": "lift_rise", "object": "platform_a"}]},
                "standby_lift": {"defaultOn": false, "bindings": [{"clip": "lift_rise", "object": "platform_b"}]}
            },
            "hierarchies": []
        });
        let mut recs = vec![
            identity_record([3.0, 1.0, -4.0]),
            identity_record([-8.0, 2.0, 6.0]),
        ];
        let names = vec![Some("platform_a".into()), Some("platform_b".into())];
        let anim = WorldAnimations::new(&wa, &names, &recs);
        assert!(!anim.is_empty());

        // Midpoint u = 0.5 at tick 75 (t = 1.25 s):
        // h00 = 0.5, h10 = 0.125, h01 = 0.5, h11 = -0.125, dt = 2.5
        // dx = 0.5*0 + 0.125*2.5*2 + 0.5*4 + (-0.125)*2.5*0 = 2.625
        // dy = 0.5*0 + 0.125*2.5*0 + 0.5*10 + (-0.125)*2.5*6 = 3.125
        assert!((WorldAnimations::sim_time(75, 0.0) - 1.25).abs() < 1e-12);
        anim.apply_sim(75, 0.0, &mut recs);
        assert!((recs[0][I::REC_WORLD + 12] - 5.625).abs() < 1e-9);
        assert!((recs[0][I::REC_WORLD + 13] - 4.125).abs() < 1e-9);
        assert!((recs[0][I::REC_WORLD + 14] - (-4.0)).abs() < 1e-9);
        // Pitch rotated 45 deg around X: cos(pi/4) in m[5].
        assert!((recs[0][I::REC_WORLD + 5] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        // Inactive group leaves platform_b untouched.
        assert_eq!(&recs[1][I::REC_WORLD + 12..I::REC_WORLD + 15], &[-8.0, 2.0, 6.0]);

        // Tick 74 + alpha 1.0 equals tick 75 + alpha 0.0.
        let mut interp = vec![identity_record([3.0, 1.0, -4.0]), identity_record([-8.0, 2.0, 6.0])];
        anim.apply_sim(74, 1.0, &mut interp);
        assert!((interp[0][I::REC_WORLD + 12] - recs[0][I::REC_WORLD + 12]).abs() < 1e-12);
        assert!((interp[0][I::REC_WORLD + 13] - recs[0][I::REC_WORLD + 13]).abs() < 1e-12);

        // Past duration clamps at endpoint (4, 10, 0) instead of wrapping.
        anim.apply_sim(300, 0.0, &mut recs);
        assert!((recs[0][I::REC_WORLD + 12] - 7.0).abs() < 1e-9);
        assert!((recs[0][I::REC_WORLD + 13] - 11.0).abs() < 1e-9);
    }

    #[test]
    fn local_translation_quarter_arc_carries_children_and_honors_disable_hierarchies() {
        let wa = json!({
            "clips": {
                "arc_sweep": {
                    "duration": 4.0,
                    "loop": true,
                    "localTranslation": true,
                    "positions": [
                        {"time": 0.0, "value": [0.0, 0.0, 0.0], "type": 1},
                        {"time": 4.0, "value": [20.0, 0.0, 0.0], "type": 1}
                    ],
                    "rotations": [
                        {"time": 0.0, "value": [0.0, 0.0, 0.0], "type": 1},
                        {"time": 2.0, "value": [0.0, std::f64::consts::FRAC_PI_4, 0.0], "type": 1},
                        {"time": 4.0, "value": [0.0, std::f64::consts::FRAC_PI_2, 0.0], "type": 1}
                    ]
                }
            },
            "groups": {
                "linked": {"defaultOn": true, "bindings": [{"clip": "arc_sweep", "object": "arm_root"}]},
                "unlinked": {"defaultOn": true, "disableHierarchies": true, "bindings": [{"clip": "arc_sweep", "object": "solo_root"}]}
            },
            "hierarchies": [
                {"parent": "arm_root", "children": ["arm_tip"]},
                {"parent": "solo_root", "children": ["solo_tip"]}
            ]
        });
        let mut recs = vec![
            identity_record([10.0, 0.0, 0.0]),
            identity_record([10.0, 3.0, -2.0]),
            identity_record([0.0, 0.0, 0.0]),
            identity_record([0.0, 5.0, 0.0]),
        ];
        let names = vec![
            Some("arm_root".into()),
            Some("arm_tip".into()),
            Some("solo_root".into()),
            Some("solo_tip".into()),
        ];
        let anim = WorldAnimations::new(&wa, &names, &recs);

        // Quarter-circle integral of speed 5 along +X rotated by yaw(t) = (pi/8) * t:
        // end_x = 40/pi ~= 12.7324, end_z = -40/pi ~= -12.7324.
        let end = anim.clips[0].local_path[LOCAL_PATH_SAMPLES];
        let analytic = 40.0 / std::f64::consts::PI;
        assert!((end[0] - analytic).abs() < 0.05, "unexpected arc x: {}", end[0]);
        assert!(end[1].abs() < 1e-9);
        assert!((end[2] + analytic).abs() < 0.05, "unexpected arc z: {}", end[2]);

        // At t = 2.0 s (tick 120), yaw = 45 deg, so child offset (0, 3, -2) rotates to (-sqrt(2), 3, -sqrt(2)).
        anim.apply_sim(120, 0.0, &mut recs);
        let px = recs[0][I::REC_WORLD + 12];
        let pz = recs[0][I::REC_WORLD + 14];
        assert!((recs[1][I::REC_WORLD + 12] - (px - std::f64::consts::SQRT_2)).abs() < 1e-5);
        assert!((recs[1][I::REC_WORLD + 13] - 3.0).abs() < 1e-5);
        assert!((recs[1][I::REC_WORLD + 14] - (pz - std::f64::consts::SQRT_2)).abs() < 1e-5);
        // disableHierarchies keeps solo_tip at its initial position while solo_root moves.
        assert!(recs[2][I::REC_WORLD + 12] > 1.0);
        assert_eq!(&recs[3][I::REC_WORLD + 12..I::REC_WORLD + 15], &[0.0, 5.0, 0.0]);

        // Looping wraps tick 240 (t = 4.0 s) back to origin.
        anim.apply_sim(240, 0.0, &mut recs);
        assert!((recs[0][I::REC_WORLD + 12] - 10.0).abs() < 1e-6);
        assert!(recs[0][I::REC_WORLD + 14].abs() < 1e-6);
    }
}
