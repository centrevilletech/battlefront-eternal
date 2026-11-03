//! A hull's rig as src/vehicles.js builds it (the Vehicle constructor and
//! attachVehicleBatches in src/vehicle-rig.js): the root, the model's bones
//! with their bind pose, the static bone the batches hang their rigid parts
//! from, and the markers the constructor hangs on bones for fire points;
//! then, per seat, the turret node, its pivot and up at rest and the rider's
//! node, and per weapon its aimers' bones and its muzzles (the nodes rounds
//! leave from, src/muzzles.js descendantMuzzles), and whether the walk pose
//! aims its guns (`aimedByPose`).
//!
//! The drawn meshes the batches hang in the tree are left out: nothing the
//! simulation reads depends on them (they have no names, no children, and
//! their own world matrices are only drawn). Posing the rig (placeRoot, the
//! walk pose, the seats' aimers, the barrels' kick) is the hull's; this
//! module gives it the tree to pose and the nodes to read.

use super::skeleton::{lower, BoneDesc, Skeleton};
use three::{Quaternion, Vector3};
use three::{NodeId, TransformNode, TransformTree};

/// A barrel of an aimer (BarrelNodeName, its FirePointName, BarrelLength
/// and BarrelRecoil, as `num()` reads them).
#[derive(Clone, Debug, Default)]
pub struct BarrelDesc {
    pub barrel_node_name: Option<String>,
    pub fire_point_name: Option<String>,
    /// `num(b.BarrelLength, 0)`.
    pub length: f64,
    /// `num(b.BarrelRecoil, 0)` where the class sets it, None where not.
    pub recoil: Option<f64>,
}

/// An aimer of a weapon (its node, FirePointName, HierarchyLevel and
/// barrels).
#[derive(Clone, Debug, Default)]
pub struct AimerDesc {
    /// `a.node`, as the class names it (case kept: the missile racks' test
    /// reads it).
    pub node: Option<String>,
    pub fire_point_name: Option<String>,
    /// `num(a.HierarchyLevel, 0)`.
    pub hierarchy_level: f64,
    pub barrels: Vec<BarrelDesc>,
}

/// A weapon of a seat: its definition's FirePointName and class, and its
/// aimers.
#[derive(Clone, Debug, Default)]
pub struct WeaponDesc {
    pub fire_point_name: Option<String>,
    pub class: String,
    pub aimers: Vec<AimerDesc>,
}

/// A seat's section keys the rig reads.
#[derive(Clone, Debug, Default)]
pub struct SeatDesc {
    pub turret_node_name: Option<String>,
    pub fire_point_name: Option<String>,
    /// `s.Pilot9Pose || s.PilotAnimation`, where truthy.
    pub pilot_animation: Option<String>,
    /// `Boolean(s.Pilot9Pose)`.
    pub nine_pose: bool,
    /// `s.PilotPosition`, where truthy.
    pub pilot_position: Option<String>,
    /// The creature classes' rider node (hash 11ce2025), where truthy.
    pub rider_node_hash: Option<String>,
    /// `num(s['2b60e217'], 1)`: 0 leaves the rider out of collision.
    pub shootable: f64,
    /// A turret seat's pivot without a turret node (hash f2aa4a90, else 0).
    pub pivot: Vector3,
    pub weapons: Vec<WeaponDesc>,
}

/// What the rig is built from: the class's model and seats.
#[derive(Clone, Debug, Default)]
pub struct HullDesc {
    /// `def.model`.
    pub model: String,
    pub flying: bool,
    /// `healthType === 'animal'`.
    pub creature: bool,
    pub bones: Vec<BoneDesc>,
    /// The muzzle tips found on the model's geometry (src/muzzles.js
    /// vehicleMuzzleTips), by lower-cased bone name.
    pub muzzle_tips: Vec<(String, Vector3)>,
    /// The bones the class's clips drive (every track at or above 0), and
    /// whether it has clips at all (a walker's pose).
    pub animated: Vec<usize>,
    pub has_clips: bool,
    pub seats: Vec<SeatDesc>,
}

/// `Object.fromEntries` of lower-cased bone names: each key at its first
/// insertion's place (array-index keys first, ascending), holding the last
/// bone of that name. Its key order is what `Object.entries` and
/// `Object.keys` walk.
#[derive(Clone, Debug, Default)]
pub struct ByName {
    pub entries: Vec<(String, usize)>,
}

fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    if n < 4_294_967_295 {
        Some(n as u32)
    } else {
        None
    }
}

impl ByName {
    pub fn new(names: &[String]) -> ByName {
        let mut entries: Vec<(String, usize)> = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let key = lower(name);
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = i,
                None => entries.push((key, i)),
            }
        }
        // Integer-like keys come first, in ascending order; the rest keep
        // their insertion order.
        let mut indexed: Vec<(String, usize)> = entries.iter().filter(|(k, _)| array_index(k).is_some()).cloned().collect();
        indexed.sort_by_key(|(k, _)| array_index(k).unwrap());
        let rest = entries.into_iter().filter(|(k, _)| array_index(k).is_none());
        ByName { entries: indexed.into_iter().chain(rest).collect() }
    }

    /// `byName[key]`.
    pub fn get(&self, key: &str) -> Option<usize> {
        self.entries.iter().find(|(k, _)| k == key).map(|&(_, i)| i)
    }

    /// `byName[String(name || '').toLowerCase()]`.
    pub fn named(&self, name: Option<&str>) -> Option<usize> {
        self.get(&lower(name.unwrap_or("")))
    }

    /// The key holding a bone (`Object.keys(byName).find(n => byName[n] === bone)`).
    pub fn key_of(&self, bone: usize) -> Option<&str> {
        self.entries.iter().find(|&&(_, i)| i == bone).map(|(k, _)| k.as_str())
    }
}

/// A muzzle: the node a round leaves from, and for an aimer's barrel the
/// barrel node it kicks back and how far, and its rest place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Muzzle {
    pub fire_node: NodeId,
    /// The aimer whose turn it follows (index in its weapon's aimers).
    pub source_aimer: Option<usize>,
    pub barrel: Option<NodeId>,
    pub recoil: Option<f64>,
    pub rest: Option<Vector3>,
}

/// An aimer's rig: its bone, and the seat that turns that bone as its
/// turret (-1: none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimerRig {
    pub bone: Option<NodeId>,
    pub owner: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeaponRig {
    pub aimers: Vec<AimerRig>,
    pub muzzles: Vec<Muzzle>,
    /// The seat's FirePointName node, else the weapon's (its beams' source).
    pub fire: Option<NodeId>,
}

/// A seat's rider, where it has a pilot animation.
#[derive(Clone, Debug, PartialEq)]
pub struct RiderRig {
    /// The pilot animation, lower-cased, and the rider's clip.
    pub anim: String,
    pub clip: String,
    pub nine_pose: bool,
    pub node: Option<NodeId>,
    pub shootable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeatRig {
    pub weapons: Vec<WeaponRig>,
    /// A turret seat's turret node, its pivot and its up at rest.
    pub turret: Option<NodeId>,
    pub pivot: Vector3,
    pub up: Vector3,
    pub rider: Option<RiderRig>,
}

/// A hull's rig.
#[derive(Clone, Debug)]
pub struct HullRig {
    pub tree: TransformTree,
    pub root: NodeId,
    /// The bones' nodes, by bone index.
    pub bones: Vec<NodeId>,
    /// attachVehicleBatches' static bone, on the root.
    pub static_bone: NodeId,
    /// The constructor's markers, in the order it made them.
    pub markers: Vec<NodeId>,
    /// The world's scene, which the root hangs from once built
    /// (`system.scene.add(this.root)`): an identity, so every world matrix
    /// three makes for the root after that is the identity times the root's
    /// own matrix rather than a copy of it, which turns some of its -0s into
    /// +0s. Made last, after the markers.
    pub scene: NodeId,
    pub by_name: ByName,
    /// Each bone's bind quaternion and position.
    pub bind_q: Vec<Quaternion>,
    pub bind_p: Vec<Vector3>,
    /// The walk pose's root bone (DummyRoot, -1 without one) where the hull
    /// has a pose (clips, and not a flyer).
    pub pose_root: Option<i32>,
    pub seats: Vec<SeatRig>,
    /// Whether a seat's turret, an aimer, a fire point or a rider's node
    /// hangs from a bone the clips animate.
    pub aimed_by_pose: bool,
}

/// `/^hp_(fire|gun(?!ner)|cannon|laser|turret)/i` on a lower-cased name.
fn hardpoint(name: &str) -> bool {
    let n = lower(name);
    let Some(rest) = n.strip_prefix("hp_") else { return false };
    if rest.starts_with("fire") || rest.starts_with("cannon") || rest.starts_with("laser") || rest.starts_with("turret") {
        return true;
    }
    rest.starts_with("gun") && !rest[3..].starts_with("ner")
}

/// `/^missile_\d+$/`.
fn missile_rack_name(name: &str) -> bool {
    match name.strip_prefix("missile_") {
        Some(digits) => !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// `/^missile_(left|right)$/i`.
fn missile_side_name(name: &str) -> bool {
    let n = lower(name);
    n == "missile_left" || n == "missile_right"
}

impl HullRig {
    pub fn new(desc: &HullDesc) -> HullRig {
        let skeleton = Skeleton::new(&desc.bones, true);
        let mut tree = TransformTree::new();
        let root = tree.add_node(TransformNode::default());
        let bones: Vec<NodeId> = skeleton.bind.iter().map(|b| tree.add_node(b.clone())).collect();
        for (i, d) in desc.bones.iter().enumerate() {
            let parent = if d.parent >= 0 { bones[d.parent as usize] } else { root };
            tree.add_child(parent, bones[i]);
        }
        let by_name = ByName::new(&skeleton.names);
        let bind_q: Vec<Quaternion> = skeleton.bind.iter().map(|b| b.quaternion).collect();
        let bind_p: Vec<Vector3> = skeleton.bind.iter().map(|b| b.position).collect();
        let pose_root = if desc.has_clips && !desc.flying {
            Some(skeleton.names.iter().position(|n| lower(n) == "dummyroot").map_or(-1, |i| i as i32))
        } else {
            None
        };
        // attachVehicleBatches: a static bone on the root, then the world
        // matrices.
        let static_bone = tree.add_node(TransformNode::default());
        tree.add_child(root, static_bone);
        tree.update_matrix_world(root, true);
        let mut rig = HullRig {
            tree,
            root,
            bones,
            static_bone,
            markers: Vec::new(),
            scene: 0,
            by_name,
            bind_q,
            bind_p,
            pose_root,
            seats: Vec::new(),
            aimed_by_pose: false,
        };
        rig.build_seats(desc);
        // The wheels' pass: every world matrix at rest.
        rig.tree.update_matrix_world(rig.root, true);
        for (index, seat) in desc.seats.iter().enumerate() {
            rig.seat_view(index, seat, desc.creature);
        }
        // A node one seat turns as its turret while another seat's aimer
        // turns it too (the AT-AT's head).
        for s in 0..rig.seats.len() {
            for w in 0..rig.seats[s].weapons.len() {
                for a in 0..rig.seats[s].weapons[w].aimers.len() {
                    let bone = rig.seats[s].weapons[w].aimers[a].bone;
                    let owner = (0..rig.seats.len())
                        .position(|o| o != s && rig.seats[o].turret.is_some() && rig.seats[o].turret == bone)
                        .map_or(-1, |o| o as i32);
                    rig.seats[s].weapons[w].aimers[a].owner = owner;
                }
            }
        }
        rig.aimed_by_pose = rig.aimed_by_pose(desc);
        // The constructor's last word: into the scene.
        rig.scene = rig.tree.add_node(TransformNode::default());
        rig.tree.add_child(rig.scene, rig.root);
        rig
    }

    /// Where a seat's weapon's muzzle is in the world (src/muzzles.js
    /// mountedMuzzle: `seats[seat].weapons[weapon].muzzles[barrel].fireNode
    /// .getWorldPosition()`, the world matrices refreshed from the root down
    /// to it); None where the hull has no such muzzle. The JavaScript also
    /// answers null for a hull that is not alive, which is the caller's to
    /// know.
    pub fn muzzle_point(&mut self, seat: usize, weapon: usize, barrel: usize) -> Option<Vector3> {
        let node = self.seats.get(seat)?.weapons.get(weapon)?.muzzles.get(barrel)?.fire_node;
        let mut out = Vector3::default();
        Some(self.tree.get_world_position(node, &mut out))
    }

    /// The node of a bone named by a class key: `byName[String(name ||
    /// '').toLowerCase()]`.
    pub fn named(&self, name: Option<&str>) -> Option<NodeId> {
        self.by_name.named(name).map(|i| self.bones[i])
    }

    /// The bone index of a node, if it is a bone.
    pub fn bone_index(&self, node: NodeId) -> Option<usize> {
        self.bones.iter().position(|&b| b == node)
    }

    fn marker(&mut self, parent: NodeId, position: Vector3) -> NodeId {
        let node = self.tree.add_node(TransformNode::placed(position, Vector3::new(1.0, 1.0, 1.0)));
        self.tree.add_child(parent, node);
        self.markers.push(node);
        node
    }

    /// `muzzleNode(bone)`: the bone's geometry muzzle (a marker at the tip
    /// its mesh shows, made once a bone), or the bone itself without one.
    fn muzzle_node(&mut self, bone: Option<NodeId>, desc: &HullDesc, geometry: &mut Vec<(NodeId, NodeId)>) -> Option<NodeId> {
        let bone = bone?;
        let name = self.bone_index(bone).and_then(|i| self.by_name.key_of(i)).map(str::to_string);
        let tip = name.and_then(|n| desc.muzzle_tips.iter().find(|(k, _)| *k == n).map(|&(_, t)| t));
        let Some(tip) = tip else { return Some(bone) };
        if let Some(&(_, marker)) = geometry.iter().find(|&&(b, _)| b == bone) {
            return Some(marker);
        }
        let marker = self.marker(bone, tip);
        geometry.push((bone, marker));
        Some(marker)
    }

    /// descendantMuzzles(node, byName, excluded): the fire hardpoints under
    /// a node, depth first, not looking past another aimer's node.
    fn descendant_muzzles(&self, node: Option<NodeId>, excluded: &[Option<NodeId>]) -> Vec<NodeId> {
        let mut found = Vec::new();
        if let Some(node) = node {
            self.visit_muzzles(node, node, excluded, &mut found);
        }
        found
    }

    fn visit_muzzles(&self, bone: NodeId, node: NodeId, excluded: &[Option<NodeId>], found: &mut Vec<NodeId>) {
        if bone != node && excluded.contains(&Some(bone)) {
            return;
        }
        // names.get(bone) || '': a marker, the static bone and a bone another
        // of its name shadows have none.
        let name = self.bone_index(bone).and_then(|i| self.by_name.key_of(i)).unwrap_or("");
        if hardpoint(name) {
            found.push(bone);
            return;
        }
        for &child in &self.tree.node(bone).children {
            self.visit_muzzles(child, node, excluded, found);
        }
    }

    fn build_seats(&mut self, desc: &HullDesc) {
        let all_aimers: Vec<Option<NodeId>> =
            desc.seats.iter().flat_map(|s| s.weapons.iter().flat_map(|w| w.aimers.iter().map(|a| a.node.as_deref()))).map(|n| self.named(n)).collect();
        let mut geometry: Vec<(NodeId, NodeId)> = Vec::new();
        for seat in &desc.seats {
            let mut weapons = Vec::with_capacity(seat.weapons.len());
            for w in &seat.weapons {
                let aimers: Vec<AimerRig> = w.aimers.iter().map(|a| AimerRig { bone: self.named(a.node.as_deref()), owner: -1 }).collect();
                let mut muzzles: Vec<Muzzle> = Vec::new();
                for (ai, a) in w.aimers.iter().enumerate() {
                    let bone = aimers[ai].bone;
                    // A parent aimer only steers when this weapon also
                    // defines child aimers.
                    let parent_of_another = aimers.iter().any(|other| {
                        let mut b = other.bone.and_then(|o| self.tree.node(o).parent);
                        while let Some(id) = b {
                            if Some(id) == bone {
                                return true;
                            }
                            b = self.tree.node(id).parent;
                        }
                        false
                    });
                    if parent_of_another {
                        continue;
                    }
                    // So does one at a HierarchyLevel above 0.
                    if a.hierarchy_level != 0.0 {
                        continue;
                    }
                    if !a.barrels.is_empty() {
                        for b in &a.barrels {
                            let barrel = self.named(b.barrel_node_name.as_deref());
                            let mut node = self.named(b.fire_point_name.as_deref()).or(barrel);
                            let length = b.length;
                            if node.is_some() && length != 0.0 && !length.is_nan() && node == barrel {
                                let marker = self.marker(barrel.unwrap(), Vector3::new(0.0, 0.0, length));
                                node = Some(marker);
                            }
                            let Some(node) = node else { continue };
                            muzzles.push(Muzzle {
                                fire_node: node,
                                source_aimer: Some(ai),
                                barrel,
                                recoil: Some(b.recoil.unwrap_or(0.25 * length)),
                                rest: barrel.map(|id| self.tree.node(id).position),
                            });
                        }
                        continue;
                    }
                    let explicit = self.named(a.fire_point_name.as_deref());
                    let mut descendants = match explicit {
                        Some(e) => vec![e],
                        None => self.descendant_muzzles(bone, &all_aimers),
                    };
                    // Hailfire launch racks carry missile meshes, not hp nodes.
                    if descendants.is_empty() && bone.is_some() {
                        let racks: Vec<NodeId> = self
                            .by_name
                            .entries
                            .iter()
                            .filter(|(name, i)| missile_rack_name(name) && self.tree.node(self.bones[*i]).parent == bone)
                            .map(|&(_, i)| self.bones[i])
                            .collect();
                        for rack in racks {
                            if let Some(n) = self.muzzle_node(Some(rack), desc, &mut geometry) {
                                descendants.push(n);
                            }
                        }
                    }
                    let nodes = if !descendants.is_empty() {
                        descendants
                    } else {
                        self.muzzle_node(bone, desc, &mut geometry).into_iter().collect()
                    };
                    for node in nodes {
                        if !muzzles.iter().any(|m| m.fire_node == node) {
                            muzzles.push(Muzzle { fire_node: node, source_aimer: Some(ai), barrel: None, recoil: None, rest: None });
                        }
                    }
                }
                if muzzles.len() > 2 && w.aimers.iter().any(|a| a.node.as_deref().is_some_and(missile_side_name)) {
                    // Interleave the racks: each rack's first, then each
                    // one's second, ...
                    let mut rack_bones: Vec<Option<NodeId>> = Vec::new();
                    for m in &muzzles {
                        let b = m.source_aimer.and_then(|a| aimers[a].bone);
                        if !rack_bones.contains(&b) {
                            rack_bones.push(b);
                        }
                    }
                    let racks: Vec<Vec<Muzzle>> = rack_bones
                        .iter()
                        .map(|&b| muzzles.iter().filter(|m| m.source_aimer.and_then(|a| aimers[a].bone) == b).copied().collect())
                        .collect();
                    let longest = racks.iter().map(|r| r.len()).max().unwrap_or(0);
                    muzzles = (0..longest).flat_map(|i| racks.iter().filter_map(move |r| r.get(i).copied())).collect();
                }
                let turret = self.named(seat.turret_node_name.as_deref());
                let fire = self.named(seat.fire_point_name.as_deref()).or_else(|| self.named(w.fire_point_name.as_deref()));
                if muzzles.is_empty() {
                    let mut nodes = match fire {
                        Some(f) => vec![f],
                        None => self.descendant_muzzles(turret, &[]),
                    };
                    if nodes.is_empty() && turret.is_some() {
                        if let Some(n) = self.muzzle_node(turret, desc, &mut geometry) {
                            nodes.push(n);
                        }
                    }
                    // The retail Hailfire has no cannon hardpoint: its lower
                    // barrel is skinned to its centre body, whose rim is its
                    // mouth.
                    if nodes.is_empty() && desc.model == "cis_tread_hailfire" && w.class == "cis_weap_walk_spider_cannon" {
                        let center = self.by_name.get("cis_hailfire_droid_center").map(|i| self.bones[i]);
                        if let Some(n) = self.muzzle_node(center, desc, &mut geometry) {
                            nodes.push(n);
                        }
                    }
                    for node in nodes {
                        muzzles.push(Muzzle { fire_node: node, source_aimer: None, barrel: None, recoil: None, rest: None });
                    }
                }
                weapons.push(WeaponRig { aimers, muzzles, fire });
            }
            self.seats.push(SeatRig { weapons, turret: None, pivot: Vector3::default(), up: Vector3::new(0.0, 1.0, 0.0), rider: None });
        }
    }

    /// seatView's rig: a turret seat's node, its pivot and up at rest, and
    /// the rider's node.
    fn seat_view(&mut self, index: usize, seat: &SeatDesc, creature: bool) {
        if index > 0 {
            match self.named(seat.turret_node_name.as_deref()) {
                Some(node) => {
                    let mut pivot = Vector3::default();
                    self.tree.get_world_position(node, &mut pivot);
                    let mut q = Quaternion::default();
                    self.tree.get_world_quaternion(node, &mut q);
                    let mut up = Vector3::new(0.0, 1.0, 0.0);
                    up.apply_quaternion(&q);
                    let s = &mut self.seats[index];
                    s.turret = Some(node);
                    s.pivot = pivot;
                    s.up = up;
                }
                None => self.seats[index].pivot = seat.pivot,
            }
        }
        if let Some(anim) = &seat.pilot_animation {
            let name = match &seat.pilot_position {
                Some(p) => p.clone(),
                None if creature && self.by_name.get("hp_active").is_some() => "hp_active".to_string(),
                None => seat.rider_node_hash.clone().unwrap_or_default(),
            };
            let clip = lower(anim);
            self.seats[index].rider = Some(RiderRig {
                clip: format!("human_{clip}"),
                anim: clip,
                nine_pose: seat.nine_pose,
                node: self.named(Some(&name)),
                shootable: seat.shootable != 0.0,
            });
        }
    }

    /// The `aimedByPose` getter.
    fn aimed_by_pose(&self, desc: &HullDesc) -> bool {
        if self.pose_root.is_none() || !desc.has_clips {
            return false;
        }
        let animated: Vec<NodeId> = desc.animated.iter().map(|&j| self.bones[j]).collect();
        let hangs = |node: Option<NodeId>| {
            let mut n = node;
            while let Some(id) = n {
                if id == self.root {
                    return false;
                }
                if animated.contains(&id) {
                    return true;
                }
                n = self.tree.node(id).parent;
            }
            false
        };
        let mut result = false;
        for (s, seat) in self.seats.iter().enumerate() {
            if seat.turret.is_some() && hangs(seat.turret) || seat.rider.as_ref().is_some_and(|r| r.node.is_some() && hangs(r.node)) {
                result = true;
            }
            for (w, weapon) in seat.weapons.iter().enumerate() {
                let wdesc = &desc.seats[s].weapons[w];
                for (a, aimer) in weapon.aimers.iter().enumerate() {
                    let fire_point = wdesc.aimers[a].fire_point_name.as_deref().filter(|n| !n.is_empty());
                    if aimer.bone.is_some() && hangs(aimer.bone) || fire_point.is_some_and(|n| hangs(self.by_name.get(&lower(n)).map(|i| self.bones[i]))) {
                        result = true;
                    }
                }
                let fire = self.named(wdesc.fire_point_name.as_deref());
                if fire.is_some() && hangs(fire) {
                    result = true;
                }
            }
        }
        result
    }
}
