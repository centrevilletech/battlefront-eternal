//! Local floor support for Battlefront II infantry. Heights on the planning
//! plane locate corridors horizontally; collision supplies the walking levels.
use crate::{
    ai::nav::{
        executor::PathExecutor,
        math::Vec3,
        swbf2::{
            graph::Swbf2Hub,
            walk::{
                drive_limit, ProbeKind, Survey, DEEP_DROP, DEEP_DROP_LIMIT, DROP, FLOOR_LIMIT, MAX_DROP,
                PARAPET_DROP,
            },
        },
    },
    movement::{
        soldier::{Mover, GROUND_NORMAL, V3},
        MovementOwner,
    },
    world::owner::World,
};
use three::Vector3;
mod interior;
mod steering;
mod work;
use work::Work;

#[cfg(test)]
std::thread_local! {
    static TEST_CASTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_cast_count() -> usize {
    TEST_CASTS.with(std::cell::Cell::get)
}

fn cast(world: &mut World, p: Vec3, d: [f64; 3], far: f64) -> Option<crate::units::world::CastHit> {
    #[cfg(test)]
    TEST_CASTS.with(|c| c.set(c.get() + 1));
    let id = crate::units::f3::collision_world_in(&world.units);
    crate::units::native::cast_static_in(
        world,
        id,
        &Vector3::new(p.x, p.y, p.z),
        &Vector3::new(d[0], d[1], d[2]),
        far,
        true,
    )
}

pub(super) fn support(world: &mut World, p: Vec3, above: f64, below: f64) -> Option<Vec3> {
    let hit = cast(
        world,
        Vec3::new(p.x, p.y + above, p.z),
        [0., -1., 0.],
        above + below,
    )?;
    (hit.normal.y > GROUND_NORMAL).then_some(Vec3::new(p.x, hit.point.y, p.z))
}

pub(super) fn sees_approach(world: &mut World, at: Vec3, target: Vec3) -> bool {
    let start = Vec3::new(at.x, at.y + 1.4, at.z);
    let delta = [
        target.x - start.x,
        target.y + 0.85 - start.y,
        target.z - start.z,
    ];
    let length = v8math::hypot(v8math::hypot(delta[0], delta[2]), delta[1]);
    length <= 0.05 || cast(world, start, delta.map(|x| x / length), length - 0.05).is_none()
}

fn headroom(world: &mut World, p: Vec3) -> bool {
    cast(world, Vec3::new(p.x, p.y + 0.12, p.z), [0., 1., 0.], 1.6).is_none()
}

/// How far a floor line has come.
#[derive(Clone, Copy)]
struct Line {
    start: Vec3,
    end: Vec3,
    at: Vec3,
    step: usize,
}

/// Walkable floor under a straight line. It stops between two steps when
/// `work` is spent, with its place in `line`.
fn floor_line(
    world: &mut World,
    a: Vec3,
    b: Vec3,
    line: &mut Option<Line>,
    work: &mut Work,
) -> Option<bool> {
    if line.is_none() {
        if ![a.x, a.y, a.z, b.x, b.y, b.z]
            .into_iter()
            .all(f64::is_finite)
        {
            return Some(false);
        }
        // A narrow crease can place the centre ray on a steep face while the
        // soldier still has ground under its footprint.
        let mut ground = |dx, dz| {
            work.cast();
            support(world, Vec3::new(a.x + dx, a.y, a.z + dz), 0.32, 1.25)
        };
        let Some(start) = ground(0., 0.).or_else(|| {
            [(0.25, 0.), (-0.25, 0.), (0., 0.25), (0., -0.25)]
                .into_iter()
                .find_map(|(dx, dz)| ground(dx, dz))
        }) else {
            return Some(false);
        };
        if a == b {
            work.cast();
            return Some(headroom(world, start));
        }
        work.cast();
        let Some(end) = support(world, b, 0.32, 1.25) else {
            return Some(false);
        };
        let span = v8math::hypot(end.x - start.x, end.z - start.z);
        if span > 256. {
            return Some(false);
        }
        work.cast();
        if !headroom(world, start) {
            return Some(false);
        }
        work.cast();
        if !headroom(world, end) {
            return Some(false);
        }
        if span < 0.01 {
            return Some((start.y - end.y).abs() < 0.15);
        }
        *line = Some(Line {
            start,
            end,
            at: start,
            step: 1,
        });
    }
    let l = line.as_mut().unwrap();
    let (dx, dz) = (l.end.x - l.start.x, l.end.z - l.start.z);
    let span = v8math::hypot(dx, dz);
    let normal = [-dz / span, dx / span];
    let steps = (span / 0.7).ceil() as usize;
    while l.step <= steps {
        if work.spent() {
            return None;
        }
        let p = l.at;
        let t = l.step as f64 / steps as f64;
        let at = Vec3::new(l.start.x + t * dx, p.y, l.start.z + t * dz);
        work.cast();
        let Some(q) = support(world, at, 0.65, 0.75) else {
            return Some(false);
        };
        work.cast();
        if !headroom(world, q) {
            return Some(false);
        }
        let dist = v8math::sqrt(
            (q.x - p.x) * (q.x - p.x) + (q.y - p.y) * (q.y - p.y) + (q.z - p.z) * (q.z - p.z),
        );
        let dir = [(q.x - p.x) / dist, (q.y - p.y) / dist, (q.z - p.z) / dist];
        for side in [-0.42, 0., 0.42] {
            let origin = Vec3::new(p.x + side * normal[0], p.y + 0.85, p.z + side * normal[1]);
            work.cast();
            if cast(world, origin, dir, dist + 0.12).is_some() {
                return Some(false);
            }
        }
        l.at = q;
        l.step += 1;
    }
    Some((l.at.y - l.end.y).abs() <= 0.32)
}

/// A direct walking check outside a navigation order uses the combat allowance.
pub fn line_of_walk(world: &mut World, a: Vec3, b: Vec3) -> bool {
    if let Some(probe) = world.ai.ground_probe.as_mut() {
        let Some(left) = probe.direct_checks.checked_sub(1) else {
            return false;
        };
        probe.direct_checks = left;
    }
    try_crosses(world, a, b, 0, ProbeKind::Direct).unwrap_or(false)
}

/// Whether a soldier can walk, jump or drop from `a` to `b`. `None` when the
/// tick's work allowance for `kind` is spent before that is known.
pub(crate) fn try_crosses(
    world: &mut World,
    a: Vec3,
    b: Vec3,
    movement: u8,
    kind: ProbeKind,
) -> Option<bool> {
    if ![a.x, a.y, a.z, b.x, b.y, b.z]
        .into_iter()
        .all(f64::is_finite)
    {
        return Some(false);
    }
    let Some(mut probe) = world.ai.ground_probe.take() else {
        return Some(false);
    };
    let result = probe.check(world, a, b, movement, kind);
    world.ai.ground_probe = Some(probe);
    result
}

/// Only movement parameters that can affect a normal standing jump enter
/// the survey. Classes with the same parameters share a physical trial.
#[derive(Clone, Copy, PartialEq)]
struct JumpMotion {
    speed: f64,
    strafe: f64,
    turn: f64,
    pitch: f64,
    jump: f64,
    accel: f64,
    scale: [f64; 3],
    threshold: [f64; 3],
    pitch_limits: [f64; 2],
}

impl JumpMotion {
    fn from_body(s: &crate::gen::Soldier) -> Self {
        Self {
            speed: s.max_speed,
            strafe: s.max_strafe,
            turn: s.max_turn,
            pitch: s.max_pitch,
            jump: s.max_jump,
            accel: s.accel,
            scale: s.collision_rule.scale,
            threshold: s.collision_rule.threshold,
            pitch_limits: s.pitch_limits,
        }
    }

    fn apply(self, mover: &mut Mover) {
        mover.max_speed = self.speed;
        mover.max_strafe = self.strafe;
        mover.max_turn = self.turn;
        mover.max_pitch = self.pitch;
        mover.max_jump = self.jump;
        mover.accel = self.accel;
        mover.scale = self.scale;
        mover.threshold = self.threshold;
        mover.pitch_limits = self.pitch_limits;
    }
}

/// How far a driven body has come.
struct Drive {
    executor: PathExecutor,
    best: f64,
    idle: usize,
    last_ground: f64,
    tick: usize,
    jumped: bool,
}

/// A ground check that can stop between two steps and go on in a later tick.
struct Check {
    a: Vec3,
    b: Vec3,
    movement: u8,
    /// The tick in which it was asked or last gone on with.
    tick: u32,
    line: Option<Line>,
    drive: Option<Drive>,
}

impl Check {
    fn new(a: Vec3, b: Vec3, movement: u8, tick: u32) -> Self {
        Self {
            a,
            b,
            movement,
            tick,
            line: None,
            drive: None,
        }
    }
    fn asks(&self, a: Vec3, b: Vec3, movement: u8) -> bool {
        movement == self.movement && a == self.a && b == self.b
    }
}

/// The private body of one kind of check, and the check that ran out of work.
struct Slot {
    mover: u32,
    steering: steering::Steering,
    stopped: Option<Check>,
    /// The last stopped check that a later tick finished.
    finished: Option<(Check, bool)>,
    resumed: Option<u32>,
}

/// Private movers serve clearance checks during the battle and stay out of the live mover arena.
pub(super) struct GroundProbe {
    movement: MovementOwner,
    slots: [Slot; ProbeKind::SLOTS],
    epoch: (u64, u64),
    flags: Vec<u8>,
    enabled: usize,
    moves: usize,
    tick: Option<u32>,
    direct_checks: usize,
    work: usize,
}

impl GroundProbe {
    // Combat can ask about a walking line before it places a navigation order.
    const DIRECT_CHECKS: usize = 16;
    /// A stopped check, and its answer, lapse when nothing asks for them this long.
    const KEEP: u32 = 8;

    pub(super) fn begin_tick(&mut self, tick: u32) {
        if self.tick != Some(tick) {
            self.tick = Some(tick);
            self.direct_checks = Self::DIRECT_CHECKS;
            self.work = 0;
        }
    }

    pub(super) fn has_work(&self, kind: ProbeKind) -> bool {
        self.work < kind.allowance()
    }

    #[cfg(test)]
    pub(crate) fn work_used(&self) -> usize {
        self.work
    }

    fn new(world: &mut World) -> Self {
        let mut movement = MovementOwner::new();
        movement
            .flags_mut()
            .copy_from_slice(world.movement.flags_mut());
        let id = crate::units::f3::collision_world_in(&world.units);
        let slots = [(); ProbeKind::SLOTS].map(|_| {
            let mover = movement.new_mover(id, |_| {});
            movement.reserve_mover(mover, 256, 16, 512);
            Slot {
                mover,
                steering: steering::Steering::new(),
                stopped: None,
                finished: None,
                resumed: None,
            }
        });
        movement.prepare_world_gathers(&world.collision.state, id);
        Self {
            movement,
            slots,
            epoch: (0, 0),
            flags: Vec::with_capacity(crate::collision::MAX_BODIES),
            enabled: 0,
            moves: 0,
            tick: None,
            direct_checks: Self::DIRECT_CHECKS,
            work: 0,
        }
    }

    /// Follow the live collision bodies. Stopped checks belong to the bodies they met.
    fn observe(&mut self, world: &mut World) {
        let state = &world.collision.state;
        let id = crate::units::f3::collision_world_in(&world.units);
        let Some(scene) = state.worlds.get(id) else {
            return;
        };
        let bodies = scene.order.len().min(crate::collision::MAX_BODIES);
        let flags = &world.movement.flags_mut()[..bodies];
        let epoch = (scene.version, state.fixed_version);
        if self.epoch != epoch || self.flags != flags {
            self.epoch = epoch;
            self.flags.clear();
            self.flags.extend_from_slice(flags);
            self.enabled = scene
                .order
                .iter()
                .zip(flags)
                .filter(|(_, &on)| on != 0)
                .count();
            self.moves = scene
                .order
                .iter()
                .filter(|&&b| state.bodies[b as usize].moving)
                .count();
            self.movement.flags_mut()[..bodies].copy_from_slice(flags);
            for slot in &mut self.slots {
                slot.stopped = None;
                slot.finished = None;
            }
        }
    }

    fn check(
        &mut self,
        world: &mut World,
        a: Vec3,
        b: Vec3,
        movement: u8,
        kind: ProbeKind,
    ) -> Option<bool> {
        self.observe(world);
        let tick = self.tick.unwrap_or(0);
        if kind.waits() {
            self.resume(world, kind, tick);
            let slot = &self.slots[kind.slot()];
            if let Some((done, clear)) = &slot.finished {
                if done.asks(a, b, movement) && tick.abs_diff(done.tick) <= Self::KEEP {
                    return Some(*clear);
                }
            }
        }
        if !self.has_work(kind) {
            return None;
        }
        let mut check = Check::new(a, b, movement, tick);
        let result = self.go_on(world, &mut check, kind);
        if result.is_none() && kind.waits() {
            self.slots[kind.slot()].stopped = Some(check);
        }
        result
    }

    /// The check that stopped for work goes on before any other of its kind,
    /// whoever asks first in this tick.
    fn resume(&mut self, world: &mut World, kind: ProbeKind, tick: u32) {
        if !self.has_work(kind) {
            return;
        }
        let slot = &mut self.slots[kind.slot()];
        if slot.resumed == Some(tick) {
            return;
        }
        slot.resumed = Some(tick);
        let Some(mut check) = slot.stopped.take() else {
            return;
        };
        if tick.abs_diff(check.tick) > Self::KEEP {
            return;
        }
        let result = self.go_on(world, &mut check, kind);
        let slot = &mut self.slots[kind.slot()];
        check.tick = tick;
        match result {
            Some(clear) => slot.finished = Some((check, clear)),
            None => slot.stopped = Some(check),
        }
    }

    fn go_on(&mut self, world: &mut World, check: &mut Check, kind: ProbeKind) -> Option<bool> {
        let left = kind.allowance().saturating_sub(self.work);
        let mut work = Work::new(left, self.enabled, self.moves);
        let result = self.walk(world, check, kind.slot(), &mut work);
        self.work += work.used();
        result
    }

    /// The floor under the whole line, then the body driven along it.
    fn walk(
        &mut self,
        world: &mut World,
        check: &mut Check,
        slot: usize,
        work: &mut Work,
    ) -> Option<bool> {
        let (a, b, movement) = (check.a, check.b, check.movement);
        if check.drive.is_none() {
            if movement == 0 && !floor_line(world, a, b, &mut check.line, work)? {
                return Some(false);
            }
            if a.x == b.x && a.z == b.z {
                return Some(true);
            }
        }
        let (jump, parapet_drop, drop_limit) = if movement == DROP {
            (false, false, MAX_DROP)
        } else if movement == DEEP_DROP {
            (false, false, DEEP_DROP_LIMIT)
        } else if movement == PARAPET_DROP {
            (true, true, DEEP_DROP_LIMIT)
        } else {
            (movement != 0, false, 0.75)
        };
        self.drive_profile(
            world,
            slot,
            a,
            b,
            jump,
            parapet_drop,
            drop_limit,
            None,
            &mut check.drive,
            work,
        )
    }

    /// The survey drives a body without a limit on its work.
    fn drive(
        &mut self,
        world: &mut World,
        a: Vec3,
        b: Vec3,
        jump: bool,
        drop_limit: f64,
        profile: Option<JumpMotion>,
    ) -> bool {
        let work = &mut Work::unlimited();
        self.drive_profile(
            world,
            0,
            a,
            b,
            jump,
            false,
            drop_limit,
            profile,
            &mut None,
            work,
        ) == Some(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_profile(
        &mut self,
        world: &mut World,
        slot: usize,
        a: Vec3,
        b: Vec3,
        jump: bool,
        parapet_drop: bool,
        drop_limit: f64,
        profile: Option<JumpMotion>,
        drive: &mut Option<Drive>,
        work: &mut Work,
    ) -> Option<bool> {
        let span = v8math::hypot(b.x - a.x, b.z - a.z);
        if span > 256. {
            return Some(false);
        }
        let Self {
            movement, slots, ..
        } = self;
        let Slot {
            mover: id,
            steering,
            ..
        } = &mut slots[slot];
        let id = *id;
        if drive.is_none() {
            let mover = movement.mover(id);
            *mover = Mover::new(mover.world);
            mover.max_speed = 6.5;
            mover.max_strafe = 4.5;
            mover.max_jump = 7.5;
            mover.max_turn = 6.;
            mover.max_pitch = 4.;
            mover.accel = 70.;
            if let Some(profile) = profile {
                profile.apply(mover);
            }
            mover.position = V3::new(a.x, a.y, a.z);
            mover.settled = mover.position;
            mover.grounded = true;
            if parapet_drop {
                mover.state_time = 1.0;
            }
            // A waypoint can be entered while turning and decelerating.
            mover.velocity = if profile.is_some() {
                V3::ZERO
            } else {
                V3::new(0.55, 0., 0.)
            };
            *drive = Some(Drive {
                executor: steering.start(a, b, mover),
                best: f64::INFINITY,
                idle: 0,
                last_ground: a.y,
                tick: 0,
                jumped: false,
            });
        }
        let d = drive.as_mut().unwrap();
        while d.tick < drive_limit(span) {
            if work.spent() {
                return None;
            }
            let m = movement.mover(id);
            if !jump && d.last_ground - m.position.y > drop_limit {
                // A descending capsule can lose contact briefly on a supported slope.
                let feet = Vec3::new(m.position.x, m.position.y, m.position.z);
                work.cast();
                if support(world, feet, 0.15, 0.75).is_none() {
                    return Some(false);
                }
                d.last_ground = m.position.y;
            }
            if m.grounded {
                d.last_ground = m.position.y;
            }
            let dx = b.x - m.position.x;
            let dz = b.z - m.position.z;
            let left = v8math::hypot(dx, dz);
            if left < 0.32 && (m.position.y - b.y).abs() < 0.4 && m.grounded {
                return Some(true);
            }
            if left + 0.1 < d.best {
                d.best = left;
                d.idle = 0;
            } else {
                d.idle += 1;
            }
            if d.idle > 90 || m.position.y < v8math::min(a.y, b.y) - 1.5 {
                return Some(false);
            }
            let mut input = if parapet_drop {
                steering.tick_aim(&mut d.executor, m, Some(b), m.position.y <= b.y + 0.75)
            } else {
                steering.tick(&mut d.executor, m)
            };
            if parapet_drop {
                if !d.jumped && m.grounded && d.idle >= 4 && left > 0.25 {
                    let (fx, fz) = (v8math::sin(m.aim_yaw), v8math::cos(m.aim_yaw));
                    if fx * dx + fz * dz > 0.7 * left {
                        input.jump = true;
                        input.right = 0.0;
                        input.forward = v8math::max(input.forward, 0.75);
                        d.jumped = true;
                    }
                }
            } else {
                input.jump = jump && d.tick == usize::from(profile.is_none());
            }
            m.forget_climb();
            movement.advance(&mut world.collision.state, id, &input, 1. / 60.);
            let climbs = movement.mover(id).tried_climb();
            work.step(movement.gathered_triangles(), climbs);
            d.tick += 1;
        }
        Some(false)
    }
}

pub(super) struct GroundSurvey<'a> {
    world: &'a mut World,
    pub(super) probe: GroundProbe,
    bottom: f64,
    top: f64,
    sectors: Vec<interior::Sector>,
    doors: Vec<Vec3>,
    jump_motions: Option<Vec<JumpMotion>>,
}

impl<'a> GroundSurvey<'a> {
    pub fn new(world: &'a mut World, bounds: Option<&bfe_content::Value>) -> Self {
        let y = |key| bounds?.get(key)?.at(1)?.as_f64();
        let probe = GroundProbe::new(world);
        Self {
            world,
            probe,
            bottom: y("min").unwrap_or(-2048.) - 2.,
            top: y("max").unwrap_or(2048.) + 2.,
            sectors: Vec::new(),
            doors: Vec::new(),
            jump_motions: Some(Vec::new()),
        }
    }

    fn stand_clear(&mut self, at: Vec3) -> bool {
        if !headroom(self.world, at) {
            return false;
        }
        for [x, z] in [
            [1., 0.],
            [-1., 0.],
            [0., 1.],
            [0., -1.],
            [0.707, 0.707],
            [-0.707, 0.707],
            [0.707, -0.707],
            [-0.707, -0.707],
        ] {
            if cast(
                self.world,
                Vec3::new(at.x, at.y + 0.85, at.z),
                [x, 0., z],
                0.6,
            )
            .is_some()
            {
                return false;
            }
        }
        true
    }

    fn drive(&mut self, a: Vec3, b: Vec3, jump: bool) -> bool {
        self.probe.drive(self.world, a, b, jump, 0.75, None)
    }
}

/// Admissible indoor paths include the space an open door leaves. Restore the
/// live collision and keep the affected corridors pending until checked.
pub(super) fn survey_map(
    world: &mut World,
    bounds: Option<&bfe_content::Value>,
    sectors: Option<&bfe_content::Value>,
    doors: &[usize],
    classes: &[bfe_content::defs::ClassDef],
    graph: &crate::ai::nav::swbf2::Swbf2Graph,
    barriers: &mut crate::ai::nav::barriers::BarrierSet,
) -> Result<(crate::ai::nav::swbf2::walk::Floors, GroundProbe), String> {
    use crate::ai::nav::{
        barriers::{inside, BarrierSet},
        swbf2::walk::Floors,
    };
    let sectors = interior::Sector::read(sectors)?;
    let world_id = crate::units::f3::collision_world_in(&world.units);
    let door_places: Vec<(usize, usize, u8)> = {
        let order = world
            .collision
            .state
            .worlds
            .get(world_id)
            .map_or(&[][..], |scene| &scene.order);
        doors
            .iter()
            .filter_map(|&id| {
                let place = order.iter().position(|&b| b as usize == id)?;
                (place < crate::collision::MAX_BODIES).then_some((
                    id,
                    place,
                    world.movement.flags_mut()[place],
                ))
            })
            .collect()
    };
    let boxes = door_places
        .iter()
        .map(|&(id, _, _)| world.collision.state.bodies[id].bounds)
        .collect::<Vec<_>>();
    // Doorway barriers remain active in the live navigation.
    let mut opened = (!boxes.is_empty()).then(|| {
        let mut list = barriers.list.clone();
        for bar in &mut list {
            if boxes.iter().any(|bb| {
                inside(
                    bar,
                    (bb[0] + bb[3]) * 0.5,
                    (bb[1] + bb[4]) * 0.5,
                    (bb[2] + bb[5]) * 0.5,
                    0.5,
                )
            }) {
                bar.flags = 0;
            }
        }
        BarrierSet::with_cell(list, barriers.cell)
    });
    for &(_, place, _) in &door_places {
        world.movement.flags_mut()[place] = 0;
    }
    let mut survey = GroundSurvey::new(world, bounds);
    if graph.arcs.iter().any(|arc| arc.jump()) {
        let profiles = survey.jump_motions.as_mut().unwrap();
        for class in classes {
            if class.body.is_none() || class.walker_clips.is_some() {
                continue;
            }
            let body = crate::units::class::soldier_body(
                &class.record,
                0.,
                class.getup_time,
                class.dive_time,
            );
            let profile = JumpMotion::from_body(&body);
            if !profiles.contains(&profile) {
                if profiles.len() == 32 {
                    survey.jump_motions = None;
                    break;
                }
                profiles.push(profile);
            }
        }
    }
    survey.sectors = sectors;
    survey.doors = boxes
        .iter()
        .map(|bb| Vec3::new((bb[0] + bb[3]) * 0.5, bb[1], (bb[2] + bb[5]) * 0.5))
        .collect();
    let result = Floors::build(graph, opened.as_mut().unwrap_or(barriers), &mut survey);
    for &(_, place, on) in &door_places {
        survey.world.movement.flags_mut()[place] = on;
    }
    let mut floors = result?;
    if !boxes.is_empty() {
        let places = door_places
            .iter()
            .map(|&(_, place, _)| place)
            .collect::<Vec<_>>();
        floors.record_open_doors(&places, &boxes);
        floors.changed_bodies(&boxes);
        floors.restore_open_passages(survey.world.movement.flags_mut());
    }
    Ok((floors, survey.probe))
}

impl Survey for GroundSurvey<'_> {
    fn floors(&mut self, hub: &Swbf2Hub, out: &mut Vec<Vec3>) {
        // Offset samples recover a hub whose centre is just inside a doorframe.
        for [ox, oz] in [[0., 0.], [1., 0.], [-1., 0.], [0., 1.], [0., -1.]] {
            let offset = v8math::min(hub.radius, 2.) * 0.65;
            let (x, z) = (hub.x + offset * ox, hub.z + offset * oz);
            let mut y = self.top;
            for _ in 0..64 {
                if y <= self.bottom || out.len() == FLOOR_LIMIT {
                    break;
                }
                let Some(hit) = cast(
                    self.world,
                    Vec3::new(x, y, z),
                    [0., -1., 0.],
                    y - self.bottom,
                ) else {
                    break;
                };
                let floor = Vec3::new(x, hit.point.y, z);
                y = hit.point.y - 0.12;
                if hit.normal.y > GROUND_NORMAL
                    && out.iter().all(|p| (p.y - floor.y).abs() > 1.2)
                    && self.stand_clear(floor)
                {
                    out.push(floor);
                }
            }
        }
    }
    fn floor_sites(
        &mut self,
        hub: &Swbf2Hub,
        points: &mut Vec<Vec3>,
        barriers: &mut crate::ai::nav::barriers::BarrierSet,
    ) {
        if self.sectors.is_empty() {
            self.floors(hub, points);
            return;
        }
        // Cover the disc rather than assuming its centre can hold a soldier.
        let directions = [
            [1., 0.],
            [0., 1.],
            [-1., 0.],
            [0., -1.],
            [0.707, 0.707],
            [-0.707, 0.707],
            [-0.707, -0.707],
            [0.707, -0.707],
        ];
        for fraction in [0., 0.25, 0.5, 0.8] {
            for [dx, dz] in directions {
                let x = hub.x + dx * hub.radius * fraction;
                let z = hub.z + dz * hub.radius * fraction;
                let mut levels = [(0., 0.); 8];
                let mut n = 0;
                for sector in &self.sectors {
                    if let Some(range) = sector.range(x, z) {
                        if n < levels.len() {
                            levels[n] = range;
                            n += 1;
                        }
                    }
                }
                for &(low, high) in &levels[..n] {
                    let mut y = high;
                    for _ in 0..32 {
                        if y <= low || points.len() == FLOOR_LIMIT {
                            break;
                        }
                        let Some(hit) =
                            cast(self.world, Vec3::new(x, y, z), [0., -1., 0.], y - low)
                        else {
                            break;
                        };
                        y = hit.point.y - 0.12;
                        let p = Vec3::new(x, hit.point.y, z);
                        if hit.normal.y > GROUND_NORMAL
                            && points.iter().all(|q| (p.y - q.y).abs() > 1.2)
                            && barriers.point_clear(p, 0.55, 1).0
                            && self.stand_clear(p)
                        {
                            points.push(p);
                        }
                    }
                }
                if fraction == 0. {
                    break;
                }
            }
        }
    }
    fn doorway(&self, a: Vec3, b: Vec3) -> Option<Vec3> {
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let span = dx * dx + dz * dz;
        if span < 1. || self.sectors.is_empty() {
            return None;
        }
        self.doors
            .iter()
            .filter_map(|&p| {
                let t = ((p.x - a.x) * dx + (p.z - a.z) * dz) / span;
                if !(0.05..=0.95).contains(&t) || (p.y - a.y - t * (b.y - a.y)).abs() > 3.5 {
                    return None;
                }
                let x = p.x - a.x - t * dx;
                let z = p.z - a.z - t * dz;
                let d = x * x + z * z;
                (d < 100.).then_some((d, p))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p)
    }
    fn floor(&mut self, near: Vec3) -> Option<Vec3> {
        support(self.world, near, 1.4, 1.8).filter(|p| self.stand_clear(*p))
    }
    fn open_landing(&mut self, from: Vec3, to: Vec3) -> bool {
        let drop = from.y - to.y;
        drop < 2.
            || cast(
                self.world,
                Vec3::new(to.x, to.y + 0.2, to.z),
                [0., 1., 0.],
                drop + 1.5,
            )
            .is_none()
    }
    fn crosses(&mut self, a: Vec3, b: Vec3, flags: u8) -> bool {
        if flags == DROP {
            return self.probe.drive(self.world, a, b, false, MAX_DROP, None);
        }
        if flags & crate::ai::nav::swbf2::ARC_JUMP != 0 {
            return self.drive(a, b, true)
                && self.jump_motions.as_ref().is_some_and(|profiles| {
                    profiles
                        .iter()
                        .all(|&p| self.probe.drive(self.world, a, b, true, 0.75, Some(p)))
                });
        }
        let floor = floor_line(self.world, a, b, &mut None, &mut Work::unlimited()) == Some(true);
        (floor || flags != 0) && self.drive(a, b, flags != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::context::Context;
    use bfe_content::{
        bvh::{build, Positions},
        collision::{triangle_bounds, MapCollision},
    };

    fn stepped_floor() -> Context {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut strip = |left: f32, right: f32, low_z: f32, high_z: f32, y0: f32, y1: f32| {
            let base = (vertices.len() / 3) as u32;
            vertices.extend([
                left, y0, low_z, right, y1, low_z, right, y1, high_z, left, y0, high_z,
            ]);
            triangles.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        };
        strip(-11., 13., -6., 4., 0., 0.);
        strip(-11., -1., -5., 3., 3., 3.);
        strip(-11., 13., 7., 15., 3., 0.);
        strip(-5., 6., 18., 23., 0., 0.);
        strip(-0.05, 0.05, 19., 22., 0., 0.26);
        let polygons = (0..triangles.len() as u32 / 3).collect::<Vec<_>>();
        let scene = MapCollision {
            tri_bounds: triangle_bounds(&vertices, &triangles),
            positions: vertices.into(),
            indices: triangles.into(),
            polygons: polygons.into(),
            masks: Vec::new().into(),
            grid: None,
            terrain_positions: Vec::new().into(),
            terrain_indices: Vec::new().into(),
            has_terrain: false,
            kill_y: -40.,
        };
        let ctx = Context::new();
        let tree = build(Positions::F32(&scene.positions), &scene.indices);
        let installed = ctx.collision_install(&scene, &tree, None).unwrap();
        ctx.world(|world| {
            crate::units::f3::f3_init_in(world, installed.world, -40., 0).unwrap();
        })
        .unwrap();
        ctx
    }

    fn crosses(world: &mut World, a: Vec3, b: Vec3, movement: u8) -> bool {
        try_crosses(world, a, b, movement, ProbeKind::Leg) == Some(true)
    }

    #[test]
    fn a_soldier_can_walk_from_a_crease_under_its_feet() {
        stepped_floor().world(|world| {
            let feet = Vec3::new(0., 0.13, 20.);
            let flat = Vec3::new(3., 0., 20.);
            assert!(support(world, feet, 0.32, 1.25).is_none());
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            assert!(crosses(world, feet, flat, 0));
            assert!(crosses(world, feet, feet, 0));
            assert!(!crosses(world, Vec3::new(8., 0., 20.), flat, 0));
            assert!(!crosses(world, flat, feet, 0));
        }).unwrap();
    }

    #[test]
    fn ordinary_descent_needs_continuous_floor_under_the_mover() {
        stepped_floor().world(|world| {
            let mut survey = GroundSurvey::new(world, None);
            let shelf = Vec3::new(-6., 3., 0.);
            let landing = Vec3::new(7., 0., 0.);
            assert!(!survey.drive(shelf, landing, false));
            assert!(survey.drive(shelf, landing, true));
            assert!(survey.drive(Vec3::new(-7., 2.5, 10.), Vec3::new(9., 0.5, 10.), false));
        })
        .unwrap();
    }

    #[test]
    fn direct_walks_from_combat_share_one_allowance_per_tick() {
        stepped_floor().world(|world| {
            let start = Vec3::new(-6., 3., 0.);
            let end = Vec3::new(-4., 3., 0.);
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            for _ in 0..GroundProbe::DIRECT_CHECKS {
                world.ai.ground_probe.as_mut().unwrap().begin_tick(10);
                assert!(line_of_walk(world, start, end));
            }
            assert!(!line_of_walk(world, start, end));
            // An order still gets its clearance.
            assert!(crate::ai::owned::live::line_of_walk(world, start, end));
            assert!(crosses(world, start, end, 0));
            let probe = world.ai.ground_probe.as_mut().unwrap();
            probe.begin_tick(11);
            probe.work = ProbeKind::Direct.allowance();
            assert!(!line_of_walk(world, start, end));
            world.ai.ground_probe.as_mut().unwrap().begin_tick(12);
            assert!(line_of_walk(world, start, end));
        })
        .unwrap();
    }

    fn multi_body_corridor() -> Context {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| {
            let base = (vertices.len() / 3) as u32;
            vertices.extend(a);
            vertices.extend(b);
            vertices.extend(c);
            vertices.extend(d);
            triangles.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        };
        // Flat 52 m deck (z = -6..6, y = 0) with a solid wall blocking only the z = 2..6 strip at x = 18.
        quad([-2., 0., -6.], [50., 0., -6.], [50., 0., 6.], [-2., 0., 6.]);
        quad([18., 0., 2.], [18., 0., 6.], [18., 3., 6.], [18., 3., 2.]);
        let polygons = (0..triangles.len() as u32 / 3).collect::<Vec<_>>();
        let deck = MapCollision {
            tri_bounds: triangle_bounds(&vertices, &triangles),
            positions: vertices.into(),
            indices: triangles.into(),
            polygons: polygons.into(),
            masks: Vec::new().into(),
            grid: None,
            terrain_positions: Vec::new().into(),
            terrain_indices: Vec::new().into(),
            has_terrain: false,
            kill_y: -40.,
        };
        // Overhead structural beam at y = 2.7 (clear of soldier headroom at 1.72 m, inside local body bounds).
        let beam_verts = vec![
            -2.0f32, 2.7, -5.0, 50.0, 2.7, -5.0, 50.0, 2.7, 5.0, -2.0, 2.7, 5.0,
        ];
        let beam_tris = vec![0u32, 2, 1, 0, 3, 2];
        let beam_scene = MapCollision {
            tri_bounds: triangle_bounds(&beam_verts, &beam_tris),
            positions: beam_verts.into(),
            indices: beam_tris.into(),
            polygons: vec![0, 1].into(),
            masks: Vec::new().into(),
            grid: None,
            terrain_positions: Vec::new().into(),
            terrain_indices: Vec::new().into(),
            has_terrain: false,
            kill_y: -40.,
        };
        let ctx = Context::new();
        let deck_tree = build(Positions::F32(&deck.positions), &deck.indices);
        let installed = ctx.collision_install(&deck, &deck_tree, None).unwrap();
        let beam_tree = build(Positions::F32(&beam_scene.positions), &beam_scene.indices);
        let beam = ctx
            .collision_install(&beam_scene, &beam_tree, None)
            .unwrap();
        ctx.world(|world| {
            crate::units::f3::f3_init_in(world, installed.world, -40., 0).unwrap();
            let beam_tree_id = world.collision.state.worlds[beam.world as usize].tree as u32;
            let count = 64u32;
            let mut slots = Vec::with_capacity(count as usize);
            for i in 0..count {
                let slot = world.collision.body_new(beam_tree_id, 0) as u32;
                let y = if i < 8 { 2.7 } else { 20.0 + f64::from(i) };
                world.collision.io_mut()[20..26].copy_from_slice(&[-2., y, -5., 50., y + 0.2, 5.]);
                world.collision.body_set(slot);
                world.collision.enable_body(slot, true);
                slots.push(slot);
            }
            world
                .collision
                .world_order_slots(installed.world as u32, count)
                .copy_from_slice(&slots);
            for place in 0..count as usize {
                world.movement.flags_mut()[place] = 1;
            }
        })
        .unwrap();
        ctx
    }

    fn probe(world: &mut World) -> &mut GroundProbe {
        world.ai.ground_probe.as_mut().unwrap()
    }

    /// A tick in which other checks have left this much work for plans.
    fn begin(world: &mut World, tick: u32, left: usize) {
        let probe = probe(world);
        probe.begin_tick(tick);
        probe.work = ProbeKind::Plan.allowance() - left;
    }

    #[test]
    fn a_check_that_runs_out_of_work_goes_on_first_in_the_next_tick() {
        multi_body_corridor().world(|world| {
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            let (a, b) = (Vec3::new(0., 0., 0.), Vec3::new(42., 0., 0.));
            let (near, beside) = (Vec3::new(1.5, 0., 0.), Vec3::new(0., 0., -1.5));
            begin(world, 1, 400);
            assert_eq!(try_crosses(world, a, b, 0, ProbeKind::Plan), None);
            assert!(!probe(world).has_work(ProbeKind::Plan));
            // Its kind has no work left in this tick. A step and a leg still do.
            assert_eq!(try_crosses(world, a, near, 0, ProbeKind::Plan), None);
            assert_eq!(try_crosses(world, a, near, 0, ProbeKind::Step), Some(true));
            assert_eq!(try_crosses(world, a, near, 0, ProbeKind::Leg), Some(true));
            // Whoever asks first in the next tick, the stopped check goes on.
            let mut tick = 1;
            while probe(world).slots[0].finished.is_none() {
                tick += 1;
                assert!(tick < 12);
                begin(world, tick, 400);
                let _ = try_crosses(world, a, beside, 0, ProbeKind::Plan);
            }
            assert!(tick > 3);
            let spent = probe(world).work;
            assert_eq!(try_crosses(world, a, b, 0, ProbeKind::Plan), Some(true));
            assert_eq!(probe(world).work, spent, "a finished answer costs nothing");

            // Stopping and going on leaves the body where one run does.
            let end = |probe: &mut GroundProbe, slot: usize| {
                let m = probe.movement.mover(probe.slots[slot].mover);
                [m.position.x, m.position.y, m.position.z].map(f64::to_bits)
            };
            let mut whole = GroundProbe::new(world);
            whole.observe(world);
            let mut check = Check::new(a, b, 0, 0);
            let work = &mut Work::unlimited();
            assert_eq!(whole.walk(world, &mut check, 3, work), Some(true));
            let mut split = GroundProbe::new(world);
            split.observe(world);
            let mut check = Check::new(a, b, 0, 0);
            let mut parts = 0;
            while split
                .walk(world, &mut check, 3, &mut Work::new(400, 64, 0))
                .is_none()
            {
                parts += 1;
            }
            assert!(parts > 5);
            assert_eq!(end(&mut split, 3), end(&mut whole, 3));
        })
        .unwrap();
    }

    #[test]
    fn a_changed_body_drops_a_stopped_check_and_its_answer() {
        multi_body_corridor().world(|world| {
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            let (a, b) = (Vec3::new(0., 0., 0.), Vec3::new(42., 0., 0.));
            let near = Vec3::new(1.5, 0., 0.);
            let mut tick = 0;
            let mut finish = |world: &mut World, stopped_once: bool| {
                tick += 1;
                begin(world, tick, 400);
                assert_eq!(try_crosses(world, a, b, 0, ProbeKind::Plan), None);
                assert!(probe(world).slots[0].stopped.is_some());
                if stopped_once {
                    return;
                }
                loop {
                    tick += 1;
                    begin(world, tick, 400);
                    if try_crosses(world, a, b, 0, ProbeKind::Plan).is_some() {
                        break;
                    }
                }
                assert!(probe(world).slots[0].finished.is_some());
            };
            for stopped_once in [true, false] {
                finish(world, stopped_once);
                let flag = &mut world.movement.flags_mut()[0];
                *flag = 1 - *flag;
                assert_eq!(try_crosses(world, a, near, 0, ProbeKind::Leg), Some(true));
                let slot = &probe(world).slots[0];
                assert!(slot.stopped.is_none() && slot.finished.is_none());
            }
            // Nothing asks for a while. The bodies that move on their own may
            // stand elsewhere by then.
            finish(world, true);
            probe(world).begin_tick(100);
            assert_eq!(try_crosses(world, a, near, 0, ProbeKind::Plan), Some(true));
            assert!(probe(world).slots[0].stopped.is_none());
        })
        .unwrap();
    }

    #[test]
    fn a_starved_kind_does_not_refresh_or_charge_its_stopped_check() {
        multi_body_corridor().world(|world| {
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            let (a, b) = (Vec3::new(0., 0., 0.), Vec3::new(42., 0., 0.));
            let near = Vec3::new(1.5, 0., 0.);
            begin(world, 1, 400);
            assert_eq!(try_crosses(world, a, b, 0, ProbeKind::Plan), None);
            assert_eq!(probe(world).slots[0].stopped.as_ref().unwrap().tick, 1);
            // Exhaust work past Plan's allowance across more than KEEP ticks.
            for tick in 2..=10 {
                probe(world).begin_tick(tick);
                probe(world).work = ProbeKind::Plan.allowance();
                assert_eq!(try_crosses(world, a, near, 0, ProbeKind::Plan), None);
                assert_eq!(probe(world).work, ProbeKind::Plan.allowance());
                assert_eq!(probe(world).slots[0].stopped.as_ref().unwrap().tick, 1);
            }
            // Once work returns after KEEP ticks without progress, the stale check lapses.
            probe(world).begin_tick(11);
            assert_eq!(try_crosses(world, a, near, 0, ProbeKind::Plan), Some(true));
            assert!(probe(world).slots[0].stopped.is_none());
            assert!(probe(world).slots[0].finished.is_none());
        })
        .unwrap();
    }

    #[test]
    fn all_kinds_of_checks_share_the_work_of_a_tick() {
        multi_body_corridor().world(|world| {
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            probe(world).begin_tick(1);
            let kinds = [
                ProbeKind::Plan,
                ProbeKind::Step,
                ProbeKind::Question,
                ProbeKind::Direct,
                ProbeKind::Leg,
                ProbeKind::Turn,
            ];
            let mut last = 0;
            for kind in kinds {
                for n in 0..40 {
                    // Half of them press against the wall in the deck.
                    let z = if n % 2 == 0 { 4. } else { -4. + 0.1 * n as f64 };
                    let (a, b) = (Vec3::new(0., 0., z), Vec3::new(42., 0., z));
                    let _ = try_crosses(world, a, b, 0, kind);
                }
                let work = probe(world).work;
                assert!(work >= kind.allowance() && work < kind.allowance() + 200, "{kind:?} {work}");
                assert!(work >= last);
                last = work;
            }
        })
        .unwrap();
    }

    #[test]
    fn ground_probe_tracks_appended_moving_bodies_in_scene_order() {
        multi_body_corridor().world(|world| {
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            let world_id = crate::units::f3::collision_world_in(&world.units);
            let beam_tree = world.collision.state.bodies[0].tree as u32;
            let (a, b) = (Vec3::new(0., 0., 0.), Vec3::new(1.5, 0., 0.));
            probe(world).begin_tick(1);
            assert_eq!(try_crosses(world, a, b, 0, ProbeKind::Leg), Some(true));
            assert_eq!(probe(world).moves, 0);
            assert_eq!(probe(world).flags.len(), 64);

            // Create an inactive prepared moving body not yet in scene order, then
            // append a second moving body to scene order after admission.
            let prepared = world.collision.body_new(beam_tree, 1) as u32;
            let appended = world.collision.body_new(beam_tree, 1) as u32;
            assert_ne!(prepared, appended);
            world.collision.io_mut()[20..26].copy_from_slice(&[10., 25., -2., 14., 27., 2.]);
            world.collision.body_set(appended);
            world.collision.enable_body(appended, true);
            {
                let scene = &mut world.collision.state.worlds[world_id];
                scene.order.push(appended);
                scene.version += 1;
            }
            world.movement.flags_mut()[64] = 1;

            probe(world).begin_tick(2);
            assert_eq!(try_crosses(world, a, b, 0, ProbeKind::Leg), Some(true));
            // Flags slice tracks scene.order (65), not state.bodies (66), and counts
            // the appended moving body.
            assert_eq!(probe(world).flags.len(), 65);
            assert_eq!(probe(world).moves, 1);
            assert_eq!(probe(world).enabled, 65);
        })
        .unwrap();
    }

    fn parapet_rooftop() -> Context {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| {
            let base = (vertices.len() / 3) as u32;
            vertices.extend(a);
            vertices.extend(b);
            vertices.extend(c);
            vertices.extend(d);
            triangles.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        };
        let mut add_box = |x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32| {
            // +Y top and -Y bottom
            quad([x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]);
            quad([x0, y0, z0], [x0, y0, z1], [x1, y0, z1], [x1, y0, z0]);
            // -X and +X faces
            quad([x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]);
            quad([x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]);
            // -Z and +Z faces
            quad([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]);
            quad([x0, y0, z1], [x0, y1, z1], [x1, y1, z1], [x1, y0, z1]);
        };
        // Open street at y = 0.
        add_box(-20., -1., -20., 25., 0., 20.);
        // Building 1: flat roof at y = 4.5 ringed by a 0.6 m parapet (y = 4.5..5.1).
        add_box(-4., 0., -4., 4., 4.5, 4.);
        add_box(3.75, 4.5, -4., 4.0, 5.1, 4.);
        add_box(-4.0, 4.5, -4., -3.75, 5.1, 4.);
        add_box(-4.0, 4.5, 3.75, 4.0, 5.1, 4.0);
        add_box(-4.0, 4.5, -4.0, 4.0, 5.1, -3.75);
        // Building 2: flat roof at y = 4.5 blocked on +X by a 2.2 m wall (y = 4.5..6.7).
        add_box(10., 0., -4., 18., 4.5, 4.);
        add_box(17.75, 4.5, -4., 18.0, 6.7, 4.);

        let polygons = (0..triangles.len() as u32 / 3).collect::<Vec<_>>();
        let scene = MapCollision {
            tri_bounds: triangle_bounds(&vertices, &triangles),
            positions: vertices.into(),
            indices: triangles.into(),
            polygons: polygons.into(),
            masks: Vec::new().into(),
            grid: None,
            terrain_positions: Vec::new().into(),
            terrain_indices: Vec::new().into(),
            has_terrain: false,
            kill_y: -40.,
        };
        let ctx = Context::new();
        let tree = build(Positions::F32(&scene.positions), &scene.indices);
        let installed = ctx.collision_install(&scene, &tree, None).unwrap();
        ctx.world(|world| {
            crate::units::f3::f3_init_in(world, installed.world, -40., 0).unwrap();
        })
        .unwrap();
        ctx
    }

    #[test]
    fn parapet_drop_vaults_low_rooftop_parapet_and_rejects_tall_wall() {
        use crate::ai::nav::swbf2::walk::PARAPET_DROP;
        parapet_rooftop().world(|world| {
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            let landing = Vec3::new(5.0, 0.0, 0.0);
            let near_parapet = Vec3::new(3.1, 4.5, 0.0);
            let mid_roof = Vec3::new(-1.5, 4.5, 0.0);
            let close_parapet = Vec3::new(3.1, 4.5, 0.0);
            let close_landing = Vec3::new(4.5, 0.0, 0.0);

            probe(world).begin_tick(1);
            assert!(!crosses(world, near_parapet, landing, DROP));
            assert!(!crosses(world, mid_roof, landing, DROP));

            probe(world).begin_tick(2);
            assert!(crosses(world, near_parapet, landing, PARAPET_DROP));
            probe(world).begin_tick(3);
            assert!(crosses(world, mid_roof, landing, PARAPET_DROP));
            probe(world).begin_tick(4);
            assert!(crosses(world, close_parapet, close_landing, PARAPET_DROP));

            // A 2.2 m wall on top of the roof cannot be vaulted.
            probe(world).begin_tick(5);
            assert!(!crosses(
                world,
                Vec3::new(15.0, 4.5, 0.0),
                Vec3::new(19.0, 0.0, 0.0),
                PARAPET_DROP
            ));
        })
        .unwrap();
    }

    #[test]
    fn a_straight_walk_steps_off_a_roof_deeper_than_a_planned_drop() {
        use crate::ai::nav::swbf2::walk::{drop_kind, DEEP_DROP};
        assert_eq!(drop_kind(0.5), None);
        assert_eq!(drop_kind(4.0), Some(DROP));
        assert_eq!(drop_kind(7.3), Some(DEEP_DROP));
        assert_eq!(drop_kind(9.0), None);
        let mut vertices: Vec<f32> = Vec::new();
        let mut triangles: Vec<u32> = Vec::new();
        let mut quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| {
            let base = (vertices.len() / 3) as u32;
            vertices.extend(a);
            vertices.extend(b);
            vertices.extend(c);
            vertices.extend(d);
            triangles.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        };
        // A street at y = 0 and a block whose flat roof stands 7.3 m above it.
        quad([-30.0, 0.0, -30.0], [30.0, 0.0, -30.0], [30.0, 0.0, 30.0], [-30.0, 0.0, 30.0]);
        let (x0, y0, z0, x1, y1, z1) = (-6.0, 0.0, -5.0, 3.0, 7.3, 5.0);
        quad([x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]);
        quad([x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]);
        quad([x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]);
        quad([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]);
        quad([x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]);
        let polygons = (0..triangles.len() as u32 / 3).collect::<Vec<_>>();
        let scene = MapCollision {
            tri_bounds: triangle_bounds(&vertices, &triangles),
            positions: vertices.into(),
            indices: triangles.into(),
            polygons: polygons.into(),
            masks: Vec::new().into(),
            grid: None,
            terrain_positions: Vec::new().into(),
            terrain_indices: Vec::new().into(),
            has_terrain: false,
            kill_y: -40.,
        };
        let ctx = Context::new();
        let tree = build(Positions::F32(&scene.positions), &scene.indices);
        let installed = ctx.collision_install(&scene, &tree, None).unwrap();
        ctx.world(|world| {
            crate::units::f3::f3_init_in(world, installed.world, -40., 0).unwrap();
            world.ai.ground_probe = Some(Box::new(GroundProbe::new(world)));
            let edge = Vec3::new(2.2, 7.3, 0.0);
            let street = Vec3::new(4.6, 0.0, 0.0);
            probe(world).begin_tick(1);
            assert!(!crosses(world, edge, street, DROP), "a planned drop stops at its limit");
            probe(world).begin_tick(2);
            assert!(crosses(world, edge, street, DEEP_DROP), "the deep drop reaches the street");
        })
        .unwrap();
    }
}
