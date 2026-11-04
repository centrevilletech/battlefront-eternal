//! Where someone getting out of a seat is set down: the 2004 game's exit
//! search for bots and every turret crew (src/seats.js classicSeatExit:
//! nine candidates, up to 99 casts), and a person's search out of a hull
//! (src/vehicles.js safeExitPoint and personExitPoint: twelve spots, up to
//! 24 casts and 12 overlap tests).

use super::SeatWorld;
use crate::gen::Body;
use crate::store::Ref;
use crate::rig::math;
use three::Vector3;

const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const DOWN: Vector3 = Vector3::new(0.0, -1.0, 0.0);

/// seats.js:265-269, at float32 where the 2004 game's constants are.
const EXIT_EXTRA: f64 = 0.6840001f64 as f32 as f64;
// The 2004 game's own 0.7071 (not 1/sqrt(2)): kept as it is.
#[allow(clippy::approx_constant)]
const EXIT_DIAGONAL: f64 = 0.7071f64 as f32 as f64;
const SUPPORT_STEP: f64 = 0.72f64 as f32 as f64;
const SUPPORT_UP: f64 = 5.0;
const SUPPORT_DOWN: f64 = 10.0;

#[inline(always)]
fn f32r(x: f64) -> f64 {
    x as f32 as f64
}

/// seats.js exitCandidates: behind, left, right, the four diagonals, in
/// front and the vehicle's own spot, each 1 m up, at R = the radius + 0.684.
pub fn exit_candidates(radius: f64) -> [[f64; 3]; 9] {
    let r = f32r(f32r(radius) + EXIT_EXTRA);
    let d = f32r(EXIT_DIAGONAL * r);
    [[0.0, 1.0, -r], [-r, 1.0, 0.0], [r, 1.0, 0.0], [-d, 1.0, -d], [d, 1.0, -d], [d, 1.0, d], [-d, 1.0, d], [0.0, 1.0, r], [0.0, 1.0, 0.0]]
}

/// seats.js exitSupportOffsets: the sample itself, then ±0.72 m along the
/// world's axes.
pub fn exit_support_offsets() -> [[f64; 3]; 5] {
    let s = SUPPORT_STEP;
    [[0.0, 0.0, 0.0], [s, 0.0, s], [s, 0.0, -s], [-s, 0.0, s], [-s, 0.0, -s]]
}

/// seats.js highestSupport: of support heights (None where a sample found
/// none), the index of the highest; equal heights keep the earlier. -1
/// when none.
pub fn highest_support(heights: &[Option<f64>]) -> i32 {
    let mut best: i32 = -1;
    for (i, h) in heights.iter().enumerate() {
        let Some(h) = *h else { continue };
        if best < 0 || h > heights[best as usize].unwrap() {
            best = i as i32;
        }
    }
    best
}

/// The occupied seat's frame and the vehicle's bounding sphere
/// (classicSeatExit's second argument).
#[derive(Clone, Copy, Debug)]
pub struct ExitFrame {
    pub origin: Vector3,
    pub forward: Vector3,
    pub up: Vector3,
    pub center: Vector3,
    pub radius: f64,
    pub body: Option<Ref<Body>>,
}

/// classicSeatExit's out: where he goes, the frame's forward, and the
/// candidate taken (-1: none had support, he stays at the origin).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExitSpot {
    pub position: Vector3,
    pub forward: Vector3,
    pub candidate: i32,
}

/// seats.js classicSeatExit (303): the 2004 game's seat exit placement.
/// Each of the first eight candidates needs a clear line from the
/// vehicle's centre (the vehicle left out); a candidate is taken if any of
/// its five support samples finds a floor (a 5 m ray up, the sample rising
/// to just under what it meets, then a 10 m ray down that must hit); he
/// goes to its X/Z at the highest support found. Without one, the origin.
pub fn classic_seat_exit<W: SeatWorld + ?Sized>(w: &mut W, frame: &ExitFrame, out: &mut ExitSpot) {
    // The orthonormal frame: up, right = up x forward, forward = right x up.
    let mut up = frame.up;
    up.normalize();
    let mut right = Vector3::default();
    right.cross_vectors(&up, &frame.forward);
    if right.length_sq() < 1e-12 {
        right.set(1.0, 0.0, 0.0);
    }
    right.normalize();
    let mut forward = Vector3::default();
    forward.cross_vectors(&right, &up);
    out.forward = forward;
    out.candidate = -1;
    out.position = frame.origin;
    let offsets = exit_support_offsets();
    let candidates = exit_candidates(frame.radius);
    let mut heights: [Option<f64>; 5] = [None; 5];
    for (k, &[x, y, z]) in candidates.iter().enumerate() {
        let mut point = frame.origin;
        point.add_scaled_vector(&right, x).add_scaled_vector(&up, y).add_scaled_vector(&forward, z);
        if k < 8 {
            let mut dir = Vector3::default();
            dir.sub_vectors(&point, &frame.center);
            let span = dir.length();
            if span > 1e-6 {
                dir.divide_scalar(span);
                if w.cast(&frame.center, &dir, span, frame.body).is_some() {
                    continue;
                }
            }
        }
        let mut found = false;
        for (i, &[dx, _, dz]) in offsets.iter().enumerate() {
            let mut sample = Vector3::new(point.x + dx, point.y, point.z + dz);
            let above = w.cast_support(&sample, &UP, SUPPORT_UP);
            sample.y += (match above {
                Some(hit) => hit.distance,
                None => SUPPORT_UP,
            }) - 0.1;
            match w.cast_support(&sample, &DOWN, SUPPORT_DOWN) {
                Some(below) if below.distance < SUPPORT_DOWN => {
                    heights[i] = Some(sample.y - below.distance);
                    found = true;
                }
                _ => heights[i] = None,
            }
        }
        if !found {
            continue;
        }
        let best = highest_support(&heights);
        out.position.set(point.x, heights[best as usize].unwrap(), point.z);
        out.candidate = k as i32;
        return;
    }
}

/// What a person's exit search reads of the hull.
#[derive(Clone, Copy, Debug)]
pub struct HullFrame {
    pub position: Vector3,
    pub yaw: f64,
    /// `radius`: half the larger of its box's width and length.
    pub radius: f64,
    /// Its box's height (`size.y`).
    pub height: f64,
    pub body: Option<Ref<Body>>,
    /// boarding.js inFlight: a craft off the ground.
    pub airborne: bool,
}

/// vehicles.js safeExitPoint (320): outward from the hull, a floor, room for
/// the body, and a clear line from the vehicle. A craft in flight needs no
/// floor under the spot (he bails out at its height). Writes the spot
/// (feet) to out and says whether one was found.
pub fn safe_exit_point<W: SeatWorld + ?Sized>(w: &mut W, v: &HullFrame, out: &mut Vector3) -> bool {
    let distance = v.radius + 1.6;
    let airborne = v.airborne;
    for i in 0..12 {
        let a = v.yaw + std::f64::consts::PI / 2.0 + (i as f64) * std::f64::consts::PI / 6.0;
        out.set(v.position.x + math::sin(a) * distance, v.position.y + 3.0, v.position.z + math::cos(a) * distance);
        let floor = w.cast(out, &DOWN, 10.0, v.body);
        match &floor {
            None if !airborne => continue,
            Some(f) if f.normal.y < 0.65 => continue,
            _ => {}
        }
        out.y = match &floor {
            Some(f) => f.point.y + 1.0,
            None => v.position.y + 1.0,
        };
        if w.overlaps_sphere(out, 0.65, v.body) {
            continue;
        }
        let mut target = v.position;
        target.add_scaled_vector(&UP, 1.5);
        let mut direction = Vector3::default();
        direction.sub_vectors(out, &target);
        let span = direction.length();
        direction.divide_scalar(span);
        if w.cast(&target, &direction, span, v.body).is_some() {
            continue;
        }
        out.y -= 1.0;
        return true;
    }
    false
}

/// vehicles.js personExitPoint (341): the search above, or, in flight or
/// forced (`anywhere`), above the hull when it finds nothing. False: boxed
/// in on the ground, he stays aboard.
pub fn person_exit_point<W: SeatWorld + ?Sized>(w: &mut W, v: &HullFrame, out: &mut Vector3, anywhere: bool) -> bool {
    if safe_exit_point(w, v, out) {
        return true;
    }
    if !anywhere {
        return false;
    }
    *out = v.position;
    out.add_scaled_vector(&UP, v.height + 1.0);
    true
}
