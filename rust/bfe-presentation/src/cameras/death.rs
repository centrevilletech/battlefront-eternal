//! DeathShot from src/death-shot.js. The body, killer and drawn view follow
//! exact critically damped springs on the display clock, never the world tick.
// The death camera as this page draws it. The world keeps the original's
// (src/camera.js DeathCamera) as the camera its Classic AI schedules from;
// this one is presentation only. It runs on the page in every host (a local
// battle, a world worker's, a server's) from what the page draws: the body
// where it lies, and the killer the world named (DeathCamera.blame) where he
// is drawn. While he is dead it is the drawn camera's only writer, on the
// frame's own clock.
//
// Every move is a function of the time since the death, eased, over points
// that follow what is drawn through critically damped springs. A spring's
// step is exact for any length of frame, so 60, 144 and uneven frames trace
// the same path. The pose it arrives at passes through springs of its own
// as well, so nothing it reads (a killer getting into a hull, a ray starting
// to meet a wall, a change of style) can make it jump.
//
// Classic is the original's framing and timing (docs/formats.md, "Death
// camera"): it rises to look down on the body from 1 m back toward the view
// it took over and 1.5 m up, 5 m back along that line or 1 m short of what
// two rays meet. From 1.5 s it turns to look over the body at the killer and
// follows him, and goes back to the body while he has none. Its blends are
// the original's fractions (t / 1 s, then (t - 1.5 s) / 2 s) applied at the
// original's 60 frames a second, integrated over time.
//
// Enhanced is a replay as a sports broadcast shows one: a hold on the body
// from a high three-quarter angle, drifting slowly round it; a crane move
// round the body to the reverse angle, looking over it at the killer with
// both in frame while the killing shot's trace runs from him to the body;
// then a push in on the killer, the lens narrowing to frame him, following
// him. With no killer it stays on the body, drifting round it. It is over
// in about 3.3 s, before the corpse leaves the world at 4 s.

use super::{math::*, Camera, Cast};
use three::{Quaternion as Q, Vector3 as V};

#[derive(Clone, Copy, Default)]
struct Follow {
    x: f64,
    v: f64,
    goal: f64,
}
impl Follow {
    fn new(x: f64) -> Self {
        Self { x, v: 0.0, goal: x }
    }
    fn reset(&mut self, x: f64) {
        *self = Self::new(x)
    }
    fn step<M: Math>(&mut self, goal: f64, rate: f64, dt: f64) -> f64 {
        let (x, v) = spring::<M>(self.x, self.v, self.goal, goal, rate, dt);
        self.x = x;
        self.v = v;
        self.goal = goal;
        x
    }
}
// spring in death-shot.js: the moving goal's lag is kept separate from the
// decaying homogeneous solution, so a long frame traces the same path.
// One step of a critically damped spring: x (and its velocity v) after dt,
// following a goal that moved evenly from a0 to a1 over the step. Exact for
// any dt, so how the time is cut into frames doesn't change the path.
// 0 to 1 over [0, 1], starting and stopping without a jolt (smootherstep).

// Ports spring from death-shot.js for a target moving linearly this frame.
// The closed form preserves the same path for different cuts of elapsed time.
// A zero dt returns the prior follower without dividing by the clock.
// Rate is a response in inverse seconds, not a per-frame blend fraction.
// Both the retained value and velocity are returned as stack scalars.
// Tracking and final-pose followers deliberately use separate state.
fn spring<M: Math>(x: f64, v: f64, a0: f64, a1: f64, rate: f64, dt: f64) -> (f64, f64) {
    if dt.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return (x, v);
    }
    let speed = (a1 - a0) / dt;
    let lag = (-2.0 * speed) / rate;
    let c1 = x - a0 - lag;
    let c2 = v - speed + rate * c1;
    let e = M::exp(-rate * dt);
    (
        a1 + lag + (c1 + c2 * dt) * e,
        speed + (c2 - rate * (c1 + c2 * dt)) * e,
    )
}
#[derive(Default)]
struct FollowVector {
    value: V,
    axes: [Follow; 3],
}
impl FollowVector {
    fn reset(&mut self, p: V) {
        self.value = p;
        self.axes = [Follow::new(p.x), Follow::new(p.y), Follow::new(p.z)];
    }
    fn step<M: Math>(&mut self, p: V, rate: f64, dt: f64) -> V {
        self.value = V::new(
            self.axes[0].step::<M>(p.x, rate, dt),
            self.axes[1].step::<M>(p.y, rate, dt),
            self.axes[2].step::<M>(p.z, rate, dt),
        );
        self.value
    }
    // Where it will be in this many seconds at the speed it has (at most
    // limit metres on).
    // A rotation following its goal: the same spring on the turn between them
    // (axis times angle), the goal turning evenly over the step.

    fn ahead(&self, seconds: f64, limit: f64) -> V {
        let mut v = scale(
            V::new(self.axes[0].v, self.axes[1].v, self.axes[2].v),
            seconds,
        );
        v.clamp_length(0.0, limit);
        add(v, self.value)
    }
}
#[derive(Default)]
struct FollowTurn {
    q: Q,
    goal: Q,
    velocity: V,
}
// The shorter way round.
// Where it is, and where the goal went, from the goal's last place.
// The killer where this page draws him, from the world's record of him
// (DeathCamera.blame): { point (his eyes, or a hull's or turret's), size (m,
// for framing), living (he has a body now), friendly }, into out, or null.
// follow: once his life has ended, look for his Character's next body (as
// the original does); otherwise a corpse still lying there is shown.
// refs: { units, characters, vehicles, relation(team) }.

fn log_turn<M: Math>(q: Q) -> V {
    let s = if q.w < 0.0 { -1.0 } else { 1.0 };
    let (x, y, z, w) = (q.x * s, q.y * s, q.z * s, q.w * s);
    let length = M::hypot(x, y, z);
    if length < 1e-12 {
        return V::default();
    }
    let a = 2.0 * M::atan2(length, w);
    V::new((x / length) * a, (y / length) * a, (z / length) * a)
}
fn exp_turn<M: Math>(v: V) -> Q {
    let a = v.length();
    if a < 1e-12 {
        return Q::default();
    }
    let s = M::sin(a / 2.0) / a;
    Q::new(v.x * s, v.y * s, v.z * s, M::cos(a / 2.0))
}
impl FollowTurn {
    fn reset(&mut self, q: Q) {
        self.q = q;
        self.goal = q;
        self.velocity = V::default()
    }
    fn step<M: Math>(&mut self, goal: Q, rate: f64, dt: f64) {
        if dt.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return;
        }
        let inverse = inv(self.goal);
        let o = log_turn::<M>(mul(self.q, inverse));
        let g = log_turn::<M>(mul(goal, inverse));
        let (x, vx) = spring::<M>(o.x, self.velocity.x, 0.0, g.x, rate, dt);
        let (y, vy) = spring::<M>(o.y, self.velocity.y, 0.0, g.y, rate, dt);
        let (z, vz) = spring::<M>(o.z, self.velocity.z, 0.0, g.z, rate, dt);
        self.velocity = V::new(vx, vy, vz);
        self.q = normalize(mul(exp_turn::<M>(V::new(x, y, z)), self.goal));
        self.goal = goal;
    }
}
#[derive(Clone, Copy, Default)]
struct Pose {
    p: V,
    q: Q,
    hfov: f64,
    top: V,
    boom: f64,
}
#[derive(Default)]
struct From {
    p: V,
    q: Q,
    yaw: f64,
    pitch: f64,
    roll: f64,
    hfov: f64,
    bearing: f64,
    radius: f64,
    height: f64,
}
#[derive(Default)]
struct Last {
    valid: bool,
    p: V,
    q: Q,
    velocity: V,
    spin: V,
}
pub struct Input {
    pub serial: f64,
    pub hfov: f64,
    pub classic: bool,
    pub feet: Option<V>,
    pub chest: Option<V>,
    pub killer: Option<Killer>,
}
#[derive(Clone, Copy)]
pub struct Killer {
    pub point: V,
    pub size: f64,
    pub living: bool,
    pub friendly: bool,
}
#[derive(Default)]
pub struct Death {
    pub active: bool,
    pub serial: f64,
    pub time: f64,
    classic: bool,
    switch_from: Option<bool>,
    switch_at: f64,
    from: From,
    start_body: V,
    anchor: V,
    body: FollowVector,
    killer: FollowVector,
    killer_aim: V,
    killer_size: Follow,
    presence: Follow,
    pub known: bool,
    known_at: f64,
    living: bool,
    friendly: bool,
    shot_from: V,
    bearing_r: Option<f64>,
    booms: [Follow; 3],
    fresh: [bool; 3],
    position: FollowVector,
    turn: FollowTurn,
    hfov: Follow,
    drawn: Pose,
    last: Last,
    handoff_velocity: V,
    handoff_spin: V,
    pub trace: Trace,
}
#[derive(Default)]
pub struct Trace {
    pub visible: bool,
    pub from: V,
    pub to: V,
    pub friendly: bool,
    pub head: f64,
    pub fade: f64,
    pub since: f64,
}
fn rates(classic: bool) -> [f64; 3] {
    if classic {
        [40.0, 45.0, 30.0]
    } else {
        [8.0, 10.0, 5.0]
    }
}
fn classic_blend<M: Math>(s: f64, rate: f64) -> f64 {
    if s <= 0.0 {
        return 0.0;
    }
    let f = rate * s;
    if f >= 1.0 {
        return 1.0;
    }
    1.0 - M::exp(-60.0 * (((1.0 - f) * M::log(1.0 - f)) / rate + s))
}
impl Death {
    // Every frame he is alive: the drawn view, for the motion it hands over.

    pub fn watch<M: Math>(&mut self, cam: &Camera, dt: f64) {
        if self.active {
            self.stop()
        }
        if self.last.valid && dt > 0.0 {
            self.last.velocity = scale(sub(cam.p, self.last.p), 1.0 / dt);
            self.last.spin = scale(log_turn::<M>(mul(cam.q, inv(self.last.q))), 1.0 / dt);
        } else {
            self.last.velocity = V::default();
            self.last.spin = V::default();
        }
        self.last.p = cam.p;
        self.last.q = cam.q;
        self.last.valid = dt > 0.0 || self.last.valid;
    }
    pub fn stop(&mut self) {
        self.active = false;
        self.last.valid = false;
        self.trace.visible = false;
    }
    // DeathShot.bodyPoint keeps two distinct notions of the corpse's middle.
    // Classic uses a fixed height above feet, matching the original timing.
    // Enhanced uses the displayed ribcage while its death animation plays.
    // A missing chest falls back to feet without inventing a scene handle.
    // The follower retains its previous point after that input is sampled.
    fn body_point(&self, input: &Input) -> V {
        if !input.classic {
            if let Some(chest) = input.chest {
                return chest;
            }
        }
        let feet = input.feet.unwrap_or(self.start_body);
        V::new(feet.x, feet.y + 0.8, feet.z)
    }
    // From the view as it is drawn now.
    // Where the view stood round the body: bearing (the way from the body,
    // as a heading), distance across and height. From his eyes, his heading.
    // Classic's first near point: 1 m from the body toward where the view
    // looked from, level, and 1.5 m above the feet.
    // The view glides on with the motion it had for a moment, as a camera on
    // a crane would; the poses start from rest where it is.

    fn begin<M: Math>(&mut self, cam: &Camera, input: &Input) {
        self.active = true;
        self.serial = input.serial;
        self.time = 0.0;
        self.classic = input.classic;
        self.switch_from = None;
        let e = angles::<M>(cam.q);
        let body = self.body_point(input);
        self.start_body = body;
        self.body.reset(body);
        let dx = cam.p.x - body.x;
        let dz = cam.p.z - body.z;
        let radius = M::hypot2(dx, dz);
        let bearing = if radius > 0.3 { M::atan2(dx, dz) } else { e.y };
        self.from = From {
            p: cam.p,
            q: cam.q,
            yaw: e.y,
            pitch: e.x,
            roll: e.z,
            hfov: 2.0 * M::atan(M::tan(cam.fov * DEG / 2.0) * cam.aspect),
            bearing: e.y + wrap(bearing - e.y),
            radius,
            height: cam.p.y - body.y,
        };
        let mut back = rotate(V::new(0.0, 0.0, 1.0), cam.q);
        back.y = 0.0;
        if back.length_sq() < 1e-8 {
            back = V::new(0.0, 0.0, 1.0)
        }
        back = norm(back);
        let feet = input.feet.unwrap_or(body);
        self.anchor = V::new(feet.x + back.x, feet.y + 1.5, feet.z + back.z);
        self.known = false;
        self.living = false;
        self.presence.reset(0.0);
        self.bearing_r = None;
        self.fresh = [true; 3];
        // Preserve the incoming view's velocity for a short crane glide,
        // clamped as DeathShot.begin does before its position springs start.
        if self.last.valid {
            self.handoff_velocity = self.last.velocity;
            self.handoff_velocity.clamp_length(0.0, 8.0);
            self.handoff_spin = self.last.spin;
            self.handoff_spin.clamp_length(0.0, 2.0);
        } else {
            self.handoff_velocity = V::default();
            self.handoff_spin = V::default()
        }
        self.position.reset(cam.p);
        self.turn.reset(cam.q);
        self.hfov.reset(self.from.hfov);
        self.drawn.p = cam.p;
        self.drawn.q = cam.q;
        self.trace.visible = false;
    }
    // The body's middle: Classic, the original's 0.8 m above the feet;
    // Enhanced, the ribcage where it is drawn.
    // A critically damped follower trails a steady mover by 2 / rate of his
    // speed: aiming that far ahead of him keeps a runner in the middle.

    // DeathShot.track follows the drawn body and generation-checked killer.
    // Body position comes from the ribcage for Enhanced and feet for Classic.
    // Presence is eased separately from position, preserving a lost killer.
    // Size changes when a killer boards a vehicle; its own follower prevents
    // an abrupt lens jump from soldier scale to hull scale.
    // Classic can follow the same Character's next life, as its input names.
    // Enhanced retains the corpse that actually caused this death.
    // Leading the killer compensates for follower lag, capped at six metres.
    // A late name starts its reveal after the same short delay as JavaScript.
    fn track<M: Math>(&mut self, dt: f64, input: &Input) {
        let body = self.body_point(input);
        self.body.step::<M>(body, 10.0, dt);
        if let Some(k) = input.killer {
            if !self.known {
                self.known = true;
                self.known_at = self.time;
                self.killer.reset(k.point);
                self.killer_size.reset(k.size);
                self.shot_from = k.point;
            } else {
                self.killer.step::<M>(k.point, 12.0, dt);
                self.killer_size.step::<M>(k.size, 4.0, dt);
            }
            self.living = k.living;
            self.friendly = k.friendly;
        } else {
            self.living = false
        }
        self.presence
            .step::<M>(if self.living { 1.0 } else { 0.0 }, 7.0, dt);
        self.killer_aim = self
            .killer
            .ahead(2.0 / 12.0 + 2.0 / rates(self.classic)[1], 6.0);
    }
    // DeathShot.reach keeps each boom's obstruction history independent.
    // A wall pulls in quickly while a cleared ray lets the boom out slowly.
    // Sharing this value between styles would jump during a style change.
    // Each stored fraction is bounded by the cast's clear segment.
    // There is no scene query or allocation in this follower.
    fn reach<M: Math>(&mut self, i: usize, clear: f64, dt: f64) -> f64 {
        if self.fresh[i] {
            self.fresh[i] = false;
            self.booms[i].reset(clear);
            return clear;
        }
        let rate = if clear < self.booms[i].x { 25.0 } else { 2.5 };
        clamp(self.booms[i].step::<M>(clear, rate, dt), 0.0, 1.0)
    }
    fn place<M: Math, C: Cast>(
        &mut self,
        near: V,
        far: V,
        boom: usize,
        dt: f64,
        world: &mut C,
    ) -> Pose {
        let mut dir = sub(far, near);
        if dir.length_sq() < 1e-10 {
            dir = rotate(V::new(0.0, 0.0, -1.0), self.turn.q)
        }
        dir = norm(dir);
        let back = scale(dir, -1.0);
        let mut clear: f64 = 1.0;
        for lift in [0.75, -0.75] {
            if let Some(d) = world.cast(V::new(near.x, near.y + lift, near.z), back, 6.0, true) {
                clear = clear.min(d / 6.0);
            }
        }
        let reach = self.reach::<M>(boom, clear, dt);
        let p = plus(near, dir, 1.0 - 6.0 * reach);
        Pose {
            p,
            q: look(p, add(p, dir), UP),
            ..Pose::default()
        }
    }
    // Classic: the original's two poses, blended as it blended them.
    // The original's pose from a near point toward a far one: 5 m back along
    // the line, or 1 m short of what the two rays meet (terrain, buildings
    // and props), looking along it with the world's up. The rays' reach is
    // followed in fast and out slowly.
    // A boom's clear fraction: in quickly when something comes between, out
    // slowly when it clears.

    fn classic<M: Math, C: Cast>(&mut self, dt: f64, input: &Input, world: &mut C) -> Pose {
        let rise = classic_blend::<M>(self.time, 1.0);
        let a = self.place::<M, C>(self.anchor, self.body.value, 0, dt, world);
        let mut want = Pose {
            p: mix_vectors(self.from.p, a.p, rise),
            q: slerp_vectors::<M>(self.from.q, a.q, rise),
            hfov: lerp(self.from.hfov, input.hfov, rise),
            ..Pose::default()
        };
        let presence = clamp(self.presence.x, 0.0, 1.0);
        if self.known && presence > 0.0 {
            let turn = classic_blend::<M>(self.time - 1.5f64.max(self.known_at), 0.5) * presence;
            if turn > 0.0 {
                let k = self.place::<M, C>(self.body.value, self.killer_aim, 1, dt, world);
                want.p = mix(want.p, k.p, turn);
                want.q = slerp::<M>(want.q, k.q, turn);
            }
        }
        want
    }
    // DeathShot.place2 expresses a pose around a tracked point by bearing.
    // Radius lies on the horizontal plane; height is added independently.
    // The old expression order matters when a shot settles near the body.
    // The resulting target is still subject to final spring/collision rules.
    // No camera or subject identity is inferred from this geometric helper.
    fn place2<M: Math>(pivot: V, bearing: f64, radius: f64, height: f64) -> V {
        V::new(
            pivot.x + M::sin(bearing) * radius,
            pivot.y + height,
            pivot.z + M::cos(bearing) * radius,
        )
    }
    // Enhanced: the hold, the reveal and the push in, each a place round the
    // body (bearing, distance across, height) and a look (yaw, pitch), blended
    // round the body as a crane moves.
    // The hold: from the view it took over to a high three-quarter look at
    // the body, on the side it looked from, drifting round it.
    // The reverse angle: across the body from the killer, kept the same
    // way round as he moves (and turning the way it drifts if he is
    // right behind it).
    // Looking over the body at him: him in frame, the body below him as
    // far as the frame allows.
    // The share of the way from the body to him to look: him within
    // KILLER_EDGE of the frame's half, the body within BODY_FRAME of
    // it, else him first.
    // The push in: closer, looking at him, the lens framing him.
    // Clear of walls: a boom from just above the body (resolve), holding
    // as the view leaves the place it took over (which was where it drew).
    // A place round the pivot: on the bearing (a heading from it), radius
    // across, height up.

    // DeathShot.enhanced constructs the requested broadcast pose in stages.
    // The high body hold occupies the first nine tenths of a second.
    // A slow bearing drift follows, expressed on the death's visual clock.
    // With a known killer the reveal reverses the body/killer framing.
    // Its two-point lens keeps both characters inside authored frame limits.
    // Then the settling interval moves toward and frames the killer alone.
    // A missing killer retains the body hold rather than aiming at zero.
    // Body size and killer size remain metres, before projection to a lens.
    // The minimum vertical field prevents an unbounded telephoto close-up.
    // The pose still passes through final followers and collision resolution.
    // None of these stages overwrites the authority DeathCamera's position.
    fn enhanced<M: Math>(&mut self, input: &Input, cam: &Camera) -> Pose {
        let t = self.time;
        let f = &self.from;
        let body = self.body.value;
        let hold = ease(t / 0.9);
        let drift = 0.12 * (t - 0.6 * (1.0 - M::exp(-t / 0.6)));
        let bearing_a = f.yaw + drift;
        let pitch_a = -M::atan2(2.6 + 0.2, 3.6);
        let mut bearing = lerp(f.bearing, bearing_a, hold);
        let mut radius = lerp(f.radius, 3.6, hold);
        let mut height = lerp(f.height, 2.6, hold);
        let mut yaw = lerp(f.yaw, bearing_a, hold);
        let mut pitch = lerp(f.pitch, pitch_a, hold);
        let roll = lerp(f.roll, 0.0, hold);
        let mut hfov = lerp(f.hfov, input.hfov, hold);
        let pivot = mix_vectors(self.start_body, body, hold);
        if self.known {
            let start = 1.0f64.max(self.known_at + 0.15);
            let reveal = ease((t - start) / 1.25);
            let settle = ease((t - start - 1.0) / 1.2);
            if reveal > 0.0 {
                // Keep the reverse angle on its original side when the killer
                // crosses behind the body; no single-frame wrap is visible.
                let kx = body.x - self.killer.value.x;
                let kz = body.z - self.killer.value.z;
                let across = M::hypot2(kx, kz);
                let goal = if across > 1e-3 {
                    M::atan2(kx, kz)
                } else {
                    bearing_a
                };
                let goal = lerp(
                    bearing_a,
                    bearing_a + wrap(goal - bearing_a),
                    ease((across - 0.5) / 2.5),
                );
                let br = if let Some(br) = self.bearing_r {
                    br + wrap(goal - br)
                } else {
                    let mut turn = wrap(goal - bearing_a);
                    if turn.abs() > PI * 0.9 && turn < 0.0 {
                        turn += 2.0 * PI
                    }
                    bearing_a + turn
                };
                self.bearing_r = Some(br);
                let pos_a = Self::place2::<M>(pivot, br, 3.4, 1.3);
                let half = M::atan(M::tan(input.hfov / 2.0) / cam.aspect);
                let to_body = norm(sub(body, pos_a));
                let to_killer = norm(sub(self.killer_aim, pos_a));
                let apart = M::acos(clamp(to_body.dot(&to_killer), -1.0, 1.0)).max(1e-3);
                let most = (0.75 * half) / apart;
                let least = 1.0 - (0.55 * half) / apart;
                let share = if least > most {
                    clamp(least, 0.0, 1.0)
                } else {
                    clamp(0.5, least, most)
                };
                let aim = norm(mix(to_body, to_killer, share));
                let yaw_r = br + wrap(M::atan2(-aim.x, -aim.z) - br);
                let pitch_r = M::asin(clamp(aim.y, -1.0, 1.0));
                let pos_k = Self::place2::<M>(pivot, br, 2.6, 1.35);
                let toward = sub(self.killer_aim, pos_k);
                let distance = toward.length().max(0.5);
                let toward = scale(toward, 1.0 / distance);
                let yaw_k = br + wrap(M::atan2(-toward.x, -toward.z) - br);
                let pitch_k = M::asin(clamp(toward.y, -1.0, 1.0));
                let base = 2.0 * half;
                let vfov = clamp(
                    2.0 * M::atan(self.killer_size.x / 0.4 / 2.0 / distance),
                    (11.0 * DEG).min(base),
                    base,
                );
                let hfov_k = 2.0 * M::atan(M::tan(vfov / 2.0) * cam.aspect);
                bearing = lerp(bearing, br, reveal);
                radius = lerp(radius, 3.4, reveal);
                height = lerp(height, 1.3, reveal);
                yaw = lerp(yaw, yaw_r, reveal);
                pitch = lerp(pitch, pitch_r, reveal);
                radius = lerp(radius, 2.6, settle);
                height = lerp(height, 1.35, settle);
                yaw = lerp(yaw, yaw_k, settle);
                pitch = lerp(pitch, pitch_k, settle);
                hfov = lerp(hfov, hfov_k, settle);
            }
        }
        Pose {
            p: Self::place2::<M>(pivot, bearing, radius, height),
            q: euler::<M>(clamp(pitch, -1.45, 1.45), yaw, roll, false),
            hfov,
            top: plus(pivot, UP, 0.45),
            boom: hold,
        }
    }
    fn pose<M: Math, C: Cast>(
        &mut self,
        classic: bool,
        dt: f64,
        input: &Input,
        cam: &Camera,
        world: &mut C,
    ) -> Pose {
        if classic {
            self.classic::<M, C>(dt, input, world)
        } else {
            self.enhanced::<M>(input, cam)
        }
    }
    // The drawn pose: where the springs are, clear of walls (Enhanced's boom
    // comes in after its slow follow, so it never lags into a wall, as the
    // chase camera resolves the place it draws), gliding on with the motion
    // the view it took over had.

    // DeathShot.resolve follows the requested pose, lens and quaternion.
    // Enhanced collision is resolved after following, on the eye we publish.
    // Otherwise follower lag could briefly leave a camera through a new wall.
    // Handoff translation and turn preserve the view's preceding motion.
    // Their exponential decay is independent of the body and killer springs.
    // Classic and Enhanced can blend without resetting any of these followers.
    // The world up vector remains the camera's authored up throughout death.
    fn resolve<M: Math, C: Cast>(&mut self, dt: f64, want: Pose, world: &mut C) {
        self.drawn.p = self.position.value;
        self.drawn.q = self.turn.q;
        if want.boom > 0.0 {
            let offset = sub(self.drawn.p, want.top);
            let length = offset.length();
            let mut clear = 1.0;
            if length > 1e-3 {
                if let Some(d) =
                    world.cast(want.top, scale(offset, 1.0 / length), length + 0.35, true)
                {
                    clear = clamp((d - 0.35) / length, 0.1, 1.0);
                }
            }
            self.drawn.p = plus(
                want.top,
                offset,
                lerp(1.0, self.reach::<M>(2, clear, dt), want.boom),
            );
        }
        let glide = self.time * M::exp(-4.0 * self.time);
        self.drawn.p = plus(self.drawn.p, self.handoff_velocity, glide);
        self.drawn.q = normalize(mul(
            exp_turn::<M>(scale(self.handoff_spin, glide)),
            self.drawn.q,
        ));
    }
    // Enhanced: the killing shot's trace, from where the killer stood when he
    // was named to the body where it lies, as the reveal starts.
    // Into the drawn camera, with its field of view for the camera's shape.

    fn show_trace(&mut self) {
        let since = self.time - 1.0f64.max(self.known_at + 0.15);
        self.trace.visible = !self.classic && self.known && (0.0..=2.3).contains(&since);
        if !self.trace.visible {
            return;
        }
        self.trace.from = self.shot_from;
        self.trace.to = self.body.value;
        self.trace.friendly = self.friendly;
        self.trace.head = ease(since / 0.5);
        self.trace.fade = (1.0 - ease((since - 1.7) / (2.3 - 1.7))) * ease(since / 0.12);
        self.trace.since = since;
    }
    // One frame. input: { serial (the world's death count), hfov (the death
    // camera's field of view across), style, feet, chest (Vector3s, as
    // drawn; chest may be null), killer (drawnKiller's, or null) }.
    // A change of style: from the old one's pose to the new one's.

    // Ports DeathShot.step, called once on the visual clock when death wins.
    // A new serial starts from this frame's incoming player/vehicle/recon view.
    // Repeated frames of the same serial never restart the handoff.
    // A style change blends from the old style while both booms retain state.
    // Freeze uses zero visual dt without substituting the world tick clock.
    // Trace outputs are geometric facts for the retained DeathTrace mesh.
    // Its previous-camera draw ordering is preserved by the page publisher.
    // A stopped or Classic trace is hidden without advancing another owner.
    pub fn frame<M: Math, C: Cast>(
        &mut self,
        dt: f64,
        cam: &mut Camera,
        input: &Input,
        world: &mut C,
    ) {
        if !self.active || input.serial != self.serial {
            self.begin::<M>(cam, input)
        }
        if dt > 0.0 {
            self.time += dt;
            if input.classic != self.classic {
                self.switch_from = Some(self.classic);
                self.switch_at = self.time;
                self.classic = input.classic
            }
            if self.switch_from.is_some() && self.time - self.switch_at >= 0.8 {
                self.switch_from = None
            }
            self.track::<M>(dt, input);
            let mut want = self.pose::<M, C>(self.classic, dt, input, cam, world);
            let mut rate = rates(self.classic);
            if let Some(from) = self.switch_from {
                let share = ease((self.time - self.switch_at) / 0.8);
                let old = self.pose::<M, C>(from, dt, input, cam, world);
                want = Pose {
                    p: mix_vectors(old.p, want.p, share),
                    q: slerp_vectors::<M>(old.q, want.q, share),
                    hfov: lerp(old.hfov, want.hfov, share),
                    top: mix_vectors(
                        if old.boom != 0.0 { old.top } else { want.top },
                        if want.boom != 0.0 { want.top } else { old.top },
                        share,
                    ),
                    boom: lerp(old.boom, want.boom, share),
                };
                let r = rates(from);
                for i in 0..3 {
                    rate[i] = lerp(r[i], rate[i], share)
                }
            }
            self.position.step::<M>(want.p, rate[0], dt);
            self.turn.step::<M>(want.q, rate[1], dt);
            self.hfov.step::<M>(want.hfov, rate[2], dt);
            self.resolve::<M, C>(dt, want, world);
            self.show_trace();
        }
        cam.p = self.drawn.p;
        cam.q = self.drawn.q;
        cam.fov = 2.0 * M::atan(M::tan(self.hfov.x / 2.0) / cam.aspect) * (180.0 / PI);
    }
}
