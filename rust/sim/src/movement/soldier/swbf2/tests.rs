use super::*;
use crate::collision;
use crate::movement::{self, MovementOwner};
use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
};
use bfe_family::GameFamily;

const DT: f64 = 1.0 / 60.0;

fn tuning() -> Tuning {
    let speed = |forward, strafe, turn| ControlSpeed {
        forward,
        strafe,
        turn,
    };
    Tuning {
        stand: speed(1.0, 1.0, 1.0),
        crouch: speed(0.5, 0.5, 1.0),
        sprint: speed(1.75, 0.4, 0.5),
        jump: speed(0.04, 0.04, 0.4),
        jet: speed(0.8, 0.65, 0.4),
        jetpack: None,
        roll: speed(0.0, 0.0, 0.4),
        tumble: speed(0.0, 0.0, 0.2),
        sprint_accelerate_time: 0.25,
        energy_capacity: 60.0,
        energy_floor: 0.0,
        energy_restore: 6.0,
        energy_restore_idle: 9.0,
        energy_drain_sprint: 16.0,
        energy_min_sprint: 8.0,
        energy_cost_jump: 2.0,
        energy_cost_jump_sprint: None,
        energy_cost_roll: 12.0,
        jump_height: 2.0,
        jump_forward_speed_factor: 1.25,
        jump_strafe_speed_factor: 0.75,
        roll_speed_factor: 2.0,
        roll_frames: 20,
        recover_from_tumble: false,
        getup_front_seconds: DEFAULT_GETUP_FRONT_SECONDS,
        getup_back_seconds: DEFAULT_GETUP_BACK_SECONDS,
    }
}

fn ground(roof: bool) -> (u32, collision::State) {
    terrain(40.0, 0.0, roof.then_some([-8., 1.8, 1., 8., 1.8, 1., 8., 1.8, 8., -8., 1.8, 8.]))
}

fn terrain(end: f32, slope: f32, building: Option<[f32; 12]>) -> (u32, collision::State) {
    let floor = vec![-40., -40. * slope, -40., 40., -40. * slope, -40.,
        40., end * slope, end, -40., end * slope, end];
    let positions = building.unwrap_or([-6., -100., -6., 6., -100., -6.,
        6., -100., 6., -6., -100., 6.]).to_vec();
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
    let mut collision = collision::CollisionOwner::new();
    let world = collision.native_install(&map, &buildings, Some(&terrain)).unwrap().world;
    (world, collision.state)
}

struct Rig {
    collision: collision::State,
    movement: MovementOwner,
    id: u32,
    tuning: Tuning,
    state: State,
}

impl Rig {
    fn new(roof: bool) -> Self {
        Self::on(ground(roof))
    }

    fn on((world, collision): (u32, collision::State)) -> Self {
        let tuning = tuning();
        let mut movement = MovementOwner::new();
        movement.select_family(GameFamily::Swbf2).unwrap();
        let id = movement.new_mover(world as usize, |mover| {
            mover.grounded = true;
            mover.max_speed = 6.0;
            mover.max_strafe = 5.0;
            mover.dive_time = 0.6;
        });
        Self {
            collision,
            movement,
            id,
            tuning,
            state: State::new(&tuning),
        }
    }

    fn tick(&mut self, input: Input, actions: Actions) -> bool {
        self.try_tick(input, actions, DT).unwrap()
    }

    fn try_tick(&mut self, input: Input, actions: Actions, dt: f64) -> Result<bool, ProfileError> {
        self.movement.advance_profiled(
            &mut self.collision,
            self.id,
            &input,
            dt,
            Some(Step { override_: None,
                actions,
                tuning: &self.tuning,
                state: &mut self.state,
            }),
        )
    }

    fn mover(&mut self) -> &mut Mover {
        self.movement.mover(self.id)
    }
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn sprint_from_rest_walks_to_speed_then_ramps_and_recovers_by_velocity() {
    let mut rig = Rig::new(false);
    let input = Input { forward: 1., ..Input::default() };
    let sprint = Actions { sprint: true, ..Actions::default() };
    let mut walked = 0;
    while rig.mover().velocity.z <= 0.9 * rig.mover().max_speed {
        assert!(rig.tick(input, sprint));
        assert!(rig.state.sprint_time.is_none());
        assert_eq!(rig.state.energy, rig.tuning.energy_capacity);
        walked += 1;
        assert!(walked < 60);
    }
    for tick in 1..=60 {
        assert!(!rig.tick(input, sprint));
        near(rig.state.sprint_time.unwrap(), js_min(tick as f64 * DT, rig.tuning.sprint_accelerate_time));
    }
    near(rig.mover().velocity.z, 6.0 * rig.tuning.sprint.forward);
    near(rig.state.energy, 44.0);
    rig.tick(Input::default(), Actions::default());
    near(rig.state.energy, 44.0 + rig.tuning.energy_restore * DT);
    rig.mover().velocity = V3::ZERO;
    rig.tick(input, Actions::default());
    near(rig.state.energy, 44.0 + (rig.tuning.energy_restore + rig.tuning.energy_restore_idle) * DT);
}

#[test]
fn exact_spend_and_overdraw_are_distinct_and_recovery_clears_exhaustion() {
    let mut rig = Rig::new(false);
    let forward = Input { forward: 1., ..Input::default() };
    let sprint = Actions { sprint: true, roll: false, ai: false };
    rig.tuning.energy_drain_sprint = 15.;
    rig.state.energy = 15. * DT;
    rig.mover().velocity.z = 6.;
    assert!(!rig.tick(forward, sprint));
    assert_eq!(rig.state.energy, 0.);
    assert!(!rig.state.exhausted);
    rig.tuning.energy_floor = -0.125;
    assert!(!rig.tick(forward, sprint));
    assert!(rig.state.exhausted);
    assert_eq!(rig.state.energy, -0.125);
    assert!(rig.tick(forward, sprint));
    assert!(rig.state.sprint_time.is_none());
    assert!(rig.state.exhausted);
}

#[test]
fn roll_boosts_existing_motion_once_and_uses_the_class_steering_row() {
    for (vx, vz) in [(0., 0.), (0., 0.05), (3., 4.), (-3., 4.), (3., -4.), (0., 14.)] {
        let mut rig = Rig::new(false);
        rig.mover().velocity = V3::new(vx, 0., vz);
        let input = Input { forward: 1., ..Input::default() };
        assert!(!rig.tick(input, ROLL));
        let gain = if vx.abs() == 3.0 { 12.0 / 5.0 - 1.0 } else { 0. };
        near(rig.mover().velocity.x, vx * (1. + gain));
        if vz == 0.05 {
            // Ground contact can remove this small residual. It cannot become a boost.
            assert!(rig.mover().velocity.z.abs() <= vz);
        } else {
            near(rig.mover().velocity.z, vz + js_max(vz, 0.) * gain);
        }
        assert_eq!(rig.state.roll.unwrap().direction, RollDirection::Forward);
        near(rig.state.energy, rig.tuning.energy_capacity - rig.tuning.energy_cost_roll);
        assert_eq!(rig.state.hit_posture(Posture::Stand), Posture::Crouch);
        rig.tuning.roll.strafe = 0.7;
        let before = rig.mover().velocity.x;
        rig.tick(Input { right: if before >= 0. { 1. } else { -1. }, ..Input::default() }, Actions::default());
        assert_ne!(rig.mover().velocity.x, before);
    }
}

#[test]
fn roll_expiry_requests_standing_without_the_crouch_clearance_check() {
    let mut rig = Rig::new(true);
    rig.mover().position.z = 3.;
    rig.mover().settled.z = 3.;
    rig.mover().posture = Posture::Crouch;
    rig.tuning.roll_frames = 5;
    let input = Input { forward: 1., ..Input::default() };
    assert!(!rig.tick(input, ROLL));
    while rig.state.roll.is_some() {
        assert!(!rig.tick(Input::default(), Actions::default()));
    }
    assert_eq!(rig.mover().posture, Posture::Stand);
}

#[test]
fn crouch_toggles_on_foot_and_prone_has_no_effect() {
    let mut rig = Rig::new(false);
    let sprint = Actions { sprint: true, ..Actions::default() };
    let forward = Input { forward: 1.0, prone: true, ..Input::default() };
    rig.mover().velocity.z = 6.;
    assert!(!rig.tick(forward, sprint));
    assert_eq!(rig.mover().posture, Posture::Stand);
    let crouch = Input { crouch: true, ..forward };
    assert!(!rig.tick(crouch, sprint));
    assert!(rig.tick(forward, Actions::default()));
    assert!(rig.tick(crouch, sprint));
    assert_eq!(rig.mover().posture, Posture::Crouch);
    assert!(rig.tick(forward, sprint));
    assert_eq!(rig.mover().posture, Posture::Crouch);
    rig.mover().velocity.z = 6.;
    assert!(rig.tick(crouch, sprint));
    assert_eq!(rig.mover().posture, Posture::Stand);
}

#[test]
fn jumping_leaves_sprint_and_replaces_the_strafe_dive() {
    let mut rig = Rig::new(false);
    let sprint = Actions { sprint: true, ..Actions::default() };
    let forward = Input { forward: 1.0, ..Input::default() };
    for _ in 0..60 {
        rig.tick(forward, sprint);
    }
    let speed = rig.mover().velocity.z;
    let energy = rig.state.energy;
    let height = rig.mover().position.y;
    assert!(rig.tick(Input { jump: true, right: 1.0, ..Input::default() }, sprint));
    assert_eq!(rig.mover().air, Air::Jump);
    assert_eq!(rig.mover().dive, 0.0);
    assert!(rig.state.sprint_time.is_none());
    near(rig.mover().velocity.z, speed * rig.tuning.jump_forward_speed_factor);
    near(rig.state.energy, energy - rig.tuning.energy_cost_jump);
    let mut apex = height;
    for _ in 1..120 {
        rig.tick(Input::default(), Actions::default());
        apex = js_max(apex, rig.mover().position.y);
    }
    let rise = apex - height;
    assert!((rig.tuning.jump_height - 0.25..rig.tuning.jump_height).contains(&rise));
    assert_eq!(rig.mover().air, Air::None);
}

#[test]
fn repeated_profile_steps_produce_identical_state_bits() {
    let mut a = Rig::new(false);
    let mut b = Rig::new(false);
    for i in 0..160 {
        let input = Input {
            forward: 0.8,
            right: if i < 80 { -0.2 } else { 0.9 },
            turn: 0.5,
            pitch: 0.1,
            crouch: i == 65 || i == 76,
            jump: i == 135,
            ..Input::default()
        };
        let actions = Actions { sprint: i < 45, roll: i == 90, ai: false };
        assert_eq!(a.tick(input, actions), b.tick(input, actions));
        let (mut av, mut bv) = ([0.0; movement::IO_LEN], [0.0; movement::IO_LEN]);
        movement::store(a.mover(), &mut av);
        movement::store(b.mover(), &mut bv);
        assert_eq!(av.map(f64::to_bits), bv.map(f64::to_bits), "tick {i}");
        assert_eq!(a.state.energy.to_bits(), b.state.energy.to_bits());
        assert_eq!(a.state.sprint_time.map(f64::to_bits), b.state.sprint_time.map(f64::to_bits));
        assert_eq!(a.state.exhausted, b.state.exhausted);
        let roll_bits = |roll: Roll| (roll.direction, roll.elapsed.to_bits());
        assert_eq!(a.state.roll.map(roll_bits), b.state.roll.map(roll_bits));
    }
}

#[test]
fn store_and_prediction_run_the_same_profile_and_report_missing_state() {
    use crate::gen::{MoveInput, Soldier, UnitBody};
    use crate::prediction::actors::Session;
    use crate::units::soldier::tick_soldier_in;

    let (world, mut collision) = ground(false);
    let mut owner = MovementOwner::new();
    owner.select_family(GameFamily::Swbf2).unwrap();
    let mut soldier = Soldier {
        mover: -1.0, max_speed: 6.0, max_strafe: 5.0, accel: 52.0, max_turn: 2.0,
        grounded: true, normal: [0.0, 1.0, 0.0], max_fly_height: 150.0, ..Soldier::default()
    };
    let input = MoveInput { forward: 1.0, turn: 0.3, ..MoveInput::default() };
    let mut actor = Session::default();
    actor.configure_for_family(&[0.0], GameFamily::Swbf2).unwrap();
    actor.configure(&[0.0]).unwrap(); // A later body switch retains the battle family.
    actor.unit.soldier = UnitBody::Soldier(soldier.clone());
    actor.unit.input = input.clone();
    assert_eq!(actor.step(&mut collision, world as usize, -1, &[], DT, (0.0, V3::ZERO, false),
        true, false, false, false, false, false, 0.0, None), Err(movement::ProfileError::MissingStep.message()));
    assert_eq!(tick_soldier_in(&mut owner, &mut collision, &mut soldier, &input, DT, world as usize, -1, None),
        Err(movement::ProfileError::MissingStep));
    assert_eq!(soldier.mover, -1.0);

    let tuning = tuning();
    let mut authority = State::new(&tuning);
    let mut replay = authority;
    for tick in 0..45 {
        let actions = Actions { sprint: tick < 10, roll: tick == 10, ai: false };
        let allowed = tick_soldier_in(&mut owner, &mut collision, &mut soldier, &input, DT, world as usize, -1,
            Some(Step { override_: None, actions, tuning: &tuning, state: &mut authority })).unwrap();
        let predicted = actor.step(&mut collision, world as usize, -1, &[], DT, (0.0, V3::ZERO, false),
            true, false, false, false, false, false, 0.0,
            Some(Step { override_: None, actions, tuning: &tuning, state: &mut replay })).unwrap();
        assert_eq!(allowed, predicted);
        if tick == 0 || tick >= 39 { assert!(allowed); }
        if (10..39).contains(&tick) { assert!(!allowed); }
        let UnitBody::Soldier(s) = &actor.unit.soldier else { unreachable!() };
        assert_eq!(s.position.map(f64::to_bits), soldier.position.map(f64::to_bits));
        assert_eq!(s.aim_yaw.to_bits(), soldier.aim_yaw.to_bits());
        assert_eq!(replay.energy.to_bits(), authority.energy.to_bits());
    }
}

#[test]
fn legacy_exchanges_refuse_a_2005_tick_before_loading_it() {
    use crate::abi::context::MovementError;
    let (world, mut collision) = ground(false);
    let mut owner = MovementOwner::new();
    owner.select_family(GameFamily::Swbf2).unwrap();
    let id = owner.native_new_mover(world, [0.0; 21]).unwrap();
    let before = owner.native_snapshot(id).unwrap().io;
    let mut io = before;
    io[movement::POSITION] = 7.0;
    *owner.io_mut() = io;
    assert_eq!(owner.tick(&mut collision, id), 3);
    assert_eq!(owner.mover(id).position, V3::ZERO);
    assert!(matches!(owner.native_tick(id, io, vec![], &mut collision),
        Err(MovementError::Profile(movement::ProfileError::MissingStep))));
    assert!(matches!(owner.native_advance(id, Input::default(), DT, vec![], &mut collision),
        Err(MovementError::Profile(movement::ProfileError::MissingStep))));
    assert_eq!(owner.native_snapshot(id).unwrap().io.map(f64::to_bits), before.map(f64::to_bits));
    assert_eq!(owner.mover(id).position, V3::ZERO);
}

#[test]
fn expired_restored_roll_finishes_before_displacement() {
    for elapsed in [-0.5, 1.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
        for lock in [0.0, 0.5] {
            let mut rig = Rig::new(false);
            rig.state.roll = Some(Roll { direction: RollDirection::Forward, elapsed });
            rig.mover().velocity.z = 12.0;
            rig.mover().lock = lock;
            let input = Input { forward: f64::from(lock > 0.0), ..Input::default() };
            assert_eq!(rig.tick(input, Actions::default()), lock == 0.0);
            assert!(rig.state.roll.is_none());
            assert_eq!(rig.mover().position, V3::ZERO);
            assert_eq!(rig.mover().velocity, V3::ZERO);
            near(rig.mover().lock, js_max(0.0, lock - DT));
        }
    }
}

#[test]
fn zero_speed_roll_never_normalizes_a_zero_vector() {
    let mut rig = Rig::new(false);
    assert!(!rig.tick(Input { right: 1., ..Input::default() }, ROLL));
    assert_eq!(rig.mover().position, V3::ZERO);
    assert_eq!(rig.mover().velocity, V3::ZERO);
}

#[test]
fn timestep_and_tuning_are_checked_before_actions_or_motion() {
    let mut rig = Rig::new(false);
    let roll = Actions { roll: true, sprint: false, ai: false };
    for dt in [-0.25, f64::INFINITY, f64::NAN] {
        assert_eq!(rig.try_tick(Input::default(), roll, dt), Err(ProfileError::InvalidDelta));
    }
    assert_eq!(rig.try_tick(Input::default(), roll, 0.0), Ok(true));
    assert!(rig.state.roll.is_none());
    let good = rig.tuning;
    for invalid in [Tuning { energy_cost_roll: -0.1, ..good }, Tuning { energy_floor: f64::NEG_INFINITY, ..good },
        Tuning { jump: ControlSpeed { strafe: f64::NAN, ..good.jump }, ..good }] {
        rig.tuning = invalid;
        assert_eq!(rig.try_tick(Input::default(), roll, DT), Err(ProfileError::InvalidTuning));
    }
    rig.tuning = good;
    rig.mover().accel = f64::NAN;
    assert_eq!(rig.try_tick(Input::default(), roll, DT), Err(ProfileError::InvalidTuning));
    assert_eq!(rig.state.energy, good.energy_capacity);
    assert_eq!(rig.mover().position, V3::ZERO);
    assert_eq!(rig.mover().state_time, 0.0);
}

#[test]
fn retained_slots_keep_their_battle_family_through_readmission_and_removal() {
    let mut owner = MovementOwner::new();
    owner.select_family(GameFamily::Swbf2).unwrap();
    let id = owner.native_new_mover(0, [0.0; 21]).unwrap();
    owner.mover(id).position.z = 7.0;
    owner.select_family(GameFamily::Swbf2).unwrap();
    let error = owner.select_family(GameFamily::Swbf1).unwrap_err();
    assert!(error.contains("fresh world"));
    assert_eq!(owner.native_snapshot(id).unwrap().mover.position.z, 7.0);
    assert_eq!(owner.mover(id).family(), GameFamily::Swbf2);
    owner.native_remove(id).unwrap();
    assert!(owner.native_snapshot(id).is_err());
    assert_eq!(owner.mover(id).family(), GameFamily::Swbf2);
    let next = owner.native_new_mover(0, [0.0; 21]).unwrap();
    assert!(next > id);
    assert_eq!(owner.mover(next).family(), GameFamily::Swbf2);
}

fn airborne_jet(hover: bool, energy_cost: f64, force_jump: bool) -> Rig {
    let mut rig = Rig::new(false);
    rig.tuning.jetpack = Some(Jetpack {
        physics: Jet { jump: 4.0, push: 7.25, recharge: 0.04, cost: 0.42,
            initial: 0.17, min_border: 0.65, hover },
        acceleration: 110.0,
        energy_cost,
        force_jump,
    });
    let m = rig.mover();
    m.air = Air::Fall;
    m.grounded = false;
    m.position.y = 11.0;
    m.settled = m.position;
    m.fuel = 0.125;
    rig.state.energy = 10.0;
    rig
}

#[test]
fn force_double_jump_stays_in_jump_pays_energy_once_and_replaces_planar_velocity() {
    let mut rig = Rig::new(false);
    rig.tuning.energy_capacity = 60.0;
    rig.tuning.energy_min_sprint = 12.0;
    rig.tuning.energy_cost_jump = 0.0;
    rig.tuning.jump_height = 3.0;
    rig.tuning.jetpack = Some(Jetpack {
        physics: Jet { jump: 8.0, push: 0.0, recharge: 0.0, cost: 0.0, initial: 0.0, min_border: 0.0, hover: false },
        acceleration: 10.0,
        energy_cost: 24.0,
        force_jump: true,
    });
    rig.state = State::new(&rig.tuning);
    let jump_v = (2.0 * GRAVITY * rig.tuning.jump_height).sqrt();

    // Launch from a sprint so planar speed exceeds max_speed, then trigger a Force double jump.
    rig.mover().velocity.z = 11.0;
    rig.tick(Input { jump: true, forward: 1.0, ..Input::default() }, Actions::default());
    assert_eq!(rig.mover().air, Air::Jump);
    assert_eq!(rig.state.jump_count, 1);
    near(rig.state.energy, 60.0);

    // Double jump while rising: clamps vertical speed to 1.5 * jump_v, replaces horizontal speed
    // from stick (no compounding sprint boost), stays in Air::Jump, and pays energy_cost once.
    rig.tick(Input { jump: true, forward: 1.0, ..Input::default() }, Actions::default());
    assert_eq!(rig.mover().air, Air::Jump);
    assert_eq!(rig.state.jump_count, 2);
    near(rig.state.energy, 36.0);
    near(rig.mover().velocity.y, 1.5 * jump_v);
    near(rig.mover().velocity.z, rig.mover().max_speed);
    near(rig.mover().velocity.x, 0.0);

    // Subsequent airborne ticks do not keep draining energy.
    for _ in 0..5 {
        rig.tick(Input::default(), Actions::default());
    }
    assert_eq!(rig.mover().air, Air::Jump);
    near(rig.state.energy, 36.0);

    // Second and third mid-air jumps spend 36 -> 12 -> 0 (exhausted); fourth is refused.
    rig.mover().velocity = V3::new(0.0, -14.0, 0.0);
    rig.tick(Input { jump: true, right: 1.0, ..Input::default() }, Actions::default());
    assert_eq!(rig.state.jump_count, 3);
    near(rig.state.energy, 12.0);
    assert!(!rig.state.exhausted);
    assert!(rig.mover().velocity.y > 0.0 && rig.mover().velocity.y < 1.5 * jump_v);
    near(rig.mover().velocity.x, -rig.mover().max_strafe);

    rig.tick(Input { jump: true, ..Input::default() }, Actions::default());
    assert_eq!(rig.state.jump_count, 4);
    near(rig.state.energy, 0.0);
    assert!(rig.state.exhausted);

    rig.tick(Input { jump: true, ..Input::default() }, Actions::default());
    assert_eq!(rig.state.jump_count, 4);
    assert!(rig.state.exhausted);
}

#[test]
fn hybrid_hover_charges_energy_once_and_drains_fuel_without_recharging_in_flight() {
    let mut rig = airborne_jet(true, 18.0, false);
    rig.state.energy = 40.0;
    rig.mover().fuel = 0.8;
    let press = Input { jump: true, ..Input::default() };

    rig.tick(press, Actions::default());
    assert_eq!(rig.mover().air, Air::Jet);
    near(rig.state.energy, 22.0);
    let ignition_fuel = 0.8 + 0.04 * DT - 0.17;
    near(rig.mover().fuel, ignition_fuel);

    // During flight, energy stays unchanged and fuel burns without recharging.
    rig.tick(Input::default(), Actions::default());
    assert_eq!(rig.mover().air, Air::Jet);
    near(rig.state.energy, 22.0);
    near(rig.mover().fuel, ignition_fuel - 0.42 * DT);

    // Exhausted stamina blocks ignition without consuming fuel.
    let mut blocked = airborne_jet(true, 18.0, false);
    blocked.mover().fuel = 0.9;
    blocked.state.energy = 0.0;
    blocked.state.exhausted = true;
    blocked.tick(press, Actions::default());
    assert_eq!(blocked.mover().air, Air::Fall);
    near(blocked.mover().fuel, 0.9 + 0.04 * DT);
}

#[test]
fn tank_jet_charges_its_border_ignition_and_flight_from_fuel() {
    let mut rig = airborne_jet(true, 0.0, false);
    let press = Input { jump: true, ..Input::default() };
    rig.tick(press, Actions::default());
    assert_eq!(rig.mover().air, Air::Fall);
    rig.mover().fuel = 0.8;
    rig.tick(press, Actions::default());
    let ignition = 0.8 + 0.04 * DT - 0.17;
    near(rig.mover().fuel, ignition);
    assert_eq!(rig.mover().air, Air::Jet);
    let energy = rig.state.energy;
    rig.tick(Input::default(), Actions::default());
    near(rig.mover().fuel, ignition - 0.42 * DT);
    near(rig.state.energy, energy);

    let mut ordinary = airborne_jet(true, 0.0, false);
    ordinary.tuning.jetpack = None;
    ordinary.mover().fuel = 1.0;
    ordinary.mover().jet.jump = 8.0;
    ordinary.tick(press, Actions::default());
    assert_eq!(ordinary.mover().air, Air::Fall);
}

#[test]
fn restored_hover_drives_along_the_raised_aim_and_the_right_axis_at_the_class_rates() {
    for acceleration in [0.0, 50000.0] {
        let mut rig = airborne_jet(true, 0.0, false);
        rig.tuning.jetpack.as_mut().unwrap().acceleration = acceleration;
        rig.mover().air = Air::Jet;
        rig.mover().aim_pitch = 0.47;
        let input = Input { forward: 0.5, right: 0.25, turn: 0.75, ..Input::default() };
        rig.tick(input, Actions::default());
        let row = rig.tuning.jet;
        let m = rig.mover();
        let moving = f64::from(acceleration > 0.0);
        let mut aim = V3::new(0.0, (v8math::sin(0.47) + 0.08) * 2.0, v8math::cos(0.47));
        aim.normalize();
        near(m.velocity.z, moving * aim.z * input.forward * m.max_speed * row.forward);
        near(m.velocity.x, -moving * input.right * m.max_strafe * row.strafe);
        near(m.aim_yaw, -m.max_turn * row.turn * input.turn * DT);
    }
}

#[test]
fn a_hover_with_the_stick_at_rest_hangs_and_its_speed_decays_by_the_game_rate() {
    let mut rig = airborne_jet(true, 0.0, false);
    rig.tuning.jetpack.as_mut().unwrap().acceleration = 0.0;
    rig.mover().air = Air::Jet;
    rig.mover().max_fly_height = 1000.0;
    rig.mover().fuel = 0.9;
    rig.mover().velocity = V3::new(1.0, 2.0, -3.0);
    let before = rig.mover().position;
    rig.tick(Input::default(), Actions::default());
    let decay = v8math::exp(-2.0 * DT);
    let m = rig.mover();
    assert_eq!(m.air, Air::Jet);
    near(m.velocity.x, decay);
    near(m.velocity.y, 2.0 * decay);
    near(m.velocity.z, -3.0 * decay);
    near(m.position.y - before.y, 2.0 * decay * DT);
    // The stick's push along the aim is capped at JetAcceleration x dt.
    let mut push = airborne_jet(true, 0.0, false);
    push.mover().air = Air::Jet;
    push.mover().max_fly_height = 1000.0;
    push.mover().fuel = 0.9;
    push.tick(Input { forward: 1.0, ..Input::default() }, Actions::default());
    let mut aim = V3::new(0.0, 0.16, 1.0);
    aim.normalize();
    near(push.mover().velocity.z, aim.z * 110.0 * DT);
}

const ROLL: Actions = Actions { roll: true, sprint: false, ai: false };

#[test]
fn held_sprint_waits_for_exhaustion_recovery_and_accepts_a_fresh_low_balance() {
    let mut rig = Rig::new(false);
    let forward = Input { forward: 1., ..Input::default() };
    let held = Actions { sprint: true, roll: false, ai: false };
    let minimum = rig.tuning.energy_min_sprint;
    rig.mover().velocity.z = 6.;
    rig.state.energy = minimum.next_down();
    assert!(!rig.tick(forward, held));
    while !rig.state.exhausted { assert!(!rig.tick(forward, held)); }
    assert_eq!(rig.state.energy, 0.);
    while rig.state.energy < minimum {
        assert!(rig.tick(forward, held));
        assert_eq!(rig.state.exhausted, rig.state.energy < minimum);
    }
    assert!(!rig.tick(forward, held));
    assert_eq!(rig.state.sprint_time, Some(DT));
}

#[test]
fn unsupported_roll_enters_fall_with_its_horizontal_velocity() {
    let mut rig = Rig::on(terrain(1.0, 0.0, None));
    rig.mover().velocity.z = 2.;
    assert!(!rig.tick(Input { forward: 1., ..Input::default() }, ROLL));
    for _ in 0..10 {
        if rig.state.roll.is_none() { break; }
        assert!(!rig.tick(Input::default(), Actions::default()));
    }
    assert!(rig.state.roll.is_none());
    assert!(!rig.mover().grounded);
    assert_eq!(rig.mover().air, Air::Fall);
    let pos = rig.mover().position;
    assert!((0.5..2.0).contains(&pos.z));
    near(rig.mover().velocity.z, 12.0);
    near(rig.state.energy, rig.tuning.energy_capacity - rig.tuning.energy_cost_roll);
    rig.tick(Input::default(), Actions::default());
    assert!(rig.mover().position.y < pos.y);
    assert!(rig.mover().position.z > pos.z);
}

#[test]
fn wall_clips_roll_motion_while_its_window_elapses() {
    let wall = [-8., -1., 1.5, 8., -1., 1.5, 8., 5., 1.5, -8., 5., 1.5];
    let mut rig = Rig::on(terrain(40.0, 0.0, Some(wall)));
    let ticks = (rig.tuning.roll_seconds() / DT).ceil() as usize;
    rig.mover().velocity.z = 2.;
    for i in 0..ticks {
        assert!(!rig.tick(Input { forward: 1., ..Input::default() }, if i == 0 { ROLL } else { Actions::default() }));
        if i > 6 {
            near(rig.mover().position.z, 1.5 - 0.15 - 0.57);
            near(rig.mover().velocity.z, 0.0);
        }
        assert_eq!(rig.state.roll.is_some(), i + 1 < ticks);
    }
    assert!(rig.mover().grounded);
}

#[test]
fn roll_follows_walkable_slopes_in_both_directions() {
    for slope in [-0.125, 0.125] {
        let mut rig = Rig::on(terrain(40.0, slope, None));
        rig.tick(Input::default(), Actions::default());
        let start = rig.mover().position;
        rig.mover().velocity.z = 2.;
        let ticks = (rig.tuning.roll_seconds() / DT).ceil() as usize;
        for i in 0..ticks {
            assert!(!rig.tick(Input { forward: 1., ..Input::default() }, if i == 0 { ROLL } else { Actions::default() }));
            assert!(rig.mover().grounded);
            assert_eq!(rig.state.roll.is_some(), i + 1 < ticks);
            if i > 0 && i + 1 < ticks {
                let m = rig.mover();
                near(m.velocity.y, m.velocity.z * f64::from(slope));
                assert!(m.velocity.z > 10.);
            }
        }
        let delta = V3::sub(rig.mover().position, start);
        assert!(delta.z > 4.0);
        assert!((delta.y - delta.z * f64::from(slope)).abs() < 0.05);
    }
}

#[test]
fn chained_rolls_debit_once_per_accepted_edge_and_gate_on_exhaustion() {
    let mut rig = Rig::new(false);
    rig.tuning.roll_frames = 3;
    rig.state.energy = rig.tuning.energy_cost_roll;
    let input = Input { right: -1., ..Input::default() };
    for exhausted in [false, true] {
        assert!(!rig.tick(input, ROLL));
        assert_eq!(rig.state.exhausted, exhausted);
        assert_eq!(rig.state.energy, 0.);
        while rig.state.roll.is_some() {
            assert!(!rig.tick(input, ROLL));
            assert_eq!(rig.state.energy, 0.);
        }
    }
    assert!(rig.tick(input, ROLL));
    assert!(rig.state.roll.is_none());
}

#[test]
fn roll_direction_gate_rejects_backwards_and_short_stick() {
    for (forward, right, expected) in [
        (-1., 0., None), (-1., 0.839, None), (0.89, 0., None), (0., 0., None),
        (-1., -0.84, Some(RollDirection::Left)), (0., 1., Some(RollDirection::Right)),
        (1., 0., Some(RollDirection::Forward)),
    ] {
        let mut rig = Rig::new(false);
        rig.tick(Input { forward, right, ..Input::default() }, ROLL);
        assert_eq!(rig.state.roll.map(|r| r.direction), expected);
        assert_eq!(rig.state.energy < rig.tuning.energy_capacity, expected.is_some());
    }
}

#[test]
fn a_jump_with_a_bot_jet_vector_leaves_along_its_flat_direction_and_drops_the_old_speed() {
    let take_off = (2.0 * GRAVITY * 2.0f64).sqrt();
    let mut rig = Rig::new(false);
    rig.mover().velocity = V3::new(3.0, 0.0, 2.0);
    let vector = V3::new(0.6, 0.9, 0.8);
    rig.tick(Input { jump: true, jet_direction: Some(vector), ..Input::default() }, Actions::default());
    let mut along = V3::new(0.6, 0.01, 0.8);
    along.normalize();
    let speed = 6.0 * rig.tuning.jump_forward_speed_factor;
    let m = rig.mover();
    assert_eq!(m.air, Air::Jump);
    let expected = V3::new(along.x * speed, take_off + along.y * speed - GRAVITY * DT, along.z * speed);
    near(m.velocity.x, expected.x);
    near(m.velocity.z, expected.z);
    assert!((m.velocity.y - expected.y).abs() < 0.05, "{} != {}", m.velocity.y, expected.y);
    // A vector no longer than the threshold leaves the ordinary jump alone.
    let mut short = Rig::new(false);
    short.mover().velocity = V3::new(3.0, 0.0, 2.0);
    short.tick(Input { jump: true, jet_direction: Some(V3::new(0.2, 0.2, 0.1)), ..Input::default() }, Actions::default());
    near(short.mover().velocity.x, 3.0 * short.tuning.jump_strafe_speed_factor);
    near(short.mover().velocity.z, 2.0 * short.tuning.jump_forward_speed_factor);
}

#[test]
fn jump_scales_existing_strafe_and_forward_velocity_independently() {
    for x in [-3.0, 3.0] {
        let mut rig = Rig::new(false);
        rig.mover().velocity = V3::new(x, 0.0, 2.0);
        rig.tick(Input { jump: true, ..Input::default() }, Actions::default());
        assert_eq!(rig.mover().air, Air::Jump);
        near(rig.mover().velocity.x, x * rig.tuning.jump_strafe_speed_factor);
        near(rig.mover().velocity.z, 2.0 * rig.tuning.jump_forward_speed_factor);
    }
}

#[test]
fn two_movers_sharing_collision_scratch_are_independent_of_tick_order() {
    let make_pair = || {
        let mut rig = Rig::new(true);
        rig.mover().velocity.z = 2.;
        let world = rig.mover().world;
        let second = rig.movement.new_mover(world, |m| {
            m.position.x = -12.0; m.settled = m.position; m.grounded = true;
        });
        (rig, second)
    };
    let (mut a, second) = make_pair();
    let (mut b, _) = make_pair();
    let mut a_states = [a.state; 2];
    let mut b_states = a_states;
    for tick in 0..75 {
        let advance = |rig: &mut Rig, states: &mut [State; 2], order: [u32; 2]| {
            for id in order {
                let input = Input { forward: f64::from(id == second || tick == 0), ..Input::default() };
                let actions = Actions { roll: id == 0 && tick == 0, sprint: id == second, ai: false };
                rig.movement.advance_profiled(&mut rig.collision, id, &input, DT, Some(Step { override_: None,
                    actions, tuning: &rig.tuning, state: &mut states[id as usize],
                })).unwrap();
            }
        };
        advance(&mut a, &mut a_states, [0, second]);
        advance(&mut b, &mut b_states, [second, 0]);
        for id in [0, second] {
            let (mut x, mut y) = ([0.0; movement::IO_LEN], [0.0; movement::IO_LEN]);
            movement::store(a.movement.mover(id), &mut x);
            movement::store(b.movement.mover(id), &mut y);
            assert_eq!(x.map(f64::to_bits), y.map(f64::to_bits));
            let (left, right) = (a_states[id as usize], b_states[id as usize]);
            assert_eq!(left.energy.to_bits(), right.energy.to_bits());
            assert_eq!(left.sprint_time.map(f64::to_bits), right.sprint_time.map(f64::to_bits));
            assert_eq!(left.roll.map(|r| r.elapsed.to_bits()), right.roll.map(|r| r.elapsed.to_bits()));
        }
    }
    assert_eq!(a.movement.mover(0).posture, Posture::Stand);
    assert_eq!(a.movement.mover(second).posture, Posture::Stand);
    assert!(a_states[1].sprint_time.is_some());
}

#[test]
fn roll_ends_strictly_after_the_selected_clip_window() {
    for frames in [6, 11, 31] {
        let mut rig = Rig::new(false);
        rig.tuning.roll_frames = frames;
        let limit = 0.75 * f64::from(frames - 1) / 30.;
        let input = Input { forward: 1., ..Input::default() };
        assert_eq!(rig.try_tick(input, ROLL, limit), Ok(false));
        assert_eq!(rig.state.roll.unwrap().elapsed, limit);
        assert_eq!(rig.try_tick(input, Actions::default(), DT), Ok(false));
        assert!(rig.state.roll.is_none());
        assert!(rig.tick(input, Actions::default()));
    }
    let mut rig = Rig::new(false);
    let forward = Input { forward: 1., ..Input::default() };
    rig.tuning.roll_frames = 0;
    assert!(rig.tick(forward, ROLL));
    assert_eq!(rig.state.energy, rig.tuning.energy_capacity);
    rig.tuning.roll_frames = 1;
    assert_eq!(rig.tuning.roll_seconds(), 0.);
    assert!(!rig.tick(forward, ROLL));
    assert!(rig.state.roll.is_none());
    assert_eq!(rig.state.energy, rig.tuning.energy_capacity - rig.tuning.energy_cost_roll);
}

#[test]
fn nonfinite_body_and_overflowing_takeoff_reject_the_step() {
    for corrupt in [
        (|m: &mut Mover| m.aim_pitch = f64::NAN) as fn(&mut Mover),
        |m| m.position.y = f64::INFINITY, |m| m.velocity.x = f64::NAN,
        |m| m.settled.z = f64::NAN, |m| m.normal.x = f64::NAN,
        |m| m.avoidance.z = f64::NAN, |m| m.state_time = f64::NAN,
        |m| m.lock = f64::INFINITY, |m| m.fuel = f64::NAN,
        |m| m.ground_adjustment = f64::NAN, |m| m.dive = f64::NAN,
    ] {
        let mut rig = airborne_jet(true, 0.0, false);
        rig.mover().air = Air::Jet;
        corrupt(rig.mover());
        let energy = rig.state.energy;
        assert_eq!(rig.try_tick(Input::default(), Actions::default(), DT), Err(ProfileError::InvalidState));
        assert_eq!(rig.state.energy, energy);
    }
    let mut rig = Rig::new(false);
    rig.tuning.jump_height = f64::MAX;
    assert_eq!(rig.tuning.validate(), Err(ProfileError::InvalidTuning));
    assert_eq!(rig.try_tick(Input { jump: true, ..Input::default() }, Actions::default(), DT), Err(ProfileError::InvalidTuning));
    assert_eq!(rig.mover().velocity, V3::ZERO);
}

#[test]
fn zero_time_preserves_expired_roll_and_rejects_incompatible_active_roll() {
    let mut rig = Rig::new(true);
    let origin = V3::new(0., 0., 2.);
    rig.mover().position = origin;
    rig.mover().velocity.z = 4.;
    rig.mover().state_time = 2.;
    let roll = Roll { direction: RollDirection::Forward, elapsed: 3. };
    rig.state.roll = Some(roll);
    assert_eq!(rig.try_tick(Input::default(), ROLL, 0.), Ok(true));
    assert!(rig.state.roll.is_some());
    assert_eq!(rig.mover().position, origin);
    assert_eq!(rig.mover().velocity.z, 4.);
    assert_eq!(rig.mover().state_time, 2.);
    assert_eq!(rig.mover().posture, Posture::Stand);
    rig.state.roll = Some(Roll { elapsed: 0.2, ..roll });
    for dt in [0., DT] {
        for locked in [false, true] {
            rig.mover().grounded = locked;
            rig.mover().air = if locked { Air::None } else { Air::Fall };
            rig.mover().lock = if locked { 1. } else { 0. };
            assert_eq!(rig.try_tick(Input::default(), ROLL, dt), Err(ProfileError::InvalidState));
        }
    }
}

#[test]
fn smallest_positive_steps_preserve_roll_window() {
    for dt in [f64::MIN_POSITIVE, f64::from_bits(1)] {
        let mut rig = Rig::new(false);
        let input = Input { forward: 1., ..Input::default() };
        assert_eq!(rig.try_tick(input, ROLL, dt), Ok(false));
        assert!(rig.state.roll.is_some());
        assert_eq!(rig.state.energy, rig.tuning.energy_capacity - rig.tuning.energy_cost_roll);
    }
}
#[test]
fn last_fuel_tick_flies_then_integrates_a_fall() {
    let input = Input { forward: 0.6, right: 0.3, turn: 0.7, ..Input::default() };
    for hover in [true, false] {
        let flight = |fuel| {
            let mut rig = airborne_jet(hover, 0.0, false);
            rig.mover().air = Air::Jet;
            rig.mover().velocity = V3::new(0., 2., 5.);
            rig.mover().fuel = fuel;
            rig
        };
        let mut last = flight(0.42 * DT);
        let mut full = flight(0.9);
        last.tick(input, Actions::default());
        full.tick(input, Actions::default());
        assert_eq!(last.mover().fuel, 0.);
        assert_eq!(last.mover().air, Air::Jet);
        assert_eq!(last.mover().velocity, full.mover().velocity);
        assert_eq!(last.mover().position, full.mover().position);
        assert_eq!(last.mover().aim_yaw, full.mover().aim_yaw);
        for mut rig in [last, flight(0.001)] {
            let before = rig.mover().position;
            let vel = rig.mover().velocity;
            let yaw = rig.mover().aim_yaw;
            rig.tick(input, Actions::default());
            assert_eq!(rig.mover().air, Air::Fall);
            let drag = v8math::exp(-SWBF2_AIR_DRAG * vel.length() * DT);
            near(rig.mover().velocity.y, vel.y * drag - GRAVITY * DT);
            assert_ne!(rig.mover().position, before);
            assert_ne!(rig.mover().aim_yaw, yaw);
        }
    }
}


#[test]
fn sprint_start_and_keeping_have_distinct_steering_and_speed_thresholds() {
    let sprint = Actions { sprint: true, roll: false, ai: false };
    for (active, forward, right, speed, eligible) in [
        (false, 1., 0., 5.4, false), (false, 1., 0., 5.401, true),
        (false, 0.89, 0., 6., false), (false, 0.9, 0., 6., true),
        (false, 1., 0.577, 6., false), (false, 1., -0.576, 6., true),
        (true, 0.5, 0.865, 1., true), (true, 0.5, 0.866, 1., false),
        (true, 1., 0., 0.2, false), (true, 1., 0., 0.201, true),
        (true, -1., 0., 6., false),
    ] {
        let mut rig = Rig::new(false);
        rig.state.sprint_time = active.then_some(0.);
        // Face along X to exercise the heading projection.
        rig.mover().aim_yaw = std::f64::consts::FRAC_PI_2;
        rig.mover().velocity.x = speed;
        let input = Input { forward, right, ..Input::default() };
        assert_eq!(!rig.tick(input, sprint), eligible, "{active} {forward} {right} {speed}");
    }
    let mut rig = Rig::new(false);
    rig.tuning.sprint.forward = rig.tuning.stand.forward;
    rig.mover().velocity.z = 6.;
    assert!(rig.tick(Input { forward: 1., ..Input::default() }, sprint));
}

#[test]
fn roll_consumes_a_simultaneous_action_even_if_entry_fails() {
    for posture in [Posture::Stand, Posture::Crouch] {
        for (exhausted, frames, enters) in [(false, 15, true), (true, 15, false), (false, 0, false)] {
            let mut rig = Rig::new(false);
            rig.mover().posture = posture;
            rig.mover().velocity.z = 6.;
            rig.state.exhausted = exhausted;
            rig.state.energy = 1.;
            rig.tuning.energy_floor = -2.;
            rig.tuning.roll_frames = frames;
            let input = Input { forward: 1., crouch: true, jump: true, ..Input::default() };
            rig.tick(input, Actions { roll: true, sprint: true, ai: false });
            assert_eq!(rig.state.roll.is_some(), enters);
            assert_eq!(rig.mover().posture, if enters { Posture::Crouch } else { posture });
            assert_eq!(rig.mover().air, Air::None);
            assert!(rig.state.sprint_time.is_none());
            if enters {
                assert_eq!(rig.state.energy, -2.);
                assert!(rig.state.exhausted);
            } else { assert!(rig.state.energy >= 1.); }
        }
    }
    let mut rig = Rig::new(false);
    rig.state.sprint_time = Some(DT);
    rig.mover().velocity.z = 9.;
    rig.tick(Input { forward: 1., ..Input::default() }, Actions { roll: true, sprint: true, ai: false });
    assert!(rig.state.roll.is_some());
    assert!(rig.state.sprint_time.is_none());
}

#[test]
fn jump_carries_vertical_and_backward_momentum_and_charges_for_forward_speed() {
    for (forward, cost, factor) in [(-3., 2., 0.75), (6.0001, 2., 1.25), (6.001, 8., 1.25)] {
        let mut rig = Rig::new(false);
        rig.tuning.energy_cost_jump_sprint = Some(8.);
        rig.mover().velocity = V3::new(2., 1.5, forward);
        let start = rig.state.energy;
        assert!(rig.tick(Input { jump: true, ..Input::default() }, Actions::default()));
        near(rig.mover().velocity.y, 1.5 + (2. * 18. * rig.tuning.jump_height).sqrt() - 18. * DT);
        near(rig.mover().velocity.z, forward * factor);
        near(rig.mover().velocity.x, 2. * rig.tuning.jump_strafe_speed_factor);
        near(rig.state.energy, start - cost);
        let vel = rig.mover().velocity;
        let drag = v8math::exp(-SWBF2_AIR_DRAG * vel.length() * DT);
        rig.tick(Input::default(), Actions::default());
        near(rig.mover().velocity.y, vel.y * drag - 18. * DT);
        near(rig.state.energy, start - cost);
    }
    let mut rig = Rig::new(true);
    rig.mover().position.z = 3.;
    rig.mover().settled.z = 3.;
    rig.mover().posture = Posture::Crouch;
    rig.tick(Input { jump: true, ..Input::default() }, Actions::default());
    assert_eq!(rig.mover().air, Air::None);
    assert_eq!(rig.mover().posture, Posture::Crouch);
    assert_eq!(rig.state.energy, rig.tuning.energy_capacity);
}

#[test]
fn rejected_input_and_integration_overflow_leave_finite_state_untouched() {
    let mut rig = Rig::new(false);
    for input in [Input { forward: f64::NAN, ..Input::default() },
        Input { turn: f64::INFINITY, ..Input::default() },
        Input { jet_direction: Some(V3::new(0., f64::NAN, 1.)), ..Input::default() }] {
        assert_eq!(rig.try_tick(input, ROLL, DT), Err(ProfileError::InvalidState));
    }
    rig.mover().velocity = V3::new(2., 1., f64::MAX);
    rig.tuning.jump_forward_speed_factor = 2.;
    assert_eq!(rig.try_tick(Input { jump: true, ..Input::default() }, Actions::default(), DT), Err(ProfileError::InvalidState));
    assert_eq!(rig.mover().position, V3::ZERO);
    assert_eq!(rig.mover().velocity.z, f64::MAX);
    assert_eq!(rig.state.energy, rig.tuning.energy_capacity);
}

#[test]
fn zero_time_store_and_prediction_preserve_body_history_and_unallocated_handle() {
    use crate::gen::{MoveInput, Soldier, UnitBody};
    use crate::prediction::actors::Session;
    use crate::units::soldier::tick_soldier_in;
    let (world, mut collision) = ground(false);
    let mut owner = MovementOwner::new();
    owner.select_family(GameFamily::Swbf2).unwrap();
    let mut body = Soldier { mover: -1., position: [0., 0., 2.], previous: [2., 3., 4.],
        look_x: 0.5, tick_length: 0.25, grounded: true, ..Soldier::default() };
    let mut actor = Session::default();
    actor.configure_for_family(&[0.], GameFamily::Swbf2).unwrap();
    actor.unit.soldier = UnitBody::Soldier(body.clone());
    let tuning = tuning();
    let mut life = State::new(&tuning);
    life.roll = Some(Roll { direction: RollDirection::Right, elapsed: 99. });
    let step = Step { override_: None, actions: ROLL, tuning: &tuning, state: &mut life };
    assert_eq!(tick_soldier_in(&mut owner, &mut collision, &mut body, &MoveInput::default(), 0.,
        world as usize, -1, Some(step)), Ok(true));
    let step = Step { override_: None, actions: ROLL, tuning: &tuning, state: &mut life };
    assert_eq!(actor.step(&mut collision, world as usize, -1, &[], 0., (0., V3::ZERO, false),
        true, false, false, false, false, false, 0., Some(step)), Ok(true));
    let UnitBody::Soldier(predicted) = &actor.unit.soldier else { unreachable!() };
    for s in [&body, predicted] {
        assert_eq!(s.mover, -1.);
        assert_eq!(s.previous, [2., 3., 4.]);
        assert_eq!((s.look_x, s.tick_length), (0.5, 0.25));
    }
    for input in [MoveInput { turn: f64::NAN, ..MoveInput::default() },
        MoveInput { jet_direction: Some([f64::NAN, 0., 0.]), ..MoveInput::default() }] {
        let step = Step { override_: None, actions: ROLL, tuning: &tuning, state: &mut life };
        assert_eq!(tick_soldier_in(&mut owner, &mut collision, &mut body, &input, 0.,
            world as usize, -1, Some(step)), Err(ProfileError::InvalidState));
    }
    assert_eq!(life.roll.unwrap().elapsed, 99.);
}

#[test]
fn ground_posture_selects_one_action_per_press() {
    for posture in [Posture::Stand, Posture::Crouch] {
        for roof in [false, true] {
            for exhausted in [false, true] {
                for crouch in [false, true] {
                    for jump in [false, true] {
                        let mut rig = Rig::new(roof);
                        rig.mover().position.z = 3.;
                        rig.mover().settled.z = 3.;
                        rig.mover().posture = posture;
                        rig.state.energy = 4.;
                        rig.state.exhausted = exhausted;
                        rig.tuning.energy_restore_idle = 0.;
                        rig.tick(Input { crouch, jump, ..Input::default() }, Actions::default());
                        let launches = posture == Posture::Stand && !crouch && jump && !exhausted;
                        let expected = match posture {
                            Posture::Stand if crouch => Posture::Crouch,
                            Posture::Crouch if (jump || crouch) && !roof => Posture::Stand,
                            _ => posture,
                        };
                        assert_eq!(rig.mover().posture, expected, "{posture:?} {roof} {exhausted} {crouch} {jump}");
                        assert_eq!(rig.mover().air == Air::Jump, launches);
                        assert_eq!(rig.state.energy, if launches { 4. - rig.tuning.energy_cost_jump } else { 4. });
                    }
                }
            }
        }
    }
    // A free jump is eligible while exhausted, but standing up still takes its own press.
    let mut rig = Rig::new(false);
    rig.state.exhausted = true;
    rig.state.energy = 0.;
    rig.tuning.energy_cost_jump = 0.;
    rig.mover().posture = Posture::Crouch;
    let jump = Input { jump: true, ..Input::default() };
    rig.tick(jump, Actions::default());
    assert_eq!((rig.mover().posture, rig.mover().air), (Posture::Stand, Air::None));
    rig.tick(jump, Actions::default());
    assert_eq!(rig.mover().air, Air::Jump);
}

#[test]
fn sprint_ignores_crouch_and_accepts_jump() {
    for crouch in [false, true] {
        for jump in [false, true] {
            let mut rig = Rig::new(false);
            rig.mover().velocity.z = 8.;
            rig.state.sprint_time = Some(DT);
            let can_fire = rig.tick(Input { forward: 1., crouch, jump, ..Input::default() },
                Actions { sprint: true, ..Actions::default() });
            assert_eq!(rig.mover().posture, Posture::Stand);
            assert_eq!(rig.mover().air == Air::Jump, jump);
            assert_eq!(rig.state.sprint_time.is_some(), !jump);
            assert_eq!(can_fire, jump);
        }
    }
}

mod combat;
mod replay;
mod trials;

#[test]
fn a_zero_recovery_rate_cannot_rearm_an_exhausted_soldier() {
    let mut rig = Rig::new(false);
    rig.tuning.energy_restore_idle = 0.;
    rig.tuning.energy_min_sprint = 0.;
    rig.state.energy = 0.;
    rig.state.exhausted = true;
    rig.tick(Input::default(), Actions::default());
    assert!(rig.state.exhausted);
    assert_eq!(rig.state.energy, 0.);
}

#[test]
fn sprint_overdraw_ends_the_ramp_but_completes_the_paid_step() {
    let mut rig = Rig::new(false);
    let input = Input { forward: 1., ..Input::default() };
    let held = Actions { sprint: true, ..Actions::default() };
    rig.state.energy = 0.;
    rig.mover().velocity.z = 6.;
    assert!(!rig.tick(input, held));
    assert!(rig.mover().position.z > 0.);
    assert!(rig.mover().velocity.z > 6.);
    assert!(rig.state.exhausted);
    assert!(rig.state.sprint_time.is_none());
    assert_eq!(rig.try_tick(input, held, 0.), Ok(true));
}

#[test]
fn a_sprint_leaving_ground_has_no_ramp_in_its_checkpoint() {
    let mut rig = Rig::on(terrain(0.5, 0., None));
    let input = Input { forward: 1., ..Default::default() };
    let held = Actions { sprint: true, ..Default::default() };
    rig.mover().velocity.z = 7.;
    assert!(!rig.tick(input, held));
    for _ in 0..30 {
        if !rig.mover().grounded { break; }
        assert!(!rig.tick(input, held));
    }
    assert!(!rig.mover().grounded);
    assert!(rig.state.sprint_time.is_none());
    assert_eq!(rig.try_tick(input, held, 0.), Ok(true));
    rig.state.sprint_time = Some(DT);
    assert_eq!(rig.try_tick(input, held, 0.), Err(ProfileError::InvalidState));
}

#[test]
fn failed_roll_consumes_the_press_and_keeps_an_existing_sprint() {
    for unavailable_clip in [false, true] {
        for continuing in [false, true] {
            let mut rig = Rig::new(false);
            rig.mover().velocity.z = 7.;
            if unavailable_clip { rig.tuning.roll_frames = 0; } else { rig.tuning.roll_speed_factor = 0.; }
            rig.state.sprint_time = continuing.then_some(DT);
            let input = Input { right: 1., jump: true, crouch: true, ..Input::default() };
            // The ordinary sprint-stop test would fail on both stick direction and release.
            assert_eq!(rig.tick(input, ROLL), !continuing);
            assert_eq!((rig.mover().posture, rig.mover().air), (Posture::Stand, Air::None));
            assert!(rig.state.roll.is_none());
            assert_eq!(rig.state.sprint_time, continuing.then_some(2. * DT));
        }
    }
}

#[test]
fn roll_publishes_one_posture_and_rejects_unplayable_checkpoints() {
    for start in [Posture::Stand, Posture::Crouch] {
        let mut rig = Rig::new(false);
        rig.mover().posture = start;
        assert!(!rig.tick(Input { right: 1., ..Input::default() }, ROLL));
        assert_eq!(rig.mover().posture, Posture::Crouch);
        assert_eq!(rig.state.hit_posture(start), rig.mover().posture);
        assert!(!rig.tick(Input::default(), Actions::default()));
        assert_eq!(rig.mover().posture, Posture::Crouch);
        rig.mover().posture = Posture::Stand;
        assert_eq!(rig.try_tick(Input::default(), ROLL, 0.), Err(ProfileError::InvalidState));
    }
    for dt in [-0., DT] {
        let mut rig = Rig::new(false);
        rig.mover().posture = Posture::Prone;
        assert_eq!(rig.try_tick(Input::default(), Actions::default(), dt), Err(ProfileError::InvalidState));
        assert_eq!(rig.mover().posture, Posture::Prone);
    }
}

#[test]
fn landing_locks_only_postures_on_soft_touchdown_and_hard_locks_after_long_airtime() {
    // Soft jump landing (<= 1.5s): posture cooldown blocks jump/crouch/sprint for 0.3s,
    // while horizontal movement and firing stay free.
    let mut soft = Rig::new(false);
    soft.mover().air = Air::Jump;
    soft.mover().grounded = false;
    soft.mover().state_time = 1.2;
    soft.mover().position.y = 0.05;
    soft.mover().settled.y = 0.05;
    soft.mover().velocity = V3::new(0.0, -6.0, 0.0);
    assert!(soft.tick(Input::default(), Actions::default()));
    assert_eq!(soft.mover().air, Air::None);
    assert_eq!(soft.mover().lock, 0.0);
    near(soft.state.land_cooldown, LAND_LOCK);
    let can_fire = soft.tick(
        Input { forward: 1.0, jump: true, crouch: true, ..Input::default() },
        Actions { sprint: true, roll: false, ai: false },
    );
    assert!(can_fire);
    assert_eq!(soft.mover().air, Air::None);
    assert_eq!(soft.mover().posture, Posture::Stand);
    assert!(soft.state.sprint_time.is_none());
    assert!(soft.mover().velocity.z > 0.0);

    // After 0.3s cooldown expires, jumping resumes cleanly.
    for _ in 0..18 {
        soft.tick(Input::default(), Actions::default());
    }
    assert_eq!(soft.state.land_cooldown, 0.0);
    soft.tick(Input { jump: true, ..Input::default() }, Actions::default());
    assert_eq!(soft.mover().air, Air::Jump);

    // Long fall (> 1.5s): sets 0.5s full lock, blocking horizontal movement and firing.
    let mut hard = Rig::new(false);
    hard.mover().air = Air::Fall;
    hard.mover().grounded = false;
    hard.mover().state_time = 1.6;
    hard.mover().position.y = 0.05;
    hard.mover().settled.y = 0.05;
    hard.mover().velocity = V3::new(0.0, -8.0, 0.0);
    assert!(!hard.tick(Input::default(), Actions::default()));
    assert_eq!(hard.mover().air, Air::None);
    near(hard.mover().lock, HARD_LOCK);
    for _ in 0..30 {
        assert!(!hard.tick(Input { forward: 1.0, ..Input::default() }, Actions::default()));
        assert_eq!(hard.mover().velocity.z, 0.0);
    }
    assert_eq!(hard.mover().lock, 0.0);
    assert!(hard.tick(Input { forward: 1.0, ..Input::default() }, Actions::default()));
    assert!(hard.mover().velocity.z > 0.0);

    // Long jet flight (> 2.0s) touching down: always soft landing, never hard lock.
    let mut jet = airborne_jet(true, 0.0, false);
    jet.mover().air = Air::Jet;
    jet.mover().grounded = true;
    jet.mover().position.y = 0.0;
    jet.mover().settled.y = 0.0;
    jet.mover().fuel = 0.5;
    jet.mover().state_time = 2.4;
    jet.tick(Input::default(), Actions::default());
    assert_eq!(jet.mover().air, Air::None);
    assert_eq!(jet.mover().lock, 0.0);
    near(jet.state.land_cooldown, LAND_LOCK);

    // Any short Fall landing (even <= 0.3s) also applies the 0.3s soft cooldown.
    let mut short_fall = Rig::new(false);
    short_fall.mover().air = Air::Fall;
    short_fall.mover().grounded = false;
    short_fall.mover().state_time = 0.15;
    short_fall.mover().position.y = 0.05;
    short_fall.mover().settled.y = 0.05;
    short_fall.mover().velocity = V3::new(0.0, -6.0, 0.0);
    short_fall.tick(Input::default(), Actions::default());
    assert_eq!(short_fall.mover().air, Air::None);
    assert_eq!(short_fall.mover().lock, 0.0);
    near(short_fall.state.land_cooldown, LAND_LOCK);
}

#[test]
fn unsupported_stand_window_holds_for_one_second_and_allows_coyote_jump() {
    // Stepping off a high drop stays in Stand (Air::None) for up to 1.0 s,
    // avoiding rapid land/fall flicker at curbs and roof edges.
    let mut drop_rig = Rig::new(false);
    drop_rig.mover().grounded = false;
    drop_rig.mover().position.y = 40.0;
    drop_rig.mover().settled = drop_rig.mover().position;
    for tick in 0..=60 {
        drop_rig.tick(Input::default(), Actions::default());
        assert_eq!(drop_rig.mover().air, Air::None, "tick {tick} should stay in Stand");
        assert!(drop_rig.state.unsupported_time.is_some());
    }
    // Past 1.0 s unsupported, the soldier transitions cleanly into Fall.
    drop_rig.tick(Input::default(), Actions::default());
    assert_eq!(drop_rig.mover().air, Air::Fall);
    assert!(drop_rig.state.unsupported_time.is_none());

    // Coyote jump succeeds inside the first 0.3 s unsupported and is refused after 0.3 s.
    let mut coyote = Rig::new(false);
    coyote.mover().grounded = false;
    coyote.mover().position.y = 40.0;
    coyote.mover().settled = coyote.mover().position;
    for _ in 0..10 {
        coyote.tick(Input::default(), Actions::default());
    }
    assert_eq!(coyote.mover().air, Air::None);
    coyote.tick(Input { jump: true, ..Input::default() }, Actions::default());
    assert_eq!(coyote.mover().air, Air::Jump);
    assert!(coyote.state.unsupported_time.is_none());

    let mut late = Rig::new(false);
    late.mover().grounded = false;
    late.mover().position.y = 40.0;
    late.mover().settled = late.mover().position;
    for _ in 0..20 {
        late.tick(Input::default(), Actions::default());
    }
    assert_eq!(late.mover().air, Air::None);
    late.tick(Input { jump: true, ..Input::default() }, Actions::default());
    assert_eq!(late.mover().air, Air::None);

    // Crouching off a ledge stands up after 0.3 s and falls after 1.0 s total.
    let mut crouch = Rig::new(false);
    crouch.mover().posture = Posture::Crouch;
    crouch.mover().grounded = false;
    crouch.mover().position.y = 40.0;
    crouch.mover().settled = crouch.mover().position;
    for _ in 0..20 {
        crouch.tick(Input::default(), Actions::default());
    }
    assert_eq!(crouch.mover().posture, Posture::Stand);
    assert_eq!(crouch.mover().air, Air::None);
    for _ in 0..45 {
        crouch.tick(Input::default(), Actions::default());
    }
    assert_eq!(crouch.mover().air, Air::Fall);
}

#[test]
fn running_jump_onto_upward_slope_lands_on_first_contact() {
    let mut rig = Rig::on(terrain(40.0, 0.25, None));
    rig.tick(Input::default(), Actions::default());
    assert!(rig.mover().grounded);
    rig.mover().velocity.z = 5.0;
    let forward = Input { forward: 1.0, ..Input::default() };
    rig.tick(Input { jump: true, ..forward }, Actions::default());
    assert_eq!(rig.mover().air, Air::Jump);

    let mut contact_ticks_in_air = 0;
    let mut landed_at = None;
    for tick in 1..120 {
        rig.tick(forward, Actions::default());
        if rig.mover().air == Air::None {
            landed_at = Some(tick);
            break;
        }
        if rig.mover().grounded {
            contact_ticks_in_air += 1;
        }
    }
    let landed = landed_at.expect("should land on the upward slope");
    assert!(landed < 60, "expected prompt up-slope landing, landed at tick {landed}");
    assert_eq!(contact_ticks_in_air, 0, "must not skate in Jump while grounded on an up-slope");
    assert_eq!(rig.mover().lock, 0.0, "short up-slope jump must be a soft landing");
    assert!(rig.mover().velocity.y > 0.0, "running up-slope keeps positive tangent vy");

    // On a convex dome facet, contact_mean can ground a soldier with a small positive
    // normal velocity (above LEAVING) while running up toward the crest. Once past the
    // 0.3 s takeoff window, any grounded contact must land immediately instead of skating.
    let mut crest = Rig::new(true);
    crest.mover().air = Air::Jump;
    crest.mover().state_time = 0.6;
    crest.mover().grounded = false;
    crest.mover().position = V3::new(0.0, -0.02, 0.0);
    crest.mover().settled = crest.mover().position;
    crest.mover().velocity = V3::new(0.0, 0.55, 4.0);
    crest.tick(forward, Actions::default());
    assert!(crest.mover().grounded, "expected contact_mean support on the facet");
    assert_eq!(crest.mover().air, Air::None, "must land on first grounded contact after takeoff window");
}

#[test]
fn airborne_drag_scales_velocity_exponentially_by_speed() {
    let mut rig = Rig::new(false);
    rig.mover().air = Air::Jump;
    rig.mover().grounded = false;
    rig.mover().position.y = 20.0;
    rig.mover().settled = rig.mover().position;
    let initial = V3::new(3.0, 4.0, 12.0);
    rig.mover().velocity = initial;
    let speed = initial.length();
    let expected_scale = v8math::exp(-SWBF2_AIR_DRAG * speed * DT);
    rig.tick(Input::default(), Actions::default());
    near(rig.mover().velocity.x, initial.x * expected_scale);
    near(rig.mover().velocity.z, initial.z * expected_scale);
    near(rig.mover().velocity.y, initial.y * expected_scale - GRAVITY * DT);
}

#[test]
fn ground_axes_change_speed_by_the_class_acceleration_each_way() {
    let mut rig = Rig::new(false);
    rig.mover().accel = 42.0;
    rig.mover().max_speed = 5.0;
    rig.mover().max_strafe = 3.0;
    let step = 42.0 * DT;
    let ahead = Input { forward: 1.0, ..Input::default() };
    for tick in 1..=10 {
        rig.tick(ahead, Actions::default());
        near(rig.mover().velocity.z, js_min(step * f64::from(tick), 5.0));
    }
    // Sideways at the same rate while the forward axis comes down to its share.
    let both = Input { forward: 1.0, right: -1.0, ..Input::default() };
    rig.tick(both, Actions::default());
    near(rig.mover().velocity.z, 5.0 - step);
    near(rig.mover().velocity.x, step);
    for _ in 0..20 {
        rig.tick(both, Actions::default());
    }
    near(rig.mover().velocity.z, 5.0 * std::f64::consts::FRAC_1_SQRT_2);
    near(rig.mover().velocity.x, 3.0 * std::f64::consts::FRAC_1_SQRT_2);
    // Released, each axis stops at the same rate.
    let before = rig.mover().velocity;
    rig.tick(Input::default(), Actions::default());
    near(rig.mover().velocity.z, before.z - step);
    near(rig.mover().velocity.x, before.x - step);
    // Held still by a landing lock, 1.7 times as fast.
    for _ in 0..30 {
        rig.tick(ahead, Actions::default());
    }
    rig.mover().lock = 1.0;
    rig.tick(ahead, Actions::default());
    near(rig.mover().velocity.z, 5.0 - 1.7 * step);
}

#[test]
fn a_climbing_target_is_shortened_by_the_square_of_its_climb() {
    let speed_on = |forward: f64| {
        let mut rig = Rig::on(terrain(40.0, 0.5, None));
        rig.mover().max_speed = 4.0;
        rig.mover().accel = 30.0;
        rig.mover().position = V3::new(0.0, 0.0, 0.0);
        rig.mover().settled = rig.mover().position;
        for _ in 0..45 {
            rig.tick(Input { forward, ..Input::default() }, Actions::default());
        }
        let v = rig.mover().velocity;
        (v.length(), rig.mover().grounded)
    };
    // The floor rises 0.5 m per metre along +Z: its climb share is 0.5 / sqrt(1.25).
    let climb_sq = 0.25 / 1.25;
    let (up, grounded) = speed_on(1.0);
    assert!(grounded);
    assert!((up - 4.0 * (1.0 - climb_sq)).abs() < 0.02, "uphill {up}");
}

#[test]
fn standing_backward_runs_at_strafe_speed_and_jumps_from_it() {
    let mut rig = Rig::new(false);
    rig.mover().max_speed = 7.5;
    rig.mover().max_strafe = 4.5;
    rig.tuning.stand = ControlSpeed { forward: 0.8, strafe: 1.0, turn: 1.0 };
    let back = Input { forward: -1.0, ..Input::default() };
    for _ in 0..90 {
        rig.tick(back, Actions::default());
    }
    near(rig.mover().velocity.z, -4.5 * 0.8);
    // Forward keeps MaxSpeed.
    let ahead = Input { forward: 1.0, ..Input::default() };
    for _ in 0..90 {
        rig.tick(ahead, Actions::default());
    }
    near(rig.mover().velocity.z, 7.5 * 0.8);
    for _ in 0..90 {
        rig.tick(back, Actions::default());
    }
    rig.tick(Input { jump: true, ..back }, Actions::default());
    assert_eq!(rig.mover().air, Air::Jump);
    near(rig.mover().velocity.z, -4.5 * 0.8 * rig.tuning.jump_strafe_speed_factor);
}

#[test]
fn an_airborne_body_on_a_surface_stops_moving_into_it() {
    let mut rig = Rig::on(terrain(40.0, 1.4, None));
    rig.tuning.jump = ControlSpeed { forward: 0.0, strafe: 0.0, turn: 0.4 };
    let n = V3::new(0.0, 1.0, -1.4);
    let len = n.length();
    let normal = V3::new(0.0, 1.0 / len, -1.4 / len);
    rig.mover().air = Air::Fall;
    rig.mover().state_time = 0.1;
    rig.mover().grounded = true;
    rig.mover().normal = normal;
    rig.mover().position = V3::new(0.0, 7.0, 5.0);
    rig.mover().settled = rig.mover().position;
    rig.mover().velocity = V3::new(1.5, -6.0, 2.0);
    let before = rig.mover().velocity;
    rig.tick(Input::default(), Actions::default());
    let into = before.dot(normal);
    assert!(into < 0.0);
    let mut expected = before;
    expected.add_scaled(normal, -into);
    let v = rig.mover().velocity;
    assert!((v.x - expected.x).abs() < 1e-9, "{v:?} {expected:?}");
    assert!(v.dot(normal) > -1e-9, "{v:?}");
}

#[test]
fn airborne_stick_accelerates_by_the_jump_row_without_a_speed_target() {
    let mut rig = Rig::new(false);
    rig.tuning.jump = ControlSpeed { forward: 0.3, strafe: 0.2, turn: 0.4 };
    rig.mover().accel = 50.0;
    rig.mover().air = Air::Fall;
    rig.mover().grounded = false;
    rig.mover().position.y = 30.0;
    rig.mover().settled = rig.mover().position;
    let initial = V3::new(0.0, 2.0, 8.0);
    rig.mover().velocity = initial;
    let held = Input { forward: 1.0, right: 1.0, ..Input::default() };
    rig.tick(held, Actions::default());
    let scale = v8math::exp(-SWBF2_AIR_DRAG * initial.length() * DT);
    let along = 0.3 * std::f64::consts::FRAC_1_SQRT_2 * 50.0 * DT;
    let across = 0.2 * std::f64::consts::FRAC_1_SQRT_2 * 50.0 * DT;
    // Facing +Z at yaw 0, the stick's right is -X.
    near(rig.mover().velocity.z, initial.z * scale + along);
    near(rig.mover().velocity.x, -across);
    near(rig.mover().velocity.y, initial.y * scale - GRAVITY * DT);
    // Well above MaxSpeed x the jump row, a held stick still adds speed.
    for _ in 0..20 {
        rig.tick(held, Actions::default());
    }
    assert!(rig.mover().velocity.z > initial.z + 1.0, "{}", rig.mover().velocity.z);
    // A locked body is not steered.
    let before = rig.mover().velocity;
    rig.mover().lock = 1.0;
    rig.tick(held, Actions::default());
    let speed = before.length();
    let scale = v8math::exp(-SWBF2_AIR_DRAG * speed * DT);
    near(rig.mover().velocity.z, before.z * scale);
}

#[test]
fn thrown_soldier_drags_in_air_tumbles_slides_gets_up_without_yaw_spin() {
    let mut rig = Rig::new(false);
    rig.mover().aim_yaw = 0.75;
    rig.mover().grounded = false;
    rig.mover().air = Air::Thrown;
    rig.mover().position = V3::new(0.0, 0.5, 0.0);
    rig.mover().settled = rig.mover().position;
    // Strong backward shove like a Force push.
    rig.mover().velocity = V3::new(-10.0, 7.0, -10.0);
    rig.state.tumble = Some(Tumble::enter(rig.mover().velocity, rig.mover().aim_yaw, 42));
    let initial_speed_xz = v8math::hypot(rig.mover().velocity.x, rig.mover().velocity.z);
    let mut phases = Vec::new();
    let mut air_speed_dropped = false;
    let mut stood_tick = None;
    let hostile_input = Input {
        forward: 1.0,
        right: 1.0,
        turn: 1.0,
        pitch: 1.0,
        crouch: true,
        ..Input::default()
    };
    for tick in 0..360 {
        let can_fire = rig.tick(hostile_input, Actions { sprint: true, roll: true, ai: false });
        if let Some(t) = rig.state.tumble {
            assert!(!can_fire, "weapon fire blocked during tumble phase {:?}", t.phase);
            if phases.last().copied() != Some(t.phase) {
                phases.push(t.phase);
            }
            if t.phase == TumblePhase::Thrown && tick == 10 {
                let speed_xz = v8math::hypot(rig.mover().velocity.x, rig.mover().velocity.z);
                assert!(speed_xz < initial_speed_xz - 0.1, "airborne drag slows horizontal speed: {speed_xz} vs {initial_speed_xz}");
                air_speed_dropped = true;
            }
            // World yaw must NEVER spin while thrown, tumbling, sliding, or getting up.
            assert_eq!(rig.mover().aim_yaw, 0.75, "aim_yaw stayed fixed at tick {tick} phase {:?}", t.phase);
        } else {
            stood_tick = Some(tick);
            break;
        }
    }
    assert!(air_speed_dropped);
    assert_eq!(phases, vec![TumblePhase::Thrown, TumblePhase::Tumble, TumblePhase::Landed, TumblePhase::GetUp]);
    assert!(stood_tick.is_some(), "soldier completed full getup cycle");
    assert_eq!(rig.mover().posture, Posture::Stand);
    assert_eq!(rig.mover().air, Air::None);
    assert!(rig.mover().grounded);
}

#[test]
fn recover_from_tumble_flips_upright_automatically_after_delay() {
    let mut rig = Rig::new(false);
    rig.tuning.recover_from_tumble = true;
    rig.mover().grounded = false;
    rig.mover().air = Air::Thrown;
    rig.mover().position = V3::new(0.0, 12.0, 0.0);
    rig.mover().settled = rig.mover().position;
    rig.mover().velocity = V3::new(0.0, 4.0, 3.0);
    rig.state.tumble = Some(Tumble::enter(rig.mover().velocity, 0.0, 7));
    // Before 0.5 s (30 ticks), stays in Thrown even with zero input.
    for _ in 0..29 {
        rig.tick(Input::default(), Actions::default());
        assert_eq!(rig.state.tumble.unwrap().phase, TumblePhase::Thrown);
    }
    // After 0.5 s airborne while alive, Recover triggers automatically with NO jump press,
    // immediately clearing somersault angle, and exits back to Air::Fall after 0.5 s.
    let mut saw_recover = false;
    for _ in 0..50 {
        rig.tick(Input::default(), Actions::default());
        match rig.state.tumble {
            Some(t) if t.phase == TumblePhase::Recover => {
                assert_eq!(t.angle, 0.0, "Recover zeroes somersault angle immediately");
                saw_recover = true;
            }
            None => break,
            _ => {}
        }
    }
    assert!(saw_recover, "RecoverFromTumble must trigger automatically without jump press");
    assert!(rig.state.tumble.is_none());
    assert_eq!(rig.mover().air, Air::Fall);
    assert_eq!(rig.mover().state_time, 2.0);

    // Dead soldiers and classes without RecoverFromTumble must NOT auto-recover.
    for (recover_flag, dead) in [(false, false), (true, true)] {
        let mut r = Rig::new(false);
        r.tuning.recover_from_tumble = recover_flag;
        r.mover().grounded = false;
        r.mover().air = Air::Thrown;
        r.mover().position = V3::new(0.0, 20.0, 0.0);
        r.mover().settled = r.mover().position;
        r.mover().velocity = V3::new(0.0, 4.0, 2.0);
        let mut t = Tumble::enter(r.mover().velocity, 0.0, 7);
        t.dead = dead;
        r.state.tumble = Some(t);
        for _ in 0..45 {
            r.tick(Input { jump: true, ..Input::default() }, Actions::default());
            assert_eq!(
                r.state.tumble.unwrap().phase,
                TumblePhase::Thrown,
                "recover_flag={recover_flag}, dead={dead} must stay Thrown"
            );
        }
    }
}

#[test]
fn fast_flat_ground_push_enters_tumble_directly_and_getup_locks_look() {
    let mut rig = Rig::new(false);
    rig.tuning.getup_front_seconds = 1.6;
    rig.tuning.getup_back_seconds = 0.8;
    rig.mover().aim_yaw = 0.0;
    rig.mover().grounded = true;
    rig.mover().air = Air::None;
    rig.mover().posture = Posture::Crouch;
    rig.mover().velocity = V3::new(0.0, 0.0, -9.5);
    rig.state.tumble = Some(Tumble::enter_ground(rig.mover().velocity, rig.mover().aim_yaw));
    assert_eq!(rig.state.tumble.unwrap().phase, TumblePhase::Tumble);
    assert!(!rig.state.tumble.unwrap().front);
    assert!(rig.state.is_tumble_look());
    // Thrown/Tumble/Landed keep lock == 0.0 (mouse look allowed at tumble turn scale),
    // whereas GetUp sets mover.lock > 0.0 for the back getup duration and clears it on exit.
    let mut saw_getup = false;
    let mut getup_ticks = 0u32;
    for _ in 0..240 {
        rig.tick(Input::default(), Actions::default());
        match rig.state.tumble.map(|t| t.phase) {
            Some(TumblePhase::Tumble | TumblePhase::Landed) => {
                assert_eq!(rig.mover().lock, 0.0, "lock stays 0 before GetUp");
                assert!(rig.state.is_tumble_look());
            }
            Some(TumblePhase::GetUp) => {
                saw_getup = true;
                getup_ticks += 1;
                assert!(rig.mover().lock > 0.0, "GetUp must lock look & movement");
                assert!(!rig.state.is_tumble_look());
            }
            None => break,
            other => panic!("unexpected phase {other:?}"),
        }
    }
    assert!(saw_getup);
    // Back getup duration is 0.8 s = 48 ticks (entry tick + 47 active ticks).
    assert!((46..=50).contains(&getup_ticks), "back getup ticks={getup_ticks}");
    assert!(rig.state.tumble.is_none());
    assert_eq!(rig.mover().lock, 0.0, "lock cleared when GetUp finishes");
}

#[test]
fn combo_step_cancels_attack_and_never_spins_yaw_while_thrown() {
    use crate::combat::combo::{
        step_soldier_combo_and_move, ComboButton, ComboInput, ComboMachine, ComboState,
        WeaponMeleeSpec,
    };
    let doc = serde_json::json!({
        "nameHash": "0000cafe",
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    {
                        "key": "Transition",
                        "values": ["SWING"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Fire", "Press"] }
                        ]}]
                    }
                ]
            },
            {
                "name": "SWING",
                "children": [
                    { "key": "Duration", "values": [0.5] },
                    { "key": "AlignedToThrust" }
                ]
            }
        ]
    });
    let machine = ComboMachine::from_json_value(&doc, &WeaponMeleeSpec::default(), 30.0, |_| Some(0.5)).unwrap();
    let mut combo_state = ComboState::new(&machine);
    let mut rig = Rig::new(false);
    rig.mover().aim_yaw = 1.1;
    // Enter SWING (which has AlignedToThrust).
    {
        let mut cmd = ComboInput::default().stick(0.0, 1.0);
        cmd.buttons_pressed = ComboButton::Fire.bit();
        cmd.buttons_held = ComboButton::Fire.bit();
        rig.movement
            .with_soldier_scene(&mut rig.collision, rig.id, |mover, scene| {
                step_soldier_combo_and_move(
                    &machine,
                    &mut combo_state,
                    &cmd,
                    mover,
                    &mut rig.state,
                    &rig.tuning,
                    &Input::default(),
                    Actions::default(),
                    scene,
                    DT,
                )
            })
            .unwrap();
    }
    assert_eq!(combo_state.state.0, 1);
    // Now throw the soldier mid-swing; yaw must stay completely frozen and combo must reset to IDLE.
    let yaw_at_throw = rig.mover().aim_yaw;
    {
        let m = rig.mover();
        m.grounded = false;
        m.air = Air::Thrown;
        m.velocity = V3::new(0.0, 5.0, -9.0);
    }
    rig.state.tumble = Some(Tumble::enter(V3::new(0.0, 5.0, -9.0), yaw_at_throw, 99));
    for _ in 0..30 {
        let cmd = ComboInput::default().stick(0.0, 1.0);
        let (out, allowed) = rig
            .movement
            .with_soldier_scene(&mut rig.collision, rig.id, |mover, scene| {
                step_soldier_combo_and_move(
                    &machine,
                    &mut combo_state,
                    &cmd,
                    mover,
                    &mut rig.state,
                    &rig.tuning,
                    &Input { forward: 1.0, right: 1.0, turn: 1.0, ..Input::default() },
                    Actions::default(),
                    scene,
                    DT,
                )
            })
            .unwrap();
        assert!(!allowed);
        assert!(!out.active);
        assert_eq!(combo_state.state.0, 0, "combo stayed at idle while thrown");
        assert_eq!(rig.mover().aim_yaw, yaw_at_throw, "aim_yaw never spun during throw");
    }
}

#[test]
fn standing_unsupported_window_holds_curb_drops_and_jump_grace() {
    // 1. Stepping off a 0.36 m curb onto lower terrain never enters Air::Fall.
    let curb = [-4., 0.36, -4., 4., 0.36, -4., 4., 0.36, 0.5, -4., 0.36, 0.5];
    let mut rig = Rig::on(terrain(40.0, 0.0, Some(curb)));
    rig.mover().position = V3::new(0.0, 0.36, 0.2);
    rig.mover().settled = rig.mover().position;
    let walk = Input { forward: 1.0, ..Input::default() };
    let mut saw_unsupported = false;
    let mut landed_lower = false;
    for _ in 0..60 {
        rig.tick(walk, Actions::default());
        assert_eq!(rig.mover().air, Air::None, "curb drop should stay on foot");
        if !rig.mover().grounded {
            saw_unsupported = true;
        } else if saw_unsupported && rig.mover().position.y < 0.05 {
            landed_lower = true;
            break;
        }
    }
    assert!(saw_unsupported, "soldier must step off the curb");
    assert!(landed_lower, "soldier must land on the lower street");

    // 2. Walking off a cliff stays Stand for 1.0 s and accepts jump only under 0.3 s.
    let walk_off = |ticks_before_jump: usize| -> Air {
        let mut r = Rig::on(terrain(0.4, 0.0, None));
        r.mover().velocity.z = 5.0;
        let mut off = 0usize;
        for _ in 0..100 {
            let was_off = !r.mover().grounded;
            if was_off {
                off += 1;
            }
            let jump = was_off && off == ticks_before_jump;
            r.tick(Input { forward: 1.0, jump, ..Input::default() }, Actions::default());
            if jump {
                return r.mover().air;
            }
        }
        r.mover().air
    };
    assert_eq!(walk_off(10), Air::Jump, "jump inside 0.3 s unsupported grace must launch");
    assert_eq!(walk_off(24), Air::None, "jump after 0.3 s unsupported grace must be ignored while still Stand");
    assert_eq!(walk_off(65), Air::Fall, "falling past 1.0 s unsupported must enter Air::Fall");
}

#[test]
fn thrown_gravity_and_air_drag_match_spec() {
    // Pure vertical drop from rest: drag is zero at v=0, so 1 tick adds exactly -GRAVITY*DT (-0.30 m/s).
    let mut rig = Rig::new(false);
    rig.mover().grounded = false;
    rig.mover().air = Air::Thrown;
    rig.mover().position = V3::new(0.0, 10.0, 0.0);
    rig.mover().settled = rig.mover().position;
    rig.mover().velocity = V3::ZERO;
    rig.state.tumble = Some(Tumble::enter(V3::ZERO, 0.0, 1));
    rig.tick(Input::default(), Actions::default());
    near(rig.mover().velocity.y, -GRAVITY * DT);

    // Pure horizontal speed at y=10: drag exp(-0.012 * |v| * dt) slows 15 m/s measurably in 1 tick.
    let mut drag = Rig::new(false);
    drag.mover().grounded = false;
    drag.mover().air = Air::Thrown;
    drag.mover().position = V3::new(0.0, 10.0, 0.0);
    drag.mover().settled = drag.mover().position;
    drag.mover().velocity = V3::new(15.0, 0.0, 0.0);
    drag.state.tumble = Some(Tumble::enter(drag.mover().velocity, 0.0, 2));
    drag.tick(Input::default(), Actions::default());
    let expected_vx = 15.0 * v8math::exp(-THROWN_AIR_DRAG * 15.0 * DT);
    near(drag.mover().velocity.x, expected_vx);
    assert!(drag.mover().velocity.x < 14.99, "air drag must reduce speed: {}", drag.mover().velocity.x);
}

#[test]
fn thrown_landing_gate_holds_until_030s() {
    let mut rig = Rig::new(false);
    rig.mover().grounded = false;
    rig.mover().air = Air::Thrown;
    rig.mover().position = V3::new(0.0, 0.05, 0.0);
    rig.mover().settled = rig.mover().position;
    // Low hop touches the floor well before 0.30 s, yet must remain Thrown until elapsed > 0.30 s.
    rig.mover().velocity = V3::new(0.0, 1.0, -5.0);
    rig.state.tumble = Some(Tumble::enter(rig.mover().velocity, 0.0, 5));
    for tick in 1..=17 {
        rig.tick(Input::default(), Actions::default());
        assert_eq!(
            rig.state.tumble.unwrap().phase,
            TumblePhase::Thrown,
            "tick {tick} ({:.3}s) must stay Thrown before 0.30s gate",
            tick as f64 * DT
        );
    }
    assert!(rig.mover().grounded, "soldier already touched ground before 0.30s");
    // Once past 0.30 s (ticks 18..=20), grounded soldier transitions out of Thrown.
    let mut left_thrown = false;
    for _ in 0..4 {
        rig.tick(Input::default(), Actions::default());
        if rig.state.tumble.unwrap().phase != TumblePhase::Thrown {
            left_thrown = true;
            break;
        }
    }
    assert!(left_thrown, "must leave Thrown once elapsed > 0.30s and grounded");
}

#[test]
fn thrown_landing_speed_thresholds_and_70_30_split() {
    let land_phase = |vxz: f64, seed_i: usize| -> TumblePhase {
        let mut rig = Rig::new(false);
        rig.mover().grounded = true;
        rig.mover().air = Air::Thrown;
        rig.mover().position = V3::new(seed_i as f64 * 0.13, 0.0, 0.0);
        rig.mover().settled = rig.mover().position;
        rig.mover().velocity = V3::new(vxz, -0.5, 0.0);
        let mut t = Tumble::enter(rig.mover().velocity, 0.0, seed_i as u32 + 1);
        t.elapsed = 0.31;
        t.air_time = 0.31;
        t.angle = (seed_i as f64) * 0.17;
        t.spin_rate = 0.0;
        rig.state.tumble = Some(t);
        rig.tick(Input::default(), Actions::default());
        rig.state.tumble.unwrap().phase
    };
    // 1. vxz = 7.0 m/s (49 > 36, < 64): ALWAYS enters Tumble across all 20 seeds (never Landed).
    for i in 0..20 {
        assert_eq!(
            land_phase(7.0, i),
            TumblePhase::Tumble,
            "seed {i} with vxz=7.0 (v^2=49 > 36) must always enter Tumble"
        );
    }
    // 2. vxz = 4.5 m/s (9 < 20.25 <= 36): ~70% Tumble, ~30% Landed across 40 seeds.
    let tumbles = (0..40).filter(|&i| land_phase(4.5, i) == TumblePhase::Tumble).count();
    assert!(
        (16..=36).contains(&tumbles),
        "expected ~70% tumble split at vxz=4.5 across 40 seeds, got {tumbles}/40"
    );
    // 3. vxz = 2.0 m/s (4 <= 9): NEVER enters Tumble.
    for i in 0..20 {
        assert_eq!(
            land_phase(2.0, i),
            TumblePhase::Landed,
            "seed {i} with vxz=2.0 (v^2=4 <= 9) must enter Landed"
        );
    }
}

#[test]
fn tumble_friction_and_min_duration_hold_before_landed() {
    // 1. Friction at 2.5 s^-1 slows 10 m/s well below drag-only (~9.4 m/s) in 0.25 s.
    let mut fast = Rig::new(false);
    fast.mover().grounded = true;
    fast.mover().posture = Posture::Crouch;
    fast.mover().velocity = V3::new(10.0, 0.0, 0.0);
    fast.state.tumble = Some(Tumble::enter_ground(fast.mover().velocity, 0.0));
    for _ in 0..15 {
        fast.tick(Input::default(), Actions::default());
    }
    let speed = v8math::hypot(fast.mover().velocity.x, fast.mover().velocity.z);
    assert!(speed < 6.0 && speed > 4.0, "tumble friction 2.5 should slow 10 m/s to ~5 m/s in 0.25s, got {speed}");

    // 2. Min tumble duration 0.5 s holds even when speed^2 < 16.
    let mut slow = Rig::new(false);
    slow.mover().grounded = true;
    slow.mover().posture = Posture::Crouch;
    slow.mover().velocity = V3::new(1.0, 0.0, 0.0);
    slow.state.tumble = Some(Tumble::enter_ground(slow.mover().velocity, 0.0));
    for tick in 1..=29 {
        slow.tick(Input::default(), Actions::default());
        assert_eq!(
            slow.state.tumble.unwrap().phase,
            TumblePhase::Tumble,
            "tick {tick} must remain in Tumble until elapsed > 0.5s"
        );
    }
    slow.tick(Input::default(), Actions::default());
    slow.tick(Input::default(), Actions::default());
    assert_eq!(slow.state.tumble.unwrap().phase, TumblePhase::Landed);
}

#[test]
fn landed_friction_and_max_slide_time_enter_getup() {
    // 1. Slide friction 10.0 s^-1 slows 6.0 m/s to ~2.0 m/s in 6 ticks (vs ~4.6 m/s at 2.5).
    let mut slide = Rig::new(false);
    slide.mover().grounded = true;
    slide.mover().posture = Posture::Crouch;
    slide.mover().velocity = V3::new(6.0, 0.0, 0.0);
    let mut t = Tumble::enter_ground(slide.mover().velocity, 0.0);
    t.phase = TumblePhase::Landed;
    slide.state.tumble = Some(t);
    for _ in 0..6 {
        slide.tick(Input::default(), Actions::default());
    }
    let speed = v8math::hypot(slide.mover().velocity.x, slide.mover().velocity.z);
    assert!(speed < 2.5, "slide friction 10.0 must rapidly brake 6 m/s in 0.1s, got {speed}");

    // 2. Slide max time 1.0 s forces transition to GetUp even while speed^2 > 1.0.
    let mut cap = Rig::new(false);
    cap.mover().grounded = true;
    cap.mover().posture = Posture::Crouch;
    cap.mover().velocity = V3::new(4.0, 0.0, 0.0);
    let mut tc = Tumble::enter_ground(cap.mover().velocity, 0.0);
    tc.phase = TumblePhase::Landed;
    tc.elapsed = 0.96;
    cap.state.tumble = Some(tc);
    for _ in 0..4 {
        cap.tick(Input::default(), Actions::default());
    }
    assert_eq!(
        cap.state.tumble.unwrap().phase,
        TumblePhase::GetUp,
        "Landed elapsed > 1.0s must enter GetUp even while speed^2 > 1.0"
    );
}

#[test]
fn a_slide_that_ends_where_there_is_no_room_to_stand_hands_control_back_in_a_crouch() {
    // Under the fixture's roof (1.8 m up) the ray from the knees finds no room; in the open it does.
    let finish = |roof: bool| {
        let mut rig = Rig::new(roof);
        rig.mover().position = V3::new(0.0, 0.0, 4.0);
        rig.mover().grounded = true;
        rig.mover().posture = Posture::Crouch;
        let mut t = Tumble::enter_ground(V3::ZERO, 0.0);
        t.phase = TumblePhase::Landed;
        t.elapsed = LANDED_MAX_TIME + 0.1;
        rig.state.tumble = Some(t);
        for _ in 0..4 {
            rig.tick(Input::default(), Actions::default());
        }
        rig
    };

    let mut roofed = finish(true);
    assert!(roofed.state.tumble.is_none(), "a lie that never ends: {:?}", roofed.state.tumble);
    assert_eq!(roofed.mover().posture, Posture::Crouch);
    assert_eq!(roofed.mover().air, Air::None);
    assert!(roofed.mover().grounded);
    assert_eq!(roofed.mover().lock, 0.0);
    // Control is back: the stick walks him, still crouched, and he does not stand under the roof.
    let z = roofed.mover().position.z;
    for _ in 0..30 {
        roofed.tick(Input { forward: 1., ..Input::default() }, Actions::default());
    }
    assert!(roofed.mover().position.z > z + 0.2, "he did not walk: {} -> {}", z, roofed.mover().position.z);
    assert_eq!(roofed.mover().posture, Posture::Crouch);

    let mut open = finish(false);
    assert_eq!(open.state.tumble.map(|t| t.phase), Some(TumblePhase::GetUp));
    assert!(open.mover().lock > 0.0);
}

#[test]
fn getup_front_and_back_use_distinct_durations_and_block_mouse_look() {
    let measure = |front: bool| -> u32 {
        let mut rig = Rig::new(false);
        rig.tuning.getup_front_seconds = 1.6;
        rig.tuning.getup_back_seconds = 0.8;
        rig.mover().grounded = true;
        rig.mover().posture = Posture::Crouch;
        let mut t = Tumble::enter_ground(V3::ZERO, 0.0);
        t.phase = TumblePhase::Landed;
        t.front = front;
        rig.state.tumble = Some(t);
        let mut ticks = 0u32;
        for _ in 0..160 {
            rig.tick(Input::default(), Actions::default());
            if let Some(cur) = rig.state.tumble {
                if cur.phase == TumblePhase::GetUp {
                    ticks += 1;
                    // Verify Soldier::look_raw respects mover.lock while standing up.
                    let mut body = crate::gen::Soldier {
                        aim_yaw: 0.4,
                        aim_pitch: 0.1,
                        max_turn: 2.0,
                        pitch_limits: [-0.8, 1.1],
                        lock: rig.mover().lock,
                        ..Default::default()
                    };
                    body.look_raw(200.0, -100.0, 1.0, false);
                    assert_eq!((body.aim_yaw, body.aim_pitch), (0.4, 0.1), "GetUp lock must freeze yaw & pitch");
                }
            } else {
                break;
            }
        }
        assert!(rig.state.tumble.is_none());
        assert_eq!(rig.mover().lock, 0.0);
        ticks
    };
    let front_ticks = measure(true);
    let back_ticks = measure(false);
    assert!((94..=98).contains(&front_ticks), "front getup (1.6s) ticks={front_ticks}");
    assert!((46..=50).contains(&back_ticks), "back getup (0.8s) ticks={back_ticks}");
}

#[test]
fn recover_landing_checks_ground_plane_tangential_speed() {
    // Fast horizontal landing in Recover (> 8 m/s tangential) -> enters Tumble.
    let mut fast = Rig::new(false);
    fast.mover().grounded = true;
    fast.mover().air = Air::Fall;
    fast.mover().velocity = V3::new(9.5, -2.0, 0.0);
    let mut tf = Tumble::enter(fast.mover().velocity, 0.0, 1);
    tf.phase = TumblePhase::Recover;
    fast.state.tumble = Some(tf);
    fast.tick(Input::default(), Actions::default());
    assert_eq!(fast.state.tumble.unwrap().phase, TumblePhase::Tumble);

    // Slow horizontal landing in Recover (<= 8 m/s tangential) -> lands cleanly upright.
    let mut slow = Rig::new(false);
    slow.mover().grounded = true;
    slow.mover().air = Air::Fall;
    slow.mover().velocity = V3::new(3.0, -9.0, 0.0);
    let mut ts = Tumble::enter(slow.mover().velocity, 0.0, 2);
    ts.phase = TumblePhase::Recover;
    slow.state.tumble = Some(ts);
    slow.tick(Input::default(), Actions::default());
    assert!(slow.state.tumble.is_none(), "low tangential speed in Recover must land upright");
    assert_eq!(slow.mover().air, Air::None);
}

#[test]
fn a_bot_turns_at_four_times_max_turn_speed_without_the_ground_row_and_with_the_air_row() {
    let input = Input { turn: 0.6, ..Input::default() };
    for ai in [false, true] {
        let actions = Actions { ai, ..Actions::default() };
        let mut standing = Rig::new(false);
        standing.tuning.stand = ControlSpeed { forward: 1.0, strafe: 1.0, turn: 0.5 };
        standing.mover().max_turn = 2.0;
        standing.tick(input, actions);
        let scale = if ai { SWBF2_AI_TURN } else { 0.5 };
        near(standing.mover().aim_yaw, -2.0 * scale * 0.6 * DT);

        let mut falling = Rig::new(false);
        falling.tuning.jump = ControlSpeed { forward: 0.0, strafe: 0.0, turn: 0.4 };
        falling.mover().max_turn = 2.0;
        falling.mover().air = Air::Fall;
        falling.mover().grounded = false;
        falling.mover().position.y = 30.0;
        falling.mover().settled = falling.mover().position;
        falling.tick(input, actions);
        let scale = if ai { SWBF2_AI_TURN * 0.4 } else { 0.4 };
        near(falling.mover().aim_yaw, -2.0 * scale * 0.6 * DT);

        let mut jetting = airborne_jet(true, 0.0, false);
        jetting.mover().air = Air::Jet;
        jetting.mover().fuel = 0.9;
        jetting.tick(input, actions);
        let row = jetting.tuning.jet.turn;
        let m = jetting.mover();
        let scale = if ai { SWBF2_AI_TURN } else { row };
        near(m.aim_yaw, -m.max_turn * scale * 0.6 * DT);
    }
}
