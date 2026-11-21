//! A droideka against steps, walls, ceilings and terrain, with the real tick.
//! As in the 2004 game, deployed it rides up an edge lower than 0.42 m on
//! its 1.4 m building sphere and stops at a higher one; its ball, rolling at
//! full speed, takes a kerb but bounces back from an edge it meets steeper
//! than MaxBallAngle; it falls at four times 9.8 m/s2, and however fast it
//! falls its collision pass sweeps the whole move.
use bfe_sim::{
    gen::*,
    pose::Clip,
    units::{
        droideka::{self, DroidekaDef, DroidekaParts, ModeClips},
        shapes::{Bounds, HitShape},
        world::{CastHit, UnitsWorld},
    },
};
use std::rc::Rc;
use three::Vector3;

const DT: f64 = 1.0 / 60.0;

fn v(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// The course: quads (each one polygon) and terrain, a plane rising
/// `slope` metres per metre of z from `height` at z = 0, or none.
struct Course {
    quads: Vec<[Vector3; 4]>,
    terrain: Option<(f64, f64)>,
}

impl Course {
    fn new() -> Self {
        Course { quads: Vec::new(), terrain: None }
    }
    fn quad(mut self, q: [Vector3; 4]) -> Self {
        self.quads.push(q);
        self
    }
    /// Level ground (a building's floor) at height y.
    fn floor(self, y: f64) -> Self {
        self.quad([v(-40., y, -60.), v(-40., y, 70.), v(40., y, 70.), v(40., y, -60.)])
    }
    /// Level ground to z = 5, then a step of `height` across the whole way.
    fn step(height: f64) -> Self {
        Course::new()
            .quad([v(-40., 0., -60.), v(-40., 0., 5.), v(40., 0., 5.), v(40., 0., -60.)])
            .quad([v(-40., 0., 5.), v(-40., height, 5.), v(40., height, 5.), v(40., 0., 5.)])
            .quad([v(-40., height, 5.), v(-40., height, 70.), v(40., height, 70.), v(40., height, 5.)])
    }
    fn terrain(mut self, height: f64, slope: f64) -> Self {
        self.terrain = Some((height, slope));
        self
    }
}

impl UnitsWorld for Course {
    fn cast(&mut self, _: &Vector3, _: &Vector3, _: f64) -> Option<CastHit> {
        None
    }
    fn visit_gathered(&mut self, _: u32, _: &Vector3, _: f64, _: usize, visit: &mut dyn FnMut(bool, f64, &[Vector3; 3])) {
        for (i, q) in self.quads.iter().enumerate() {
            visit(false, i as f64, &[q[0], q[1], q[2]]);
            visit(false, i as f64, &[q[0], q[2], q[3]]);
        }
    }
    fn ground(&mut self, _: f64, z: f64, top: f64) -> Option<(f64, Vector3)> {
        let (height, slope) = self.terrain?;
        let y = height + slope * z;
        let l = (1.0 + slope * slope).sqrt();
        (y <= top).then(|| (y, v(0., 1.0 / l, -slope / l)))
    }
    fn seat_exposed(&mut self, _: u32, _: &Unit) -> bool { false }
    fn rider_shapes(&mut self, _: u32, _: &Unit, _: &mut [HitShape]) -> usize { 0 }
    fn held_shapes(&mut self, _: u32, _: &Unit, _: &mut [HitShape]) -> usize { 0 }
    fn target_alive(&mut self, _: u32) -> bool { false }
    fn target_dead(&mut self, _: u32) -> bool { false }
    fn target_sphere(&mut self, _: u32) -> Bounds { Bounds::default() }
    fn move_soldier(&mut self, _: u32, _: &mut Unit, _: f64) {}
    fn damage(&mut self, _: u32, _: &mut Unit, _: f64) {}
    fn aim(&mut self, _: u32, _: &mut Unit, _: f64) {}
    fn has_remote_step(&mut self, _: u32, _: &Unit) -> bool { false }
    fn remote_step(&mut self, _: u32, _: &mut Unit, _: f64, _: bool) {}
    fn combat_reset(&mut self, _: u32, _: &mut Unit) {}
    fn combat_update(&mut self, _: u32, _: &mut Unit, _: f64, _: f64) {}
    fn flush_muzzles(&mut self, _: u32, _: &mut Unit) {}
    fn clear_charge(&mut self, _: u32, _: &mut Unit) {}
    fn death_blast(&mut self, _: u32, _: &mut Unit) {}
    fn blow_apart(&mut self, _: u32, _: &mut Unit, _: f64) {}
    fn let_go_of_character(&mut self, _: u32, _: &mut Unit) {}
    fn release(&mut self, _: u32, _: &Unit) {}
}

/// Every mode a clip of 10 frames; the walking steps carry the root 0.5 m
/// forward, starting and stopping 0.25 m.
fn parts() -> DroidekaParts {
    let clip = |travel: f64| {
        let mut data = vec![0f32; 10 * 7];
        for f in 0..10 {
            data[f * 7 + 3] = 1.0;
            data[f * 7 + 6] = (travel * f as f64 / 9.0) as f32;
        }
        Some(Rc::new(Clip { frames: 10, fps: 30., root_motion: true, tracks: vec![0], data }))
    };
    let travel = [0., 0., 0., 0., 0., 0.25, 0., 0.5, 0.5, 0.25, 0.25, 0., 0., 0.];
    let by_mode = std::array::from_fn(|mode| if mode == 4 || mode == 6 { None } else { clip(travel[mode]) });
    DroidekaParts { def: DroidekaDef::default(), clips: Rc::new(ModeClips { by_mode }), rig: None }
}

fn droideka(p: [f64; 3], yaw: f64) -> Unit {
    let d = Droideka {
        max_turn: 1.25,
        speed: 1.0,
        turn_threshold: 0.5,
        ball_radius: 0.65,
        max_spin: 20.0,
        max_ball_slope: 35f64.to_radians().cos(),
        root_index: 0.0,
        collision_rule: CollisionRule { threshold: [7.5, 2.5, 0.0], scale: [1.0, 0.0, 0.0] },
        grounded: true,
        position: p,
        previous: p,
        settled: p,
        aim_yaw: yaw,
        ..Default::default()
    };
    Unit { alive: true, soldier: UnitBody::Droideka(d), ..Default::default() }
}

fn body(unit: &Unit) -> &Droideka {
    match &unit.soldier {
        UnitBody::Droideka(d) => d,
        UnitBody::Soldier(_) => unreachable!(),
    }
}

fn body_mut(unit: &mut Unit) -> &mut Droideka {
    match &mut unit.soldier {
        UnitBody::Droideka(d) => d,
        UnitBody::Soldier(_) => unreachable!(),
    }
}

/// A droideka at `p` facing `yaw`, rolled up first (in the open, then put
/// back at `p`) if `rolled`.
fn start(world: &mut Course, parts: &mut DroidekaParts, p: [f64; 3], yaw: f64, rolled: bool) -> Unit {
    let mut unit = droideka(p, yaw);
    if rolled {
        let mut input = MoveInput { r#use: true, ..Default::default() };
        for _ in 0..120 {
            droideka::tick_input(0, &mut unit, &input, DT, world, parts);
            input.r#use = false;
        }
        assert_eq!(body(&unit).mode, 12.0, "the droideka rolls up");
        let d = body_mut(&mut unit);
        d.position = p;
        d.previous = p;
        d.settled = p;
        d.velocity = [0.0; 3];
    }
    unit
}

/// Ticks with `input` for `seconds`, or until `stop` holds.
fn run(world: &mut Course, parts: &mut DroidekaParts, unit: &mut Unit, input: &MoveInput, seconds: f64, stop: &dyn Fn(&Droideka) -> bool) {
    for _ in 0..(seconds / DT) as usize {
        droideka::tick_input(0, unit, input, DT, world, parts);
        if stop(body(unit)) {
            return;
        }
    }
}

/// Drives a droideka (rolled up first if `rolled`) straight at the step for
/// `seconds`; whether it got 2 m onto the step.
fn over(height: f64, rolled: bool, z: f64, seconds: f64) -> bool {
    let mut world = Course::step(height);
    let mut parts = parts();
    let mut unit = start(&mut world, &mut parts, [0., 0., z], 0.0, rolled);
    let input = MoveInput { forward: 1.0, ..Default::default() };
    run(&mut world, &mut parts, &mut unit, &input, seconds, &|d| d.position[2] > 7.0);
    body(&unit).position[2] > 7.0
}

fn ahead() -> MoveInput {
    MoveInput { forward: 1.0, ..Default::default() }
}

#[test]
fn deployed_rides_up_an_edge_lower_than_its_building_sphere_allows() {
    assert!(over(0.40, false, 0.0, 8.0), "deployed over a 0.40 m step");
    assert!(!over(0.45, false, 0.0, 8.0), "deployed stopped by a 0.45 m step");
}

#[test]
fn rolled_takes_a_kerb_and_bounces_back_from_a_higher_edge() {
    assert!(over(0.15, true, -12.0, 4.0), "the ball over a 0.15 m kerb");
    assert!(!over(0.35, true, -12.0, 4.0), "the ball bounced back by a 0.35 m step");
}

#[test]
fn falls_at_four_times_gravity() {
    let mut world = Course::new().floor(0.0);
    let mut parts = parts();
    let mut unit = start(&mut world, &mut parts, [0., 10., 0.], 0.0, false);
    let mut ticks = 0;
    while body(&unit).position[1] > 0.01 {
        droideka::tick_input(0, &mut unit, &MoveInput::default(), DT, &mut world, &mut parts);
        ticks += 1;
        assert!(ticks < 120, "it lands");
    }
    // sqrt(2 x 10 / (4 x 9.8)) = 0.71 s; at 9.8 it would take 1.43 s.
    let seconds = ticks as f64 * DT;
    assert!((0.70..0.76).contains(&seconds), "a 10 m fall took {seconds} s");
}

#[test]
fn a_long_fall_lands_on_a_deck_and_on_the_terrain() {
    // At the end of a 1000 m fall a tick moves it some 4.7 m: the pass must
    // sweep all of it, or it ends a tick under a deck or more than its
    // ball's height under the ground and falls through. The drops spread
    // its last tick over that length.
    for k in 0..16 {
        let drop = 1000.0 + 0.3 * k as f64;
        for rolled in [false, true] {
            for terrain in [false, true] {
                let mut world = if terrain { Course::new().terrain(0.0, 0.0) } else { Course::new().floor(0.0) };
                let mut parts = parts();
                let mut unit = start(&mut world, &mut parts, [0., drop, 0.], 0.0, rolled);
                run(&mut world, &mut parts, &mut unit, &MoveInput::default(), 9.0, &|_| false);
                let y = body(&unit).position[1];
                assert!(y.abs() < 0.05, "{} droideka dropped {drop} m onto {} ends at y {y}",
                    if rolled { "rolled" } else { "deployed" }, if terrain { "the terrain" } else { "a deck" });
            }
        }
    }
}

#[test]
fn stands_on_level_terrain_and_slides_off_a_slope_too_steep() {
    let mut parts = parts();
    let mut world = Course::new().terrain(2.0, 0.0);
    let mut unit = start(&mut world, &mut parts, [0., 2.0, 0.], 0.0, false);
    run(&mut world, &mut parts, &mut unit, &MoveInput::default(), 1.0, &|_| false);
    let d = body(&unit);
    assert!((d.position[1] - 2.0).abs() < 1e-9 && d.grounded, "stands on the terrain's height: {:?}", d.position);

    // 50 degrees: the ground normal's y is 0.64, under the 0.7 a deployed
    // droideka stands on.
    let slope = 50f64.to_radians().tan();
    let mut world = Course::new().terrain(0.0, slope);
    let mut unit = start(&mut world, &mut parts, [0., 5.0 * slope, 5.0], 0.0, false);
    droideka::tick_input(0, &mut unit, &MoveInput::default(), DT, &mut world, &mut parts);
    assert!(!body(&unit).grounded, "not grounded on a 50 degree slope");
    run(&mut world, &mut parts, &mut unit, &MoveInput::default(), 1.0, &|_| false);
    assert!(body(&unit).position[1] < 5.0 * slope - 0.5, "slid down it: {:?}", body(&unit).position);
}

#[test]
fn the_ball_rolls_up_terrain_within_max_ball_angle() {
    let mut parts = parts();
    let slope = 30f64.to_radians().tan();
    let mut world = Course::new().terrain(0.0, slope);
    let mut unit = start(&mut world, &mut parts, [0., 0., 0.], 0.0, true);
    run(&mut world, &mut parts, &mut unit, &ahead(), 2.0, &|_| false);
    let d = body(&unit);
    assert!(d.position[1] > 3.0 && d.grounded, "rolled up the 30 degree slope: {:?}", d.position);
}

#[test]
fn walls_corners_and_a_low_flat_ceiling() {
    let mut parts = parts();
    // A wall across the way: the 1.4 m sphere, centred 0.44 m ahead of the
    // feet, stops them 1.84 m short of it.
    let mut world = Course::new().floor(0.0).quad([v(-40., 0., 5.), v(-40., 4., 5.), v(40., 4., 5.), v(40., 0., 5.)]);
    let mut unit = start(&mut world, &mut parts, [0., 0., 0.], 0.0, false);
    run(&mut world, &mut parts, &mut unit, &ahead(), 6.0, &|_| false);
    let z = body(&unit).position[2];
    assert!((z - (5.0 - 1.4 - 0.4363)).abs() < 0.05, "stopped by the wall at z {z}");

    // Into a corner on the diagonal: through neither wall.
    let mut world = Course::new()
        .floor(0.0)
        .quad([v(-40., 0., 5.), v(-40., 4., 5.), v(40., 4., 5.), v(40., 0., 5.)])
        .quad([v(5., 0., -40.), v(5., 0., 40.), v(5., 4., 40.), v(5., 4., -40.)]);
    let mut unit = start(&mut world, &mut parts, [0., 0., 0.], std::f64::consts::FRAC_PI_4, false);
    run(&mut world, &mut parts, &mut unit, &ahead(), 6.0, &|_| false);
    let p = body(&unit).position;
    assert!(p[0] < 5.0 - 0.9 && p[2] < 5.0 - 0.9 && p[1].abs() < 0.01, "held off both walls: {p:?}");

    // A flat ceiling 2.5 m up, under the 2.8 m top of its building sphere.
    // Under it, a push straight down against the floor's straight up is no
    // push, and it walks on; at its edge the push has a part back along the
    // floor, and it stops 1.30 m short of it, as at a doorway lower than
    // 2.8 m.
    let ceiling = |from: f64| Course::new().floor(0.0).quad([v(-40., 2.5, from), v(40., 2.5, from), v(40., 2.5, 30.), v(-40., 2.5, 30.)]);
    let mut world = ceiling(-10.0);
    let mut unit = start(&mut world, &mut parts, [0., 0., 0.], 0.0, false);
    run(&mut world, &mut parts, &mut unit, &ahead(), 6.0, &|_| false);
    let p = body(&unit).position;
    assert!(p[2] > 5.0 && p[1].abs() < 0.01, "walked on under the ceiling: {p:?}");
    let mut world = ceiling(0.0);
    let mut unit = start(&mut world, &mut parts, [0., 0., -4.], 0.0, false);
    run(&mut world, &mut parts, &mut unit, &ahead(), 6.0, &|_| false);
    let p = body(&unit).position;
    assert!((p[2] + 1.30).abs() < 0.05 && p[1].abs() < 0.01, "stopped at the ceiling's edge: {p:?}");
}

#[test]
fn guards_against_a_teleport_and_an_endless_push() {
    let mut parts = parts();
    // A move of more than 100 m in a tick meets nothing and is not ground.
    let mut world = Course::new().floor(0.0);
    let mut unit = start(&mut world, &mut parts, [0., 0., 0.], 0.0, true);
    run(&mut world, &mut parts, &mut unit, &MoveInput::default(), 0.5, &|_| false);
    assert!(body(&unit).grounded);
    body_mut(&mut unit).velocity = [0., 0., 7000.];
    droideka::tick_input(0, &mut unit, &MoveInput::default(), DT, &mut world, &mut parts);
    assert!(!body(&unit).grounded && body(&unit).position[2] > 100.0, "a teleport: {:?}", body(&unit).position);

    // A ceiling all but flat, 2.5 m up: out of it along the floor would be
    // a push of hundreds of metres; the 2004 game makes none past 5 m.
    let tilt = 0.001;
    let mut world = Course::new()
        .floor(0.0)
        .quad([v(-40., 2.5 - 40. * tilt, -40.), v(40., 2.5 + 40. * tilt, -40.), v(40., 2.5 + 40. * tilt, 40.), v(-40., 2.5 - 40. * tilt, 40.)]);
    let mut unit = start(&mut world, &mut parts, [0., 0., 0.], 0.0, false);
    let mut last = body(&unit).position;
    for _ in 0..60 {
        droideka::tick_input(0, &mut unit, &MoveInput::default(), DT, &mut world, &mut parts);
        let p = body(&unit).position;
        assert!((p[0] - last[0]).hypot(p[2] - last[2]) < 0.1 && p[1].abs() < 0.01, "no shove: {p:?}");
        last = p;
    }
}
