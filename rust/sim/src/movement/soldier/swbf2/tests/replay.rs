use super::*;
use crate::client::{capture, restore};
use crate::gen::{Air as BodyAir, MoveInput, Soldier, UnitBody};
use crate::prediction::actors::Session;
use crate::units::soldier::tick_soldier_in;

fn command(tick: usize, class: &Tuning) -> (MoveInput, Actions, Tuning, MovementOverride) {
    let input = MoveInput { forward: if tick < 17 { 1. } else { 0. },
        turn: 0.13, jump: matches!(tick, 57 | 64), ..Default::default() };
    let actions = Actions { sprint: tick < 17, roll: false, ai: false };
    let mut tuning = *class;
    let mut control = MovementOverride::default();
    if tick >= 100 { tuning.roll_frames = 11; }
    match tick {
        22 | 107 => {
            tuning.energy_cost_roll = 0.;
            control.force_roll = Some(RollDirection::Right);
        }
        48..=54 => control.energy_rate = Some(180.),
        55 => control.force_jump = true,
        72..=76 => {
            control.velocity_xz = Some([-2., 1.]);
            control.lock_locomotion = true;
            control.gravity_scale = 0.25;
            control.input_lock = InputLockMask::TURN;
        }
        _ => {}
    }
    (input, actions, tuning, control)
}

fn life_bytes(
    state: &State,
) -> (
    [u8; 8],
    bool,
    Option<[u8; 8]>,
    Option<(RollDirection, [u8; 8])>,
    [u8; 8],
    Option<[u8; 8]>,
    u32,
) {
    (
        state.energy.to_le_bytes(),
        state.exhausted,
        state.sprint_time.map(f64::to_le_bytes),
        state.roll.map(|r| (r.direction, r.elapsed.to_le_bytes())),
        state.land_cooldown.to_le_bytes(),
        state.unsupported_time.map(f64::to_le_bytes),
        state.jump_count,
    )
}

#[test]
fn compact_body_and_infantry_state_replay_each_intervening_tick() {
    let mut class = tuning();
    class.energy_floor = -2.;
    class.energy_min_sprint = 2.;
    class.energy_drain_sprint = 24.;
    class.roll_frames = 17;
    class.jetpack = Some(Jetpack { physics: Jet { jump: 2., push: 3., hover: true, ..Default::default() },
        acceleration: 13., energy_cost: 7., force_jump: false });
    let template = Soldier { mover: -1., max_speed: 6., max_strafe: 5., accel: 52., max_turn: 2.,
        grounded: true, velocity: [0., 0., 6.], normal: [0., 1., 0.], max_fly_height: 150., ..Default::default() };
    let (world, mut collision) = ground(false);
    let mut owner = MovementOwner::new();
    owner.select_family(GameFamily::Swbf2).unwrap();
    let mut body = template.clone();
    let mut state = State::new(&class);
    state.energy = 1.;
    let mut history = Vec::new();
    let mut jet = false;
    for tick in 0..180 {
        let (input, actions, tuning, controls) = command(tick, &class);
        let can_fire = tick_soldier_in(&mut owner, &mut collision, &mut body, &input, DT, world as usize, -1,
            Some(Step { actions, tuning: &tuning, state: &mut state, override_: Some(controls) })).unwrap();
        let mut bytes = Vec::new();
        capture(&UnitBody::Soldier(body.clone()), &mut bytes).unwrap();
        history.push((bytes, state, can_fire));
        jet |= body.air == Some(BodyAir::Jet);
    }
    assert!(jet);
    assert!(history.iter().any(|(_, s, _)| s.sprint_time.is_some()));
    assert!(history.iter().any(|(_, s, _)| s.exhausted && s.energy < 0.));
    assert!(history.iter().any(|(_, s, _)| s.roll.is_some()));

    let (world, mut peer) = ground(false);
    for start in (0..history.len() - 8).step_by(3) {
        let mut actor = Session::default();
        actor.configure_for_family(&[0.], GameFamily::Swbf2).unwrap();
        actor.unit.soldier = UnitBody::Soldier(template.clone());
        restore(&mut actor.unit.soldier, &history[start].0).unwrap();
        let mut life = history[start].1;
        for (tick, expected) in history.iter().enumerate().skip(start + 1).take(8) {
            let (input, actions, tuning, controls) = command(tick, &class);
            actor.unit.input = input;
            let can_fire = actor.step(&mut peer, world as usize, -1, &[], DT, (0., V3::ZERO, false),
                true, false, false, false, false, false, 0., Some(Step {
                    actions, tuning: &tuning, state: &mut life, override_: Some(controls),
                })).unwrap();
            let mut bytes = Vec::new();
            capture(&actor.unit.soldier, &mut bytes).unwrap();
            assert_eq!(bytes, expected.0, "body from {start} at {tick}");
            assert_eq!(life_bytes(&life), life_bytes(&expected.1), "life from {start} at {tick}");
            assert_eq!(can_fire, expected.2, "firearm permission from {start} at {tick}");
        }
    }
}
