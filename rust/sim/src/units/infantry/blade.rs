//! The attacker's retained socket pose in the authoritative world.
use crate::combat::combo::{
    ActiveAttack, ComboMachine, ComboStateDef, DamageSample, DamageSampleList,
};
use bfe_content::{content::Content, Value};
use bfe_pose::{
    character::{array, field, Archive, Character, Local, Mask, Rig},
    Clip, Pose,
};
use three::{Matrix4, Quaternion, Vector3};

struct Layer {
    clip: Clip,
    moving: Vec<bool>,
}

struct Edge {
    joint: usize,
    offset: Matrix4,
}

#[derive(Default)]
pub(super) struct BladeHistory {
    ends: [Option<(u32, u32, Root, DamageSample)>; crate::combat::combo::MAX_BLADE_EDGES],
}

pub(super) struct BladePose {
    rig: Rig,
    layers: Vec<Layer>,
    edges: Vec<Edge>,
    throw_origin: [f64; 3],
    pub(super) reactions: Vec<(usize, f64)>,
    accumulator: Pose,
    local: Vec<Local>,
    matrices: Vec<Matrix4>,
}

#[cfg(test)]
impl BladePose {
    pub(super) fn empty() -> Self {
        let root = Matrix4::default().elements;
        let json = serde_json::json!({"parts":[],"bones":[{"name":"base","parent":-1,"local":root}]});
        let actor = bfe_content::actor::build_actor(
            &bfe_content::json::parse_bytes(&serde_json::to_vec(&json).unwrap()).unwrap(),
            &[],
        )
        .unwrap();
        let meta = bfe_content::json::parse_bytes(br#"{"fps":30}"#).unwrap();
        let bank = Archive::new(&meta, &[]).unwrap();
        let rig = Character::bind("figure", &Value::Null, &[], &actor, &bank, &bank)
            .unwrap()
            .rig;
        Self {
            rig,
            layers: Vec::new(),
            edges: Vec::new(),
            throw_origin: [0.; 3],
            reactions: Vec::new(),
            accumulator: Pose::new(1, -1),
            local: vec![Local::default()],
            matrices: vec![Matrix4::default()],
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
pub(super) struct Root {
    pub position: [f64; 3],
    pub yaw: f64,
}

/// Bone name, three components of its local firing axis, then the local origin.
fn attached_frame(record: &str) -> Result<(&str, Matrix4), String> {
    let mut words = record.split_ascii_whitespace();
    let joint = words.next().ok_or("missing attached firepoint joint")?;
    let mut frame = Matrix4::default();
    for i in [8, 9, 10, 12, 13, 14] {
        frame.elements[i] = words
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite())
            .ok_or("invalid attached firepoint vector")?;
    }
    if words.next().is_some() || frame.elements[8..11].iter().all(|v| *v == 0.) {
        return Err("invalid attached firepoint axis or extent".into());
    }
    Ok((joint, frame))
}

/// Whether a blade end last recorded on tick `last` carries over to `tick`: only to the very next
/// one, and never across the end of the tick range.
fn follows(last: u32, tick: u32) -> bool {
    last.checked_add(1) == Some(tick)
}

fn sample_joint_position(
    rig: &Rig,
    clip: Option<&Clip>,
    joint: usize,
    time: f64,
    accumulator: &mut Pose,
    local: &mut [Local],
    matrices: &mut [Matrix4],
) -> [f64; 3] {
    for (p, j) in local.iter_mut().zip(&rig.joints) {
        *p = j.bind;
    }
    if let Some(clip) = clip {
        accumulator.clear();
        accumulator.add(clip, time, 1., false);
        accumulator.apply_out();
        for &track in &clip.tracks {
            let i = track as usize;
            let info = &rig.joints[i];
            let k = &accumulator.out[7 * i..7 * i + 7];
            let mut position =
                info.animated_position(Vector3::new(k[4], k[5], k[6]), rig.root_scale);
            if clip.root_motion && info.parent.is_none() {
                position.x = info.bind.position.x;
                position.z = info.bind.position.z;
            }
            local[i] = Local {
                position,
                rotation: Quaternion::new(k[0], k[1], k[2], k[3]),
            };
        }
    }
    for &i in &rig.order {
        let info = &rig.joints[i];
        let p = &local[i];
        let mut matrix = Matrix4::default();
        matrix.compose(&p.position, &p.rotation, &info.scale);
        if let Some(parent) = info.parent {
            let mut world = matrices[parent];
            world.multiply(&matrix);
            matrix = world;
        }
        matrices[i] = matrix;
    }
    let m = matrices[joint].elements;
    [m[12], m[13], m[14]]
}

impl BladePose {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bind(
        content: &Content,
        name: &str,
        side: &str,
        unit: &Value,
        weapon: &Value,
        binding: &Value,
        machine: &ComboMachine,
        throw_release: Option<f64>,
    ) -> Result<Self, String> {
        let battle = content
            .battle
            .swbf2
            .as_ref()
            .ok_or("missing character banks")?;
        let model = format!("{side}/{}", field(unit, "model"));
        let (_, actor) = content
            .actors
            .iter()
            .find(|(key, _)| key == &model)
            .ok_or_else(|| format!("missing blade rig {model}"))?;
        let key = format!("{side}/{name}");
        let (_, banks) = battle
            .unit_anim_banks
            .iter()
            .find(|(unit, _)| unit == &key)
            .ok_or_else(|| format!("missing animation packages for {key}"))?;
        let (_, meta, bytes) = battle
            .side_anim_banks
            .iter()
            .find(|(s, _, _)| s == side)
            .ok_or_else(|| format!("missing side banks for {key}"))?;
        let side_bank = Archive::new(meta, bytes)?;
        let shared = Archive::new(&battle.common_anim_banks, &battle.common_anim_bin)?;
        let character = Character::bind(name, unit, banks, actor, &side_bank, &shared)?;
        let joints = character.rig.joints.len();
        let mut layers = Vec::with_capacity(machine.animations.len());
        for animation in &machine.animations {
            let clip = &character.clips[character.weapon_clip(binding, &animation.name)?];
            layers.push(Layer {
                clip: clip.simulation_clip(),
                moving: (0..joints)
                    .map(|i| clip.owns_joint(i, Mask::Moving))
                    .collect(),
            });
        }
        let reactions = machine
            .deflect_animations
            .iter()
            .map(|record| {
                let id = character.weapon_clip(binding, &record.clip_name)?;
                Ok::<_, String>((id, character.clips[id].duration()))
            })
            .collect::<Result<_, _>>()?;
        let mut edges = Vec::new();
        for hilt in array(weapon, "saberHilts") {
            let attachment = hilt.get("attachment").ok_or("missing hilt attachment")?;
            let joint = ["name", "bone"]
                .iter()
                .find_map(|k| character.rig.joint(field(attachment, k)))
                .or_else(|| {
                    u32::from_str_radix(field(attachment, "boneCrc"), 16)
                        .ok()
                        .and_then(|crc| character.rig.joints.iter().position(|j| j.crc == crc))
                })
                .ok_or("unbound hilt attachment")?;
            for blade in array(hilt, "blades") {
                let mut offset = Matrix4::default();
                let values = array(blade, "transform");
                if values.len() != 16 {
                    return Err("invalid blade transform extent".into());
                }
                for (out, value) in offset.elements.iter_mut().zip(values) {
                    *out = value
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or("invalid blade transform")?;
                }
                edges.push(Edge { joint, offset });
            }
        }
        // A damage edge may be attached directly to a body joint, independently
        // of the weapon model and its visible emitters. Preserve weapon order.
        let mut visible = edges.into_iter();
        let mut edges = Vec::new();
        for (field, value) in super::pairs(weapon) {
            if field.eq_ignore_ascii_case("FirePointName") {
                if let Some(edge) = visible.next() {
                    edges.push(edge);
                }
            } else if field.eq_ignore_ascii_case("AttachedFirePoint") {
                let (bone, offset) = attached_frame(value)?;
                let joint = character
                    .rig
                    .joint(bone)
                    .ok_or_else(|| format!("missing attached firepoint joint {bone}"))?;
                edges.push(Edge { joint, offset });
            }
        }
        edges.extend(visible);
        if machine
            .states
            .iter()
            .flat_map(|s| &s.attacks)
            .any(|a| a.edge as usize >= edges.len())
        {
            return Err(format!("missing damage edge for {key}"));
        }
        let mut accumulator = Pose::new(joints, -1);
        let mut local = vec![Local::default(); joints];
        let mut matrices = vec![Matrix4::default(); joints];
        let throw_origin = if let Some(primary) = edges.first() {
            let clip = match (throw_release, machine.throw_animation.as_deref()) {
                (Some(time), Some(anim)) => Some((
                    character.clips[character.weapon_clip(binding, anim)?].simulation_clip(),
                    time,
                )),
                _ => None,
            };
            sample_joint_position(
                &character.rig,
                clip.as_ref().map(|(c, _)| c),
                primary.joint,
                clip.as_ref().map_or(0., |(_, t)| *t),
                &mut accumulator,
                &mut local,
                &mut matrices,
            )
        } else {
            [0.; 3]
        };
        Ok(Self {
            rig: character.rig,
            layers,
            edges,
            throw_origin,
            reactions,
            accumulator,
            local,
            matrices,
        })
    }

    fn layer(&mut self, id: usize, time: f64, lower: Option<usize>) {
        let layer = &self.layers[id];
        self.accumulator.clear();
        self.accumulator.add(&layer.clip, time, 1., false);
        self.accumulator.apply_out();
        for &track in &layer.clip.tracks {
            let i = track as usize;
            if lower.is_some_and(|l| self.layers[l].moving[i] && !layer.moving[i]) {
                continue;
            }
            let joint = &self.rig.joints[i];
            let k = &self.accumulator.out[7 * i..7 * i + 7];
            let mut position =
                joint.animated_position(Vector3::new(k[4], k[5], k[6]), self.rig.root_scale);
            if layer.clip.root_motion && joint.parent.is_none() {
                position.x = joint.bind.position.x;
                position.z = joint.bind.position.z;
            }
            self.local[i] = Local {
                position,
                rotation: Quaternion::new(k[0], k[1], k[2], k[3]),
            };
        }
    }

    fn socket(
        &mut self,
        state: &ComboStateDef,
        edge: usize,
        raw: f64,
        time: f64,
    ) -> ([f64; 3], [f64; 3]) {
        for (p, j) in self.local.iter_mut().zip(&self.rig.joints) {
            *p = j.bind;
        }
        let lower = state.lower_animation.map(|id| usize::from(id.0));
        if let Some(id) = lower {
            let clip = &self.layers[id].clip;
            let duration = (clip.frames - 1) as f64 / clip.fps;
            let mode = state
                .lower_anim_layer
                .as_ref()
                .map_or(state.loop_mode, |l| l.loop_mode);
            self.layer(
                id,
                crate::combat::combo::clip_playback_time(raw, duration, mode),
                None,
            );
        }
        if let Some(id) = state.animation {
            self.layer(id.0 as usize, time, lower);
        }
        for &i in &self.rig.order {
            let joint = &self.rig.joints[i];
            let p = &self.local[i];
            let mut matrix = Matrix4::default();
            matrix.compose(&p.position, &p.rotation, &joint.scale);
            if let Some(parent) = joint.parent {
                let mut world = self.matrices[parent];
                world.multiply(&matrix);
                matrix = world;
            }
            self.matrices[i] = matrix;
        }
        let edge = &self.edges[edge];
        let mut frame = self.matrices[edge.joint];
        frame.multiply(&edge.offset);
        let m = frame.elements;
        ([m[12], m[13], m[14]], [m[8], m[9], m[10]])
    }

    pub(super) fn throw_hand(&self, root: Root) -> [f64; 3] {
        crate::wire::thrown::release_point(root.position, root.yaw, self.throw_origin)
    }

    /// The release point in the soldier's own frame.
    pub(super) fn throw_hand_actor(&self) -> [f64; 3] {
        self.throw_origin
    }

    #[cfg(test)]
    pub(super) fn from_throw_clip(rig: Rig, joint: usize, clip: Option<Clip>, time: f64) -> Self {
        let joints = rig.joints.len();
        let mut accumulator = Pose::new(joints, -1);
        let mut local = vec![Local::default(); joints];
        let mut matrices = vec![Matrix4::default(); joints];
        let throw_origin = sample_joint_position(
            &rig,
            clip.as_ref(),
            joint,
            time,
            &mut accumulator,
            &mut local,
            &mut matrices,
        );
        Self {
            rig,
            layers: Vec::new(),
            edges: vec![Edge {
                joint,
                offset: Matrix4::default(),
            }],
            throw_origin,
            reactions: Vec::new(),
            accumulator,
            local,
            matrices,
        }
    }

    pub(super) fn samples(
        &mut self,
        machine: &ComboMachine,
        history: &mut BladeHistory,
        active: &ActiveAttack,
        frame: &super::AttackFrame,
        after: Root,
        tick: u32,
    ) -> DamageSampleList {
        let state = machine.state(active.state_id);
        let edge = active.edge as usize;
        let prior = history.ends[edge];
        let before = frame.before;
        let turn = after.yaw - before.yaw;
        let turn = v8math::atan2(v8math::sin(turn), v8math::cos(turn));
        let samples = active.sample_with_time(machine, |raw, time| {
            if let Some((last, key, root, end)) = prior {
                if follows(last, tick) && key == active.attack_key && root == before && end.time == raw
                {
                    return (end.position, end.direction);
                }
            }
            let (p, d) = self.socket(state, edge, raw, time);
            let weight = ((raw - frame.start) / frame.dt).clamp(0., 1.);
            let (sin, cos) = v8math::sin_cos(before.yaw + weight * turn);
            let rotate = |v: [f64; 3]| [cos * v[0] + sin * v[2], v[1], -sin * v[0] + cos * v[2]];
            let point = rotate(p);
            (
                std::array::from_fn(|i| {
                    before.position[i]
                        + weight * (after.position[i] - before.position[i])
                        + point[i]
                }),
                rotate(d),
            )
        });
        if let Some(end) = samples.as_slice().last() {
            history.ends[edge] = Some((tick, active.attack_key, after, *end));
        }
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::combo::{ClipId, StateId};

    #[test]
    fn a_blade_end_carries_over_only_to_the_next_tick_even_at_the_tick_bound() {
        assert!(follows(10, 11));
        assert!(!follows(10, 10) && !follows(10, 12));
        assert!(follows(u32::MAX - 1, u32::MAX));
        assert!(!follows(u32::MAX, 0), "the tick bound does not wrap round to tick 0");
        assert!(!follows(u32::MAX, u32::MAX));
    }

    #[test]
    fn a_two_frame_parent_track_moves_its_child_socket() {
        let root = Matrix4::default().elements;
        let mut child = root;
        child[13] = 2.;
        let json = serde_json::json!({"parts":[],"bones":[
            {"name":"base","parent":-1,"local":root},
            {"name":"wrist","parent":0,"local":child}
        ]});
        let actor = bfe_content::actor::build_actor(
            &bfe_content::json::parse_bytes(&serde_json::to_vec(&json).unwrap()).unwrap(),
            &[],
        )
        .unwrap();
        let meta = bfe_content::json::parse_bytes(br#"{"fps":12}"#).unwrap();
        let bank = Archive::new(&meta, &[]).unwrap();
        let rig = Character::bind("figure", &Value::Null, &[], &actor, &bank, &bank)
            .unwrap()
            .rig;
        let (name, offset) = attached_frame("wrist 0 0 1 0.25 0 0").unwrap();
        let joint = rig.joint(name).unwrap();
        let mut pose = BladePose {
            rig,
            edges: vec![Edge { joint, offset }],
            throw_origin: [0., 2., 0.],
            reactions: vec![],
            layers: vec![Layer {
                clip: Clip {
                    frames: 2,
                    fps: 12.,
                    root_motion: false,
                    tracks: vec![0],
                    data: vec![0., 0., 0., 1., 0., 0., 0., 0., 0., 0., 1., 2., 0., 0.],
                },
                moving: vec![true, false],
            }],
            accumulator: Pose::new(2, -1),
            local: vec![Local::default(); 2],
            matrices: vec![Matrix4::default(); 2],
        };
        let mut state = ComboStateDef::new(StateId(0));
        state.animation = Some(ClipId(0));
        for (time, x) in [(0., 0.25), (1. / 24., 1.25), (1. / 12., 2.25)] {
            let (point, axis) = pose.socket(&state, 0, time, time);
            assert_eq!(point, [x, 2., 0.]);
            assert_eq!(axis, [0., 0., 1.]);
        }
        assert!(attached_frame("wrist 0 0 0 0 0 0").is_err());
        assert!(attached_frame("wrist 0 0 1 0 0").is_err());
    }

    #[test]
    fn throw_origin_samples_release_time_bone_without_edge_offset_and_rotates_with_yaw() {
        let root = Matrix4::default().elements;
        let mut child = root;
        child[12] = -0.10;
        child[13] = 1.10;
        child[14] = 0.05;
        let json = serde_json::json!({"parts":[],"bones":[
            {"name":"base","parent":-1,"local":root},
            {"name":"grip","parent":0,"local":child}
        ]});
        let actor = bfe_content::actor::build_actor(
            &bfe_content::json::parse_bytes(&serde_json::to_vec(&json).unwrap()).unwrap(),
            &[],
        )
        .unwrap();
        let meta = bfe_content::json::parse_bytes(br#"{"fps":20}"#).unwrap();
        let bank = Archive::new(&meta, &[]).unwrap();
        let rig = Character::bind("figure", &Value::Null, &[], &actor, &bank, &bank)
            .unwrap()
            .rig;
        let joint = rig.joint("grip").unwrap();
        // Two-frame throw clip moving grip from (-0.10, 1.10, 0.05) to (-0.34, 1.50, 0.65) over 0.50 s.
        let clip = Clip {
            frames: 2,
            fps: 2.0,
            root_motion: false,
            tracks: vec![joint as i32],
            data: vec![
                0., 0., 0., 1., -0.10, 1.10, 0.05,
                0., 0., 0., 1., -0.34, 1.50, 0.65,
            ],
        };
        let pose = BladePose::from_throw_clip(rig, joint, Some(clip), 0.25);
        // Halfway between frames: (-0.22, 1.30, 0.35) -> strictly from clip interpolation, not bind.
        let at_zero = pose.throw_hand(Root { position: [5.0, 1.0, -3.0], yaw: 0.0 });
        assert!((at_zero[0] - (5.0 - 0.22)).abs() < 1e-6);
        assert!((at_zero[1] - (1.0 + 1.30)).abs() < 1e-6);
        assert!((at_zero[2] - (-3.0 + 0.35)).abs() < 1e-6);
        let at_quarter = pose.throw_hand(Root {
            position: [5.0, 1.0, -3.0],
            yaw: std::f64::consts::FRAC_PI_2,
        });
        // +90 deg around Y maps (x, y, z) -> (z, y, -x).
        assert!((at_quarter[0] - (5.0 + 0.35)).abs() < 1e-6);
        assert!((at_quarter[1] - (1.0 + 1.30)).abs() < 1e-6);
        assert!((at_quarter[2] - (-3.0 + 0.22)).abs() < 1e-6);
    }
}
