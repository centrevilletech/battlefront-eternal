//! SoldierCamera.update and its first-person and responsive chase paths
//!. Collision is the host-neutral world's native query.
// Third-person chase camera, following the 2004 engine.
//
// The camera looks along the soldier's aim, pitched down by the section's
// TiltValue, and sits at TrackCenter (the pivot, in the soldier's frame) plus
// TrackOffset (in the camera's own frame: right, up, back). With no sideways
// offset that puts the pivot dead center. Rotation eases toward its target
// at 30/s and position at 15/s per camera axis. Five rays from the pivot pull
// the camera in at walls; it snaps in and eases back out.
//
// ThirdPersonFOV is the horizontal field of view: the engine keeps
// tan(fov / 2) on x and scales y by the window's height over width. Zoomed in,
// the camera takes the posture's ZOOM section and the field of view narrows
// by the weapon's zoom factor.
//
// In first person the camera sits at the eye the soldier's aim gives it (see
// PlayerCombat.eye), or at the one its caller gives, and looks straight
// along the aim with no tilt or easing. (A turret's views are its own seat
// camera's, src/gunner.js TurretCamera.) First person, and
// zoomed in from either view, the field of view is FirstPersonFOV.
//
// In first person the eye also plays the game's camera motions (see
// FirstPersonMotion): diving, lying down and getting up, landing, and the
// head bob.
//
// When the soldier dies, DeathCamera takes over from whichever view he had:
// it looks down on the body, then over it at the killer.

use super::{math::*, motion::Motion, Camera, Cast};
use serde::Deserialize;
use three::Vector3 as V;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default)]
pub struct Section {
    pub eye: [f64; 3],
    pub center: [f64; 3],
    pub offset: [f64; 3],
    pub tilt: f64,
}
impl Default for Section {
    fn default() -> Self {
        Self {
            eye: [0.0, 1.8, 0.0],
            center: [0.0, 1.8, 0.0],
            offset: [0.0, 0.0, 3.5],
            tilt: 5.0 * DEG,
        }
    }
}
#[derive(Deserialize)]
pub struct Def {
    pub third: f64,
    pub first: f64,
    pub sections: [Section; 6],
}
impl Default for Def {
    fn default() -> Self {
        Self {
            third: 65.0 * DEG,
            first: 70.0 * DEG,
            sections: [Section::default(); 6],
        }
    }
}
#[derive(Default)]
pub struct Soldier {
    pub fresh: bool,
    pub reach: f64,
    pub fov: f64,
    pub center: V,
    pub offset: V,
    pub definition: usize,
    /// The place the chase camera would take this frame, and how far the
    /// drawn camera trails it (`Input::follow`).
    pub rest: V,
    pub lag: V,
    /// When true (2005 soldiers), obstruction pullback follows the 2005 game's
    /// chase rule (`Soldier::clearance_swbf2`): each drawn frame casts one of
    /// five rays parallel to the boom, in turn; the clear fraction is the
    /// smallest of the five kept results, filtered over frames; and the eye
    /// sits the near distance beyond the clear part of the boom, so a wall
    /// stops the near plane, never the eye, which stays at least the near
    /// distance from `TrackCenter`. False keeps the 2004 fanned five-ray
    /// solver unchanged.
    pub near_clearance: bool,
    clear_slots: [f64; 5],
    clear_slot: u8,
    clear_smooth: f64,
}
pub struct Input {
    pub feet: V,
    pub yaw: f64,
    pub pitch: f64,
    pub posture: usize,
    pub zoom: f64,
    pub first: bool,
    pub responsive: bool,
    pub eye: Option<V>,
    /// 0 to 1: how far the camera has gone from the posture's section to the
    /// crouched one (a droideka going down into its ball and up again).
    pub crouch: f64,
    /// 1/s: the rate at which the chase camera's place follows its rest place
    /// along the aim and up and down (0: it keeps up at once).
    pub follow: f64,
}
pub fn v(a: [f64; 3]) -> V {
    V::new(a[0], a[1], a[2])
}
impl Soldier {
    pub fn new() -> Self {
        Self {
            fresh: true,
            reach: 1.0,
            fov: 65.0 * DEG,
            definition: usize::MAX,
            clear_slots: [1.0; 5],
            clear_slot: 0,
            clear_smooth: 1.0,
            ..Self::default()
        }
    }
    // Another class's camera sections and fields of view (the player's next
    // body); the first-person motions stay.

    pub fn reset(&mut self, def: &Def) {
        self.fresh = true;
        self.reach = 1.0;
        self.fov = def.third;
        self.clear_slots = [1.0; 5];
        self.clear_slot = 0;
        self.clear_smooth = 1.0;
    }
    // The renderer's vertical FOV for a horizontal one at this window shape.
    // The field of view eases toward FirstPersonFOV (first person or zoomed) or
    // ThirdPersonFOV, narrowed by the zoom.
    // eye: the first-person eye, if the caller has one.
    // Pivot: TrackCenter in the soldier's frame (position and facing).
    // Look along the aim, tipped down by the tilt; ease toward it.
    // Wanted position: the pivot plus TrackOffset in the camera's frame.
    // Ease each camera-frame axis of the move separately.
    // Easing from last frame can leave the actual view behind a new wall even
    // though the desired boom is clear. Resolve the position we will render.
    // The player's aim/orbit and interpolated feet already have their own
    // timing. Filtering those again adds mouse lag and makes the character
    // slide across the screen on every start/stop/turn. Ease only changes to
    // the stance's local rig and the boom's release from an obstruction.
    // At the eye, looking along the aim; the chase camera starts afresh after.
    // The camera motion: the eye moved in the view's frame, then the view
    // turned in its own.
    // Fraction of the boom that is clear: the nearest hit of the center ray and
    // four rays to the camera's corners. The engine runs the corner rays
    // parallel to the boom; here they fan out from the pivot, so a ray never
    // starts inside a wall the soldier is pressed against.
    // Include the safety skin in the query. Testing only to the endpoint
    // then subtracting skin after a hit makes the camera jump by 8 cm each
    // time a pitched corner crosses a floor/wall (and repeat on easing out).

    pub fn frame<M: Math, C: Cast>(
        &mut self,
        dt: f64,
        input: &Input,
        def: &Def,
        motion: &Motion,
        cam: &mut Camera,
        world: &mut C,
    ) {
        let mut section = def.sections[input.posture.min(2) + if input.zoom > 1.0 { 3 } else { 0 }];
        if input.crouch > 0.0 {
            let (low, k) = (def.sections[1], input.crouch.min(1.0));
            let mix = |a: [f64; 3], b: [f64; 3]| -> [f64; 3] { std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k) };
            section = Section {
                eye: mix(section.eye, low.eye),
                center: mix(section.center, low.center),
                offset: mix(section.offset, low.offset),
                tilt: section.tilt + (low.tilt - section.tilt) * k,
            };
        }
        let base = if input.first || input.zoom > 1.0 {
            def.first
        } else {
            def.third
        };
        let want = 2.0 * M::atan(M::tan(base / 2.0) / input.zoom);
        self.fov += (want - self.fov) * (1.0 - M::exp(-25.0 * dt));
        cam.fov = 2.0 * M::atan(M::tan(self.fov / 2.0) / cam.aspect) * (180.0 / PI);
        if input.first {
            cam.q = euler::<M>(input.pitch, input.yaw + PI, 0.0, false);
            cam.p = input
                .eye
                .unwrap_or_else(|| pivot::<M>(input.feet, input.yaw, v(section.eye)));
            cam.p = add(cam.p, rotate(motion.offset(), cam.q));
            cam.q = mul(cam.q, motion.turn::<M>());
            self.fresh = true;
            return;
        }
        if input.responsive {
            // The player's look is already timed by prediction. Follow only
            // changes in the local stance and recovery from an obstruction.
            let fresh = self.fresh || cam.p.distance_to(&input.feet) > 80.0;
            if fresh || input.follow > 0.0 {
                // A following camera eases its place instead (below), as
                // the 2004 tracker does: once, not on top of this.
                self.center = v(section.center);
                self.offset = v(section.offset);
                if fresh {
                    self.reach = 1.0;
                }
            } else {
                let k = 1.0 - M::exp(-15.0 * if input.posture == 2 { 0.5 } else { 1.0 } * dt);
                self.center = mix(self.center, v(section.center), k);
                self.offset = mix(self.offset, v(section.offset), k);
            }
            let pivot = pivot::<M>(input.feet, input.yaw, self.center);
            cam.q = euler::<M>(input.pitch - section.tilt, input.yaw + PI, 0.0, false);
            let boom = rotate(self.offset, cam.q);
            if self.near_clearance {
                if fresh {
                    self.clear_slots = [1.0; 5];
                    self.clear_slot = 0;
                    self.clear_smooth = 1.0;
                }
                self.reach = self.clearance_swbf2(world, pivot, boom, cam, dt, false);
                cam.p = plus(pivot, boom, self.reach);
                self.rest = cam.p;
                self.lag = V::new(0.0, 0.0, 0.0);
                self.fresh = false;
                return;
            }
            let clear = clearance(world, pivot, boom, cam.q, 0.08, false);
            if fresh || clear < self.reach {
                self.reach = clear
            } else {
                self.reach += (clear - self.reach) * (1.0 - M::exp(-6.0 * dt));
            }
            cam.p = plus(pivot, boom, self.reach);
            // The 2004 tracker eases the camera's place toward this rest
            // place, so it glides to rest when the body stops dead (a
            // droideka unfolding at speed). Sideways it keeps up at once: the
            // reticle stays in the middle across, on the shot's line.
            let rest = cam.p;
            self.lag = if fresh || input.follow <= 0.0 {
                V::new(0.0, 0.0, 0.0)
            } else {
                let (sin, cos) = (M::sin(input.yaw), M::cos(input.yaw));
                let left = add(self.lag, sub(self.rest, rest));
                let k = M::exp(-input.follow * dt);
                let along = (left.x * sin + left.z * cos) * k;
                V::new(along * sin, left.y * k, along * cos)
            };
            self.rest = rest;
            if self.lag.length_sq() > 1e-8 {
                let boom = sub(add(rest, self.lag), pivot);
                let clear = clearance(world, pivot, boom, cam.q, 0.08, false);
                if clear < 1.0 {
                    cam.p = plus(pivot, boom, clear);
                } else {
                    cam.p = plus(pivot, boom, 1.0);
                }
            }
            self.fresh = false;
            return;
        }
        let pivot = pivot::<M>(input.feet, input.yaw, v(section.center));
        let want = euler::<M>(input.pitch - section.tilt, input.yaw + PI, 0.0, false);
        cam.q = if self.fresh || cam.p.distance_to(&pivot) > 80.0 {
            want
        } else {
            slerp::<M>(cam.q, want, 1.0 - M::exp(-30.0 * dt))
        };
        let boom = rotate(v(section.offset), cam.q);
        let clear = clearance(world, pivot, boom, cam.q, 0.08, false);
        if clear < self.reach {
            self.reach = clear
        } else {
            self.reach += (clear - self.reach) * (1.0 - M::exp(-6.0 * dt));
        }
        let target = plus(pivot, boom, self.reach);
        if self.fresh || cam.p.distance_to(&target) > 80.0 {
            cam.p = target;
            self.fresh = false;
            return;
        }
        ease_axes::<M>(
            cam,
            target,
            [15.0 * if input.posture == 2 { 0.5 } else { 1.0 }; 6],
            dt,
        );
        let boom = sub(cam.p, pivot);
        let clear = clearance(world, pivot, boom, cam.q, 0.08, false);
        if clear < 1.0 {
            cam.p = plus(pivot, boom, clear)
        }
    }
}
pub fn pivot<M: Math>(feet: V, yaw: f64, center: V) -> V {
    let sin = M::sin(yaw);
    let cos = M::cos(yaw);
    V::new(
        feet.x + sin * center.z - cos * center.x,
        feet.y + center.y,
        feet.z + cos * center.z + sin * center.x,
    )
}
pub fn clearance<C: Cast>(
    world: &mut C,
    pivot: V,
    boom: V,
    q: three::Quaternion,
    skin: f64,
    static_only: bool,
) -> f64 {
    if boom.length_sq() < 1e-8 {
        return 1.0;
    }
    let right = rotate(V::new(0.4, 0.0, 0.0), q);
    let up = rotate(V::new(0.0, 0.4, 0.0), q);
    let mut reach: f64 = 1.0;
    // SoldierCamera.clearance's rays fan from one pivot, with the safety
    // skin included in each query, so touching a floor cannot jump 8 cm.
    for (a, b) in [
        (0.0, 0.0),
        (1.0, 1.0),
        (1.0, -1.0),
        (-1.0, 1.0),
        (-1.0, -1.0),
    ] {
        let dir = plus(plus(boom, right, a), up, b);
        let far = dir.length();
        let dir = scale(dir, 1.0 / far);
        if let Some(distance) = world.cast(pivot, dir, far + skin, static_only) {
            reach = reach.min((distance - skin).max(0.0) / far);
        }
    }
    reach
}

/// 2005 game chase-camera parallel-ray offset from the boom (m).
const SWBF2_PARALLEL_RAY_OFFSET: f64 = 0.4;
/// 2005 game obstruction-fraction hold band while obstructed.
const SWBF2_CLEAR_HOLD: f64 = 0.15;
/// 2005 game obstruction-fraction linear ease-out rate (1/s).
const SWBF2_CLEAR_EASE_RATE: f64 = 0.75;
/// 2005 game round-robin parallel ray offsets in camera right/up coordinates.
const SWBF2_RAY_OFFSETS: [(f64, f64); 5] = [
    (0.0, 0.0),
    (SWBF2_PARALLEL_RAY_OFFSET, SWBF2_PARALLEL_RAY_OFFSET),
    (-SWBF2_PARALLEL_RAY_OFFSET, SWBF2_PARALLEL_RAY_OFFSET),
    (SWBF2_PARALLEL_RAY_OFFSET, -SWBF2_PARALLEL_RAY_OFFSET),
    (-SWBF2_PARALLEL_RAY_OFFSET, -SWBF2_PARALLEL_RAY_OFFSET),
];

impl Soldier {
    /// The 2005 game's chase-camera obstruction pullback, as fraction of the
    /// boom `L` from `TrackCenter` to the wanted eye.
    ///
    /// - The ray runs from `TrackCenter` toward the wanted eye less the near
    ///   distance (length `L - near`), moved sideways by this frame's offset
    ///   from [`SWBF2_RAY_OFFSETS`] in the camera's right and up axes. One ray
    ///   is cast per call, the offsets taken in turn, so a full sweep takes
    ///   five drawn frames (the 2005 game runs this once per drawn frame in a
    ///   single-player battle).
    /// - Each offset keeps the clear fraction of its last ray (1 when clear);
    ///   the smallest of the five is filtered over frames: a smaller value is
    ///   taken at once; a larger one is ignored while the view is still
    ///   obstructed and within [`SWBF2_CLEAR_HOLD`] of the current one; any
    ///   other is approached at [`SWBF2_CLEAR_EASE_RATE`] a second.
    /// - The eye goes at `near + s * (L - near)` from `TrackCenter`: the near
    ///   plane, not the eye, stops at the nearest hit, and the eye never comes
    ///   closer to `TrackCenter` than the near distance.
    ///
    /// A boom shorter than 1 cm has no pullback. No heap allocation.
    fn clearance_swbf2<C: Cast>(
        &mut self,
        world: &mut C,
        pivot: V,
        boom: V,
        cam: &Camera,
        dt: f64,
        static_only: bool,
    ) -> f64 {
        let len = boom.length();
        if len * len <= 1e-4 {
            return 1.0;
        }
        let near = cam.near.max(0.0);
        let dir = scale(boom, 1.0 / len);
        // Shorter than the near distance, the ray runs back past TrackCenter.
        let seg = len - near;
        let right = rotate(V::new(1.0, 0.0, 0.0), cam.q);
        let up = rotate(V::new(0.0, 1.0, 0.0), cam.q);
        let slot = usize::from(self.clear_slot % 5);
        let (ox, oy) = SWBF2_RAY_OFFSETS[slot];
        let start = plus(plus(pivot, right, ox), up, oy);
        let reach = seg.abs();
        let frac = if reach <= 0.0 {
            1.0
        } else {
            world
                .cast(start, scale(dir, seg.signum()), reach, static_only)
                .map_or(1.0, |d| (d / reach).clamp(0.0, 1.0))
        };
        self.clear_slots[slot] = frac;
        self.clear_slot = (self.clear_slot + 1) % 5;
        let mut min_frac = 1.0_f64;
        for &s in &self.clear_slots {
            if s < min_frac {
                min_frac = s;
            }
        }
        if min_frac < self.clear_smooth {
            self.clear_smooth = min_frac;
        } else if min_frac != 1.0 && min_frac <= self.clear_smooth + SWBF2_CLEAR_HOLD {
            // Hold small increases while still obstructed.
        } else {
            self.clear_smooth = (self.clear_smooth + SWBF2_CLEAR_EASE_RATE * dt.max(0.0)).min(min_frac);
        }
        (near + self.clear_smooth * seg) / len
    }
}
pub fn ease_axes<M: Math>(cam: &mut Camera, target: V, rates: [f64; 6], dt: f64) {
    // easeAxes: each signed camera-frame axis follows
    // at its own authored rate, before being rotated back to world space.
    let mut local = rotate(sub(target, cam.p), inv(cam.q));
    local.x *= 1.0 - M::exp(-rates[if local.x > 0.0 { 0 } else { 1 }] * dt);
    local.y *= 1.0 - M::exp(-rates[if local.y > 0.0 { 2 } else { 3 }] * dt);
    local.z *= 1.0 - M::exp(-rates[if local.z > 0.0 { 4 } else { 5 }] * dt);
    cam.p = add(cam.p, rotate(local, cam.q));
}

/// PlayerCombat.viewBasis, viewEye and eye. A displayed
/// soldier supplies its own body-position clock independently of the feet
/// after stair/correction smoothing. Authority supplies its canonical basis.
// The authority basis can precede a correction to the displayed feet.
// Subtract body position and add smoothed feet as two separate operations.
// Prone selects the lateral pivot eye before the zoom-dependent setback.
// A droideka supplies its own scalar height through the outer phase instead.
// HUD and speculative shots still have separate gameplay basis consumers.
// This function is the sole eye writer used by the own camera phase.
// SoldierCamera.combatEye eye/target/FOV solver across posture and first-person state.
#[allow(clippy::too_many_arguments)]
pub fn combat_eye<M: Math>(
    feet: V,
    position: V,
    yaw: f64,
    pitch: f64,
    posture: usize,
    air: bool,
    zoom: f64,
    basis: Option<[V; 4]>,
) -> V {
    let posture = if air { 0 } else { posture.min(2) };
    let (pivot, origin, left, forward) = if let Some([pivot, origin, left, forward]) = basis {
        (pivot, origin, left, forward)
    } else {
        let basis=crate::aim::Basis::new::<M>(position,yaw,pitch,posture,air);
        let (pivot,origin,left,forward)=(basis.pivot,basis.origin,basis.left,basis.direction);
        (pivot, origin, left, forward)
    };
    let eye = if !air && posture == 2 {
        plus(pivot, left, -0.12)
    } else {
        origin
    };
    plus(
        add(sub(eye, position), feet),
        forward,
        if zoom > 1.0 { -0.6 } else { -0.3 },
    )
}
