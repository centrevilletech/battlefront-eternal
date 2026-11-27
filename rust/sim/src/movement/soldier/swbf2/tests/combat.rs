use super::*;

impl Rig {
    fn combat_step(&mut self, controls: MovementOverride, input: Input, actions: Actions, dt: f64)
        -> Result<bool, ProfileError> {
        self.movement.advance_profiled(&mut self.collision, self.id, &input, dt, Some(Step {
            actions, tuning: &self.tuning, state: &mut self.state, override_: Some(controls),
        }))
    }
}

#[test]
fn authored_velocity_reaches_collision_on_the_current_tick() {
    let mut rig = Rig::new(false);
    let input = Input { forward: -1., right: 1., ..Input::default() };
    let mut z = 0.;
    for speed in [8., 0., -6.] {
        let controls = MovementOverride { velocity_xz: Some([0., speed]), ..Default::default() };
        rig.combat_step(controls, input, Actions::default(), DT).unwrap();
        z += speed * DT;
        near(rig.mover().position.z, z);
        near(rig.mover().position.x, 0.);
        near(rig.mover().velocity.z, speed);
    }
    let held = MovementOverride { lock_locomotion: true, ..Default::default() };
    rig.combat_step(held, input, Actions::default(), DT).unwrap();
    near(rig.mover().velocity.z, -6.);
    near(rig.mover().position.z, z - 6. * DT);

    // Held locomotion keeps its velocity on a slope. The ground drive of a body
    // without class acceleration still lets gravity act along the slope.
    let on_slope = |control: bool| {
        let mut slope = Rig::on(terrain(30., 0.2, None));
        slope.tick(Input::default(), Actions::default());
        assert!(slope.mover().normal.y < 1.);
        slope.mover().velocity = V3::ZERO;
        if control { slope.mover().accel = 0.; }
        let action = MovementOverride { lock_locomotion: !control, ..Default::default() };
        slope.combat_step(action, input, Actions::default(), DT).unwrap();
        (slope.mover().position, slope.mover().velocity)
    };
    let (_, held_velocity) = on_slope(false);
    let (_, driven_velocity) = on_slope(true);
    assert_eq!(held_velocity, V3::ZERO);
    assert!(driven_velocity.y < 0.0, "{driven_velocity:?}");

    let wall = [-5., -2., 2., 5., -2., 2., 5., 6., 2., -5., 6., 2.];
    let mut rig = Rig::on(terrain(30., 0., Some(wall)));
    let forward = MovementOverride { velocity_xz: Some([0., 10.]), ..held };
    for _ in 0..40 {
        rig.combat_step(forward, Input::default(), Actions::default(), DT).unwrap();
    }
    assert!((1.0..2.0).contains(&rig.mover().position.z));
    assert!(rig.mover().grounded);
}

#[test]
fn input_lock_masks_raw_actions_and_each_movement_axis() {
    let mut rig = Rig::new(false);
    let all = MovementOverride { input_lock: InputLockMask::ALL, ..Default::default() };
    let input = Input { forward: 1., right: 1., turn: 1., pitch: 1., jump: true, crouch: true, ..Default::default() };
    let actions = Actions { sprint: true, roll: true, ai: false };
    assert!(rig.combat_step(all, input, actions, DT).unwrap());
    assert_eq!(rig.mover().air, Air::None);
    assert_eq!(rig.mover().posture, Posture::Stand);
    assert_eq!(rig.mover().position, V3::ZERO);
    assert_eq!((rig.mover().aim_yaw, rig.mover().aim_pitch, rig.mover().lock), (0., 0., 0.));
    assert!(rig.state.sprint_time.is_none() && rig.state.roll.is_none());

    for (mask, x, z, yaw) in [(InputLockMask::THRUST, true, false, true),
        (InputLockMask::STRAFE, false, true, true), (InputLockMask::TURN, true, true, false)] {
        let mut rig = Rig::new(false);
        let control = MovementOverride { input_lock: mask, ..Default::default() };
        let axes = Input { crouch: false, jump: false, ..input };
        rig.combat_step(control, axes, Actions::default(), DT).unwrap();
        assert_eq!(rig.mover().position.x != 0., x);
        assert_eq!(rig.mover().position.z != 0., z);
        assert_eq!(rig.mover().aim_yaw != 0., yaw);
        assert_eq!(rig.mover().aim_pitch != 0., yaw);
    }
    // A crouch lock filters the raw roll and crouch, leaving jump available.
    let mut rig = Rig::new(false);
    rig.combat_step(MovementOverride { input_lock: InputLockMask::CROUCH, ..all }, input, actions, DT).unwrap();
    assert_eq!(rig.mover().air, Air::Jump);
    assert!(rig.state.roll.is_none());
    let mut rig = Rig::new(false);
    rig.mover().velocity.z = 7.;
    let control = MovementOverride { input_lock: InputLockMask::SPRINT, ..all };
    rig.combat_step(control, Input { forward: 1., ..Default::default() }, Actions { sprint: true, roll: false, ai: false }, DT).unwrap();
    assert!(rig.state.sprint_time.is_none());
}

#[test]
fn aerial_impulse_facing_and_terminal_speed_precede_displacement() {
    let mut rig = Rig::new(false);
    rig.mover().grounded = false;
    rig.mover().air = Air::Fall;
    rig.mover().position.y = 7.;
    rig.mover().settled.y = 7.;
    let controls = MovementOverride { vertical_impulse: Some(3.), gravity_scale: 0.,
        aim_yaw: Some(1.25), ..Default::default() };
    rig.combat_step(controls, Input::default(), Actions::default(), DT).unwrap();
    near(rig.mover().velocity.y, 3.);
    near(rig.mover().position.y, 7. + 3. * DT);
    assert_eq!(rig.mover().aim_yaw, 1.25);
    let reverse = MovementOverride { gravity_scale: -0.5, ..Default::default() };
    let y = rig.mover().position.y;
    rig.combat_step(reverse, Input::default(), Actions::default(), DT).unwrap();
    near(rig.mover().velocity.y, 3. + GRAVITY * 0.5 * DT);
    near(rig.mover().position.y, y + (3. + GRAVITY * 0.5 * DT) * DT);
    for locked in [true, false] {
        rig.mover().velocity.y = -4.;
        let controls = MovementOverride { gravity_scale: 0.5, gravity_velocity_target: Some(-4.1),
            lock_locomotion: locked, ..Default::default() };
        let y = rig.mover().position.y;
        rig.combat_step(controls, Input::default(), Actions::default(), DT).unwrap();
        near(rig.mover().velocity.y, -4.1);
        near(rig.mover().position.y, y - 4.1 * DT);
    }
}

#[test]
fn authored_postures_share_one_charge_even_with_all_raw_controls_locked() {
    for roll in [false, true] {
        let mut rig = Rig::new(false);
        rig.mover().velocity.z = 9.;
        rig.state.energy = 3.;
        assert!(rig.state.can_charge(5.));
        assert!(rig.state.charge(5., &rig.tuning));
        assert!(rig.state.exhausted && !rig.state.can_charge(1.));
        assert!(rig.state.can_charge(0.));
        rig.tuning.energy_cost_roll = 0.;
        rig.tuning.energy_cost_jump = 0.;
        rig.tuning.energy_cost_jump_sprint = Some(0.);
        let controls = MovementOverride { input_lock: InputLockMask::ALL, force_jump: true,
            force_roll: roll.then_some(RollDirection::Left), energy_rate: Some(0.), ..Default::default() };
        rig.combat_step(controls, Input::default(), Actions::default(), DT).unwrap();
        assert_eq!(rig.state.roll.map(|r| r.direction), roll.then_some(RollDirection::Left));
        assert_eq!(rig.mover().air == Air::Jump, !roll);
        assert_eq!(rig.state.energy, 0.);
        assert!(rig.state.exhausted);
    }
}

#[test]
fn action_energy_rate_replaces_passive_recovery_and_preserves_the_latch() {
    let mut rig = Rig::new(false);
    rig.tuning.energy_floor = -2.;
    rig.state.energy = 3.;
    let mut controls = MovementOverride { energy_rate: Some(-5.), ..Default::default() };
    rig.combat_step(controls, Input::default(), Actions::default(), 1.).unwrap();
    assert_eq!(rig.state.energy, -2.);
    assert!(rig.state.exhausted);
    rig.tuning.energy_min_sprint = 0.;
    rig.state.energy = 0.;
    controls.energy_rate = Some(0.);
    rig.combat_step(controls, Input::default(), Actions::default(), DT).unwrap();
    assert!(rig.state.exhausted);
    rig.tuning.energy_min_sprint = 2.;
    controls.energy_rate = Some(120.);
    controls.force_roll = Some(RollDirection::Forward);
    rig.tuning.energy_cost_roll = 0.;
    rig.combat_step(controls, Input::default(), Actions::default(), DT).unwrap();
    near(rig.state.energy, 2.);
    assert!(rig.state.roll.is_some() && !rig.state.exhausted);
    for invalid in [f64::NAN, -1., f64::INFINITY] {
        assert!(!rig.state.can_charge(invalid));
        assert!(!rig.state.charge(invalid, &rig.tuning));
        rig.state.recover(invalid, &rig.tuning);
        assert_eq!(rig.state.energy, 2.);
    }
    assert!(!rig.state.apply_energy_rate(f64::MAX, 2., &rig.tuning));
    assert!(!rig.state.apply_energy_rate(1., -DT, &rig.tuning));
    assert_eq!(rig.state.energy, 2.);
}

#[test]
fn invalid_combat_controls_fail_before_any_motion_or_stamina_change() {
    let ordinary = MovementOverride::default();
    for bad in [
        MovementOverride { velocity_xz: Some([1., f64::NAN]), ..ordinary },
        MovementOverride { air_accel_xz: Some([1., f64::NAN]), ..ordinary },
        MovementOverride { vertical_impulse: Some(f64::INFINITY), ..ordinary },
        MovementOverride { gravity_scale: f64::NAN, ..ordinary },
        MovementOverride { gravity_scale: f64::MAX, ..ordinary },
        MovementOverride { gravity_velocity_target: Some(f64::NAN), ..ordinary },
        MovementOverride { aim_yaw: Some(f64::INFINITY), ..ordinary },
        MovementOverride { energy_rate: Some(f64::NAN), ..ordinary },
        MovementOverride { input_lock: InputLockMask(1 << 12), ..ordinary },
    ] {
        let mut rig = Rig::new(false);
        for dt in [0., DT] {
            assert!(rig.combat_step(bad, Input::default(), ROLL, dt).is_err());
            assert_eq!(rig.mover().position, V3::ZERO);
            assert_eq!(rig.state.energy, rig.tuning.energy_capacity);
        }
    }
    let mut rig = Rig::new(false);
    let controls = MovementOverride { aim_yaw: Some(0.5), velocity_xz: Some([1., 2.]),
        force_jump: true, energy_rate: Some(-10.), ..ordinary };
    rig.combat_step(controls, Input::default(), ROLL, 0.).unwrap();
    assert_eq!(rig.mover().aim_yaw, 0.);
    assert_eq!(rig.mover().velocity, V3::ZERO);
    assert_eq!(rig.state.energy, rig.tuning.energy_capacity);
}

#[test]
fn an_authored_move_still_integrates_when_hover_ends() {
    for cancel in [false, true] {
        let mut rig = airborne_jet(true, 0.0, false);
        rig.mover().air = Air::Jet;
        rig.mover().fuel = if cancel { 0.75 } else { 0. };
        let position = rig.mover().position;
        let controls = MovementOverride { velocity_xz: Some([1.5, 4.]), vertical_impulse: Some(-2.),
            gravity_scale: 0.5, ..Default::default() };
        rig.combat_step(controls, Input { jump: cancel, ..Default::default() }, Actions::default(), DT).unwrap();
        assert_eq!(rig.mover().air, Air::Fall);
        near(rig.mover().position.x, position.x + 1.5 * DT);
        near(rig.mover().position.z, position.z + 4. * DT);
        near(rig.mover().position.y, position.y + (-2. - GRAVITY * 0.5 * DT) * DT);
    }
}

#[test]
fn end_jet_cancels_energy_jet_and_air_accel_preserves_momentum_with_drag() {
    // The energy cost is paid once at ignition now and a melee hero's double jump never
    // enters the jet state, so the end-of-jet path matters for the hover packs; the checks
    // below still hold and still test that the jet ends and the momentum survives.
    let mut rig = airborne_jet(false, 36., false);
    rig.mover().air = Air::Jet;
    rig.mover().velocity = V3::new(0., 4., 12.);
    rig.state.energy = 8.;
    let controls = MovementOverride {
        air_accel_xz: Some([0., 0.]),
        gravity_scale: 2.,
        end_jet: true,
        ..Default::default()
    };
    rig.combat_step(controls, Input { jump: true, ..Default::default() }, Actions::default(), DT).unwrap();
    assert_eq!(rig.mover().air, Air::Fall);
    near(rig.state.energy, 8.);
    let drag = v8math::exp(-0.012 * v8math::sqrt(16. + 144.) * DT);
    near(rig.mover().velocity.z, 12. * drag);
    near(rig.mover().velocity.y, 4. * drag - GRAVITY * 2. * DT);

    // Non-zero airborne acceleration adds a*dt after quadratic air drag.
    let mut dive = airborne_jet(false, 36., false);
    dive.mover().air = Air::Fall;
    dive.mover().velocity = V3::new(0., -3., 8.);
    let accel = MovementOverride {
        air_accel_xz: Some([0., 25.5]),
        gravity_scale: 1.,
        ..Default::default()
    };
    dive.combat_step(accel, Input::default(), Actions::default(), DT).unwrap();
    let d2 = v8math::exp(-0.012 * v8math::sqrt(9. + 64.) * DT);
    near(dive.mover().velocity.z, 8. * d2 + 25.5 * DT);
    near(dive.mover().velocity.y, -3. * d2 - GRAVITY * DT);
}
