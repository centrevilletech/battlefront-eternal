//! The turret crews (src/seats.js TurretSeats: man, canMan, requestExit,
//! leave, vacate, destroyed, update, drive, pose; turretAimPoint) and the
//! turrets they drive (src/props.js Turret: steer, holdYaw, present,
//! gunWant, aimGun, muzzle, fireDirection, viewDirection, aimFrame,
//! eyePoint, cameraTurn, cameraRest, aimOrigin, seatPoint, seatYaw).
//!
//! A turret is the props package's; what its crew reads and writes of it
//! (its aim, its gun's angles, its barrel, its lease and gunner, its rig's
//! nodes) is a `TurretSide` here until that package declares turrets in
//! the store. Its rig is a `TransformTree` as src/props.js builds it (the
//! scene, the root placed by the turret's matrix, the model's bones); what
//! a turret is for the battle (its limits, rates, camera, pivot and the
//! turns its constructor works out) is its `TurretDesc`. The weapon is the
//! combat package's: its tick, lock and rounds go through the trait.

use super::bodies::{drop_body, place_on_foot, seat_body, unseat_body};
use super::exits::{classic_seat_exit, ExitFrame, ExitSpot};
use super::hulls::RiderFacts;
use super::lease::{latch_lease, live, network_human, new_lease, person_lease, take, take_reload, Controls};
use super::{unit_ref, Attacker, Holder, Reason, SeatWorld};
use crate::gen::{Body, Post, SeatLease, SeatRole, SeatUnit, Store, Turret};
use crate::rig::math;
use crate::store::Ref;
use crate::vehicles::pose::{aim_vector, look_basis};
use std::collections::BTreeMap;
use three::{clamp, Euler, EulerOrder, Matrix4, NodeId, Quaternion, TransformTree, Vector3};

const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const X_AXIS: Vector3 = Vector3::new(1.0, 0.0, 0.0);
const ONE: Vector3 = Vector3::new(1.0, 1.0, 1.0);
/// seats.js AIM_REACH: m along the aim to the point the gun turns to.
const AIM_REACH: f64 = 512.0;
/// props.js AIMED: a gun this close to its target fires straight at it.
const AIMED: f64 = std::f64::consts::PI / 180.0;

/// What a turret is for the battle, as the Turret constructor works it out.
#[derive(Clone, Debug, Default)]
pub struct TurretDesc {
    /// The building's heading: atan2 of its matrix's forward axis.
    pub facing: f64,
    /// Building.position (its matrix's translation) and Building.matrix.
    pub position: Vector3,
    pub matrix: [f64; 16],
    /// Its built look's collision body.
    pub body: Option<Ref<Body>>,
    /// The view's pitch limits, up positive.
    pub pitch_limits: [f64; 2],
    /// The yaw's arc from the heading (None: free all round), and whether it
    /// is one round the back (kept in 0..2pi).
    pub yaw_limits: Option<[f64; 2]>,
    pub wrap_yaw: bool,
    pub gun_pitch_limits: [f64; 2],
    pub gun_yaw_limits: [f64; 2],
    pub turn_rate: f64,
    pub filter: f64,
    pub gun_pitch_rate: f64,
    pub gun_yaw_rate: f64,
    pub yaw_factor: f64,
    pub pitch_factor: f64,
    /// The camera section: EyePointOffset, TrackCenter, TrackOffset, TiltValue.
    pub camera_eye: Vector3,
    pub camera_center: Vector3,
    pub camera_offset: Vector3,
    pub camera_tilt: f64,
    /// The aim frame's pivot and up in the model's frame.
    pub pivot: Vector3,
    pub up: Vector3,
    /// The turn from the model's frame into the drum's parent's (None
    /// without a drum, or a drum with no parent).
    pub drum_parent_turn: Option<Quaternion>,
    /// The gun's bind turn.
    pub gun_bind: Option<Quaternion>,
    /// Whether its gunner shows (a pilot animation): hidesGunner is the
    /// opposite; and whether rounds meet him there (hash 2b60e217 not 0).
    pub rider: bool,
    pub shootable: bool,
}

/// A turret's rig: the scene it hangs from, its root (placed by the
/// building's matrix, not updated from its own transform) and its model's
/// bones, with the named ones the turret reads.
#[derive(Clone, Debug, Default)]
pub struct TurretRig {
    pub tree: TransformTree,
    pub root: NodeId,
    pub bones: Vec<NodeId>,
    pub drum: Option<NodeId>,
    pub gun: Option<NodeId>,
    pub seat: Option<NodeId>,
    pub fire_points: Vec<NodeId>,
}

impl TurretRig {
    /// A rig from its root's matrix and its bones (each one's parent among
    /// the bones, None for the root, and its local position, turn and
    /// scale), the scene above it as three's identity Scene. `world`: each
    /// bone's world matrix as the last update left it, where known (a bone
    /// no update reaches keeps it).
    pub fn new(root_matrix: &[f64; 16], bones: &[(Option<usize>, Vector3, Quaternion, Vector3)], world: Option<&[[f64; 16]]>) -> TurretRig {
        let mut tree = TransformTree::new();
        let scene = tree.add_node(three::TransformNode::default());
        let root = tree.add_node(three::TransformNode::default());
        {
            let r = tree.node_mut(root);
            r.matrix_auto_update = false;
            r.matrix.from_array(root_matrix, 0);
        }
        tree.add_child(scene, root);
        let mut ids = Vec::with_capacity(bones.len());
        for (i, (_, p, q, s)) in bones.iter().enumerate() {
            let mut n = three::TransformNode::placed(*p, *s);
            n.quaternion = *q;
            if let Some(world) = world {
                n.matrix_world.from_array(&world[i], 0);
            }
            ids.push(tree.add_node(n));
        }
        for (i, (parent, ..)) in bones.iter().enumerate() {
            let parent = match parent {
                Some(p) => ids[*p],
                None => root,
            };
            tree.node_mut(parent).children.push(ids[i]);
            tree.node_mut(ids[i]).parent = Some(parent);
        }
        TurretRig { tree, root, bones: ids, drum: None, gun: None, seat: None, fire_points: Vec::new() }
    }
}

/// A turret as its crew reads and writes it.
#[derive(Clone, Debug, Default)]
pub struct TurretSide {
    /// Its place among the battle's turrets (LiveObjects.turrets).
    pub index: usize,
    /// The store's name for it.
    pub turret: Ref<Turret>,
    /// Building.alive: it stands built.
    pub alive: bool,
    pub lease: Option<Ref<SeatLease>>,
    /// Whose body mans it (`gunner`: his soldier).
    pub gunner: Option<u32>,
    pub yaw: f64,
    pub pitch: f64,
    pub yaw_vel: f64,
    pub pitch_vel: f64,
    pub gun_yaw: f64,
    pub gun_pitch: f64,
    pub barrel: f64,
    pub last_attacker: Attacker,
    /// Building.sphere as it stands: its centre in the world and radius.
    pub sphere: (Vector3, f64),
    /// Whether it has a weapon, and whether that weapon locks on.
    pub weapon: bool,
    pub missile_lock: bool,
    pub desc: TurretDesc,
    pub rig: TurretRig,
}

/// A shot a weapon fired (Weapon.update's): its spread.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shot {
    pub yaw: f64,
    pub pitch: f64,
}

/// What the turret crews ask of the rest of the battle beyond the seats'
/// own calls: the turret's weapon (the combat package's) and its rounds.
/// `turret` names a turret by its place among the battle's turrets.
pub trait CrewWorld: SeatWorld {
    /// updateWeaponLock for the turret's weapon (its view from `origin`
    /// along `dir`, the turret's own target and body left out), its bot's
    /// target given (seatLockTarget(lease)).
    fn update_lock(&mut self, turret: usize, dt: f64, origin: &Vector3, dir: &Vector3, lease: Ref<SeatLease>);
    /// Weapon.update(dt, true, { held, pressed }, reload, a standing
    /// soldier's stance): the shots it fires.
    fn weapon_update(&mut self, turret: usize, dt: f64, held: bool, pressed: bool, reload: bool) -> Vec<Shot>;
    /// Ordnance.fire: the weapon's round from `origin` along `direction`,
    /// the lease's unit's, the turret's body left out, following the
    /// lease's emitter; what it returns, for bind_round.
    fn fire(&mut self, turret: usize, origin: &Vector3, direction: &Vector3, owner: Option<u32>, lease: Ref<SeatLease>, shot: usize) -> usize;
    /// bindMountedRound(round, turret, -1, -1, barrel).
    fn bind_round(&mut self, round: usize, turret: usize, barrel: f64);
    /// emitSound("turretFire", turret, shot).
    fn sound(&mut self, turret: usize, shot: usize);
    /// The battle's hook: the turret's seat fired.
    fn fired(&mut self, turret: usize, shot: usize, owner: Option<u32>, muzzle: &Vector3, direction: &Vector3);
}

/// TurretSeats.vacate's options.
#[derive(Clone, Copy, Debug)]
pub struct Let {
    pub placement: bool,
    pub post: Option<Ref<Post>>,
    pub kill: bool,
    pub attacker: Attacker,
    pub reason: Reason,
}

impl Default for Let {
    fn default() -> Self {
        Let { placement: true, post: None, kill: false, attacker: Attacker::Nobody, reason: Reason::Exit }
    }
}

/// `Math.atan2(Math.sin(v), Math.cos(v))`.
fn wrap(v: f64) -> f64 {
    math::atan2(math::sin(v), math::cos(v))
}

impl TurretSide {
    /// Turret.viewDirection: the heading turned by the yaw, tipped by the
    /// pitch.
    pub fn view_direction(&self, yaw: f64, pitch: f64) -> Vector3 {
        aim_vector(self.desc.facing + yaw, pitch)
    }

    /// Turret.holdYaw (props.js:652).
    pub fn hold_yaw(&self, yaw: f64) -> f64 {
        let Some(arc) = self.desc.yaw_limits else { return yaw };
        let mut a = wrap(yaw);
        if self.desc.wrap_yaw && a < 0.0 {
            a += 2.0 * std::f64::consts::PI;
        }
        clamp(a, arc[0], arc[1])
    }

    /// Turret.steer (props.js:662): turn and pitch rates steer the aim,
    /// inside the arc and pitch limits.
    pub fn steer(&mut self, dt: f64, turn_input: f64, pitch_input: f64, zoom: f64) {
        let k = v8math::min(1.0, self.desc.filter * dt);
        let rate = self.desc.turn_rate / zoom;
        self.yaw_vel += (turn_input * rate - self.yaw_vel) * k;
        self.pitch_vel += (pitch_input * rate - self.pitch_vel) * k;
        let yaw = self.yaw + self.yaw_vel * dt;
        self.yaw = if self.desc.yaw_limits.is_some() { self.hold_yaw(yaw) } else { wrap(yaw) };
        if self.desc.yaw_limits.is_some() && v8math::abs(wrap(yaw - self.yaw)) > 1e-9 {
            self.yaw_vel = 0.0;
        }
        self.pitch = clamp(self.pitch + self.pitch_vel * dt, self.desc.pitch_limits[0], self.desc.pitch_limits[1]);
    }

    /// `rig.root.updateMatrixWorld(true)`.
    fn update_rig(&mut self) {
        let root = self.rig.root;
        self.rig.tree.update_matrix_world(root, true);
    }

    /// Turret.present (props.js:755): the drum and gun from the aim and the
    /// gun's own angles.
    pub fn present(&mut self) {
        let (yaw, pitch) = (self.yaw, self.pitch);
        if let (Some(drum), Some(parent_turn)) = (self.rig.drum, self.desc.drum_parent_turn) {
            let q = look_basis(&aim_vector(self.desc.yaw_factor * yaw, self.desc.pitch_factor * pitch), &self.desc.up);
            self.rig.tree.node_mut(drum).with_quaternion(|dq| {
                dq.copy(&parent_turn);
                dq.multiply(&q);
            });
        }
        let Some(gun) = self.rig.gun else { return };
        let e = Euler::new(-self.gun_pitch, self.gun_yaw, 0.0, EulerOrder::YXZ);
        let mut q = Quaternion::default();
        q.set_from_euler(&e);
        let bind = self.desc.gun_bind.expect("a gun's bind turn");
        self.rig.tree.node_mut(gun).with_quaternion(|gq| {
            gq.copy(&bind);
            gq.multiply(&q);
        });
    }

    /// Turret.gunWant (props.js:769): the gun's angles toward a point (None:
    /// the drum's own pitch), held inside its limits: (yaw, pitch).
    pub fn gun_want(&mut self, target: Option<&Vector3>) -> (f64, f64) {
        let mut yaw = 0.0;
        let mut pitch = self.pitch;
        if let Some(target) = target {
            self.update_rig();
            let gun = self.rig.gun.expect("a gun");
            let parent = self.rig.tree.node(gun).parent.expect("a gun's parent");
            let mut aim = *target;
            self.rig.tree.world_to_local(parent, &mut aim);
            let p = self.rig.tree.node(gun).position;
            aim.sub(&p);
            yaw = math::atan2(aim.x, aim.z);
            pitch = math::atan2(aim.y, v8math::hypot(aim.x, aim.z));
        }
        let l = &self.desc;
        (clamp(yaw, l.gun_yaw_limits[0], l.gun_yaw_limits[1]), clamp(pitch, l.gun_pitch_limits[0], l.gun_pitch_limits[1]))
    }

    /// Turret.aimGun (props.js:787): toward the target at the gun's rates.
    pub fn aim_gun(&mut self, dt: f64, target: Option<&Vector3>) {
        if self.rig.gun.is_none() {
            return;
        }
        let (want_yaw, want_pitch) = self.gun_want(target);
        let step = |from: f64, to: f64, rate: f64| from + clamp(to - from, -rate * dt, rate * dt);
        self.gun_yaw = step(self.gun_yaw, want_yaw, self.desc.gun_yaw_rate);
        self.gun_pitch = step(self.gun_pitch, want_pitch, self.desc.gun_pitch_rate);
    }

    /// Turret.pose (props.js:745): the gun toward a point, then the drum and
    /// gun posed.
    pub fn pose(&mut self, dt: f64, target: Option<&Vector3>) {
        self.aim_gun(dt, target);
        self.present();
    }

    /// Turret.muzzle (props.js:796): its fire point in the world (barrels
    /// take turns when it has several; advance moves on to the next), and
    /// the direction the gun points.
    pub fn muzzle(&mut self, advance: bool, direction: Option<&mut Vector3>) -> Vector3 {
        self.update_rig();
        let n = self.rig.fire_points.len();
        let point = if n > 1 { Some(self.rig.fire_points[(self.barrel % n as f64) as usize]) } else { self.rig.fire_points.first().copied() };
        if advance {
            self.barrel += 1.0;
        }
        let node = point.or(self.rig.gun).unwrap_or(self.rig.root);
        let mut position = Vector3::default();
        self.rig.tree.get_world_position(node, &mut position);
        if let Some(direction) = direction {
            let node = self.rig.gun.unwrap_or(self.rig.root);
            let m = self.rig.tree.node(node).matrix_world;
            direction.set(0.0, 0.0, 1.0).transform_direction(&m);
        }
        position
    }

    /// Turret.fireDirection (props.js:815), with `along` and `out` the same
    /// vector, as TurretSeats.drive passes them: straight at the target once
    /// the gun has turned as far toward it as it can; along the gun while it
    /// is still turning.
    pub fn fire_direction(&mut self, from: &Vector3, target: &Vector3, dir: &mut Vector3) {
        let Some(gun) = self.rig.gun else { return };
        let mut eye = Vector3::default();
        self.rig.tree.get_world_position(gun, &mut eye);
        let mut aim = Vector3::default();
        aim.sub_vectors(target, &eye);
        let span = aim.length();
        let (want_yaw, want_pitch) = self.gun_want(Some(target));
        let settled = v8math::abs(want_yaw - self.gun_yaw) <= AIMED && v8math::abs(want_pitch - self.gun_pitch) <= AIMED;
        if span > 1e-3 && (settled || aim.dot(dir) > span * math::cos(AIMED)) {
            dir.sub_vectors(target, from);
            if dir.length_sq() > 1e-8 {
                dir.normalize();
            }
            // (Else `out.copy(along)`: along is out, which now holds the
            // short line to the target, unnormalized.)
        }
    }

    /// Turret.aimFrame (props.js:678): at the drum's pivot, looking along
    /// the aim, its up the drum's. Also the frame's direction and up, as
    /// cameraTurn reads them after.
    fn aim_frame(&mut self, yaw: f64, pitch: f64) -> (Matrix4, Vector3, Vector3) {
        self.update_rig();
        let world = self.rig.tree.node(self.rig.root).matrix_world;
        let mut point = self.desc.pivot;
        point.apply_matrix4(&world);
        let mut up = self.desc.up;
        up.transform_direction(&world);
        let dir = self.view_direction(yaw, pitch);
        let turn = look_basis(&dir, &up);
        let mut out = Matrix4::default();
        out.compose(&point, &turn, &ONE);
        (out, dir, up)
    }

    /// Turret.eyePoint (props.js:689): EyePointOffset in the aim frame.
    pub fn eye_point(&mut self, yaw: f64, pitch: f64) -> Vector3 {
        let (frame, ..) = self.aim_frame(yaw, pitch);
        let mut out = self.desc.camera_eye;
        out.apply_matrix4(&frame);
        out
    }

    /// Turret.cameraTurn (props.js:697): along the aim with the frame's up,
    /// pitched down by TiltValue; the aim frame with it.
    fn camera_turn(&mut self, yaw: f64, pitch: f64) -> (Quaternion, Matrix4) {
        let tilt = self.desc.camera_tilt;
        let (frame, dir, up) = self.aim_frame(yaw, pitch);
        let mut back = dir;
        back.negate();
        let mut out = look_basis(&back, &up);
        if super::truthy(tilt) {
            let mut t = Quaternion::default();
            t.set_from_axis_angle(&X_AXIS, -tilt);
            out.multiply(&t);
        }
        (out, frame)
    }

    /// Turret.cameraRest (props.js:708): TrackCenter in the aim frame, then
    /// TrackOffset in the camera's turn.
    fn camera_rest(&mut self, yaw: f64, pitch: f64) -> Vector3 {
        let (turn, frame) = self.camera_turn(yaw, pitch);
        let mut point = self.desc.camera_center;
        point.apply_matrix4(&frame);
        let mut out = self.desc.camera_offset;
        out.apply_quaternion(&turn);
        out.add(&point);
        out
    }

    /// Turret.aimOrigin (props.js:721): a person's aim starts at his eye in
    /// first person, else where the chase camera rests, brought forward
    /// along the aim to the eye's plane.
    pub fn aim_origin(&mut self, first_person: bool, yaw: f64, pitch: f64) -> Vector3 {
        if first_person {
            return self.eye_point(yaw, pitch);
        }
        let mut out = self.camera_rest(yaw, pitch);
        let mut eye = self.eye_point(yaw, pitch);
        let dir = self.view_direction(yaw, pitch);
        let d = eye.sub(&out).dot(&dir);
        out.add_scaled_vector(&dir, v8math::max(0.0, d));
        out
    }

    /// seats.js turretOrigin (740): a turret's gun pivot (the gun node, else
    /// its fire point, else the drum's bounding centre), what its gunner
    /// sees and fires from.
    pub fn origin(&mut self) -> Vector3 {
        match self.rig.gun.or(self.rig.fire_points.first().copied()) {
            Some(node) => {
                self.update_rig();
                let mut out = Vector3::default();
                self.rig.tree.get_world_position(node, &mut out);
                out
            }
            None => {
                let mut m = Matrix4::default();
                m.from_array(&self.desc.matrix, 0);
                let mut out = Vector3::new(0.0, 1.65, 0.32);
                out.apply_matrix4(&m);
                out
            }
        }
    }

    /// Turret.exposedSeat (props.js:905): rounds, swings and blasts meet its
    /// gunner: one shows, and the class leaves him shootable.
    pub fn exposed_seat(&self) -> bool {
        self.alive && self.desc.rider && self.desc.shootable
    }

    /// Turret.seatPoint (props.js:875): the seat node's place in the world.
    pub fn seat_point(&mut self) -> Vector3 {
        let node = self.rig.seat.unwrap_or(self.rig.root);
        let mut out = Vector3::default();
        self.rig.tree.get_world_position(node, &mut out);
        out
    }

    /// Turret.seatYaw (props.js:891): the heading of the seat node's turn.
    pub fn seat_yaw(&mut self) -> f64 {
        let node = self.rig.seat.unwrap_or(self.rig.root);
        self.rig.tree.update_world_matrix(node, true, false);
        let e = self.rig.tree.node(node).matrix_world.elements;
        math::atan2(e[8], e[10])
    }
}

/// seats.js turretAimPoint (759): along `direction` from the turret's aim
/// origin, the first wall, prop, hull or soldier (`exclude` left out, the
/// turret's own body too) within 512 m, else 512 m out.
pub fn turret_aim_point<W: SeatWorld + ?Sized>(w: &mut W, t: &mut TurretSide, first_person: bool, direction: &Vector3, collide: bool, exclude: Option<u32>) -> Vector3 {
    let (yaw, pitch) = (t.yaw, t.pitch);
    let from = t.aim_origin(first_person, yaw, pitch);
    let wall = if collide { w.cast(&from, direction, AIM_REACH, t.desc.body) } else { None };
    let mut far = match wall {
        Some(h) => h.distance,
        None => AIM_REACH,
    };
    if let Some(d) = w.raycast_units(&from, direction, far, exclude) {
        far = d;
    }
    let mut out = from;
    out.add_scaled_vector(direction, far);
    out
}

/// The turret crews (src/seats.js TurretSeats): their turrets in the
/// battle's order, the riders' facts and seat controls they read, and the
/// leases they made in a frame. Their leases, time and updates are the
/// store's (`turret_seats`).
#[derive(Clone, Debug, Default)]
pub struct Crews {
    pub turrets: Vec<TurretSide>,
    pub riders: BTreeMap<u32, RiderFacts>,
    pub controls: Controls,
    pub made: Vec<Ref<SeatLease>>,
    /// The seat controls whose intents the updates took edges from (as
    /// Hulls::latched).
    pub latched: Vec<Ref<SeatUnit>>,
    /// Whether the system has a collision world (`this.collision`).
    pub collide: bool,
}

impl Crews {
    fn turret_of(&self, t: Ref<Turret>) -> usize {
        self.turrets.iter().position(|x| x.turret == t).expect("the lease's turret")
    }

    fn facts(&mut self, unit: u32) -> &mut RiderFacts {
        self.riders.entry(unit).or_default()
    }

    /// TurretSeats.seatOf (820).
    pub fn seat_of(&self, st: &Store, unit: u32) -> Option<Ref<SeatLease>> {
        let r = st.unit.get(unit_ref(unit))?.seat?;
        let lease = st.seat_lease.get(r)?;
        (lease.turret.is_some() && st.turret_seats.leases.contains(&r)).then_some(r)
    }

    /// TurretSeats.canMan (828): a living soldier (not a droideka) on foot,
    /// a bot or a network human, a built turret nobody mans.
    pub fn can_man(&self, st: &Store, unit: u32, turret: usize) -> bool {
        let Some(u) = st.unit.get(unit_ref(unit)) else { return false };
        let t = &self.turrets[turret];
        let no_enter = self.riders.get(&unit).map_or(0.0, |f| f.no_enter);
        u.alive && !u.dead && (!u.human || network_human(u)) && u.seat.is_none() && !u.walker && no_enter == 0.0 && t.alive && t.gunner.is_none() && t.lease.is_none()
    }

    /// TurretSeats.man (834): a bot or a network human takes the turret (a
    /// person takes a bot's over: the bot is set down beside it first).
    pub fn man<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, unit: u32, turret: usize) -> bool {
        let network = st.unit.get(unit_ref(unit)).is_some_and(network_human);
        if network {
            if let Some(r) = self.turrets[turret].lease {
                let lease = st.seat_lease.get(r).unwrap();
                if !lease.released && !person_lease(lease, &st.unit) {
                    self.vacate(st, w, r, Let::default());
                }
            }
        }
        if !self.can_man(st, unit, turret) {
            return false;
        }
        let u = st.unit.get(unit_ref(unit)).unwrap();
        let tref = self.turrets[turret].turret;
        let lease = new_lease(Some((unit, u.generation)), false, u.team, None, Some(tref), 0.0, SeatRole::Gunner, st.turret_seats.time, st.turret_seats.ticks);
        let r = st.seat_lease.insert(lease);
        st.seats.leases.push(r);
        self.made.push(r);
        let t = &mut self.turrets[turret];
        t.lease = Some(r);
        t.gunner = Some(unit);
        t.yaw_vel = 0.0;
        t.pitch_vel = 0.0;
        w.seat_policy(Holder::Turret(self.turrets[turret].index), 0, network, true);
        if !network {
            let c = w.controllable(r, 0, SeatRole::Gunner);
            st.seat_lease.get_mut(r).unwrap().controllable = Some(c);
        }
        let mut combat = self.facts(unit).combat;
        let u = st.unit.get_mut(unit_ref(unit)).unwrap();
        u.seat = Some(r);
        seat_body(u, &mut combat);
        self.facts(unit).combat = combat;
        w.seated(unit);
        st.turret_seats.leases.push(r);
        self.pose(st, r);
        w.boarded(unit, Holder::Turret(self.turrets[turret].index), 0);
        true
    }

    /// TurretSeats.requestExit (855): his exit at the next update.
    pub fn request_exit(&mut self, st: &mut Store, unit: u32, post: Option<Ref<Post>>, placement: bool) -> bool {
        let Some(r) = self.seat_of(st, unit) else { return false };
        st.seat_lease.get_mut(r).unwrap().exit_request = Some(crate::gen::ExitRequest { post, promote: None, placement });
        true
    }

    /// TurretSeats.leave (863): out now.
    pub fn leave<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, unit: u32, placement: bool, post: Option<Ref<Post>>) -> bool {
        let Some(r) = self.seat_of(st, unit) else { return false };
        self.vacate(st, w, r, Let { placement, post, ..Let::default() });
        true
    }

    /// TurretSeats.vacate (875): out now. kill: the turret was destroyed
    /// under him and he dies; reason Killed: he died at the gun. Either way
    /// his body falls from where he stood.
    pub fn vacate<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, r: Ref<SeatLease>, how: Let) {
        let lease = st.seat_lease.get(r).unwrap();
        if lease.released {
            return;
        }
        let turret = self.turret_of(lease.turret.expect("a turret's lease"));
        let unit = lease.unit.map(|u| u.slot);
        let died = how.kill || how.reason == Reason::Killed;
        let has_body = unit.and_then(|u| st.unit.get(unit_ref(u))).is_some();
        let fall = if died && has_body { w.rider_drop(Holder::Turret(self.turrets[turret].index), 0, unit.unwrap()) } else { None };
        let mut yaw: Option<f64> = None;
        let mut spot = ExitSpot::default();
        if how.placement && !died && self.collide && has_body {
            let t = &self.turrets[turret];
            let (center, radius) = t.sphere;
            let facing = t.desc.facing + t.yaw;
            let view = Vector3::new(math::sin(facing), 0.0, math::cos(facing));
            let frame = ExitFrame { origin: t.desc.position, forward: view, up: UP, center, radius, body: t.desc.body };
            classic_seat_exit(w, &frame, &mut spot);
            yaw = Some(math::atan2(spot.forward.x, spot.forward.z));
        }
        st.seat_lease.get_mut(r).unwrap().released = true;
        let t = &mut self.turrets[turret];
        if t.lease == Some(r) {
            t.lease = None;
            t.gunner = None;
        }
        if let Some(i) = st.turret_seats.leases.iter().position(|x| *x == r) {
            st.turret_seats.leases.remove(i);
        }
        if let Some(id) = unit {
            let mut combat = self.facts(id).combat;
            let u = st.unit.get_mut(unit_ref(id)).unwrap();
            if u.seat == Some(r) {
                u.seat = None;
                unseat_body(u, &mut combat);
                // `unit.soldier?.aimYaw ?? 0`.
                let aim_yaw = super::bodies::soldier(u).map_or(0.0, |s| s.aim_yaw);
                self.facts(id).combat = combat;
                w.unseated(id, aim_yaw);
                if let Some(yaw) = yaw {
                    place_on_foot(w, st.unit.get_mut(unit_ref(id)).unwrap(), &spot.position, yaw, false);
                    w.placed(id, yaw);
                }
            } else {
                self.facts(id).combat = combat;
            }
        }
        if let Some(fall) = fall {
            drop_body(st.unit.get_mut(unit_ref(unit.unwrap())).unwrap(), &fall, None);
            w.dropped(unit.unwrap(), &fall);
        }
        if how.kill {
            if let Some(id) = unit {
                let u = st.unit.get(unit_ref(id)).unwrap();
                if u.alive && !u.combat.dead {
                    w.kill(st, super::Seating::Crews(self), id, how.attacker);
                }
            }
        }
        if !how.kill {
            w.unboarded(unit, Holder::Turret(self.turrets[turret].index), 0, how.post, how.reason);
        }
    }

    /// TurretSeats.destroyed (909): whoever mans a destroyed turret dies,
    /// credited to whoever destroyed it.
    pub fn destroyed<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, turret: usize) {
        let Some(r) = self.turrets[turret].lease else { return };
        if st.seat_lease.get(r).unwrap().human {
            return;
        }
        let attacker = self.turrets[turret].last_attacker;
        self.vacate(st, w, r, Let { placement: false, kill: true, attacker, ..Let::default() });
    }

    /// TurretSeats.update (915): each crew's batch: a destroyed turret's
    /// gunner dies, a stale lease is let go, an exit asked for is taken,
    /// else he drives.
    pub fn update<W: CrewWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, dt: f64) {
        st.turret_seats.time += dt;
        st.turret_seats.ticks += 1.0;
        for r in st.turret_seats.leases.clone() {
            let lease = st.seat_lease.get(r).unwrap();
            let turret = self.turret_of(lease.turret.unwrap());
            if !self.turrets[turret].alive {
                let attacker = self.turrets[turret].last_attacker;
                self.vacate(st, w, r, Let { placement: false, kill: true, attacker, ..Let::default() });
                continue;
            }
            if !live(lease, r, &st.unit) {
                self.vacate(st, w, r, Let { placement: false, ..Let::default() });
                continue;
            }
            if let Some(c) = latch_lease(&mut st.seat_lease, &mut st.unit, &mut self.controls, r) {
                self.latched.push(c);
            }
            let lease = st.seat_lease.get(r).unwrap();
            if lease.input.exit || lease.exit_request.is_some() {
                let (placement, post) = match &lease.exit_request {
                    Some(q) => (q.placement, q.post),
                    None => (true, None),
                };
                self.vacate(st, w, r, Let { placement, post, ..Let::default() });
                continue;
            }
            self.drive(st, w, r, dt);
        }
    }

    /// TurretSeats.drive (939): one frame of a bot or a network human at the
    /// gun: the drum, the gun, the body, the trigger.
    pub fn drive<W: CrewWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, r: Ref<SeatLease>, dt: f64) {
        let lease = st.seat_lease.get(r).unwrap();
        let turret = self.turret_of(lease.turret.unwrap());
        let unit = lease.unit.map(|u| u.slot);
        let person = unit.and_then(|u| st.unit.get(unit_ref(u))).is_some_and(network_human);
        let (turn, pitch, aiming, aim) = (lease.input.turn, lease.input.pitch, lease.input.aiming, lease.input.aim);
        let facts = unit.and_then(|u| self.riders.get(&u)).copied().unwrap_or_default();
        let zoom = if person { if super::truthy(facts.combat.zoom) { facts.combat.zoom } else { 1.0 } } else { 1.0 };
        let collide = self.collide;
        let index = self.turrets[turret].index;
        let t = &mut self.turrets[turret];
        // A positive turn analog turns right (yaw down), as for soldiers.
        t.steer(dt, -turn, pitch, zoom);
        t.present();
        t.update_rig();
        let mut muzzle = t.muzzle(false, None);
        let view = if aiming { Vector3::new(aim[0], aim[1], aim[2]) } else { t.view_direction(t.yaw, t.pitch) };
        // The gun follows what lies under the aim: a person's from where his
        // own page aims, a bot's from the gun's fire point.
        let target = if person {
            turret_aim_point(w, t, facts.combat.first_person, &view, collide, unit)
        } else {
            let hit = if collide { w.cast(&muzzle, &view, AIM_REACH, t.desc.body) } else { None };
            let mut target = muzzle;
            target.add_scaled_vector(&view, match hit {
                Some(h) => h.distance,
                None => AIM_REACH,
            });
            target
        };
        t.pose(dt, Some(&target));
        self.pose(st, r);
        let t = &mut self.turrets[turret];
        if !t.weapon {
            return;
        }
        let input = &mut st.seat_lease.get_mut(r).unwrap().input;
        let pressed = take(input, 0);
        let held = input.held[0] || pressed;
        if t.missile_lock {
            w.update_lock(index, dt, &muzzle, &view, r);
        }
        let reload = take_reload(&mut st.seat_lease.get_mut(r).unwrap().input);
        let shots = w.weapon_update(index, dt, held, pressed, reload);
        let mut view = view;
        for (k, shot) in shots.iter().enumerate() {
            let t = &mut self.turrets[turret];
            let mut gun_dir = Vector3::default();
            muzzle = t.muzzle(true, Some(&mut gun_dir));
            if t.rig.gun.is_some() {
                t.fire_direction(&muzzle, &target, &mut gun_dir);
            }
            if super::truthy(shot.yaw) {
                gun_dir.apply_axis_angle(&UP, shot.yaw);
            }
            if super::truthy(shot.pitch) {
                view.cross_vectors(&gun_dir, &UP).normalize();
                gun_dir.apply_axis_angle(&view, shot.pitch);
            }
            let barrel = t.barrel;
            let points = t.rig.fire_points.len();
            let round = w.fire(index, &muzzle, &gun_dir, unit, r, k);
            w.bind_round(round, index, (barrel - 1.0) % (if points == 0 { 1 } else { points }) as f64);
            w.sound(index, k);
            w.fired(index, k, unit, &muzzle, &gun_dir);
        }
    }

    /// TurretSeats.pose (984): the body at the drum's seat, facing along the
    /// gun (the seat node's turn), at rest. (How it is drawn there is the
    /// page's.)
    pub fn pose(&mut self, st: &mut Store, r: Ref<SeatLease>) {
        let lease = st.seat_lease.get(r).unwrap();
        let turret = self.turret_of(lease.turret.unwrap());
        let unit = lease.unit.expect("a crew lease's unit").slot;
        let t = &mut self.turrets[turret];
        let point = t.seat_point();
        let yaw = t.seat_yaw();
        let pitch = t.pitch;
        let u = st.unit.get_mut(unit_ref(unit)).unwrap();
        if let Some(s) = super::bodies::soldier_mut(u) {
            let p = super::a3(&point);
            s.position = p;
            s.previous = p;
            s.settled = p;
            s.velocity = [0.0, 0.0, 0.0];
            s.aim_yaw = yaw;
            s.aim_pitch = pitch;
        }
        u.tick_count += 1.0;
    }
}
