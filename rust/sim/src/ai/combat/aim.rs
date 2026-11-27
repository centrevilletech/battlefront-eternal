//! Classic aiming: the direction filter every
//! bot's gun sits behind, the curves it reads, the lead it aims with, and
//! the point its body turns toward. Operation for operation in float32 as
//! the JavaScript writes it (`Math.fround` at every step it rounds).

use std::rc::Rc;
use three::Vector3;

use super::math::{dot32, f, length_sq32, normalize_d3dx};
use super::world::{draw_range, Curves, Gen, Obj, World};
use crate::combat::defs::{Js, JsObj};

/// cos and the angle of 30 degrees, float32.
fn theta_30() -> f64 {
    f(0.5235987901687622)
}
fn cos_30() -> f64 {
    f(v8math::cos(theta_30()))
}

/// A direction or rate as the filter keeps it ({x, y, z}).
pub type V3 = Vector3;

/// `filterDirection(d, q, v, dt, A, D, out)`: v is updated in
/// place; returns the new direction (the caller's `out`).
pub fn filter_direction(d: &Vector3, q: &Vector3, v: &mut Vector3, dt: f64, a: f64, damp: f64) -> Vector3 {
    let dt = f(dt);
    let a = f(a);
    let damp = f(damp);
    let (dx, dy, dz) = (f(d.x), f(d.y), f(d.z));
    let (mut qx, mut qy, mut qz) = (f(q.x), f(q.y), f(q.z));
    let (mut vx, mut vy, mut vz) = (f(v.x), f(v.y), f(v.z));
    // 1. The 30-degree correction.
    if cos_30() > dot32(dx, dy, dz, qx, qy, qz) {
        let cross_y = f(f(qx * dz) - f(dx * qz));
        let theta = if cross_y > 0.0 { theta_30() } else { -theta_30() };
        let c = f(v8math::cos(theta));
        let s = f(v8math::sin(theta));
        qx = f(f(dx * c) + f(dz * s));
        qy = dy;
        qz = f(f(dx * f(-s)) + f(dz * c));
        vx = 0.0;
        vy = 0.0;
        vz = 0.0;
    }
    // 2. a0 = ((d - q) / dt - v) / dt, dividing by multiplying by 1/dt.
    let inv = f(1.0 / dt);
    let mut ax = f(f(f(f(dx - qx) * inv) - vx) * inv);
    let mut ay = f(f(f(f(dy - qy) * inv) - vy) * inv);
    let mut az = f(f(f(f(dz - qz) * inv) - vz) * inv);
    // 3. Clamp its length to A.
    let len = f(v8math::sqrt(length_sq32(ax, ay, az)));
    let k = if a > len { 1.0 } else { f(a / len) };
    ax = f(ax * k);
    ay = f(ay * k);
    az = f(az * k);
    // 4.-5. a = a0 - D v; v += a dt.
    ax = f(ax - f(vx * damp));
    ay = f(ay - f(vy * damp));
    az = f(az - f(vz * damp));
    vx = f(vx + f(ax * dt));
    vy = f(vy + f(ay * dt));
    vz = f(vz + f(az * dt));
    v.x = vx;
    v.y = vy;
    v.z = vz;
    // 6. q + v dt, normalized.
    let n = normalize_d3dx(f(qx + f(vx * dt)), f(qy + f(vy * dt)), f(qz + f(vz * dt)));
    Vector3 { x: n.x, y: n.y, z: n.z }
}

// ---------------------------------------------------------------------------
// Weapon classes as the filter and firing code test them.

/// `def.base` as a string.
pub fn base_of(def: &JsObj) -> Option<&str> {
    def.get("base").str()
}

/// `isCannon`.
pub fn is_cannon(def: Option<&JsObj>) -> bool {
    matches!(def.and_then(base_of), Some("cannon" | "weapon" | "launcher" | "grapplinghookweapon"))
}

/// `isLauncher`.
pub fn is_launcher(def: Option<&JsObj>) -> bool {
    def.and_then(base_of) == Some("launcher")
}

/// `skillCurves(skill)`: each curve through its anchors at
/// skill 1, 14 and 20, in float32.
pub fn skill_curves(skill: f64) -> Curves {
    let k = v8math::min(v8math::max(v8math::trunc(skill), 1.0), 20.0);
    let curve = |a: f64, b: f64, c: f64| {
        let (a, b, c) = (f(a), f(b), f(c));
        if k < 14.0 {
            f(a + f(f(f(k - 1.0) / 13.0) * f(b - a)))
        } else {
            f(b + f(f(f(k - 14.0) / 6.0) * f(c - b)))
        }
    };
    Curves { a: curve(0.8, 1.9, 20.0), d: curve(0.8, 1.9, 2.4), b: Some(curve(3.0, 1.6, 1.0)), s: Some(curve(2.0, 1.0, 0.0)) }
}

/// The four curves as the Aimer keeps them (`this.curves`, all numbers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimCurves {
    pub a: f64,
    pub d: f64,
    pub b: f64,
    pub s: f64,
}

impl Default for AimCurves {
    /// `{ A: 0, D: 0, B: 1, S: 0 }`.
    fn default() -> Self {
        AimCurves { a: 0.0, d: 0.0, b: 1.0, s: 0.0 }
    }
}

/// `aimCurves(curves, weaponDef, out)`: Medium's when the curves
/// have no finite A; A and D halved for a weapon that is not a cannon.
pub fn aim_curves(curves: Option<&Curves>, weapon_def: Option<&JsObj>, out: &mut AimCurves) {
    let fallback;
    let c = match curves {
        Some(c) if c.a.is_finite() => c,
        _ => {
            fallback = skill_curves(14.0);
            &fallback
        }
    };
    out.a = f(c.a);
    out.d = f(c.d);
    out.b = f(c.b.unwrap_or(1.0));
    out.s = f(c.s.unwrap_or(0.0));
    if weapon_def.is_some() && !is_cannon(weapon_def) {
        out.a = f(out.a * 0.5);
        out.d = f(out.d * 0.5);
    }
}

/// `Aimer`: the filtered direction a bot's weapons fire along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aimer {
    pub direction: Vector3,
    pub velocity: Vector3,
    pub started: bool,
    pub curves: AimCurves,
}

impl Default for Aimer {
    fn default() -> Self {
        Aimer { direction: Vector3 { x: 0.0, y: 0.0, z: 1.0 }, velocity: Vector3::default(), started: false, curves: AimCurves::default() }
    }
}

/// What Aimer.update hands the weapon in hand (a cannon's spread curves).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpreadScales {
    pub spread_scale: f64,
    pub sniper_scale: f64,
}

impl Aimer {
    /// `reset(direction)`.
    pub fn reset(&mut self, direction: &Vector3) {
        self.direction = *direction;
        self.velocity = Vector3::default();
        self.started = true;
    }

    /// `update(desired, dt, curves, weapon, hero)`: `weapon` is
    /// the selected weapon's def; the spread curves a cannon takes come
    /// back for the caller to write on it.
    pub fn update(&mut self, desired: &Vector3, dt: f64, curves: Option<&Curves>, weapon: Option<&JsObj>, hero: bool) -> (Vector3, Option<SpreadScales>) {
        if !self.started {
            self.reset(desired);
        }
        if hero {
            self.direction = *desired;
            self.velocity = Vector3::default();
            return (self.direction, None);
        }
        let mut c = self.curves;
        aim_curves(curves, weapon, &mut c);
        self.curves = c;
        let scales = if weapon.is_some() && is_cannon(weapon) { Some(SpreadScales { spread_scale: c.b, sniper_scale: c.s }) } else { None };
        if !(dt > 0.0) {
            return (self.direction, scales);
        }
        let q = self.direction;
        self.direction = filter_direction(desired, &q, &mut self.velocity, dt, c.a, c.d);
        if !self.direction.x.is_finite() || !self.velocity.x.is_finite() {
            self.reset(desired);
        }
        (self.direction, scales)
    }
}

// ---------------------------------------------------------------------------
// Projectile speed and gravity.

/// `projectileOf(def)`: (speed, gravity).
pub fn projectile_of(def: Option<&JsObj>) -> (f64, f64) {
    let Some(def) = def else { return (0.0, 0.0) };
    let ordnance = def.get("ordnance");
    if !ordnance.truthy() {
        return (0.0, 0.0);
    }
    let base = ordnance.get("base");
    let v = ordnance.get("Velocity");
    let g = ordnance.get("Gravity");
    projectile_values(base.str(),(!v.is_nullish()).then(||v.num()),(!g.is_nullish()).then(||g.num()))
}

pub fn projectile_values(b: Option<&str>, velocity: Option<f64>, scale: Option<f64>) -> (f64, f64) {
    let falling = matches!(b, Some("shell" | "grenade" | "sticky" | "haywire" | "beacon"));
    let straight = matches!(b, Some("bolt" | "laser" | "bullet" | "fatray" | "towcable" | "grapplinghook"));
    let mut speed = 0.0;
    let mut gravity = 0.0;
    if b == Some("missile") {
        speed = 3e8;
    } else if straight || falling {
        speed = f(velocity.unwrap_or(if b == Some("shell") {35.0} else {150.0}));
    }
    if falling {
        gravity = f(f(scale.unwrap_or(1.0)) * f(-9.8));
    }
    (speed, gravity)
}

/// `ballisticLead(p, v, origin, speed, gravity, out)`: the
/// corrected point and the solver's validity.
pub fn ballistic_lead(p: &Vector3, v: &Vector3, origin: &Vector3, speed: f64, gravity: f64) -> (Vector3, bool) {
    let mut out = Vector3 { x: f(p.x), y: f(p.y), z: f(p.z) };
    let s = f(speed);
    if !(s > f(0.0001)) {
        return (out, true);
    }
    let rx = f(p.x - origin.x);
    let ry = f(p.y - origin.y);
    let rz = f(p.z - origin.z);
    let g = f(gravity);
    let s2 = f(s * s);
    let (vx, vy, vz) = (f(v.x), f(v.y), f(v.z));
    if g == 0.0 {
        let b = f(-dot32(rx, ry, rz, vx, vy, vz));
        let c = dot32(rx, ry, rz, rx, ry, rz);
        let b2 = f(b * b);
        let vv = dot32(vx, vy, vz, vx, vy, vz);
        let d = f(b2 - f(f(vv - s2) * c));
        let den = if d > 0.0 { f(f(v8math::sqrt(d)) + b) } else { b };
        let t = f(c / den);
        if t > 0.0 {
            out.x = f(out.x + f(t * vx));
            out.y = f(out.y + f(t * vy));
            out.z = f(out.z + f(t * vz));
        }
        return (out, true);
    }
    let mut valid = true;
    let g2 = f(g * g);
    let k = f(f(ry * g) + s2);
    let k2 = f(k * k);
    let d = f(k2 - f(length_sq32(rx, ry, rz) * g2));
    let mut h = k;
    if d > 0.0 {
        h = f(k - f(v8math::sqrt(d)));
    } else {
        valid = false;
    }
    let t2 = f(f(h + h) / g2);
    if !(t2 > 0.0) {
        return (out, false);
    }
    out.y = f(out.y + f(f(g * -0.5) * t2));
    let t = f(v8math::sqrt(t2));
    out.x = f(out.x + f(t * vx));
    out.y = f(out.y + f(t * vy));
    out.z = f(out.z + f(t * vz));
    (out, valid)
}

/// `leadPoint(h, shooter, target, weapon, out)`: the point a
/// shooter leads `target` to; `weapon` the selected weapon's def (None: no
/// projectile correction).
pub fn lead_point<W: World + ?Sized>(w: &mut W, h: f64, shooter: Obj, target: Obj, weapon: Option<&JsObj>) -> (Vector3, bool) {
    let mut lead_p = w.aim_point(target);
    let zero = Vector3::default();
    let tv = w.unit_velocity(target).unwrap_or(zero);
    let sv = w.unit_velocity(shooter).unwrap_or(zero);
    // leadRel.subVectors(tv, sv).multiplyScalar(f(h))
    let fh = f(h);
    let rel = Vector3 { x: (tv.x - sv.x) * fh, y: (tv.y - sv.y) * fh, z: (tv.z - sv.z) * fh };
    lead_p = Vector3 { x: f(lead_p.x + f(rel.x)), y: f(lead_p.y + f(rel.y)), z: f(lead_p.z + f(rel.z)) };
    let Some(def) = weapon else { return (lead_p, true) };
    let origin = w.origin(shooter);
    let (speed, gravity) = projectile_of(Some(def));
    ballistic_lead(&lead_p, &tv, &origin, speed, gravity)
}

/// `dropCorrect(point, shooter, weapon, out)`.
pub fn drop_correct<W: World + ?Sized>(w: &mut W, point: &Vector3, shooter: Obj, weapon: &JsObj) -> (Vector3, bool) {
    let origin = w.origin(shooter);
    let (speed, gravity) = projectile_of(Some(weapon));
    ballistic_lead(point, &Vector3::default(), &origin, speed, gravity)
}

/// `selectedWeapon(unit)`: the arsenal's current weapon of the
/// unit's controller's channel, else of channel 0 (the arsenal index).
pub fn selected_weapon_aim<W: World + ?Sized>(w: &W, unit: Obj) -> Option<usize> {
    let controller = w.unit_controller(unit);
    if !w.has_arsenal(unit) {return None;}
    let channel = controller.and_then(|c| w.ctl(c).perception.as_ref().map(|p| p.fire.channel)).unwrap_or(0.0);
    w.current_weapon(unit,channel).or_else(||w.current_weapon(unit,0.))
}

/// The def of a unit's weapon at `index`.
pub fn weapon_def<W: World + ?Sized>(w: &W, unit: Obj, index: usize) -> Rc<JsObj> {
    w.weapon_definition(unit,index).expect("arsenal member")
}

/// Target motion events that widen the 2005 aim-error sphere on rising edge.
pub const EVASION_JUMP: u8 = 1;
pub const EVASION_ROLL: u8 = 2;
pub const EVASION_SPRINT: u8 = 4;
pub const EVASION_JET: u8 = 8;

/// Evasion budget cap: limits how much sudden motion can widen the miss sphere in a burst.
const EVASION_BUDGET_CAP: f64 = 5.0;
/// Maximum stored evasion factor before speed scaling is applied.
const EVASION_MAX: f64 = 2.0;
/// Evasion spikes added on the rising edge of jump, roll, sprint, and jet actions.
const SPIKE_JUMP: f64 = 2.0;
const SPIKE_ROLL: f64 = 2.0;
const SPIKE_SPRINT: f64 = 1.0;
const SPIKE_JET: f64 = 5.0;
/// Velocity-delta multiplier converting speed changes into evasion growth.
const VELOCITY_DELTA_EVASION: f64 = 0.455;
/// Speed-ratio multiplier widening the miss sphere while the target is moving.
const SPEED_MOTION_SCALE: f64 = 0.6;
/// Fraction of the miss-sphere offset applied when the target is a bot rather than a human.
const BOT_TARGET_MISS_SCALE: f64 = 0.3;
/// Reference distance (m) and upper cap for distance scaling of the miss-sphere offset.
const MISS_REF_DISTANCE: f64 = 20.0;
const MISS_MAX_DISTANCE_SCALE: f64 = 10.0;

/// Per-target aim-error sphere used by 2005 bots.
///
/// Tracks a slowly drifting unit direction (integrated as a unit quaternion so
/// aim error sweeps smoothly around the target) and a radius driven by the
/// target's size, speed, velocity changes, and discrete evasion actions (jump,
/// roll, sprint, jet), bounded by a refilling budget and the opposing team's
/// AI skill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimMiss {
    pub prev_vel: Vector3,
    pub evasion: f64,
    pub budget: f64,
    pub radius: f64,
    pub axis: Vector3,
    pub quat: [f64; 4],
    pub dir: Vector3,
    pub prev_motion_flags: u8,
}

impl Default for AimMiss {
    fn default() -> Self {
        Self {
            prev_vel: Vector3::default(),
            evasion: 0.0,
            budget: 0.0,
            radius: 0.0,
            axis: Vector3::default(),
            quat: [0.0, 0.0, 0.0, 1.0],
            dir: Vector3::default(),
            prev_motion_flags: 0,
        }
    }
}

fn swbf2_skill_lerp(skill: f64, lo: f64, hi: f64) -> f64 {
    let k = v8math::min(v8math::max(v8math::trunc(skill), 1.0), 20.0);
    let t = f((k - 1.0) / 19.0);
    f(f(lo) + f(t * f(f(hi) - f(lo))))
}

fn quat_mul_norm(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let x = f(f(f(a[3] * b[0]) + f(a[0] * b[3])) + f(f(a[1] * b[2]) - f(a[2] * b[1])));
    let y = f(f(f(a[3] * b[1]) - f(a[0] * b[2])) + f(f(a[1] * b[3]) + f(a[2] * b[0])));
    let z = f(f(f(a[3] * b[2]) + f(a[0] * b[1])) - f(f(a[1] * b[0]) - f(a[2] * b[3])));
    let w = f(f(f(a[3] * b[3]) - f(a[0] * b[0])) - f(f(a[1] * b[1]) + f(a[2] * b[2])));
    let n2 = f(f(f(x * x) + f(y * y)) + f(f(z * z) + f(w * w)));
    if n2 > 0.0 {
        let inv = f(1.0 / f(v8math::sqrt(n2)));
        [f(x * inv), f(y * inv), f(z * inv), f(w * inv)]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

fn quat_rotate_z(q: [f64; 4]) -> Vector3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    Vector3 {
        x: f(2.0 * f(f(x * z) + f(w * y))),
        y: f(2.0 * f(f(y * z) - f(w * x))),
        z: f(1.0 - f(2.0 * f(f(x * x) + f(y * y)))),
    }
}

impl AimMiss {
    fn add_evasion(&mut self, mut amount: f64, opp_skill: f64) {
        if amount > 0.0 {
            let take = v8math::min(amount, self.budget);
            self.budget = f(self.budget - take);
            amount = take;
        }
        // Higher opposing skill lowers the resting evasion floor from 1.6 down to 0.4.
        let floor = swbf2_skill_lerp(opp_skill, 1.6, 0.4);
        self.evasion = v8math::min(v8math::max(f(self.evasion + amount), floor), EVASION_MAX);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        dt: f64,
        vel: &Vector3,
        max_speed: f64,
        bound_radius: f64,
        kind: f64,
        is_vehicle: bool,
        opp_skill: f64,
        rand_box: (f64, f64, f64),
        motion_flags: u8,
    ) {
        let dt = f(dt);
        if !(dt > 0.0) {
            return;
        }
        // Evasion decays faster against higher-skilled opponents (halved for vehicles),
        // while half the decay refills the evasion spike budget up to the cap.
        let mut rate = f(swbf2_skill_lerp(opp_skill, -0.1, -1.1) * dt);
        if is_vehicle {
            rate = f(rate * 0.5);
        }
        self.add_evasion(rate, opp_skill);
        self.budget = v8math::min(v8math::max(f(self.budget - f(rate * 0.5)), 0.0), EVASION_BUDGET_CAP);
        // Discrete evasion actions boost the miss radius once on their rising edge.
        let started = motion_flags & !self.prev_motion_flags;
        self.prev_motion_flags = motion_flags;
        if (started & EVASION_JUMP) != 0 {
            self.add_evasion(SPIKE_JUMP, opp_skill);
        }
        if (started & EVASION_ROLL) != 0 {
            self.add_evasion(SPIKE_ROLL, opp_skill);
        }
        if (started & EVASION_SPRINT) != 0 {
            self.add_evasion(SPIKE_SPRINT, opp_skill);
        }
        if (started & EVASION_JET) != 0 {
            self.add_evasion(SPIKE_JET, opp_skill);
        }
        // Sudden changes in velocity relative to top speed also widen the sphere.
        let ms = v8math::max(f(0.1), f(max_speed));
        let dv = f(v8math::sqrt(length_sq32(
            f(self.prev_vel.x - f(vel.x)),
            f(self.prev_vel.y - f(vel.y)),
            f(self.prev_vel.z - f(vel.z)),
        )));
        self.prev_vel = *vel;
        self.add_evasion(f(f(dv * f(VELOCITY_DELTA_EVASION)) / ms), opp_skill);

        // Perturb the rotation axis and advance the error direction smoothly on the unit sphere.
        let axis = normalize_d3dx(
            f(self.axis.x + f(rand_box.0)),
            f(self.axis.y + f(rand_box.1)),
            f(self.axis.z + f(rand_box.2)),
        );
        self.axis = Vector3 { x: axis.x, y: axis.y, z: axis.z };
        let (s, c) = v8math::sin_cos(dt);
        let dq = [f(axis.x * f(s)), f(axis.y * f(s)), f(axis.z * f(s)), f(c)];
        self.quat = quat_mul_norm(self.quat, dq);
        self.dir = quat_rotate_z(self.quat);

        // Scale the target's bounding diameter by speed-amplified evasion, capped by skill.
        let speed = f(v8math::sqrt(length_sq32(f(vel.x), f(vel.y), f(vel.z))));
        let motion = f(1.0 + f(f(speed / ms) * f(SPEED_MOTION_SCALE)));
        let mut bound = f(f(bound_radius) * 2.0);
        if kind == 7.0 {
            bound = f(bound * f(0.4));
        } else if kind == 2.0 {
            bound = f(bound * 2.0);
        }
        let cap = if is_vehicle {
            swbf2_skill_lerp(opp_skill, 4.0, 1.2)
        } else {
            swbf2_skill_lerp(opp_skill, 2.2, 1.2)
        };
        let eff = v8math::min(f(self.evasion * motion), cap);
        self.radius = f(bound * eff);
    }
}

fn apply_aim_miss<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj, target: Obj, lead: &mut Vector3) {
    if w.unit_scalars(unit).human {
        return;
    }
    let Some(shooter_dir) = w.ctl(controller).perception.as_ref().map(|p| p.miss.dir) else { return };
    let Some(target_radius) = w.target_aim_miss_radius(target) else { return };
    if !(target_radius > 0.0) {
        return;
    }
    let sp = w.unit_scalars(unit).position;
    let (_, shooter_max_speed) = w.unit_weapon_motion(unit);
    let sv = w.unit_velocity(unit).unwrap_or_default();
    let speed_sq = length_sq32(f(sv.x), f(sv.y), f(sv.z));
    // Shooters moving near top speed project the error perpendicular to the sight line.
    let thresh = f(f(shooter_max_speed) * f(0.95));
    let rx = f(lead.x - sp.x);
    let ry = f(lead.y - sp.y);
    let rz = f(lead.z - sp.z);
    let dir = if speed_sq > f(thresh * thresh) {
        let xy = f(v8math::sqrt(f(f(shooter_dir.x * shooter_dir.x) + f(shooter_dir.y * shooter_dir.y))));
        let (sx, sy) = if xy > 0.0 { (f(shooter_dir.x / xy), f(shooter_dir.y / xy)) } else { (1.0, 0.0) };
        let right = normalize_d3dx(f(-rz), 0.0, rx);
        let up = normalize_d3dx(
            f(f(right.z * ry) - f(right.y * rz)),
            f(f(right.x * rz) - f(right.z * rx)),
            f(f(right.y * rx) - f(right.x * ry)),
        );
        Vector3 {
            x: f(f(right.x * sx) + f(up.x * sy)),
            y: f(f(right.y * sx) + f(up.y * sy)),
            z: f(f(right.z * sx) + f(up.z * sy)),
        }
    } else {
        shooter_dir
    };
    let mut ox = f(dir.x * target_radius);
    let mut oy = f(dir.y * target_radius);
    let mut oz = f(dir.z * target_radius);
    let target_scalars = w.unit_scalars(target);
    // Keep non-flyer vertical aim error above the ground plane within a bounded upward band.
    if target_scalars.kind != 5.0 {
        let hi = draw_range(w, Gen::A, 2.0, 3.0);
        oy = v8math::min(v8math::max(oy, 0.0), hi);
    }
    // Bot targets receive 30% of the error sphere; human targets receive full scale.
    if !target_scalars.human {
        let bot_scale = f(BOT_TARGET_MISS_SCALE);
        ox = f(ox * bot_scale);
        oy = f(oy * bot_scale);
        oz = f(oz * bot_scale);
    }
    let dist = f(v8math::sqrt(length_sq32(rx, ry, rz)));
    let ref_dist = f(MISS_REF_DISTANCE);
    let max_scale = f(MISS_MAX_DISTANCE_SCALE);
    let raw = f(f(dist / ref_dist) + f(0.5));
    let scale = if dist >= f(ref_dist * f(0.5)) {
        v8math::min(v8math::max(raw, 1.0), max_scale)
    } else {
        raw
    };
    lead.x = f(lead.x + f(ox * scale));
    lead.y = f(lead.y + f(oy * scale));
    lead.z = f(lead.z + f(oz * scale));
}

/// `aimPointFor(controller, unit, dt, out)`: the point the
/// body turns toward for aim states 0 and 1 (None: the steering states).
pub fn aim_point_for<W: World + ?Sized>(w: &mut W, controller: Obj, unit: Obj, dt: f64) -> Option<Vector3> {
    let (state, point, aim_target, target) = {
        let c = w.ctl(controller);
        (c.aim_state, c.aim_point, c.aim_target, c.target)
    };
    if state == 1.0 {
        return Some(point);
    }
    if state != 0.0 {
        return None;
    }
    let target = resolve(w, aim_target.or(target));
    let weapon = selected_weapon_aim(w, unit);
    if let (Some(target), Some(index)) = (target, weapon) {
        let def = weapon_def(w, unit, index);
        let (mut p, _) = lead_point(w, dt, unit, target, Some(&def));
        if w.uses_swbf2() {
            apply_aim_miss(w, controller, unit, target, &mut p);
        }
        return Some(p);
    }
    let o = w.origin(unit);
    let fwd = w.forward(unit);
    Some(Vector3 { x: o.x + fwd.x * 30.0, y: o.y + fwd.y * 30.0, z: o.z + fwd.z * 30.0 })
}

/// A weak reference's unit while it is the same life and alive
/// (perception.js:318 and every `resolve` of these files).
pub fn resolve<W: World + ?Sized>(w: &W, r: Option<super::world::Ref>) -> Option<Obj> {
    let r = r?;
    let (alive,generation) = w.unit_life(r.obj);
    if alive && generation == r.generation {
        Some(r.obj)
    } else {
        None
    }
}

/// `x ?? d` on a definition field read as a number (`Js::nn`).
pub fn js_nn(v: &Js, d: f64) -> f64 {
    v.nn(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medium_curves() {
        let c = skill_curves(14.0);
        assert_eq!(c.a, f(1.9));
        assert_eq!(c.d, f(1.9));
        assert_eq!(c.b, Some(f(1.6)));
        assert_eq!(c.s, Some(f(1.0)));
    }

    #[test]
    fn aim_miss_sphere_tracks_motion_skill_and_stays_unit_length() {
        let mut low = AimMiss::default();
        let mut high = AimMiss::default();
        let walk = Vector3 { x: 4.5, y: 0.0, z: 0.0 };
        for _ in 0..30 {
            low.step(1.0 / 60.0, &walk, 6.0, 1.4, 0.0, false, 4.0, (0.3, -0.2, 0.4), 0);
            high.step(1.0 / 60.0, &walk, 6.0, 1.4, 0.0, false, 18.0, (0.3, -0.2, 0.4), 0);
        }
        let n2 = low.dir.x * low.dir.x + low.dir.y * low.dir.y + low.dir.z * low.dir.z;
        assert!((n2 - 1.0).abs() < 1e-4, "aim miss direction should stay normalized: {n2}");
        assert!(low.radius > high.radius, "lower opposing skill should keep a wider miss radius: {} vs {}", low.radius, high.radius);
        let calm = high.radius;
        high.step(1.0 / 60.0, &Vector3 { x: 0.0, y: 8.0, z: 0.0 }, 6.0, 1.4, 0.0, false, 18.0, (0.1, 0.2, -0.3), 0);
        assert!(high.radius > calm, "sudden jump velocity change should widen miss radius: {} vs {}", high.radius, calm);

        // Rising-edge jump/roll/sprint/jet spikes spend evasion budget once per transition.
        let mut spike = AimMiss::default();
        for _ in 0..120 {
            spike.step(1.0 / 60.0, &walk, 6.0, 1.4, 0.0, false, 18.0, (0.0, 0.0, 0.0), 0);
        }
        let before = spike.evasion;
        spike.step(1.0 / 60.0, &walk, 6.0, 1.4, 0.0, false, 18.0, (0.0, 0.0, 0.0), EVASION_JUMP);
        let after_jump = spike.evasion;
        assert!(after_jump > before, "jump rising edge should boost evasion: {before} -> {after_jump}");
        let budget_after_jump = spike.budget;
        spike.step(1.0 / 60.0, &walk, 6.0, 1.4, 0.0, false, 18.0, (0.0, 0.0, 0.0), EVASION_JUMP);
        assert!(spike.budget >= budget_after_jump, "holding jump should not re-spend evasion budget");
    }

    struct AimMissTestWorld {
        shooter: super::super::world::UnitScalars,
        target: super::super::world::UnitScalars,
        shooter_ctl: super::super::world::Ctl,
        target_ctl: Option<super::super::world::Ctl>,
        human_radius: Option<f64>,
    }

    impl super::super::world::Draws for AimMissTestWorld {
        fn uniform_a(&mut self) -> f64 { 0.5 }
        fn uniform_b(&mut self) -> f64 { 0.5 }
        fn uniform_c(&mut self) -> f64 { 0.5 }
    }

    impl World for AimMissTestWorld {
        fn uses_swbf2(&self) -> bool { true }
        fn target_aim_miss_radius(&self, target: Obj) -> Option<f64> {
            if let Some(c) = self.unit_controller(target) {
                return self.ctl(c).perception.as_ref().map(|p| p.miss.radius);
            }
            (target == 2).then_some(self.human_radius).flatten()
        }
        fn time(&self) -> f64 { 1.0 }
        fn difficulty(&self) -> super::super::world::DifficultyView { Default::default() }
        fn mission_stay_in_turrets(&self) -> bool { false }
        fn mission_no_jet_obstacle_test(&self) -> bool { false }
        fn ai_stay_in_turrets(&mut self) -> bool { false }
        fn unit(&self, _u: Obj) -> super::super::world::UnitView { Default::default() }
        fn unit_weapon_motion(&self, _u: Obj) -> (f64, f64) { (0.0, 7.0) }
        fn unit_scalars(&self, u: Obj) -> super::super::world::UnitScalars {
            if u == 1 { self.shooter } else { self.target }
        }
        fn unit_velocity(&self, _u: Obj) -> Option<Vector3> { Some(Vector3::default()) }
        fn unit_controller(&self, u: Obj) -> Option<Obj> {
            if u == 1 { Some(10) } else { self.target_ctl.as_ref().map(|_| 20) }
        }
        fn has_arsenal(&self, _u: Obj) -> bool { false }
        fn weapon_count(&self, _u: Obj) -> usize { 0 }
        fn current_weapon(&self, _u: Obj, _channel: f64) -> Option<usize> { None }
        fn weapon(&self, _u: Obj, _index: usize) -> Option<super::super::world::WeaponView> { None }
        fn body_lease_count(&self, _u: Obj) -> usize { 0 }
        fn body_lease(&self, _u: Obj, _index: usize) -> Option<super::super::world::LeaseView> { None }
        fn weapon_settings(&self, _u: Obj, _index: usize) -> super::super::world::WeaponSettings { unimplemented!() }
        fn edit_weapon<R>(&mut self, _u: Obj, _index: usize, _edit: impl FnOnce(&mut super::super::world::WeaponSettings) -> R) -> R { unimplemented!() }
        fn origin(&mut self, _u: Obj) -> Vector3 { Vector3::default() }
        fn forward(&mut self, _u: Obj) -> Vector3 { Vector3 { x: 1.0, y: 0.0, z: 0.0 } }
        fn aim_point(&mut self, _u: Obj) -> Vector3 { Vector3::default() }
        fn sphere(&mut self, _u: Obj) -> super::super::world::Sphere { Default::default() }
        fn set_intent_buttons(&mut self, _u: Obj, _b0: f64, _b1: f64) {}
        fn set_intent_aim(&mut self, _u: Obj, _aim: Option<Vector3>) {}
        fn arsenal_select(&mut self, _u: Obj, _index: usize) -> bool { false }
        fn use_classic_random(&mut self, _u: Obj) {}
        fn object_buffer(&self) -> crate::ai::owned::scratch::List<Obj> { unimplemented!() }
        fn query(&mut self, _center: &Vector3, _radius: f64, _filter: super::super::world::QueryFilter) -> crate::ai::owned::scratch::List<Obj> { unimplemented!() }
        fn relation(&mut self, _a: f64, _b: f64) -> f64 { -1.0 }
        fn living(&mut self) -> crate::ai::owned::scratch::List<Obj> { unimplemented!() }
        fn los(&mut self, _from: &Vector3, _to: &Vector3, _a: Option<Obj>, _b: Option<Obj>) -> bool { true }
        fn curves_for(&mut self, _team: f64) -> Option<Curves> { None }
        fn character_team(&self, _character: Obj) -> f64 { 1.0 }
        fn fresh_perception(&mut self, _c: Obj) -> super::super::perception::Perception { Default::default() }
        fn ctl(&self, c: Obj) -> &super::super::world::Ctl {
            if c == 10 { &self.shooter_ctl } else { self.target_ctl.as_ref().unwrap() }
        }
        fn ctl_mut(&mut self, c: Obj) -> &mut super::super::world::Ctl {
            if c == 10 { &mut self.shooter_ctl } else { self.target_ctl.as_mut().unwrap() }
        }
        fn agent_controller(&self, _agent: Obj) -> Obj { 10 }
        fn tracker(&mut self, _agent: Obj) -> Option<&mut super::super::perception::TargetTracker> { None }
        fn is_controller(&self, _c: Obj) -> bool { true }
        fn agent_accepts_alert(&mut self, _agent: Obj, _known: bool, _point: &Vector3) -> bool { false }
        fn agent_accepts_hazard(&mut self, _agent: Obj) -> bool { false }
        fn agent_accepts_acoustic(&mut self, _agent: Obj, _source: Obj) -> bool { false }
        fn shared_target(&mut self, _controller: Obj) -> Option<Obj> { None }
        fn has_vehicle_dispatch(&self) -> bool { false }
        fn vehicle_dispatch(&mut self, _shared: &mut super::super::weapons_ai::Shared, _controller: Obj, _unit: Obj, _target: Obj, _switch_allowed: bool) -> super::super::world::Engagement { Default::default() }
        fn has_mount_reach(&self) -> bool { false }
        fn mount_reach(&mut self, _unit: Obj, _point: &Vector3) -> bool { false }
        fn current_controllable(&mut self, _character: Obj) -> (bool, Option<Obj>) { (false, None) }
        fn character_remote(&self, _character: Obj) -> Option<Obj> { None }
        fn character_unit(&self, _character: Obj) -> Option<Obj> { None }
        fn is_unit(&self, _o: Obj) -> bool { true }
        fn enter_pattern(&mut self, _agent: Obj, _state: f64) {}
        fn exit_pattern(&mut self, _agent: Obj, _state: f64) {}
        fn tick_pattern(&mut self, _agent: Obj, _state: f64, _dt: f64) {}
        fn sidestep(&mut self, _agent: Obj) {}
        fn cover_hints(&mut self, _center: &Vector3, _half: f64) -> crate::ai::owned::scratch::List<super::super::world::HintView> { unimplemented!() }
        fn has_jet_hints(&self) -> bool { false }
        fn jet_hints_excluded(&mut self, _controller: Obj) -> bool { false }
        fn jet_hints_find(&mut self, _controller: Obj) -> Option<Obj> { None }
        fn jet_hints_enter(&mut self, _controller: Obj, _hint: Option<Obj>) {}
        fn jet_hints_exit(&mut self, _controller: Obj, _hint: Option<Obj>) {}
        fn jet_hints_tick(&mut self, _controller: Obj, _hint: Option<Obj>, _dt: f64) -> bool { true }
        fn nav_point_clear(&mut self, _point: &Vector3, _unit: Obj) -> Option<bool> { None }
        fn nav_segment_blocked(&mut self, _from: &Vector3, _to: &Vector3, _unit: Obj) -> Option<bool> { None }
        fn nav_direct_clear(&mut self, _unit: Obj, _point: &Vector3) -> Option<bool> { None }
        fn nav_in_danger_region(&mut self, _unit: Obj) -> Option<bool> { None }
        fn agent_sync(&mut self, _agent: &super::super::agent::CombatAgent) {}
        fn agent_resync(&mut self, _agent: &mut super::super::agent::CombatAgent) {}
        fn interrupts_tick(&mut self, _agent: Obj, _combat: &mut super::super::binding::CombatAi) -> bool { false }
        fn agent_board_policy(&mut self, _agent: Obj, _candidate: Obj) -> bool { false }
        fn has_helper(&self, _agent: Obj) -> bool { false }
        fn helper_arrived(&mut self, _agent: Obj) -> bool { false }
        fn helper_flying(&mut self, _agent: Obj) -> bool { false }
        fn helper_has_follow(&self, _agent: Obj) -> bool { false }
        fn helper_goto(&mut self, _agent: Obj, _point: &Vector3, _mode: Option<super::super::world::GotoMode>) {}
        fn helper_stop(&mut self, _agent: Obj) {}
        fn helper_tick(&mut self, _agent: Obj, _dt: f64) {}
        fn helper_follow(&mut self, _agent: Obj, _target: Obj) {}
        fn helper_terminate(&mut self, _agent: Obj) {}
        fn request_jet(&mut self, _controller: Obj, _vector: &Vector3) {}
        fn emit_vo(&mut self, _unit: Obj, _event: &str, _property: &str) {}
    }

    #[test]
    fn apply_aim_miss_applies_full_scale_to_humans_and_three_tenths_to_bots() {
        let mut shooter_p = super::super::perception::Perception::default();
        shooter_p.miss.dir = Vector3 { x: 1.0, y: 0.0, z: 0.0 };
        let mut target_p = super::super::perception::Perception::default();
        target_p.miss.radius = 2.0;

        // Human target has no bot controller and receives the full miss-sphere offset.
        let mut human_w = AimMissTestWorld {
            shooter: super::super::world::UnitScalars { alive: true, position: Vector3::default(), ..Default::default() },
            target: super::super::world::UnitScalars { alive: true, human: true, position: Vector3 { x: 20.0, y: 0.0, z: 0.0 }, ..Default::default() },
            shooter_ctl: super::super::world::Ctl { perception: Some(shooter_p.clone()), ..Default::default() },
            target_ctl: None,
            human_radius: Some(2.0),
        };
        let mut human_lead = Vector3 { x: 20.0, y: 0.0, z: 0.0 };
        apply_aim_miss(&mut human_w, 10, 1, 2, &mut human_lead);
        let human_dx = human_lead.x - 20.0;
        assert!((human_dx - 3.0).abs() < 1e-4, "human target should get full sphere offset (2.0 * 1.5 = 3.0), got {human_dx}");

        // Bot target with the same radius receives 0.3 of the human offset.
        let mut bot_w = AimMissTestWorld {
            shooter: human_w.shooter,
            target: super::super::world::UnitScalars { human: false, ..human_w.target },
            shooter_ctl: super::super::world::Ctl { perception: Some(shooter_p), ..Default::default() },
            target_ctl: Some(super::super::world::Ctl { perception: Some(target_p), ..Default::default() }),
            human_radius: None,
        };
        let mut bot_lead = Vector3 { x: 20.0, y: 0.0, z: 0.0 };
        apply_aim_miss(&mut bot_w, 10, 1, 2, &mut bot_lead);
        let bot_dx = bot_lead.x - 20.0;
        assert!((bot_dx - 0.9).abs() < 1e-4 && (bot_dx - human_dx * 0.3).abs() < 1e-4, "bot offset {bot_dx} should be 0.3x human offset {human_dx}");

        // Human shooters never receive bot aim-error offsets.
        human_w.shooter.human = true;
        let mut unchanged = Vector3 { x: 20.0, y: 0.0, z: 0.0 };
        apply_aim_miss(&mut human_w, 10, 1, 2, &mut unchanged);
        assert_eq!(unchanged.x, 20.0);
    }
}
