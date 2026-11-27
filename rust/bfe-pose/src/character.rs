//! Bind each soldier's declared animation packages to its mesh and animation joints.

/// Includes animation-only joints as well as the skinned mesh.
pub const MAX_JOINTS: usize = 64;
pub fn check_joints(count: usize) -> Result<(), String> {
    if count > MAX_JOINTS {
        Err(format!(
            "character skeleton has {count} joints, maximum {MAX_JOINTS}"
        ))
    } else {
        Ok(())
    }
}

pub mod stance;

use bfe_content::{actor::Actor, bin_arrays, zero_hash, Value};
use std::{collections::HashMap, rc::Rc};
use three::{Matrix4, Quaternion, Vector3};

pub fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
pub fn array<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).unwrap_or(&[])
}
fn crc(s: &str) -> Result<u32, String> {
    u32::from_str_radix(s, 16).map_err(|_| format!("invalid animation CRC {s}"))
}
fn index(v: &Value, k: &str) -> Result<usize, String> {
    v.get(k)
        .and_then(Value::as_f64)
        .filter(|x| x.is_finite() && *x >= 0. && x.fract() == 0. && *x < usize::MAX as f64)
        .map(|x| x as usize)
        .ok_or_else(|| format!("invalid animation {k}"))
}
fn components<const N: usize>(v: &Value, k: &str) -> Result<[f64; N], String> {
    let values = array(v, k);
    let mut out = [0.; N];
    if values.len() != N {
        return Err(format!("invalid joint {k} extent"));
    }
    for (dst, src) in out.iter_mut().zip(values) {
        *dst = src
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("invalid joint {k}"))?;
    }
    Ok(out)
}

pub struct Archive<'a> {
    pub meta: &'a Value,
    samples: Rc<[f32]>,
    fps: f64,
}
impl<'a> Archive<'a> {
    pub fn new(meta: &'a Value, bytes: &[u8]) -> Result<Self, String> {
        let fps = meta
            .get("fps")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or("invalid animation rate")?;
        Ok(Self {
            meta,
            samples: bin_arrays::f32s(bytes).map_err(|e| e.to_string())?.into(),
            fps,
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Local {
    pub position: Vector3,
    pub rotation: Quaternion,
}
#[derive(Debug)]
pub struct Joint {
    pub crc: u32,
    pub parent: Option<usize>,
    pub bind: Local,
    pub scale: Vector3,
    pub inverse: Matrix4,
    bind_length: bool,
    scale_translation: bool,
}
pub struct Rig {
    pub joints: Vec<Joint>,
    pub order: Vec<usize>,
    pub mesh_joints: usize,
    pub root_scale: f64,
    pub root: Option<usize>,
    pub aim: Option<usize>,
    pub spine: [Option<usize>; 3],
    pub head_chain: [Option<usize>; 2],
    pub torso: Option<usize>,
    pub thighs: [Option<usize>; 2],
    pub upper: Vec<bool>,
    by_crc: HashMap<u32, usize>,
}
impl Rig {
    pub fn joint(&self, name: &str) -> Option<usize> {
        self.by_crc.get(&zero_hash::crc(name)).copied()
    }
    /// Share of torso aim pitch carried by `joint` (lower spine, upper spine, ribcage).
    pub fn aim_share(&self, joint: usize) -> f64 {
        match self.spine {
            [Some(a), Some(b), Some(c)] => {
                if joint == a {
                    0.125
                } else if joint == b {
                    0.375
                } else if joint == c {
                    0.5
                } else {
                    0.
                }
            }
            [None, Some(b), Some(c)] => {
                if joint == b || joint == c {
                    0.5
                } else {
                    0.
                }
            }
            [None, None, Some(c)] if joint == c => 1.,
            _ => 0.,
        }
    }
    /// Share of head aim pitch carried by `joint` (neck and head).
    pub fn head_aim_share(&self, joint: usize) -> f64 {
        match self.head_chain {
            [Some(neck), Some(head)] => {
                if joint == neck {
                    0.25
                } else if joint == head {
                    0.375
                } else {
                    0.
                }
            }
            [None, Some(head)] if joint == head => 0.625,
            [Some(neck), None] if joint == neck => 0.625,
            _ => 0.,
        }
    }
    fn bank_stride_scale(&self, skeleton: &Value) -> f64 {
        if self.root_scale == 1. {
            return 1.;
        }
        let Some(root) = self.joints.iter().find(|j| j.scale_translation) else {
            return self.root_scale;
        };
        let p = root.bind.position;
        // The bank and mesh store the body's local bind offset as floats.
        // A matching nonzero height puts the authored stride in mesh units.
        let offset = [p.x, p.y, p.z].map(|v| v as f32);
        if offset[1] > 0.
            && offset.iter().all(|v| v.is_finite())
            && array(skeleton, "joints").iter().any(|joint| {
                u32::from_str_radix(field(joint, "boneCrc"), 16) == Ok(root.crc)
                    && components::<3>(joint, "position")
                        .is_ok_and(|pos| pos.map(|v| v as f32) == offset)
            })
        {
            1.
        } else {
            self.root_scale
        }
    }
    fn from_mesh(actor: &Actor, scale: f64) -> Self {
        let map = |p: bfe_content::three::Vector3| Vector3::new(p.x, p.y, p.z);
        let joints: Vec<_> = actor
            .bones
            .iter()
            .map(|b| Joint {
                crc: zero_hash::crc(&b.name),
                parent: (b.parent >= 0.).then_some(b.parent as usize),
                bind: Local {
                    position: map(b.position),
                    rotation: Quaternion::new(
                        b.quaternion.x,
                        b.quaternion.y,
                        b.quaternion.z,
                        b.quaternion.w,
                    ),
                },
                scale: map(b.scale),
                inverse: Matrix4 {
                    elements: b.inverse.elements,
                },
                scale_translation: b.name.eq_ignore_ascii_case("bone_root"),
                bind_length: bfe_content::actor::BIND_LENGTH
                    .iter()
                    .any(|n| b.name.eq_ignore_ascii_case(n)),
            })
            .collect();
        let by_crc = joints.iter().enumerate().map(|(i, j)| (j.crc, i)).collect();
        Self {
            mesh_joints: joints.len(),
            joints,
            by_crc,
            order: Vec::new(),
            root_scale: scale,
            root: None,
            aim: None,
            spine: [None; 3],
            head_chain: [None; 2],
            torso: None,
            thighs: [None; 2],
            upper: Vec::new(),
        }
    }
    fn extend(&mut self, records: &[&Value]) -> Result<(), String> {
        let mut extra = Vec::new();
        for rig in records {
            let joints = array(rig, "joints");
            check_joints(joints.len())?;
            for desc in joints {
                let id = crc(field(desc, "boneCrc"))?;
                if self.by_crc.contains_key(&id) {
                    continue;
                }
                check_joints(self.joints.len() + 1)?;
                let [x, y, z] = components(desc, "position")?;
                let [qx, qy, qz, qw] = components(desc, "rotation")?;
                let parent = crc(field(desc, "parentCrc"))?;
                self.by_crc.insert(id, self.joints.len());
                extra.push(parent);
                self.joints.push(Joint {
                    crc: id,
                    parent: None,
                    bind: Local {
                        position: Vector3::new(x, y, z),
                        rotation: Quaternion::new(qx, qy, qz, qw),
                    },
                    scale: Vector3::new(1., 1., 1.),
                    inverse: Matrix4::default(),
                    bind_length: false,
                    scale_translation: false,
                });
            }
        }
        for (joint, parent) in self.joints[self.mesh_joints..].iter_mut().zip(extra) {
            if parent != 0 && parent != joint.crc {
                joint.parent = Some(
                    *self
                        .by_crc
                        .get(&parent)
                        .ok_or_else(|| format!("missing animation parent {parent:08x}"))?,
                );
            }
        }
        // Store one link per child. Visiting each edge once also handles a
        // skeleton whose parents are all listed after their children.
        let mut children = vec![Vec::new(); self.joints.len()];
        for (i, joint) in self.joints.iter().enumerate() {
            if let Some(parent) = joint.parent {
                children
                    .get_mut(parent)
                    .ok_or("absent parent in character skeleton")?
                    .push(i);
            } else {
                self.order.push(i);
            }
        }
        let mut next = 0;
        while next < self.order.len() {
            self.order.extend_from_slice(&children[self.order[next]]);
            next += 1;
        }
        if self.order.len() != self.joints.len() {
            return Err("cycle in character skeleton".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub enum Mask<'a> {
    All,
    Moving,
    Upper(&'a [bool]),
    Over(&'a Clip),
}
pub struct Clip {
    pub crc: u32,
    pub bank: usize,
    pub frames: usize,
    pub fps: f64,
    pub root_motion: bool,
    pub speed: f64,
    /// Factor from the gait bank's authored stride to the drawn mesh.
    pub stride_scale: f64,
    /// Horizontal displacement of the authored root over one cycle.
    pub travel: [f64; 2],
    pub tracks: Vec<usize>,
    channels: Vec<Channel>,
    samples: Rc<[f32]>,
    offset: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Channel {
    Absent,
    Constant,
    Moving,
}

impl Clip {
    pub fn duration(&self) -> f64 {
        self.frames.saturating_sub(1) as f64 / self.fps
    }
    fn read(
        archive: &Archive,
        rig: &Rig,
        desc: &Value,
        bank: usize,
        stride_scale: f64,
    ) -> Result<Self, String> {
        let id = crc(field(desc, "crc"))?;
        let frames = index(desc, "frames")?;
        let offset = index(desc, "offset")?;
        let mut tracks = Vec::new();
        for t in array(desc, "tracks") {
            let h = crc(t.as_str().ok_or("invalid animation joint")?)?;
            tracks.push(
                *rig.by_crc
                    .get(&h)
                    .ok_or_else(|| format!("clip {id:08x} has unbound joint {h:08x}"))?,
            );
        }
        let extent = tracks
            .len()
            .checked_mul(frames)
            .and_then(|n| n.checked_mul(7))
            .and_then(|n| n.checked_add(offset));
        if frames == 0 || extent.is_none_or(|end| end > archive.samples.len()) {
            return Err(format!("invalid sample extent for {id:08x}"));
        }
        let root_motion = index(desc, "flags")? & 1 != 0;
        let mut clip = Self {
            crc: id,
            bank,
            frames,
            fps: archive.fps,
            root_motion,
            speed: 0.,
            stride_scale,
            travel: [0.; 2],
            tracks,
            channels: vec![Channel::Absent; rig.joints.len()],
            samples: archive.samples.clone(),
            offset,
        };
        for (track, &joint) in clip.tracks.iter().enumerate() {
            let start = offset + track * frames * 7;
            let keys = &clip.samples[start..start + frames * 7];
            let first = &keys[..7];
            let moves = keys.as_chunks::<7>().0.iter().skip(1).any(|k| {
                // q and -q describe the same orientation.
                let sign = if first[..4].iter().zip(k).map(|(a, b)| a * b).sum::<f32>() < 0. {
                    -1.
                } else {
                    1.
                };
                (0..4).any(|i| (first[i] - sign * k[i]).abs() > 1e-5)
                    || (4..7).any(|i| (first[i] - k[i]).abs() > 1e-5)
            });
            clip.channels[joint] = if moves {
                Channel::Moving
            } else {
                Channel::Constant
            };
        }
        if let Some(root) = rig
            .joint("DummyRoot")
            .and_then(|r| clip.tracks.iter().position(|&i| i == r))
            .filter(|_| root_motion && frames > 1)
        {
            let first = clip.key(root, 0).position;
            let last = clip.key(root, frames - 1).position;
            clip.travel = [last.x - first.x, last.z - first.z];
            clip.speed = v8math::hypot(clip.travel[0], clip.travel[1]) / clip.duration();
        }
        Ok(clip)
    }
    fn key(&self, track: usize, frame: usize) -> Local {
        let i = self.offset + 7 * (track * self.frames + frame);
        let d = &self.samples[i..i + 7];
        Local {
            position: Vector3::new(d[4] as f64, d[5] as f64, d[6] as f64),
            rotation: Quaternion::new(d[0] as f64, d[1] as f64, d[2] as f64, d[3] as f64),
        }
    }
    fn owns(&self, joint: usize, mask: Mask) -> bool {
        match mask {
            Mask::All => true,
            Mask::Upper(joints) => joints[joint],
            Mask::Moving => self.channels[joint] == Channel::Moving,
            // A constant channel may hold an upper joint in position. Only
            // defer it when the accompanying lower clip actually moves it.
            Mask::Over(lower) => {
                self.channels[joint] == Channel::Moving || lower.channels[joint] != Channel::Moving
            }
        }
    }
    /// A named gait may supply only the arms. Check the bound leg channels.
    pub fn moves_legs(&self, rig: &Rig) -> bool {
        rig.thighs == [None; 2]
            || self.tracks.iter().any(|&joint| {
                self.channels[joint] == Channel::Moving
                    && std::iter::successors(Some(joint), |&i| rig.joints[i].parent)
                        .any(|i| rig.thighs.contains(&Some(i)))
            })
    }
    pub fn applied_tracks(&self, mask: Mask) -> usize {
        self.tracks.iter().filter(|&&j| self.owns(j, mask)).count()
    }
    pub fn apply(&self, rig: &Rig, time: f64, repeat: bool, mask: Mask, local: &mut [Local]) {
        let span = (self.frames - 1) as f64;
        let frame = if repeat && span > 0. {
            (time * self.fps).rem_euclid(span)
        } else {
            (time * self.fps).clamp(0., span)
        };
        let start = frame.floor() as usize;
        let end = (start + 1).min(self.frames - 1);
        for (track, &i) in self.tracks.iter().enumerate() {
            let j = &rig.joints[i];
            if !self.owns(i, mask) {
                continue;
            }
            let mut p = self.key(track, start);
            let b = self.key(track, end);
            p.rotation.slerp(&b.rotation, frame.fract());
            p.rotation.normalize();
            p.position.lerp(&b.position, frame.fract());
            if j.bind_length {
                p.position = j.bind.position;
            }
            if j.scale_translation {
                p.position.multiply_scalar(rig.root_scale);
            }
            if self.root_motion && j.parent.is_none() {
                p.position.x = j.bind.position.x;
                p.position.z = j.bind.position.z;
            }
            local[i] = p;
        }
    }
}
pub struct Character {
    pub rig: Rig,
    pub clips: Vec<Clip>,
    pub banks: Vec<String>,
    by_clip: HashMap<(usize, u32), usize>,
}
impl Character {
    pub fn named_clip(&self, name: &str) -> Option<usize> {
        let id = zero_hash::crc(name);
        (0..self.banks.len()).find_map(|bank| self.by_clip.get(&(bank, id)).copied())
    }
    pub fn clip(&self, bank: &str, id: u32) -> Option<usize> {
        let b = self
            .banks
            .iter()
            .position(|name| name.eq_ignore_ascii_case(bank))?;
        self.by_clip.get(&(b, id)).copied()
    }
    /// Resolve one action from the equipped weapon's bank-qualified binding.
    pub fn weapon_clip(&self, binding: &Value, action: &str) -> Result<usize, String> {
        let timing = binding
            .get("clipTiming")
            .and_then(|v| v.get(action))
            .ok_or_else(|| format!("missing weapon clip timing for {action}"))?;
        let bank = field(timing, "bank");
        let id = crc(field(timing, "crc"))?;
        let index = self
            .clip(bank, id)
            .ok_or_else(|| format!("missing weapon clip {action} in bank {bank}"))?;
        let clip = &self.clips[index];
        if timing.get("frames").and_then(Value::as_f64) != Some(clip.frames as f64)
            || timing.get("fps").and_then(Value::as_f64) != Some(clip.fps)
        {
            return Err(format!(
                "weapon clip timing differs from {bank}/{id:08x} for {action}"
            ));
        }
        Ok(index)
    }
    pub fn bind(
        name: &str,
        def: &Value,
        banks: &[String],
        actor: &Actor,
        side: &Archive,
        common: &Archive,
    ) -> Result<Self, String> {
        let package = side
            .meta
            .get("packages")
            .and_then(|p| p.get(&zero_hash::fnv1a_hex(name)))
            .unwrap_or(&Value::Null);
        let prop = |name: &str| {
            def.get(name).and_then(Value::as_str).or_else(|| {
                array(def, "properties")
                    .iter()
                    .filter_map(Value::as_array)
                    .find_map(|r| {
                        (r.first()?.as_str()?.eq_ignore_ascii_case(name))
                            .then(|| r.get(1)?.as_str())
                            .flatten()
                    })
            })
        };
        let root_scale = prop("SkeletonRootScale")
            .and_then(|v| v.parse().ok())
            .or_else(|| def.get("SkeletonRootScale").and_then(Value::as_f64))
            .unwrap_or(1.);
        check_joints(actor.bones.len())?;
        let mut rig = Rig::from_mesh(actor, root_scale);
        let mut rigs: Vec<_> = array(package, "skeletons")
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|name| side.meta.get("skeletons")?.get(name))
            .collect();
        let mut bank_sources = Vec::with_capacity(banks.len());
        // The content loader lists banks in overlay order. A pose chooses the
        // highest-priority matching layer first, including the unit's own sockets.
        for name in banks.iter().rev() {
            let (archive, key) = [side, common]
                .into_iter()
                .find_map(|a| {
                    let bank = a
                        .meta
                        .get("banks")?
                        .as_object()?
                        .keys()
                        .find(|key| key.eq_ignore_ascii_case(name))?;
                    Some((a, bank))
                })
                .ok_or_else(|| format!("absent animation bank {name}"))?;
            let skeleton = archive.meta.get("skeletons").and_then(|s| s.get(key));
            rigs.extend(skeleton);
            bank_sources.push((archive, key.to_owned(), skeleton));
        }
        rig.extend(&rigs)?;
        // Aim around the torso shared by the head and the weapon attachment.
        // A character without both branches simply uses its authored pose.
        if let (Some(socket), Some(head)) = (
            actor.attach.as_deref().and_then(|n| rig.joint(n)),
            rig.joint("bone_head"),
        ) {
            let chain = |i: usize| std::iter::successors(Some(i), |&j| rig.joints[j].parent);
            rig.aim = chain(head)
                .find(|&i| rig.joints[i].parent.is_some() && chain(socket).any(|j| j == i));
        }
        if rig.aim.is_some() {
            let spine_a = rig.joint("bone_a_spine");
            let spine_b = rig.joint("bone_b_spine");
            let ribcage = rig.joint("bone_ribcage");
            rig.spine = if spine_a.is_some() && spine_b.is_some() && ribcage.is_some() {
                [spine_a, spine_b, ribcage]
            } else if let (Some(mid), Some(rib)) =
                (rig.joint("bone_abdomen").or_else(|| rig.joint("bone_spine")), ribcage)
            {
                [None, Some(mid), Some(rib)]
            } else {
                [None, None, rig.aim]
            };
        }
        rig.root = rig.joint("bone_root");
        rig.head_chain = [rig.joint("bone_neck"), rig.joint("bone_head")];
        rig.thighs = ["bone_l_thigh", "bone_r_thigh"].map(|n| rig.joint(n));
        rig.torso = rig
            .joint("bone_a_spine")
            .or_else(|| {
                let head = rig.joint("bone_head")?;
                let thigh = rig.thighs[0]?;
                let chain = |i| std::iter::successors(Some(i), |&j: &usize| rig.joints[j].parent);
                chain(head).find(|&i| {
                    rig.joints[i]
                        .parent
                        .is_some_and(|p| chain(thigh).any(|j| j == p))
                })
            })
            .or(rig.aim);
        rig.upper = vec![false; rig.joints.len()];
        for &i in &rig.order {
            rig.upper[i] =
                Some(i) == rig.torso || rig.joints[i].parent.is_some_and(|p| rig.upper[p]);
        }
        let mut result = Self {
            rig,
            clips: Vec::new(),
            banks: Vec::new(),
            by_clip: HashMap::new(),
        };
        for (archive, name, skeleton) in bank_sources {
            let bank = result.banks.len();
            let stride_scale = skeleton.map_or(root_scale, |s| result.rig.bank_stride_scale(s));
            let record = archive
                .meta
                .get("banks")
                .and_then(|b| b.get(&name))
                .ok_or_else(|| format!("absent animation bank {name}"))?;
            for d in array(record, "clips") {
                let c = Clip::read(archive, &result.rig, d, bank, stride_scale)
                    .map_err(|e| format!("{name}: {e}"))?;
                result
                    .by_clip
                    .entry((bank, c.crc))
                    .or_insert(result.clips.len());
                result.clips.push(c);
            }
            result.banks.push(name);
        }
        Ok(result)
    }
}

impl Clip {
    /// Bind the authored tracks to the fixed-world pose accumulator. Keep its
    /// sample format and per-joint ownership independent of a display clock.
    pub fn simulation_clip(&self) -> crate::Clip {
        let count = self.frames * self.tracks.len() * 7;
        crate::Clip {
            frames: self.frames,
            fps: self.fps,
            root_motion: self.root_motion,
            tracks: self.tracks.iter().map(|&i| i as i32).collect(),
            data: self.samples[self.offset..self.offset + count].to_vec(),
        }
    }

    pub fn owns_joint(&self, joint: usize, mask: Mask) -> bool {
        self.owns(joint, mask)
    }
}

impl Joint {
    /// Position from a sampled track after applying the character's bind rules.
    pub fn animated_position(&self, mut position: Vector3, root_scale: f64) -> Vector3 {
        if self.bind_length {
            return self.bind.position;
        }
        if self.scale_translation {
            position.multiply_scalar(root_scale);
        }
        position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_travel_uses_the_simulation_length_on_a_two_frame_clip() {
        let root = zero_hash::crc("DummyRoot");
        let rig = Rig {
            joints: vec![Joint {
                crc: root,
                parent: None,
                bind: Local::default(),
                scale: Vector3::new(1., 1., 1.),
                inverse: Matrix4::default(),
                bind_length: false,
                scale_translation: false,
            }],
            order: vec![0],
            mesh_joints: 1,
            root_scale: 1.,
            root: None,
            aim: None,
            spine: [None; 3],
            head_chain: [None; 2],
            torso: None,
            thighs: [None; 2],
            upper: vec![false],
            by_crc: HashMap::from([(root, 0)]),
        };
        let meta = bfe_content::json::parse(r#"{"fps":1}"#).unwrap();
        let mut bytes = Vec::new();
        for value in [0_f32, 0., 0., 1., 0., 0., 0., 0., 0., 0., 1., 1., 9., 5.] {
            bytes.extend(value.to_le_bytes());
        }
        let archive = Archive::new(&meta, &bytes).unwrap();
        let desc = bfe_content::json::parse(&format!(
            r#"{{"crc":"01","frames":2,"flags":1,"offset":0,"tracks":["{root:08x}"]}}"#
        ))
        .unwrap();
        let clip = Clip::read(&archive, &rig, &desc, 0, 1.).unwrap();
        // Horizontal travel of (1,5) for one second, after V8's scale and square root.
        assert_eq!(clip.speed.to_bits(), 0x4014_6565_5f12_2ff7);
    }
}
