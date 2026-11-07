//! DeathCamera.update from src/camera.js. This is the authoritative 60 Hz
//! view used by Classic scheduling. The displayed death replay is separate:
//! it must never advance the world's camera on a render-only frame.
// The death camera, following the 2004 engine's. From the soldier's death
// until he has a body again it takes over from whatever view he had: the
// chase camera, his own eyes, a vehicle's, a turret's or a recon droid's.
//
// Each step (each frame in the original, each 60 Hz world tick here) it works
// out a pose that looks from a near point toward a far one, then moves a
// fraction t of the way there and turns the same fraction, so it eases in:
// - At first the near point is 1 m from the body toward where the camera
// looked from and 1.5 m above the feet, and the far point is the body's
// middle, 0.8 m up. So it rises and backs off to look down on the body at
// about 35 degrees, keeping its heading. t grows from 0 to 1 over the
// first second, then stays 1.
// - After 1.5 s, if someone else killed him, the near point is the body's
// middle and the far point the killer's eyes (his vehicle's or turret's
// while he sits in one). It looks over the body at the killer and follows
// him. t starts again from 0 at 1.5 s and reaches 1 at 3.5 s. While the
// killer has no body (he died too) it goes on looking down on the body.
// It sits 5 m behind the near point, on the line from the far one, or 1 m
// short of what two rays find: they run 6 m back from 0.75 m above and below
// the near point and, like the original's camera rays, meet only terrain,
// buildings and props, not vehicles or soldiers. The world's up stays up.
// The body's middle follows the body while it is in the world; the first
// near point stays where it started. The field of view stays as the chase
// camera left it (see deadFov), unzoomed, and it doesn't shake.

use super::{math::*, Camera, Cast};
use three::{Quaternion, Vector3};
#[derive(Clone, Copy)]
pub struct State {
    pub time: f64,
    pub position: Vector3,
    pub quaternion: Quaternion,
    pub previous_position: Vector3,
    pub previous_quaternion: Quaternion,
    pub body: Vector3,
    pub anchor: Vector3,
    pub killed: bool,
    pub hfov: f64,
}
impl State {
    pub fn start<M: Math>(
        cam: &Camera,
        feet: Vector3,
        killed: bool,
        hfov: f64,
        dead_dt: f64,
    ) -> Self {
        let mut back = rotate(Vector3::new(0.0, 0.0, 1.0), cam.q);
        back.y = 0.0;
        if back.length_sq() < 1e-8 {
            back = Vector3::new(0.0, 0.0, 1.0)
        }
        back = norm(back);
        Self {
            time: 0.0,
            position: cam.p,
            quaternion: cam.q,
            previous_position: cam.p,
            previous_quaternion: cam.q,
            body: Vector3::new(feet.x, feet.y + 0.8, feet.z),
            anchor: Vector3::new(feet.x + back.x, feet.y + 1.5, feet.z + back.z),
            killed,
            hfov: if dead_dt == 0.0 {
                hfov
            } else {
                0.837758 + (hfov - 0.837758) * M::exp(-25.0 * dead_dt)
            },
        }
    }

    pub fn frame<M: Math, C: Cast>(
        &mut self,
        dt: f64,
        feet: Option<Vector3>,
        killer: Option<Vector3>,
        cam: &mut Camera,
        world: &mut C,
    ) {
        self.previous_position = self.position;
        self.previous_quaternion = self.quaternion;
        self.time += dt;
        if let Some(feet) = feet {
            self.body = Vector3::new(feet.x, feet.y + 0.8, feet.z)
        }
        let (mut near, mut far, mut t) = (self.anchor, self.body, 1.0);
        if self.time < 1.0 {
            t = (self.time / 1.0).min(1.0)
        } else if self.time > 1.5 && self.killed {
            t = ((self.time - 1.5) * 0.5).min(1.0);
            if let Some(killer) = killer {
                near = self.body;
                far = killer;
            }
        }
        // A coincident killer keeps the previous direction. Both terrain
        // and building rays omit moving actors, as DeathCamera.update did.
        let mut dir = sub(far, near);
        if dir.length_sq() < 1e-10 {
            dir = rotate(Vector3::new(0.0, 0.0, -1.0), self.quaternion)
        }
        dir = norm(dir);
        let back = scale(dir, -1.0);
        let mut clear: f64 = 1.0;
        for lift in [0.75, -0.75] {
            if let Some(distance) =
                world.cast(Vector3::new(near.x, near.y + lift, near.z), back, 6.0, true)
            {
                clear = clear.min(distance / 6.0)
            }
        }
        let target = plus(near, dir, 1.0 - 6.0 * clear);
        let want = look(target, add(target, dir), UP);
        self.position = mix(self.position, target, t);
        self.quaternion = slerp::<M>(self.quaternion, want, t);
        // The fixed-tick camera publishes the current endpoint. Rendered
        // interpolation belongs to the page's independent DeathShot owner.
        cam.p = mix_vectors(self.previous_position, self.position, 1.0);
        cam.q = slerp_vectors::<M>(self.previous_quaternion, self.quaternion, 1.0);
        cam.fov = 2.0 * M::atan(M::tan(self.hfov / 2.0) / cam.aspect) * (180.0 / PI);
    }
}
