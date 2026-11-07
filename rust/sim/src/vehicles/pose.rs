//! A hull's pose at the tick (src/vehicles.js aimTick, render, placeRoot,
//! aimSeats, seatEye, seatFrame, seatForward, poseSeat, recoilBarrels,
//! lookBasis, aimVector): its root placed between its last two substeps,
//! each manned seat's aim target (the first thing along its aim within
//! 512 m), its turret node and its aimers turned toward that target at
//! their rates, its barrels' kick, and one pass over the rig for the
//! muzzles, eyes and riders the rest of the tick reads.
//!
//! A walker also advances its walk cycle and is posed from its clips (walk,
//! applyPose, liftLegs), and as drawn feels the ground under its feet
//! (feelLegs). The rig is rust/sim/src/rig/hull.rs's, posed here; a flyer's
//! frame, lean and gear are the flight package's (crate::flight::pose,
//! crate::flight::gear), called from here.

use super::hull::{angle, approach, Hull, DOWN, UP};
use super::VehicleWorld;
use crate::pose::{Clip, Pose};
use crate::rig::hull::HullRig;
use crate::rig::math;
use three::{clamp, lerp, EulerOrder, Matrix4, NodeId, Quaternion, Vector3};
use v8math::hypot;

const RIGHT: Vector3 = Vector3::new(1.0, 0.0, 0.0);
const UNIT_SCALE: Vector3 = Vector3::new(1.0, 1.0, 1.0);
/// m along a seat's aim to the point its guns turn to.
const AIM_REACH: f64 = 512.0;
/// rad: a barrel this close to its target fires at it exactly (`DEG`).
const AIMED: f64 = std::f64::consts::PI / 180.0;

/// An aimer: its node, its turn (yaw, pitch) from its node's rest pose,
/// its rates and limits, where it points and whether it posed its node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aimer {
    pub bone: Option<NodeId>,
    pub yaw: f64,
    pub pitch: f64,
    /// undefined until it first turns.
    pub in_limits: Option<bool>,
    pub on_target: bool,
    pub facing: Vector3,
    pub posed: bool,
    pub owner: i32,
    pub yaw_rate: f64,
    pub pitch_rate: f64,
    pub yaw_limits: [f64; 2],
    pub pitch_limits: [f64; 2],
    pub wrap_yaw: bool,
}

/// A muzzle's barrel and its kick (recoilBarrels).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kick {
    pub barrel: Option<NodeId>,
    pub rest: Option<Vector3>,
    pub recoil: Option<f64>,
    pub kick_at: Option<f64>,
}

/// A seat's weapon as its pose reads it: the barrel it fires next, its
/// aimers and its muzzles' barrels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponPose {
    pub barrel: f64,
    pub aimers: Vec<Aimer>,
    pub muzzles: Vec<Kick>,
}

/// A seat as its pose reads it (seatView and the seat's aim state).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeatPose {
    pub last_yaw: Option<f64>,
    pub yaw_rate: f64,
    pub target: Option<Vector3>,
    pub eye: Vector3,
    pub pivot: Vector3,
    pub up: Vector3,
    pub yaw_factor: f64,
    pub pitch_factor: f64,
    pub turret: Option<NodeId>,
    pub weapons: Vec<WeaponPose>,
    /// The lease's frozen aim (SeatInput aiming, aim).
    pub aiming: bool,
    pub aim: Vector3,
}

/// A clip of the hull's (its frames for the sampler, its length in seconds
/// and its stride).
#[derive(Clone)]
pub struct WalkClip {
    pub clip: std::rc::Rc<Clip>,
    pub duration: f64,
    pub stride: f64,
}

impl std::fmt::Debug for WalkClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WalkClip({} frames, {} s)", self.clip.frames, self.duration)
    }
}

/// The clips a hull's walk pose reads (`this.clips`: none without any).
#[derive(Clone, Debug, Default)]
pub struct WalkClips {
    pub idle: Option<WalkClip>,
    pub left: Option<WalkClip>,
    pub right: Option<WalkClip>,
    pub lift_left: Option<WalkClip>,
    pub lift_right: Option<WalkClip>,
}

/// A walker's leg pairs as its pose reads them (the class's `fx.legs`):
/// each foot's place in the hull's frame, each pair's leg bones by name,
/// the rays' length.
#[derive(Clone, Debug, Default)]
pub struct LegPairs {
    pub ray_length: f64,
    pub at: Vec<[[f64; 3]; 2]>,
    pub names: Vec<[String; 2]>,
}

/// What a hull's pose keeps beside its tick's state.
#[derive(Clone, Debug)]
pub struct HullPose {
    pub work: PoseWork,
    pub rig: HullRig,
    pub visual: Vector3,
    pub age: f64,
    pub hover_phase: f64,
    pub walk_phase: f64,
    pub walk_blend: f64,
    /// Whether it has a walk pose (`this.pose`: clips, not a flyer).
    pub has_pose: bool,
    /// Whether its class has legs (`def.fx.legs`).
    pub legs: bool,
    /// Its wheels' bones and axles, by wheel.
    pub wheel_bones: Vec<(Option<NodeId>, Vector3)>,
    pub seats: Vec<SeatPose>,
    /// Its clips (None: `this.clips` null) and its legs (a walker's).
    pub clips: Option<WalkClips>,
    pub leg_pairs: Option<LegPairs>,
}

/// applyPose, liftLegs and poseGearAt each retain their sampler. Leg bone
/// membership is compiled once; drawn poses never walk names or allocate it.
#[derive(Clone, Debug, Default)]
pub struct PoseWork {
    pub walk: Option<Pose>,
    pub lift: Option<Pose>,
    pub gear: Option<Pose>,
    leg_bones: Option<Vec<Vec<usize>>>,
}

impl HullPose {
    /// Vehicle construction resolves applyPose/liftLegs membership before a
    /// first tick or a respawn can request it. Keep samplers with the rig,
    /// including a flying hull whose takeoff clip starts only on another life.
    pub(crate) fn prepare_work(&mut self, flying: bool) {
        let count = self.rig.bones.len();
        if self.has_pose && self.work.walk.is_none() {
            self.work.walk = Some(Pose::new(count, self.rig.pose_root.unwrap_or(-1)));
        }
        if flying && self.work.gear.is_none() {
            self.work.gear = Some(Pose::new(count, -1));
        }
        let Some(legs) = self.leg_pairs.as_ref() else { return };
        let Some(clips) = self.clips.as_ref() else { return };
        if clips.lift_left.is_none() || clips.lift_right.is_none() { return; }
        if self.work.lift.is_none() {
            self.work.lift = Some(Pose::new(count, self.rig.pose_root.unwrap_or(-1)));
        }
        if self.work.leg_bones.is_none() {
            // liftLegs walks descendants in authored bone order. Compiling
            // that membership here preserves its order without live names.
            self.work.leg_bones = Some(legs.names.iter().map(|pair| {
                let mut under = Vec::new();
                for name in pair {
                    if let Some(node) = self.rig.by_name.get(name).map(|i| self.rig.bones[i]) {
                        self.rig.tree.traverse(node, &mut |bone| under.push(bone));
                    }
                }
                (0..count).filter(|&i| under.contains(&self.rig.bones[i])).collect()
            }).collect());
        }
    }
}

/// `aimVector(yaw, pitch, out)`: a direction from a yaw (positive to the
/// left, +X) and an up-positive pitch.
pub fn aim_vector(yaw: f64, pitch: f64) -> Vector3 {
    let x = math::sin(yaw) * math::cos(pitch);
    let y = math::sin(pitch);
    let z = math::cos(yaw) * math::cos(pitch);
    Vector3::new(x, y, z)
}

/// `lookBasis(forward, up, out)`: the rotation whose +Z is forward, keeping
/// up as near its +Y as it can.
pub fn look_basis(forward: &Vector3, up: &Vector3) -> Quaternion {
    let mut z = *forward;
    z.normalize();
    let mut x = Vector3::default();
    x.cross_vectors(up, &z);
    if x.length_sq() < 1e-10 {
        x.set(0.0, 0.0, 1.0).cross(&z);
    }
    if x.length_sq() < 1e-10 {
        x.set(1.0, 0.0, 0.0);
    }
    x.normalize();
    let mut y = Vector3::default();
    y.cross_vectors(&z, &x);
    let mut m = Matrix4::default();
    m.make_basis(&x, &y, &z);
    let mut q = Quaternion::default();
    q.set_from_rotation_matrix(&m);
    q
}

// Vehicle.seatFrame is shared by solver Hulls and canonical AI sensors.
// A read needs only these scalars and retained seat geometry, never a loaded
// physics working row. Keep compose/multiply order identical for both users.
pub(crate) fn seat_frame_values(position:&Vector3,quaternion:&Quaternion,flying:bool,yaw:f64,aim:Option<(f64,f64,&SeatPose)>)->Matrix4 {
    let mut out=Matrix4::default();let mut q=Quaternion::default();
    if flying {q=*quaternion;}else{q.set_from_axis_angle(&UP,yaw);}
    out.compose(position,&q,&UNIT_SCALE);
    if let Some((aim_yaw,pitch,view))=aim {
        let q2=look_basis(&aim_vector(angle(aim_yaw-yaw),pitch),&view.up);
        let mut m=Matrix4::default();m.compose(&view.pivot,&q2,&UNIT_SCALE);out.multiply(&m);
    }
    out
}

impl Hull {
    fn pose_mut(&mut self) -> &mut HullPose {
        self.pose.as_mut().expect("a posed hull's rig")
    }

    /// Whether `aimTick` and `render` leave the hull as it is.
    pub fn rests(&self, dt: f64) -> bool {
        let p = self.pose.as_ref().unwrap();
        dt > 0.0 && self.sleeping == Some(true) && !self.repulsor && !self.occupied() && !p.has_pose && self.health >= self.max_health * 0.35
    }

    /// `held(i)`: someone holds seat i (a lease, or the player).

    /// vehicles.js aimTick: the tick's part of the pose (a ground hull's).
    pub fn aim_tick<W: VehicleWorld>(&mut self, w: &mut W, alpha: f64, dt: f64) {
        if !self.alive || self.rests(dt) {
            return;
        }
        self.place_root(alpha);
        self.walk(dt);
        if self.pose.as_ref().unwrap().rig.aimed_by_pose {
            self.apply_pose();
        }
        // A craft's gear, doors and foils at its own gear, before its
        // aimers turn (crate::flight::gear).
        if self.flying {
            let gear = self.flight.gear;
            self.pose_gear_at(gear);
        }
        self.pose_mut().age += dt;
        if dt > 0.0 {
            self.aim_seats(w, dt);
        }
        self.pose_seat(self.seat_index, dt);
        for i in 0..self.seats.len() {
            if i != self.seat_index && self.crewed(i) {
                self.pose_seat(i, dt);
            }
        }
        self.recoil_barrels(w);
        let p = self.pose_mut();
        let root = p.rig.root;
        p.rig.tree.update_matrix_world(root, true);
    }

    /// vehicles.js render: the drawn pose. ticked: aimTick has just posed
    /// the hull at this alpha (Vehicles.update; a spawn's render is not),
    /// and then a craft whose flight this world steps (no walk pose, no
    /// legs, its own gear) is drawn as aimTick posed it, only its barrels'
    /// kick read again.
    pub fn render<W: VehicleWorld>(&mut self, w: &mut W, alpha: f64, dt: f64, ticked: bool) {
        if !self.alive || self.rests(dt) {
            return;
        }
        if ticked && self.flying && !self.pose.as_ref().unwrap().has_pose && !self.pose.as_ref().unwrap().legs {
            self.recoil_barrels(w);
            let p = self.pose_mut();
            for seat in &p.seats {
                for weapon in &seat.weapons {
                    for m in &weapon.muzzles {
                        if let (Some(barrel), Some(_)) = (m.barrel, m.rest) {
                            p.rig.tree.update_matrix_world(barrel, true);
                        }
                    }
                }
            }
            return;
        }
        self.place_root(alpha);
        self.feel_legs(w, dt);
        self.apply_pose();
        self.pose_flight_gear();
        self.pose_seat(self.seat_index, 0.0);
        for i in 0..self.seats.len() {
            if i != self.seat_index && self.crewed(i) {
                self.pose_seat(i, 0.0);
            }
        }
        self.recoil_barrels(w);
        let p = self.pose_mut();
        let root = p.rig.root;
        p.rig.tree.update_matrix_world(root, true);
    }

    /// vehicles.js placeRoot: the hull between its last two substeps, with
    /// the repulsor's bob, every bone back at its bind turn, and its wheels.
    fn place_root(&mut self, alpha: f64) {
        let hover = self.repulsor && !self.flying;
        let flyer = if self.flying { Some(self.flyer_frame(alpha)) } else { None };
        let (previous, position) = (self.previous, self.position);
        let (previous_tilt, tilt, previous_yaw, yaw, previous_roll, roll) = (self.previous_tilt, self.tilt, self.previous_yaw, self.yaw, self.previous_roll, self.roll);
        let p = self.pose.as_mut().unwrap();
        p.visual.lerp_vectors(&previous, &position, alpha);
        let root = p.rig.root;
        p.rig.tree.node_mut(root).position = p.visual;
        let phase = p.age * 1.7 + p.hover_phase;
        let bob = if hover { math::sin(phase) * 0.07 + math::sin(phase * 0.61) * 0.025 } else { 0.0 };
        p.rig.tree.node_mut(root).position.y += bob;
        if let Some((frame, bank)) = flyer {
            crate::flight::pose::draw_flyer(&mut p.rig, &mut self.flight.frame_q, &frame, bank);
        } else {
            let x = -lerp(previous_tilt, tilt, alpha);
            let y = previous_yaw + angle(yaw - previous_yaw) * alpha;
            let z = previous_roll + angle(roll - previous_roll) * alpha + if hover { math::sin(phase * 0.73) * 0.006 } else { 0.0 };
            p.rig.tree.node_mut(root).set_rotation(x, y, z, Some(EulerOrder::YXZ));
        }
        for i in 0..p.rig.bones.len() {
            let bind = p.rig.bind_q[i];
            let bone = p.rig.bones[i];
            p.rig.tree.node_mut(bone).copy_quaternion(&bind);
        }
        if p.has_pose {
            for i in 0..p.rig.bones.len() {
                let bone = p.rig.bones[i];
                p.rig.tree.node_mut(bone).position = p.rig.bind_p[i];
            }
        }
        for (k, wheel) in self.wheels.iter().enumerate() {
            let (Some(bone), axis) = p.wheel_bones[k] else { continue };
            let i = p.rig.bone_index(bone).expect("a wheel's bone");
            let bind = p.rig.bind_q[i];
            let mut turn = Quaternion::default();
            turn.set_from_axis_angle(&axis, lerp(wheel.previous, wheel.phase, alpha));
            let node = p.rig.tree.node_mut(bone);
            node.copy_quaternion(&bind);
            node.with_quaternion(|q| {
                q.multiply(&turn);
            });
        }
    }

    /// vehicles.js seatFrame (at the tick): the frame a seat's camera and
    /// aim hang from: a flyer's own frame, a ground hull's heading.
    pub(crate) fn seat_frame(&self, i: usize) -> Matrix4 {
        let aim=if i==0 {None}else{let seat=&self.seats[i];Some((seat.aim_yaw,seat.pitch,&self.pose.as_ref().unwrap().seats[i]))};
        seat_frame_values(&self.position,&self.quaternion,self.flying,self.yaw,aim)
    }

    /// vehicles.js seatEye: a seat's eye in the world.
    pub(crate) fn seat_eye(&self, i: usize) -> Vector3 {
        let frame = self.seat_frame(i);
        let mut out = self.pose.as_ref().unwrap().seats[i].eye;
        out.apply_matrix4(&frame);
        out
    }

    /// vehicles.js seatForward: a seat's aim direction (a flyer's pilot's
    /// along its nose).
    pub(crate) fn seat_forward(&self, i: usize) -> Vector3 {
        if self.flying && i == 0 {
            let mut out = Vector3::new(0.0, 0.0, 1.0);
            out.apply_quaternion(&self.quaternion);
            return out;
        }
        let seat = self.seats.get(i).unwrap_or(&self.seats[0]);
        aim_vector(seat.aim_yaw, seat.pitch)
    }

    /// vehicles.js aimSeats: the point each manned seat's guns turn to, and
    /// how fast each seat's aim turns.
    fn aim_seats<W: VehicleWorld>(&mut self, w: &mut W, dt: f64) {
        for i in 0..self.seats.len() {
            let aim_yaw = self.seats[i].aim_yaw;
            {
                let sp = &mut self.pose.as_mut().unwrap().seats[i];
                if let Some(last) = sp.last_yaw {
                    sp.yaw_rate += (angle(aim_yaw - last) / dt - sp.yaw_rate) * (1.0 - math::exp(-12.0 * dt));
                }
                sp.last_yaw = Some(aim_yaw);
            }
            let player = self.driver && i == self.seat_index;
            debug_assert!(!player, "the page's player has no seat in a world");
            if !player && !self.crewed(i) {
                continue;
            }
            let origin = self.seat_eye(i);
            let sp = &self.pose.as_ref().unwrap().seats[i];
            let direction = if sp.aiming { sp.aim } else { self.seat_forward(i) };
            let hit = w.cast(&origin, &direction, AIM_REACH, Some(self.body));
            let mut far = match hit {
                Some(h) => h.distance,
                None => AIM_REACH,
            };
            if let Some(distance) = w.raycast_units(&origin, &direction, far) {
                far = distance;
            }
            let mut target = origin;
            target.add_scaled_vector(&direction, far);
            self.pose.as_mut().unwrap().seats[i].target = Some(target);
        }
    }

    /// vehicles.js poseSeat: a seat's turret node swung to its aim, and its
    /// aimers turned toward its target (a turn only with time, dt > 0).
    fn pose_seat(&mut self, i: usize, dt: f64) {
        let (aim_yaw, pitch) = (self.seats[i].aim_yaw, self.seats[i].pitch);
        let seats = &self.seats; let driver = self.driver; let seat_index = self.seat_index;
        let p = self.pose.as_mut().unwrap();
        let root = p.rig.root;
        let turret = p.seats[i].turret;
        if let Some(turret) = turret {
            let root_yaw = p.rig.tree.node_mut(root).rotation().y;
            let sp = &p.seats[i];
            let q2 = look_basis(&aim_vector(sp.yaw_factor * angle(aim_yaw - root_yaw), sp.pitch_factor * pitch), &sp.up);
            let parent = p.rig.tree.node(turret).parent.expect("a turret's parent");
            let mut q = Quaternion::default();
            p.rig.tree.get_world_quaternion(parent, &mut q);
            q.invert();
            let root_q = p.rig.tree.node(root).quaternion;
            q.multiply(&root_q).multiply(&q2);
            p.rig.tree.node_mut(turret).copy_quaternion(&q);
            p.rig.tree.update_matrix_world(turret, true);
        }
        let target = p.seats[i].target;
        for wi in 0..p.seats[i].weapons.len() {
            for ai in 0..p.seats[i].weapons[wi].aimers.len() {
                let Some(bone) = p.seats[i].weapons[wi].aimers[ai].bone else { continue };
                if Some(bone) == turret {
                    let m = p.rig.tree.node(bone).matrix_world;
                    let a = &mut p.seats[i].weapons[wi].aimers[ai];
                    a.facing.set(0.0, 0.0, 1.0).transform_direction(&m);
                    continue;
                }
                let rest = p.rig.bind_q[p.rig.bone_index(bone).expect("an aimer's bone")];
                let parent = p.rig.tree.node(bone).parent.expect("an aimer's parent");
                p.rig.tree.update_world_matrix(parent, true, false);
                let parent_world = p.rig.tree.node(parent).matrix_world;
                let bone_position = p.rig.tree.node(bone).position;
                let a = &mut p.seats[i].weapons[wi].aimers[ai];
                if dt > 0.0 {
                    let mut v;
                    let mut q = Quaternion::default();
                    if let Some(target) = target {
                        v = target;
                        let mut to_local = parent_world;
                        to_local.invert();
                        q.copy(&rest).invert();
                        v.apply_matrix4(&to_local).sub(&bone_position).apply_quaternion(&q);
                    } else {
                        let (mut at, mut scale) = (Vector3::default(), Vector3::default());
                        parent_world.decompose(&mut at, &mut q, &mut scale);
                        v = aim_vector(aim_yaw, pitch);
                        q.multiply(&rest).invert();
                        v.apply_quaternion(&q);
                    }
                    let step = a.yaw_rate * dt;
                    let mut yaw = a.yaw + clamp(angle(math::atan2(v.x, v.z) - a.yaw), -step, step);
                    if a.wrap_yaw && yaw < 0.0 {
                        yaw += 2.0 * std::f64::consts::PI;
                    }
                    let pitch = approach(a.pitch, math::atan2(v.y, hypot(v.x, v.z)), a.pitch_rate * dt);
                    let ([y0, y1], [p0, p1]) = (a.yaw_limits, a.pitch_limits);
                    a.in_limits = Some((y0 == y1 || (yaw >= y0 && yaw <= y1)) && (p0 == p1 || (pitch >= p0 && pitch <= p1)));
                    a.yaw = clamp(yaw, y0, y1);
                    a.pitch = clamp(pitch, p0, p1);
                }
                let mut turn = rest;
                let mut q = Quaternion::default();
                q.set_from_axis_angle(&UP, a.yaw);
                turn.multiply(&q);
                let mut q2 = Quaternion::default();
                q2.set_from_axis_angle(&RIGHT, -a.pitch);
                turn.multiply(&q2);
                let parent_world = p.rig.tree.node(parent).matrix_world;
                a.facing.set(0.0, 0.0, 1.0).apply_quaternion(&turn).transform_direction(&parent_world);
                a.posed = a.owner < 0 || !(seats[a.owner as usize].lease.is_some() || driver && seat_index == a.owner as usize);
                if a.posed {
                    p.rig.tree.node_mut(bone).copy_quaternion(&turn);
                    p.rig.tree.update_matrix_world(bone, true);
                }
            }
            for ai in 0..p.seats[i].weapons[wi].aimers.len() {
                p.seats[i].weapons[wi].aimers[ai].on_target = false;
                let (Some(target), Some(bone)) = (target, p.seats[i].weapons[wi].aimers[ai].bone) else { continue };
                let mut v = Vector3::default();
                p.rig.tree.get_world_position(bone, &mut v);
                v.sub(&target).negate();
                let span = v.length();
                let a = &mut p.seats[i].weapons[wi].aimers[ai];
                a.on_target = span > 1e-3 && v.dot(&a.facing) > span * math::cos(AIMED);
            }
        }
    }

    /// vehicles.js recoilBarrels: each barrel back along its z by its kick.
    fn recoil_barrels<W: VehicleWorld>(&mut self, w: &mut W) {
        let now = w.clock();
        let p = self.pose.as_mut().unwrap();
        for seat in &p.seats {
            for weapon in &seat.weapons {
                for m in &weapon.muzzles {
                    let (Some(barrel), Some(rest)) = (m.barrel, m.rest) else { continue };
                    let kick = match m.kick_at {
                        None => 0.0,
                        Some(at) => v8math::max(0.0, 1.0 - 4.0 * (now - at)),
                    };
                    p.rig.tree.node_mut(barrel).position = rest;
                    if kick > 0.0 {
                        if let Some(recoil) = m.recoil.filter(|&r| super::hull::truthy(r)) {
                            let mut axis = Vector3::new(0.0, 0.0, 1.0);
                            let q = p.rig.tree.node(barrel).quaternion;
                            axis.apply_quaternion(&q);
                            p.rig.tree.node_mut(barrel).position.add_scaled_vector(&axis, -recoil * kick);
                        }
                    }
                }
            }
        }
    }

    /// vehicles.js walk: the walk cycle advanced by a tick.
    pub fn walk(&mut self, dt: f64) {
        let speed = hypot(self.velocity.x, self.velocity.z);
        let (vx, vz, yaw, creature) = (self.velocity.x, self.velocity.z, self.yaw, self.creature);
        let p = self.pose.as_mut().unwrap();
        if !p.has_pose || dt <= 0.0 {
            return;
        }
        let clips = p.clips.as_ref();
        let (left, right) = (clips.and_then(|c| c.left.as_ref()), clips.and_then(|c| c.right.as_ref()));
        let length = left.map_or(0.0, |c| or_zero(c.duration)) + right.map_or(0.0, |c| or_zero(c.duration));
        let moving = speed > 0.05 && left.is_some() && right.is_some() && length > 0.0;
        p.walk_blend += ((if moving { 1.0 } else { 0.0 }) - p.walk_blend) * (1.0 - math::exp(-8.0 * dt));
        if let (true, Some(left), Some(right), true) = (p.walk_blend > 0.0001, left, right, length > 0.0) {
            let natural = v8math::max(0.1, (left.stride + right.stride) / length);
            let forward_speed = vx * math::sin(yaw) + vz * math::cos(yaw);
            let direction = if creature && forward_speed < 0.0 { -1.0 } else { 1.0 };
            p.walk_phase = ((p.walk_phase + dt * speed / natural * direction) % length + length) % length;
        }
    }

    /// vehicles.js applyPose: the bones from the walk cycle (the idle clip
    /// at the hull's age under the walk clips at its phase, by its blend),
    /// then the legs lifted toward the higher ground.
    fn apply_pose(&mut self) {
        let p = self.pose.as_mut().unwrap();
        if !p.has_pose {
            return;
        }
        let (phase, blend, age) = (p.walk_phase, p.walk_blend, p.age);
        let count = p.rig.bones.len();
        let root = p.rig.pose_root.unwrap_or(-1);
        let mut pose = p.work.walk.take().unwrap_or_else(|| Pose::new(count, root));
        let clips = p.clips.as_ref();
        let (left, right) = (clips.and_then(|c| c.left.as_ref()), clips.and_then(|c| c.right.as_ref()));
        let length = left.map_or(0.0, |c| or_zero(c.duration)) + right.map_or(0.0, |c| or_zero(c.duration));
        pose.clear();
        add(&mut pose, clips.and_then(|c| c.idle.as_ref()), age, 1.0 - blend, true);
        if let (true, Some(left), Some(right), true) = (blend > 0.0001, left, right, length > 0.0) {
            let (clip, time) = if phase < left.duration { (left, phase) } else { (right, phase - left.duration) };
            add(&mut pose, Some(clip), time, blend, false);
        }
        apply(&mut pose, &mut p.rig);
        p.work.walk = Some(pose);
        self.lift_legs();
    }

    /// vehicles.js liftLegs: each leg pair's legs drawn toward the pose with
    /// the foot on the higher ground lifted, by how much higher it is.
    fn lift_legs(&mut self) {
        let Some(sides) = self.leg_side.as_ref() else { return };
        let p = self.pose.as_mut().unwrap();
        let (Some(legs), Some(clips)) = (p.leg_pairs.as_ref(), p.clips.as_ref()) else { return };
        let (Some(up_left), Some(up_right)) = (clips.lift_left.as_ref(), clips.lift_right.as_ref()) else { return };
        let count = p.rig.bones.len();
        // legBones: the bones under each pair's two leg bones, in bone order.
        let leg_bones = p.work.leg_bones.get_or_insert_with(|| legs
            .names
            .iter()
            .map(|pair| {
                let mut under: Vec<NodeId> = Vec::new();
                for name in pair {
                    if let Some(node) = p.rig.by_name.get(name).map(|i| p.rig.bones[i]) {
                        p.rig.tree.traverse(node, &mut |b| under.push(b));
                    }
                }
                (0..count).filter(|&i| under.contains(&p.rig.bones[i])).collect()
            })
            .collect());
        let mut lp = p.work.lift.take().unwrap_or_else(|| Pose::new(count, p.rig.pose_root.unwrap_or(-1)));
        for (n, bones) in leg_bones.iter().enumerate() {
            let side = or_zero(sides.get(n).copied().unwrap_or(f64::NAN));
            let weight = v8math::min(1.0, v8math::abs(side));
            if weight < 1e-3 {
                continue;
            }
            lp.clear();
            add(&mut lp, Some(if side < 0.0 { up_left } else { up_right }), 0.0, 1.0, false);
            for &i in bones {
                let w = lp.w[i] as f64;
                if w <= 1e-6 {
                    continue;
                }
                let mut q = Quaternion::new(lp.q[i * 4] as f64, lp.q[i * 4 + 1] as f64, lp.q[i * 4 + 2] as f64, lp.q[i * 4 + 3] as f64);
                q.normalize();
                let target = Vector3::new(lp.t[i * 3] as f64 / w, lp.t[i * 3 + 1] as f64 / w, lp.t[i * 3 + 2] as f64 / w);
                let bone = p.rig.tree.node_mut(p.rig.bones[i]);
                bone.slerp_quaternion(&q, weight);
                bone.position.lerp(&target, weight);
            }
        }
        p.work.lift = Some(lp);
    }

    /// vehicles.js feelLegs: each leg pair's ground under its feet as drawn
    /// (legSide, from the placed root), the hull's own body out of the casts.
    fn feel_legs<W: VehicleWorld>(&mut self, w: &mut W, dt: f64) {
        let p = self.pose.as_mut().unwrap();
        let Some(legs) = p.leg_pairs.as_ref() else { return };
        if self.creature || !(dt > 0.0) {
            return;
        }
        let l = legs.ray_length;
        let root = p.rig.root;
        let r = p.rig.tree.node_mut(root).rotation();
        let root_position = p.rig.tree.node(root).position;
        let mut q = Quaternion::default();
        q.set_from_euler(&three::Euler::new(r.x, r.y, r.z, EulerOrder::YXZ));
        if self.leg_side.is_none() {
            let mut sides = std::mem::take(&mut self.leg_side_scratch);
            sides.resize(legs.at.len(), 0.0);
            self.leg_side = Some(sides);
        }
        let enabled = w.body_enabled(self.body);
        w.enable_body(self.body, false);
        for (i, pair) in legs.at.iter().enumerate() {
            let mut h = [0.0; 2];
            for (j, at) in pair.iter().enumerate() {
                let mut from = Vector3::new(at[0], at[1], at[2]);
                from.apply_quaternion(&q).add(&root_position);
                from.y += l;
                h[j] = match w.cast(&from, &DOWN, 2.0 * l, None) {
                    Some(hit) => l - hit.distance,
                    None => -l,
                };
            }
            let sides = self.leg_side.as_mut().unwrap();
            sides[i] += (clamp(-0.6 * (h[0] - h[1]), -1.0, 1.0) - sides[i]) * v8math::min(1.0, 0.4 * dt * 60.0);
        }
        w.enable_body(self.body, enabled);
    }
}

/// `x || 0` for a clip's duration.
fn or_zero(x: f64) -> f64 {
    super::hull::or_zero(x)
}

/// Pose.add: nothing for a missing clip or a weight at or under 1e-4.
fn add(pose: &mut Pose, clip: Option<&WalkClip>, time: f64, weight: f64, looping: bool) {
    let Some(clip) = clip else { return };
    if weight <= 1e-4 {
        return;
    }
    pose.add(&clip.clip, time, weight, looping);
}

/// Pose.apply: each touched bone's normalized quaternion and its
/// translation over its weight.
fn apply(pose: &mut Pose, rig: &mut HullRig) {
    pose.apply_out();
    for i in 0..rig.bones.len() {
        if pose.w[i] as f64 <= 1e-6 {
            continue;
        }
        let o = i * 7;
        let node = rig.tree.node_mut(rig.bones[i]);
        node.set_quaternion(pose.out[o], pose.out[o + 1], pose.out[o + 2], pose.out[o + 3]);
        node.position = Vector3::new(pose.out[o + 4], pose.out[o + 5], pose.out[o + 6]);
    }
}

/// A hull class's walk clips, from its template (rust/content): the pose
/// sampler's copy of each, its length and its stride. None for a template
/// without clips (`this.clips` null).
pub fn walk_clips(template: &bfe_content::content::VehicleTemplate) -> Option<WalkClips> {
    let clips = template.clips.as_ref()?;
    let get = |name: &str| {
        clips.get(name).map(|c| WalkClip { clip: std::rc::Rc::new(crate::rig::content::clip(c)), duration: c.duration, stride: c.stride })
    };
    Some(WalkClips {
        idle: get("idle"),
        left: get("walk_leftfoot_rightfoot"),
        right: get("walk_rightfoot_leftfoot"),
        lift_left: get("leftfoot_up"),
        lift_right: get("rightfoot_up"),
    })
}

/// A walker class's leg pairs (`def.fx.legs`), or None without legs.
pub fn leg_pairs(def: &bfe_content::value::Value) -> Option<LegPairs> {
    let legs = def.get("fx")?.get("legs")?;
    let num = |v: Option<&bfe_content::value::Value>| v.and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
    let mut out = LegPairs { ray_length: num(legs.get("rayLength")), at: Vec::new(), names: Vec::new() };
    for pair in legs.get("pairs").and_then(|p| p.as_array()).map(|a| a.iter().collect::<Vec<_>>()).unwrap_or_default() {
        let foot = |which: &str| {
            let f = pair.get(which);
            let at = f.and_then(|f| f.get("at"));
            let c = |i: usize| at.and_then(|a| a.as_array()).and_then(|a| a.get(i)).and_then(|x| x.as_f64()).unwrap_or(f64::NAN);
            let name = f.and_then(|f| f.get("leg")).and_then(|l| l.as_str()).unwrap_or("").to_string();
            ([c(0), c(1), c(2)], name)
        };
        let (left, right) = (foot("left"), foot("right"));
        out.at.push([left.0, right.0]);
        out.names.push([left.1, right.1]);
    }
    Some(out)
}

/// vehicles.js ninePose(view, yaw, pitch, yawRate): where a 9-pose rider's
/// clip is sampled, (pitch, turn) each in -1..1: the seat's pitch across
/// its limits; its yaw across a limited arc (from the hull's heading, a
/// wrapped arc's back half past 2pi), else how fast it turns against the
/// seat's turn rate.
pub fn nine_pose(pitch_limits: [f64; 2], yaw_limits: Option<[f64; 2]>, wrap_yaw: bool, turn_rate: f64, yaw: f64, pitch: f64, yaw_rate: f64) -> (f64, f64) {
    let (low, high) = (pitch_limits[0], pitch_limits[1]);
    let p = if high > low { clamp(2.0 * ((high - pitch) / (high - low) - 0.5), -1.0, 1.0) } else { 0.0 };
    let t = match yaw_limits {
        Some([a, b]) if b - a < 1.5 * std::f64::consts::PI => {
            let y = if wrap_yaw && yaw < 0.0 { yaw + 2.0 * std::f64::consts::PI } else { yaw };
            if b > a {
                clamp(2.0 * ((y - a) / (b - a) - 0.5), -1.0, 1.0)
            } else {
                0.0
            }
        }
        _ => clamp(yaw_rate / v8math::max(turn_rate, 1e-3), -1.0, 1.0),
    };
    (p, t)
}
