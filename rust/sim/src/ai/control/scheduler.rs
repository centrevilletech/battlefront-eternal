//! The scheduler's data and its pure geometry (scheduler.js): the
//! candidate budget, the camera gate, and the degraded analog
//! conversion. The scheduling itself is the core's (mod.rs).

use super::world::{AiUnit, CameraView};
use super::super::nav::math::Vec3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SchedulerStats {
    pub serviced: f64,
    pub skipped: f64,
    pub considered: f64,
    pub budget: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scheduler {
    pub counter: f64,
    pub parity: f64,
    pub input_parity: f64,
    pub skips: f64,
    pub stats: SchedulerStats,
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler { counter: 0.0, parity: 0.0, input_parity: 0.0, skips: 0.0, stats: SchedulerStats::default() }
    }
}

/// candidateBudget (scheduler.js:38).
pub fn candidate_budget(count: f64, cycle: f64, step: f64) -> f64 {
    if step == cycle {
        return count;
    }
    v8math::floor(count / cycle) + if cycle - step < js_rem(count, cycle) { 1.0 } else { 0.0 }
}

/// JavaScript's % (truncated remainder).
pub fn js_rem(a: f64, b: f64) -> f64 {
    a - v8math::trunc(a / b) * b
}

/// cameraAccepts (scheduler.js:52).
pub fn camera_accepts(camera: &CameraView, point: Vec3) -> bool {
    let e = &camera.elements;
    let scale = camera.zoom;
    let near = camera.near;
    let far = v8math::min(camera.far, 8.0 * scale + 70.0);
    let to = Vec3::new(point.x - e[12], point.y - e[13], point.z - e[14]);
    let depth = -super::vec::dot(to, Vec3::new(e[8], e[9], e[10]));
    if !(near <= depth && depth <= far) {
        return false;
    }
    let tan_v = v8math::tan((camera.fov * std::f64::consts::PI) / 360.0) / scale;
    let tan_h = tan_v * camera.aspect;
    if v8math::abs(super::vec::dot(to, Vec3::new(e[0], e[1], e[2]))) > tan_h * depth {
        return false;
    }
    if v8math::abs(super::vec::dot(to, Vec3::new(e[4], e[5], e[6]))) > tan_v * depth {
        return false;
    }
    true
}

/// fallbackAnalogs (scheduler.js:300): the degraded conversion,
/// writing (forward, right, turn, pitch).
pub fn fallback_analogs(unit: &AiUnit, request: &super::controller::MoveRequest, aim_point: Option<Vec3>, dt: f64, facing: bool) -> [f64; 4] {
    let mut out = [0.0; 4];
    let Some(s) = &unit.soldier else { return out };
    let p = unit.position;
    let yaw = s.aim_yaw.unwrap_or(0.0);
    let fx = v8math::sin(yaw);
    let fz = v8math::cos(yaw);
    if request.valid {
        let dx = request.steering_target.x - p.x;
        let dz = request.steering_target.z - p.z;
        if v8math::abs(dx) >= 1e-4 || v8math::abs(dz) >= 1e-4 {
            let length = v8math::hypot(dx, dz);
            let nx = dx / length;
            let nz = dz / length;
            let a = fx * nx + fz * nz;
            let b = -(fz * nx - fx * nz);
            let slow = v8math::min(v8math::max(length, 0.0), 1.0);
            let fc = match s.max_speed {
                Some(v) if v != 0.0 && !v.is_nan() => v,
                _ => 1.0,
            };
            let sc = s.max_strafe.unwrap_or(0.0);
            if sc > 1e-4 {
                let u = a / fc;
                let v = b / sc;
                let m0 = v8math::max(v8math::abs(u), v8math::abs(v));
                let m = if m0 == 0.0 || m0.is_nan() { 1.0 } else { m0 };
                out[0] = (u / m) * slow;
                out[1] = (v / m) * slow;
            } else {
                out[0] = v8math::min(v8math::max(a, 0.0), 1.0) * slow;
            }
        }
    }
    let (tx, ty, tz) = if let Some(aim) = aim_point {
        (aim.x - p.x, aim.y - (p.y + 1.6), aim.z - p.z)
    } else if request.valid {
        (request.steering_target.x - p.x, 0.0, request.steering_target.z - p.z)
    } else {
        return out;
    };
    let flat = v8math::hypot(tx, tz);
    if flat > 1e-6 {
        let err = -(fz * (tx / flat) - fx * (tz / flat));
        let limit = if facing { 2.0 } else { 1.0 };
        let rate = (match s.max_turn {
            Some(v) if v != 0.0 && !v.is_nan() => v,
            _ => 3.0,
        }) * dt;
        out[2] = v8math::max(-limit, v8math::min(limit, if rate > 0.0 { err / rate } else { 0.0 }));
        let pitch_err = v8math::sin(v8math::atan2(ty, flat) - s.aim_pitch.unwrap_or(0.0));
        let pitch_rate = (unit.class_def.as_ref().and_then(|c| c.max_pitch_speed).unwrap_or(0.5)) * dt;
        out[3] = v8math::max(-1.0, v8math::min(1.0, if pitch_rate > 0.0 { pitch_err / pitch_rate } else { 0.0 }));
    }
    out
}

/// visibleOrNear (scheduler.js): offline distance uses the first camera,
/// network distance uses every camera, and frustum tests always use every view.
pub fn visible_or_near(unit: &AiUnit, cameras: &[CameraView], network_views: bool, lod_disabled: bool) -> bool {
    position_visible_or_near(unit.position, unit.velocity, cameras, network_views, lod_disabled)
}

/// The same gate for canonical rows, without materializing an AI unit snapshot.
pub fn position_visible_or_near(p: Vec3, velocity: Option<Vec3>, cameras: &[CameraView], network_views: bool, lod_disabled: bool) -> bool {
    if lod_disabled { return true; }
    if let Some(vel) = velocity {
        if vel.x * vel.x + vel.y * vel.y + vel.z * vel.z >= 100.0 { return true; }
    }
    if cameras.is_empty() { return true; }
    let near_count = if network_views { cameras.len() } else { 1 };
    for camera in cameras.iter().take(near_count) {
        let e = &camera.elements;
        let dx = p.x - e[12];
        let dy = p.y - e[13];
        let dz = p.z - e[14];
        if dx * dx + dy * dy + dz * dz <= 2025.0 { return true; }
    }
    for camera in cameras {
        if camera_accepts(camera, p) { return true; }
    }
    false
}
