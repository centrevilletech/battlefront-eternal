//! The page's hull presentation (src/vehicles.js Vehicle.present, poseCrew,
//! applyPose, liftLegs, feelLegs, poseSeat, recoilBarrels, drawFlyer and
//! presentDying; src/flight/gear.js poseGearAt), batched over every hull
//! drawn this frame. Each hull's rig is a native tree of its root and bones
//! (the scene above it, as rust/sim's HullRig hangs it), registered once;
//! the frame reads each hull's replicated or predicted state from fixed
//! records and publishes its root, its bones and its aimers back.
//!
//! The host keeps the hull's own fields (its age, leg sides and aimers
//! arrive and leave as records), so the own predicted hull is one more
//! entry over the same rig. A walker's legs (and a lifting craft's gear
//! clock) feel the page's collision between the two calls: `place` puts
//! every root and writes one block of CastRay records, the host casts the
//! whole block in one call, and `pose` finishes the frame.
use crate::layout::{
    cast_ray as cr, vehicle_aimer_output as ao, vehicle_node as vn, vehicle_entry as ve,
    vehicle_entry_output as eo, vehicle_frame as vf, vehicle_header as vh, vehicle_limits as lim,
    vehicle_flight_state as fs, vehicle_mode as mode, vehicle_root as vr, vehicle_seat_input as si, vehicle_wheel_input as wi,
    status,
};
use bfe_pose::rig::math;
use bfe_pose::{Clip, Pose};
use serde::Deserialize;
use three::{clamp, lerp, EulerOrder, Matrix4, NodeId, Quaternion, TransformNode, TransformTree, Vector3};

const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const RIGHT: Vector3 = Vector3::new(1.0, 0.0, 0.0);
const NOSE: Vector3 = Vector3::new(0.0, 0.0, 1.0);
const PI: f64 = std::f64::consts::PI;
/// rad: a barrel this close to its target is on it (vehicles.js AIMED).
const AIMED: f64 = PI / 180.0;
/// s a dying walker takes at most from its last pose into its clip.
const DEATH_BLEND: f64 = 0.3;

/// vehicles.js `angle`: v wrapped into -pi..pi.
fn angle(v: f64) -> f64 {
    math::atan2(math::sin(v), math::cos(v))
}
/// vehicles.js `approach`.
fn approach(v: f64, target: f64, delta: f64) -> f64 {
    v + clamp(target - v, -delta, delta)
}
/// JavaScript's `x || 0` for a number.
fn or_zero(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        0.0
    } else {
        x
    }
}
/// vehicles.js aimVector.
pub(crate) fn aim_vector(yaw: f64, pitch: f64) -> Vector3 {
    Vector3::new(math::sin(yaw) * math::cos(pitch), math::sin(pitch), math::cos(yaw) * math::cos(pitch))
}
/// vehicles.js lookBasis: the turn whose +Z is forward, its up near `up`.
pub(crate) fn look_basis(forward: &Vector3, up: &Vector3) -> Quaternion {
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

/// A bone of the Vehicle constructor's rig as it stands when the hull is
/// first drawn: its parent (-1: the root), its local transform and its
/// bind (`bindQuaternions`, `bindPositions`: quaternion then position).
#[derive(Deserialize)]
pub struct BoneInput {
    /// The parent bone (-1: the root).
    pub parent: i32,
    /// `bone.position` as it stands.
    pub position: [f64; 3],
    /// `bone.quaternion` as it stands.
    pub quaternion: [f64; 4],
    /// `bone.scale`.
    pub scale: [f64; 3],
    /// Bind quaternion then bind position.
    pub bind: [f64; 7],
}
/// An aimer of a seat's weapon (the constructor's `aimers`): its bone, the
/// seat that turns that bone as its turret (`owner`, -1: none), its rates,
/// its limits in the port's terms and whether its arc wraps past the back.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AimerInput {
    /// The aimer's bone (-1: none, or not a bone).
    pub bone: i32,
    /// The seat whose turret this bone is (-1: none).
    pub owner: i32,
    /// AimerYawRate, rad/s.
    pub yaw_rate: f64,
    /// AimerPitchRate, rad/s.
    pub pitch_rate: f64,
    /// AimerYawLimits, rad.
    pub yaw_limits: [f64; 2],
    /// AimerPitchLimits, rad, up positive.
    pub pitch_limits: [f64; 2],
    /// An arc round the back, kept in 0..2pi.
    pub wrap_yaw: bool,
    /// Its angles as the hull holds them at registration; the phase keeps
    /// them from then on (nothing else on a page turns them).
    pub yaw: f64,
    pub pitch: f64,
}
/// A muzzle whose barrel kicks back (recoilBarrels: `barrel`, `rest`,
/// `recoil`).
#[derive(Deserialize)]
pub struct MuzzleInput {
    /// The barrel bone.
    pub barrel: i32,
    /// Where it rests (`m.rest`).
    pub rest: [f64; 3],
    /// BarrelRecoil (0: no kick).
    pub recoil: f64,
}
/// A seat as poseSeat reads it (seatView: the turret node, its up at rest
/// and the turn factors).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatInput {
    /// TurretNodeName's bone (-1: none).
    pub turret: i32,
    /// The turret's up at rest.
    pub up: [f64; 3],
    /// YawTurnFactor.
    pub yaw_factor: f64,
    /// PitchTurnFactor.
    pub pitch_factor: f64,
    /// Each weapon's aimers.
    pub weapons: Vec<Vec<AimerInput>>,
    /// Every muzzle with a barrel and its rest (recoilBarrels' own).
    pub muzzles: Vec<MuzzleInput>,
}
/// A walker's leg pairs (`def.fx.legs`): each foot's place in the hull's
/// frame and the rays' length (feelLegs).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegsInput {
    /// rayLength: each ray starts this far above the foot.
    pub ray_length: f64,
    /// Each pair's left and right foot in the hull's frame.
    pub at: Vec<[[f64; 3]; 2]>,
    /// Each pair's leg bones (liftLegs' legBones), in bone order.
    pub bones: Vec<Vec<usize>>,
}
/// The flight rules a craft's drawn gear follows its flight state by
/// (`flightConfig`: TakeoffTime, LandingTime, TakeoffHeight and the height
/// it rests at landed).
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct FlightInput {
    pub takeoff_time: f64,
    pub landing_time: f64,
    pub takeoff_height: f64,
    pub landed_height: f64,
    /// How far below its origin it stands on its gear (gearFoot), if known.
    #[serde(default)]
    pub gear_foot: Option<f64>,
}
impl FlightInput {
    /// heightAbove's lift: a craft whose foot is near or above its origin
    /// casts from 0.1 m over that foot.
    fn lift(&self) -> f64 {
        match self.gear_foot {
            Some(f) if f < 0.1 => 0.1 - f,
            _ => 0.0,
        }
    }
}
/// A clip handle and the JS clip's `duration`.
#[derive(Deserialize, Clone, Copy)]
pub struct ClipRef {
    /// From load_clip.
    pub handle: u32,
    /// `clip.duration`.
    pub duration: f64,
}
/// An object of the hull's tree that is neither its root nor a bone (a fire
/// point marker, the static bone, a mesh), in three's traverse order: its
/// parent (0 the root, 1.. the bones, then these) and its own transform.
#[derive(Deserialize)]
pub struct NodeInput {
    pub parent: i32,
    pub position: [f64; 3],
    pub quaternion: [f64; 4],
    pub scale: [f64; 3],
    /// A skinned mesh: its bind inverse follows its world matrix (three's
    /// attached bind mode).
    pub skinned: bool,
}

/// Everything the frame reads of a hull that does not change: its bones,
/// whether it has a walk pose (`this.pose`) and its root (DummyRoot), its
/// clips by role, its legs, seats and wheels, and its death clip's span.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigInput {
    /// The rig's bones, by index.
    pub bones: Vec<BoneInput>,
    /// The root's Euler order: true for 'YXZ'.
    pub yxz: bool,
    /// `Boolean(this.pose)`.
    pub has_pose: bool,
    /// The walk pose's DummyRoot (-1: none).
    pub pose_root: i32,
    /// A craft (drawFlyer, its gear).
    pub flying: bool,
    /// An animal: its legs feel nothing.
    pub creature: bool,
    /// The idle clip.
    pub idle: Option<ClipRef>,
    /// walk_leftfoot_rightfoot.
    pub left: Option<ClipRef>,
    /// walk_rightfoot_leftfoot.
    pub right: Option<ClipRef>,
    /// leftfoot_up.
    pub lift_left: Option<ClipRef>,
    /// rightfoot_up.
    pub lift_right: Option<ClipRef>,
    /// death01.
    pub death: Option<ClipRef>,
    /// The gear's takeoff clip.
    pub takeoff: Option<ClipRef>,
    /// The bone /^dummyroot$/i gear posing puts back, or -1.
    pub gear_root: i32,
    /// A craft's flight rules (None: its gear holds where it was).
    #[serde(default)]
    pub flight: Option<FlightInput>,
    /// `def.fx.legs`, None without.
    pub legs: Option<LegsInput>,
    /// Every seat, in order.
    pub seats: Vec<SeatInput>,
    /// Each wheel's bone and axle.
    pub wheels: Vec<(usize, [f64; 3])>,
    /// The rest of its tree (markers, meshes), whose matrices it publishes.
    pub nodes: Vec<NodeInput>,
    /// `def.fx.dying.span` (0: none).
    pub death_span: f64,
}

/// An aimer as the frame turns it, its angles kept here; the host reads
/// back what it shoots by (facing, onTarget, posed).
#[derive(Clone, Copy)]
struct Aimer {
    /// Its node, None for none.
    bone: Option<NodeId>,
    /// Its bone index (the bind turn it starts from).
    index: usize,
    /// The seat whose turret it is (-1: none).
    owner: i32,
    yaw_rate: f64,
    pitch_rate: f64,
    yaw_limits: [f64; 2],
    pitch_limits: [f64; 2],
    wrap_yaw: bool,
    /// Its turn (yaw, pitch), the phase's own.
    yaw: f64,
    pitch: f64,
}
/// A seat's turret and its weapons' aimers.
struct Seat {
    /// The turret node, None for none.
    turret: Option<NodeId>,
    /// Its up at rest.
    up: Vector3,
    yaw_factor: f64,
    pitch_factor: f64,
    /// Each weapon's aimers, as indexes into the rig's.
    weapons: Vec<std::ops::Range<usize>>,
}
/// A barrel and where it rests.
struct Muzzle {
    /// The barrel node.
    barrel: NodeId,
    /// Where it rests.
    rest: Vector3,
    /// How far it kicks back.
    recoil: f64,
}

/// The gear a hull drawn from its records shows (gear.js shownGear's view):
/// the life it belongs to, whether it follows the flight state, the state
/// last seen and how far down it has the gear (1 down, 0 stowed).
#[derive(Clone, Copy, Default)]
struct GearClock {
    valid: bool,
    generation: f64,
    follows: bool,
    state: f64,
    value: f64,
}

/// One hull's rig and what its frame keeps: the tree, its bind, its clips
/// and samplers, and the pose it was last drawn in alive (presentDying).
struct Rig {
    /// The root and bones, the scene above the root (an identity, as three
    /// multiplies it in), by bone index.
    tree: TransformTree,
    root: NodeId,
    bones: Vec<NodeId>,
    /// Every node published, in the host's order: the root, the bones,
    /// then the rest of the tree; and the skinned meshes among them.
    nodes: Vec<NodeId>,
    skinned: Vec<NodeId>,
    /// Each bone's bind turn and place (bindQuaternions, bindPositions).
    bind_q: Vec<Quaternion>,
    bind_p: Vec<Vector3>,
    /// A walk pose (clips and not a flyer); a flyer; an animal (no legs
    /// felt).
    has_pose: bool,
    flying: bool,
    creature: bool,
    /// The clips by role (IDLE..TAKEOFF).
    clips: [Option<ClipRef>; 7],
    /// DummyRoot for gear posing.
    gear_root: Option<usize>,
    /// A craft's flight rules, and each slot's gear clock.
    flight: Option<FlightInput>,
    gear_clock: [GearClock; lim::SLOTS],
    /// Leg pairs, a walker's.
    legs: Option<LegsInput>,
    /// The seats, then every aimer and kicking muzzle in order.
    seats: Vec<Seat>,
    aimers: Vec<Aimer>,
    muzzles: Vec<Muzzle>,
    /// Each wheel's node, bone index and axle.
    wheels: Vec<(NodeId, usize, Vector3)>,
    /// The death clip's span (s).
    death_span: f64,
    /// The samplers applyPose (and the death clip), liftLegs and poseGearAt
    /// keep, made at registration.
    walk: Pose,
    lift: Pose,
    gear: Pose,
    /// presentDying's pose last drawn alive, per slot (the hull and its
    /// predicted view each keep their own `deathFrom`).
    death_q: [Vec<Quaternion>; lim::SLOTS],
    death_t: [Vec<Vector3>; lim::SLOTS],
    death_taken: [Option<bool>; lim::SLOTS],
    /// Record offsets within an entry's input and output, and their sizes.
    input_len: usize,
    output_len: usize,
    /// The slots' first input and output words.
    slots: [(usize, usize); lim::SLOTS],
}
// The clips' roles: the idle, the two walk steps, the two lifted feet
// (liftLegs), the death and a craft's takeoff (its gear).
const IDLE: usize = 0;
const LEFT: usize = 1;
const RIGHT_CLIP: usize = 2;
const LIFT_LEFT: usize = 3;
const LIFT_RIGHT: usize = 4;
const DEATH: usize = 5;
const TAKEOFF: usize = 6;

impl Rig {
    /// The leg pairs (0 without legs).
    fn pairs(&self) -> usize {
        self.legs.as_ref().map_or(0, |l| l.at.len())
    }
    // Input: leg sides, the casts' heights (the phase's own, from the ray
    // block), seats, kicks, wheels.
    fn in_rays(&self) -> usize {
        self.pairs()
    }
    fn in_seats(&self) -> usize {
        self.in_rays() + self.pairs() * 2
    }
    fn in_aimers(&self) -> usize {
        self.in_seats() + self.seats.len() * si::STRIDE
    }
    fn in_kicks(&self) -> usize {
        self.in_aimers()
    }
    fn in_wheels(&self) -> usize {
        self.in_kicks() + self.muzzles.len()
    }
    // Output: the entry record, leg sides, aimers, every
    // node's matrices, the skinned meshes' bind inverses.
    fn out_sides(&self) -> usize {
        eo::STRIDE
    }
    fn out_aimers(&self) -> usize {
        self.out_sides() + self.pairs()
    }
    fn out_nodes(&self) -> usize {
        self.out_aimers() + self.aimers.len() * ao::STRIDE
    }
    fn out_skins(&self) -> usize {
        self.out_nodes() + self.nodes.len() * vn::STRIDE
    }
    /// A clip of a role, with its `duration`, or None (`this.clips.get`).
    fn clip<'a>(&self, clips: &'a [Clip], which: usize) -> Option<(&'a Clip, f64)> {
        let r = self.clips[which]?;
        clips.get(r.handle as usize).map(|c| (c, r.duration))
    }
}

/// The page's hull phase: loaded clips, registered rigs, and the records
/// the host writes and reads each frame.
pub struct Vehicles {
    pub network:crate::network::Bindings,
    clips: Vec<Clip>,
    rigs: Vec<Rig>,
    /// VehicleFrame, then up to `entries_capacity` VehicleEntry records.
    pub input: Vec<f64>,
    /// Each rig's slots' input and output words.
    pub stream_in: Vec<f64>,
    pub stream_out: Vec<f64>,
    pub header: Vec<f64>,
    /// The frame's CastRay block, and each ray's entry and what it feels
    /// (a foot's index, or GROUND); sized at registration for every entry.
    pub rays: Vec<f64>,
    route: Vec<(u32, u32)>,
    /// Each entry's ground height under a lifting craft (NaN: none cast).
    heights: Vec<f64>,
    /// The most rays one entry asks for.
    per_entry: usize,
}
/// A ray's route: a lifting craft's ground (heightAbove), not a foot.
const GROUND: u32 = u32::MAX;
/// heightAbove's reach and its answer for no ground within it.
const GROUND_REACH: f64 = 1024.0;

/// The frame's ray block as `place` fills it.
struct RayBlock<'a> {
    data: &'a mut [f64],
    route: &'a mut [(u32, u32)],
    count: usize,
}
impl RayBlock<'_> {
    /// One cast straight down from `from` over `far`, past `ignore` (the
    /// hull's own body slot, -1 none); false when the block is full.
    fn down(&mut self, from: &Vector3, far: f64, ignore: f64, entry: usize, index: u32) -> bool {
        if self.count >= self.route.len() {
            return false;
        }
        let x = &mut self.data[self.count * cr::STRIDE..(self.count + 1) * cr::STRIDE];
        x[cr::ORIGIN..cr::ORIGIN + 3].copy_from_slice(&[from.x, from.y, from.z]);
        // Math.hypot(0, 0) of the down direction picks the terrain grid.
        x[cr::DIRECTION..cr::DIRECTION + 3].copy_from_slice(&[0.0, -1.0, 0.0]);
        x[cr::FAR] = far;
        x[cr::HYPOT] = 0.0;
        x[cr::IGNORE] = ignore;
        x[cr::HIT] = 0.0;
        x[cr::DISTANCE] = 0.0;
        self.route[self.count] = (entry as u32, index);
        self.count += 1;
        true
    }
}

impl Default for Vehicles {
    fn default() -> Self {
        Self::new()
    }
}

impl Vehicles {
    pub fn new() -> Vehicles {
        Vehicles {
            network:Default::default(),
            clips: Vec::new(),
            rigs: Vec::new(),
            input: vec![0.0; vf::STRIDE],
            stream_in: Vec::new(),
            stream_out: Vec::new(),
            header: vec![0.0; vh::STRIDE],
            rays: Vec::new(),
            route: Vec::new(),
            heights: Vec::new(),
            per_entry: 1,
        }
    }
    /// Every rig twice (the hull and its predicted view).
    pub fn entries_capacity(&self) -> usize {
        self.rigs.len() * lim::SLOTS
    }
    /// A clip, loaded once (its frames for the sampler). Returns its handle.
    pub fn load_clip(&mut self, frames: usize, fps: f64, root_motion: bool, tracks: Vec<i32>, data: Vec<f32>) -> u32 {
        self.clips.push(Clip { frames, fps, root_motion, tracks, data });
        (self.clips.len() - 1) as u32
    }
    /// A hull's rig (the Vehicle constructor's tree as it stands now).
    /// Returns its id; `slot_input`/`slot_output` give its slots' words.
    pub fn add_rig(&mut self, d: RigInput) -> usize {
        let mut tree = TransformTree::new();
        let root = tree.add_node(TransformNode::default());
        if d.yxz {
            tree.node_mut(root).set_rotation_order(EulerOrder::YXZ);
        }
        // The Vehicle constructor's bones under the root, each at the transform
        // it has now (a barrel may stand kicked back, a walker mid-stride):
        // the frame starts every bone it poses from there or from bind.
        let mut bones = Vec::with_capacity(d.bones.len());
        let (mut bind_q, mut bind_p) = (Vec::new(), Vec::new());
        for b in &d.bones {
            let mut node = TransformNode::placed(Vector3::new(b.position[0], b.position[1], b.position[2]), Vector3::new(b.scale[0], b.scale[1], b.scale[2]));
            node.copy_quaternion(&Quaternion::new(b.quaternion[0], b.quaternion[1], b.quaternion[2], b.quaternion[3]));
            bones.push(tree.add_node(node));
            bind_q.push(Quaternion::new(b.bind[0], b.bind[1], b.bind[2], b.bind[3]));
            bind_p.push(Vector3::new(b.bind[4], b.bind[5], b.bind[6]));
        }
        for (i, b) in d.bones.iter().enumerate() {
            let parent = if b.parent >= 0 && (b.parent as usize) < bones.len() { bones[b.parent as usize] } else { root };
            tree.add_child(parent, bones[i]);
        }
        // The rest of the tree, each under its parent in the combined order.
        let mut nodes: Vec<NodeId> = std::iter::once(root).chain(bones.iter().copied()).collect();
        let mut skinned = Vec::new();
        for n in &d.nodes {
            let mut node = TransformNode::placed(Vector3::new(n.position[0], n.position[1], n.position[2]), Vector3::new(n.scale[0], n.scale[1], n.scale[2]));
            node.copy_quaternion(&Quaternion::new(n.quaternion[0], n.quaternion[1], n.quaternion[2], n.quaternion[3]));
            let id = tree.add_node(node);
            let parent = if n.parent >= 0 && (n.parent as usize) < nodes.len() { nodes[n.parent as usize] } else { root };
            tree.add_child(parent, id);
            nodes.push(id);
            if n.skinned {
                skinned.push(id);
            }
        }
        // The scene the root hangs from (an identity; see rust/sim HullRig).
        let scene = tree.add_node(TransformNode::default());
        tree.add_child(scene, root);
        // A bone index from the host, or None for one that is not a bone
        // (an aimer on a marker, a stale index): such a part poses nothing.
        let node = |i: i32| (i >= 0 && (i as usize) < bones.len()).then(|| bones[i as usize]);
        let mut seats = Vec::new();
        let (mut aimers, mut muzzles) = (Vec::new(), Vec::new());
        for s in &d.seats {
            let mut weapons = Vec::new();
            for w in &s.weapons {
            let start = aimers.len();
            for a in w {
                aimers.push(Aimer {
                    bone: node(a.bone),
                    index: a.bone.max(0) as usize,
                    owner: a.owner,
                    yaw_rate: a.yaw_rate,
                    pitch_rate: a.pitch_rate,
                    yaw_limits: a.yaw_limits,
                    pitch_limits: a.pitch_limits,
                    wrap_yaw: a.wrap_yaw,
                    yaw: a.yaw,
                    pitch: a.pitch,
                });
            }
            weapons.push(start..aimers.len());
            }
            for m in &s.muzzles {
                if let Some(barrel) = node(m.barrel) {
                    muzzles.push(Muzzle { barrel, rest: Vector3::new(m.rest[0], m.rest[1], m.rest[2]), recoil: m.recoil });
                }
            }
            seats.push(Seat {
                turret: node(s.turret),
                up: Vector3::new(s.up[0], s.up[1], s.up[2]),
                yaw_factor: s.yaw_factor,
                pitch_factor: s.pitch_factor,
                weapons,
            });
        }
        // Leg bone lists hold bones only, one list per pair.
        let count = bones.len();
        let mut legs = d.legs;
        if let Some(l) = legs.as_mut() {
            for list in &mut l.bones {
                list.retain(|&i| i < count);
            }
            l.bones.resize(l.at.len(), Vec::new());
        }
        let mut rig = Rig {
            tree,
            root,
            nodes,
            skinned,
            wheels: d.wheels.iter().filter(|(i, _)| *i < count).map(|&(i, a)| (bones[i], i, Vector3::new(a[0], a[1], a[2]))).collect(),
            bones,
            bind_q,
            bind_p,
            has_pose: d.has_pose,
            flying: d.flying,
            creature: d.creature,
            clips: [d.idle, d.left, d.right, d.lift_left, d.lift_right, d.death, d.takeoff],
            gear_root: (d.gear_root >= 0 && (d.gear_root as usize) < count).then_some(d.gear_root as usize),
            flight: d.flight,
            gear_clock: [GearClock::default(); lim::SLOTS],
            legs,
            seats,
            aimers,
            muzzles,
            death_span: d.death_span,
            walk: Pose::new(count, d.pose_root),
            lift: Pose::new(count, d.pose_root),
            gear: Pose::new(count, -1),
            death_q: std::array::from_fn(|_| vec![Quaternion::default(); count]),
            death_t: std::array::from_fn(|_| vec![Vector3::default(); count]),
            death_taken: [None; lim::SLOTS],
            input_len: 0,
            output_len: 0,
            slots: [(0, 0); lim::SLOTS],
        };
        // Two slots of records (the hull and its predicted view), sized to
        // this rig once: a frame writes into them and never grows them.
        rig.input_len = rig.in_wheels() + rig.wheels.len() * wi::STRIDE;
        rig.output_len = rig.out_skins() + rig.skinned.len() * 16;
        for slot in 0..lim::SLOTS {
            rig.slots[slot] = (self.stream_in.len(), self.stream_out.len());
            self.stream_in.resize(self.stream_in.len() + rig.input_len, 0.0);
            self.stream_out.resize(self.stream_out.len() + rig.output_len, 0.0);
        }
        // Each entry asks for at most its feet's rays and one ground ray.
        self.per_entry = self.per_entry.max(rig.pairs() * 2 + 1);
        self.rigs.push(rig);
        self.input.resize(vf::STRIDE + self.entries_capacity() * ve::STRIDE, 0.0);
        let rays = self.entries_capacity() * self.per_entry;
        self.rays.resize(rays * cr::STRIDE, 0.0);
        self.route.resize(rays, (0, 0));
        self.heights.resize(self.entries_capacity(), f64::NAN);
        self.rigs.len() - 1
    }
    /// A rig's slot's first input and output words (0 for no such rig).
    pub fn slot_input(&self, rig: usize, slot: usize) -> usize {
        self.rigs.get(rig).map_or(0, |r| r.slots[slot.min(lim::SLOTS - 1)].0)
    }
    pub fn slot_output(&self, rig: usize, slot: usize) -> usize {
        self.rigs.get(rig).map_or(0, |r| r.slots[slot.min(lim::SLOTS - 1)].1)
    }

    /// The entries the host wrote, clamped to the records; one whose rig or
    /// streams do not fit is skipped (counted in the header).
    fn entries(&self) -> usize {
        let n = self.input[vf::ENTRIES];
        if !(n > 0.0) {
            return 0;
        }
        (n as usize).min(self.entries_capacity())
    }
    fn entry(&self, e: usize) -> Option<(usize, usize, usize, usize)> {
        entry_of(&self.input, &self.rigs, e)
    }

    /// First call: every root placed, the bones back at bind, and the block
    /// of rays the frame asks for: each walker's feet (Vehicle.present up to
    /// feelLegs' casts) and the ground under each lifting craft whose gear
    /// follows its records (heightAbove).
    pub fn place(&mut self) {
        self.network.vehicles(&mut self.input);
        if self.input[vf::VERSION] != crate::layout::VERSION as f64 {
            self.header[vh::STATUS] = status::VERSION_MISMATCH as f64;
            return;
        }
        self.header[vh::STATUS] = status::OK as f64;
        let mut skipped = 0;
        let entries = self.entries();
        let mut block = RayBlock { data: &mut self.rays, route: &mut self.route, count: 0 };
        for e in 0..entries {
            let Some((rig, slot, _, o)) = entry_of(&self.input, &self.rigs, e) else {
                skipped += 1;
                continue;
            };
            // present() and render() (a spawn) place a root; a dying hull is
            // placed with its death pose, in the second call.
            let at = vf::STRIDE + e * ve::STRIDE;
            let x: &[f64] = &self.input[at..at + ve::STRIDE];
            let r = &mut self.rigs[rig];
            let out = &mut self.stream_out[o..o + r.output_len];
            out[eo::RAYS] = 0.0;
            if x[ve::DEFERRED]!=0. {continue;}
            let m = x[ve::MODE];
            if m == mode::ALIVE as f64 || m == mode::RENDER as f64 {
                place_alive(r, slot, x, out, &mut block, e);
                // heightAbove for the gear clock: from where the craft is (its
                // tick position) lifted over its foot, its own body left out.
                if let (Some(f), true) = (r.flight, wants_ground(r, x)) {
                    let lift = f.lift();
                    let from = Vector3::new(x[ve::GROUND_FROM], x[ve::GROUND_FROM + 1] + lift, x[ve::GROUND_FROM + 2]);
                    block.down(&from, GROUND_REACH + lift, x[ve::BODY], e, GROUND);
                }
            }
        }
        self.header[vh::RAYS] = block.count as f64;
        self.header[vh::SKIPPED] = skipped as f64;
    }

    /// Second call: the rest of each entry (the legs from the host's casts,
    /// the walk pose, gear, seats, barrels and wheels, or a dying walker),
    /// and every root, bone and aimer published.
    pub fn pose(&mut self) {
        if self.header[vh::STATUS] != status::OK as f64 {
            return;
        }
        // The host's casts: each foot's height L - distance (or -L for
        // nothing within 2L), into its slot's input; each lifting craft's
        // ground height, distance - lift (or 1024 for none).
        let entries = self.entries();
        self.heights[..entries].fill(f64::NAN);
        let rays = (self.header[vh::RAYS] as usize).min(self.route.len());
        for k in 0..rays {
            let (e, index) = self.route[k];
            let Some((rig, _, i, _)) = self.entry(e as usize) else { continue };
            let x = &self.rays[k * cr::STRIDE..(k + 1) * cr::STRIDE];
            let (hit, distance) = (x[cr::HIT] != 0.0, x[cr::DISTANCE]);
            let r = &self.rigs[rig];
            if index == GROUND {
                let lift = r.flight.map_or(0.0, |f| f.lift());
                self.heights[e as usize] = if hit { distance - lift } else { GROUND_REACH };
            } else if let Some(legs) = r.legs.as_ref() {
                let l = legs.ray_length;
                let at = i + r.in_rays() + index as usize;
                if at < i + r.input_len {
                    self.stream_in[at] = if hit { l - distance } else { -l };
                }
            }
        }
        for e in 0..entries {
            let Some((rig, slot, i, o)) = self.entry(e) else { continue };
            let at = vf::STRIDE + e * ve::STRIDE;
            let x: &[f64] = &self.input[at..at + ve::STRIDE];
            let r = &mut self.rigs[rig];
            let out = &mut self.stream_out[o..o + r.output_len];
            let sin = &self.stream_in[i..i + r.input_len];
            if x[ve::DEFERRED]!=0. {continue;}
            let m = x[ve::MODE];
            if m == mode::ALIVE as f64 || m == mode::RENDER as f64 {
                pose_alive(r, slot, &self.clips, x, sin, self.heights[e], out);
            } else if m == mode::DYING as f64 {
                pose_dying(r, slot, &self.clips, x, out);
            } else {
                continue;
            }
            // The offset still being smoothed away on his own hull, then every
            // world matrix from the root down (poseCrew's last word).
            let root = r.root;
            r.tree.node_mut(root).position.x += x[ve::OFFSET];
            r.tree.node_mut(root).position.y += x[ve::OFFSET + 1];
            r.tree.node_mut(root).position.z += x[ve::OFFSET + 2];
            r.tree.update_matrix_world(root, true);
            root_out(r, out);
            publish(r, out);
        }
    }
}


/// An entry's rig, slot and its slot's input and output words, or None for
/// one whose rig or offsets are not a registered slot's.
fn entry_of(input: &[f64], rigs: &[Rig], e: usize) -> Option<(usize, usize, usize, usize)> {
    let at = vf::STRIDE + e * ve::STRIDE;
    let x = &input[at..at + ve::STRIDE];
    let rig = x[ve::RIG];
    if !(rig >= 0.0) || rig as usize >= rigs.len() {
        return None;
    }
    let r = &rigs[rig as usize];
    let (i, o) = (x[ve::INPUT], x[ve::OUTPUT]);
    // Only a slot's own words: a stale or forged offset poses nothing.
    let slot = r.slots.iter().position(|&(si, so)| si as f64 == i && so as f64 == o)?;
    Some((rig as usize, slot, i as usize, o as usize))
}

/// A Vector3 or Quaternion from a record's words.
fn v3(x: &[f64]) -> Vector3 {
    Vector3::new(x[0], x[1], x[2])
}
fn q4(x: &[f64]) -> Quaternion {
    Quaternion::new(x[0], x[1], x[2], x[3])
}

/// Whether an entry's gear clock reads the ground this frame: a craft with
/// its takeoff clip whose gear follows its records, lifting off, with time.
fn wants_ground(r: &Rig, x: &[f64]) -> bool {
    r.flying && r.clips[TAKEOFF].is_some() && x[ve::GEAR_FOLLOW] > 0.0 && x[ve::FRAME_DT] > 0.0 && x[ve::FLIGHT_STATE] == fs::TAKING_OFF as f64
}

/// Vehicle.present to feelLegs' casts (or render(1, 0)'s placeRoot, for a
/// spawn: the previous tick's state in the tick fields, no time), its feet's
/// rays added to the block.
fn place_alive(r: &mut Rig, slot: usize, x: &[f64], out: &mut [f64], block: &mut RayBlock, entry: usize) {
    // render() (a spawn's pose) leaves presentDying's pose alone.
    if x[ve::MODE] == mode::ALIVE as f64 && r.death_taken[slot].is_some() {
        r.death_taken[slot] = Some(false);
    }
    // A spawn aims every aimer down its rest (Vehicle.spawn).
    if x[ve::MODE] == mode::RENDER as f64 {
        for a in &mut r.aimers {
            a.yaw = 0.0;
            a.pitch = 0.0;
        }
    }
    let alpha = x[ve::ALPHA];
    let network = x[ve::NETWORK] != 0.0;
    let hover = x[ve::HOVER] != 0.0;
    // this.age += dt, then the bob's phase from it.
    let age = x[ve::AGE] + x[ve::FRAME_DT];
    out[eo::AGE] = age;
    // Where its record's sample puts it (networkPose), else between the
    // two ticks it moved over (tickFrom to position by alpha).
    let position = v3(&x[ve::POSITION..]);
    let mut visual = Vector3::default();
    if network {
        visual = position;
    } else {
        visual.lerp_vectors(&v3(&x[ve::TICK_FROM..]), &position, alpha);
    }
    out[eo::VISUAL..eo::VISUAL + 3].copy_from_slice(&[visual.x, visual.y, visual.z]);
    // A repulsor's bob over its own phase, and its small roll wobble.
    let root = r.root;
    let phase = age * 1.7 + x[ve::HOVER_PHASE];
    {
        let n = r.tree.node_mut(root);
        n.position = visual;
        if hover {
            n.position.y += math::sin(phase) * 0.07 + math::sin(phase * 0.61) * 0.025;
        }
    }
    let wobble = if hover { math::sin(phase * 0.73) * 0.006 } else { 0.0 };
    // The root's turn: a craft's frame (drawFlyer); a ground hull's heading,
    // tilt and roll as Euler angles in YXZ order, exactly as present() sets
    // root.rotation (the host writes those same three angles back).
    let (tilt, roll) = (x[ve::TILT], x[ve::ROLL]);
    let flyer = |r: &mut Rig, frame: &Quaternion, bank: f64, out: &mut [f64]| {
        // drawFlyer: its frame, leaned about its nose by its bank.
        let mut lean = Quaternion::default();
        lean.set_from_axis_angle(&NOSE, or_zero(bank));
        r.tree.node_mut(r.root).with_quaternion(|q| {
            q.copy(frame).multiply(&lean);
        });
        out[eo::FRAME..eo::FRAME + 4].copy_from_slice(&[frame.x, frame.y, frame.z, frame.w]);
        out[eo::ROOT_MODE] = vr::QUATERNION as f64;
    };
    if network && r.flying {
        flyer(r, &q4(&x[ve::QUATERNION..]), roll, out);
    } else if network {
        // A replicated ground hull's quaternion is its heading alone.
        let q = q4(&x[ve::QUATERNION..]);
        let yaw = math::atan2(2.0 * (q.x * q.z + q.w * q.y), 1.0 - 2.0 * (q.y * q.y + q.z * q.z));
        r.tree.node_mut(root).set_rotation(-tilt, yaw, roll + wobble, Some(EulerOrder::YXZ));
        out[eo::ROOT_MODE] = vr::EULER as f64;
        // Between ticks: a craft's frame slerped, its bank lerped.
    } else if r.flying {
        let mut q = Quaternion::default();
        q.slerp_quaternions(&q4(&x[ve::TICK_QUATERNION..]), &q4(&x[ve::QUATERNION..]), alpha);
        flyer(r, &q, lerp(x[ve::TICK_ROLL], roll, alpha), out);
    } else {
        let (ty, yaw, tr) = (x[ve::TICK_YAW], x[ve::YAW], x[ve::TICK_ROLL]);
        r.tree.node_mut(root).set_rotation(
            -lerp(x[ve::TICK_TILT], tilt, alpha),
            ty + angle(yaw - ty) * alpha,
            tr + angle(roll - tr) * alpha + wobble,
            Some(EulerOrder::YXZ),
        );
        out[eo::ROOT_MODE] = vr::EULER as f64;
    }
    // poseCrew: every bone from bind (a walker's positions too).
    for k in 0..r.bones.len() {
        let bind = r.bind_q[k];
        r.tree.node_mut(r.bones[k]).copy_quaternion(&bind);
        if r.has_pose {
            r.tree.node_mut(r.bones[k]).position = r.bind_p[k];
        }
    }
    // The walk cycle as drawn between its last two ticks.
    let (mut phase, mut blend) = (x[ve::WALK_PHASE], x[ve::WALK_BLEND]);
    let length = walk_length(r);
    let from = x[ve::WALK_FROM_PHASE];
    if !from.is_nan() && alpha < 1.0 && length > 0.0 {
        let step = angle((phase - from) / length * 2.0 * PI) / (2.0 * PI) * length;
        phase = ((from + step * alpha) % length + length) % length;
        blend = lerp(x[ve::WALK_FROM_BLEND], blend, alpha);
    }
    out[eo::SHOWN_PHASE] = phase;
    out[eo::SHOWN_BLEND] = blend;
    // feelLegs: each foot's ray from the drawn root, for the host to cast.
    let dt = x[ve::DT];
    let Some(legs) = r.legs.as_ref() else { return };
    if r.creature || !(dt > 0.0) {
        return;
    }
    // The root's turn from its Euler angles (legEuler), as feelLegs takes it.
    let e = r.tree.node_mut(root).rotation();
    let mut q = Quaternion::default();
    q.set_from_euler(&three::Euler::new(e.x, e.y, e.z, EulerOrder::YXZ));
    let at = r.tree.node(root).position;
    // Each foot's ray starts L above it and runs 2L down, cast with the
    // hull's own body out of the world. A full block feels no feet this
    // frame (the legs keep their sides).
    let mut cast = 0;
    for (i, pair) in legs.at.iter().enumerate() {
        for (j, foot) in pair.iter().enumerate() {
            let mut from = Vector3::new(foot[0], foot[1], foot[2]);
            from.apply_quaternion(&q).add(&at);
            from.y += legs.ray_length;
            cast += block.down(&from, 2.0 * legs.ray_length, x[ve::BODY], entry, (i * 2 + j) as u32) as usize;
        }
    }
    if cast == legs.at.len() * 2 {
        out[eo::RAYS] = cast as f64;
    } else {
        // Drop a partial set: the routes past the block's count are unread.
        block.count -= cast;
    }
}

/// The walk cycle's length (both steps), 0 without one.
fn walk_length(r: &Rig) -> f64 {
    match (r.clips[LEFT], r.clips[RIGHT_CLIP]) {
        (Some(a), Some(b)) => a.duration + b.duration,
        _ => 0.0,
    }
}

/// Pose.add: nothing for a missing clip or a weight at or under 1e-4.
fn add(pose: &mut Pose, clip: Option<(&Clip, f64)>, time: f64, weight: f64, looping: bool) {
    let Some((c, _)) = clip else { return };
    if weight <= 1e-4 {
        return;
    }
    pose.add(c, time, weight, looping);
}
/// Pose.apply: each weighted bone's normalized turn and its translation.
fn apply(pose: &mut Pose, tree: &mut TransformTree, bones: &[NodeId]) {
    pose.apply_out();
    for (i, &bone) in bones.iter().enumerate() {
        if pose.w[i] as f64 <= 1e-6 {
            continue;
        }
        let o = &pose.out[i * 7..];
        let n = tree.node_mut(bone);
        n.set_quaternion(o[0], o[1], o[2], o[3]);
        n.position = Vector3::new(o[4], o[5], o[6]);
    }
}

/// Vehicle.present (or render) from feelLegs on: the legs, the walk pose, the gear,
/// the held seats, the barrels and the wheels.
fn pose_alive(r: &mut Rig, slot: usize, clips: &[Clip], x: &[f64], sin: &[f64], ground: f64, out: &mut [f64]) {
    let dt = x[ve::DT];
    let pairs = r.pairs();
    // feelLegs' sides, from the host's casts (L - distance, or -L).
    let mut sides = x[ve::LEG_SIDES] != 0.0;
    let rays = out[eo::RAYS] as usize;
    for i in 0..pairs {
        out[r.out_sides() + i] = if sides { sin[i] } else { 0.0 };
    }
    if rays > 0 {
        sides = true;
        for i in 0..pairs {
            let h0 = sin[r.in_rays() + i * 2];
            let h1 = sin[r.in_rays() + i * 2 + 1];
            let s = &mut out[r.out_sides() + i];
            *s += (clamp(-0.6 * (h0 - h1), -1.0, 1.0) - *s) * v8math::min(1.0, 0.4 * dt * 60.0);
        }
    }
    // The legs are lifted only once a side exists (`this.legSide`).
    out[eo::LEG_SIDES] = sides as u8 as f64;
    // applyPose: the idle at its age under the walk at its phase.
    if r.has_pose {
        let (phase, blend) = (out[eo::SHOWN_PHASE], out[eo::SHOWN_BLEND]);
        // The phase and blend as drawn (place_alive), the idle at the age.
        let left = r.clip(clips, LEFT);
        let right = r.clip(clips, RIGHT_CLIP);
        let length = left.map_or(0.0, |c| or_zero(c.1)) + right.map_or(0.0, |c| or_zero(c.1));
        r.walk.clear();
        let idle = r.clip(clips, IDLE);
        add(&mut r.walk, idle, out[eo::AGE], 1.0 - blend, true);
        // The step the phase is in: the left foot's first, then the right's.
        if let (true, Some(l), Some(rt), true) = (blend > 0.0001, left, right, length > 0.0) {
            let (c, t) = if phase < l.1 { (l, phase) } else { (rt, phase - l.1) };
            add(&mut r.walk, Some(c), t, blend, false);
        }
        apply(&mut r.walk, &mut r.tree, &r.bones);
        if sides {
            lift_legs(r, clips, out);
        }
    }
    // poseFlightGear: the takeoff clip at the drawn gear.
    if r.flying {
        if let Some((c, duration)) = r.clip(clips, TAKEOFF) {
            // The gear as drawn (shownGear): 1 down, 0 stowed; DummyRoot's
            // lift is put back to bind.
            let gear = shown_gear(r, slot, x, ground);
            out[eo::GEAR] = gear;
            r.gear.clear();
            r.gear.add(c, (1.0 - gear) * duration, 1.0, false);
            apply(&mut r.gear, &mut r.tree, &r.bones);
            if let Some(k) = r.gear_root {
                let n = r.tree.node_mut(r.bones[k]);
                n.position = r.bind_p[k];
                n.copy_quaternion(&r.bind_q[k]);
            }
        }
    }
    // poseSeat for each seat present() poses (held) or render() does (the
    // player's and each crewed one); the rest leave their aimers as the
    // host holds them.
    let base = r.in_seats();
    for s in 0..r.seats.len() {
        let seat = &sin[base + s * si::STRIDE..];
        if seat[si::POSE] != 0.0 {
            pose_seat(r, s, sin, dt, out);
        } else {
            for wk in 0..r.seats[s].weapons.len() {
                for a in r.seats[s].weapons[wk].clone() {
                    out[r.out_aimers() + a * ao::STRIDE + ao::WRITTEN] = 0.0;
                }
            }
        }
    }
    // recoilBarrels: each barrel back along its z by its kick.
    // The kick: 1 as it fires, decaying at 4 a second on the host's clock.
    let now = x[ve::NOW];
    let kicks = r.in_kicks();
    for (k, m) in r.muzzles.iter().enumerate() {
        let at = sin[kicks + k];
        let kick = if at.is_nan() { 0.0 } else { v8math::max(0.0, 1.0 - 4.0 * (now - at)) };
        let n = r.tree.node_mut(m.barrel);
        n.position = m.rest;
        if kick > 0.0 && m.recoil != 0.0 && !m.recoil.is_nan() {
            let mut axis = Vector3::new(0.0, 0.0, 1.0);
            axis.apply_quaternion(&n.quaternion);
            n.position.add_scaled_vector(&axis, -m.recoil * kick);
        }
    }
    // poseCrew's last word: every world matrix from the root down.
    let root = r.root;
    r.tree.update_matrix_world(root, true);
    // The wheels, from bind, after the crew pose.
    let network = x[ve::NETWORK] != 0.0;
    let alpha = x[ve::ALPHA];
    let wheels = r.in_wheels();
    for (k, &(bone, i, axis)) in r.wheels.iter().enumerate() {
        let w = &sin[wheels + k * wi::STRIDE..];
        let turn = if network { w[wi::PHASE] } else { lerp(w[wi::TICK_FROM], w[wi::PHASE], alpha) };
        let mut q = Quaternion::default();
        q.set_from_axis_angle(&axis, turn);
        let bind = r.bind_q[i];
        r.tree.node_mut(bone).with_quaternion(|b| {
            b.copy(&bind).multiply(&q);
        });
    }
}

/// gear.js `clamp01`.
fn clamp01(x: f64) -> f64 {
    v8math::min(1.0, v8math::max(0.0, x))
}
/// The gear a flight state rests at (gear.js `resting`): down on the ground
/// and through the lift, stowed in flight (a landing brings it down).
fn resting(state: f64) -> f64 {
    if state == fs::FLYING as f64 || state == fs::LANDING as f64 { 0.0 } else { 1.0 }
}

/// gear.js shownGear with followFlightState's view: how far down the gear is
/// drawn. The hull's own gear where this page steps or corrects its flight
/// (GEAR_FOLLOW -1, or 0 for his own craft); else its replicated flight
/// state's, advanced by the frame's dt: held down through the lift, stowed
/// over TakeoffTime once the craft is TakeoffHeight above where it rests,
/// down over LandingTime from the start of a landing (`ground`: this
/// frame's cast under a lifting craft). The host says which
/// life its records follow (GEAR_GENERATION); a new one starts from its
/// state, a handover from a replay from where the replay left the gear.
fn shown_gear(r: &mut Rig, slot: usize, x: &[f64], ground: f64) -> f64 {
    // `v.gear ?? 1` (the host writes NaN for none).
    let own = x[ve::GEAR];
    let own_or = |fallback: f64| if own.is_nan() { fallback } else { own };
    let follow = x[ve::GEAR_FOLLOW];
    if !(follow >= 0.0) {
        return clamp01(own_or(1.0));
    }
    let (generation, state, follows) = (x[ve::GEAR_GENERATION], x[ve::FLIGHT_STATE], follow > 0.0);
    let c = &mut r.gear_clock[slot];
    // followFlightState: a new life's view, or the switch to the records.
    if !c.valid || c.generation != generation {
        let value = if follows { resting(state) } else { clamp01(own_or(1.0)) };
        *c = GearClock { valid: true, generation, follows, state, value };
    } else {
        if follows && !c.follows {
            c.state = state;
            c.value = clamp01(own_or(c.value));
        }
        c.follows = follows;
    }
    if !c.follows {
        return clamp01(own_or(1.0));
    }
    let Some(f) = r.flight else { return c.value };
    c.state = state;
    let dt = x[ve::FRAME_DT];
    let stow = dt / v8math::max(f.takeoff_time, 1e-3);
    let lower = dt / v8math::max(f.landing_time, 1e-3);
    if state == fs::LANDED as f64 || state == fs::LANDING as f64 {
        c.value = v8math::min(1.0, c.value + lower);
    } else if state == fs::TAKING_OFF as f64 {
        // The ground height under the craft (heightAbove, this frame's
        // cast), read only once the lift has time.
        if dt > 0.0 && ground > f.takeoff_height + f.landed_height {
            c.value = v8math::max(0.0, c.value - stow);
        }
    } else if state == fs::FLYING as f64 {
        c.value = v8math::max(0.0, c.value - stow);
    }
    c.value
}

/// The root as the host writes it back.
fn root_out(r: &mut Rig, out: &mut [f64]) {
    let root = r.root;
    let p = r.tree.node(root).position;
    out[eo::POSITION..eo::POSITION + 3].copy_from_slice(&[p.x, p.y, p.z]);
    let q = r.tree.node(root).quaternion;
    out[eo::QUATERNION..eo::QUATERNION + 4].copy_from_slice(&[q.x, q.y, q.z, q.w]);
    if out[eo::ROOT_MODE] == vr::EULER as f64 {
        let e = r.tree.node_mut(root).rotation();
        out[eo::ROTATION..eo::ROTATION + 3].copy_from_slice(&[e.x, e.y, e.z]);
    }
}

/// Every node's local and world matrix, and each skinned mesh's bind
/// inverse (three's attached mode: the inverse of its world matrix), as
/// the host copies them into three without walking the tree.
fn publish(r: &Rig, out: &mut [f64]) {
    let base = r.out_nodes();
    for (k, &n) in r.nodes.iter().enumerate() {
        let node = r.tree.node(n);
        let o = base + k * vn::STRIDE;
        out[o + vn::MATRIX..o + vn::MATRIX + 16].copy_from_slice(&node.matrix.elements);
        out[o + vn::WORLD..o + vn::WORLD + 16].copy_from_slice(&node.matrix_world.elements);
    }
    let base = r.out_skins();
    for (k, &n) in r.skinned.iter().enumerate() {
        let mut inverse = r.tree.node(n).matrix_world;
        inverse.invert();
        out[base + k * 16..base + (k + 1) * 16].copy_from_slice(&inverse.elements);
    }
}

/// liftLegs: each pair's legs drawn toward the higher foot's lifted pose.
fn lift_legs(r: &mut Rig, clips: &[Clip], out: &[f64]) {
    let (Some(up_left), Some(up_right)) = (r.clip(clips, LIFT_LEFT), r.clip(clips, LIFT_RIGHT)) else { return };
    let Some(legs) = r.legs.as_ref() else { return };
    for (n, bones) in legs.bones.iter().enumerate() {
        // `sides[n] || 0`: how much higher one foot's ground is, as weight.
        let side = or_zero(out[r.out_sides() + n]);
        let weight = v8math::min(1.0, side.abs());
        if weight < 1e-3 {
            continue;
        }
        // The left foot's lifted pose when its ground is higher, else the
        // right's, at its first frame.
        r.lift.clear();
        r.lift.add(if side < 0.0 { up_left.0 } else { up_right.0 }, 0.0, 1.0, false);
        let p = &r.lift;
        for &i in bones {
            let w = p.w[i] as f64;
            if w <= 1e-6 {
                continue;
            }
            let mut q = Quaternion::new(p.q[i * 4] as f64, p.q[i * 4 + 1] as f64, p.q[i * 4 + 2] as f64, p.q[i * 4 + 3] as f64);
            q.normalize();
            let target = Vector3::new(p.t[i * 3] as f64 / w, p.t[i * 3 + 1] as f64 / w, p.t[i * 3 + 2] as f64 / w);
            // three's slerp and lerp toward it (the Euler synced as three
            // syncs it).
            let bone = r.tree.node_mut(r.bones[i]);
            bone.slerp_quaternion(&q, weight);
            bone.position.lerp(&target, weight);
        }
    }
}

/// poseSeat: a held seat's turret swung to its aim, and its aimers turned
/// toward its target at their rates (a turn only with time, dt > 0).
fn pose_seat(r: &mut Rig, s: usize, sin: &[f64], dt: f64, out: &mut [f64]) {
    let seat_at = r.in_seats() + s * si::STRIDE;
    let seat = &sin[seat_at..seat_at + si::STRIDE];
    // The seat's aim and the point its guns turn to (seat.target).
    let (aim_yaw, pitch) = (seat[si::AIM_YAW], seat[si::PITCH]);
    let target = (seat[si::TARGET] != 0.0).then(|| v3(&seat[si::TARGET_POINT..]));
    let root = r.root;
    let turret = r.seats[s].turret;
    // The turret node: the aim from the hull's heading by the turn factors,
    // in its parent's frame, keeping its own up (lookBasis).
    if let Some(turret) = turret {
        let root_yaw = r.tree.node_mut(root).rotation().y;
        let sp = &r.seats[s];
        let q2 = look_basis(&aim_vector(sp.yaw_factor * angle(aim_yaw - root_yaw), sp.pitch_factor * pitch), &sp.up);
        let parent = r.tree.node(turret).parent.expect("a turret's parent");
        let mut q = Quaternion::default();
        r.tree.get_world_quaternion(parent, &mut q);
        q.invert();
        let root_q = r.tree.node(root).quaternion;
        q.multiply(&root_q).multiply(&q2);
        r.tree.node_mut(turret).copy_quaternion(&q);
        r.tree.update_matrix_world(turret, true);
    }
    // Whether someone holds a seat (Vehicle.held), for the aimers another
    // seat turns as its turret.
    let (seats, seat_base) = (r.seats.len(), r.in_seats());
    let held = |owner: i32| -> bool { owner >= 0 && (owner as usize) < seats && sin[seat_base + owner as usize * si::STRIDE + si::HELD] != 0.0 };
    let out_aimers = r.out_aimers();
    for wk in 0..r.seats[s].weapons.len() {
    for k in r.seats[s].weapons[wk].clone() {
        let a = r.aimers[k];
        let o = out_aimers + k * ao::STRIDE;
        out[o + ao::WRITTEN] = 1.0;
        out[o + ao::ON_TARGET] = 0.0;
        out[o + ao::POSED] = -1.0;
        // No bone: nothing turns (NaN facing: the host keeps its own).
        let Some(bone) = a.bone else {
            out[o + ao::FACING] = f64::NAN;
            continue;
        };
        // The seat's own turret node: it faces where the turret does.
        if Some(bone) == turret {
            let m = r.tree.node(bone).matrix_world;
            let mut f = Vector3::new(0.0, 0.0, 1.0);
            f.transform_direction(&m);
            out[o + ao::FACING..o + ao::FACING + 3].copy_from_slice(&[f.x, f.y, f.z]);
            continue;
        }
        // From its rest pose, in its parent's world frame as three
        // refreshes it from the root down.
        let rest = r.bind_q[a.index];
        let parent = r.tree.node(bone).parent.expect("an aimer's parent");
        r.tree.update_world_matrix(parent, true, false);
        let parent_world = r.tree.node(parent).matrix_world;
        let bone_position = r.tree.node(bone).position;
        let (mut yaw_now, mut pitch_now) = (a.yaw, a.pitch);
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
            // The shorter way round at its rate, then held inside its limits
            // (0..2pi for an arc round the back).
            let step = a.yaw_rate * dt;
            let mut yaw = yaw_now + clamp(angle(math::atan2(v.x, v.z) - yaw_now), -step, step);
            if a.wrap_yaw && yaw < 0.0 {
                yaw += 2.0 * PI;
            }
            let p = approach(pitch_now, math::atan2(v.y, v8math::hypot(v.x, v.z)), a.pitch_rate * dt);
            let ([y0, y1], [p0, p1]) = (a.yaw_limits, a.pitch_limits);
            yaw_now = clamp(yaw, y0, y1);
            pitch_now = clamp(p, p0, p1);
            let state = &mut r.aimers[k];
            state.yaw = yaw_now;
            state.pitch = pitch_now;
        }
        // Yaw about the node's up, then pitch about its side.
        let mut turn = rest;
        let mut q = Quaternion::default();
        q.set_from_axis_angle(&UP, yaw_now);
        turn.multiply(&q);
        let mut q2 = Quaternion::default();
        q2.set_from_axis_angle(&RIGHT, -pitch_now);
        turn.multiply(&q2);
        let parent_world = r.tree.node(parent).matrix_world;
        let mut f = Vector3::new(0.0, 0.0, 1.0);
        f.apply_quaternion(&turn).transform_direction(&parent_world);
        out[o + ao::FACING..o + ao::FACING + 3].copy_from_slice(&[f.x, f.y, f.z]);
        // A node another seat holds as its turret is that seat's to pose.
        let posed = a.owner < 0 || !held(a.owner);
        out[o + ao::POSED] = posed as u8 as f64;
        if posed {
            r.tree.node_mut(bone).copy_quaternion(&turn);
            r.tree.update_matrix_world(bone, true);
        }
    }
    // On target: pointing at it from its pivot (the weapon's aimers, once
    // all of them have turned).
    let Some(target) = target else { continue };
    for k in r.seats[s].weapons[wk].clone() {
        let Some(bone) = r.aimers[k].bone else { continue };
        let o = out_aimers + k * ao::STRIDE;
        let mut v = Vector3::default();
        r.tree.get_world_position(bone, &mut v);
        v.sub(&target).negate();
        let span = v.length();
        let facing = v3(&out[o + ao::FACING..]);
        out[o + ao::ON_TARGET] = (span > 1e-3 && v.dot(&facing) > span * math::cos(AIMED)) as u8 as f64;
    }
    }
}

/// presentDying: where it was destroyed, its bones in its death clip at
/// the time it has been dying, eased in from its last pose alive.
fn pose_dying(r: &mut Rig, slot: usize, clips: &[Clip], x: &[f64], out: &mut [f64]) {
    let root = r.root;
    // Where it was destroyed; a ground hull at its last heading and lean.
    r.tree.node_mut(root).position = v3(&x[ve::POSITION..]);
    out[eo::ROOT_MODE] = vr::POSITION as f64;
    if !r.flying {
        r.tree.node_mut(root).set_rotation(-x[ve::TILT], x[ve::YAW], x[ve::ROLL], Some(EulerOrder::YXZ));
        out[eo::ROOT_MODE] = vr::EULER as f64;
    }
    if r.has_pose {
        // The pose it was last drawn in alive, taken once per death.
        if r.death_taken[slot] != Some(true) {
            r.death_taken[slot] = Some(true);
            for (k, &bone) in r.bones.iter().enumerate() {
                let n = r.tree.node(bone);
                r.death_q[slot][k] = n.quaternion;
                r.death_t[slot][k] = n.position;
            }
        }
        for k in 0..r.bones.len() {
            let (q, p) = (r.bind_q[k], r.bind_p[k]);
            let n = r.tree.node_mut(r.bones[k]);
            n.copy_quaternion(&q);
            n.position = p;
        }
        // From bind, its death clip at the time it has been dying (at its
        // AnimationSpeed: frames over span), holding the last frame.
        let span = r.death_span;
        let dying = x[ve::DYING];
        if let Some((c, _)) = r.clip(clips, DEATH) {
            if span > 0.0 {
                r.walk.clear();
                r.walk.add(c, (span - dying) * (c.frames as f64 / c.fps / span), 1.0, false);
                apply(&mut r.walk, &mut r.tree, &r.bones);
            }
        }
        // Over its first DEATH_BLEND s (an eighth of a shorter clip) the clip
        // takes over from that pose, smoothstepped.
        let blend = if span > 0.0 {
            v8math::min(v8math::max((span - dying) / v8math::min(DEATH_BLEND, span / 8.0), 0.0), 1.0)
        } else {
            1.0
        };
        if blend < 1.0 {
            let w = blend * blend * (3.0 - 2.0 * blend);
            for k in 0..r.bones.len() {
                let (from_q, from_t) = (r.death_q[slot][k], r.death_t[slot][k]);
                let n = r.tree.node_mut(r.bones[k]);
                let mut q = from_q;
                q.slerp(&n.quaternion, w);
                n.copy_quaternion(&q);
                let p = n.position;
                n.position.lerp_vectors(&from_t, &p, w);
            }
        }
    }
    r.tree.update_matrix_world(root, true);
}
