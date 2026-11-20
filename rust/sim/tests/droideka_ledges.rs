//! A droideka against a step across its whole way, with the real tick. As in
//! the 2004 game, deployed it rides up an edge lower than 0.42 m on its 1.4 m
//! building sphere and stops at a higher one; its ball, rolling at full
//! speed, takes a kerb but bounces back from an edge it meets steeper than
//! MaxBallAngle; and it falls at four times 9.8 m/s2.
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

/// The course: quads (each one polygon), no terrain.
struct Course {
    quads: Vec<[Vector3; 4]>,
}

impl Course {
    /// Level ground to z = 5, then a step of `height` across the whole way.
    fn step(height: f64) -> Self {
        let v = Vector3::new;
        Course {
            quads: vec![
                [v(-40., 0., -60.), v(-40., 0., 5.), v(40., 0., 5.), v(40., 0., -60.)],
                [v(-40., 0., 5.), v(-40., height, 5.), v(40., height, 5.), v(40., 0., 5.)],
                [v(-40., height, 5.), v(-40., height, 70.), v(40., height, 70.), v(40., height, 5.)],
            ],
        }
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
    fn ground(&mut self, _: f64, _: f64, _: f64) -> Option<(f64, Vector3)> {
        None
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

fn droideka(z: f64) -> Unit {
    let p = [0., 0., z];
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

/// Drives a droideka (rolled up first if `rolled`) straight at the step for
/// `seconds`; whether it got 2 m onto the step.
fn over(height: f64, rolled: bool, z: f64, seconds: f64) -> bool {
    let mut world = Course::step(height);
    let mut parts = parts();
    let mut unit = droideka(z);
    let mut input = MoveInput { r#use: rolled, ..Default::default() };
    if rolled {
        for _ in 0..120 {
            droideka::tick_input(0, &mut unit, &input, DT, &mut world, &mut parts);
            input.r#use = false;
        }
        assert_eq!(body(&unit).mode, 12.0, "the droideka rolls up");
    }
    input.forward = 1.0;
    for _ in 0..(seconds / DT) as usize {
        droideka::tick_input(0, &mut unit, &input, DT, &mut world, &mut parts);
        if body(&unit).position[2] > 7.0 {
            return true;
        }
    }
    false
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
    let mut world = Course::step(0.0);
    let mut parts = parts();
    let mut unit = droideka(0.0);
    if let UnitBody::Droideka(d) = &mut unit.soldier {
        d.position[1] = 10.0;
        d.previous = d.position;
    }
    let input = MoveInput::default();
    let mut ticks = 0;
    while body(&unit).position[1] > 0.01 {
        droideka::tick_input(0, &mut unit, &input, DT, &mut world, &mut parts);
        ticks += 1;
        assert!(ticks < 120, "it lands");
    }
    // sqrt(2 x 10 / (4 x 9.8)) = 0.71 s; at 9.8 it would take 1.43 s.
    let seconds = ticks as f64 * DT;
    assert!((0.70..0.76).contains(&seconds), "a 10 m fall took {seconds} s");
}
