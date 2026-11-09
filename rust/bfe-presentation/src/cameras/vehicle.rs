//! VehicleCamera and updateFlightCamera from vehicles.js and flight/camera.js.
//! Matrices here are factual hull/seat frames supplied by retained rigs;
//! camera placement, obstruction, blending and FOV belong to this phase.

use super::{
    math::*,
    soldier::{clearance, ease_axes},
    Camera, Cast,
};
use three::{Matrix4 as M, Quaternion as Q, Vector3 as V};

#[derive(Clone, Copy)]
pub struct Seat {
    pub eye: V,
    pub center: V,
    pub offset: V,
    pub tilt: f64,
    pub first: f64,
    pub third: f64,
    pub rates: [f64; 6],
    pub aim: f64,
}
// VehicleCamera's inputs distinguish several frames that can share a hull.
// Position is authority or displayed position on the caller's own clock.
// The seat frame leaves a repulsor pilot's lean and vertical bob out.
// The hull frame retains that lean for a cockpit mounted inside the model.
// The flight frame retains the controller's pitch/roll without display bank.
// Drawn orientation includes the bank used by the visible pilot's head.
// An exposed rider can supply a posed head node instead of EyePointOffset.
// Direction may already be canonical F2 aim rather than a previewed angle.
// Preview is optional; absence must not be replaced with a zero vector.
// Bounds are local model extents, used only to decide whether to hide a hull.
// Velocity is a world-space fact, shared by chase lead and speed response.
// Human look yaw can be absent, distinct from an explicitly level look.
// Health and age drive the original deterministic flight damage response.
// Each scalar was formerly read from VehicleDriver or the selected Vehicle.
pub struct Input {
    pub id: f64,
    pub reset: bool,
    pub flying: bool,
    pub walking: bool,
    pub seat: usize,
    pub eye_hull: bool,
    pub first: bool,
    pub frame: M,
    pub hull: M,
    pub position: V,
    pub drawn: Q,
    pub flight_frame: Q,
    pub eye_node: Option<V>,
    pub direction: V,
    pub preview: Option<V>,
    pub velocity: V,
    pub center: V,
    pub size: V,
    pub bounds: [f64; 6],
    pub yaw: f64,
    pub tilt: f64,
    pub roll: f64,
    pub spin: f64,
    pub max_speed: f64,
    pub human: bool,
    pub look_yaw: Option<f64>,
    pub look_pitch: f64,
    pub boosting: bool,
    pub age: f64,
    pub health: f64,
    pub blend_duration: f64,
    pub bike: bool,
    pub view: Seat,
}
#[derive(Default)]
pub struct Vehicle {
    pub set: bool,
    last_id: f64,
    first: bool,
    mode: u8,
    pub speed_effect: f64,
    pub reach: f64,
    pub fov: f64,
    pub enclosed: bool,
    pub look_at: V,
    last_health: Option<f64>,
    hit_shake: f64,
    blend_time: Option<f64>,
    blend_p: V,
    blend_q: Q,
    blend_fov: f64,
}
impl Vehicle {
    pub fn new() -> Self {
        Self {
            reach: 1.0,
            ..Self::default()
        }
    }
    // Ports VehicleCamera.update from vehicles.js as one selected owner.
    // A changed hull, perspective or camera kind starts a fresh section.
    // Only a flyer pilot enters the separate flight controller below.
    // Passenger sections retain the ordinary seat camera, even in a flyer.
    // Ground pilots keep the page's hover/bike chase values and speed easing.
    // Walking pilots use their authored section instead of a hover boom.
    // FOV is horizontal content; only the final lens converts it to vertical.
    // Native frame state survives the host's body and attachment phases.
    pub fn frame<T: Math, C: Cast>(
        &mut self,
        input: &Input,
        cam: &mut Camera,
        dt: f64,
        world: &mut C,
    ) {
        if input.reset {
            self.set = false
        }
        if input.flying && input.seat == 0 {
            self.flight::<T, C>(input, cam, dt, world);
            return;
        }
        let mode = if input.seat == 0 && !input.walking {
            0
        } else {
            1
        };
        let fresh =
            !self.set || self.first != input.first || self.last_id != input.id || self.mode != mode;
        self.last_id = input.id;
        self.first = input.first;
        self.mode = mode;
        let speed = input.velocity.length();
        let intensity = if input.flying {
            0.0
        } else {
            let x = clamp(
                (speed - 12.0) / ((25.0f64.max(input.max_speed * 0.85)) - 12.0),
                0.0,
                1.0,
            );
            x * x * (3.0 - 2.0 * x)
        };
        self.speed_effect += (intensity - self.speed_effect) * (1.0 - T::exp(-4.0 * dt));
        let fov = if mode == 0 {
            (if input.first {
                if input.bike {
                    80.0
                } else {
                    78.0
                }
            } else if input.bike {
                82.0
            } else {
                85.0
            }) * DEG
        } else if input.first {
            input.view.first
        } else {
            input.view.third
        };
        self.fov = if fresh {
            fov
        } else {
            self.fov + (fov - self.fov) * (1.0 - T::exp(-25.0 * dt))
        };
        cam.fov = 2.0 * T::atan(T::tan(self.fov / 2.0) / cam.aspect) * (180.0 / PI);
        if input.first {
            self.eye(input, cam)
        } else if mode == 0 {
            self.chase::<T, C>(input, cam, dt, fresh, world)
        } else {
            self.seat::<T, C>(input, cam, dt, fresh, world)
        }
        self.set = true;
    }
    // The hull's frame with its lean: as drawn between ticks on the page, at
    // the tick in the simulation.
    // Inside the hull's box (a cockpit) the hull leaves the view; outside
    // it (a saddle) it stays.

    // VehicleCamera.eyeView uses the full hull for a pilot or rigid seat.
    // A turret seat with a rotating node instead uses its aiming frame.
    // The rider head receives the original ten-centimetre up displacement.
    // Enclosure is measured back in the same frame that placed the eye.
    // Its strict 15 cm margins keep a saddle outside the hidden hull case.
    // Heading comes from aim or preview, while up remains the subject's up.
    // This path snaps at once; only flight cockpit transitions blend a pose.
    fn eye(&mut self, i: &Input, cam: &mut Camera) {
        let frame = if i.eye_hull { &i.hull } else { &i.frame };
        let up = column(frame, 1);
        cam.p = if let Some(eye) = i.eye_node {
            plus(eye, up, 0.1)
        } else {
            transformed(i.view.eye, frame)
        };
        let mut inverse = *frame;
        inverse.invert();
        let local = transformed(cam.p, &inverse);
        let b = i.bounds;
        self.enclosed = local.x > b[0] + 0.15
            && local.x < b[3] - 0.15
            && local.y > b[1] + 0.15
            && local.y < b[4] - 0.15
            && local.z > b[2] + 0.15
            && local.z < b[5] - 0.15;
        cam.q = look_basis(scale(i.preview.unwrap_or(i.direction), -1.0), up);
    }
    // Where it rests: behind the heading (half the nose's pitch) and above.
    // The walls: the boom from the hull's centre to the camera.
    // What it looks at: a point ahead of the nose (a flyer: nose metres
    // along the heading, so the view stays behind the tail looking a
    // little past the nose) or ahead along the velocity (a hover), a
    // little above the hull, swung a share of the way toward the look the
    // nose is following (steer).
    // The bank rolls the view a share of the way; the barrel roll (spin)
    // takes it all the way round with the hull.

    // VehicleCamera.hoverChase keeps the game's page-specific hover framing.
    // The minimum boom avoids tiny hulls moving the eye inside the body.
    // Bike height and distance retain their separate authored-name rule.
    // Velocity lead is world-space and does not rotate with the view.
    // Ground roll and tilt affect the requested turn, not the body origin.
    // A fresh owner or a distant camera snaps instead of crossing the map.
    // Obstruction retracts its boom before any lagged position is published.
    // A later clear frame eases outward without filtering mouse aim twice.
    fn chase<T: Math, C: Cast>(
        &mut self,
        i: &Input,
        cam: &mut Camera,
        dt: f64,
        fresh: bool,
        world: &mut C,
    ) {
        let spec: [f64; 6] = if i.bike {
            [1.9, 7.0, 1.6, 2.2, 10.0, 9.0]
        } else {
            [1.7, 8.0, 1.1, 2.6, 9.0, 8.0]
        };
        let pivot = add(rotate(i.center, axis::<T>(UP, i.yaw)), i.position);
        let back = spec[1].max(spec[0] * i.size.z);
        let up = spec[3].max(spec[2] * i.size.y);
        let pitch = i.tilt * 0.5;
        let dir = V::new(
            T::sin(i.yaw) * T::cos(pitch),
            T::sin(pitch),
            T::cos(i.yaw) * T::cos(pitch),
        );
        let rest = plus(plus(pivot, dir, -back), UP, up);
        let snap = fresh || cam.p.distance_to(&rest) > 80.0;
        cam.p = if snap {
            rest
        } else {
            mix(cam.p, rest, 1.0 - T::exp(-spec[4] * dt))
        };
        let boom = sub(cam.p, pivot);
        let clear = clearance(world, pivot, boom, cam.q, 0.08, false);
        if snap || clear < self.reach {
            self.reach = clear
        } else {
            self.reach += (clear - self.reach) * (1.0 - T::exp(-6.0 * dt));
        }
        cam.p = plus(pivot, boom, self.reach);
        // Hover cameras lead the velocity by 0.22 s. Their nose and steering
        // factors are zero in both authored chase presets, kept out of the
        // lens rather than calculated and discarded on each frame.
        self.look_at = plus(plus(pivot, i.velocity, 0.22), UP, up * 0.2);
        let mut dir = sub(self.look_at, cam.p);
        if dir.length_sq() < 1e-6 {
            dir = V::new(T::sin(i.yaw), 0.0, T::cos(i.yaw))
        }
        dir = norm(dir);
        let bank = (i.roll - i.spin) * 0.2 + i.spin;
        let mut upward = UP;
        if bank != 0.0 {
            upward = rotate(upward, axis::<T>(dir, bank))
        }
        let want = mul(
            look_basis(scale(dir, -1.0), upward),
            axis::<T>(V::new(1.0, 0.0, 0.0), -7.0 * DEG),
        );
        cam.q = if snap {
            want
        } else {
            slerp::<T>(cam.q, want, 1.0 - T::exp(-spec[5] * dt))
        };
    }
    // Human mouse aim already has its final direction. Keep positional
    // follow and wall recovery smooth without filtering the sight a second time.
    // Where it should be, then as far along the rays from the seat frame as
    // the walls allow (pulled in at once, eased back out).
    // Each camera axis eases separately, at its section's MoveTension.
    // The eased spot can lag behind a new wall: check where it will draw.
    // The share of the boom (this.boom from this.origin) that is clear: the
    // nearest hit of the centre ray and four to the camera's corners, the
    // hull itself left out.

    // VehicleCamera.seatChase is also the walking pilot's camera.
    // Its TrackCenter is transformed by the subject frame before the offset.
    // TrackOffset is transformed by the camera turn, so tilt affects the boom.
    // The seat's aim tension eases its turn except on the fresh snap.
    // A vehicle passenger does not inherit the pilot's speed field of view.
    // A hull id change resets the camera even if the section values match.
    // The collision origin is 1.5 m up in the seat's subject frame.
    // The shared follow routine also resolves the actual lagged endpoint.
    fn seat<T: Math, C: Cast>(
        &mut self,
        i: &Input,
        cam: &mut Camera,
        dt: f64,
        fresh: bool,
        world: &mut C,
    ) {
        let want = mul(
            look_basis(
                scale(i.preview.unwrap_or(i.direction), -1.0),
                column(&i.frame, 1),
            ),
            axis::<T>(V::new(1.0, 0.0, 0.0), -i.view.tilt),
        );
        let pivot = transformed(i.view.center, &i.frame);
        let snap = fresh || cam.p.distance_to(&pivot) > 80.0;
        cam.q = if snap || i.human {
            want
        } else {
            slerp::<T>(cam.q, want, 1.0 - T::exp(-i.view.aim * dt))
        };
        let origin = transformed(V::new(0.0, 1.5, 0.0), &i.frame);
        self.follow::<T, C>(i.view, cam, origin, pivot, snap, dt, world);
    }
    // Ports VehicleCamera.follow's obstruction and per-axis release.
    // Pulling a boom in is immediate; releasing it retains its own clock.
    // The desired target can be clear while last frame's eye is behind a wall.
    // Resolve that eased endpoint too before publishing the rendered camera.
    // Its corner rays include the near-plane skin in their query length.
    // The own hull is excluded by stable collision slot in the native adapter.
    // No candidate list or scratch vector crosses back to the page.
    // VehicleCamera.follow third-person chase solver.
    #[allow(clippy::too_many_arguments)]
    fn follow<T: Math, C: Cast>(
        &mut self,
        seat: Seat,
        cam: &mut Camera,
        origin: V,
        pivot: V,
        snap: bool,
        dt: f64,
        world: &mut C,
    ) {
        let target = add(rotate(seat.offset, cam.q), pivot);
        let boom = sub(target, origin);
        let clear = clearance(world, origin, boom, cam.q, 0.08, false);
        if snap || clear < self.reach {
            self.reach = clear
        } else {
            self.reach += (clear - self.reach) * (1.0 - T::exp(-6.0 * dt));
        }
        let target = plus(origin, boom, self.reach);
        if snap {
            cam.p = target
        } else {
            ease_axes::<T>(cam, target, seat.rates, dt);
            // The actual eased point can still trail behind a new wall.
            // VehicleCamera.seatChase resolves that point a second time.
            let boom = sub(cam.p, origin);
            let clear = clearance(world, origin, boom, cam.q, 0.08, false);
            if clear < 1.0 {
                cam.p = plus(origin, boom, clear)
            }
        }
    }
    // Flight cameras beyond 80 m jump; wall release eases back out at 6/s.
    // Each axis of a camera eases toward where it should be on its own, in the
    // camera's frame, at its section's rate for the way it has to go
    // (MoveTension: right, left, up, down, back, forward), as the 2004 game's
    // camera section moves it.
    // A camera turned to look along `ahead` with `upward` as its up.
    // The pilot's views of his craft, as the 2004 game shows them (docs/
    // vehicles.md "Camera"). Behind it, the chase camera of his section: it looks
    // along the craft's nose with the craft's up, pitched down by TiltValue,
    // turning toward that at AimTension; it rests at TrackCenter in the craft's
    // frame and TrackOffset (right, up, back) in its own, walls pulling it in,
    // and each of its axes eases there at its MoveTension. The frame is the one
    // the craft flies by: the camera does not lean with the drawn hull. In the
    // cockpit, the eye (EyePointOffset) on the drawn hull looks along the nose.
    // The field of view across is ThirdPersonFOV behind and FirstPersonFOV in
    // the cockpit (48 degrees unless the section sets one); it does not change
    // with speed, and nothing blurs. The port blends a switch between the two
    // views over cameraBlendTime.
    // The eased spot can lag behind a new wall: check where it will draw.
    // The way between the two views stays out of walls.

    // Ports updateFlightCamera from flight/camera.js.
    // The flight frame excludes the display bank that belongs to hull posing.
    // Pilot look and preview aim are applied in their original frames.
    // Chase turn follows AimTension; signed movement uses six MoveTensions.
    // Cockpit/chase changes blend from the view and lens currently on screen.
    // The blend is eased over the class's cameraBlendTime, when supplied.
    // A collision resolution follows blending so the transition stays clear.
    // Boost and low-health oscillations retain the vehicle's age clock.
    // They consume no cosmetic random stream and never perturb world RNG.
    // First-person near plane is restored on every cockpit frame.
    fn flight<T: Math, C: Cast>(&mut self, i: &Input, cam: &mut Camera, dt: f64, world: &mut C) {
        let fresh = !self.set || self.last_id != i.id;
        if !fresh && self.first != i.first {
            self.blend_p = cam.p;
            self.blend_q = cam.q;
            self.blend_fov = cam.fov;
            self.blend_time = Some(0.0)
        }
        self.last_id = i.id;
        self.first = i.first;
        if let Some(h) = self.last_health {
            if i.health < h {
                self.hit_shake = 0.3
            }
        }
        self.last_health = Some(i.health);
        self.hit_shake = (self.hit_shake - dt).max(0.0);
        self.speed_effect = 0.0;
        if i.first {
            let up = rotate(UP, i.drawn);
            cam.up = up;
            cam.p = add(rotate(i.view.eye, i.drawn), i.position);
            cam.q = mul(i.drawn, axis::<T>(UP, PI));
            self.enclosed = true;
            cam.near = 0.03;
            if self.hit_shake != 0.0 {
                cam.p = plus(cam.p, up, T::sin(i.age * 73.0) * self.hit_shake * 0.25)
            }
            if i.boosting {
                cam.p = plus(cam.p, up, T::sin(i.age * 51.0) * 0.025)
            }
        } else {
            self.enclosed = false;
            cam.near = 0.1;
            let mut frame = M::default();
            frame.compose(&i.position, &i.flight_frame, &V::new(1.0, 1.0, 1.0));
            let want = mul(
                look_basis(
                    rotate(V::new(0.0, 0.0, -1.0), i.flight_frame),
                    rotate(UP, i.flight_frame),
                ),
                axis::<T>(V::new(1.0, 0.0, 0.0), -i.view.tilt),
            );
            let pivot = transformed(i.view.center, &frame);
            let snap = fresh || cam.p.distance_to(&pivot) > 80.0;
            cam.q = if snap {
                want
            } else {
                slerp::<T>(cam.q, want, 1.0 - T::exp(-i.view.aim * dt))
            };
            cam.up = rotate(UP, cam.q);
            let origin = transformed(V::new(0.0, 1.5, 0.0), &frame);
            self.follow::<T, C>(i.view, cam, origin, pivot, snap, dt, world);
            self.look_at = pivot;
        }
        let across = if i.first { i.view.first } else { i.view.third };
        let mut fov = 2.0 * T::atan(T::tan(across / 2.0) / cam.aspect) * (180.0 / PI);
        if let Some(time) = self.blend_time {
            if time < i.blend_duration {
                let time = time + dt;
                self.blend_time = Some(time);
                let t = (time / i.blend_duration).min(1.0);
                let a = t * t * (3.0 - 2.0 * t);
                cam.p = mix_vectors(self.blend_p, cam.p, a);
                cam.q = slerp_vectors::<T>(self.blend_q, cam.q, a);
                fov = self.blend_fov + (fov - self.blend_fov) * a;
                let boom = sub(cam.p, i.position);
                cam.p = plus(
                    i.position,
                    boom,
                    clearance(world, i.position, boom, cam.q, 0.08, false),
                );
            }
        }
        cam.fov = fov;
        self.set = true;
    }
}
// A turret seat's camera, as the 2004 game's (the seat's camera on the
// camera section; docs/formats.md "Vehicle seats"). Behind the
// gun, it looks along the aim with the aim frame's up, pitched down by
// TiltValue, from TrackCenter in the aim frame plus TrackOffset in its own
// (Turret.cameraRest); its turn follows the mouse at once (a player's aim
// already has its final direction, as a vehicle seat's), and each of its
// axes eases toward where it should be at MoveTension, 30/s unless the
// class sets it. Five rays from 1.5 m up in the aim frame, the turret
// itself left out, pull it in at walls at once; it eases back out at 6/s.
// More than 80 m from where it should be, it jumps. In first person it is
// at the eye looking straight down the aim. Both fields of view are the
// class's (48 degrees across); zoomed, the first person's, narrowed by the
// zoom.

#[derive(Default)]
pub struct Turret {
    pub fresh: bool,
    pub reach: f64,
    pub fov: Option<f64>,
}
// TurretCamera from gunner.js keeps its own reach and null initial FOV.
// Changing emplacements recreates these three scalars in the outer phase.
// Turret yaw is immediate because input preview already owns its filtering.
// First person is the authored eye, with zero tilt and a fresh chase next time.
// A zero TiltValue skips multiplication, preserving signed quaternion zeros.
// Its section rates are prepared once from the class, outside the frame.
// The same native clearance fan excludes every built turret collision piece.
impl Turret {
    pub fn new() -> Self {
        Self {
            fresh: true,
            reach: 1.0,
            fov: None,
        }
    }
    // TurretCamera.frame entry point across seat, frame, and aim state.
    #[allow(clippy::too_many_arguments)]
    pub fn frame<T: Math, C: Cast>(
        &mut self,
        cam: &mut Camera,
        dt: f64,
        seat: Seat,
        frame: M,
        up: V,
        forward: V,
        eye: V,
        first: bool,
        zoom: f64,
        world: &mut C,
    ) {
        // TurretCamera.updateFov retains a null initial lens: its first frame
        // takes the target exactly, unlike SoldierCamera's initial easing.
        let base = if first || zoom > 1.0 {
            seat.first
        } else {
            seat.third
        };
        let want = 2.0 * T::atan(T::tan(base / 2.0) / zoom.max(1.0));
        let fov = self
            .fov
            .map_or(want, |f| f + (want - f) * (1.0 - T::exp(-25.0 * dt)));
        self.fov = Some(fov);
        cam.fov = 2.0 * T::atan(T::tan(fov / 2.0) / cam.aspect) * (180.0 / PI);
        cam.q = look_basis(scale(forward, -1.0), up);
        if first {
            cam.p = eye;
            self.fresh = true;
            return;
        }
        if seat.tilt != 0.0 {
            cam.q = mul(cam.q, axis::<T>(V::new(1.0, 0.0, 0.0), -seat.tilt));
        }
        let pivot = transformed(seat.center, &frame);
        let target = add(rotate(seat.offset, cam.q), pivot);
        let origin = transformed(V::new(0.0, 1.5, 0.0), &frame);
        let snap = self.fresh || cam.p.distance_to(&target) > 80.0;
        let boom = sub(target, origin);
        let clear = clearance(world, origin, boom, cam.q, 0.08, false);
        if snap || clear < self.reach {
            self.reach = clear
        } else {
            self.reach += (clear - self.reach) * (1.0 - T::exp(-6.0 * dt));
        }
        let target = plus(origin, boom, self.reach);
        if snap {
            cam.p = target
        } else {
            ease_axes::<T>(cam, target, seat.rates, dt);
            let boom = sub(cam.p, origin);
            let clear = clearance(world, origin, boom, cam.q, 0.08, false);
            if clear < 1.0 {
                cam.p = plus(origin, boom, clear)
            }
        }
        self.fresh = false;
    }
}
