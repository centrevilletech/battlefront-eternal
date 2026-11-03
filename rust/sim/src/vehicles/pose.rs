//! A hull's pose at the tick (src/vehicles.js aimTick, render, placeRoot,
//! aimSeats, seatEye, seatFrame, seatForward, poseSeat, recoilBarrels,
//! lookBasis, aimVector): its root placed between its last two substeps,
//! each manned seat's aim target (the first thing along its aim within
//! 512 m), its turret node and its aimers turned toward that target at
//! their rates, its barrels' kick, and one pass over the rig for the
//! muzzles, eyes and riders the rest of the tick reads.
//!
//! The rig is rust/sim/src/rig/hull.rs's, posed here; a flyer's pose (its
//! gear, its drawn bank) is the flight package's, and a walker's walk
//! cycle (walk, applyPose, liftLegs, feelLegs) comes next.

use super::hull::{angle, approach, Hull, UP};
use super::VehicleWorld;
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

/// What a hull's pose keeps beside its tick's state.
#[derive(Clone, Debug)]
pub struct HullPose {
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

impl Hull {
    fn pose_mut(&mut self) -> &mut HullPose {
        self.pose.as_mut().expect("a posed hull's rig")
    }

    /// Whether `aimTick` and `render` leave the hull as it is.
    fn rests(&self, dt: f64) -> bool {
        let p = self.pose.as_ref().unwrap();
        dt > 0.0 && self.sleeping == Some(true) && !self.repulsor && !self.occupied() && !p.has_pose && self.health >= self.max_health * 0.35
    }

    /// `held(i)`: someone holds seat i (a lease, or the player).
    fn held(&self, i: i32) -> bool {
        i >= 0 && (self.seats.get(i as usize).is_some_and(|s| s.lease.is_some()) || self.driver && self.seat_index == i as usize)
    }

    /// vehicles.js aimTick: the tick's part of the pose (a ground hull's).
    pub fn aim_tick<W: VehicleWorld>(&mut self, w: &mut W, alpha: f64, dt: f64) {
        if !self.alive || self.rests(dt) {
            return;
        }
        debug_assert!(!self.flying && !self.pose.as_ref().unwrap().has_pose, "a flyer's or walker's pose is not here yet");
        self.place_root(alpha);
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

    /// vehicles.js render (ticked: after aimTick, in a world): the drawn
    /// pose, a ground hull's.
    pub fn render<W: VehicleWorld>(&mut self, w: &mut W, alpha: f64, dt: f64) {
        if !self.alive || self.rests(dt) {
            return;
        }
        debug_assert!(!self.flying && !self.pose.as_ref().unwrap().has_pose && !self.pose.as_ref().unwrap().legs, "a flyer's or walker's pose is not here yet");
        self.place_root(alpha);
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
        let (previous, position) = (self.previous, self.position);
        let (previous_tilt, tilt, previous_yaw, yaw, previous_roll, roll) = (self.previous_tilt, self.tilt, self.previous_yaw, self.yaw, self.previous_roll, self.roll);
        let p = self.pose.as_mut().unwrap();
        p.visual.lerp_vectors(&previous, &position, alpha);
        let root = p.rig.root;
        p.rig.tree.node_mut(root).position = p.visual;
        let phase = p.age * 1.7 + p.hover_phase;
        let bob = if hover { math::sin(phase) * 0.07 + math::sin(phase * 0.61) * 0.025 } else { 0.0 };
        p.rig.tree.node_mut(root).position.y += bob;
        let x = -lerp(previous_tilt, tilt, alpha);
        let y = previous_yaw + angle(yaw - previous_yaw) * alpha;
        let z = previous_roll + angle(roll - previous_roll) * alpha + if hover { math::sin(phase * 0.73) * 0.006 } else { 0.0 };
        p.rig.tree.node_mut(root).set_rotation(x, y, z, Some(EulerOrder::YXZ));
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

    /// vehicles.js seatFrame (at the tick, a ground hull's): the frame a
    /// seat's camera and aim hang from.
    fn seat_frame(&self, i: usize) -> Matrix4 {
        let mut out = Matrix4::default();
        let mut q = Quaternion::default();
        q.set_from_axis_angle(&UP, self.yaw);
        out.compose(&self.position, &q, &UNIT_SCALE);
        if i == 0 {
            return out;
        }
        let seat = &self.seats[i];
        let view = &self.pose.as_ref().unwrap().seats[i];
        let q2 = look_basis(&aim_vector(angle(seat.aim_yaw - self.yaw), seat.pitch), &view.up);
        let mut m = Matrix4::default();
        m.compose(&view.pivot, &q2, &UNIT_SCALE);
        out.multiply(&m);
        out
    }

    /// vehicles.js seatEye: a seat's eye in the world.
    fn seat_eye(&self, i: usize) -> Vector3 {
        let frame = self.seat_frame(i);
        let mut out = self.pose.as_ref().unwrap().seats[i].eye;
        out.apply_matrix4(&frame);
        out
    }

    /// vehicles.js seatForward: a seat's aim direction (a ground hull's).
    fn seat_forward(&self, i: usize) -> Vector3 {
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
        let held: Vec<bool> = (0..self.seats.len() as i32).map(|k| self.held(k)).collect();
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
                a.posed = a.owner < 0 || !held[a.owner as usize];
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
}
