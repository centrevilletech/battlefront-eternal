//! The droideka (cis_inf_droideka, ODF class walkerdroid)
//! as the 2004 game runs it: its modes (each a clip from its own bank,
//! played at AnimationSpeed), its walk by the clips' root travel, the ball
//! it rolls up into, its shield, and its collision against the buildings
//! and the terrain. Its hidden gun rig, posed every tick for its muzzles, is
//! `crate::rig::droideka`.
//!
//! The 2004 game moves a droideka twice a frame, in its update and again
//! after the collision pass, gravity each time: the ball covers twice its
//! stored velocity and a droideka falls at four times 9.8 m/s2, while the
//! stored velocity a fall leaves (what collision damage reads) is that of a
//! fall at 9.8. Here the move after the pass is made at the start of the
//! next tick, in the same order, so that a tick ends where its pass left the
//! droideka and the next pass sweeps both moves. Deployed, it meets
//! buildings and props with two spheres: one of 1.4 m resting on the
//! ground 0.44 m ahead of its feet along its legs, which rides it up step
//! edges lower than 0.42 m, then the 0.65 m ball at its feet; the terrain
//! under its feet. Rolled, the ball alone, which bounces off what it rolls
//! into and rolls along anything flatter than MaxBallAngle.

use super::damage::contact_damage;
use super::shapes::{arr, v3};
use super::unit::or_zero;
use super::world::{UnitsWorld};
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
/// m: the deployed droideka's sphere against buildings and props
/// (p_building), resting on the ground with its centre this far ahead of
/// the feet.
const BUILDING_RADIUS: f64 = 1.4;
const BUILDING_AHEAD: f64 = 0.4363;
/// normal.y above which a deployed contact is ground.
const GROUND: f64 = 0.7;
/// normal.y above which the ball is lifted straight up out of a contact
/// rather than pushed along its normal.
const BALL_GROUND: f64 = 0.6;
/// m/s of the ball's velocity per unit of spin along what it rolls on:
/// BallRadius over BallSpinScale (0.65 / 2). Moved twice a tick, a spin of
/// 20 rolls it at 13 m/s.
const TRACTION: f64 = 0.325;
/// The contact normals a collision pass keeps (the 2004 game keeps ten).
const KEPT: usize = 10;
/// The most polygons one sphere meets in a step of a pass.
const CONTACTS: usize = 16;
/// m: a shallower overlap is rounding after a push, not a contact.
const TOUCH: f64 = 1e-6;
/// m: a push up out of a ceiling stops this far short (the ball's).
const CEILING_GAP: f64 = 0.02;
/// m: a push out of what is above it is not made when it would be longer
/// (the 2004 game crushes the droideka there instead).
const CRUSH: f64 = 5.0;
/// m: a longer move since the tick began skips collision (a teleport).
const TELEPORT: f64 = 100.0;
/// m of clearance the unfold test wants on an axis.
const HEADROOM: f64 = 1.5;
const UP: Vector3 = Vector3 { x: 0.0, y: 1.0, z: 0.0 };

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

/// A droideka's clips by mode (CLIPS: `clips.get(name)`).
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
/// As in the 2004 game the clip's frames are spread over the whole mode, so
/// a step moves the droideka on every tick of it, the last one included.
fn travel(d: &Droideka, clip: &Clip, t: f64) -> f64 {
    if d.root_index < 0.0 {
        return 0.0;
    }
    let Some(k) = clip.tracks.iter().position(|&b| b as f64 == d.root_index) else { return 0.0 };
    let frames = clip.frames as f64;
    let length = frames / 30.0 / d.speed;
    let f = v8math::min(v8math::max(t / length * (frames - 1.0), 0.0), frames - 1.0);
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

/// tick(input, dt), on the unit's `input`.
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
    // The last tick's move after its collision pass, swept by this pass.
    integrate(d, dt);
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

    // The update's move, then the collision pass over both moves.
    let d = body(unit);
    integrate(d, dt);
    collide(id, d, &def, dt, world);
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

/// One of the two moves of a tick: by the velocity (deployed, only its
/// vertical part: the clips walk it), then gravity.
fn integrate(d: &mut Droideka, dt: f64) {
    if d.mode == BALL {
        let mut p = v3(d.position);
        p.add_scaled_vector(&v3(d.velocity), dt);
        d.position = arr(&p);
    } else {
        d.position[1] += d.velocity[1] * dt;
    }
    d.velocity[1] -= GRAVITY * dt;
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

/// ballMove: the ball's spin, turn and lean into a turn. The spin builds
/// at BallAcceleration times the throttle squared, signed as the throttle,
/// and each contact of the collision pass takes BallRollingFriction off it;
/// what it rolls on sets its velocity there.
fn ball_move(d: &mut Droideka, def: &DroidekaDef, dt: f64, mv: f64, turn: f64, mouse_turn: f64) {
    let rate = def.ball_turn_speed.unwrap_or(2.0);
    d.aim_yaw -= turn * rate * dt + mouse_turn;
    let target_lean = deg_to_rad(def.ball_max_lean.unwrap_or(10.0)) * turn * (d.spin / d.max_spin).abs();
    d.lean += (target_lean - d.lean) * (1.0 - v8math::exp(-12.0 * dt));
    let acceleration = def.ball_acceleration.unwrap_or(100.0);
    let push = if mv < 0.0 { -acceleration } else { acceleration } * mv * mv;
    d.spin = clamp(d.spin + push * dt, -d.max_spin, d.max_spin);
    d.roll += d.spin * dt;
}

/// collide(dt): the collision pass over the move since the last pass (the
/// tick's start), in steps of at most half BallRadius. At each step the
/// terrain under its feet, then the polygons of buildings, props and hulls,
/// nearest contact first and each handled at once: deployed with
/// p_building and then the ball, rolled with the ball alone. After a step
/// with contacts the rest of the move loses its part into each contact
/// normal of the pass. A move too long to be a move (a teleport) is not
/// swept and meets nothing.
fn collide(id: u32, d: &mut Droideka, def: &DroidekaDef, dt: f64, world: &mut dyn UnitsWorld) {
    let ball = d.mode == BALL;
    let r = d.ball_radius;
    let (sin, cos) = (v8math::sin(d.aim_yaw), v8math::cos(d.aim_yaw));
    // p_building hangs on DummyRoot, which stays with the planted legs while
    // the body turns over them.
    let (legs_sin, legs_cos) = (v8math::sin(d.aim_yaw + d.legs), v8math::cos(d.aim_yaw + d.legs));
    let mut pass = Pass {
        ball,
        position: v3(d.previous),
        velocity: v3(d.velocity),
        avoidance: v3(d.avoidance),
        spin: d.spin,
        grounded: false,
        damage: 0.0,
        ground: UP,
        kept: [Vector3::default(); KEPT],
        count: 0,
        forward: Vector3 { x: sin, y: 0.0, z: cos },
        right: Vector3 { x: cos, y: 0.0, z: -sin },
        friction: def.ball_rolling_friction.unwrap_or(50.0) * dt,
        slope: if ball { d.max_ball_slope } else { GROUND },
    };
    let mut step = v3(d.position);
    step.sub(&pass.position);
    let length = step.length();
    if length > TELEPORT {
        d.grounded = false;
        return;
    }
    let steps = v8math::trunc(1.0 + length / (0.5 * r));
    step.divide_scalar(steps);
    for _ in 0..steps as usize {
        pass.position.add(&step);
        let mut hit = pass.terrain(world, r, d);
        if !ball {
            let ahead = Vector3 { x: legs_sin * BUILDING_AHEAD, y: BUILDING_RADIUS, z: legs_cos * BUILDING_AHEAD };
            hit |= pass.sphere(id, world, ahead, BUILDING_RADIUS, 0, d);
        }
        hit |= pass.sphere(id, world, Vector3 { x: 0.0, y: r, z: 0.0 }, r, 1, d);
        if hit {
            for q in &pass.kept[..pass.count] {
                let into = step.dot(q);
                if into < 0.0 {
                    step.add_scaled_vector(q, -into);
                }
            }
        }
    }
    d.position = arr(&pass.position);
    d.velocity = arr(&pass.velocity);
    d.avoidance = arr(&pass.avoidance);
    d.spin = pass.spin;
    d.grounded = pass.grounded;
    d.collision_damage += pass.damage;
}

/// A collision pass's working state.
struct Pass {
    ball: bool,
    position: Vector3,
    velocity: Vector3,
    avoidance: Vector3,
    spin: f64,
    grounded: bool,
    damage: f64,
    /// The last surface the ball rolled on in this pass (up before one).
    ground: Vector3,
    /// The pass's contact normals, as the 2004 game keeps them (ten at most).
    kept: [Vector3; KEPT],
    count: usize,
    forward: Vector3,
    right: Vector3,
    /// Spin a contact takes off the ball.
    friction: f64,
    /// normal.y above which a contact is ground.
    slope: f64,
}

impl Pass {
    /// The terrain under the feet: a contact while its height there lies
    /// between the bottom and the top of the ball, its depth that height
    /// difference times the ground's normal.y.
    fn terrain(&mut self, world: &mut dyn UnitsWorld, r: f64, d: &Droideka) -> bool {
        let p = self.position;
        let Some((height, normal)) = world.ground(p.x, p.z, p.y + 2.0 * r) else { return false };
        let depth = height - p.y;
        if !(0.0..=2.0 * r).contains(&depth) {
            return false;
        }
        self.contact(normal, depth * normal.y, false, false, d);
        true
    }

    /// One sphere (its centre `at` from the feet, radius r) against the
    /// polygons it overlaps, the nearest first, each handled at once and
    /// the search made again from where that left it; each polygon once.
    fn sphere(&mut self, id: u32, world: &mut dyn UnitsWorld, at: Vector3, r: f64, cache: usize, d: &Droideka) -> bool {
        let mut done = [0.0; CONTACTS];
        let mut count = 0;
        while count < CONTACTS {
            let mut c = self.position;
            c.add(&at);
            let mut best = r - TOUCH;
            let mut n = Vector3::default();
            let mut moving = false;
            let mut polygon = None;
            world.visit_gathered(id, &c, r, cache, &mut |is_moving, which, tri| {
                if done[..count].contains(&which) {
                    return;
                }
                let closest = v3(closest_point_to_point(arr(&tri[0]), arr(&tri[1]), arr(&tri[2]), arr(&c)));
                let distance = closest.distance_to(&c);
                if distance < best && distance > 1e-5 {
                    best = distance;
                    moving = is_moving;
                    polygon = Some(which);
                    n.sub_vectors(&c, &closest).divide_scalar(distance);
                }
            });
            let Some(which) = polygon else { break };
            done[count] = which;
            count += 1;
            self.contact(n, r - best, true, moving, d);
        }
        count > 0
    }

    fn keep(&mut self, n: Vector3) {
        if self.count < KEPT {
            self.kept[self.count] = n;
            self.count += 1;
        }
    }

    /// n with its part into each contact normal of the pass taken out, and
    /// the depth grown to match; then the push along it (none past CRUSH).
    fn clear_of_kept(&self, n: Vector3, depth: f64) -> Vector3 {
        let (mut m, mut depth) = (n, depth);
        if depth > CRUSH {
            return Vector3::default();
        }
        for q in &self.kept[..self.count] {
            let into = m.dot(q);
            if into < 0.0 {
                m.add_scaled_vector(q, -into);
                let length = m.length();
                if length > 0.0 {
                    m.divide_scalar(length);
                    depth /= length;
                }
            }
        }
        if depth > CRUSH {
            return Vector3::default();
        }
        m.multiply_scalar(depth);
        m
    }

    /// One contact: its normal n (toward the droideka) and depth.
    fn contact(&mut self, n: Vector3, depth: f64, wall: bool, moving: bool, d: &Droideka) {
        if wall && n.y.abs() < 0.3 {
            self.avoidance.add(&n);
        }
        // Coming into it too fast hurts (collision damage).
        if !moving {
            let facing = n.x * self.forward.x + n.z * self.forward.z;
            self.damage += contact_damage(&d.collision_rule, self.velocity.dot(&n), n.y, facing);
        }
        if n.y > self.slope {
            self.grounded = true;
        }
        if !self.ball {
            if n.y > GROUND {
                // Ground: lifted onto it.
                self.velocity.y = 0.0;
                self.position.y += depth / n.y;
                self.keep(n);
            } else if n.y > 0.0 {
                // A wall: pushed back level.
                let flat = v8math::hypot(n.x, n.z);
                let side = Vector3 { x: n.x / flat, y: 0.0, z: n.z / flat };
                self.position.add_scaled_vector(&side, depth / flat);
                self.keep(side);
            } else {
                // Above it: pushed along the normal, clear of the ground.
                let push = self.clear_of_kept(n, depth);
                self.position.add(&push);
                self.keep(n);
            }
            return;
        }
        if n.y > BALL_GROUND {
            self.position.y += depth / n.y;
        } else {
            self.position.add_scaled_vector(&n, depth);
        }
        // A full bounce off what it moves into.
        let into = self.velocity.dot(&n);
        if into < 0.0 {
            self.velocity.add_scaled_vector(&n, -2.0 * into);
        }
        if n.y > self.slope {
            // Rolling on it: along it as the spin turns the ball.
            self.velocity.cross_vectors(&self.right, &n).multiply_scalar(TRACTION * self.spin);
            self.ground = n;
        } else if n.y < 0.0 {
            // Under it: pushed along it, clear of the ground and of the
            // pass's other contacts, never quite back up.
            let mut m = n;
            m.add_scaled_vector(&self.ground, -self.ground.dot(&n));
            let mut depth = depth;
            let length = m.length();
            if length > 0.0 {
                m.divide_scalar(length);
                depth /= length;
            }
            let mut push = self.clear_of_kept(m, depth);
            if push.y > 0.0 {
                push.y = v8math::max(push.y - CEILING_GAP, 0.0);
            }
            self.position.add(&push);
            self.keep(m);
        } else {
            self.keep(n);
        }
        // Every contact costs the ball some spin.
        self.spin -= v8math::sign(self.spin) * v8math::min(self.spin.abs(), self.friction);
    }
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
    rig.pose_guns(&[[d.gun_yaw[0], d.gun_pitch[0]], [d.gun_yaw[1], d.gun_pitch[1]]]);
    take_animator(animator, rig);
}

/// The guns' aimers, `dt` on toward `target` (None: back to their line in
/// the clip, as when the droideka folds, rolls or unfolds), as the 2004
/// game's aimers turn them: the store's angles step, and the hidden rig's
/// blasters turn so the rounds fired this tick leave along them.
pub fn turn_guns(unit: &mut Unit, dt: f64, target: Option<&Vector3>, parts: &mut DroidekaParts) {
    let Some(rig) = &mut parts.rig else { return };
    let d = body(unit);
    let aimer = crate::rig::droideka_guns::Aimer {
        yaw_rate: d.gun_rates[0], pitch_rate: d.gun_rates[1], yaw: d.gun_yaw_limits, pitch: d.gun_pitch_limits,
    };
    let mut aims = [[d.gun_yaw[0], d.gun_pitch[0]], [d.gun_yaw[1], d.gun_pitch[1]]];
    rig.aim_guns(&mut aims, target, dt, &aimer);
    d.gun_yaw = [aims[0][0], aims[1][0]];
    d.gun_pitch = [aims[0][1], aims[1][1]];
}

/// Whether a droideka's guns follow its aim: not while it folds, rolls or
/// unfolds.
pub fn guns_ready(d: &crate::gen::Droideka) -> bool {
    d.mode < FOLD
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
    // DroidekaRig's saved transition pose has a fixed bone count. Retain
    // the imported animator's storage rather than replacing it each tick.
    a.from.resize_with(r.from_q.len(), BonePose::default);
    for i in 0..r.from_q.len() {
        let q = &r.from_q[i];
        a.from[i].q = [q.x, q.y, q.z, q.w];
        a.from[i].p = arr(&r.from_p[i]);
    }
    a.pivot = arr(&r.pivot);
    a.roll_center = arr(&r.roll_center);
}

#[cfg(test)]
mod tests {
    use super::*;

    // A walking step 17 frames long at AnimationSpeed 2 lasts 17 ticks and
    // moves the droideka the same distance on every one of them.
    #[test]
    fn a_step_moves_the_droideka_on_every_tick() {
        let frames = 17;
        let mut data = vec![0.0f32; frames * 7];
        for i in 0..frames {
            data[i * 7 + 3] = 1.0;
            data[i * 7 + 6] = 0.04 * i as f32;
        }
        let clip = Clip { frames, fps: 30.0, root_motion: true, tracks: vec![0], data };
        let d = Droideka { root_index: 0.0, speed: 2.0, ..Default::default() };
        let tick = 1.0 / 60.0;
        let steps: Vec<f64> = (0..frames).map(|k| travel(&d, &clip, (k + 1) as f64 * tick) - travel(&d, &clip, k as f64 * tick)).collect();
        let whole = travel(&d, &clip, frames as f64 * tick) - travel(&d, &clip, 0.0);
        assert!((whole - 0.04 * 16.0).abs() < 1e-6, "{whole}");
        assert!(steps.iter().all(|step| (step - whole / frames as f64).abs() < 1e-6), "{steps:?}");
    }
}
