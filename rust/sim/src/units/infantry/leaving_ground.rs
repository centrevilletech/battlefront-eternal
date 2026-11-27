//! A soldier can leave the ground, or stop being on it, in ways its own step
//! does not see: a blast or a Force push throws it, a power tumbles it, a seat
//! takes it and puts it down, a choke holds it. The windows of its per-life
//! movement state (the standing window over an unsupported foot, the sprint
//! ramp, the roll) must not outlive the footing that allowed them, or the
//! step refuses the soldier and, before the fault was contained, the world.
use super::*;
use crate::combat::powers::{Held, LifeKey};
use crate::gen::{Air, Posture};
use crate::movement::soldier::{
    swbf2::{Roll, RollDirection, Tumble},
    V3,
};

const DT: f64 = 1. / 60.;

struct Fixture {
    infantry: Infantry,
    movement: MovementOwner,
    collision: crate::collision::State,
    world: usize,
    class: StrId,
    body: Soldier,
}

impl Fixture {
    /// One soldier of invented numbers on a flat floor, standing, after its first tick.
    fn new() -> Self {
        use bfe_content::{
            bvh::{build, Positions},
            collision::{triangle_bounds, MapCollision, TriangleGrid},
        };
        let floor = vec![-30., 0., -30., 30., 0., -30., 30., 0., 30., -30., 0., 30.];
        let positions = vec![-6., -100., -6., 6., -100., -6., 6., -100., 6., -6., -100., 6.];
        let indices = vec![0, 2, 1, 0, 3, 2];
        let buildings = build(Positions::F32(&positions), &indices);
        let terrain = build(Positions::F32(&floor), &indices);
        let map = MapCollision {
            tri_bounds: triangle_bounds(&positions, &indices),
            grid: Some(TriangleGrid::new(&floor, &indices)),
            positions: positions.into(),
            indices: indices.clone().into(),
            polygons: vec![0, 1].into(),
            masks: Vec::new().into(),
            has_terrain: true,
            terrain_positions: floor.into(),
            terrain_indices: indices.into(),
            kill_y: -200.,
        };
        let mut owner = crate::collision::CollisionOwner::new();
        let world = owner.native_install(&map, &buildings, Some(&terrain)).unwrap().world;
        let mut movement = MovementOwner::new();
        movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        let row = ControlSpeed { forward: 1., strafe: 1., turn: 1. };
        let tuning = Tuning {
            stand: row,
            crouch: row,
            sprint: row,
            jump: row,
            jet: row,
            jetpack: Some(Jetpack {
                physics: Jet {
                    jump: 7.,
                    push: 8.,
                    recharge: 0.12,
                    cost: 0.15,
                    initial: 0.25,
                    min_border: 0.24,
                    hover: true,
                },
                acceleration: 10.,
                energy_cost: 0.,
                force_jump: false,
            }),
            roll: row,
            tumble: row,
            sprint_accelerate_time: 0.5,
            energy_capacity: 100.,
            energy_floor: 0.,
            energy_restore: 15.,
            energy_restore_idle: 25.,
            energy_drain_sprint: 10.,
            energy_min_sprint: 20.,
            energy_cost_jump: 0.,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 25.,
            jump_height: 2.5,
            jump_forward_speed_factor: 1.3,
            jump_strafe_speed_factor: 1.,
            roll_speed_factor: 1.5,
            roll_frames: 20,
            recover_from_tumble: false,
            getup_front_seconds: DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: DEFAULT_GETUP_BACK_SECONDS,
        };
        let class = StrId(1);
        let infantry = Infantry {
            power_kits: vec![None],
            classes: vec![Class { name: class, registry_id: None, tuning, acceleration: None, offhand_guns: Vec::new() }],
            lives: vec![None; 2],
            bot_inputs: bot_input::BotInputs::new(2),
            mailbox: 0,
            player: 0,
            weapons: vec![None],
            heroes: vec![None, None],
            sample: None,
            contacts: Contacts::default(),
            strike: None,
            bolts: Vec::new(),
            bullets: Vec::new(),
            observed: vec![None; 2],
            life_serial: 0,
            tick: 0,
        };
        let body = Soldier {
            mover: -1.,
            max_speed: 8.,
            max_strafe: 6.,
            max_jump: 8.,
            max_turn: 2.,
            max_pitch: 0.5,
            accel: 70.,
            pitch_limits: [-0.8, 1.1],
            max_fly_height: 200.,
            grounded: true,
            normal: [0., 1., 0.],
            fuel: 0.5,
            ..Soldier::default()
        };
        let mut f = Self { infantry, movement, collision: owner.state, world: world as usize, class, body };
        f.step(MoveInput::default(), true, None).unwrap();
        f
    }

    fn step(&mut self, input: MoveInput, alive: bool, held: Option<Held>) -> Result<bool, ProfileError> {
        self.infantry.advance(
            &mut self.movement,
            &mut self.collision,
            0,
            1,
            self.class,
            1.,
            true,
            alive,
            Controls::default(),
            None,
            held,
            &mut self.body,
            &input,
            DT,
            self.world,
            -1,
        )
    }

    fn life(&mut self) -> &mut State {
        &mut self.infantry.lives[0].as_mut().unwrap().movement
    }

    /// Ticks the soldier through its flight and landing: no tick may be refused.
    fn settle(&mut self, what: &str) {
        for n in 0..240 {
            if let Err(e) = self.step(MoveInput::default(), true, None) {
                panic!("{what}: tick {n} after the change was refused: {e:?}");
            }
        }
    }
}

// What a blast does to a body (the blast's push in the combat solver).
fn blast_throw(b: &mut Soldier) {
    b.velocity = [3., 7., 1.];
    b.air = Some(Air::Thrown);
    b.state_time = 0.;
    b.grounded = false;
}

// What a Force push does to a thrown body (the power's push), then the host's
// tumble entry on the life: it ends the roll and the sprint and starts the tumble.
fn push_throw(b: &mut Soldier, life: &mut State) {
    b.velocity = [-2., 6., 4.];
    b.air = Some(Air::Thrown);
    b.state_time = 0.;
    b.grounded = false;
    b.lock = 0.;
    life.roll = None;
    life.sprint_time = None;
    life.tumble = Some(Tumble::enter(V3::new(-2., 6., 4.), 0., 7));
}

// A fast push along the ground: crouched, on the ground, at once in a ground tumble.
fn push_tumble(b: &mut Soldier, life: &mut State) {
    b.velocity = [5., 0., 0.];
    b.air = None;
    b.posture = Posture::Crouch;
    b.state_time = 0.;
    b.grounded = true;
    b.lock = 0.;
    life.roll = None;
    life.sprint_time = None;
    life.tumble = Some(Tumble::enter_ground(V3::new(5., 0., 0.), 0.));
}

// A seat takes the body (no air, at rest) and lets it out above the ground:
// at rest, off the ground, no lock, falling and standing.
fn board_and_leave_a_seat(b: &mut Soldier) {
    b.air = None;
    b.velocity = [0.; 3];
    b.dive = 0.;
    b.lock = 0.;
    b.grounded = false;
    b.position[1] = 1.5;
    b.air = Some(Air::Fall);
    b.posture = Posture::Stand;
    b.state_time = 0.;
}

fn unsupported_values() -> impl Iterator<Item = f64> {
    (0..60).map(|k| f64::from(k) * DT)
}

fn sprint_values() -> impl Iterator<Item = f64> {
    (0..=30).map(|k| f64::from(k) * DT)
}

#[test]
fn a_blast_throw_in_the_standing_window_does_not_stop_the_step() {
    for window in unsupported_values() {
        let mut f = Fixture::new();
        f.life().unsupported_time = Some(window);
        blast_throw(&mut f.body);
        f.step(MoveInput::default(), true, None)
            .unwrap_or_else(|e| panic!("window {window}: {e:?}"));
        assert_eq!(f.life().unsupported_time, None, "window {window}");
        f.settle("blast in the standing window");
    }
}

#[test]
fn a_power_throw_in_the_standing_window_does_not_stop_the_step() {
    for window in unsupported_values() {
        let mut f = Fixture::new();
        f.life().unsupported_time = Some(window);
        let mut life = *f.life();
        push_throw(&mut f.body, &mut life);
        *f.life() = life;
        f.step(MoveInput::default(), true, None)
            .unwrap_or_else(|e| panic!("window {window}: {e:?}"));
        assert_eq!(f.life().unsupported_time, None, "window {window}");
        f.settle("power throw in the standing window");
    }
}

#[test]
fn a_blast_throw_ends_a_sprint() {
    for ramp in sprint_values() {
        let mut f = Fixture::new();
        f.life().sprint_time = Some(ramp);
        blast_throw(&mut f.body);
        f.step(MoveInput::default(), true, None).unwrap_or_else(|e| panic!("ramp {ramp}: {e:?}"));
        assert_eq!(f.life().sprint_time, None, "ramp {ramp}");
        f.settle("blast in a sprint");
    }
}

#[test]
fn a_blast_throw_ends_a_roll() {
    for elapsed in [0., 0.1, 0.3, 0.5] {
        let mut f = Fixture::new();
        f.body.posture = Posture::Crouch;
        f.life().roll = Some(Roll { direction: RollDirection::Forward, elapsed });
        blast_throw(&mut f.body);
        f.step(MoveInput::default(), true, None).unwrap_or_else(|e| panic!("roll {elapsed}: {e:?}"));
        assert!(f.life().roll.is_none(), "roll {elapsed}");
        f.settle("blast in a roll");
    }
}

#[test]
fn a_crouched_body_with_a_sprint_ramp_is_not_refused() {
    // A push that tumbles a body on the ground crouches it; the ramp cannot outlive that.
    for ramp in sprint_values() {
        let mut f = Fixture::new();
        f.life().sprint_time = Some(ramp);
        f.body.posture = Posture::Crouch;
        f.body.velocity = [5., 0., 0.];
        f.body.state_time = 0.;
        f.step(MoveInput::default(), true, None).unwrap_or_else(|e| panic!("ramp {ramp}: {e:?}"));
        assert_eq!(f.life().sprint_time, None, "ramp {ramp}");
        let mut life = *f.life();
        push_tumble(&mut f.body, &mut life);
        *f.life() = life;
        f.settle("ground tumble");
    }
}

#[test]
fn a_seat_taking_and_returning_a_soldier_in_a_window_does_not_stop_the_step() {
    for window in unsupported_values() {
        let mut f = Fixture::new();
        f.life().unsupported_time = Some(window);
        f.life().sprint_time = (window <= 0.5).then_some(window);
        board_and_leave_a_seat(&mut f.body);
        f.step(MoveInput::default(), true, None)
            .unwrap_or_else(|e| panic!("window {window}: {e:?}"));
        assert_eq!((f.life().unsupported_time, f.life().sprint_time), (None, None), "window {window}");
        f.settle("seat");
    }
}

#[test]
fn a_choke_hold_keeps_the_footing_and_the_windows_valid() {
    // The hold zeroes the stick; it does not move the body.
    let held = Held {
        by: LifeKey { slot: 1, generation: 1, life: 1. },
        left: 30,
        toward: [0., 0., 1.],
        anim: 0,
    };
    for window in unsupported_values() {
        let mut f = Fixture::new();
        f.life().unsupported_time = Some(window);
        for _ in 0..30 {
            f.step(MoveInput::default(), true, Some(held)).unwrap_or_else(|e| panic!("window {window}: {e:?}"));
        }
    }
}

#[test]
fn a_jump_in_the_standing_window_ends_the_window() {
    for window in unsupported_values() {
        for input in [MoveInput { jump: true, ..MoveInput::default() }, MoveInput { jump: true, crouch: true, ..MoveInput::default() }] {
            let mut f = Fixture::new();
            f.life().unsupported_time = Some(window);
            f.step(input.clone(), true, None).unwrap_or_else(|e| panic!("window {window}: {e:?}"));
            f.step(input, true, None).unwrap_or_else(|e| panic!("window {window}: {e:?}"));
            if f.body.air.is_some() {
                assert_eq!(f.life().unsupported_time, None, "window {window}");
            }
            f.settle("jump");
        }
    }
}

#[test]
fn a_killed_soldier_thrown_in_a_window_goes_on_to_lie_down() {
    for window in unsupported_values() {
        let mut f = Fixture::new();
        f.life().unsupported_time = Some(window);
        blast_throw(&mut f.body);
        for n in 0..240 {
            f.step(MoveInput::default(), false, None)
                .unwrap_or_else(|e| panic!("window {window}: tick {n}: {e:?}"));
        }
    }
}

/// Every writer at every tick of every window, back to back: whatever one does,
/// the next tick is accepted and every window that remains is one the body allows.
#[test]
fn throws_pushes_blasts_and_seats_at_every_tick_of_every_window_are_accepted() {
    let writers: [(&str, fn(&mut Fixture)); 6] = [
        ("blast", |f| blast_throw(&mut f.body)),
        ("power throw", |f| {
            let mut life = *f.life();
            push_throw(&mut f.body, &mut life);
            *f.life() = life;
        }),
        ("power tumble", |f| {
            let mut life = *f.life();
            push_tumble(&mut f.body, &mut life);
            *f.life() = life;
        }),
        ("seat", |f| board_and_leave_a_seat(&mut f.body)),
        ("jet", |f| {
            f.body.velocity = [0., 4., 0.];
            f.body.air = Some(Air::Jet);
            f.body.grounded = false;
        }),
        ("nothing", |_| {}),
    ];
    for (name, write) in writers {
        for window in unsupported_values() {
            for with in 0..3 {
                let mut f = Fixture::new();
                match with {
                    0 => f.life().unsupported_time = Some(window),
                    1 => f.life().sprint_time = Some(window.min(0.5)),
                    _ => {
                        f.body.posture = Posture::Crouch;
                        f.life().roll = Some(Roll { direction: RollDirection::Left, elapsed: window.min(0.6) });
                    }
                }
                write(&mut f);
                for n in 0..40 {
                    f.step(MoveInput::default(), true, None)
                        .unwrap_or_else(|e| panic!("{name}, window {window}, case {with}: tick {n}: {e:?}"));
                }
            }
        }
    }
}

#[test]
fn a_window_the_body_still_allows_is_kept() {
    let mut f = Fixture::new();
    f.body.grounded = false;
    f.life().unsupported_time = Some(0.2);
    // Unsupported on its feet, no air: the standing window is the body's own.
    let kept = f.life().end_windows_the_body_left(crate::movement::soldier::swbf2::BodyFacts {
        air: crate::movement::soldier::Air::None,
        grounded: false,
        lock: 0.,
        posture: crate::movement::soldier::Posture::Stand,
    });
    assert!(!kept);
    assert_eq!(f.life().unsupported_time, Some(0.2));
}
