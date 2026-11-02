//! src/soldier.js's Soldier.tick, operation by operation: the posture and
//! jump states, the drive along the ground, the jetpack, and the collision
//! pass against the terrain, the buildings and the bodies.
//!
//! Every value must come out as the JavaScript's own, to the last bit, so a
//! world or a page may run either. Each method below repeats the method of
//! the same name in soldier.js in the same order of operations, in f64, with
//! three.js's Vector3 and Triangle methods spelt out as they compute
//! (`V3`). JavaScript's Math.min and Math.max are `js_min` and `js_max`, and
//! Math.hypot of two numbers is `js_hypot2`, V8's algorithm. Math.sin and
//! Math.cos are not computed here: V8's last bit can differ from any Rust
//! sine's, so JavaScript hands in the sine and cosine of the aim's yaw
//! before and after the tick's turn, and of its pitch (`Trig`).

use super::scene::{Kind, Scene};
use crate::{js_max, js_min};

/// three.js's Vector3, as its methods compute.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const ZERO: V3 = V3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const UP: V3 = V3 { x: 0.0, y: 1.0, z: 0.0 };

    #[inline(always)]
    pub const fn new(x: f64, y: f64, z: f64) -> V3 {
        V3 { x, y, z }
    }
    #[inline(always)]
    pub fn arr(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    #[inline(always)]
    pub fn from(a: [f64; 3]) -> V3 {
        V3 { x: a[0], y: a[1], z: a[2] }
    }
    /// addScaledVector(v, s)
    #[inline(always)]
    pub fn add_scaled(&mut self, v: V3, s: f64) {
        self.x += v.x * s;
        self.y += v.y * s;
        self.z += v.z * s;
    }
    /// add(v)
    #[inline(always)]
    pub fn add(&mut self, v: V3) {
        self.x += v.x;
        self.y += v.y;
        self.z += v.z;
    }
    /// subVectors(a, b)
    #[inline(always)]
    pub fn sub(a: V3, b: V3) -> V3 {
        V3 { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
    }
    /// dot(v)
    #[inline(always)]
    pub fn dot(self, v: V3) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }
    /// lengthSq()
    #[inline(always)]
    pub fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }
    /// length()
    #[inline(always)]
    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    /// multiplyScalar(s)
    #[inline(always)]
    pub fn multiply_scalar(&mut self, s: f64) {
        self.x *= s;
        self.y *= s;
        self.z *= s;
    }
    /// divideScalar(s): a multiplication by 1 / s.
    #[inline(always)]
    pub fn divide_scalar(&mut self, s: f64) {
        self.multiply_scalar(1.0 / s);
    }
    /// normalize(): divideScalar(length() || 1).
    #[inline(always)]
    pub fn normalize(&mut self) {
        let length = self.length();
        self.divide_scalar(if length == 0.0 || length.is_nan() { 1.0 } else { length });
    }
    /// lerp(v, alpha)
    #[inline(always)]
    pub fn lerp(&mut self, v: V3, alpha: f64) {
        self.x += (v.x - self.x) * alpha;
        self.y += (v.y - self.y) * alpha;
        self.z += (v.z - self.z) * alpha;
    }
    /// lerpVectors(a, b, alpha)
    #[inline(always)]
    pub fn lerp_vectors(a: V3, b: V3, alpha: f64) -> V3 {
        V3 { x: a.x + (b.x - a.x) * alpha, y: a.y + (b.y - a.y) * alpha, z: a.z + (b.z - a.z) * alpha }
    }
    /// crossVectors(a, b)
    #[inline(always)]
    pub fn cross(a: V3, b: V3) -> V3 {
        V3 { x: a.y * b.z - a.z * b.y, y: a.z * b.x - a.x * b.z, z: a.x * b.y - a.y * b.x }
    }
    /// negate()
    #[inline(always)]
    pub fn negate(&mut self) {
        self.x = -self.x;
        self.y = -self.y;
        self.z = -self.z;
    }
    /// distanceToSquared(v)
    #[inline(always)]
    pub fn distance_to_squared(self, v: V3) -> f64 {
        let dx = self.x - v.x;
        let dy = self.y - v.y;
        let dz = self.z - v.z;
        dx * dx + dy * dy + dz * dz
    }
}

/// THREE.MathUtils.clamp: Math.max(low, Math.min(high, value)).
#[inline(always)]
pub fn clamp(value: f64, low: f64, high: f64) -> f64 {
    js_max(low, js_min(high, value))
}

/// Math.hypot(a, b) as V8 computes it: the larger magnitude divides both,
/// their squares are summed with Kahan's compensation, and the root of the
/// sum is scaled back.
#[inline(always)]
pub fn js_hypot2(a: f64, b: f64) -> f64 {
    let (x, y) = (a.abs(), b.abs());
    let mut max = 0.0f64;
    if !a.is_nan() && x > max {
        max = x;
    }
    if !b.is_nan() && y > max {
        max = y;
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for v in [x, y] {
        let n = v / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// Math.sign
#[inline(always)]
fn js_sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        v
    }
}

// ENGINE (soldier.js)
pub const GRAVITY: f64 = 18.0;
const DRIVE: f64 = 1.2;
const AIR_DRIVE: f64 = 0.2;
const STEEP_DRIVE: f64 = 0.01;
const BRAKE: f64 = 0.6;
const SLOPE_PULL: f64 = 0.4;
const STEEP_NORMAL: f64 = 0.75;
pub const GROUND_NORMAL: f64 = 0.6;
const LEAVING: f64 = 0.1;
const JUMP_KICK: f64 = 0.25;
const LAND_LOCK: f64 = 0.3;
const READY: f64 = 0.5;
const HARD_LANDING: f64 = 2.0;
const HARD_LOCK: f64 = 0.5;
const DIVE_STICK: f64 = 0.9;
const THROWN_GRAVITY: f64 = 13.5;
const JET_LIFT: f64 = 13.0;
const JET_DAMPING: f64 = 2.5;
const JET_LIFT_SPEED: f64 = 2.0;
const JET_CLIMB: f64 = 4.0;
const JET_BRAKE: f64 = 0.3;
const JET_SPEED: f64 = 2.0;
const JET_LOW_SPEED: f64 = 0.7;
const CEILING_BAND: f64 = 5.0;
const CEILING_PUSH: f64 = 10.0;
const PRONE_PITCH: f64 = 0.4;
const LOCKED_BRAKE: f64 = 1.7;

/// MODES: the stick scale per posture and the brake factor.
#[derive(Clone, Copy)]
struct Mode {
    forward: f64,
    strafe: f64,
    turn: f64,
    brake: f64,
}
const STAND_MODE: Mode = Mode { forward: 1.0, strafe: 1.0, turn: 1.0, brake: 1.2 };
const CROUCH_MODE: Mode = Mode { forward: 0.7, strafe: 0.6, turn: 1.0, brake: 1.0 };
const PRONE_MODE: Mode = Mode { forward: 0.3, strafe: 0.2, turn: 0.5, brake: 1.0 };
const AIR_MODE: Mode = Mode { forward: 0.2, strafe: 0.2, turn: 0.5, brake: 0.0 };
const JET_MODE: Mode = Mode { forward: 1.0, strafe: 1.0, turn: 1.0, brake: 0.3 };
const DIVE_MODE: Mode = Mode { forward: 0.1, strafe: 1.7, turn: 0.6, brake: 1.0 };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posture {
    Stand = 0,
    Crouch = 1,
    Prone = 2,
}

impl Posture {
    pub fn from_code(code: f64) -> Option<Posture> {
        match code as i32 {
            0 => Some(Posture::Stand),
            1 => Some(Posture::Crouch),
            2 => Some(Posture::Prone),
            _ => None,
        }
    }
}

/// `air`: null, "jump", "fall", "thrown" or "jet".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Air {
    None = 0,
    Jump = 1,
    Fall = 2,
    Thrown = 3,
    Jet = 4,
}

impl Air {
    pub fn from_code(code: f64) -> Option<Air> {
        match code as i32 {
            0 => Some(Air::None),
            1 => Some(Air::Jump),
            2 => Some(Air::Fall),
            3 => Some(Air::Thrown),
            4 => Some(Air::Jet),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Lower,
    Upper,
    Medium,
}

/// A body sphere (BODY.parts): its centre in the soldier's frame, its radius,
/// and the gather cache it keeps (one per part, as soldier.js keys them).
#[derive(Clone, Copy)]
pub struct Part {
    role: Role,
    y: f64,
    z: f64,
    pub r: f64,
    pub cache: usize,
}

const STAND_PARTS: [Part; 2] = [
    Part { role: Role::Lower, y: 0.57, z: 0.15, r: 0.57, cache: 0 },
    Part { role: Role::Upper, y: 1.48, z: 0.15, r: 0.57, cache: 1 },
];
const CROUCH_PARTS: [Part; 2] = [
    Part { role: Role::Lower, y: 0.57, z: 0.15, r: 0.57, cache: 2 },
    Part { role: Role::Upper, y: 1.18, z: 0.15, r: 0.57, cache: 3 },
];
const PRONE_PARTS: [Part; 4] = [
    Part { role: Role::Medium, y: 0.72, z: 0.2023, r: 0.8, cache: 4 },
    Part { role: Role::Upper, y: 0.3465, z: 0.6075, r: 0.385, cache: 5 },
    Part { role: Role::Lower, y: 0.3465, z: -0.6075, r: 0.385, cache: 6 },
    Part { role: Role::Medium, y: 0.3465, z: -0.2023, r: 0.385, cache: 7 },
];
pub const PART_CACHES: usize = 8;

const STAND_STEP: f64 = 0.57;
const CROUCH_STEP: f64 = 0.45;
const PRONE_STEP: f64 = 0.1375;
const SLACK: f64 = 0.02;
const STAND_CLEAR: [f64; 2] = [0.55, 1.35];
const STEP_HEIGHT: f64 = 0.45;
const GROUND_SNAP: f64 = 0.25;

fn parts_of(posture: Posture) -> &'static [Part] {
    match posture {
        Posture::Stand => &STAND_PARTS,
        Posture::Crouch => &CROUCH_PARTS,
        Posture::Prone => &PRONE_PARTS,
    }
}

fn step_of(posture: Posture) -> f64 {
    match posture {
        Posture::Stand => STAND_STEP,
        Posture::Crouch => CROUCH_STEP,
        Posture::Prone => PRONE_STEP,
    }
}

/// The class's jetpack (Soldier.jet).
#[derive(Clone, Copy, Default)]
pub struct Jet {
    pub jump: f64,
    pub push: f64,
    pub recharge: f64,
    pub cost: f64,
    pub initial: f64,
    pub min_border: f64,
    pub hover: bool,
}

/// What a tick reads of its input (the JavaScript evaluates `input.turn || 0`,
/// `input.pitch || 0`, and the truthiness of the presses and of
/// `input.jetDirection`).
#[derive(Clone, Copy, Default)]
pub struct Input {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub pitch: f64,
    pub jump: bool,
    pub crouch: bool,
    pub prone: bool,
    pub jet_direction: Option<V3>,
}

/// Math.sin and Math.cos of the aim's yaw before the tick's turn, after it
/// (filled in once the turn is known), and of its pitch.
#[derive(Clone, Copy, Default)]
pub struct Trig {
    pub sin0: f64,
    pub cos0: f64,
    pub sin_pitch: f64,
    pub cos_pitch: f64,
    pub sin1: f64,
    pub cos1: f64,
}

/// Where a tick stopped for the sine and cosine of its turned yaw.
#[derive(Clone, Copy)]
pub enum Rest {
    /// The jet ran this tick (jetTick returned true): ceiling, landing,
    /// then the collision pass.
    Jetted,
    /// The jet ended: the collision pass only.
    JetEnded,
    /// On foot or in a jump or fall: the state step, then the collision pass.
    Walk { locked: bool },
}

/// One soldier's movement: the class's numbers, and the state the
/// JavaScript object holds between ticks (copied in and out each tick).
#[derive(Clone)]
pub struct Mover {
    pub world: usize,
    // The class (Soldier's constructor).
    pub max_speed: f64,
    pub max_strafe: f64,
    pub max_jump: f64,
    pub max_turn: f64,
    pub max_pitch: f64,
    pub accel: f64,
    pub pitch_limits: [f64; 2],
    pub jet: Jet,
    pub threshold: [f64; 3],
    pub scale: [f64; 3],
    // Set after construction (units.js buildBody).
    pub max_fly_height: f64,
    pub getup_time: f64,
    pub dive_time: f64,
    // State.
    pub position: V3,
    pub settled: V3,
    pub velocity: V3,
    pub normal: V3,
    pub avoidance: V3,
    pub aim_yaw: f64,
    pub aim_pitch: f64,
    pub posture: Posture,
    pub air: Air,
    pub dive: f64,
    pub dive_from: Posture,
    pub state_time: f64,
    pub lock: f64,
    pub grounded: bool,
    pub fuel: f64,
    pub ground_adjustment: f64,
    pub collision_damage: f64,
    /// passThrough's body slot, or -1.
    pub pass_through: i64,
    // A collision pass's own (contactCount and contactMean end each pass at 0).
    contact_count: f64,
    contact_mean: V3,
    motion: V3,
    step_start: V3,
    step_target: V3,
    step_velocity: V3,
    step_resolved: V3,
    step_blocked: bool,
    pub rest: Option<Rest>,
}

impl Mover {
    pub fn new(world: usize) -> Mover {
        Mover {
            world,
            max_speed: 9.0,
            max_strafe: 7.0,
            max_jump: 8.0,
            max_turn: 2.0,
            max_pitch: 0.5,
            accel: 70.0,
            pitch_limits: [-0.8, 1.1],
            jet: Jet::default(),
            threshold: [10.0, 3.0, 0.0],
            scale: [1.0, 0.0, 0.0],
            max_fly_height: 200.0,
            getup_time: 0.0,
            dive_time: 0.0,
            position: V3::ZERO,
            settled: V3::ZERO,
            velocity: V3::ZERO,
            normal: V3::UP,
            avoidance: V3::ZERO,
            aim_yaw: 0.0,
            aim_pitch: 0.0,
            posture: Posture::Stand,
            air: Air::None,
            dive: 0.0,
            dive_from: Posture::Stand,
            state_time: 0.0,
            lock: 0.0,
            grounded: false,
            fuel: 0.0,
            ground_adjustment: 0.0,
            collision_damage: 0.0,
            pass_through: -1,
            contact_count: 0.0,
            contact_mean: V3::ZERO,
            motion: V3::ZERO,
            step_start: V3::ZERO,
            step_target: V3::ZERO,
            step_velocity: V3::ZERO,
            step_resolved: V3::ZERO,
            step_blocked: false,
            rest: None,
        }
    }

    /// The `mode` getter.
    fn mode(&self) -> Mode {
        if self.air == Air::Jet {
            return JET_MODE;
        }
        if self.air != Air::None {
            return AIR_MODE;
        }
        if self.dive != 0.0 && !self.dive.is_nan() {
            return DIVE_MODE;
        }
        match self.posture {
            Posture::Stand => STAND_MODE,
            Posture::Crouch => CROUCH_MODE,
            Posture::Prone => PRONE_MODE,
        }
    }

    fn pitch_range(&self) -> [f64; 2] {
        if self.posture == Posture::Prone && self.air == Air::None {
            return [-PRONE_PITCH, PRONE_PITCH];
        }
        self.pitch_limits
    }

    /// tick(input, dt) up to the aim's turn: everything that reads the yaw's
    /// sine and cosine from before it. The rest (`finish`) reads them from
    /// after it. Returns where to go on from.
    pub fn begin(&mut self, input: &Input, dt: f64, trig: &Trig) -> Rest {
        self.state_time += dt;
        self.collision_damage = 0.0;
        let locked = self.lock > 0.0;
        self.lock = js_max(self.lock - dt, 0.0);
        self.fuel = js_min(self.fuel + self.jet.recharge * dt, 1.0);
        if !self.grounded {
            self.normal = V3::UP;
        }

        if self.air == Air::Jet {
            return if self.jet_tick(input, dt, trig) { Rest::Jetted } else { Rest::JetEnded };
        }

        let mode = self.mode();
        let mut f = if locked { 0.0 } else { clamp(input.forward, -1.0, 1.0) };
        let mut s = if locked { 0.0 } else { clamp(input.right, -1.0, 1.0) };
        let len = js_hypot2(f, s);
        if len > 1.0 {
            f /= len;
            s /= len;
        }
        let slope = self.normal.y.abs();
        f *= mode.forward * slope;
        s = (if self.dive != 0.0 && !self.dive.is_nan() { self.dive } else { s }) * mode.strafe * slope;
        let brake = if self.posture == Posture::Stand && locked && self.air == Air::None { LOCKED_BRAKE } else { mode.brake };
        self.drive(f, s, brake, dt, false, trig.sin0, trig.cos0);
        let v = self.velocity;
        self.position.add_scaled(v, dt);
        if !locked {
            self.aim_yaw -= self.max_turn * mode.turn * input.turn * dt;
        }
        if !locked && input.pitch != 0.0 && !input.pitch.is_nan() {
            let [low, high] = self.pitch_range();
            self.aim_pitch = clamp(self.aim_pitch + self.max_pitch * input.pitch * dt, low, high);
        }
        Rest::Walk { locked }
    }

    /// The rest of tick(input, dt), with the sine and cosine of the turned yaw.
    pub fn finish(&mut self, rest: Rest, input: &Input, dt: f64, trig: &Trig, scene: &mut Scene) {
        match rest {
            Rest::Jetted => {
                self.ceiling(dt);
                if self.grounded {
                    self.land();
                }
            }
            Rest::JetEnded => {}
            Rest::Walk { locked } => {
                if self.air != Air::None {
                    self.ceiling(dt);
                    let can_jet = (self.air == Air::Jump || self.air == Air::Fall) && (self.state_time <= LAND_LOCK || !self.grounded);
                    if !(input.jump && can_jet && self.start_jet(input, trig)) && self.state_time > LAND_LOCK && self.grounded {
                        self.land();
                    }
                } else if self.dive != 0.0 && !self.dive.is_nan() {
                    if self.state_time > self.dive_time {
                        self.dive = 0.0;
                        self.posture = self.dive_from;
                        self.state_time = 0.0;
                    }
                } else {
                    if self.posture == Posture::Stand {
                        self.wait_out_drop();
                    }
                    if !locked && self.air == Air::None {
                        self.act(input, trig, scene);
                    }
                }
            }
        }
        self.collide(trig, scene);
    }

    fn wait_out_drop(&mut self) {
        let mut t = self.state_time;
        if !self.grounded {
            if t >= 5.0 {
                if t < 10.0 {
                    t = 10.0;
                }
                if t > 11.0 {
                    self.set_air(Air::Fall);
                    return;
                }
            } else {
                t = 10.3;
            }
        } else if t >= 10.0 {
            t = 5.0;
        }
        self.state_time = t;
    }

    fn act(&mut self, input: &Input, trig: &Trig, scene: &mut Scene) {
        if input.prone {
            let next = if self.posture == Posture::Prone { Posture::Stand } else { Posture::Prone };
            self.set_posture(next, scene);
        }
        if input.crouch {
            let next = if self.posture == Posture::Crouch { Posture::Stand } else { Posture::Crouch };
            self.set_posture(next, scene);
        }
        if !input.jump {
            return;
        }
        let t = self.state_time;
        let mut f = clamp(input.forward, -1.0, 1.0);
        let mut s = clamp(input.right, -1.0, 1.0);
        let len = js_hypot2(f, s);
        if len > 1.0 {
            f /= len;
            s /= len;
        }
        if self.posture != Posture::Prone && t < 10.3 && self.dive_time > 0.0 && s.abs() > DIVE_STICK && f.abs() < s.abs() {
            self.dive = js_sign(s);
            self.dive_from = self.posture;
            self.posture = Posture::Crouch;
            self.state_time = 0.0;
            return;
        }
        if self.posture == Posture::Stand {
            if t > READY && t < 10.3 {
                self.jump(input.forward, trig);
            }
        } else if t > READY {
            self.set_posture(Posture::Stand, scene);
        }
    }

    fn set_posture(&mut self, next: Posture, scene: &mut Scene) {
        if next == self.posture {
            return;
        }
        if next == Posture::Stand && !self.headroom(scene) {
            return;
        }
        if self.posture == Posture::Prone && next == Posture::Stand {
            self.lock = self.getup_time;
        }
        self.posture = next;
        self.state_time = 0.0;
    }

    fn set_air(&mut self, kind: Air) {
        self.air = kind;
        self.posture = Posture::Stand;
        self.state_time = 0.0;
    }

    fn land(&mut self) {
        if self.state_time > HARD_LANDING {
            self.lock = HARD_LOCK;
        }
        self.air = Air::None;
        self.posture = Posture::Stand;
        self.state_time = 0.0;
    }

    fn start_jet(&mut self, input: &Input, trig: &Trig) -> bool {
        let jet = self.jet;
        if self.fuel < jet.min_border {
            return false;
        }
        self.fuel = js_max(self.fuel - jet.initial, 0.0);
        let (f, s) = stick(input);
        let (sin, cos) = (trig.sin1, trig.cos1);
        let forward = js_max(f, -0.1);
        let mut dir = V3::new(sin * forward - 0.1 * s * cos, 1.1, cos * forward + 0.1 * s * sin);
        dir.normalize();
        if let Some(aimed) = input.jet_direction {
            dir = aimed;
        }
        self.velocity.add_scaled(dir, jet.jump);
        self.set_air(Air::Jet);
        true
    }

    /// jetTick up to its turn; false when the jet ends this tick.
    fn jet_tick(&mut self, input: &Input, dt: f64, trig: &Trig) -> bool {
        let jet = self.jet;
        self.posture = Posture::Stand;
        let (f, s) = stick(input);
        let (forward, strafe, brake);
        if jet.hover {
            self.velocity.y += (JET_LIFT - JET_DAMPING * (self.velocity.y - JET_LIFT_SPEED)) * dt;
            self.velocity.y += jet.push * JET_CLIMB * trig.sin_pitch * f * dt;
            forward = JET_SPEED * trig.cos_pitch * f;
            strafe = JET_SPEED * s;
            brake = JET_BRAKE;
            self.fuel -= jet.cost * dt;
            if self.fuel < 0.0 {
                self.fuel = 0.0;
                self.set_air(Air::Fall);
                return false;
            }
            if input.jump {
                self.set_air(Air::Fall);
                return false;
            }
        } else {
            self.velocity.y += jet.push * dt;
            forward = JET_LOW_SPEED * f;
            strafe = JET_LOW_SPEED * s;
            brake = 0.0;
        }
        self.drive(forward, strafe, brake, dt, true, trig.sin0, trig.cos0);
        let v = self.velocity;
        self.position.add_scaled(v, dt);
        self.aim_yaw -= self.max_turn * JET_MODE.turn * input.turn * dt;
        true
    }

    fn ceiling(&mut self, dt: f64) {
        let h = self.max_fly_height - self.position.y;
        if h < CEILING_BAND {
            self.velocity.y -= CEILING_PUSH * (CEILING_BAND - h) * dt;
        }
    }

    fn jump(&mut self, forward: f64, trig: &Trig) {
        let kick = self.max_jump * JUMP_KICK * clamp(forward, -1.0, 1.0);
        self.velocity.y = self.max_jump;
        self.velocity.x += trig.sin1 * kick;
        self.velocity.z += trig.cos1 * kick;
        self.set_air(Air::Jump);
    }

    #[allow(clippy::too_many_arguments)]
    fn drive(&mut self, f: f64, s: f64, brake: f64, dt: f64, jet: bool, sin: f64, cos: f64) {
        let n = self.normal;
        let facing = V3::new(sin, 0.0, cos);
        let mut forward_axis = facing;
        forward_axis.add_scaled(n, -facing.dot(n));
        forward_axis.normalize();
        let mut right_axis = V3::cross(forward_axis, n);
        right_axis.normalize();
        let v = &mut self.velocity;
        if self.grounded {
            let mut pull = V3::new(0.0, -self.accel * SLOPE_PULL * dt, 0.0);
            pull.add_scaled(n, -n.dot(pull));
            v.add(pull);
            let into = v.dot(n);
            if into < 0.0 || self.air == Air::None {
                v.add_scaled(n, -into);
            }
        }
        let mut vf = v.dot(forward_axis);
        let mut vr = v.dot(right_axis);
        v.add_scaled(forward_axis, -vf);
        v.add_scaled(right_axis, -vr);
        if brake > 0.0 && (jet || (self.grounded && n.y > STEEP_NORMAL)) {
            let slow = self.accel * BRAKE * brake * dt;
            vf = if vf > 0.0 { js_max(vf - slow, 0.0) } else { js_min(vf + slow, 0.0) };
            vr = if vr > 0.0 { js_max(vr - slow, 0.0) } else { js_min(vr + slow, 0.0) };
        }
        let mut gain = self.accel * DRIVE;
        if jet {
            gain *= JET_BRAKE;
        } else {
            if !self.grounded {
                gain *= AIR_DRIVE;
            }
            if n.y < STEEP_NORMAL {
                gain *= STEEP_DRIVE;
            }
        }
        gain *= dt;
        let want_f = self.max_speed * f;
        let want_r = self.max_strafe * s;
        if f > 0.0 && vf < want_f {
            vf = js_min(vf + gain, want_f);
        }
        if f < 0.0 && vf > want_f {
            vf = js_max(vf - gain, want_f);
        }
        if s > 0.0 && vr < want_r {
            vr = js_min(vr + gain, want_r);
        }
        if s < 0.0 && vr > want_r {
            vr = js_max(vr - gain, want_r);
        }
        v.add_scaled(forward_axis, vf);
        v.add_scaled(right_axis, vr);
        if !self.grounded {
            v.y -= (if self.air == Air::Thrown { THROWN_GRAVITY } else { GRAVITY }) * dt;
        }
    }

    /// One collision pass (collide()).
    fn collide(&mut self, trig: &Trig, scene: &mut Scene) {
        let posture = if self.air != Air::None { Posture::Stand } else { self.posture };
        let parts = parts_of(posture);
        let walking = self.grounded && self.air == Air::None && self.posture != Posture::Prone;
        let intended_y = self.position.y;
        self.grounded = false;
        self.motion = V3::sub(self.position, self.settled);
        let length = self.motion.length();
        if length <= 100.0 {
            let count = (1.0 + length / step_of(posture)).floor();
            self.motion.divide_scalar(count);
            self.position = self.settled;
            let mut i = 0.0;
            while i < count {
                self.step_start = self.position;
                self.step_blocked = false;
                self.step_velocity = self.velocity;
                let motion = self.motion;
                self.position.add(motion);
                self.step_target = self.position;
                self.touch_terrain(parts, trig, scene);
                self.touch_polygons(parts, trig, scene);
                if walking {
                    self.step_up(parts, trig, scene);
                    self.snap_ground(trig, scene);
                }
                i += 1.0;
            }
        }
        self.settled = self.position;
        let mean = &mut self.contact_mean;
        let length_sq = mean.length_sq();
        if length_sq > 0.0001 {
            mean.divide_scalar(length_sq.sqrt());
            if mean.y > GROUND_NORMAL && !self.grounded {
                self.normal = *mean;
                self.grounded = true;
            }
        }
        self.contact_mean = V3::ZERO;
        self.contact_count = 0.0;
        if walking && self.grounded {
            self.ground_adjustment += self.position.y - intended_y;
        }
    }

    fn step_up(&mut self, parts: &[Part], trig: &Trig, scene: &mut Scene) {
        if !self.step_blocked {
            return;
        }
        let dx = self.step_target.x - self.step_start.x;
        let dz = self.step_target.z - self.step_start.z;
        let distance = js_hypot2(dx, dz);
        if distance < 1e-5 {
            return;
        }
        let lost = (self.step_target.x - self.position.x) * dx + (self.step_target.z - self.position.z) * dz;
        if lost < distance * 0.005 {
            return;
        }
        // parts.find(part => part.role === "lower"): stand and crouch alone step.
        let Some(lower) = parts.iter().find(|part| part.role == Role::Lower) else { return };
        let origin = V3::new(
            self.step_target.x + trig.sin1 * lower.z + dx / distance * lower.r,
            self.step_start.y + STEP_HEIGHT + SLACK,
            self.step_target.z + trig.cos1 * lower.z + dz / distance * lower.r,
        );
        let Some(hit) = scene.cast(origin, V3::new(0.0, -1.0, 0.0), STEP_HEIGHT + SLACK) else { return };
        if hit.normal.y <= GROUND_NORMAL {
            return;
        }
        let rise = hit.point.y - self.step_start.y;
        if rise <= SLACK || rise > STEP_HEIGHT {
            return;
        }
        self.step_resolved = self.position;
        let height = hit.point.y;
        let samples = ((rise + distance) / 0.1).ceil();
        let mut i = 1.0;
        while i <= samples {
            self.position = self.step_start;
            self.position.y += rise * i / samples;
            if !self.clear(parts, trig, scene) {
                self.position = self.step_resolved;
                return;
            }
            i += 1.0;
        }
        let mut i = 1.0;
        while i <= samples {
            self.position = V3::lerp_vectors(self.step_start, self.step_target, i / samples);
            self.position.y = height;
            if !self.clear(parts, trig, scene) {
                self.position = self.step_resolved;
                return;
            }
            i += 1.0;
        }
        self.velocity = self.step_velocity;
        self.velocity.y = js_min(self.velocity.y, 0.0);
        self.touch_terrain(parts, trig, scene);
        self.touch_polygons(parts, trig, scene);
    }

    /// stepUp's `clear`: no part, shrunk by the slack, overlaps anything.
    fn clear(&self, parts: &[Part], trig: &Trig, scene: &mut Scene) -> bool {
        parts.iter().all(|part| !scene.overlaps_sphere(self.part_center(part, trig), part.r - SLACK))
    }

    fn snap_ground(&mut self, trig: &Trig, scene: &mut Scene) {
        if self.grounded {
            return;
        }
        let origin = V3::new(self.position.x + trig.sin1 * 0.15, self.position.y + SLACK * 2.0, self.position.z + trig.cos1 * 0.15);
        let Some(hit) = scene.cast(origin, V3::new(0.0, -1.0, 0.0), GROUND_SNAP + SLACK * 2.0) else { return };
        if hit.normal.y <= GROUND_NORMAL {
            return;
        }
        let height = hit.point.y + (if hit.terrain { 0.0 } else { STAND_PARTS[0].r / hit.normal.y - STAND_PARTS[0].y });
        let drop = self.position.y - height;
        if drop < -SLACK || drop > GROUND_SNAP {
            return;
        }
        self.position.y = height - SLACK * 0.25;
        self.normal = hit.normal;
        self.grounded = true;
        let into = self.velocity.dot(self.normal);
        let normal = self.normal;
        self.velocity.add_scaled(normal, -into);
    }

    #[inline(always)]
    fn part_center(&self, part: &Part, trig: &Trig) -> V3 {
        V3::new(self.position.x + trig.sin1 * part.z, self.position.y + part.y, self.position.z + trig.cos1 * part.z)
    }

    fn touch_terrain(&mut self, parts: &[Part], trig: &Trig, scene: &mut Scene) {
        for part in parts {
            let c = self.part_center(part, trig);
            let Some((height, normal)) = scene.ground(c.x, c.z, 1e4) else { continue };
            let sink = height - c.y + part.r;
            if sink < 0.0 || sink > 2.0 * part.r {
                continue;
            }
            self.contact(part, normal, sink * normal.y, normal, true, Some(Kind::Terrain), trig);
        }
    }

    fn touch_polygons(&mut self, parts: &[Part], trig: &Trig, scene: &mut Scene) {
        for part in parts {
            let mut c = self.part_center(part, trig);
            scene.gather(c, part.r, part.cache, self.pass_through);
            loop {
                let mut best: Option<(usize, usize)> = None;
                let mut distance_sq = part.r * part.r;
                let mut point = V3::ZERO;
                for (p, polygon) in scene.polygons().iter().enumerate() {
                    if polygon.done {
                        continue;
                    }
                    for (t, tri) in polygon.tris.iter().enumerate() {
                        let closest = V3::from(scene.closest_point(tri, c));
                        let d_sq = closest.distance_to_squared(c);
                        if d_sq < distance_sq {
                            distance_sq = d_sq;
                            best = Some((p, t));
                            point = closest;
                        }
                    }
                }
                let Some((p, t)) = best else { break };
                let distance = distance_sq.sqrt();
                let polygon = &mut scene.polygons_mut()[p];
                polygon.done = true;
                let moving = polygon.moving;
                let mut surface = V3::from(scene.triangle_normal(p, t));
                let mut contact_normal;
                if distance >= 1e-4 {
                    contact_normal = V3::sub(c, point);
                    contact_normal.divide_scalar(distance);
                } else {
                    contact_normal = surface;
                    // closest.subVectors(stepStart, position)
                    let toward = V3::sub(self.step_start, self.position);
                    if contact_normal.dot(toward) < 0.0 {
                        contact_normal.negate();
                    }
                }
                if surface.y < 0.0 {
                    surface.negate();
                }
                let before = self.position;
                if contact_normal.y.abs() < 0.3 {
                    self.avoidance.add(contact_normal);
                }
                self.contact(part, contact_normal, part.r - distance, surface, false, if moving { None } else { Some(Kind::Building) }, trig);
                c.add(V3::sub(self.position, before));
            }
        }
    }

    /// The engine's soldier contact rule (contact()). `surface_is_n`: the
    /// caller passed the same vector as n and surface (the terrain's), which
    /// the JavaScript's `support !== n` tells apart from an equal one.
    #[allow(clippy::too_many_arguments)]
    fn contact(&mut self, part: &Part, n: V3, depth: f64, surface: V3, surface_is_n: bool, kind: Option<Kind>, trig: &Trig) {
        let vn = self.velocity.dot(n);
        let mut push = n;
        let mut clip = n;
        let surface_supports = n.y > GROUND_NORMAL && surface.y > GROUND_NORMAL;
        let support = if surface_supports { surface } else { n };
        if part.role == Role::Lower && surface_supports && !surface_is_n {
            clip = support;
        }
        if part.role == Role::Lower
            && n.y >= 0.0
            && (surface.y <= GROUND_NORMAL || n.dot(surface) < 0.98)
            && n.x * self.motion.x + n.z * self.motion.z < -1e-5
        {
            self.step_blocked = true;
        }
        let mut d = depth;
        if self.posture == Posture::Prone && self.air == Air::None {
            if n.y > GROUND_NORMAL {
                if vn < LEAVING && !(self.grounded && n.y <= self.normal.y) {
                    self.normal.lerp(support, 0.5);
                    self.normal.normalize();
                    self.grounded = true;
                }
            } else {
                let into = self.normal.dot(push);
                if into < 0.0 {
                    d = along_plane(&mut push, self.normal, -into, d);
                }
                flatten(&mut clip);
            }
        } else if part.role == Role::Lower {
            if n.y > 0.0 {
                self.contact_count += 1.0;
                self.contact_mean.lerp(support, 1.0 / self.contact_count);
            }
            if n.y > GROUND_NORMAL {
                if vn < LEAVING && !(self.grounded && n.y <= self.normal.y) {
                    self.normal = support;
                    self.grounded = true;
                }
            } else {
                if n.y < 0.0 {
                    push.y = 0.0;
                    let len = push.length();
                    if len > 0.0 {
                        push.divide_scalar(len);
                        d /= len;
                    }
                }
                flatten(&mut clip);
            }
        } else if part.role == Role::Upper {
            if n.y > 0.0 {
                flatten(&mut clip);
            } else if n.y < 0.0 && self.grounded {
                let g = self.normal;
                let into = -g.dot(push);
                d = along_plane(&mut push, g, into, d);
            }
        }

        let along = clip.dot(self.velocity);
        if let Some(kind) = kind {
            if along < 0.0 {
                let forward = n.x * trig.sin1 + n.z * trig.cos1;
                let into = if kind == Kind::Building { vn * n.y } else { vn };
                self.collision_damage += contact_damage(&self.threshold, &self.scale, into, n.y, forward);
            }
        }
        if along < 0.0 {
            self.velocity.add_scaled(clip, -along);
        }
        if self.velocity.x.abs() < 0.1 {
            self.velocity.x = 0.0;
        }
        if self.velocity.z.abs() < 0.1 {
            self.velocity.z = 0.0;
        }
        if d > 2.0 && d > depth {
            d = 0.0;
        }
        let mut shove = push;
        shove.multiply_scalar(d);
        if shove.y > 0.0 {
            shove.y = js_max(shove.y - SLACK, 0.0);
        }
        self.position.add(shove);
    }

    /// Room to stand: a ray up from knee height (headroom()).
    fn headroom(&mut self, scene: &mut Scene) -> bool {
        let [from, length] = STAND_CLEAR;
        let origin = V3::new(self.position.x, self.position.y + from, self.position.z);
        scene.cast(origin, V3::UP, length).is_none()
    }
}

/// stick(input): the stick, clamped and scaled down to length 1.
fn stick(input: &Input) -> (f64, f64) {
    let mut f = clamp(input.forward, -1.0, 1.0);
    let mut s = clamp(input.right, -1.0, 1.0);
    let len = js_hypot2(f, s);
    if len > 1.0 {
        f /= len;
        s /= len;
    }
    (f, s)
}

/// flatten(v): its height zeroed and the rest scaled to length 1.
#[inline(always)]
fn flatten(v: &mut V3) {
    v.y = 0.0;
    let len = js_hypot2(v.x, v.z);
    if len > 0.0 {
        v.divide_scalar(len);
    }
}

/// alongPlane(push, plane, into, depth)
#[inline(always)]
fn along_plane(push: &mut V3, plane: V3, into: f64, depth: f64) -> f64 {
    push.add_scaled(plane, into);
    let len = push.length();
    if len <= 0.0 {
        return depth;
    }
    push.divide_scalar(len);
    depth / len
}

/// collision-damage.js contactDamage(rule, into, up, forward).
pub fn contact_damage(threshold: &[f64; 3], scale: &[f64; 3], into: f64, up: f64, forward: f64) -> f64 {
    if !(into < 0.0) {
        return 0.0;
    }
    let [t0, t1, t2] = *threshold;
    let limit = t0 + t1 * up + t2 * forward;
    if !(-limit > into) {
        return 0.0;
    }
    let [s0, s1, s2] = *scale;
    let excess = into + limit;
    let damage = (s0 + s1 * up + s2 * forward) * excess * excess;
    if damage > 0.0 {
        damage
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hypot_follows_v8() {
        assert_eq!(js_hypot2(3.0, 4.0), 5.0);
        assert_eq!(js_hypot2(-0.0, 0.0).to_bits(), 0.0f64.to_bits());
        assert!(js_hypot2(f64::NAN, 1.0).is_nan());
        assert_eq!(js_hypot2(f64::NAN, f64::NEG_INFINITY), f64::INFINITY);
        // Values V8 answers (Node 22 and Electron 44 agree).
        assert_eq!(js_hypot2(0.7071067811865476, 0.7071067811865476), 1.0000000000000002);
        assert_eq!(js_hypot2(1.0, 1e-17), 1.0);
        assert_eq!(js_hypot2(0.3, 0.4), 0.5);
        assert_eq!(js_hypot2(1e-300, 1e-300), 1.4142135623730952e-300);
        assert_eq!(js_hypot2(0.1, 0.2), 0.223606797749979);
    }

    #[test]
    fn damage_matches_the_rule() {
        let (t, s) = ([10.0, 3.0, 0.0], [1.0, 0.0, 0.0]);
        assert_eq!(contact_damage(&t, &s, -12.0, 1.0, 0.0), 0.0);
        assert_eq!(contact_damage(&t, &s, -15.0, 1.0, 0.0), 4.0);
        assert_eq!(contact_damage(&t, &s, 2.0, 1.0, 0.0), 0.0);
    }
}
