//! World object keyframe animations and object hierarchies from prepared 2005
//! maps (`map.json.worldAnimations` and layer `worldAnimations`).
//!
//! Shared between renderer and collision admission so moving props (ships,
//! fans, sensors) and their hierarchy-attached children (ship turrets) evaluate
//! the same deterministic transforms at any time `t` using `v8math`
//! transcendentals.
//!
//! Provisional / not yet settled: Euler XYZ composition order, cubic Hermite
//! tangent layout, `disableHierarchies` group semantics, and the 256-substep
//! integration cap for `localTranslation` curves; live moving-body collision
//! updates during simulation ticks are not wired yet.

use crate::js::to_number;
use crate::three::{Matrix4, Quaternion, Vector3};
use crate::value::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct WorldAnimKey {
    pub time: f64,
    pub value: [f64; 3],
    pub kind: u32,
    pub tangents: [f64; 6],
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldAnimClip {
    pub name: String,
    pub duration: f64,
    pub looping: bool,
    pub local_translation: bool,
    pub positions: Vec<WorldAnimKey>,
    pub rotations: Vec<WorldAnimKey>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldAnimBinding {
    pub clip: String,
    pub object: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldAnimGroup {
    pub name: String,
    pub default_on: bool,
    pub stop_when_controlled: bool,
    pub disable_hierarchies: bool,
    pub bindings: Vec<WorldAnimBinding>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldAnimHierarchy {
    pub parent: String,
    pub children: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldAnimations {
    pub clips: Vec<WorldAnimClip>,
    pub groups: Vec<WorldAnimGroup>,
    pub hierarchies: Vec<WorldAnimHierarchy>,
}

fn parse_vec3_strict(value: Option<&Value>, label: &str) -> Result<[f64; 3], String> {
    let arr = match value {
        None | Some(Value::Null) => return Ok([0.0; 3]),
        Some(v) => v
            .as_array()
            .ok_or_else(|| format!("{label}: expected 3-element array"))?,
    };
    if arr.len() != 3 { return Err(format!("{label}: invalid vector length")); }
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        let Some(entry) = arr.get(i) else {
            continue;
        };
        let v = to_number(Some(entry));
        if !v.is_finite() {
            return Err(format!("{label}: non-finite number at component {i}"));
        }
        *slot = v;
    }
    Ok(out)
}

fn parse_tangents_strict(value: Option<&Value>, label: &str) -> Result<[f64; 6], String> {
    let arr = match value {
        None | Some(Value::Null) => return Ok([0.0; 6]),
        Some(v) => v
            .as_array()
            .ok_or_else(|| format!("{label}: expected tangent array"))?,
    };
    if arr.len() != 6 { return Err(format!("{label}: invalid vector length")); }
    let mut out = [0.0; 6];
    for (i, slot) in out.iter_mut().enumerate() {
        let Some(entry) = arr.get(i) else {
            continue;
        };
        let v = to_number(Some(entry));
        if !v.is_finite() {
            return Err(format!("{label}: non-finite tangent at component {i}"));
        }
        *slot = v;
    }
    Ok(out)
}

fn parse_keys_strict(value: Option<&Value>, label: &str) -> Result<Vec<WorldAnimKey>, String> {
    let list = match value {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(v) => v
            .as_array()
            .ok_or_else(|| format!("{label}: expected keyframe array"))?,
    };
    if list.len() > 65536 { return Err(format!("{label}: too many keyframes")); }
    let mut out = Vec::with_capacity(list.len());
    for (i, k) in list.iter().enumerate() {
        let t = to_number(k.get("time"));
        if !t.is_finite() || t < 0.0 || out.last().is_some_and(|k: &WorldAnimKey| k.time > t) {
            return Err(format!("{label}[{i}]: non-finite keyframe time"));
        }
        let kind_num = k.get("type").map(|v| to_number(Some(v))).unwrap_or(1.0);
        if !kind_num.is_finite() || kind_num.fract() != 0.0 || !(0.0..=3.0).contains(&kind_num) {
            return Err(format!("{label}[{i}]: invalid keyframe type"));
        }
        out.push(WorldAnimKey {
            time: t,
            value: parse_vec3_strict(k.get("value"), &format!("{label}[{i}].value"))?,
            kind: kind_num as u32,
            tangents: parse_tangents_strict(k.get("tangents"), &format!("{label}[{i}].tangents"))?,
        });
    }
    Ok(out)
}

fn sample_channel(keys: &[WorldAnimKey], time: f64) -> [f64; 3] {
    if keys.is_empty() {
        return [0.0; 3];
    }
    if keys.len() == 1 || time <= keys[0].time {
        return keys[0].value;
    }
    if time >= keys[keys.len() - 1].time {
        return keys[keys.len() - 1].value;
    }
    for window in keys.windows(2) {
        let (a, b) = (&window[0], &window[1]);
        if time >= a.time && time <= b.time {
            let span = b.time - a.time;
            if span <= 0.0 {
                return b.value;
            }
            let u = v8math::min(1.0, v8math::max(0.0, (time - a.time) / span));
            // Type 1 is linear; other spline types use cubic Hermite tangents
            // when non-zero tangents are authored, collapsing cleanly to linear
            // when tangents are zero.
            if a.kind == 1 {
                return std::array::from_fn(|i| a.value[i] + (b.value[i] - a.value[i]) * u);
            }
            let u2 = u * u;
            let u3 = u2 * u;
            let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
            let h10 = u3 - 2.0 * u2 + u;
            let h01 = -2.0 * u3 + 3.0 * u2;
            let h11 = u3 - u2;
            return std::array::from_fn(|i| {
                let m0 = a.tangents[i + 3] * span;
                let m1 = b.tangents[i] * span;
                h00 * a.value[i] + h10 * m0 + h01 * b.value[i] + h11 * m1
            });
        }
    }
    keys[keys.len() - 1].value
}

/// Composes XYZ Euler angles (radians) into a quaternion using `v8math`
/// transcendentals so simulation and drawing share identical rotations.
pub fn euler_xyz_quaternion(angles: [f64; 3]) -> Quaternion {
    let (hx, hy, hz) = (angles[0] * 0.5, angles[1] * 0.5, angles[2] * 0.5);
    let (sx, cx) = (v8math::sin(hx), v8math::cos(hx));
    let (sy, cy) = (v8math::sin(hy), v8math::cos(hy));
    let (sz, cz) = (v8math::sin(hz), v8math::cos(hz));
    Quaternion {
        x: sx * cy * cz + cx * sy * sz,
        y: cx * sy * cz - sx * cy * sz,
        z: cx * cy * sz + sx * sy * cz,
        w: cx * cy * cz - sx * sy * sz,
    }
}

fn wrapped_time(clip: &WorldAnimClip, time_s: f64) -> f64 {
    if clip.duration <= 0.0 || !time_s.is_finite() {
        return 0.0;
    }
    if clip.looping {
        let mut t = time_s % clip.duration;
        if t < 0.0 {
            t += clip.duration;
        }
        t
    } else {
        v8math::min(clip.duration, v8math::max(0.0, time_s))
    }
}

/// Provisional integration substep density for local-translation curves (Hz).
pub const PROVISIONAL_LOCAL_INTEGRATION_SUBSTEPS_PER_SEC: f64 = 4.0;
/// Provisional substep cap for local-translation curve integration.
pub const PROVISIONAL_LOCAL_INTEGRATION_MAX_SUBSTEPS: usize = 256;

/// Deterministically samples `(position_delta, rotation_euler_rad)` for `clip`
/// at `time_s` using `v8math` transcendentals. Shared between drawing and
/// simulation moving-body collision so both always produce bit-equal poses.
pub fn sample_clip_at(clip: &WorldAnimClip, time_s: f64) -> ([f64; 3], [f64; 3]) {
    let t = wrapped_time(clip, time_s);
    let rot = sample_channel(&clip.rotations, t);
    let pos = if clip.local_translation && !clip.positions.is_empty() {
        let raw_steps = (t * PROVISIONAL_LOCAL_INTEGRATION_SUBSTEPS_PER_SEC).ceil();
        let steps = if !raw_steps.is_finite() || raw_steps < 1.0 {
            1usize
        } else if raw_steps as usize > PROVISIONAL_LOCAL_INTEGRATION_MAX_SUBSTEPS {
            PROVISIONAL_LOCAL_INTEGRATION_MAX_SUBSTEPS
        } else {
            raw_steps as usize
        };
        let mut accum = Vector3::default();
        let mut prev_p = sample_channel(&clip.positions, 0.0);
        for s in 1..=steps {
            let t0 = t * ((s - 1) as f64) / (steps as f64);
            let t1 = t * (s as f64) / (steps as f64);
            let cur_p = sample_channel(&clip.positions, t1);
            let dp = [cur_p[0] - prev_p[0], cur_p[1] - prev_p[1], cur_p[2] - prev_p[2]];
            let mid_r = sample_channel(&clip.rotations, 0.5 * (t0 + t1));
            let q = euler_xyz_quaternion(mid_r);
            let rot_m = Matrix4::compose(&Vector3::default(), &q, &Vector3::new(1.0, 1.0, 1.0));
            let mut step_v = Vector3::new(dp[0], dp[1], dp[2]);
            step_v.apply_matrix4(&rot_m);
            accum.x += step_v.x;
            accum.y += step_v.y;
            accum.z += step_v.z;
            prev_p = cur_p;
        }
        accum.to_array()
    } else {
        sample_channel(&clip.positions, t)
    };
    (pos, rot)
}

impl WorldAnimations {
    pub fn from_value(value: Option<&Value>) -> Self {
        Self::from_value_strict(value, "worldAnimations").unwrap_or_default()
    }

    /// Parses `worldAnimations`, returning an error naming `source` when any
    /// duration, keyframe time, value, or tangent is non-finite or malformed.
    pub fn from_value_strict(value: Option<&Value>, source: &str) -> Result<Self, String> {
        let Some(raw) = value.filter(|v| !v.is_null()) else {
            return Ok(Self::default());
        };
        let Some(obj) = raw.as_object() else {
            return Err(format!("{source}: worldAnimations must be a JSON object"));
        };
        for key in ["clips", "groups"] {
            if let Some(v) = obj.get(key) {
                if !v.as_object().is_some_and(|table| table.len() <= 4096) { return Err(format!("{source}: invalid {key} table")); }
            }
        }
        if obj.get("hierarchies").is_some_and(|v| !v.as_array().is_some_and(|a| a.len() <= 4096)) { return Err(format!("{source}: invalid animation hierarchies")); }
        let mut clips = Vec::new();
        if let Some(clip_map) = obj.get("clips").and_then(Value::as_object) {
            for (name, rec) in clip_map.entries() {
                let duration = to_number(rec.get("duration"));
                if !duration.is_finite() || duration < 0.0 {
                    return Err(format!("{source}: clip '{name}' has invalid duration"));
                }
                clips.push(WorldAnimClip {
                    name: name.to_string(),
                    duration,
                    looping: rec.get("loop").is_some_and(Value::truthy),
                    local_translation: rec.get("localTranslation").is_some_and(Value::truthy),
                    positions: parse_keys_strict(
                        rec.get("positions"),
                        &format!("{source}: clip '{name}'.positions"),
                    )?,
                    rotations: parse_keys_strict(
                        rec.get("rotations"),
                        &format!("{source}: clip '{name}'.rotations"),
                    )?,
                });
            }
        }
        let mut groups = Vec::new();
        if let Some(group_map) = obj.get("groups").and_then(Value::as_object) {
            for (name, rec) in group_map.entries() {
                let declared = rec.get("bindings").and_then(Value::as_array).ok_or_else(|| format!("{source}: invalid bindings for {name}"))?;
                if declared.len() > 4096 || declared.iter().any(|b| ["clip","object"].iter().any(|key| !b.get(key).and_then(Value::as_str).is_some_and(|s| !s.is_empty()))) {
                    return Err(format!("{source}: invalid binding for {name}"));
                }
                let bindings = rec
                    .get("bindings")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|b| {
                        let clip = b.get("clip").and_then(Value::as_str)?;
                        let object = b.get("object").and_then(Value::as_str)?;
                        Some(WorldAnimBinding {
                            clip: clip.to_string(),
                            object: object.to_string(),
                        })
                    })
                    .collect();
                groups.push(WorldAnimGroup {
                    name: name.to_string(),
                    default_on: rec.get("defaultOn").is_some_and(Value::truthy),
                    stop_when_controlled: rec.get("stopWhenControlled").is_some_and(Value::truthy),
                    disable_hierarchies: rec.get("disableHierarchies").is_some_and(Value::truthy),
                    bindings,
                });
            }
        }
        let mut hierarchies = Vec::new();
        if let Some(list) = obj.get("hierarchies").and_then(Value::as_array) {
            for h in list {
                let Some(parent) = h.get("parent").and_then(Value::as_str) else {
                    continue;
                };
                let children = h
                    .get("children")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                hierarchies.push(WorldAnimHierarchy {
                    parent: parent.to_string(),
                    children,
                });
            }
        }
        let anims = WorldAnimations { clips, groups, hierarchies };
        anims.validate_hierarchies(source)?;
        Ok(anims)
    }

    /// Rejects parent-child cycles in `hierarchies` (case-insensitive).
    pub fn validate_hierarchies(&self, source: &str) -> Result<(), String> {
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for h in &self.hierarchies {
            let parent = h.parent.to_ascii_lowercase();
            let entry = adj.entry(parent).or_default();
            for child in &h.children {
                entry.push(child.to_ascii_lowercase());
            }
        }
        // 0 = unvisited, 1 = visiting, 2 = done.
        let mut state: HashMap<String, u8> = HashMap::new();
        fn dfs(
            node: &str,
            adj: &HashMap<String, Vec<String>>,
            state: &mut HashMap<String, u8>,
            source: &str,
        ) -> Result<(), String> {
            match state.get(node).copied().unwrap_or(0) {
                2 => return Ok(()),
                1 => return Err(format!("{source}: hierarchy cycle involving '{node}'")),
                _ => {}
            }
            state.insert(node.to_string(), 1);
            if let Some(children) = adj.get(node) {
                for child in children {
                    dfs(child, adj, state, source)?;
                }
            }
            state.insert(node.to_string(), 2);
            Ok(())
        }
        let mut roots: Vec<String> = adj.keys().cloned().collect();
        roots.sort();
        for root in roots {
            dfs(&root, &adj, &mut state, source)?;
        }
        Ok(())
    }

    /// True when `name` is bound directly to a world animation group or attached
    /// as a child inside a world animation hierarchy (case-insensitive).
    pub fn animates_object(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        for group in &self.groups {
            if group
                .bindings
                .iter()
                .any(|b| b.object.eq_ignore_ascii_case(name))
            {
                return true;
            }
        }
        for h in &self.hierarchies {
            if h.parent.eq_ignore_ascii_case(name)
                || h.children.iter().any(|c| c.eq_ignore_ascii_case(name))
            {
                return true;
            }
        }
        false
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.groups.is_empty() && self.hierarchies.is_empty()
    }

    /// Merges a layer's world animations over the base map's animations.
    /// Matching clip/group names (ASCII case-insensitive) replace the earlier
    /// entry while keeping deterministic order.
    pub fn merge(&mut self, other: WorldAnimations) {
        for clip in other.clips {
            if let Some(slot) = self.clips.iter_mut().find(|c| c.name.eq_ignore_ascii_case(&clip.name)) {
                *slot = clip;
            } else {
                self.clips.push(clip);
            }
        }
        for group in other.groups {
            if let Some(slot) = self.groups.iter_mut().find(|g| g.name.eq_ignore_ascii_case(&group.name)) {
                *slot = group;
            } else {
                self.groups.push(group);
            }
        }
        for h in other.hierarchies {
            if let Some(slot) = self.hierarchies.iter_mut().find(|e| e.parent.eq_ignore_ascii_case(&h.parent)) {
                for child in h.children {
                    if !slot.children.iter().any(|c| c.eq_ignore_ascii_case(&child)) {
                        slot.children.push(child);
                    }
                }
            } else {
                self.hierarchies.push(h);
            }
        }
    }

    pub fn clip(&self, name: &str) -> Option<&WorldAnimClip> {
        self.clips.iter().find(|c| c.name == name)
            .or_else(|| self.clips.iter().find(|c| c.name.eq_ignore_ascii_case(name)))
    }

    /// Samples `(position_delta, rotation_euler_rad)` for a clip at `time_s`.
    pub fn sample_clip(&self, name: &str, time_s: f64) -> Option<([f64; 3], [f64; 3])> {
        let clip = self.clip(name)?;
        Some(sample_clip_at(clip, time_s))
    }

    /// Evaluates world matrices at `time_s` for all placed objects (in input
    /// order), applying active `default_on` animation groups and propagating
    /// parent motion to hierarchy children.
    pub fn evaluate_transforms(&self, objects: &[(String, [f64; 16])], time_s: f64) -> Vec<[f64; 16]> {
        let mut out: Vec<[f64; 16]> = objects.iter().map(|(_, m)| *m).collect();
        if self.is_empty() || objects.is_empty() {
            return out;
        }
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (name, _)) in objects.iter().enumerate() {
            if !name.is_empty() {
                by_name.entry(name.to_ascii_lowercase()).or_default().push(i);
            }
        }
        let mut disabled_parents: HashMap<String, bool> = HashMap::new();
        for group in &self.groups {
            if !group.default_on {
                continue;
            }
            for binding in &group.bindings {
                let Some((pos, rot)) = self.sample_clip(&binding.clip, time_s) else {
                    continue;
                };
                let key = binding.object.to_ascii_lowercase();
                if group.disable_hierarchies {
                    disabled_parents.insert(key.clone(), true);
                }
                let q = euler_xyz_quaternion(rot);
                let local_delta = Matrix4::compose(
                    &Vector3::new(pos[0], pos[1], pos[2]),
                    &q,
                    &Vector3::new(1.0, 1.0, 1.0),
                );
                if let Some(indices) = by_name.get(&key) {
                    for &idx in indices {
                        let base = Matrix4::from_array(&objects[idx].1);
                        out[idx] = Matrix4::multiply_matrices(&base, &local_delta).elements;
                    }
                }
            }
        }
        // Propagate parent world motion to attached hierarchy children:
        // child_world(t) = parent_world(t) * inverse(parent_base) * child_base.
        for hierarchy in &self.hierarchies {
            let parent_key = hierarchy.parent.to_ascii_lowercase();
            if disabled_parents.get(&parent_key).copied().unwrap_or(false) {
                continue;
            }
            let Some(parent_indices) = by_name.get(&parent_key) else {
                continue;
            };
            let Some(&parent_idx) = parent_indices.first() else {
                continue;
            };
            let parent_base = Matrix4::from_array(&objects[parent_idx].1);
            let parent_live = Matrix4::from_array(&out[parent_idx]);
            if parent_base == parent_live {
                continue;
            }
            let parent_delta = Matrix4::multiply_matrices(&parent_live, &parent_base.inverted());
            for child_name in &hierarchy.children {
                if let Some(child_indices) = by_name.get(&child_name.to_ascii_lowercase()) {
                    for &child_idx in child_indices {
                        let child_base = Matrix4::from_array(&objects[child_idx].1);
                        out[child_idx] = Matrix4::multiply_matrices(&parent_delta, &child_base).elements;
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn world_animation_evaluates_parent_and_hierarchy_child_transforms() {
        let authored = parse(
            r#"{
                "clips": {
                    "orbit": {
                        "duration": 10.0,
                        "loop": true,
                        "localTranslation": false,
                        "positions": [
                            {"time": 0.0, "value": [0.0, 0.0, 0.0], "type": 1},
                            {"time": 10.0, "value": [10.0, 0.0, 0.0], "type": 1}
                        ],
                        "rotations": [
                            {"time": 0.0, "value": [0.0, 0.0, 0.0], "type": 1},
                            {"time": 10.0, "value": [0.0, 0.0, 0.0], "type": 1}
                        ]
                    }
                },
                "groups": {
                    "ship_loop": {
                        "defaultOn": true,
                        "stopWhenControlled": false,
                        "disableHierarchies": false,
                        "bindings": [{"clip": "orbit", "object": "ship_alpha"}]
                    }
                },
                "hierarchies": [
                    {"parent": "ship_alpha", "children": ["ship_alpha_turret1"]}
                ]
            }"#,
        )
        .unwrap();
        let anims = WorldAnimations::from_value(Some(&authored));
        let mut ship = Matrix4::identity();
        ship.set_position(&Vector3::new(100.0, 20.0, 30.0));
        let mut turret = Matrix4::identity();
        turret.set_position(&Vector3::new(105.0, 24.0, 30.0));
        let objects = vec![
            ("ship_alpha".to_string(), ship.elements),
            ("ship_alpha_turret1".to_string(), turret.elements),
        ];
        // At t = 5s (and t = 15s wrapped), both ship and attached turret move +5m on X.
        for t in [5.0, 15.0] {
            let live = anims.evaluate_transforms(&objects, t);
            assert!((live[0][12] - 105.0).abs() < 1e-9);
            assert!((live[1][12] - 110.0).abs() < 1e-9);
            assert!((live[1][13] - 24.0).abs() < 1e-9);
        }
    }

    #[test]
    fn world_animation_rejects_hierarchy_cycle() {
        let cyclic = parse(
            r#"{
                "clips": {},
                "groups": {},
                "hierarchies": [
                    {"parent": "hull_a", "children": ["turret_a"]},
                    {"parent": "turret_a", "children": ["hull_a"]}
                ]
            }"#,
        )
        .unwrap();
        let err = WorldAnimations::from_value_strict(Some(&cyclic), "test").err().unwrap();
        assert!(err.contains("cycle"), "expected cycle error: {err}");
    }
}
