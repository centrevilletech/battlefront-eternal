//! A 2005 soldier's own correction, from the authority's tick to the client's
//! replay. The authority steps sprints and rolls, its publication captures
//! the soldier body 0x82, and a client that restores those bytes at any tick and replays
//! the same commands arrives at the authority's exact bits.
use super::{capture_swbf2, restore_swbf2};
use crate::collision;
use crate::gen::{MoveInput, Soldier, UnitBody};
use crate::movement::soldier::swbf2::{body, Actions, ControlSpeed, State, Step, Tuning};
use crate::movement::soldier::V3;
use crate::movement::MovementOwner;
use crate::prediction::actors::Session;
use crate::units::soldier::tick_soldier_in;
use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
};
use bfe_family::GameFamily;

const DT: f64 = 1.0 / 60.0;
const TICKS: usize = 160;

// Invented class numbers chosen to exercise ramps, drains and rolls quickly.
fn tuning() -> Tuning {
    let row = |forward, strafe, turn| ControlSpeed { forward, strafe, turn };
    Tuning {
        stand: row(1.0, 0.9, 1.0), crouch: row(0.6, 0.5, 0.9), sprint: row(1.6, 0.45, 0.6),
        jump: row(0.05, 0.05, 0.3), jet: row(0.5, 0.5, 0.5), jetpack: None, roll: row(0.0, 0.0, 0.25),
        tumble: row(0.0, 0.0, 0.1), sprint_accelerate_time: 0.3, energy_capacity: 50.0, energy_floor: 0.0,
        energy_restore: 5.0, energy_restore_idle: 7.5, energy_drain_sprint: 40.0, energy_min_sprint: 10.0,
        energy_cost_jump: 1.5, energy_cost_jump_sprint: None, energy_cost_roll: 10.0, jump_height: 1.6,
        jump_forward_speed_factor: 1.2, jump_strafe_speed_factor: 0.8, roll_speed_factor: 1.8, roll_frames: 19,
        recover_from_tumble: false, getup_front_seconds: 1.0, getup_back_seconds: 1.0,
    }
}

fn floor() -> (u32, collision::State) {
    let ground = vec![-200., 0., -200., 200., 0., -200., 200., 0., 200., -200., 0., 200.];
    let positions = vec![-6., -100., -6., 6., -100., -6., 6., -100., 6., -6., -100., 6.];
    let indices = vec![0, 2, 1, 0, 3, 2];
    let buildings = build(Positions::F32(&positions), &indices);
    let terrain = build(Positions::F32(&ground), &indices);
    let map = MapCollision {
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: Some(TriangleGrid::new(&ground, &indices)),
        positions: positions.into(),
        indices: indices.clone().into(),
        polygons: vec![0, 1].into(),
        terrain_positions: ground.into(),
        terrain_indices: indices.into(),
        masks: Vec::<u8>::new().into(),
        has_terrain: true,
        kill_y: -200.,
    };
    let mut owner = collision::CollisionOwner::new();
    let world = owner.native_install(&map, &buildings, Some(&terrain)).unwrap().world;
    (world, owner.state)
}

// The class numbers both sides load: the authority's record and the client's
// prepared actor row agree on them; everything else comes from the bytes.
fn class() -> Soldier {
    Soldier { mover: -1.0, max_speed: 6.5, max_strafe: 5.0, accel: 48.0, max_turn: 2.5, max_fly_height: 150.0, ..Soldier::default() }
}

// What the player does: run and sprint, roll forward, press roll again while
// still rolling, strafe and roll sideways, sprint until the energy runs out,
// press roll on an empty bar, then stand still.
fn command(tick: usize) -> (MoveInput, Actions) {
    let strafe = (64..72).contains(&tick);
    let idle = (130..140).contains(&tick);
    let forward = if strafe || idle { 0.0 } else { 1.0 };
    let right = if strafe { -1.0 } else { 0.0 };
    let input = MoveInput { forward, right, turn: if tick % 7 == 0 { 0.4 } else { 0.0 }, ..MoveInput::default() };
    let sprint = (5..30).contains(&tick) || (96..140).contains(&tick);
    (input, Actions { sprint, roll: [34, 52, 66, 125].contains(&tick), ai: false })
}

struct Step0 { soldier: Soldier, state: State, bytes: Vec<u8> }

fn own_bytes(soldier: &Soldier, state: &State) -> Vec<u8> {
    let mut own = crate::wire::store_own::Capture::new(&crate::combat::defs::WeaponDefs::default());
    assert_eq!(own.foot_swbf2(&UnitBody::Soldier(soldier.clone()), state, None).unwrap(), body::SCHEMA);
    own.body.value.clone()
}

fn authority() -> Vec<Step0> {
    let (world, mut collision) = floor();
    let mut owner = MovementOwner::new();
    owner.select_family(GameFamily::Swbf2).unwrap();
    let tuning = tuning();
    let mut soldier = Soldier { grounded: true, normal: [0.0, 1.0, 0.0], ..class() };
    let mut state = State::new(&tuning);
    let mut out = Vec::new();
    for tick in 0..TICKS {
        let (input, actions) = command(tick);
        tick_soldier_in(&mut owner, &mut collision, &mut soldier, &input, DT, world as usize, -1,
            Some(Step { actions, tuning: &tuning, state: &mut state, override_: None })).unwrap();
        out.push(Step0 { bytes: own_bytes(&soldier, &state), soldier: soldier.clone(), state });
    }
    out
}

fn bits(s: &Soldier) -> Vec<[u8; 8]> {
    [s.position, s.velocity, s.normal, s.settled].iter().flatten().chain(&[s.aim_yaw, s.aim_pitch, s.state_time, s.lock, s.fuel])
        .map(|x| x.to_le_bytes()).collect()
}

#[test]
fn replay_from_any_correction_reaches_the_authority_exactly() {
    let tuning = tuning();
    let history = authority();
    // The scenario does reach every movement state the suffix carries.
    assert!(history.iter().any(|s| s.state.sprint_time.is_some()));
    assert!(history.iter().any(|s| s.state.roll.is_some_and(|r| r.direction == crate::movement::soldier::swbf2::RollDirection::Left)));
    assert!(history.iter().any(|s| s.state.exhausted));
    // A roll is in flight when the second press arrives, and the class's
    // single permitted overdraw is reached.
    assert!(history[52].state.roll.is_some());
    assert!(history.iter().any(|s| s.state.energy < 1.0 && s.state.exhausted));
    let last = history.last().unwrap();
    let (world, mut collision) = floor();
    for from in [0, 12, 35, 45, 52, 67, 100, 119, 126, 145] {
        let mut actor = Session::default();
        actor.configure_for_family(&[0.0], GameFamily::Swbf2).unwrap();
        actor.unit.soldier = UnitBody::Soldier(class());
        let mut swbf2 = (tuning, State::new(&tuning));
        restore_swbf2(&mut actor.unit.soldier, &history[from].bytes, &mut swbf2).unwrap();
        for tick in from + 1..TICKS {
            let (input, actions) = command(tick);
            actor.unit.input = input;
            actor.step(&mut collision, world as usize, -1, &[], DT, (0.0, V3::ZERO, false), true, false, false,
                false, false, false, 0.0, Some(Step { actions, tuning: &swbf2.0, state: &mut swbf2.1, override_: None })).unwrap();
        }
        let UnitBody::Soldier(s) = &actor.unit.soldier else { unreachable!() };
        assert_eq!(bits(s), bits(&last.soldier), "replay from tick {from}");
        let mut continuation = Vec::new();
        capture_swbf2(&actor.unit.soldier, &mut continuation, &swbf2.1).unwrap();
        assert_eq!(continuation, last.bytes, "continuation from tick {from}");
    }
}

#[test]
fn a_correction_the_class_could_not_produce_is_refused_whole() {
    let tuning = tuning();
    let history = authority();
    let rolling = history.iter().find(|s| s.state.roll.is_some()).unwrap();
    // The revision, then the compact body.
    let compact = 2 + body::compact_len(&rolling.bytes[2..]).unwrap();
    let mut actor = UnitBody::Soldier(class());
    let mut swbf2 = (tuning, State::new(&tuning));
    // A soldier body without its suffix, with a stray byte, or for a class whose
    // roll is shorter than the one in flight.
    for bytes in [rolling.bytes[..compact].to_vec(), [rolling.bytes.clone(), vec![0]].concat()] {
        assert!(restore_swbf2(&mut actor, &bytes, &mut swbf2).is_err());
    }
    let mut short = (Tuning { roll_frames: 1, ..tuning }, State::new(&tuning));
    assert!(restore_swbf2(&mut actor, &rolling.bytes, &mut short).is_err());
    // A refused restore installs nothing.
    let before = format!("{:?}", actor);
    assert!(restore_swbf2(&mut actor, &[rolling.bytes.clone(), vec![0]].concat(), &mut swbf2).is_err());
    assert_eq!(format!("{:?}", actor), before);
    // A 2004 restore never reads a 2005 body.
    assert!(super::restore(&mut actor, &rolling.bytes).is_err());
}

// A 2005 soldier's melee, thrown blade and powers answer their triggers
// themselves: the replayed arsenal that holds them fires only his guns.
#[test]
fn the_replayed_arsenal_fires_guns_and_leaves_abilities_alone() {
    use crate::combat::defs::{Js, JsObj, WeaponClass};
    use crate::{gen::Weapon, store::DefId};
    let mut arsenal = crate::prediction::weapons::Session::default();
    let bases = ["cannon", "melee", "meleethrow", "areaeffectweapon"];
    for (at, base) in bases.into_iter().enumerate() {
        let fields = vec![("base".into(), Js::Str(base.into())), ("channel".into(), Js::Num(0.)), ("ShotDelay".into(), Js::Num(0.5))];
        arsenal.defs.set(DefId(at as u32), WeaponClass::new(std::rc::Rc::new(JsObj { fields, def: None })));
    }
    let fired = [false, true].map(|abilities| {
        arsenal.soldier_abilities = abilities;
        [0, 1, 2, 3].map(|at| {
            arsenal.weapons = vec![Weapon::create(DefId(at), &arsenal.defs, true)];
            let before = arsenal.weapons.clone();
            // Identity and life, the primary trigger pressed and held, nothing
            // else; standing, in no air state.
            let mut input = vec![0.; 23];
            (input[0], input[1], input[2], input[3], input[14]) = (7., 1., 1., 1., -1.);
            arsenal.input = input;
            arsenal.run(2).unwrap();
            !arsenal.shots.is_empty() || arsenal.weapons != before
        })
    });
    assert_eq!(fired, [[true; 4], [true, false, false, false]], "{bases:?}");
}

// A 2005 soldier who is thrown turns his look by the class's tumble row while he is thrown, tumbling or
// sliding and not at all while he stands up; the replay of an online correction, from a thrown soldier's
// own `0x82` body at any age, reaches the authority's exact bits through every phase, look included.
mod thrown {
    use super::*;
    use crate::client::{look_scale, ClientPrediction};
    use crate::gen::{ClientCommand, ClientFrame, ClientLook};
    use crate::movement::soldier::swbf2::{Tumble, TumblePhase};

    const THROW_AT: usize = 20;
    const LOOK: f64 = 6.0;
    const RUN: usize = 330;

    struct Tick { soldier: Soldier, state: State, bytes: Vec<u8> }

    /// What the player holds and presses at a tick: the held keys and the action bits of the command.
    type Script = fn(usize) -> (u32, u32);
    fn idle(_: usize) -> (u32, u32) { (0, 0) }
    // Walk, sprint, roll twice, jump, strafe: every row of the class's table but the tumble.
    fn moving(tick: usize) -> (u32, u32) {
        use crate::client::{FORWARD, LEFT, SPRINT};
        use crate::gen::ClientAction;
        let held = FORWARD | if (10..40).contains(&tick) { SPRINT } else { 0 } | if (240..260).contains(&tick) { LEFT } else { 0 };
        let actions = if [80, 205].contains(&tick) { 1 << ClientAction::Roll as u8 }
            else if tick == 130 { 1 << ClientAction::Jump as u8 } else { 0 };
        (held, actions)
    }
    fn input_of(held: u32, actions: u32) -> (MoveInput, Actions) {
        use crate::client::{FORWARD, LEFT, RIGHT, SPRINT};
        use crate::gen::ClientAction;
        let axis = |plus: u32, minus: u32| f64::from(held & plus != 0) - f64::from(held & minus != 0);
        (MoveInput { forward: axis(FORWARD, 0), right: axis(RIGHT, LEFT), jump: actions & (1 << ClientAction::Jump as u8) != 0,
                ..MoveInput::default() },
            Actions { sprint: held & SPRINT != 0, roll: actions & (1 << ClientAction::Roll as u8) != 0, ai: false })
    }

    // The authority: the look of the tick, turned by the row of the state the tick begins in, then the step.
    fn authority(tuning: &Tuning, throw: Option<V3>, script: Script) -> Vec<Tick> {
        let (world, mut collision) = floor();
        let mut movement = MovementOwner::new();
        movement.select_family(GameFamily::Swbf2).unwrap();
        let mut soldier = Soldier { grounded: true, normal: [0.0, 1.0, 0.0], ..class() };
        let mut state = State::new(tuning);
        let mut out = Vec::new();
        for tick in 0..RUN {
            if let (Some(velocity), true) = (throw, tick == THROW_AT) {
                soldier.velocity = [velocity.x, velocity.y, velocity.z];
                soldier.air = Some(crate::gen::Air::Thrown);
                soldier.grounded = false;
                state.tumble = Some(Tumble::enter(velocity, soldier.aim_yaw, 1));
            }
            let turn = look_scale(tuning, &state, &soldier);
            soldier.look_raw_scaled(LOOK, 0.0, 1.0, false, turn);
            let (held, pressed) = script(tick);
            let (input, actions) = input_of(held, pressed);
            tick_soldier_in(&mut movement, &mut collision, &mut soldier, &input, DT, world as usize, -1,
                Some(Step { actions, tuning, state: &mut state, override_: None })).unwrap();
            out.push(Tick { bytes: own_bytes(&soldier, &state), soldier: soldier.clone(), state });
        }
        out
    }

    // The client's side of one frame: the correction of tick `from` and the commands after it, each with its look.
    fn replay(prediction: &mut ClientPrediction, collision: &mut collision::State, world: u32, history: &[Tick], from: usize, to: usize, script: Script) {
        let mut words = vec![0.; ClientFrame::WORDS + Soldier::WORDS];
        let mut soldier = history[from].soldier.clone();
        soldier.put(&mut words[ClientFrame::WORDS..], &mut Vec::new());
        let bytes = &history[from].bytes;
        let correction_at = words.len() as u32;
        for chunk in bytes.chunks(4) {
            let mut padded = [0; 4];
            padded[..chunk.len()].copy_from_slice(chunk);
            words.push(u32::from_le_bytes(padded).into());
        }
        let weapons_at = words.len() as u32;
        words.extend([0., 0., 0., 0., 0.]);
        let looks_at = words.len();
        for _ in from + 1..=to {
            let at = words.len();
            words.resize(at + ClientLook::WORDS, 0.);
            ClientLook { x: LOOK, y: 0.0 }.put(&mut words[at..], &mut Vec::new());
        }
        let commands_at = words.len() as u32;
        for (n, tick) in (from + 1..=to).enumerate() {
            let at = words.len();
            words.resize(at + ClientCommand::WORDS, 0.);
            let (held, actions) = script(tick);
            ClientCommand { tick: tick as f64, sequence: tick as f64, generation: 1., sent: true, held, actions,
                look_at: (looks_at + n * ClientLook::WORDS) as u32, look_count: 1, ..Default::default() }
                .put(&mut words[at..], &mut Vec::new());
        }
        let frame = ClientFrame { epoch: 1., identity: 1., generation: 1., source: 1., movement: true, world,
            ignored: -1., authority_tick: from as f64, check_tick: to as f64, held: script(from).0, actor_at: ClientFrame::WORDS as u32,
            actor_words: Soldier::WORDS as u32, correction_at, correction_bytes: bytes.len() as u32,
            weapons_at, weapons_words: 5, commands_at, commands_count: (to - from) as u32, mouse_scale: 1.,
            ..Default::default() };
        frame.put(&mut words, &mut Vec::new());
        prediction.predict_session(&words, collision).unwrap();
    }

    fn phase(state: &State) -> Option<TumblePhase> { state.tumble.map(|t| t.phase) }

    fn run(recover_from_tumble: bool, velocity: V3) {
        let tuning = Tuning { recover_from_tumble, ..tuning() };
        let history = authority(&tuning, Some(velocity), idle);
        // The scenario reaches the phases of its kind, in order.
        let mut seen = Vec::new();
        for t in &history {
            if let Some(p) = phase(&t.state) { if seen.last() != Some(&p) { seen.push(p); } }
        }
        let expected: &[TumblePhase] = if recover_from_tumble {
            &[TumblePhase::Thrown, TumblePhase::Recover]
        } else {
            &[TumblePhase::Thrown, TumblePhase::Tumble, TumblePhase::Landed, TumblePhase::GetUp]
        };
        assert_eq!(seen, expected);
        // The authority's own turn per tick: the standing rate before the throw, the tumble row (0.1) through
        // the thrown, tumbling and sliding phases, nothing in the stand-up.
        let step = |t: usize| history[t].soldier.aim_yaw - history[t - 1].soldier.aim_yaw;
        let standing = step(THROW_AT - 2);
        assert!(standing < 0.0);
        for t in THROW_AT + 1..RUN {
            let before = &history[t - 1];
            let want = match phase(&before.state) {
                Some(TumblePhase::Thrown | TumblePhase::Tumble | TumblePhase::Landed) => 0.1,
                Some(TumblePhase::GetUp) => 0.0,
                Some(TumblePhase::Recover) => continue,
                None if before.soldier.lock > 0.0 => 0.0,
                None if before.soldier.air.is_none() && before.soldier.posture == crate::gen::Posture::Stand => 1.0,
                None => continue,
            };
            assert!((step(t) - standing * want).abs() < 1e-12, "tick {t} in {:?}: {} against {}", phase(&before.state), step(t), standing * want);
        }
        // The correction that names the throw is the earliest that can be replayed: the throw is the
        // authority's own doing, no command holds it.
        compare(tuning, &history, idle, THROW_AT);
    }

    // Replay from a correction 0, 100 and 200 ms old, at every tick from `first` on, and compare with the
    // authority's bits, its continuation bytes and the turn the drawn frames of the next tick show.
    fn compare(tuning: Tuning, history: &[Tick], script: Script, first: usize) {
        let (world, mut collision) = floor();
        let mut prediction = ClientPrediction::default();
        prediction.configure_actor_for_family(&[0.0], GameFamily::Swbf2).unwrap();
        prediction.configure_swbf2(tuning).unwrap();
        for to in first..RUN {
            for delay in [0usize, 6, 12] {
                if to < first + delay { continue; }
                let from = to - delay;
                replay(&mut prediction, &mut collision, world, history, from, to, script);
                let UnitBody::Soldier(s) = &prediction.actor.unit.soldier else { unreachable!() };
                assert_eq!(bits(s), bits(&history[to].soldier), "tick {to}, {delay} ticks old, phase {:?}", phase(&history[to].state));
                let mut continuation = Vec::new();
                capture_swbf2(&prediction.actor.unit.soldier, &mut continuation, prediction.swbf2_state().unwrap()).unwrap();
                assert_eq!(continuation, history[to].bytes, "continuation at tick {to}, {delay} ticks old");
                // The frames between this tick and the next show the counts the next command will carry
                // turned by the scale the replay gives them (`Replay::present`): the drawn turns of one
                // tick add up to the tick's own turn at any frame rate, and none in the stand-up lock.
                if delay == 0 && to + 1 < RUN {
                    let scale = prediction.swbf2_look_scale().expect("a replayed 2005 soldier has a look scale");
                    let next = history[to + 1].soldier.aim_yaw - history[to].soldier.aim_yaw;
                    for frames in [1usize, 2, 3, 5] {
                        let (mut drawn, mut before) = (0.0, s.aim_yaw);
                        for k in 1..=frames {
                            let mut shown = s.clone();
                            shown.look_raw_scaled(LOOK * k as f64 / frames as f64, 0.0, 1.0, false, scale);
                            drawn += shown.aim_yaw - before;
                            before = shown.aim_yaw;
                        }
                        assert!((drawn - next).abs() < 1e-12, "tick {to}, {frames} frames: drawn {drawn} against {next}");
                    }
                }
            }
        }
    }

    // A soldier who walks, sprints, rolls, jumps and strafes turns by his class's row for each of those,
    // and the replay from a correction of any age reaches the authority's bits through all of them.
    #[test]
    fn a_walking_sprinting_rolling_and_jumping_soldier_is_replayed_to_the_authority_with_his_look() {
        let tuning = tuning();
        let history = authority(&tuning, None, moving);
        let step = |t: usize| history[t].soldier.aim_yaw - history[t - 1].soldier.aim_yaw;
        let standing_step = step(8);
        let rows = [(30, 0.6), (82, 0.25), (140, 0.3), (208, 0.25)];
        for (t, want) in rows {
            assert!((step(t) - standing_step * want).abs() < 1e-12, "tick {t}: {} against {}", step(t), standing_step * want);
        }
        assert!(history[30].state.sprint_time.is_some() && history[82].state.roll.is_some() && history[140].soldier.air.is_some());
        compare(tuning, &history, moving, 12);
    }

    #[test]
    fn a_thrown_soldier_slides_and_stands_up_and_the_replay_turns_his_look_as_the_authority_does() {
        run(false, V3::new(9.5, 6.75, 9.5));
    }

    #[test]
    fn a_thrown_soldier_who_recovers_in_the_air_is_replayed_to_the_authority_too() {
        run(true, V3::new(1.5, 14.0, 1.5));
    }
}
