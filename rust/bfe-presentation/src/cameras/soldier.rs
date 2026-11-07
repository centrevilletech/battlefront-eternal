//! SoldierCamera.update and its first-person and responsive chase paths
//! from src/camera.js. Collision is the host-neutral world's native query.
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

#[derive(Clone, Copy, Deserialize)]
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
            ..Self::default()
        }
    }
    // Another class's camera sections and fields of view (the player's next
    // body); the first-person motions stay.

    pub fn reset(&mut self, def: &Def) {
        self.fresh = true;
        self.reach = 1.0;
        self.fov = def.third;
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
        let section = def.sections[input.posture.min(2) + if input.zoom > 1.0 { 3 } else { 0 }];
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
            if fresh {
                self.center = v(section.center);
                self.offset = v(section.offset);
                self.reach = 1.0;
            } else {
                let k = 1.0 - M::exp(-15.0 * if input.posture == 2 { 0.5 } else { 1.0 } * dt);
                self.center = mix(self.center, v(section.center), k);
                self.offset = mix(self.offset, v(section.offset), k);
            }
            let pivot = pivot::<M>(input.feet, input.yaw, self.center);
            cam.q = euler::<M>(input.pitch - section.tilt, input.yaw + PI, 0.0, false);
            let boom = rotate(self.offset, cam.q);
            let clear = clearance(world, pivot, boom, cam.q, 0.08, false);
            if fresh || clear < self.reach {
                self.reach = clear
            } else {
                self.reach += (clear - self.reach) * (1.0 - M::exp(-6.0 * dt));
            }
            cam.p = plus(pivot, boom, self.reach);
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
pub fn ease_axes<M: Math>(cam: &mut Camera, target: V, rates: [f64; 6], dt: f64) {
    // easeAxes in flight/camera.js: each signed camera-frame axis follows
    // at its own authored rate, before being rotated back to world space.
    let mut local = rotate(sub(target, cam.p), inv(cam.q));
    local.x *= 1.0 - M::exp(-rates[if local.x > 0.0 { 0 } else { 1 }] * dt);
    local.y *= 1.0 - M::exp(-rates[if local.y > 0.0 { 2 } else { 3 }] * dt);
    local.z *= 1.0 - M::exp(-rates[if local.z > 0.0 { 4 } else { 5 }] * dt);
    cam.p = add(cam.p, rotate(local, cam.q));
}

/// PlayerCombat.viewBasis, viewEye and eye from combat.js. A displayed
/// soldier supplies its own body-position clock independently of the feet
/// after stair/correction smoothing. Authority supplies its canonical basis.
// The authority basis can precede a correction to the displayed feet.
// Subtract body position and add smoothed feet as two separate operations.
// Prone selects the lateral pivot eye before the zoom-dependent setback.
// A droideka supplies its own scalar height through the outer phase instead.
// HUD and speculative shots still have separate gameplay basis consumers.
// This function is the sole eye writer used by the own camera phase.
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
