//! Drive the Battlefront II mover and present its resulting position and velocity.
use bfe_content::{
    actor::build_actor,
    bvh::{build, Positions},
    collision::MapCollision,
    install, json, zero_hash,
};
use bfe_content_native::{
    animation2::{
        self,
        player::{Arena, Player},
        state::{Air, Direction, State, Tables},
        Archive, Character, Class,
    },
    GameFamily, Loader,
};
use bfe_sim::{
    collision::CollisionOwner,
    movement::{
        soldier::{
            self,
            swbf2::{Actions, ControlSpeed, RollDirection, Step, Tuning},
            Input,
        },
        MovementOwner,
    },
};
use serde_json::json;

const DT: f64 = 1. / 60.;

fn floor() -> (CollisionOwner, usize) {
    let indices: Vec<u8> = [0u32, 2, 1, 0, 3, 2]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let points = |y: f32| {
        [
            -256., y, -256., 256., y, -256., 256., y, 256., -256., y, 256.,
        ]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>()
    };
    let polygons: Vec<_> = [0u32, 1].into_iter().flat_map(u32::to_le_bytes).collect();
    let map =
        MapCollision::load(&points(-12.), &indices, &polygons, &points(0.), &indices).unwrap();
    let buildings = build(Positions::F32(&map.positions), &map.indices);
    let terrain = build(Positions::F32(&map.terrain_positions), &map.terrain_indices);
    let mut owner = CollisionOwner::new();
    let world = install::install(&mut owner, &map, &buildings, Some(&terrain))
        .unwrap()
        .world;
    (owner, world as usize)
}

struct Trial {
    movement: MovementOwner,
    collision: CollisionOwner,
    id: u32,
    tuning: Tuning,
    life: soldier::swbf2::State,
}

impl Trial {
    fn new(def: &bfe_content::Value, frames: u32, yaw: f64) -> Self {
        let properties = def
            .get("properties")
            .and_then(bfe_content::Value::as_array)
            .unwrap_or(&[]);
        let mut rows = properties
            .iter()
            .filter_map(|v| {
                let r = v.as_array()?;
                Some((r.first()?.as_str()?, r.get(1)?.as_str()?))
            })
            .collect::<Vec<_>>();
        rows.reverse(); // Derived properties override their inherited values.
        let number = |key, fallback| {
            rows.iter()
                .find(|r| r.0 == key)
                .and_then(|r| r.1.parse().ok())
                .unwrap_or(fallback)
        };
        let control = |stance: &str| {
            rows.iter()
                .filter(|r| r.0 == "ControlSpeed")
                .find_map(|r| {
                    let mut fields = r.1.split_whitespace();
                    if fields.next()? != stance {
                        return None;
                    }
                    Some(ControlSpeed {
                        forward: fields.next()?.parse().ok()?,
                        strafe: fields.next()?.parse().ok()?,
                        turn: fields.next()?.parse().ok()?,
                    })
                })
                .unwrap_or(ControlSpeed {
                    forward: 1.,
                    strafe: 1.,
                    turn: 1.,
                })
        };
        let tuning = Tuning {
            stand: control("stand"),
            crouch: control("crouch"),
            sprint: control("sprint"),
            jump: control("jump"),
            jet: control("jet"),
            jetpack: None,
            roll: control("roll"),
            tumble: control("tumble"),
            sprint_accelerate_time: number("SprintAccelerateTime", 0.2),
            energy_capacity: number("EnergyBar", 50.),
            energy_floor: 0.,
            energy_restore: number("EnergyRestore", 5.),
            energy_restore_idle: number("EnergyRestoreIdle", 7.),
            energy_drain_sprint: number("EnergyDrainSprint", 4.),
            energy_min_sprint: number("EnergyMinSprint", 2.),
            energy_cost_jump: number("EnergyCostJump", 1.),
            energy_cost_jump_sprint: None,
            energy_cost_roll: number("EnergyCostRoll", 3.),
            jump_height: number("JumpHeight", 2.),
            jump_forward_speed_factor: number("JumpForwardSpeedFactor", 1.),
            jump_strafe_speed_factor: number("JumpStrafeSpeedFactor", 1.),
            roll_speed_factor: number("RollSpeedFactor", 1.2),
            roll_frames: frames,
            recover_from_tumble: false,
            getup_front_seconds: bfe_sim::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: bfe_sim::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
        };
        let (collision, world) = floor();
        let mut movement = MovementOwner::new();
        movement.select_family(GameFamily::Swbf2).unwrap();
        let id = movement.new_mover(world, |m| {
            m.grounded = true;
            m.max_speed = number("MaxSpeed", 4.);
            m.max_strafe = number("MaxStrafeSpeed", 3.);
            m.accel = number("Acceleration", m.accel);
            m.aim_yaw = yaw;
        });
        Self {
            movement,
            collision,
            id,
            life: soldier::swbf2::State::new(&tuning),
            tuning,
        }
    }
    fn advance(&mut self, tick: u32, input: Input, actions: Actions) -> State {
        self.movement
            .advance_profiled(
                &mut self.collision.state,
                self.id,
                &input,
                DT,
                Some(Step {
                    tuning: &self.tuning,
                    state: &mut self.life,
                    actions,
                    override_: None,
                }),
            )
            .unwrap();
        let mover = self.movement.mover(self.id);
        State {
            tick,
            position: mover.position.arr(),
            velocity: mover.velocity.arr(),
            yaw: mover.aim_yaw,
            crouch: mover.posture == soldier::Posture::Crouch,
            sprint: self.life.sprint_time.is_some(),
            roll: self.life.roll.map(|roll| match roll.direction {
                RollDirection::Forward => Direction::Forward,
                RollDirection::Left => Direction::Left,
                RollDirection::Right => Direction::Right,
            }),
            air: match mover.air {
                soldier::Air::None => Air::Ground,
                soldier::Air::Jump => Air::Jump,
                soldier::Air::Jet => Air::Jet,
                _ => Air::Fall,
            },
            phase_time: Some(self.life.roll.map_or(mover.state_time, |r| r.elapsed)),
            ..State::default()
        }
    }
}

// During contact the test foot moves back two metres relative to the model
// as DummyRoot travels two metres forward. World-space planting tests both the
// clip choice and the accumulated gait clock independently of the classifier.
fn stride_fixture(scale: f64) -> (Class, bfe_content::Value) {
    let identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    // The mesh bones give the contact marker its drawn leg length.
    let mut leg = identity.map(f64::from);
    for axis in [0, 5, 10] {
        leg[axis] = scale;
    }
    let mesh = json::parse(&json!({"bones":[{"name":"DummyRoot","parent":-1,"local":identity},{"name":"shoe","parent":2,"local":identity},{"name":"bone_l_thigh","parent":0,"local":leg},{"name":"bone_r_thigh","parent":0,"local":leg},{"name":"bone_a_spine","parent":0,"local":identity}],"parts":[]}).to_string()).unwrap();
    let actor = build_actor(&mesh, &[]).unwrap();
    let name = "umber_test_stand_runforward_full";
    let stride = |name| json!({"crc":zero_hash::crc_hex(name),"tracks":[zero_hash::crc_hex("DummyRoot"),zero_hash::crc_hex("shoe")],"flags":1,"offset":0,"frames":3});
    let upper = |name, offset| json!({"crc":zero_hash::crc_hex(name),"tracks":[zero_hash::crc_hex("bone_a_spine"),zero_hash::crc_hex("bone_l_thigh")],"flags":0,"offset":offset,"frames":3});
    let data = json!({"fps":2,"banks":{"umber":{"clips":[stride(name), stride("umber_test_diveforward"), stride("umber_tool_stand_runforward"), upper("umber_pistol_stand_runforward",42), stride("umber_tool_sprint"), upper("umber_pistol_sprint",84)]}}});
    let meta = json::parse(&data.to_string()).unwrap();
    // Root and shoe translate. The spine and thigh keep their bind lengths,
    // so their independent ownership is visible in their rotations.
    let flat = (0_f32, 1_f32, 0_f32);
    let q = std::f32::consts::FRAC_1_SQRT_2;
    let turn = (q, q, 0.);
    let tracks = [
        [flat, (0., 1., 1.), (0., 1., 2.)],
        [flat, (0., 1., -1.), (0., 1., -2.)],
        [flat, turn, flat],
        [turn; 3],
        [flat, (-q, q, 0.), flat],
        [flat, turn, flat],
    ];
    let bytes: Vec<_> = tracks
        .into_iter()
        .flatten()
        .flat_map(|(y, w, z)| [0., y, 0., w, 0., 0., z])
        .flat_map(f32::to_le_bytes)
        .collect();
    let archive = Archive::new(&meta, &bytes).unwrap();
    let def = json::parse(
        &json!({"MaxSpeed":4,"SkeletonRootScale":scale,"weapons":[{"AnimationBank":"test"},{"AnimationBank":"pistol"}]}).to_string(),
    )
    .unwrap();
    let character = Character::bind(
        "traveler",
        &def,
        &["umber".into()],
        &actor,
        &archive,
        &archive,
    )
    .unwrap();
    let tables = Tables::new(
        &character,
        &def,
        &bfe_content::Value::Null,
        &bfe_content::Value::Null,
    )
    .unwrap();
    let class = Class {
        side: String::new(),
        name: "traveler".into(),
        model: String::new(),
        animation: character,
        tables,
    };
    (class, def)
}

#[test]
fn partial_stance_keeps_its_upper_channels_over_the_parent_stride() {
    let (c, _) = stride_fixture(1.);
    let mut arena = Arena::new(1, 5).unwrap();
    let mut pose = arena.slot(0, 5);
    for (sprint, child, parent, chest_x) in [
        (
            false,
            "umber_pistol_stand_runforward",
            "umber_tool_stand_runforward",
            1.,
        ),
        (true, "umber_pistol_sprint", "umber_tool_sprint", -1.),
    ] {
        let mut player = Player::default();
        for tick in 0..=30 {
            let s = State {
                tick,
                weapon: 1,
                sprint,
                velocity: [0., 0., 2.],
                ..Default::default()
            };
            let row = c.tables.sample(&c.animation, &s, 0.).motion;
            assert_eq!(row.lower, c.animation.named_clip(parent));
            assert_eq!(row.upper, c.animation.named_clip(child));
            player.publish(&c, s, &mut pose);
        }
        player.place_at(&c, 30., false, &mut pose);
        assert!(
            (pose.world[4].elements[8] - chest_x).abs() < 1e-6,
            "{child} chest {:?}",
            pose.world[4].elements
        );
        assert!((pose.world[2].elements[0] - 1.).abs() < 1e-9);
        assert!(pose.world[2].elements[8].abs() < 1e-9);
        assert!((pose.world[1].elements[14] + 1.).abs() < 1e-9);
    }
}

#[test]
fn gaits_without_a_reference_use_the_drawn_skeleton_scale() {
    for scale in [0.5, 1.5] {
        let (class, def) = stride_fixture(scale);
        let mut trial = Trial::new(&def, 10, 0.);
        let mut arena = Arena::new(1, 5).unwrap();
        let mut pose = arena.slot(0, 5);
        let mut player = Player::default();
        let mut anchor = None;
        for tick in 0..50 {
            let s = trial.advance(
                tick,
                Input {
                    forward: 1.,
                    ..Input::default()
                },
                Actions::default(),
            );
            if tick < 40 {
                continue;
            }
            player.publish(&class, s, &mut pose);
            player.place_at(&class, f64::from(tick), false, &mut pose);
            let foot = pose.world[1].elements[14];
            assert!(
                (foot - *anchor.get_or_insert(foot)).abs() < 1e-8,
                "scale {scale}, tick {tick}, foot {foot}, anchor {anchor:?}"
            );
        }
    }
}

#[test]
fn each_gait_uses_its_own_banks_reference_skeleton() {
    let root = zero_hash::crc_hex("DummyRoot");
    let body = zero_hash::crc_hex("bone_root");
    let package = |owner: &str, bank: &str, stance: &str, height: f64| {
        let gait = zero_hash::crc_hex(&format!("{bank}_{stance}_stand_runforward_full"));
        let meta = json!({
            "fps": 1,
            "packages": {owner: {"banks": [bank], "skeletons": [bank]}},
            "banks": {bank: {"clips": [{
                "crc": gait, "frames": 2, "flags": 1, "offset": 0, "tracks": [root]
            }]}},
            "skeletons": {bank: {"joints": [{
                "boneCrc": body, "parentCrc": root, "position": [0, height, 0], "rotation": [0, 0, 0, 1]
            }]}}
        });
        json::parse(&meta.to_string()).unwrap()
    };
    let own = package(&zero_hash::fnv1a_hex("weaver"), "reed", "coil", 1.875);
    let shared = package("root", "linen", "weave", 1.25);
    let bytes: Vec<_> = [0_f32, 0., 0., 1., 0., 0., 0., 0., 0., 0., 1., 0., 0., 3.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let side = Archive::new(&own, &bytes).unwrap();
    let common = Archive::new(&shared, &bytes).unwrap();
    for (height, scale, factors) in [
        (1.875, 1.5, [1., 1.5]),
        (0.9375, 0.5, [0.5, 0.5]),
        (1.25, 0.5, [0.5, 1.]),
    ] {
        let mut local = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
        let origin = local;
        local[13] = height;
        let mesh = json!({"bones": [
            {"name":"DummyRoot", "parent":-1, "local":origin},
            {"name":"bone_root", "parent":0, "local":local}
        ], "parts":[]});
        let actor = build_actor(&json::parse(&mesh.to_string()).unwrap(), &[]).unwrap();
        let def = json!({
            "SkeletonRootScale": scale,
            "weapons": [{"AnimationBank":"coil"}, {"AnimationBank":"weave"}]
        });
        let def = json::parse(&def.to_string()).unwrap();
        let c = Character::bind(
            "weaver", &def, &["linen".into(), "reed".into()], &actor, &side, &common,
        )
        .unwrap();
        let empty = bfe_content::Value::Null;
        let tables = Tables::new(&c, &def, &empty, &empty).unwrap();
        for (weapon, factor) in factors.into_iter().enumerate() {
            let state = State {
                weapon,
                velocity: [0., 0., 6.],
                ..Default::default()
            };
            let row = tables.sample(&c, &state, 0.);
            let clip = &c.clips[row.motion.lower.or(row.motion.base).unwrap()];
            assert_eq!(c.banks[clip.bank], ["reed", "linen"][weapon]);
            // Each clip travels three metres in one second before retargeting.
            assert_eq!(row.rate, 2. / factor);
        }
    }
}

#[test]
fn all_eight_inputs_plant_the_contact_foot_and_keep_the_upper_body_on_aim() {
    let (class, def) = stride_fixture(1.);
    let name = "umber_test_stand_runforward_full";
    for yaw in [0., 1.1, -1.7] {
        for (forward, right) in [
            (1., 0.),
            (-1., 0.),
            (0., 1.),
            (0., -1.),
            (1., 1.),
            (1., -1.),
            (-1., 1.),
            (-1., -1.),
        ] {
            let mut trial = Trial::new(&def, 10, yaw);
            let mut player = Player::default();
            let mut arena = Arena::new(1, 5).unwrap();
            let mut pose = arena.slot(0, 5);
            let input = Input {
                forward,
                right,
                ..Default::default()
            };
            for tick in 0..40 {
                trial.advance(tick, input, Actions::default());
            }
            let mut anchor = None;
            for tick in 40..64 {
                let s = trial.advance(tick, input, Actions::default());
                let sample = class.tables.sample(&class.animation, &s, 0.);
                assert_eq!(
                    class.animation.clips[sample.motion.base.unwrap()].crc,
                    zero_hash::crc(name)
                );
                player.publish(&class, s, &mut pose);
                player.place_at(&class, tick.into(), false, &mut pose);
                if tick >= 43 {
                    let foot = pose.world[1].elements;
                    let first = *anchor.get_or_insert([foot[12], foot[14]]);
                    assert!(
                        (first[0] - foot[12]).hypot(first[1] - foot[14]) < 1e-8,
                        "input {forward}/{right} at {yaw}, tick {tick}"
                    );
                    let chest = pose.world[4].elements;
                    assert!((chest[8] - yaw.sin()).hypot(chest[10] - yaw.cos()) < 1e-8);
                }
            }
        }
        for (forward, right, expected) in [
            (-1., 0., Direction::Backward),
            (0., 1., Direction::Right),
            (0., -1., Direction::Left),
        ] {
            let mut trial = Trial::new(&def, 10, yaw);
            let mut s = State::default();
            for tick in 0..40 {
                s = trial.advance(
                    tick,
                    Input {
                        forward,
                        right,
                        ..Input::default()
                    },
                    Actions::default(),
                );
            }
            assert_eq!(Direction::from_velocity(s.velocity, s.yaw), expected);
        }
    }
}

#[test]
#[ignore = "requires a prepared character and mission"]
fn prepared_motion_from_inputs() {
    let root = std::env::var("BFE_SWBF2_CONTENT").unwrap();
    let query = std::env::var("BFE_MOTION_MISSION").unwrap();
    let name = std::env::var("BFE_MOTION_CLASS").unwrap();
    let map = bfe_content::mission::Query::parse(&query);
    let mut loader = Loader::detect(&root);
    loader.compressed = false;
    let battle = loader
        .load_for_family(GameFamily::Swbf2, map.get("map"), &query)
        .unwrap();
    let c = animation2::admit(&battle)
        .unwrap()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap();
    let def = battle
        .content
        .battle
        .sides
        .iter()
        .find(|s| s.name == c.side)
        .unwrap()
        .units
        .get(&name)
        .unwrap();
    let roll = c
        .tables
        .sample(
            &c.animation,
            &State {
                roll: Some(Direction::Forward),
                ..State::default()
            },
            0.,
        )
        .motion;
    let frames = c.animation.clips[roll.lower.or(roll.base).unwrap()].frames as u32;
    for mode in ["walk", "run", "sprint", "roll", "jump"] {
        let mut trial = Trial::new(def, frames, 0.83);
        for tick in 0..115 {
            let previous = trial.movement.mover(trial.id).position;
            let s = trial.advance(
                tick,
                Input {
                    forward: if mode == "walk" { 0.32 } else { 1. },
                    jump: mode == "jump" && tick == 40,
                    ..Input::default()
                },
                Actions {
                    sprint: mode == "sprint" && tick >= 35,
                    roll: mode == "roll" && tick == 40,
                    ai: false,
                },
            );
            let row = c.tables.sample(&c.animation, &s, 0.);
            let clip = &c.animation.clips[row.motion.lower.or(row.motion.base).unwrap()];
            let stride_speed = clip.speed * clip.stride_scale;
            if tick == 89 && ["walk", "run", "sprint"].contains(&mode) {
                assert_eq!(
                    Direction::from_velocity(s.velocity, s.yaw),
                    Direction::Forward
                );
                assert!(clip.speed > 0., "{name} {mode} lacks authored gait travel");
                let speed = (s.position[0] - previous.x).hypot(s.position[2] - previous.z) / DT;
                assert!((row.rate * stride_speed - speed).abs() < 1e-9);
            }
            println!(
                "motion {} {}",
                mode,
                json!({"tick":tick,"position":s.position,"velocity":s.velocity,"yaw":s.yaw,"sprint":s.sprint,"roll":s.roll.map(|_|"forward"),"air":format!("{:?}",s.air).to_lowercase(),"phaseTime":s.phase_time,"clip":format!("{:08x}",clip.crc),"strideSpeed":stride_speed,"gaitRate":row.rate})
            );
        }
    }
}

#[test]
#[ignore = "requires a prepared firearm class and mission"]
fn running_weapon_actions_keep_the_contact_chain() {
    use bfe_content_native::animation2::state::Action;
    let root = std::env::var("BFE_SWBF2_CONTENT").unwrap();
    let query = std::env::var("BFE_MOTION_MISSION").unwrap();
    let name = std::env::var("BFE_MOTION_CLASS").unwrap();
    let map = bfe_content::mission::Query::parse(&query);
    let mut loader = Loader::detect(&root);
    loader.compressed = false;
    let battle = loader
        .load_for_family(GameFamily::Swbf2, map.get("map"), &query)
        .unwrap();
    let class = animation2::admit(&battle)
        .unwrap()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap();
    let def = battle
        .content
        .battle
        .sides
        .iter()
        .find(|s| s.name == class.side)
        .unwrap()
        .units
        .get(&name)
        .unwrap();
    let rig = &class.animation.rig;
    let legs = [
        "bone_root",
        "bone_pelvis",
        "bone_l_thigh",
        "bone_l_calf",
        "bone_l_foot",
        "bone_l_toe",
        "bone_r_thigh",
        "bone_r_calf",
        "bone_r_foot",
        "bone_r_toe",
    ]
    .map(|n| rig.joint(n).unwrap());
    for action in [Action::Fire, Action::Reload] {
        let mut trial = Trial::new(def, 1, -0.7);
        let mut arena = Arena::new(2, rig.joints.len()).unwrap();
        let mut players = [Player::default(), Player::default()];
        let mut changed = 0;
        for tick in 0..90 {
            let gait = trial.advance(
                tick,
                Input {
                    forward: 1.,
                    ..Default::default()
                },
                Actions::default(),
            );
            for (i, p) in players.iter_mut().enumerate() {
                let mut s = gait;
                if i == 1 && (30..75).contains(&tick) {
                    s.action = action;
                    s.elapsed = f64::from(tick - 30) / 60.;
                }
                let mut pose = arena.slot(i, rig.joints.len());
                p.publish(&class, s, &mut pose);
                p.place_at(&class, f64::from(tick) - 0.5, false, &mut pose);
            }
            let plain = arena.slot(0, rig.joints.len()).world.to_vec();
            let layered = arena.slot(1, rig.joints.len());
            for &i in &legs {
                assert_eq!(
                    plain[i], layered.world[i],
                    "{name} {action:?} joint {i} tick {tick}"
                );
            }
            let hand = rig.joint("bone_r_hand").unwrap();
            changed += usize::from(plain[hand] != layered.world[hand]);
        }
        assert!(changed > 20);
        println!(
            "overlay {name} {action:?}: 900 matching leg matrices, {changed} distinct hand frames"
        );
    }
}

#[test]
#[ignore = "requires a prepared class with a root-driven gait"]
fn prepared_directions_follow_real_inputs() {
    let root = std::env::var("BFE_SWBF2_CONTENT").unwrap();
    let query = std::env::var("BFE_MOTION_MISSION").unwrap();
    let name = std::env::var("BFE_MOTION_CLASS").unwrap();
    let map = bfe_content::mission::Query::parse(&query);
    let mut loader = Loader::detect(&root);
    loader.compressed = false;
    let battle = loader
        .load_for_family(GameFamily::Swbf2, map.get("map"), &query)
        .unwrap();
    let c = animation2::admit(&battle)
        .unwrap()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap();
    let def = battle
        .content
        .battle
        .sides
        .iter()
        .find(|s| s.name == c.side)
        .unwrap()
        .units
        .get(&name)
        .unwrap();
    let rig = &c.animation.rig;
    let feet = ["bone_l_foot", "bone_r_foot"].map(|n| rig.joint(n).unwrap());
    for (mode, forward, right, crouch) in [
        ("forward", 1., 0., false),
        ("back", -1., 0., false),
        ("left", 0., -1., false),
        ("right", 0., 1., false),
        ("forward-left", 1., -1., false),
        ("forward-right", 1., 1., false),
        ("back-left", -1., -1., false),
        ("back-right", -1., 1., false),
        ("slow-back", -0.25, 0., false),
        ("slow-left", 0., -0.3, false),
        ("crouch-left", 0., -1., true),
    ] {
        let mut t = Trial::new(def, 1, 0.83);
        let mut p = Player::default();
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let mut max_slip = 0_f64;
        let mut trace = Vec::new();
        let mut contacts = Vec::new();
        let mut last = None;
        for tick in 0..150 {
            let s = t.advance(
                tick,
                Input {
                    forward,
                    right,
                    crouch: crouch && tick == 1,
                    ..Default::default()
                },
                Actions::default(),
            );
            if tick >= 2 {
                assert_eq!(s.crouch, crouch);
            }
            let sample = c.tables.sample(&c.animation, &s, 0.);
            let clip = &c.animation.clips[sample.motion.lower.or(sample.motion.base).unwrap()];
            p.publish(&c, s, &mut pose);
            p.place_at(&c, f64::from(tick), false, &mut pose);
            let points = feet.map(|i| {
                let e = &pose.world[i].elements;
                [e[12], e[13], e[14]]
            });
            if tick >= 45 {
                let (sin, cos) = (s.yaw + sample.travel_yaw).sin_cos();
                let [x, z] = clip
                    .travel
                    .map(|d| d * clip.stride_scale * sample.rate / clip.duration());
                max_slip = max_slip.max(
                    (x * cos + z * sin - s.velocity[0]).hypot(z * cos - x * sin - s.velocity[2]),
                );
                let prev: [[f64; 3]; 2] = last.unwrap();
                for (a, b) in prev.into_iter().zip(points) {
                    contacts.push(((a[1] + b[1]) * 0.5, (b[0] - a[0]).hypot(b[2] - a[2]) / DT));
                }
            }
            if (32..=58).contains(&tick) {
                trace.push(json!({"position":s.position,"velocity":s.velocity,"yaw":s.yaw,"tick":s.tick,"crouch":s.crouch}));
            }
            last = Some(points);
        }
        assert!(max_slip < 1e-7, "{name}/{mode} stride error {max_slip}");
        contacts.sort_by(|a, b| a.0.total_cmp(&b.0));
        contacts.truncate(contacts.len() / 4);
        contacts.sort_by(|a, b| a.1.total_cmp(&b.1));
        println!(
            "direction {name} {mode} stride_error={max_slip:.9} low_foot_median={:.4} m/s",
            contacts[contacts.len() / 2].1
        );
        if ["forward", "left", "forward-right"].contains(&mode) {
            println!(
                "input-trace {mode} {}",
                serde_json::to_string(&trace).unwrap()
            );
        }
    }
}

#[test]
fn displayed_hip_turns_are_bounded_through_starts_stops_and_reversals() {
    let (c, def) = stride_fixture(1.);
    for interval in [1, 3] {
        for fps in [60, 144, 500] {
            let mut trial = Trial::new(&def, 9, 0.);
            let mut player = Player::default();
            let mut arena = Arena::new(1, 5).unwrap();
            let mut pose = arena.slot(0, 5);
            let mut previous = 0.;
            let mut peak = 0_f64;
            let mut next_frame = 0u32;
            for tick in 0..360u32 {
                let (forward, right) = match tick {
                    0..30 | 95..135 => (0., 0.),
                    30..95 | 210..270 => (0., -1.),
                    135..210 => (0., 1.),
                    270..300 => (1., 0.),
                    _ => (-1., -1.),
                };
                let s = trial.advance(
                    tick,
                    Input {
                        forward,
                        right,
                        ..Default::default()
                    },
                    Actions::default(),
                );
                if !tick.is_multiple_of(interval) {
                    continue;
                }
                player.publish(&c, s, &mut pose);
                while next_frame * 60 <= tick * fps {
                    let shown = f64::from(next_frame * 60) / f64::from(fps);
                    player.place_at(&c, shown, false, &mut pose);
                    let hip = pose.world[0].elements;
                    let yaw = hip[8].atan2(hip[10]);
                    let delta = (yaw - previous).abs();
                    assert!(
                        delta <= 8. / f64::from(fps) + 1e-9,
                        "{fps} fps tick {shown}: {delta}"
                    );
                    assert!(yaw.abs() <= 72_f64.to_radians() + 1e-9);
                    peak = peak.max(delta);
                    previous = yaw;
                    next_frame += 1;
                }
            }
            assert!(peak > 6. / f64::from(fps));
            println!(
                "turn interval={interval} fps={fps} max_deg_per_frame={:.4}",
                peak.to_degrees()
            );
        }
    }
}

#[test]
fn a_dive_reaches_its_last_frame_in_the_published_window() {
    let (c, _) = stride_fixture(1.);
    let clip = c.animation.named_clip("umber_test_diveforward").unwrap();
    let duration = c.animation.clips[clip].duration();
    let window = 0.37;
    for (elapsed, fraction) in [
        (0., 0.),
        (window * 0.5, 0.5),
        (window, 1.),
        (window * 2., 1.),
    ] {
        let state = State {
            roll: Some(Direction::Left),
            roll_seconds: Some(window),
            phase_time: Some(elapsed),
            action: bfe_content_native::animation2::state::Action::Fire,
            ..State::default()
        };
        let pose = c.tables.sample(&c.animation, &state, 0.);
        assert_eq!(pose.motion.base, Some(clip));
        assert_eq!(pose.motion_time, fraction * duration);
        assert!(pose.action.is_none());
    }
}

#[test]
fn a_completed_dive_blends_out_from_its_endpoint_between_ticks() {
    let (mut class, def) = stride_fixture(1.);
    let settings = json::parse(r#"{"movement":[{"values":["diveforward"],"children":[{"key":"BlendOutTime","values":[0.2]}]}]}"#).unwrap();
    class.tables =
        Tables::new(&class.animation, &def, &settings, &bfe_content::Value::Null).unwrap();
    let shoe = class.animation.rig.joint("shoe").unwrap();
    let window = 0.37;
    let dive = State {
        tick: 8,
        roll: Some(Direction::Forward),
        roll_seconds: Some(window),
        phase_time: Some(window),
        ..Default::default()
    };
    let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
    let mut pose = arena.slot(0, class.animation.rig.joints.len());
    let mut reference = Player::default();
    reference.publish(&class, dive, &mut pose);
    reference.place_at(&class, 8., false, &mut pose);
    let endpoint = pose.world[shoe].elements[14];
    assert!(endpoint.abs() > 1.);

    let mut player = Player::default();
    player.publish(
        &class,
        State {
            phase_time: Some(window - 0.5 * DT),
            ..dive
        },
        &mut pose,
    );
    player.publish(
        &class,
        State {
            tick: 9,
            ..Default::default()
        },
        &mut pose,
    );
    player.place_at(&class, 9., false, &mut pose);
    assert!((pose.world[shoe].elements[14] - endpoint).abs() < 1e-9);
    player.publish(
        &class,
        State {
            tick: 22,
            ..Default::default()
        },
        &mut pose,
    );
    player.place_at(&class, 22., false, &mut pose);
    assert!(pose.world[shoe].elements[14].abs() < 1e-9);
}

#[test]
fn a_side_dive_turns_from_the_strafe_to_its_heading_once() {
    let (class, def) = stride_fixture(1.);
    for side in [-1., 1.] {
        let mut trial = Trial::new(&def, 47, 0.);
        let mut player = Player::default();
        let mut arena = Arena::new(1, 5).unwrap();
        let mut pose = arena.slot(0, 5);
        let mut peak = 0_f64;
        for tick in 0..130 {
            let state = trial.advance(
                tick,
                Input {
                    right: side,
                    ..Input::default()
                },
                Actions {
                    roll: tick == 60,
                    ..Actions::default()
                },
            );
            player.publish(&class, state, &mut pose);
            player.place_at(&class, f64::from(tick), false, &mut pose);
            if state.roll.is_some() {
                let e = pose.world[0].elements;
                let toward_dive = -side * e[8].atan2(e[10]);
                assert!(
                    (0. ..=std::f64::consts::FRAC_PI_2 + 1e-9).contains(&toward_dive),
                    "tick {tick}, heading {}",
                    toward_dive.to_degrees()
                );
                peak = peak.max(toward_dive);
            }
        }
        assert!(peak.to_degrees() > 89.);
    }
}

/// Contact-window median and count. The slower-ankle median also measures
/// airborne sprint phases where neither foot stays low for four ticks.
fn contact_slide(feet: &[[[f64; 3]; 2]]) -> (Option<f64>, usize, f64) {
    let mut speeds = Vec::new();
    for ankle in 0..2 {
        let lowest = feet
            .iter()
            .map(|p| p[ankle][1])
            .fold(f64::INFINITY, f64::min);
        let mut start = None;
        for i in 0..=feet.len() {
            if feet.get(i).is_some_and(|p| p[ankle][1] <= lowest + 0.03) {
                start.get_or_insert(i);
            } else if let Some(j) = start.take() {
                if i - j >= 4 {
                    let a = feet[j][ankle];
                    let b = feet[i - 1][ankle];
                    speeds.push((b[0] - a[0]).hypot(b[2] - a[2]) / ((i - j - 1) as f64 * DT));
                }
            }
        }
    }
    speeds.sort_by(f64::total_cmp);
    let mut slower: Vec<_> = feet
        .windows(2)
        .map(|w| {
            let speed = |ankle: usize| {
                let [a, b] = [w[0][ankle], w[1][ankle]];
                (b[0] - a[0]).hypot(b[2] - a[2]) / DT
            };
            speed(0).min(speed(1))
        })
        .collect();
    slower.sort_by(f64::total_cmp);
    (
        speeds.get(speeds.len() / 2).copied(),
        speeds.len(),
        slower[slower.len() / 2],
    )
}

#[test]
#[ignore = "requires prepared character rosters"]
fn roster_ground_contacts_follow_real_inputs() {
    let root = std::env::var("BFE_SWBF2_CONTENT").unwrap();
    let queries = std::env::var("BFE_CHARACTER_MISSIONS").unwrap();
    for query in queries.split(';') {
        let map = bfe_content::mission::Query::parse(query);
        let mut loader = Loader::detect(&root);
        loader.compressed = false;
        let battle = loader
            .load_for_family(GameFamily::Swbf2, map.get("map"), query)
            .unwrap();
        for c in animation2::admit(&battle).unwrap() {
            let def = battle
                .content
                .battle
                .sides
                .iter()
                .find(|s| s.name == c.side)
                .unwrap()
                .units
                .get(&c.name)
                .unwrap();
            let rig = &c.animation.rig;
            let ankles = ["bone_l_foot", "bone_r_foot"].map(|n| rig.joint(n).unwrap());
            let mut cases = Vec::new();
            for crouched in [false, true] {
                for (mode, magnitude) in [("walk", 0.32), ("run", 1.), ("sprint", 1.)] {
                    for (dir, f, r) in [
                        ("F", 1_f64, 0_f64),
                        ("B", -1., 0.),
                        ("L", 0., -1.),
                        ("R", 0., 1.),
                        ("FL", 1., -1.),
                        ("FR", 1., 1.),
                        ("BL", -1., -1.),
                        ("BR", -1., 1.),
                    ] {
                        let scale = magnitude / f.hypot(r);
                        let mut trial = Trial::new(def, 1, 0.3);
                        let mut player = Player::default();
                        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
                        let mut pose = arena.slot(0, rig.joints.len());
                        let mut feet = Vec::new();
                        let mut speed = 0.;
                        let mut sprint_ticks = 0;
                        for tick in 0..240 {
                            let s = trial.advance(
                                tick,
                                Input {
                                    forward: if tick >= 8 { scale * f } else { 0. },
                                    right: if tick >= 8 { scale * r } else { 0. },
                                    crouch: crouched && tick == 4,
                                    ..Default::default()
                                },
                                Actions {
                                    sprint: mode == "sprint" && tick >= 20,
                                    roll: false,
                                    ai: false,
                                },
                            );
                            player.publish(&c, s, &mut pose);
                            player.place_at(&c, f64::from(tick), false, &mut pose);
                            if tick < 90 {
                                continue;
                            }
                            assert_eq!(s.crouch, crouched);
                            assert_eq!(
                                s.air,
                                Air::Ground,
                                "{} {mode} {dir} left the floor at {tick}",
                                c.name
                            );
                            let sample = c.tables.sample(&c.animation, &s, 0.);
                            let stride = &c.animation.clips[sample
                                .motion
                                .lower
                                .or(sample.motion.base)
                                .unwrap_or_else(|| {
                                    panic!(
                                        "{} {mode} {dir} crouch={crouched} lacks a stride",
                                        c.name
                                    )
                                })];
                            assert!(stride.moves_legs(rig) && stride.speed > 0., "{} {mode} {dir} crouch={crouched} lacks moving legs or authored travel",c.name);
                            sprint_ticks += usize::from(s.sprint);
                            speed += s.velocity[0].hypot(s.velocity[2]);
                            feet.push(ankles.map(|i| {
                                let e = pose.world[i].elements;
                                [e[12], e[13], e[14]]
                            }));
                        }
                        let (slide, contacts, slower) = contact_slide(&feet);
                        cases.push(json!([
                            crouched,
                            mode,
                            dir,
                            speed / feet.len() as f64,
                            sprint_ticks,
                            slide,
                            contacts,
                            slower
                        ]));
                    }
                }
            }
            println!("ground {} {}", c.name, json!(cases));
        }
    }
}
