//! Every tick contributes all 80 exchange words, including the signs of zeros.
//! Phase hashes were captured on the original Battlefront mover with this scene.
use bfe_wide::{Fnv, Wide};
use super::*;
use crate::collision;
use crate::gen::{Air as BodyAir, MoveInput, Posture as BodyPosture, Soldier, UnitBody};
use crate::prediction::actors::Session;
use crate::units::soldier::tick_soldier_in;
use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
};

const H: f64 = 1.0 / 60.0;

fn arena(obstacles: bool) -> (usize, collision::State) {
    let floor = vec![
        -120., 0., -120., 120., 0., -120., 120., 0., 120., -120., 0., 120.,
    ];
    let mut props = vec![12., 85., -18., 32., 85., -18., 32., 85., 0., 12., 85., 0.];
    if obstacles {
        let mut surface = |z0, y0, z1, y1| props.extend([
            -3., y0, z0, 3., y0, z0, 3., y1, z1, -3., y1, z1,
        ]);
        surface(0., 0., 4., 1.2);       // ramp
        surface(4., 1.2, 6., 1.2);      // landing
        surface(6., 1.2, 6., 1.48);     // step face
        surface(6., 1.48, 11., 1.48);   // step tread
        surface(10., 1.48, 10., 4.5);   // wall
        surface(-8., 1.8, -3., 1.8);    // low ceiling
    }
    let indices = vec![0, 2, 1, 0, 3, 2];
    let faces: Vec<u32> = (0..props.len() as u32 / 12)
        .flat_map(|quad| indices.iter().map(move |i| 4 * quad + i)).collect();
    let b = build(Positions::F32(&props), &faces);
    let t = build(Positions::F32(&floor), &indices);
    let map = MapCollision {
        tri_bounds: triangle_bounds(&props, &faces),
        grid: Some(TriangleGrid::new(&floor, &indices)),
        positions: props.into(),
        polygons: (0..faces.len() as u32 / 3).collect::<Vec<_>>().into(),
        indices: faces.into(),
        terrain_positions: floor.into(),
        terrain_indices: indices.into(),
        masks: Vec::new().into(),
        has_terrain: true,
        kill_y: -160.,
    };
    let mut collision = collision::CollisionOwner::new();
    let world = collision.native_install(&map, &b, Some(&t)).unwrap().world;
    (world as usize, collision.state)
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    Walk,
    Crouch,
    GetUp,
    Jump,
    Dive,
    Hover,
    Jet,
    HardLanding,
    Thrown,
    StickHover,
    StickJet,
    RampAndWall,
    LowCeiling,
}

impl Phase {
    fn soldier(self) -> Soldier {
        let mut s = Soldier {
            mover: -1.,
            max_speed: 8.25,
            max_strafe: 6.5,
            max_jump: 8.75,
            max_turn: 1.8,
            max_pitch: 0.6,
            accel: 54.,
            pitch_limits: [-0.85, 1.05],
            normal: [0., 1., 0.],
            grounded: true,
            state_time: 1.125,
            aim_yaw: -0.19,
            aim_pitch: 0.22,
            getup_time: 0.4,
            dive_time: 0.55,
            max_fly_height: 150.,
            fuel: 0.73,
            ..Soldier::default()
        };
        s.collision_rule.threshold = [8., 2., 0.25];
        s.collision_rule.scale = [1.125, 0.25, 0.125];
        s.jet.jump = 6.5;
        s.jet.push = 9.25;
        s.jet.recharge = 0.075;
        s.jet.initial = 0.15;
        s.jet.cost = 0.35;
        s.jet.min_border = 0.19;
        s.jet.hover = matches!(self, Self::Hover | Self::StickHover);
        match self {
            Self::Hover | Self::Jet => {
                s.max_fly_height = 5.25;
            }
            Self::StickHover | Self::StickJet => {
                s.fuel = 0.25;
                s.jet.recharge = 0.;
                s.jet.cost = 0.51;
            }
            Self::RampAndWall => { s.position[2] = -1.; s.aim_yaw = 0.; }
            Self::LowCeiling => {
                s.position[2] = -6.;
                s.posture = BodyPosture::Crouch;
                s.aim_yaw = 0.;
            }
            Self::HardLanding => {
                s.position = [20., 85., -1.0];
                s.aim_yaw = 0.;
            }
            Self::Thrown => {
                s.position = [0., 15., -12.];
                s.velocity = [2.5, 3., 1.75];
                s.air = Some(BodyAir::Thrown);
                s.grounded = false;
            }
            _ => {}
        }
        s.settled = s.position;
        s
    }

    fn input(self, tick: usize) -> MoveInput {
        let mut i = MoveInput::default();
        match self {
            Self::Walk | Self::Crouch | Self::GetUp => {
                i.forward = 0.85;
                i.right = -0.67;
                i.turn = 0.31;
                i.pitch = -0.22;
                i.crouch = matches!(self, Self::Crouch) && tick == 0;
                i.prone = matches!(self, Self::GetUp) && (tick == 0 || tick == 25);
            }
            Self::Jump => {
                i.jump = tick == 0;
                i.forward = 0.56;
                i.turn = -0.23;
            }
            Self::Dive => {
                i.jump = tick == 0;
                i.right = -1.;
                i.turn = 0.27;
            }
            Self::Hover | Self::Jet | Self::StickHover | Self::StickJet => {
                i.jump = tick == 0 || tick == 7 || (matches!(self, Self::Hover) && tick == 55);
                i.forward = 0.73;
                i.right = 0.29;
                i.turn = 0.36;
                i.pitch = -0.13;
                if tick == 7 && matches!(self, Self::Hover | Self::Jet) {
                    i.jet_direction = Some([0.2, 0.9, -0.1]);
                }
            }
            Self::HardLanding => {
                if tick < 40 {
                    i.forward = 1.;
                }
            }
            Self::RampAndWall => i.forward = 1.,
            Self::LowCeiling => { i.forward = 1.; i.crouch = tick == 0 || tick == 130; }
            Self::Thrown => {
                i.right = 0.28;
                i.turn = -0.15;
            }
        }
        i
    }
}

fn class(s: &Soldier) -> [f64; 21] {
    let t = s.collision_rule.threshold;
    let d = s.collision_rule.scale;
    [
        s.max_speed,
        s.max_strafe,
        s.max_jump,
        s.max_turn,
        s.max_pitch,
        s.accel,
        s.pitch_limits[0],
        s.pitch_limits[1],
        s.jet.jump,
        s.jet.push,
        s.jet.recharge,
        s.jet.cost,
        s.jet.initial,
        s.jet.min_border,
        f64::from(s.jet.hover),
        t[0],
        t[1],
        t[2],
        d[0],
        d[1],
        d[2],
    ]
}

// Read the generated body independently of movement::store (the two exchange paths use store).
fn words(s: &Soldier, i: &MoveInput) -> [f64; IO_LEN] {
    let mut io = [0.; IO_LEN];
    for (at, xyz) in [
        (POSITION, s.position),
        (SETTLED, s.settled),
        (VELOCITY, s.velocity),
        (NORMAL, s.normal),
        (AVOIDANCE, s.avoidance),
    ] {
        io[at..at + 3].copy_from_slice(&xyz);
    }
    let posture = |p| match p {
        BodyPosture::Stand => 0.,
        BodyPosture::Crouch => 1.,
        BodyPosture::Prone => 2.,
    };
    io[YAW..=JET_AIMED].copy_from_slice(&[
        s.aim_yaw,
        s.aim_pitch,
        s.state_time,
        s.lock,
        s.fuel,
        s.dive,
        posture(s.posture),
        match s.air {
            None => 0.,
            Some(BodyAir::Jump) => 1.,
            Some(BodyAir::Fall) => 2.,
            Some(BodyAir::Thrown) => 3.,
            Some(BodyAir::Jet) => 4.,
        },
        posture(s.dive_from),
        f64::from(s.grounded),
        s.ground_adjustment,
        s.collision_damage,
        s.max_fly_height,
        s.getup_time,
        s.dive_time,
        -1.,
        H,
        i.forward,
        i.right,
        i.turn,
        i.pitch,
        f64::from(i.jump),
        f64::from(i.crouch),
        f64::from(i.prone),
        f64::from(i.jet_direction.is_some()),
    ]);
    if let Some(aim) = i.jet_direction {
        io[JET_DIRECTION..JET_DIRECTION + 3].copy_from_slice(&aim);
    }
    io[CLASS..CLASS + 21].copy_from_slice(&class(s));
    io
}

#[derive(Default)]
struct Observed {
    crouched: bool,
    prone: bool,
    locked: bool,
    jumped: bool,
    dove: bool,
    jetted: bool,
    jet_at_ceiling: bool,
    fell: bool,
    thrown: bool,
    damaged: bool,
    on_ramp: bool,
    stepped: bool,
    blocked_by_ceiling: bool,
}

fn trace(phase: Phase, count: usize, checkpoint: bool) -> Wide {
    let (world, mut collision) = arena(matches!(phase, Phase::RampAndWall | Phase::LowCeiling));
    let mut body = phase.soldier();
    let mut owner = MovementOwner::new();
    let mut exchange = MovementOwner::new();
    let mut native = MovementOwner::new();
    exchange.io_mut()[CLASS..CLASS + 21].copy_from_slice(&class(&body));
    let id = exchange.mover_new(world as u32);
    let handle = native.native_new_mover(world as u32, class(&body)).unwrap();
    let mut actor = Session::default();
    actor.configure(&[0.]).unwrap();
    actor.unit.soldier = UnitBody::Soldier(body.clone());

    let mut hash = Fnv::new();
    let mut seen = Observed::default();
    for tick in 0..count {
        if checkpoint && tick == 17 {
            let mut bytes = Vec::new();
            crate::client::capture(&UnitBody::Soldier(body), &mut bytes).unwrap();
            hash.bytes(&bytes);
            let mut restored = UnitBody::Soldier(phase.soldier());
            crate::client::restore(&mut restored, &bytes).unwrap();
            actor = Session::default();
            actor.configure(&[0.]).unwrap();
            actor.unit.soldier = restored.clone();
            let UnitBody::Soldier(restored) = restored else { unreachable!() };
            body = restored;
            owner = MovementOwner::new();
        }
        let input = phase.input(tick);
        let frame = words(&body, &input);
        *exchange.io_mut() = frame;
        assert_eq!(exchange.tick(&mut collision, id), 0);
        let result = native
            .native_tick(handle, frame, vec![], &mut collision)
            .unwrap();
        assert!(tick_soldier_in(
            &mut owner,
            &mut collision,
            &mut body,
            &input,
            H,
            world,
            -1,
            None
        )
        .unwrap());
        actor.unit.input = input.clone();
        assert!(actor
            .step(
                &mut collision,
                world,
                -1,
                &[],
                H,
                (0., V3::ZERO, false),
                true,
                false,
                false,
                false,
                false,
                false,
                0.,
                None
            )
            .unwrap());
        let UnitBody::Soldier(replay) = &actor.unit.soldier else {
            unreachable!()
        };
        let expected = words(&body, &input).map(f64::to_bits);
        for (source, actual) in [
            ("prediction", words(replay, &input)),
            ("exchange", *exchange.io_mut()),
            ("native exchange", result.io),
        ] {
            assert_eq!(
                actual.map(f64::to_bits),
                expected,
                "{phase:?} tick {tick} {source}"
            );
        }
        for word in expected {
            hash.bytes(&word.to_le_bytes());
        }
        seen.on_ramp |= body.grounded && body.normal[2] < -0.1 && body.position[1] > 0.2;
        seen.stepped |= body.grounded && body.position[2] > 6.2 && body.position[1] > 1.4;
        seen.blocked_by_ceiling |= tick == 0 && body.posture == BodyPosture::Crouch;
        seen.crouched |= body.posture == BodyPosture::Crouch;
        seen.prone |= body.posture == BodyPosture::Prone;
        seen.locked |= body.lock > 0.;
        seen.jumped |= body.air == Some(BodyAir::Jump);
        seen.dove |= body.dive != 0.;
        seen.jetted |= body.air == Some(BodyAir::Jet);
        seen.jet_at_ceiling |= body.air == Some(BodyAir::Jet) && body.position[1] > 2.0;
        seen.fell |= body.air == Some(BodyAir::Fall);
        seen.thrown |= body.air == Some(BodyAir::Thrown);
        seen.damaged |= body.collision_damage > 0.;
    }
    let covered = match phase {
        Phase::Walk => body.position[2] > 0. && body.grounded,
        Phase::Crouch => seen.crouched && body.posture == BodyPosture::Crouch,
        Phase::GetUp => {
            seen.prone && seen.locked && body.lock == 0. && body.posture == BodyPosture::Stand
        }
        Phase::Jump => seen.jumped && body.air.is_none(),
        Phase::Dive => seen.dove && body.dive == 0.,
        Phase::Hover => seen.jetted && seen.fell,
        Phase::Jet => seen.jet_at_ceiling && !body.jet.hover,
        Phase::HardLanding => {
            seen.fell && seen.locked && seen.damaged && body.grounded && body.position[1] < 1.
        }
        Phase::StickHover => seen.jetted && seen.fell && body.fuel == 0.,
        Phase::StickJet => seen.jetted && !body.jet.hover,
        Phase::RampAndWall => seen.on_ramp && seen.stepped && (9.0..10.).contains(&body.position[2]),
        Phase::LowCeiling => seen.blocked_by_ceiling && body.posture == BodyPosture::Stand,
        Phase::Thrown => seen.thrown && body.air.is_none(),
    };
    assert!(covered, "missing transition in {phase:?} at {:?} ramp={} step={} posture={:?}", body.position, seen.on_ramp, seen.stepped, body.posture);
    hash.finish()
}

#[test]
fn battlefront_movement_exchange_bits() {
    use Phase::*;
    for (phase, ticks, golden) in [
        (Walk, 80, Wide::new(0x89dd081e, 0x0d0cf1c8)),
        (Crouch, 65, Wide::new(0x97dbfdea, 0xc3ad3968)),
        (GetUp, 75, Wide::new(0x3978dac1, 0xe06dd71b)),
        (Jump, 110, Wide::new(0x5f28f69e, 0x2258377d)),
        (Dive, 80, Wide::new(0xd290194d, 0x88bd1e1d)),
        (Hover, 105, Wide::new(0xf329bfdd, 0x17976dc4)),
        (Jet, 120, Wide::new(0x878e8de7, 0xabe09fcd)),
        (HardLanding, 300, Wide::new(0x5979701c, 0xfeaf1821)),
        (Thrown, 150, Wide::new(0x64581717, 0xe1071c37)),
    ] {
        assert_eq!(trace(phase, ticks, false), golden, "{phase:?}");
    }
}


#[test]
fn battlefront_collision_jet_and_checkpoint_bits() {
    use Phase::*;
    for (phase, ticks, continuous, restored) in [
        (StickHover, 130, Wide::new(0x353ac40c, 0x737e7ef8), Wide::new(0x4aefd7b7, 0x64c82e5c)), (StickJet, 130, Wide::new(0x595a3a20, 0xaddc9f4c), Wide::new(0x54a2d5d7, 0xddc52985)),
        (RampAndWall, 160, Wide::new(0xa3eaddd7, 0x63ad8e6a), Wide::new(0x6f507c40, 0xf33fe996)), (LowCeiling, 155, Wide::new(0x2874b300, 0xe245330b), Wide::new(0x26654a5c, 0x4e964cc8)),
    ] {
        assert_eq!(trace(phase, ticks, false), continuous, "{phase:?}");
        assert_eq!(trace(phase, ticks, true), restored, "{phase:?} restored");
    }
}

#[test]
fn battlefront_delayed_online_reconciliation_bits() {
    use crate::client::{capture, ClientPrediction, FORWARD, LEFT, RIGHT, TURN_RIGHT};
    use crate::gen::{ClientAction, ClientCommand, ClientFrame, ClientResult};

    fn command(tick: usize) -> ClientCommand {
        let phase = tick % 120;
        let held = match phase {
            0..25 => FORWARD | TURN_RIGHT,
            25..50 => LEFT,
            50..79 => FORWARD,
            _ => RIGHT,
        };
        let actions = match phase {
            9 | 15 | 54 | 87 => 1 << ClientAction::Jump as u8,
            30 | 42 => 1 << ClientAction::Crouch as u8,
            61 | 72 => 1 << ClientAction::Prone as u8,
            _ => 0,
        };
        ClientCommand { tick: tick as f64, sequence: tick as f64, generation: 1.,
            held, actions, sent: true, ..Default::default() }
    }

    let (world, mut collision) = arena(true);
    let mut authority = Session::default();
    authority.configure(&[0.]).unwrap();
    authority.unit.soldier = UnitBody::Soldier(Phase::StickHover.soldier());
    let mut history = vec![authority.unit.soldier.clone()];
    let mut replay = ClientPrediction::default();
    replay.configure_actor(&[0.]).unwrap();
    let (mut delivered, mut continued, mut replayed, mut dropped_up) = (0, 0, 0, 0);
    let mut last_received = 0;
    let mut ack = 0;
    let mut held = 0;
    let mut hash = Fnv::new();
    for tick in 1usize..=600 {
        let cmd = command(tick);
        let dropped = tick % 29 == 0 || tick % 120 == 9;
        if dropped { dropped_up += 1; } else { held = cmd.held; }
        let pressed = |action: ClientAction| !dropped && cmd.actions & (1 << action as u8) != 0;
        authority.unit.input = MoveInput { forward: f64::from(held & FORWARD != 0),
            right: f64::from(held & RIGHT != 0) - f64::from(held & LEFT != 0),
            turn: f64::from(held & TURN_RIGHT != 0), jump: pressed(ClientAction::Jump),
            crouch: pressed(ClientAction::Crouch), prone: pressed(ClientAction::Prone),
            ..Default::default() };
        authority.step(&mut collision, world, -1, &[], H, (0., V3::ZERO, false), true,
            false, false, false, false, false, 0., None).unwrap();
        history.push(authority.unit.soldier.clone());

        // Snapshots arrive five ticks late with periodic loss and two blackouts.
        if tick > 5 && tick % 5 >= 2 && !(130..142).contains(&tick) && !(410..422).contains(&tick) {
            last_received = tick - 5;
            delivered += 1;
        }
        // Command acknowledgements lag the snapshot and can be lost independently.
        if tick % 3 != 0 { ack = last_received.saturating_sub(4); }
        let authority_body = &history[last_received];
        let UnitBody::Soldier(body) = authority_body else { unreachable!() };
        let mut words = vec![0.; ClientFrame::WORDS + Soldier::WORDS];
        body.put(&mut words[ClientFrame::WORDS..], &mut Vec::new());
        let mut bytes = Vec::new();
        capture(authority_body, &mut bytes).unwrap();
        let correction_at = words.len() as u32;
        for chunk in bytes.chunks(4) {
            let mut padded = [0; 4];
            padded[..chunk.len()].copy_from_slice(chunk);
            words.push(u32::from_le_bytes(padded).into());
        }
        let weapons_at = words.len() as u32;
        words.extend([0., 0., 0., 0., 0.]); // empty arsenal
        let commands_at = words.len() as u32;
        for n in ack + 1..=tick {
            let at = words.len();
            words.resize(at + ClientCommand::WORDS, 0.);
            command(n).put(&mut words[at..], &mut Vec::new());
        }
        let frame = ClientFrame { epoch: 1., identity: 1., generation: 1., source: 1.,
            movement: true, world: world as u32, ignored: -1., authority_tick: last_received as f64,
            check_tick: tick.saturating_sub(1) as f64, actor_at: ClientFrame::WORDS as u32,
            actor_words: Soldier::WORDS as u32, correction_at, correction_bytes: bytes.len() as u32,
            weapons_at, weapons_words: 5, commands_at, commands_count: (tick - ack) as u32,
            held: command(last_received).held, ..Default::default() };
        frame.put(&mut words, &mut Vec::new());
        replay.predict_session(&words, &mut collision).unwrap();
        let result = ClientResult::take(&replay.output, &[]);
        assert_eq!(result.tick, tick as f64);
        continued += usize::from(result.continued);
        replayed += if result.continued { 1 } else { tick - last_received };
        hash.bytes(&bytes);
        for x in &replay.output {
            hash.bytes(&x.to_le_bytes());
        }
    }
    assert!(delivered > 300 && continued > 200 && replayed > 1800 && dropped_up > 20);
    assert_eq!(hash.finish(), Wide::new(0xb730e90b, 0x411acabf), "delivered={delivered} continuations={continued} replayed={replayed} uplink_loss={dropped_up}");
}
