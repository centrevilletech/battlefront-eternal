//! First-person pose and clip clocks from src/firstperson.js. The tree is
//! camera-relative: the host supplies its current camera in a second batch,
//! after the body's displayed pose has served the death and vehicle cameras.

use crate::display::{DisplayClip, SoldierState, NONE, PRONE};
use crate::rig::skeleton::Skeleton;
use crate::{js_max as max, js_min as min, Pose};
use three::{Matrix4, NodeId, Quaternion, TransformNode, TransformTree, Vector3};

pub const IDLE: usize = 0;
pub const RUN: usize = 1;
pub const SHOOT: usize = 2;
pub const SHOOT2: usize = 3;
pub const RELOAD: usize = 4;
pub const JUMP: usize = 5;
pub const FLAIL: usize = 6;
pub const JUMP_LAND: usize = 7;
pub const HANDS_DOWN: usize = 8;

/// FirstPersonView.setFor, resolved at load, including each set's fallbacks.
pub struct Tables {
    pub sets: Vec<[usize; 9]>,
    pub forced_set: Option<usize>,
}

/// Shown weapon state. Identity is its class handle: prediction and authority
/// may hold different objects for the same class, which do not lower the arms.
#[derive(Clone, Copy, Default)]
pub struct Weapon {
    pub set: usize,
    pub reloading: bool,
    pub hand_empty: bool,
    pub model: usize,
}

pub struct Rig {
    pub tree: TransformTree,
    pub root: NodeId,
    pub bones: Vec<NodeId>,
    root_bone: Option<usize>,
    offset: Matrix4,
}
impl Rig {
    /// FirstPersonView's rig construction keeps normal Euler callbacks, in
    /// contrast to buildActor's bones. Extra rigid meshes join this same tree.
    pub fn new(skeleton: &Skeleton, offset: Matrix4) -> Self {
        let mut tree = TransformTree::new();
        let mut node = TransformNode::default();
        node.matrix_auto_update = false;

        let root = tree.add_node(node);
        let bones: Vec<_> = skeleton
            .bind
            .iter()
            .map(|b| tree.add_node(b.clone()))
            .collect();
        for i in 0..bones.len() {
            let p = skeleton.parents[i];
            tree.add_child(if p < 0 { root } else { bones[p as usize] }, bones[i]);
        }
        let root_bone = skeleton.parents.iter().position(|&p| p < 0);
        Self {
            tree,
            root,
            bones,
            root_bone,
            offset,
        }
    }
    /// FirstPersonView's camera placement, after the body has supplied any
    /// death/rider camera anchors. The separate record phase is also used
    /// on presented frames that do not draw, before effects read the muzzle.
    pub fn place_camera(&mut self, camera: &Matrix4) {
        self.tree
            .node_mut(self.root)
            .matrix
            .multiply_matrices(camera, &self.offset);
        self.tree.update_matrix_world(self.root, true);
    }
}

/// FirstPersonView's persistent pose. Sun occlusion and material publication
/// consume its hand output on the host; neither advances the animation clock.
pub struct FirstPerson {
    pub slot: usize,
    pub clip: usize,
    pub time: f64,
    pub blend: f64,
    pub holding: usize,
    pub visible: bool,
    pub was_active: bool,
    pub model: usize,
    action: usize,
    forced_slot: usize,
    forced_left: f64,
    was_air: u32,
    air_time: f64,
    impact_speed: f64,
    firing: f64,
    from_p: Vec<Vector3>,
    from_q: Vec<Quaternion>,
    pose: Pose,
}
impl FirstPerson {
    pub fn new(rig: &Rig) -> Self {
        Self {
            slot: NONE,
            clip: NONE,
            time: 0.0,
            blend: 0.0,
            holding: NONE,
            visible: false,
            was_active: false,
            model: NONE,
            action: NONE,
            forced_slot: NONE,
            forced_left: 0.0,
            was_air: 0,
            air_time: 0.0,
            impact_speed: 0.0,
            firing: 0.0,
            from_p: rig
                .bones
                .iter()
                .map(|&i| rig.tree.node(i).position)
                .collect(),
            from_q: rig
                .bones
                .iter()
                .map(|&i| rig.tree.node(i).quaternion)
                .collect(),
            pose: Pose::new(rig.bones.len(), -1),
        }
    }
    // FirstPersonView.setFor. Class identity is a numeric weapon handle,
    // independent of the page's predicted versus authoritative JS object.
    fn set_for(&self, weapon: usize, t: &Tables, weapons: &[Weapon]) -> usize {
        t.forced_set.unwrap_or_else(|| {
            if weapon == NONE {
                0
            } else {
                weapons[weapon].set
            }
        })
    }
    fn clip_for(&self, slot: usize, weapon: usize, t: &Tables, weapons: &[Weapon]) -> usize {
        t.sets[self.set_for(weapon, t, weapons)][slot]
    }
    /// FirstPersonView.start captures a fixed source pose for a transition.
    /// A clip missing from an authored set leaves both slot and pose alone.
    fn start(&mut self, slot: usize, clip: usize, snap: bool, rig: &Rig) {
        if clip == NONE {
            return;
        }
        if clip != self.clip || snap {
            for i in 0..rig.bones.len() {
                let b = rig.tree.node(rig.bones[i]);
                self.from_p[i] = b.position;
                self.from_q[i] = b.quaternion;
            }
            self.clip = clip;
            self.time = 0.0;
            self.blend = if snap { 0.0 } else { 1.0 };
        }
        self.slot = slot;
    }
    /// FirstPersonView.play. event: shoot=0, throw=1, prepareThrow=2.
    pub fn play(
        &mut self,
        event: u32,
        weapon: usize,
        rig: &Rig,
        t: &Tables,
        clips: &[DisplayClip],
        weapons: &[Weapon],
    ) {
        if !self.was_active || event > 2 {
            return;
        }
        self.holding = weapon;
        if event == 2 {
            self.action = SHOOT;
            self.start(SHOOT, self.clip_for(SHOOT, weapon, t, weapons), false, rig);
            return;
        }
        if t.forced_set.is_some()
            && self.action == SHOOT
            && self.clip != NONE
            && self.time < clips[self.clip].duration
        {
            self.firing = 0.25;
            return;
        }
        self.action = if event == 1 { SHOOT2 } else { SHOOT };
        self.firing = 0.25;
        self.start(
            self.action,
            self.clip_for(self.action, weapon, t, weapons),
            true,
            rig,
        );
    }
    /// FirstPersonView.cue lowers the arms through jump_land during a camera
    /// motion. It crossfades instead of snapping to the clip's lowered hands.
    pub fn cue(&mut self, seconds: f64, rig: &Rig, t: &Tables, weapons: &[Weapon]) {
        if !self.was_active {
            return;
        }
        self.action = NONE;
        self.start(
            JUMP_LAND,
            self.clip_for(JUMP_LAND, self.holding, t, weapons),
            false,
            rig,
        );
        self.forced_slot = JUMP_LAND;
        self.forced_left = seconds;
    }
    /// FirstPersonView.apply, using the same f32 weighted pose as the body
    /// and world rig sampler, with a fixed outgoing pose for the crossfade.
    /// FirstPersonView.apply blends from the captured outgoing pose, rather
    /// than recursively blending last frame. Zero-time draws stay identical.
    fn apply(&mut self, time: f64, looping: bool, w: f64, rig: &mut Rig, clips: &[DisplayClip]) {
        self.pose.clear();
        self.pose.add(&clips[self.clip].sample, time, 1.0, looping);
        for i in 0..rig.bones.len() {
            let weight = self.pose.w[i] as f64;
            if weight <= 1e-6 {
                continue;
            }
            let at = i * 4;
            let q = &self.pose.q;
            let mut target = Quaternion::new(
                q[at] as f64,
                q[at + 1] as f64,
                q[at + 2] as f64,
                q[at + 3] as f64,
            );
            target.normalize();
            let at = i * 3;
            let p = &self.pose.t;
            let target_p = Vector3::new(
                p[at] as f64 / weight,
                p[at + 1] as f64 / weight,
                p[at + 2] as f64 / weight,
            );
            let bone = rig.tree.node_mut(rig.bones[i]);
            bone.slerp_quaternions(&self.from_q[i], &target, w);
            bone.position.lerp_vectors(&self.from_p[i], &target_p, w);
        }
    }
    /// FirstPersonView.update before camera placement, light rays, and flash
    /// material publication. It advances each clip exactly once per frame.
    pub fn update(
        &mut self,
        dt: f64,
        active: bool,
        zoomed: bool,
        s: &SoldierState,
        shown: usize,
        rig: &mut Rig,
        t: &Tables,
        clips: &[DisplayClip],
        weapons: &[Weapon],
    ) {
        self.visible = active && !zoomed;
        if !active {
            self.was_active = false;
            return;
        }
        let v2 = s.velocity.x * s.velocity.x + s.velocity.z * s.velocity.z;
        let air = s.air;
        if air != 0 {
            self.air_time += dt;
            self.impact_speed = max(self.impact_speed, -s.velocity.y);
        }
        if !self.was_active {
            self.was_active = true;
            self.holding = shown;
            self.action = NONE;
            self.was_air = air;
            self.start(
                HANDS_DOWN,
                self.clip_for(HANDS_DOWN, self.holding, t, weapons),
                true,
                rig,
            );
            if self.clip != NONE {
                self.apply(0.0, true, 1.0, rig, clips);
            }
            self.forced_slot = IDLE;
            self.forced_left = 1.0;
        }
        if self.was_air != 0
            && air == 0
            && (self.was_air == 1 || self.impact_speed >= 3.0 || self.air_time > 0.35)
        {
            self.forced_slot = if self.was_air == 1 {
                JUMP_LAND
            } else {
                HANDS_DOWN
            };
            self.forced_left = if self.was_air == 1 { 0.5 } else { 1.0 };
        }
        if air == 0 {
            self.air_time = 0.0;
            self.impact_speed = 0.0;
        }
        if air != 4 {
            self.was_air = air;
        }
        if self.forced_slot != NONE {
            self.forced_left -= dt;
            if self.forced_left <= 0.0 {
                self.forced_slot = NONE;
            }
        }
        let slot;
        if shown != self.holding {
            slot = HANDS_DOWN;
            if self.slot == HANDS_DOWN && self.blend <= 0.0 {
                self.holding = shown;
                self.action = NONE;
            }
        } else if self.action != NONE && self.clip != NONE && self.time < clips[self.clip].duration
        {
            slot = self.action;
        } else {
            self.action = NONE;
            slot = if self.holding != NONE && weapons[self.holding].reloading {
                RELOAD
            } else if self.forced_slot != NONE {
                self.forced_slot
            } else if air == 4 && self.slot != NONE {
                self.slot
            } else if air == 1 || s.dive != 0.0 {
                JUMP
            } else if air != 0 {
                FLAIL
            } else if s.posture == PRONE && v2 >= 0.1 {
                HANDS_DOWN
            } else if v2 >= 0.1 {
                RUN
            } else {
                IDLE
            };
        }
        let clip = self.clip_for(slot, self.holding, t, weapons);
        if slot != self.slot || self.clip != clip {
            self.start(slot, clip, false, rig);
        }
        self.model = if self.holding != NONE && !weapons[self.holding].hand_empty {
            weapons[self.holding].model
        } else {
            NONE
        };
        self.firing = max(self.firing - dt, 0.0);
        if t.forced_set.is_some()
            && slot == SHOOT
            && self.firing > 0.0
            && self.clip != NONE
            && self.time >= clips[self.clip].duration
        {
            self.time = 0.0;
        }
        let rate = if slot == RUN {
            if t.forced_set.is_some() {
                1.0
            } else {
                0.75 * min(v2 / 20.0, 1.0)
            }
        } else if t.forced_set.is_some() && slot == SHOOT {
            2.0
        } else {
            1.0
        };
        self.time += dt * rate;
        self.blend = max(self.blend - 3.0 * dt, 0.0);
        let progress = 1.0 - self.blend;
        if self.clip != NONE {
            self.apply(
                self.time,
                matches!(self.slot, IDLE | RUN | FLAIL | HANDS_DOWN) && air != 4,
                progress * progress * (3.0 - 2.0 * progress),
                rig,
                clips,
            );
        }
        if let Some(i) = rig.root_bone {
            rig.tree.node_mut(rig.bones[i]).position.set(0.0, 0.0, 0.0);
        }
    }
}
