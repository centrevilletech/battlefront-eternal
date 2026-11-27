use bfe_content::{actor::build_actor, json, zero_hash};
use bfe_content_native::animation2::{Archive, Character, Local, Mask};
use serde_json::json;

use bfe_test_alloc::Counting;
#[global_allocator]
static ALLOCATOR: Counting = Counting::new();

#[test]
fn package_precedence_and_an_animation_only_child() {
    let mesh=json::parse(r#"{"bones":[{"name":"tower","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#).unwrap();
    let actor = build_actor(&mesh, &[]).unwrap();
    let root = zero_hash::crc_hex("tower");
    let hand = zero_hash::crc_hex("lantern");
    let id = zero_hash::crc_hex("sweep");
    let data = json!({"fps":12,"banks":{"jade":{"clips":[{"crc":id,"tracks":[hand],"offset":0,"flags":0,"frames":2}]},
        "umber":{"clips":[{"crc":id,"tracks":[root],"offset":0,"flags":0,"frames":2}]}},
        "packages":{zero_hash::fnv1a_hex("spinner"):{"banks":["jade","umber"],"skeletons":["upright"]}},
        "skeletons":{"upright":{"joints":[{"boneCrc":hand,"parentCrc":root,"position":[1,2,3],"rotation":[0,0,0,1]}]}}});
    let side_meta = json::parse(&data.to_string()).unwrap();
    let bytes: Vec<_> = [0f32, 0., 0., 1., 2., 3., 4., 0., 0., 0., 1., 6., 5., 8.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let side = Archive::new(&side_meta, &bytes).unwrap();
    let meta = json::parse(r#"{"fps":17,"banks":{},"packages":{}}"#).unwrap();
    let common = Archive::new(&meta, &[]).unwrap();
    let def = json::parse(r#"{"AnimationName":"umber"}"#).unwrap();
    let c = Character::bind(
        "spinner",
        &def,
        &["jade".into(), "umber".into()],
        &actor,
        &side,
        &common,
    )
    .unwrap();
    assert_eq!(c.banks, ["umber", "jade"]);
    let binding = |bank: &str, frames: usize| {
        json::parse(
            &json!({"clipTiming":{"turn":{"bank":bank,"crc":id,"frames":frames,"fps":12}}})
                .to_string(),
        )
        .unwrap()
    };
    assert_eq!(
        c.weapon_clip(&binding("umber", 2), "turn").unwrap(),
        c.clip("umber", zero_hash::crc("sweep")).unwrap()
    );
    assert!(c.weapon_clip(&binding("elsewhere", 2), "turn").is_err());
    assert!(c.weapon_clip(&binding("jade", 3), "turn").is_err());
    assert!(c.weapon_clip(&binding("jade", 2), "another").is_err());
    let i = c.rig.joint("lantern").unwrap();
    assert_eq!(c.rig.joints[i].parent, c.rig.joint("tower"));
    let clip = c.clip("jade", zero_hash::crc("sweep")).unwrap();
    let mut out = vec![Local::default(); c.rig.joints.len()];
    c.clips[clip].apply(&c.rig, 1. / 24., false, Mask::All, &mut out);
    assert_eq!(
        [out[i].position.x, out[i].position.y, out[i].position.z],
        [4., 4., 6.]
    );
    let mut invalid = data.clone();
    invalid["banks"]["jade"]["clips"][0]["tracks"] = json!(["12345678"]);
    let invalid = json::parse(&invalid.to_string()).unwrap();
    let archive = Archive::new(&invalid, &bytes).unwrap();
    assert!(Character::bind(
        "spinner",
        &def,
        &["jade".into(), "umber".into()],
        &actor,
        &archive,
        &common
    )
    .err()
    .unwrap()
    .contains("unbound joint"));
}

#[test]
#[ignore]
fn prepared_character_bindings() {
    use bfe_content_native::{animation2, GameFamily, Loader};
    let root = std::env::var("BFE_SWBF2_CONTENT").unwrap();
    let missions = std::env::var("BFE_CHARACTER_MISSIONS").unwrap();
    let seat_clip = std::env::var("BFE_CHARACTER_SEAT_CLIP").ok();
    let mut seated_classes = 0;
    let mut coverage = Vec::new();
    for query in missions.split(';') {
        let map = bfe_content::mission::Query::parse(query);
        let mut loader = Loader::detect(&root);
        loader.compressed = false;
        let content = loader
            .load_for_family(GameFamily::Swbf2, map.get("map"), query)
            .unwrap();
        let classes = animation2::admit(&content).unwrap();
        assert!(!classes.is_empty());
        for c in classes {
            let key = format!("{}/{}", c.side, c.name);
            let (_, declared) = content
                .content
                .battle
                .swbf2
                .as_ref()
                .unwrap()
                .unit_anim_banks
                .iter()
                .find(|(k, _)| *k == key)
                .unwrap();
            assert!(c.animation.banks.iter().eq(declared.iter().rev()));
            assert!(!c.animation.clips.is_empty());
            let unit = content
                .content
                .battle
                .sides
                .iter()
                .find(|s| s.name == c.side)
                .unwrap()
                .units
                .get(&c.name)
                .unwrap();
            use animation2::state::{
                Action as Reaction, Direction as Bearing, State as DisplayState,
            };
            let sample = |state| c.tables.sample(&c.animation, &state, 0.);
            let describe = |row: Option<animation2::state::Row>| {
                row.and_then(|r| r.base.or(r.lower).or(r.upper)).map(|i| {
                    let clip = &c.animation.clips[i];
                    format!(
                        "{}/{:08x}/{}",
                        c.animation.banks[clip.bank], clip.crc, clip.frames
                    )
                })
            };
            let idle = sample(DisplayState::default()).motion;
            let roll = sample(DisplayState {
                roll: Some(Bearing::Forward),
                ..Default::default()
            })
            .motion;
            let hits = [
                Bearing::Forward,
                Bearing::Backward,
                Bearing::Left,
                Bearing::Right,
            ]
            .map(|d| {
                describe(
                    sample(DisplayState {
                        action: Reaction::Hit,
                        impact: Some(d),
                        ..Default::default()
                    })
                    .action,
                )
            });
            let fall = describe(
                sample(DisplayState {
                    action: Reaction::Death,
                    ..Default::default()
                })
                .action,
            );
            assert_ne!(roll, idle, "{key} lacks an evasion");
            assert!(fall.is_some(), "{key} lacks a defeat");
            assert!(
                hits.iter().all(Option::is_some),
                "{key} lacks a directional reaction"
            );
            coverage.push(format!(
                "coverage {key} rig={} evasion={} death={fall:?} hits={hits:?}",
                c.animation.rig.joints.len(),
                if roll == idle {
                    "absent".into()
                } else {
                    describe(Some(roll)).unwrap_or_default()
                }
            ));
            for (weapon, record) in unit
                .get("weapons")
                .and_then(bfe_content::Value::as_array)
                .unwrap_or(&[])
                .iter()
                .enumerate()
            {
                if record
                    .get("saberHilts")
                    .and_then(bfe_content::Value::as_array)
                    .is_some_and(|s| !s.is_empty())
                {
                    for action in [
                        animation2::state::Action::Fire,
                        animation2::state::Action::Reload,
                    ] {
                        let s = animation2::state::State {
                            weapon,
                            action,
                            ..Default::default()
                        };
                        assert!(
                            c.tables.sample(&c.animation, &s, 0.).action.is_none(),
                            "{key}: gun pose on a saber slot"
                        );
                    }
                }
            }
            let mut timings = 0;
            for weapon in unit
                .get("weapons")
                .and_then(bfe_content::Value::as_array)
                .unwrap_or(&[])
            {
                for b in weapon
                    .get("comboBindings")
                    .and_then(bfe_content::Value::as_array)
                    .unwrap_or(&[])
                {
                    for (action, t) in b
                        .get("clipTiming")
                        .and_then(bfe_content::Value::as_object)
                        .into_iter()
                        .flat_map(|o| o.entries())
                    {
                        let clip = &c.animation.clips[c.animation.weapon_clip(b, action).unwrap()];
                        assert_eq!(
                            clip.duration(),
                            (t.get("frames").unwrap().as_f64().unwrap() - 1.)
                                / t.get("fps").unwrap().as_f64().unwrap()
                        );
                        timings += 1;
                    }
                }
            }
            let all_upper = c.tables.check_upper_tracks(&c.animation).unwrap();
            let mut upper_rows = 0;
            for slot in 0..unit
                .get("weapons")
                .and_then(bfe_content::Value::as_array)
                .unwrap_or(&[])
                .len()
            {
                for combo in 0..c.tables.combo_states(slot) {
                    let s = animation2::state::State {
                        weapon: slot,
                        combo: Some(combo),
                        ..Default::default()
                    };
                    if let Some(row) = c.tables.sample(&c.animation, &s, 0.).action {
                        if let Some(upper) = row.upper {
                            let lower = row.lower.filter(|&i| i != upper);
                            let mask =
                                lower.map_or(Mask::All, |i| Mask::Over(&c.animation.clips[i]));
                            let count = c.animation.clips[upper].applied_tracks(mask);
                            assert!(
                                count > 0,
                                "{key}, weapon {slot}, combo state {combo}: empty upper layer"
                            );
                            upper_rows += 1;
                            println!(
                                "upper {key} slot={slot} state={combo} joints={count} shared={}",
                                row.lower == row.upper
                            );
                        }
                    }
                }
            }
            println!("upper audit {key} combo rows={upper_rows} all rows={all_upper}");
            println!("weapon timings {key} {timings}");
            let tracks: usize = c.animation.clips.iter().map(|c| c.tracks.len()).sum();
            use animation2::state::{Action, Air, Direction, State};
            if let Some(id) = seat_clip.as_deref().and_then(|s| c.animation.named_clip(s)) {
                use animation2::player::{Arena, Player};
                let joints = c.animation.rig.joints.len();
                let mut arena = Arena::new(2, joints).unwrap();
                for (slot, selected) in [None, Some(id)].into_iter().enumerate() {
                    let mut p = Player::default();
                    let mut pose = arena.slot(slot, joints);
                    let state = State {
                        seat: selected.map(|_| (1, 1, 0)),
                        seat_clip: selected,
                        ..State::default()
                    };
                    p.publish(&c, state, &mut pose);
                    p.place_at(&c, 0., false, &mut pose);
                }
                let standing = arena.slot(0, joints).world.to_vec();
                let changed = standing
                    .iter()
                    .zip(arena.slot(1, joints).world.iter())
                    .take(c.animation.rig.mesh_joints)
                    .filter(|(a, b)| {
                        a.elements
                            .iter()
                            .zip(b.elements)
                            .any(|(x, y)| (x - y).abs() > 1e-5)
                    })
                    .count();
                assert!(changed > 0, "{key} seated mesh matches standing pose");
                println!("seated {key} changed mesh joints={changed}");
                seated_classes += 1;
            }
            for (kind, state) in [
                ("idle", State::default()),
                (
                    "walk",
                    State {
                        velocity: [0., 0., 0.5],
                        ..Default::default()
                    },
                ),
                (
                    "run",
                    State {
                        velocity: [0., 0., 100.],
                        ..Default::default()
                    },
                ),
                (
                    "sprint",
                    State {
                        sprint: true,
                        velocity: [0., 0., 100.],
                        ..Default::default()
                    },
                ),
                (
                    "roll",
                    State {
                        roll: Some(Direction::Forward),
                        ..Default::default()
                    },
                ),
                (
                    "jump",
                    State {
                        air: Air::Jump,
                        ..Default::default()
                    },
                ),
                (
                    "fall",
                    State {
                        air: Air::Fall,
                        ..Default::default()
                    },
                ),
                (
                    "land",
                    State {
                        air: Air::Land,
                        ..Default::default()
                    },
                ),
                (
                    "crouch",
                    State {
                        crouch: true,
                        ..Default::default()
                    },
                ),
                (
                    "fire",
                    State {
                        action: Action::Fire,
                        ..Default::default()
                    },
                ),
                (
                    "reload",
                    State {
                        action: Action::Reload,
                        ..Default::default()
                    },
                ),
                (
                    "hit",
                    State {
                        action: Action::Hit,
                        ..Default::default()
                    },
                ),
                (
                    "death",
                    State {
                        action: Action::Death,
                        ..Default::default()
                    },
                ),
            ] {
                let sample = c.tables.sample(&c.animation, &state, 0.);
                let row = if state.action == Action::None {
                    Some(sample.motion)
                } else {
                    sample.action
                };
                let layers = row.map(|row| {
                    [row.base, row.lower, row.upper].map(|id| {
                        id.map(|i| {
                            let clip = &c.animation.clips[i];
                            format!("{}/{:08x}", c.animation.banks[clip.bank], clip.crc)
                        })
                    })
                });
                println!("state {} {} {:?}", c.name, kind, layers);
            }
            println!(
                "{}/{} mesh={} rig={} banks={} clips={} tracks={} unbound=0",
                c.side,
                c.name,
                c.animation.rig.mesh_joints,
                c.animation.rig.joints.len(),
                c.animation.banks.join(","),
                c.animation.clips.len(),
                tracks
            );
        }
    }
    for row in coverage {
        println!("{row}");
    }
    if let Some(name) = seat_clip {
        assert!(seated_classes > 0, "no opening class binds {name}");
    }
}

#[test]
fn published_clocks_interpolate_independently_of_draw_order() {
    use bfe_content_native::animation2::{
        player::{Arena, Player},
        state::{State, Tables},
        Class,
    };
    let id = zero_hash::crc_hex("umber_mosaic_stand_idle_emote_full");
    let joint = zero_hash::crc_hex("tower");
    let actor=build_actor(&json::parse(r#"{"bones":[{"name":"tower","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#).unwrap(),&[]).unwrap();
    let side=json::parse(&json!({"fps":15,"packages":{zero_hash::fnv1a_hex("beacon"):{"banks":["umber"]}},"banks":{"umber":{"clips":[{"crc":id,"flags":0,"offset":0,"frames":3,"tracks":[joint]}]}}}).to_string()).unwrap();
    let bytes: Vec<_> = [
        0f32, 0., 0., 1., 0., 0., 0., 0., 0., 0., 1., 0., 4., 0., 0., 0., 0., 1., 0., 8., 0.,
    ]
    .into_iter()
    .flat_map(f32::to_le_bytes)
    .collect();
    let archive = Archive::new(&side, &bytes).unwrap();
    let def = json::parse(r#"{"weapons":[{"AnimationBank":"mosaic"}]}"#).unwrap();
    let anim = Character::bind(
        "beacon",
        &def,
        &["umber".into()],
        &actor,
        &archive,
        &archive,
    )
    .unwrap();
    let tables = Tables::new(
        &anim,
        &def,
        &bfe_content::Value::Null,
        &bfe_content::Value::Null,
    )
    .unwrap();
    let c = Class {
        side: String::new(),
        name: "beacon".into(),
        model: String::new(),
        animation: anim,
        tables,
    };
    let mut player = Player::default();
    let mut arena = Arena::new(1, 1).unwrap();
    let mut pose = arena.slot(0, 1);
    player.publish(&c, State::default(), &mut pose);
    player.publish(
        &c,
        State {
            tick: 1,
            position: [3., 0., 0.],
            ..State::default()
        },
        &mut pose,
    );
    for rate in [60., 144., 500., 144.] {
        let a = 60. / rate;
        player.place_at(&c, a, false, &mut pose);
        assert!((pose.world[0].elements[12] - 3. * a).abs() < 1e-12);
        assert!((pose.world[0].elements[13] - a).abs() < 1e-10);
    }
    let near_pi = std::f64::consts::PI - 0.02;
    player.reset();
    player.publish(
        &c,
        State {
            tick: 20,
            yaw: near_pi,
            ..State::default()
        },
        &mut pose,
    );
    player.publish(
        &c,
        State {
            tick: 21,
            yaw: -near_pi,
            ..State::default()
        },
        &mut pose,
    );
    let root = player.place_at(&c, 20.5, false, &mut pose);
    assert!((root.elements[0] + 1.).abs() < 1e-12);
    assert!(root.elements[8].abs() < 1e-12);
    let before = bfe_test_alloc::thread_allocations();
    for tick in 22..622 {
        player.publish(
            &c,
            State {
                tick,
                ..Default::default()
            },
            &mut pose,
        );
        for phase in [0., 0.12, 0.5, 0.96, 1.] {
            std::hint::black_box(player.place_at(
                &c,
                f64::from(tick - 1) + phase,
                tick % 4 == 0,
                &mut pose,
            ));
        }
    }
    let count = bfe_test_alloc::thread_allocations().wrapping_sub(before);
    assert_eq!(count, 0);
}

#[test]
fn split_clips_keep_stationary_upper_channels_off_moving_lower_joints() {
    let identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    let mesh=json::parse(&json!({"bones":[{"name":"wheel","parent":-1,"local":identity},{"name":"hook","parent":0,"local":identity}],"parts":[]}).to_string()).unwrap();
    let actor = build_actor(&mesh, &[]).unwrap();
    let tracks = ["wheel", "hook"].map(zero_hash::crc_hex);
    let meta = json::parse(
        &json!({"fps":2,"banks":{"umber":{"clips":[
          {"crc":zero_hash::crc_hex("roll"),"offset":0,"frames":2,"flags":0,"tracks":tracks},
          {"crc":zero_hash::crc_hex("lift"),"offset":28,"frames":2,"flags":0,"tracks":tracks}
        ]}}})
        .to_string(),
    )
    .unwrap();
    let bytes: Vec<_> = [0f32, 2., 0., 0., 0., 0., 0., 3.]
        .into_iter()
        .flat_map(|v| [0., 0., 0., 1., 0., v, 0.])
        .flat_map(f32::to_le_bytes)
        .collect();
    let archive = Archive::new(&meta, &bytes).unwrap();
    let c = Character::bind(
        "cart",
        &bfe_content::Value::Null,
        &["umber".into()],
        &actor,
        &archive,
        &archive,
    )
    .unwrap();
    let lower = &c.clips[c.clip("umber", zero_hash::crc("roll")).unwrap()];
    let upper = &c.clips[c.clip("umber", zero_hash::crc("lift")).unwrap()];
    let mut pose = [Local::default(); 2];
    lower.apply(&c.rig, 0.5, false, Mask::All, &mut pose);
    upper.apply(&c.rig, 0.5, false, Mask::Over(lower), &mut pose);
    assert_eq!(upper.applied_tracks(Mask::Over(lower)), 1);
    assert_eq!([pose[0].position.y, pose[1].position.y], [2., 3.]);
}

#[test]
fn pitch_uses_the_weapon_and_head_branch_and_a_hard_touchdown_selects_its_clip() {
    use bfe_content_native::animation2::{
        player::{Arena, Player},
        state::{Air, State, Tables},
        Class,
    };
    let bone = |name: &str, parent: i32, z: i32| json!({"name":name,"parent":parent,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,z,1]});
    let mesh=json::parse(&json!({"attach":"grip","bones":[bone("base",-1,0),bone("chest",0,0),bone("bone_head",1,0),bone("grip",1,1),bone("foot",0,1)],"parts":[]}).to_string()).unwrap();
    let actor = build_actor(&mesh, &[]).unwrap();
    let names = ["amber_test_landsoft_full", "amber_test_landhard_full"];
    let clips=names.map(|n|json!({"crc":zero_hash::crc_hex(n),"flags":0,"frames":1,"offset":0,"tracks":[zero_hash::crc_hex("base")]}));
    let meta =
        json::parse(&json!({"fps":30,"banks":{"amber":{"clips":clips}}}).to_string()).unwrap();
    let data: Vec<_> = [0f32, 0., 0., 1., 0., 0., 0.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let archive = Archive::new(&meta, &data).unwrap();
    let def = json::parse(r#"{"weapons":[{"AnimationBank":"test"}]}"#).unwrap();
    let anim = Character::bind(
        "traveler",
        &def,
        &["amber".into()],
        &actor,
        &archive,
        &archive,
    )
    .unwrap();
    let tables = Tables::new(
        &anim,
        &def,
        &bfe_content::Value::Null,
        &bfe_content::Value::Null,
    )
    .unwrap();
    let c = Class {
        animation: anim,
        tables,
        name: "traveler".into(),
        side: String::new(),
        model: String::new(),
    };
    for (hard, name) in [false, true].into_iter().zip(names) {
        let state = State {
            air: Air::Land,
            hard_landing: hard,
            ..State::default()
        };
        assert_eq!(
            c.tables.sample(&c.animation, &state, 0.).motion.base,
            c.animation.named_clip(name)
        );
    }
    let mut p = Player::default();
    let mut arena = Arena::new(1, 5).unwrap();
    let mut pose = arena.slot(0, 5);
    p.publish(
        &c,
        State {
            pitch: 0.5,
            ..State::default()
        },
        &mut pose,
    );
    p.place_at(&c, 0., false, &mut pose);
    assert!((pose.world[3].elements[13] - 0.5f64.sin()).abs() < 1e-9);
    assert_eq!(pose.world[4].elements[13], 0.);
}

#[test]
fn impact_and_evasion_use_the_equipped_stance() {
    use bfe_content_native::animation2::state::{Action, Direction, State, Tables};
    let mesh=json::parse(r#"{"bones":[{"name":"hinge","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#).unwrap();
    let actor = build_actor(&mesh, &[]).unwrap();
    let mut clips = Vec::new();
    let mut data = Vec::new();
    for (name, frames) in [
        ("woven_weft_stand_death_backward", 3),
        ("woven_weft_stand_death_forward", 4),
        ("woven_weft_stand_hitfront", 2),
        ("woven_weft_stand_hitleft", 1),
        ("woven_weft_thrown_flyingleft", 2),
        ("woven_tool_diveforward", 6),
        ("woven_rifle_diveforward", 5),
        ("woven_rifle_stand_hitright", 8),
        ("woven_rifle_stand_death_backward", 3),
        ("woven_sabr_stand_idle", 2),
    ] {
        clips.push(json!({"crc":zero_hash::crc_hex(name),"tracks":[zero_hash::crc_hex("hinge")],"flags":0,"frames":frames,"offset":data.len()}));
        for i in 0..frames {
            data.extend([0f32, 0., 0., 1., 0., 0., i as f32]);
        }
    }
    let meta =
        json::parse(&json!({"fps":9,"banks":{"woven":{"clips":clips}}}).to_string()).unwrap();
    let bytes: Vec<_> = data.into_iter().flat_map(f32::to_le_bytes).collect();
    let archive = Archive::new(&meta, &bytes).unwrap();
    let def = json::parse(r#"{"weapons":[{"AnimationBank":"weft"},{"comboBindings":[{"bank":"woven_sabr","mode":"melee","combo":"orbit"}]},{}]}"#).unwrap();
    let c = Character::bind(
        "spinner",
        &def,
        &["woven".into()],
        &actor,
        &archive,
        &archive,
    )
    .unwrap();
    let machines = json::parse(r#"{"orbit":{"states":[]}}"#).unwrap();
    let table = Tables::new(&c, &def, &bfe_content::Value::Null, &machines).unwrap();
    let sample = |weapon, action, impact, roll| {
        table.sample(
            &c,
            &State {
                weapon,
                action,
                impact,
                roll,
                ..Default::default()
            },
            0.,
        )
    };
    for (incoming, ending) in [
        (Direction::Forward, "backward"),
        (Direction::Backward, "forward"),
    ] {
        assert_eq!(
            sample(0, Action::Death, Some(incoming), None)
                .action
                .unwrap()
                .base,
            c.named_clip(&format!("woven_weft_stand_death_{ending}"))
        );
    }
    assert!(sample(0, Action::Hit, Some(Direction::Forward), None)
        .action
        .is_some());
    assert!(sample(0, Action::Hit, Some(Direction::Left), None)
        .action
        .is_none());
    for slot in [1, 2] {
        assert_eq!(
            sample(slot, Action::Death, None, None).action.unwrap().base,
            c.named_clip("woven_rifle_stand_death_backward")
        );
        assert_eq!(
            sample(slot, Action::None, None, Some(Direction::Left))
                .motion
                .base,
            c.named_clip("woven_tool_diveforward")
        );
        assert_eq!(
            sample(slot, Action::Hit, Some(Direction::Right), None)
                .action
                .unwrap()
                .base,
            c.named_clip("woven_rifle_stand_hitright")
        );
    }
}

#[test]
fn excessive_joint_counts_fail_before_binding_or_pose_allocation() {
    use bfe_content_native::animation2::{player::Arena, MAX_JOINTS};
    let actor = build_actor(&json::parse(r#"{"bones":[],"parts":[]}"#).unwrap(), &[]).unwrap();
    for count in [MAX_JOINTS, 80_000] {
        let joints: Vec<_> = (1..=count)
            .rev()
            .map(|i| {
                json!({
                    "boneCrc":format!("{i:08x}"),"parentCrc":format!("{:08x}",i-1),
                    "position":[0,1,0],"rotation":[0,0,0,1]
                })
            })
            .collect();
        let meta = json::parse(
            &json!({"fps":8,"banks":{"loom":{"clips":[]}},"skeletons":{"loom":{"joints":joints}}})
                .to_string(),
        )
        .unwrap();
        let archive = Archive::new(&meta, &[]).unwrap();
        let t = std::time::Instant::now();
        let result = Character::bind(
            "lantern",
            &bfe_content::Value::Null,
            &["loom".into()],
            &actor,
            &archive,
            &archive,
        );
        if count == MAX_JOINTS {
            let rig = result.unwrap().rig;
            assert!(rig.order.into_iter().eq((0..MAX_JOINTS).rev()));
        } else {
            let error = result.err().unwrap();
            assert!(error.contains("80000 joints"));
            println!("{error}, rejected in {:?}", t.elapsed());
        }
    }
    assert!(Arena::new(1024, 80_000).is_err());
    assert!(Arena::new(1, MAX_JOINTS).is_ok());
}
