//! The droideka (src/droideka.js, cis_inf_droideka, ODF class walkerdroid)
//! as the 2004 game runs it: its modes (each a clip from its own bank,
//! played at AnimationSpeed), its walk by the clips' root travel, the ball
//! it rolls up into, its shield, and its collision against the buildings
//! and the terrain. Its hidden gun rig, posed every tick for its muzzles, is
//! `crate::rig::droideka`.
//!
//! Functions take the unit, not the body alone: entering a mode can call
//! out to the battle in the middle of a tick (a droideka at the end of its
//! death clip goes up there and then, and its blast hurts what is near it
//! where it stands at that moment).

use super::damage::contact_damage;
use super::shapes::{arr, v3};
use super::unit::or_zero;
use super::world::{GatherPolygon, UnitsWorld};
use crate::collision::spheres::closest_point_to_point;
use crate::gen::{BonePose, Droideka, RigAnimator, Unit, UnitBody};
use crate::pose::Clip;
use crate::rig::droideka::{DroidekaClips, DroidekaRig, DroidekaState};
use three::{clamp, deg_to_rad, Quaternion, Vector3};
use std::rc::Rc;

const GRAVITY: f64 = 9.8;
/// Next mode by [mode][action]; actions: 0 stop, 1 stop (in reverse), 2
/// forward, 3 backward, 4 turn left, 5 turn right, 6 die, 7 roll up.
pub const NEXT: [[f64; 8]; 14] = [
    [0.0, 0.0, 5.0, 9.0, 1.0, 2.0, 3.0, 11.0],
    [0.0, 0.0, 5.0, 9.0, 0.0, 0.0, 3.0, 11.0],
    [0.0, 0.0, 5.0, 10.0, 0.0, 0.0, 3.0, 11.0],
    [4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 11.0],
    [4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 11.0],
    [9.0, 0.0, 7.0, 10.0, 9.0, 9.0, 3.0, 11.0],
    [10.0, 0.0, 8.0, 9.0, 10.0, 10.0, 3.0, 11.0],
    [10.0, 5.0, 8.0, 8.0, 10.0, 10.0, 3.0, 11.0],
    [9.0, 7.0, 7.0, 7.0, 9.0, 9.0, 3.0, 11.0],
    [0.0, 5.0, 5.0, 8.0, 1.0, 2.0, 3.0, 11.0],
    [0.0, 7.0, 5.0, 7.0, 1.0, 2.0, 3.0, 11.0],
    [12.0, 12.0, 12.0, 12.0, 12.0, 12.0, 4.0, 12.0],
    [13.0, 13.0, 12.0, 12.0, 12.0, 12.0, 4.0, 12.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0],
];
const FOLD: f64 = 11.0;
const BALL: f64 = 12.0;
const DEATH: f64 = 3.0;
const DEAD: f64 = 4.0;
/// A turn clip brings the legs round 30 degrees.
const TURN_STEP: f64 = std::f64::consts::PI / 6.0;
/// 1/s, the legs' lag dying away outside idle and turns.
const LEG_EASE: f64 = 3.2;
/// m, p_base's radius, deployed.
const BODY_RADIUS: f64 = 1.0;
/// normal.y above which a deployed contact is ground.
const GROUND: f64 = 0.7;
const BALL_GROUND: f64 = 0.6;
/// The original's 13 m/s cap with one physics step.
const TRACTION: f64 = 0.65;
/// m of clearance the unfold test wants on an axis.
const HEADROOM: f64 = 1.5;
const UP: Vector3 = Vector3 { x: 0.0, y: 1.0, z: 0.0 };
const DOWN: Vector3 = Vector3 { x: 0.0, y: -1.0, z: 0.0 };

/// What a droideka's tick reads of its class's ODF definition (`def`),
/// None where the class leaves a key unset (`??` takes the default then).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DroidekaDef {
    pub max_pitch_speed: Option<f64>,
    pub ball_turn_speed: Option<f64>,
    pub ball_max_lean: Option<f64>,
    pub ball_acceleration: Option<f64>,
    pub ball_rolling_friction: Option<f64>,
}

/// A droideka's clips by mode (src/droideka.js CLIPS: `clips.get(name)`).
#[derive(Clone, Default)]
pub struct ModeClips {
    pub by_mode: [Option<Rc<Clip>>; 14],
}

impl ModeClips {
    pub fn clip(&self, mode: f64) -> Option<&Clip> {
        if (0.0..14.0).contains(&mode) && mode.fract() == 0.0 {
            self.by_mode[mode as usize].as_deref()
        } else {
            None
        }
    }

    pub fn view(&self) -> DroidekaClips<'_> {
        DroidekaClips { by_mode: std::array::from_fn(|i| self.by_mode[i].as_deref()) }
    }
}

/// What a droideka's tick needs besides its state: its class's definition,
/// its clips and its hidden gun rig (made by `arm`; none before).
pub struct DroidekaParts {
    pub def: DroidekaDef,
    pub clips: Rc<ModeClips>,
    pub rig: Option<Box<DroidekaRig>>,
}

impl Droideka {
    /// `rolled`: folding, rolled or unfolding.
    pub fn rolled(&self) -> bool {
        self.mode >= FOLD
    }
}

/// length(mode): the mode's clip's frames at 30 fps, sped up; 0 without one.
fn length(d: &Droideka, clips: &ModeClips, mode: f64) -> f64 {
    match clips.clip(mode) {
        Some(clip) => clip.frames as f64 / 30.0 / d.speed,
        None => 0.0,
    }
}

/// travel(clip, t): DummyRoot's forward travel at a time on the mode's clock.
fn travel(d: &Droideka, clip: &Clip, t: f64) -> f64 {
    if d.root_index < 0.0 {
        return 0.0;
    }
    let Some(k) = clip.tracks.iter().position(|&b| b as f64 == d.root_index) else { return 0.0 };
    let frames = clip.frames as f64;
    let f = v8math::min(v8math::max(t * d.speed * clip.fps, 0.0), frames - 1.0);
    let i0 = f.floor();
    let i1 = v8math::min(i0 + 1.0, frames - 1.0);
    let z = |i: f64| clip.data[(k * clip.frames + i as usize) * 7 + 6] as f64;
    let z0 = z(i0);
    let z1 = z(i1);
    z0 + (z1 - z0) * (f - i0)
}

/// want(move, dead): the action the droideka wants now.
fn want(d: &Droideka, mv: f64, dead: bool) -> usize {
    if dead {
        return 6;
    }
    if mv.abs() < 1e-3 {
        return if d.reverse { 1 } else { 0 };
    }
    if mv > 0.0 {
        2
    } else {
        3
    }
}

fn body(unit: &mut Unit) -> &mut Droideka {
    match &mut unit.soldier {
        UnitBody::Droideka(d) => d,
        UnitBody::Soldier(_) => unreachable!("a droideka's tick on a soldier"),
    }
}

/// enter(mode, carry): a mode from its start (or `carry` into it; the
/// walking steps from their end in reverse).
fn enter(id: u32, unit: &mut Unit, mode: f64, carry: f64, world: &mut dyn UnitsWorld, clips: &ModeClips) {
    let d = body(unit);
    let was = d.mode;
    d.mode = mode;
    // Only the walking steps play in reverse.
    if mode < 5.0 || mode > 10.0 {
        d.reverse = false;
    }
    d.time = if d.reverse { length(d, clips, mode) - carry } else { carry };
    if was == BALL && mode != BALL {
        d.velocity = [0.0, 0.0, 0.0];
        d.legs = 0.0;
    }
    if mode == BALL {
        d.spin = 0.0;
    }
    if mode == DEATH {
        d.blast_clock = 0.0;
    }
    if mode == DEAD && was != DEAD {
        // onExploded: combat.onBlowApart(0), the game's blast and pieces.
        world.blow_apart(id, unit, 0.0);
    }
}

/// tick(input, dt) (src/droideka.js:382), on the unit's `input`.
pub fn tick(id: u32, unit: &mut Unit, dt: f64, world: &mut dyn UnitsWorld, parts: &mut DroidekaParts) {
    let input = unit.input.clone();
    tick_input(id, unit, &input, dt, world, parts);
}

/// The same body tick with a caller's input, without changing Unit.input.
/// The local player's input is separate from his unit's intent/input rows.
pub fn tick_input(id: u32, unit: &mut Unit, input: &crate::gen::MoveInput, dt: f64, world: &mut dyn UnitsWorld, parts: &mut DroidekaParts) {
    let def = parts.def;
    let clips = parts.clips.clone();
    let d = body(unit);
    d.previous = d.position;
    d.state_time += dt;
    d.collision_damage = 0.0;
    let dead = d.dead;
    let mv = if dead { 0.0 } else { or_zero(input.forward) };
    let strafe = if dead { 0.0 } else { or_zero(input.right) };
    // The strafe keys turn it, and the mouse.
    let mut turn = if dead { 0.0 } else { clamp(or_zero(input.turn) + strafe, -1.0, 1.0) };
    let mouse_turn = if dead { 0.0 } else { d.look_x * (d.max_turn / 2048.0) };
    // A controller's pitch stick turns at MaxPitchSpeed (a bot's).
    let stick_pitch = if dead { 0.0 } else { or_zero(input.pitch) * def.max_pitch_speed.unwrap_or(0.5) * dt };
    let mouse_pitch = if dead { 0.0 } else { d.look_y * (d.max_turn / 2048.0) + stick_pitch };
    d.look_x = 0.0;
    d.look_y = 0.0;
    if d.mode == FOLD {
        turn = 0.0;
    }

    // Fold, or unfold where there is room.
    if input.r#use && !dead {
        let mode = body(unit).mode;
        if mode == BALL {
            if room_to_stand(body(unit), world) {
                enter(id, unit, NEXT[BALL as usize][0], 0.0, world, &clips);
            }
        } else if mode < FOLD && mode != DEATH && mode != DEAD {
            enter(id, unit, NEXT[mode as usize][7], 0.0, world, &clips);
        }
    }
    let mode = body(unit).mode;
    if dead && mode != DEATH && mode != DEAD {
        enter(id, unit, NEXT[mode as usize][6], 0.0, world, &clips);
    }

    let d = body(unit);
    d.aim_pitch = clamp(d.aim_pitch + mouse_pitch, d.pitch_limits[0], d.pitch_limits[1]);
    // The shield drains while up and refills while down.
    let rolled = d.rolled();
    if let Some(shield) = &mut d.shield {
        shield.value = clamp(shield.value + (if shield.on { shield.drain } else { shield.refill }) * dt, 0.0, shield.max);
        if shield.value <= 0.0 || rolled {
            shield.on = false;
        }
    }
    if d.mode == BALL {
        ball_move(d, &def, dt, mv, turn, mouse_turn);
    } else {
        walk_move(id, unit, dt, mv, turn, mouse_turn, world, &clips);
    }

    // Every displacement resolved, gravity included, before its place shows.
    let d = body(unit);
    integrate(d, dt);
    collide(id, d, world);
    d.settled = d.position;

    // Dying: blasts at its nodes every 0.4 s, then it explodes.
    if d.mode == DEATH {
        d.blast_clock -= dt;
        if d.blast_clock <= 0.0 {
            d.blast_clock += 0.4;
            world.death_blast(id, unit);
        }
    }
}

fn integrate(d: &mut Droideka, dt: f64) {
    d.velocity[1] -= GRAVITY * dt;
    if d.mode == BALL {
        let mut p = v3(d.position);
        p.add_scaled_vector(&v3(d.velocity), dt);
        d.position = arr(&p);
    } else {
        d.position[1] += d.velocity[1] * dt;
    }
}

/// walkMove: deployed, the clips walk it and the body turns over its legs.
#[allow(clippy::too_many_arguments)]
fn walk_move(id: u32, unit: &mut Unit, dt: f64, mv: f64, turn: f64, mouse_turn: f64, world: &mut dyn UnitsWorld, clips: &ModeClips) {
    let d = body(unit);
    let rate = d.max_turn;
    let a = turn * rate * dt + mouse_turn;
    d.aim_yaw -= a;
    d.legs += a;
    let moving = 5.0 * mv.abs() > turn.abs();
    let truthy = |x: f64| x != 0.0 && !x.is_nan();
    if d.mode == 0.0 && moving {
        enter(id, unit, NEXT[0][if mv > 0.0 { 2 } else { 3 }], 0.0, world, clips);
    } else if d.mode == 0.0 && (truthy(turn) || truthy(mouse_turn)) && v8math::sin(d.legs / 6.0).abs() > v8math::sin(d.turn_threshold / 6.0) {
        // The legs left behind on the left: the right turn brings them round.
        let next = NEXT[0][if d.legs > 0.0 { 5 } else { 4 }];
        enter(id, unit, next, 0.0, world, clips);
    }
    let d = body(unit);
    if d.mode > 2.0 && d.mode != DEATH && d.mode != DEAD {
        d.legs *= v8math::exp(-LEG_EASE * dt);
    }
    // Walking backward plays the steps in reverse; a change of sign turns
    // the clip round where it is.
    if d.mode >= 5.0 && d.mode <= 10.0 && moving {
        d.reverse = mv < 0.0;
    }
    advance(id, unit, dt, mv, moving, world, clips);
}

/// advance: the mode's clip run, moving by its root travel, and on to the
/// next mode at its end.
#[allow(clippy::too_many_arguments)]
fn advance(id: u32, unit: &mut Unit, dt: f64, mv: f64, moving: bool, world: &mut dyn UnitsWorld, clips: &ModeClips) {
    let mut left = dt;
    let mut walked = 0.0;
    // Its walking speed, for those who read a velocity.
    let heading = body(unit).aim_yaw;
    let report = |d: &mut Droideka, walked: f64| {
        let speed = if dt > 0.0 { walked / dt } else { 0.0 };
        d.velocity[0] = v8math::sin(heading) * speed;
        d.velocity[2] = v8math::cos(heading) * speed;
    };
    let mut guard = 0;
    while guard < 4 && left > 0.0 {
        let d = body(unit);
        let mode = d.mode;
        let length = length(d, clips, mode);
        let Some(clip) = clips.clip(mode) else { break };
        if mode == BALL || mode == DEAD {
            break;
        }
        let step = left * if d.reverse && (5.0..=10.0).contains(&mode) { -1.0 } else { 1.0 };
        let from = d.time;
        let mut to = from + step;
        let mut over = 0.0;
        if to > length {
            over = to - length;
            to = length;
        } else if to < 0.0 {
            over = -to;
            to = 0.0;
        }
        // Root travel follows the aim at once; the legs' lag only shows.
        if (5.0..=10.0).contains(&mode) {
            let travel = travel(d, clip, to) - travel(d, clip, from);
            let heading = d.aim_yaw;
            d.position[0] += v8math::sin(heading) * travel;
            d.position[2] += v8math::cos(heading) * travel;
            walked += travel;
        }
        if mode == 1.0 || mode == 2.0 {
            // The turn clip brings the legs round toward the body.
            let share = (to - from) / v8math::max(length, 1e-3);
            d.legs -= v8math::sign(d.legs) * v8math::min(d.legs.abs(), TURN_STEP * share.abs());
        }
        d.time = to;
        left = over;
        if over <= 0.0 && to > 0.0 && to < length {
            report(d, walked);
            return;
        }
        if mode == DEATH && to >= length {
            enter(id, unit, DEAD, 0.0, world, clips);
            return;
        }
        let action = want(d, if moving { mv } else { 0.0 }, d.dead);
        enter(id, unit, NEXT[mode as usize][action], 0.0, world, clips);
        guard += 1;
    }
    report(body(unit), walked);
}

/// ballMove: the ball's spin, turn and lean into a turn.
fn ball_move(d: &mut Droideka, def: &DroidekaDef, dt: f64, mv: f64, turn: f64, mouse_turn: f64) {
    let rate = def.ball_turn_speed.unwrap_or(2.0);
    d.aim_yaw -= turn * rate * dt + mouse_turn;
    let target_lean = deg_to_rad(def.ball_max_lean.unwrap_or(10.0)) * turn * (d.spin / d.max_spin).abs();
    d.lean += (target_lean - d.lean) * (1.0 - v8math::exp(-12.0 * dt));
    if mv.abs() > 1e-3 {
        d.spin = clamp(d.spin + def.ball_acceleration.unwrap_or(100.0) * mv * dt, -d.max_spin, d.max_spin);
    } else if d.grounded {
        d.spin -= v8math::sign(d.spin) * v8math::min(d.spin.abs(), def.ball_rolling_friction.unwrap_or(50.0) * dt);
    }
    if d.grounded {
        d.velocity[0] = v8math::sin(d.aim_yaw) * TRACTION * d.spin;
        d.velocity[2] = v8math::cos(d.aim_yaw) * TRACTION * d.spin;
    }
    d.roll += d.spin * dt;
}

/// collide(dt): deployed, its base stands on what faces up and slides off
/// what doesn't; rolled, its ball slides along walls and takes traction
/// where the ground is flat enough.
fn collide(id: u32, d: &mut Droideka, world: &mut dyn UnitsWorld) {
    let ball = d.mode == BALL;
    let r = if ball { d.ball_radius } else { BODY_RADIUS };
    let mut n = Vector3::default();
    let mut grounded = false;
    let mut polygons: Vec<GatherPolygon> = Vec::new();
    let mut position = v3(d.position);
    let mut velocity = v3(d.velocity);
    let mut avoidance = v3(d.avoidance);
    for _pass in 0..4 {
        let c = Vector3 { x: position.x, y: position.y + r, z: position.z };
        let mut best = f64::INFINITY;
        let mut moving = false;
        world.gather(id, &c, r, if ball { 1 } else { 0 }, &mut polygons);
        for polygon in &polygons {
            for tri in &polygon.triangles {
                let p = closest_point_to_point(arr(&tri[0]), arr(&tri[1]), arr(&tri[2]), arr(&c));
                let closest = v3(p);
                let distance = closest.distance_to(&c);
                if distance < r && distance < best && distance > 1e-5 {
                    best = distance;
                    moving = polygon.moving;
                    n.sub_vectors(&c, &closest).divide_scalar(distance);
                }
            }
        }
        let mut depth = if best < f64::INFINITY { r - best } else { 0.0 };
        let mut wall = depth > 0.0;
        // The terrain is a height field under the centre.
        if let Some(ground) = world.cast(&c, &DOWN, r + 0.05) {
            if ground.terrain && r - ground.distance > depth {
                depth = r - ground.distance;
                n.copy(&ground.normal);
                wall = false;
                moving = false;
            }
        }
        if depth <= 0.0 {
            break;
        }
        if wall && n.y.abs() < 0.3 {
            avoidance.add(&n);
        }
        if n.y > (if ball { d.max_ball_slope } else { GROUND }) {
            grounded = true;
            // Coming down on it too fast hurts (collision damage), only on
            // what it stands on.
            if !moving {
                let facing = n.x * v8math::sin(d.aim_yaw) + n.z * v8math::cos(d.aim_yaw);
                d.collision_damage += contact_damage(&d.collision_rule, velocity.dot(&n), n.y, facing);
            }
        }
        if ball {
            if n.y <= BALL_GROUND {
                position.add_scaled_vector(&n, depth);
            } else {
                position.y += depth / n.y;
            }
            let into = velocity.dot(&n);
            if into < 0.0 {
                velocity.add_scaled_vector(&n, -into);
            }
            if n.y > d.max_ball_slope {
                let right = Vector3 { x: v8math::cos(d.aim_yaw), y: 0.0, z: -v8math::sin(d.aim_yaw) };
                let mut forward = Vector3::default();
                forward.cross_vectors(&right, &n).normalize();
                velocity.copy(&forward).multiply_scalar(TRACTION * d.spin);
            }
        } else if n.y > GROUND {
            position.y += depth / n.y;
            if velocity.y < 0.0 {
                velocity.y = 0.0;
            }
        } else {
            let flat = v8math::hypot(n.x, n.z);
            if flat > 1e-4 {
                position.x += (n.x / flat) * depth;
                position.z += (n.z / flat) * depth;
            }
        }
    }
    d.position = arr(&position);
    d.velocity = arr(&velocity);
    d.avoidance = arr(&avoidance);
    d.grounded = grounded;
}

/// roomToStand(): headroom straight up, and on each of four axes at 1.5 m
/// up at least one side clear for 1.5 m.
fn room_to_stand(d: &Droideka, world: &mut dyn UnitsWorld) -> bool {
    let p = v3(d.position);
    let from = Vector3 { x: p.x, y: p.y + 1.5, z: p.z };
    if world.cast(&Vector3 { x: p.x, y: p.y + 0.7, z: p.z }, &UP, 1.1 * 2.2).is_some() {
        return false;
    }
    for [x, z] in [[1.0, 0.0], [0.0, 1.0], [0.707, 0.707], [0.707, -0.707]] {
        let plus = world.cast(&from, &Vector3 { x, y: 0.0, z }, HEADROOM).is_none();
        let minus = world.cast(&from, &Vector3 { x: -x, y: 0.0, z: -z }, HEADROOM).is_none();
        if !plus && !minus {
            return false;
        }
    }
    true
}

/// sampleMuzzles(dt): the hidden gun rig at its place, posed by its own
/// animator (the store's `muzzleAnimator`, which the rig's carries over
/// from one tick to the next).
pub fn sample_muzzles(unit: &mut Unit, dt: f64, parts: &mut DroidekaParts) {
    let Some(rig) = &mut parts.rig else { return };
    let d = body(unit);
    let Some(animator) = &mut d.muzzle_animator else { return };
    put_animator(animator, rig);
    let state = DroidekaState {
        mode: d.mode as i32,
        time: d.time,
        dead: d.dead,
        legs: d.legs,
        aim_pitch: d.aim_pitch,
        aim_yaw: d.aim_yaw,
        roll: d.roll,
        lean: d.lean,
        speed: d.speed,
        position: v3(d.position),
    };
    rig.sample_muzzles(dt, &state, &parts.clips.view());
    take_animator(animator, rig);
}

/// The store's animator into the rig's.
pub fn put_animator(a: &RigAnimator, rig: &mut DroidekaRig) {
    let r = &mut rig.animator;
    r.facing = a.facing;
    r.legs = a.legs;
    r.mode = a.mode as i32;
    r.went_up_rolled = a.went_up_rolled;
    r.roll_origin = a.roll_origin;
    r.transition = a.transition;
    for (i, pose) in a.from.iter().enumerate().take(r.from_q.len()) {
        r.from_q[i] = Quaternion { x: pose.q[0], y: pose.q[1], z: pose.q[2], w: pose.q[3] };
        r.from_p[i] = v3(pose.p);
    }
    r.pivot = v3(a.pivot);
    r.roll_center = v3(a.roll_center);
}

/// The rig's animator back into the store's.
pub fn take_animator(a: &mut RigAnimator, rig: &DroidekaRig) {
    let r = &rig.animator;
    a.facing = r.facing;
    a.legs = r.legs;
    a.mode = r.mode as f64;
    a.went_up_rolled = r.went_up_rolled;
    a.roll_origin = r.roll_origin;
    a.transition = r.transition;
    a.from = r.from_q.iter().zip(&r.from_p).map(|(q, p)| BonePose { q: [q.x, q.y, q.z, q.w], p: arr(p) }).collect();
    a.pivot = arr(&r.pivot);
    a.roll_center = arr(&r.roll_center);
}
