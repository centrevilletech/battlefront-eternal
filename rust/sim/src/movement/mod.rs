//! A soldier's movement (Soldier.tick) in the ray kernel's
//! module, for the world's ticks (the local world thread and the dedicated server)
//! and the client's replay of its own soldier alike: src/movement-kernel.js
//! copies a soldier's state in, runs the tick here against the module's own
//! copies of the collision world, and copies the state back out, the same to
//! the last bit as the JavaScript tick (soldier.rs says how).

#[cfg(test)]
mod golden;
pub(crate) mod native;
pub mod scene;
pub mod soldier;

use crate::collision::spheres::{GatherCache, Gathered};
use crate::collision::MAX_BODIES;
use bfe_family::GameFamily;
use scene::{GroundCache, Scene};
use soldier::swbf2;
use soldier::{Air, Input, Mover, Posture, Trig, PART_CACHES, V3};

// The exchange slots (mirrors them).
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
    family: GameFamily,
    /// A 2005 battle's class tuning and its soldiers' per-life state.
    pub swbf2: Swbf2Lives,
    movers: Option<Movers>,
    native_copies: native::Copies,
    io: [f64; IO_LEN],
    /// Body eligibility follows collision-world order,.
    flags: [u8; MAX_BODIES],
}
impl MovementOwner {
    pub const fn new() -> Self { Self { family: GameFamily::Swbf1, swbf2: Swbf2Lives::new(), movers: None,
        native_copies: native::Copies::new(), io: [0.; IO_LEN], flags: [0; MAX_BODIES] } }
    /// Select the battle's profile before constructing any of its movers.
    pub fn select_family(&mut self, family: GameFamily) -> Result<(), &'static str> {
        if self.family != family && self.movers.as_ref().is_some_and(|m| !m.list.is_empty()) {
            return Err("changing the battle family with retained soldier movers requires a fresh world");
        }
        if self.family != family {
            // Reserve the shared query scratch at battle admission. Individual
            // infantry slots are retained only after a successful movement step.
            self.movers = (family == GameFamily::Swbf2).then(Movers::default);
            self.family = family;
        }
        Ok(())
    }
    /// The battle's game, as admission selected it.
    pub fn family(&self) -> GameFamily { self.family }
    pub(crate) fn check_step(&self, has_step: bool) -> Result<(), ProfileError> {
        check_family(self.family, has_step)
    }
    // Pools initialize only when a mover is constructed.
    pub fn io_mut(&mut self) -> &mut [f64; IO_LEN] { &mut self.io }
    pub fn flags_mut(&mut self) -> &mut [u8; MAX_BODIES] { &mut self.flags }
}
impl Default for MovementOwner { fn default() -> Self { Self::new() } }

/// The 2005 movement profile's inputs that outlive a tick: each class's
/// tuning, resolved once from the matched content, and each soldier life's
/// energy, sprint and roll, by unit slot. A 2004 battle leaves both empty.
pub struct Swbf2Lives {
    classes: Vec<(crate::store::StrId, swbf2::Tuning)>,
    lives: Vec<Option<Swbf2Life>>,
    /// Whether each slot's last completed movement tick lets his firearms
    /// fire; a slot that never stepped a 2005 tick is unrestricted.
    refusals: Vec<bool>,
}
#[derive(Clone, Copy)]
struct Swbf2Life { generation: f64, class: crate::store::StrId, tuning: usize, state: swbf2::State }

impl Swbf2Lives {
    pub const fn new() -> Self { Self { classes: Vec::new(), lives: Vec::new(), refusals: Vec::new() } }
    /// A class's tuning, checked once here rather than on every tick.
    pub fn set_class(&mut self, class: crate::store::StrId, tuning: swbf2::Tuning) -> Result<(), ProfileError> {
        tuning.validate()?;
        match self.classes.iter_mut().find(|(c, _)| *c == class) {
            Some(entry) => entry.1 = tuning,
            None => self.classes.push((class, tuning)),
        }
        Ok(())
    }
    /// Forgets every class's numbers, before admission registers the ones
    /// its content gives.
    pub fn clear_classes(&mut self) {
        self.classes.clear();
    }
    pub fn tuning(&self, class: crate::store::StrId) -> Option<swbf2::Tuning> {
        self.classes.iter().find(|(c, _)| *c == class).map(|(_, t)| *t)
    }
    /// Every class with numbers, in the order they were first registered.
    pub fn classes(&self) -> &[(crate::store::StrId, swbf2::Tuning)] { &self.classes }
    /// Reserve one life per unit slot before the battle's first tick.
    pub fn reserve(&mut self, slots: usize) {
        self.lives.reserve(slots.saturating_sub(self.lives.len()));
        self.refusals.reserve(slots.saturating_sub(self.refusals.len()));
    }
    /// Records whether the slot's completed movement tick lets it fire.
    pub fn set_fire(&mut self, slot: u32, can_fire: bool) {
        let slot = slot as usize;
        if self.refusals.len() <= slot { self.refusals.resize(slot + 1, false); }
        self.refusals[slot] = !can_fire;
    }
    /// Whether the slot's firearms may fire in the tick its body just moved.
    pub fn fire_permitted(&self, slot: u32) -> bool { !self.refusals.get(slot as usize).copied().unwrap_or(false) }
    /// The life's tuning and state: the stored state of this life, or the
    /// fresh state a new life starts with (full energy, no sprint or roll).
    /// None when the class has no 2005 tuning.
    pub fn state(&self, slot: u32, generation: f64, class: crate::store::StrId) -> Option<(swbf2::Tuning, swbf2::State)> {
        // A stored life knows its class's entry; only a new life looks it up.
        let stored = self.lives.get(slot as usize).copied().flatten()
            .filter(|life| life.generation == generation && life.class == class);
        if let Some(life) = stored { return Some((self.classes[life.tuning].1, life.state)); }
        let tuning = self.tuning(class)?;
        Some((tuning, swbf2::State::new(&tuning)))
    }
    pub fn store(&mut self, slot: u32, generation: f64, class: crate::store::StrId, state: swbf2::State) {
        let slot = slot as usize;
        if self.lives.len() <= slot { self.lives.resize(slot + 1, None); }
        let tuning = match self.lives[slot] {
            Some(life) if life.class == class => life.tuning,
            _ => match self.classes.iter().position(|(c, _)| *c == class) { Some(at) => at, None => return },
        };
        self.lives[slot] = Some(Swbf2Life { generation, class, tuning, state });
    }
}
impl Default for Swbf2Lives { fn default() -> Self { Self::new() } }

/// The recorded admission call that registers one 2005 class's numbers:
/// the class's interned name (the call's one string), then its numbers'
/// words (swbf2::words). False, changing nothing, for an id that does not
/// name that string in this world or words no writer makes.
/// A game's number in a recorded admission: its release year.
pub fn family_code(family: GameFamily) -> f64 {
    match family { GameFamily::Swbf1 => 2004.0, GameFamily::Swbf2 => 2005.0 }
}
/// The recorded admission's game, by its number, selected before any
/// class's numbers are registered.
pub(crate) fn movement_family_in(w: &mut crate::world::owner::World, args: &[f64]) -> bool {
    let &[code] = args else { return false };
    let Some(family) = GameFamily::ALL.into_iter().find(|&f| family_code(f) == code) else { return false };
    w.movement.select_family(family).is_ok()
}
pub(crate) fn movement_swbf2_class_in(w: &mut crate::world::owner::World, args: &[f64], strings: &[Vec<u16>]) -> bool {
    let (Some((&id, words)), [name]) = (args.split_first(), strings) else { return false };
    if !(id.fract() == 0.0 && id >= 0.0 && id < w.sim.strings.list.len() as f64) { return false; }
    let class = crate::store::StrId(id as u32);
    if w.sim.strings.units(class) != name.as_slice() { return false; }
    let Some(tuning) = swbf2::words::take(words) else { return false };
    w.movement.swbf2.set_class(class, tuning).is_ok()
}
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

/// A step can only run with the state and class data of its battle family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileError {
    MissingStep,
    UnexpectedStep,
    InvalidDelta,
    InvalidTuning,
    InvalidState,
}

impl ProfileError {
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingStep => "Battlefront II movement requires class tuning, actions and per-life state",
            Self::UnexpectedStep => "Battlefront movement received a Battlefront II step",
            Self::InvalidDelta => "Battlefront II movement requires a finite nonnegative timestep",
            Self::InvalidTuning => "invalid Battlefront II movement class tuning",
            Self::InvalidState => "invalid Battlefront II movement checkpoint",
        }
    }
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.message()) }
}
impl std::error::Error for ProfileError {}

fn check_family(family: GameFamily, has_step: bool) -> Result<(), ProfileError> {
    match (family, has_step) {
        (GameFamily::Swbf2, false) => Err(ProfileError::MissingStep),
        (GameFamily::Swbf1, true) => Err(ProfileError::UnexpectedStep),
        _ => Ok(()),
    }
}

/// Returns the movement permission for firearm use during this tick.
/// A missing or mismatched profile input leaves the mover untouched.
pub fn advance_profiled(m: &mut Mover, input: &Input, dt: f64, scene: &mut Scene, swbf2: Option<swbf2::Step<'_>>) -> Result<bool, ProfileError> {
    check_family(m.family(), swbf2.is_some())?;
    if let Some(step) = swbf2 {
        return m.advance_swbf2(input, dt, scene, step);
    }
    let yaw = m.aim_yaw;
    let mut trig = Trig { sin0: v8math::sin(yaw), cos0: v8math::cos(yaw), sin_pitch: 0.0, cos_pitch: 0.0, sin1: 0.0, cos1: 0.0 };
    // jetTick's lift and climb read its pitch (a hovering jet's only).
    if m.air == Air::Jet && m.jet.hover {
        trig.sin_pitch = v8math::sin(m.aim_pitch);
        trig.cos_pitch = v8math::cos(m.aim_pitch);
    }
    let rest = m.begin(input, dt, &trig);
    trig.after_turn(yaw, m.aim_yaw);
    m.finish(rest, input, dt, &trig, scene);
    Ok(true)
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
    pub fn advance(&mut self, collision: &mut crate::collision::State, id: u32, input: &Input, dt: f64) -> Result<bool, ProfileError> {
        self.advance_profiled(collision, id, input, dt, None)
    }
    /// Steps a retained mover with the per-life state for its battle family.
    pub fn advance_profiled(&mut self, collision: &mut crate::collision::State, id: u32, input: &Input, dt: f64, swbf2: Option<swbf2::Step<'_>>) -> Result<bool, ProfileError> {
        let ms = self.movers.as_mut().expect("initialized movement owner");
        let m = &mut ms.list[id as usize];
        let world = m.world;
        let count = collision.worlds[world].order.len().min(MAX_BODIES);
        let mut scene = Scene { s: collision, world, flags: &self.flags[..count], gathered: &mut ms.gathered, caches: &mut ms.caches[id as usize], ground_cache: &mut ms.ground };
        advance_profiled(m, input, dt, &mut scene, swbf2)
    }
    /// Borrow the retained soldier and its collision scratch for a coupled combat step.
    pub(crate) fn with_soldier_scene<T>(&mut self, collision: &mut crate::collision::State,
        id: u32, run: impl FnOnce(&mut Mover, &mut Scene<'_>) -> T) -> T {
        let ms = self.movers.as_mut().expect("initialized movement owner");
        let mover = &mut ms.list[id as usize];
        let world = mover.world;
        let count = collision.worlds[world].order.len().min(MAX_BODIES);
        let mut scene = Scene { s: collision, world, flags: &self.flags[..count],
            gathered: &mut ms.gathered, caches: &mut ms.caches[id as usize], ground_cache: &mut ms.ground };
        run(mover, &mut scene)
    }
    /// Simulate a new infantry body before assigning it a retained slot.
    pub(crate) fn advance_initial_swbf2(&mut self, collision: &mut crate::collision::State,
        mut mover: Mover, input: &Input, dt: f64, step: swbf2::Step<'_>) -> Result<(u32, bool), ProfileError> {
        let ms = self.movers.get_or_insert_with(Movers::default);
        let world = mover.world;
        let count = collision.worlds[world].order.len().min(MAX_BODIES);
        let mut caches = std::array::from_fn(|_| GatherCache::new(0.75));
        let mut scene = Scene { s: collision, world, flags: &self.flags[..count], gathered: &mut ms.gathered,
            caches: &mut caches, ground_cache: &mut ms.ground };
        let can_fire = advance_profiled(&mut mover, input, dt, &mut scene, Some(step))?;
        let id = ms.list.len() as u32;
        ms.list.push(mover);
        ms.caches.push(caches);
        Ok((id, can_fire))
    }
    pub fn new_mover(&mut self, world: usize, class: impl FnOnce(&mut Mover)) -> u32 {
        let ms = self.movers.get_or_insert_with(Movers::default);
        let mut m = Mover::for_family(world, self.family);
        class(&mut m);
        ms.list.push(m);
        ms.caches.push(std::array::from_fn(|_| GatherCache::new(0.75)));
        (ms.list.len() - 1) as u32
    }
    /// The triangles around the last body part that met the world.
    pub(crate) fn gathered_triangles(&self) -> usize {
        let Some(ms) = &self.movers else { return 0 };
        ms.gathered.list().iter().map(|p| p.tris.len()).sum()
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
    /// Legacy exchange status: 0 success, 2 unknown state, 3 missing profile data.
    pub fn tick(&mut self, collision: &mut crate::collision::State, id: u32) -> u32 {
        let ms = self.movers.as_mut().expect("initialized movement owner");
        let mover = &mut ms.list[id as usize];
        if check_family(mover.family(), false).is_err() { return 3; }
        if !load(mover, &self.io) { return 2; }
        let input = input(&self.io);
        if self.advance(collision, id, &input, self.io[DT]).is_err() { return 3; }
        store(&self.movers.as_ref().expect("initialized movement owner").list[id as usize], &mut self.io);
        0
    }
}

