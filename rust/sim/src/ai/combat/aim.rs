//! Classic aiming (src/ai/classic/aim.js): the direction filter every
//! bot's gun sits behind, the curves it reads, the lead it aims with, and
//! the point its body turns toward. Operation for operation in float32 as
//! the JavaScript writes it (`Math.fround` at every step it rounds).

use std::rc::Rc;
use three::Vector3;

use super::math::{dot32, f, length_sq32, normalize_d3dx};
use super::world::{Curves, Obj, World};
use crate::combat::defs::{Js, JsObj};

/// aim.js:162-163: cos and the angle of 30 degrees, float32.
fn theta_30() -> f64 {
    f(0.5235987901687622)
}
fn cos_30() -> f64 {
    f(v8math::cos(theta_30()))
}

/// A direction or rate as the filter keeps it ({x, y, z}).
pub type V3 = Vector3;

/// aim.js:165 `filterDirection(d, q, v, dt, A, D, out)`: v is updated in
/// place; returns the new direction (the caller's `out`).
pub fn filter_direction(d: &Vector3, q: &Vector3, v: &mut Vector3, dt: f64, a: f64, damp: f64) -> Vector3 {
    let dt = f(dt);
    let a = f(a);
    let damp = f(damp);
    let (dx, dy, dz) = (f(d.x), f(d.y), f(d.z));
    let (mut qx, mut qy, mut qz) = (f(q.x), f(q.y), f(q.z));
    let (mut vx, mut vy, mut vz) = (f(v.x), f(v.y), f(v.z));
    // 1. The 30-degree correction (aim.js:174).
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
// Weapon classes as the filter and firing code test them (aim.js:216).

/// `def.base` as a string.
pub fn base_of(def: &JsObj) -> Option<&str> {
    def.get("base").str()
}

/// aim.js:218 `isCannon`.
pub fn is_cannon(def: Option<&JsObj>) -> bool {
    matches!(def.and_then(base_of), Some("cannon" | "weapon" | "launcher" | "grapplinghookweapon"))
}

/// aim.js:222 `isLauncher`.
pub fn is_launcher(def: Option<&JsObj>) -> bool {
    def.and_then(base_of) == Some("launcher")
}

/// aim.js:233 `skillCurves(skill)`: each curve through its anchors at
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
    /// aim.js:271: `{ A: 0, D: 0, B: 1, S: 0 }`.
    fn default() -> Self {
        AimCurves { a: 0.0, d: 0.0, b: 1.0, s: 0.0 }
    }
}

/// aim.js:247 `aimCurves(curves, weaponDef, out)`: Medium's when the curves
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

/// aim.js:266 `Aimer`: the filtered direction a bot's weapons fire along.
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
    /// aim.js:275 `reset(direction)`.
    pub fn reset(&mut self, direction: &Vector3) {
        self.direction = *direction;
        self.velocity = Vector3::default();
        self.started = true;
    }

    /// aim.js:283 `update(desired, dt, curves, weapon, hero)`: `weapon` is
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
// Projectile speed and gravity (aim.js:313).

/// aim.js:316 `projectileOf(def)`: (speed, gravity).
pub fn projectile_of(def: Option<&JsObj>) -> (f64, f64) {
    let Some(def) = def else { return (0.0, 0.0) };
    let ordnance = def.get("ordnance");
    if !ordnance.truthy() {
        return (0.0, 0.0);
    }
    let base = ordnance.get("base");
    let b = base.str();
    let falling = matches!(b, Some("shell" | "grenade" | "sticky" | "haywire" | "beacon"));
    let straight = matches!(b, Some("bolt" | "laser" | "bullet" | "fatray" | "towcable" | "grapplinghook"));
    let mut speed = 0.0;
    let mut gravity = 0.0;
    if b == Some("missile") {
        speed = 3e8;
    } else if straight || falling {
        let v = ordnance.get("Velocity");
        speed = f(if v.is_nullish() { if b == Some("shell") { 35.0 } else { 150.0 } } else { v.num() });
    }
    if falling {
        let g = ordnance.get("Gravity");
        gravity = f(f(g.nn(1.0)) * f(-9.8));
    }
    (speed, gravity)
}

/// aim.js:338 `ballisticLead(p, v, origin, speed, gravity, out)`: the
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

/// aim.js:390 `leadPoint(h, shooter, target, weapon, out)`: the point a
/// shooter leads `target` to; `weapon` the selected weapon's def (None: no
/// projectile correction).
pub fn lead_point<W: World + ?Sized>(w: &mut W, h: f64, shooter: Obj, target: Obj, weapon: Option<&JsObj>) -> (Vector3, bool) {
    let mut lead_p = w.aim_point(target);
    let zero = Vector3::default();
    let tv = w.unit(target).velocity.unwrap_or(zero);
    let sv = w.unit(shooter).velocity.unwrap_or(zero);
    // leadRel.subVectors(tv, sv).multiplyScalar(f(h))
    let fh = f(h);
    let rel = Vector3 { x: (tv.x - sv.x) * fh, y: (tv.y - sv.y) * fh, z: (tv.z - sv.z) * fh };
    lead_p = Vector3 { x: f(lead_p.x + f(rel.x)), y: f(lead_p.y + f(rel.y)), z: f(lead_p.z + f(rel.z)) };
    let Some(def) = weapon else { return (lead_p, true) };
    let origin = w.origin(shooter);
    let (speed, gravity) = projectile_of(Some(def));
    ballistic_lead(&lead_p, &tv, &origin, speed, gravity)
}

/// aim.js:407 `dropCorrect(point, shooter, weapon, out)`.
pub fn drop_correct<W: World + ?Sized>(w: &mut W, point: &Vector3, shooter: Obj, weapon: &JsObj) -> (Vector3, bool) {
    let origin = w.origin(shooter);
    let (speed, gravity) = projectile_of(Some(weapon));
    ballistic_lead(point, &Vector3::default(), &origin, speed, gravity)
}

/// aim.js:443 `selectedWeapon(unit)`: the arsenal's current weapon of the
/// unit's controller's channel, else of channel 0 (the arsenal index).
pub fn selected_weapon_aim<W: World + ?Sized>(w: &W, unit: Obj) -> Option<usize> {
    let u = w.unit(unit);
    let arsenal = u.arsenal.as_ref()?;
    let channel = u.controller.and_then(|c| w.ctl(c).perception.as_ref().map(|p| p.fire.channel)).unwrap_or(0.0);
    arsenal.current(channel).or_else(|| arsenal.current(0.0))
}

/// The def of a unit's weapon at `index`.
pub fn weapon_def<W: World + ?Sized>(w: &W, unit: Obj, index: usize) -> Rc<JsObj> {
    w.unit(unit).arsenal.as_ref().expect("an arsenal").weapons[index].def.clone()
}

/// aim.js:423 `aimPointFor(controller, unit, dt, out)`: the point the
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
        let (p, _) = lead_point(w, dt, unit, target, Some(&def));
        return Some(p);
    }
    let o = w.origin(unit);
    let fwd = w.forward(unit);
    Some(Vector3 { x: o.x + fwd.x * 30.0, y: o.y + fwd.y * 30.0, z: o.z + fwd.z * 30.0 })
}

/// A weak reference's unit while it is the same life and alive
/// (aim.js:437, perception.js:318 and every `resolve` of these files).
pub fn resolve<W: World + ?Sized>(w: &W, r: Option<super::world::Ref>) -> Option<Obj> {
    let r = r?;
    let u = w.unit(r.obj);
    if u.alive && u.generation == r.generation {
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
}
