//! A soldier's movement (src/soldier.js Soldier.tick) in the ray kernel's
//! module, for the world's ticks (the world worker and the dedicated server)
//! and the page's replay of its own soldier alike: src/movement-kernel.js
//! copies a soldier's state in, runs the tick here against the module's own
//! copies of the collision world, and copies the state back out, the same to
//! the last bit as the JavaScript tick (soldier.rs says how).
//!
//! A tick is one call. The sines and cosines it needs (of the aim's yaw
//! before the tick's turn and after it, and a hovering jet's pitch) are
//! computed here with `v8math`, as the production server's V8 computes
//! Math.sin and Math.cos: the same as the JavaScript tick's under Node, and
//! the server's where a client's own V8 parts from it in the last bit.

#[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
pub(crate) mod native;
pub mod scene;
pub mod soldier;

use crate::collision::spheres::{GatherCache, Gathered};
use crate::collision::MAX_BODIES;
use scene::{GroundCache, Scene};
use soldier::{Air, Input, Mover, Posture, Trig, PART_CACHES, V3};

// The exchange slots (src/movement-kernel.js mirrors them).
pub const POSITION: usize = 0;
pub const SETTLED: usize = 3;
pub const VELOCITY: usize = 6;
pub const NORMAL: usize = 9;
pub const AVOIDANCE: usize = 12;
pub const YAW: usize = 15;
pub const PITCH: usize = 16;
pub const STATE_TIME: usize = 17;
pub const LOCK: usize = 18;
pub const FUEL: usize = 19;
pub const DIVE: usize = 20;
pub const POSTURE: usize = 21;
pub const AIR: usize = 22;
pub const DIVE_FROM: usize = 23;
pub const GROUNDED: usize = 24;
pub const GROUND_ADJUSTMENT: usize = 25;
pub const DAMAGE: usize = 26;
pub const MAX_FLY: usize = 27;
pub const GETUP: usize = 28;
pub const DIVE_TIME: usize = 29;
pub const PASS_THROUGH: usize = 30;
pub const DT: usize = 31;
pub const FORWARD: usize = 32;
pub const RIGHT: usize = 33;
pub const TURN: usize = 34;
pub const PITCH_STICK: usize = 35;
pub const JUMP: usize = 36;
pub const CROUCH: usize = 37;
pub const PRONE: usize = 38;
pub const JET_AIMED: usize = 39;
pub const JET_DIRECTION: usize = 40;
/// mover_new's class numbers: MaxSpeed, MaxStrafeSpeed, MaxJumpSpeed,
/// MaxTurnSpeed, MaxPitchSpeed, Acceleraton, the pitch limits (2), the jet's
/// jump, push, recharge, cost, initial, minBorder and hover, and the
/// collision rule's threshold (3) and scale (3).
pub const CLASS: usize = 50;
pub const IO_LEN: usize = 80;

#[derive(Default)]
struct Movers {
    list: Vec<Mover>,
    caches: Vec<[GatherCache; PART_CACHES]>,
    gathered: Gathered,
    ground: GroundCache,
}

/// Soldier.tick's movers and scratch share a lifetime, while every call
/// receives collision explicitly. The retained banks preserve cache stamps
/// and lastGround between the same sequential movement operations.
pub struct MovementOwner {
    movers: Option<Movers>,
    #[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
    native_copies: native::Copies,
    io: [f64; IO_LEN],
    /// Body eligibility follows collision-world order, as in movement-kernel.js.
    flags: [u8; MAX_BODIES],
}
impl MovementOwner {
    pub const fn new() -> Self { Self { movers: None,
        #[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
        native_copies: native::Copies::new(), io: [0.; IO_LEN], flags: [0; MAX_BODIES] } }
    // Pointer discovery cannot allocate: the host may already have read the WASM
    // memory buffer before calling either pointer export. Pools initialize only
    // when a mover is constructed, where the host refreshes its views afterward.
    pub fn io_mut(&mut self) -> &mut [f64; IO_LEN] { &mut self.io }
    pub fn flags_mut(&mut self) -> &mut [u8; MAX_BODIES] { &mut self.flags }
}
impl Default for MovementOwner { fn default() -> Self { Self::new() } }
fn v3(v: &[f64; IO_LEN], at: usize) -> V3 {
    V3::new(v[at], v[at + 1], v[at + 2])
}

fn put(v: &mut [f64; IO_LEN], at: usize, value: V3) {
    v[at] = value.x;
    v[at + 1] = value.y;
    v[at + 2] = value.z;
}

/// The soldier's state from the exchange slots; false if a code is unknown.
pub(crate) fn load(m: &mut Mover, v: &[f64; IO_LEN]) -> bool {
    let (Some(posture), Some(air), Some(dive_from)) = (Posture::from_code(v[POSTURE]), Air::from_code(v[AIR]), Posture::from_code(v[DIVE_FROM])) else {
        return false;
    };
    m.position = v3(v, POSITION);
    m.settled = v3(v, SETTLED);
    m.velocity = v3(v, VELOCITY);
    m.normal = v3(v, NORMAL);
    m.avoidance = v3(v, AVOIDANCE);
    m.aim_yaw = v[YAW];
    m.aim_pitch = v[PITCH];
    m.state_time = v[STATE_TIME];
    m.lock = v[LOCK];
    m.fuel = v[FUEL];
    m.dive = v[DIVE];
    m.posture = posture;
    m.air = air;
    m.dive_from = dive_from;
    m.grounded = v[GROUNDED] != 0.0;
    m.ground_adjustment = v[GROUND_ADJUSTMENT];
    m.max_fly_height = v[MAX_FLY];
    m.getup_time = v[GETUP];
    m.dive_time = v[DIVE_TIME];
    m.pass_through = v[PASS_THROUGH] as i64;
    true
}

pub(crate) fn store(m: &Mover, v: &mut [f64; IO_LEN]) {
    put(v, POSITION, m.position);
    put(v, SETTLED, m.settled);
    put(v, VELOCITY, m.velocity);
    put(v, NORMAL, m.normal);
    put(v, AVOIDANCE, m.avoidance);
    v[YAW] = m.aim_yaw;
    v[PITCH] = m.aim_pitch;
    v[STATE_TIME] = m.state_time;
    v[LOCK] = m.lock;
    v[FUEL] = m.fuel;
    v[DIVE] = m.dive;
    v[POSTURE] = m.posture as i32 as f64;
    v[AIR] = m.air as i32 as f64;
    v[DIVE_FROM] = m.dive_from as i32 as f64;
    v[GROUNDED] = if m.grounded { 1.0 } else { 0.0 };
    v[GROUND_ADJUSTMENT] = m.ground_adjustment;
    v[DAMAGE] = m.collision_damage;
}

pub(crate) fn input(v: &[f64; IO_LEN]) -> Input {
    Input {
        forward: v[FORWARD],
        right: v[RIGHT],
        turn: v[TURN],
        pitch: v[PITCH_STICK],
        jump: v[JUMP] != 0.0,
        crouch: v[CROUCH] != 0.0,
        prone: v[PRONE] != 0.0,
        jet_direction: if v[JET_AIMED] != 0.0 { Some(v3(v, JET_DIRECTION)) } else { None },
    }
}

/// Soldier.tick using an explicit mover and collision scene. Yaw sines and
/// cosines match Math.sin/Math.cos before and after the turn, including the
/// jump, jetpack kick, body spheres and hovering pitch.
pub(crate) fn advance_in(m: &mut Mover, input: &Input, dt: f64, scene: &mut Scene) {
    let yaw = m.aim_yaw;
    let mut trig = Trig { sin0: v8math::sin(yaw), cos0: v8math::cos(yaw), sin_pitch: 0.0, cos_pitch: 0.0, sin1: 0.0, cos1: 0.0 };
    // jetTick's lift and climb read its pitch (a hovering jet's only).
    if m.air == Air::Jet && m.jet.hover {
        trig.sin_pitch = v8math::sin(m.aim_pitch);
        trig.cos_pitch = v8math::cos(m.aim_pitch);
    }
    let rest = m.begin(input, dt, &trig);
    if m.aim_yaw.to_bits() != yaw.to_bits() {
        trig.sin1 = v8math::sin(m.aim_yaw);
        trig.cos1 = v8math::cos(m.aim_yaw);
    } else {
        trig.sin1 = trig.sin0;
        trig.cos1 = trig.cos0;
    }
    m.finish(rest, input, dt, &trig, scene);
}

/// The class assignments shared by legacy and owned construction.
pub(crate) fn set_class(m: &mut Mover, c: &[f64]) {
        m.max_speed = c[0];
        m.max_strafe = c[1];
        m.max_jump = c[2];
        m.max_turn = c[3];
        m.max_pitch = c[4];
        m.accel = c[5];
        m.pitch_limits = [c[6], c[7]];
        m.jet.jump = c[8];
        m.jet.push = c[9];
        m.jet.recharge = c[10];
        m.jet.cost = c[11];
        m.jet.initial = c[12];
        m.jet.min_border = c[13];
        m.jet.hover = c[14] != 0.0;
        m.threshold = [c[15], c[16], c[17]];
        m.scale = [c[18], c[19], c[20]];
}

impl MovementOwner {
    pub fn mover_new(&mut self, world: u32) -> u32 {
        let mut class = [0.; 21];
        class.copy_from_slice(&self.io[CLASS..CLASS + 21]);
        self.new_mover(world as usize, |m| set_class(m, &class))
    }
    pub fn advance(&mut self, collision: &mut crate::collision::State, id: u32, input: &Input, dt: f64) {
        let ms = self.movers.as_mut().expect("initialized movement owner");
        let m = &mut ms.list[id as usize];
        let world = m.world;
        let count = collision.worlds[world].order.len().min(MAX_BODIES);
        let mut scene = Scene { s: collision, world, flags: &self.flags[..count], gathered: &mut ms.gathered, caches: &mut ms.caches[id as usize], ground_cache: &mut ms.ground };
        advance_in(m, input, dt, &mut scene);
    }
    pub fn new_mover(&mut self, world: usize, class: impl FnOnce(&mut Mover)) -> u32 {
        let ms = self.movers.get_or_insert_with(Movers::default);
        let mut m = Mover::new(world);
        class(&mut m);
        ms.list.push(m);
        ms.caches.push(std::array::from_fn(|_| GatherCache::new(0.75)));
        (ms.list.len() - 1) as u32
    }
    pub fn mover(&mut self, id: u32) -> &mut Mover { &mut self.movers.as_mut().expect("initialized movement owner").list[id as usize] }
    pub fn reserve_mover(&mut self, id: u32, polygons: usize, triangles: usize, candidates: usize) {
        let ms = self.movers.as_mut().expect("initialized movement owner");
        ms.gathered.reserve(polygons, triangles);
        for cache in &mut ms.caches[id as usize] { cache.reserve(candidates, candidates, candidates); }
    }
    // Soldier.touchPolygons shares geometry-keyed storage across movers.
    // Body membership is cold; prepare polygon sizes before contact.
    pub fn prepare_world_gathers(&mut self, collision: &crate::collision::State, world: usize) {
        self.movers.get_or_insert_with(Movers::default).gathered.prepare_dense(collision, world, 16);
    }
    pub fn tick(&mut self, collision: &mut crate::collision::State, id: u32) -> u32 {
        let ms = self.movers.as_mut().expect("initialized movement owner");
        if !load(&mut ms.list[id as usize], &self.io) { return 2; }
        let input = input(&self.io);
        self.advance(collision, id, &input, self.io[DT]);
        store(&self.movers.as_ref().expect("initialized movement owner").list[id as usize], &mut self.io);
        0
    }
}

#[cfg(target_arch = "wasm32")]
mod exports {
    // The module's exports, for src/movement-kernel.js.
    #[no_mangle]
    pub extern "C" fn movement_io_ptr() -> *mut f64 { crate::abi::facade::cold().movement.io_mut().as_mut_ptr() }
    #[no_mangle]
    pub extern "C" fn movement_flags_ptr() -> *mut u8 { crate::abi::facade::cold().movement.flags_mut().as_mut_ptr() }
    /// A soldier of world `world` with the class numbers in CLASS; returns its id.
    #[no_mangle]
    pub extern "C" fn mover_new(world: u32) -> u32 { crate::abi::facade::cold().movement.mover_new(world) }
    /// Soldier.tick for mover `id`: its state, input and dt in the slots. 0:
    /// done, the state back in the slots; 2: an unknown state code, rejected
    /// before changing the mover or advancing its collision scene.
    #[no_mangle]
    pub extern "C" fn mover_tick(id: u32) -> u32 { let w = crate::abi::facade::cold(); w.movement.tick(&mut w.collision.state, id) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{movement_io_ptr, movement_flags_ptr, mover_new, mover_tick};
