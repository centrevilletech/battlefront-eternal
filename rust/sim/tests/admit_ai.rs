//! Synthetic cold content exercises the admission boundary without retail
//! files. Scalar forms, canonical ids and ordering are part of the contract.
use bfe_content::{json::parse, value::Value};
use bfe_sim::gen::*;
use bfe_sim::store::Strings;
use bfe_sim::world::admit::{rows::ai::*, Options};

fn value(text: &str) -> Value {
    parse(text).expect("synthetic JSON")
}
fn options() -> Options {
    Options {
        map: "map".into(),
        side: "side".into(),
        seed: 0,
        enhanced: false,
        heroes: true,
        unit: None,
        name: None,
    }
}
fn text(strings: &Strings, id: StrId) -> String {
    String::from_utf16(strings.units(id)).expect("synthetic text")
}

#[test]
fn authored_class_primitives_and_string_intern_order_survive() {
    let source = value(
        r#"{"class":"class","label":"label","NoEnterVehicles":"enter","role":"role","ai":{"role":"ai"},
        "UnitType":"unit","AISizeType":null,"base":"soldier","MaxSpeed":-0,"MovingTurnOnly":false,
        "bounds":[[-0,1,2],[3,4,5]]}"#,
    );
    let mut strings = Strings::new();
    let row = class_facts(&mut strings, DefId(29), &source, None, &source).unwrap();
    assert_eq!(row.def, DefId(29));
    assert_eq!(row.name, StrId(0));
    assert_eq!(row.label, Some(StrId(1)));
    assert_eq!(row.no_enter_vehicles, AiClassScalar::Str(StrId(2)));
    assert_eq!(row.role, AiClassScalar::Str(StrId(3)));
    assert_eq!(row.ai_role, AiClassScalar::Str(StrId(4)));
    assert_eq!(row.unit_type, AiClassScalar::Str(StrId(5)));
    assert_eq!(row.size_type, AiClassScalar::Null);
    assert_eq!(row.health_type, AiClassScalar::Undefined);
    assert_eq!(row.moving_turn_only, AiClassScalar::Bool(false));
    assert!(matches!(row.max_speed, AiClassScalar::F64(n) if n.to_bits() == (-0.0f64).to_bits()));
    assert_eq!(row.bounds.unwrap()[0].to_bits(), (-0.0f64).to_bits());
    let invalid = value(r#"{"class":"bad","MaxSpeed":[]}"#);
    assert!(class_facts(&mut strings, DefId(1), &invalid, None, &invalid).is_err());
}

#[test]
fn item_services_keep_empty_presence_and_each_definition_identity() {
    let item = value(
        r#"{"base":"powerup","class":"same","service":{"health":[],"ammo":[null,false,-0,"3"]},
        "PersonHealth":"health","AnimalHealth":"animal","PersonAmmo":"ammo","Person":"bare"}"#,
    );
    let mut strings = Strings::new();
    let a = item_facts(&mut strings, DefId(9), &item).unwrap();
    let b = item_facts(&mut strings, DefId(10), &item).unwrap();
    assert_ne!(a.def, b.def);
    assert_eq!(a.class_name, b.class_name);
    assert_eq!(a.service_health, Some(vec![AiClassScalar::Undefined; 7]));
    assert_eq!(a.service_ammo.as_ref().unwrap()[0], AiClassScalar::Null);
    assert_eq!(
        a.service_ammo.as_ref().unwrap()[1],
        AiClassScalar::Bool(false)
    );
    assert!(
        matches!(a.service_ammo.as_ref().unwrap()[2], AiClassScalar::F64(n) if n.to_bits() == (-0.0f64).to_bits())
    );
    let health = match a.health[0] {
        AiClassScalar::Str(id) => id,
        _ => panic!("health string"),
    };
    let ammo = match a.ammo[0] {
        AiClassScalar::Str(id) => id,
        _ => panic!("ammo string"),
    };
    assert_eq!(text(&strings, health), "health");
    assert!(health.0 < ammo.0);
    assert_eq!(a.health[2], AiClassScalar::Undefined);
    assert_eq!(
        item_facts(&mut strings, DefId(11), &value("{}"))
            .unwrap()
            .service_health,
        None
    );
}

#[test]
fn raw_hull_target_and_cooked_passenger_policy_remain_distinct() {
    let def = value(
        r#"{"class":"hull","base":"walker","MaxSpeed":"raw","settings":{"MaxSpeed":"18 m/s","MaxTurnSpeed":"3",
        "YawLimits":[-1,1],"AISizeType":"LARGE"},"seats":[{"settings":{}},{"settings":{"TurnRate":".25"}}]}"#,
    );
    let pilot = value(r#"{"YawLimits":[-1,1]}"#);
    let passenger = value(r#"{"TurnRate":".25","PitchLimits":"-2 2"}"#);
    let input = HullFacts {
        hull: Ref::new(7, 3),
        member: Some(Member::VehicleTarget(Ref::new(19, 2))),
        def: DefId(8),
        definition: &def,
        name: "walker",
        base: "walker",
        target_class: &def,
        category: 3.0,
        bounds: [-1.0, -2.0, -3.0, 1.0, 2.0, 3.0],
        max_health: 100.0,
        speed: 12.0,
        turn_rate: 1.7,
        slot_desc: None,
        seats: vec![(&pilot, 2), (&passenger, 0)],
    };
    let mut strings = Strings::new();
    let row = hull_facts(&mut strings, &input).unwrap();
    assert_eq!(row.hull, Ref::new(7, 3));
    assert!(matches!(row.policy.max_speed, AiClassScalar::Str(_)));
    assert_eq!(row.max_speed, 18.0);
    assert_eq!(row.seats[0].policy.max_speed, AiClassScalar::F64(18.0));
    assert_eq!(row.seats[0].policy.max_yaw_speed, AiClassScalar::F64(1.0));
    assert_eq!(row.seats[1].policy.max_speed, AiClassScalar::F64(0.0));
    assert_eq!(row.seats[1].policy.turn_rate, AiClassScalar::F64(0.25));
    assert_eq!(row.seats[1].policy.pitch_rate, AiClassScalar::F64(0.25));
    assert_eq!((row.seats[0].kind, row.seats[1].kind), (2.0, 7.0));
    assert_eq!(text(&strings, row.seats[0].yaw.unwrap()), "-1,1");
    assert_eq!(row.seats[1].weapons, 0);
}

#[test]
fn flyer_uses_class_section_speeds_but_body_section_turn() {
    let def = value(
        r#"{"base":"flyer","settings":{"TurnRate":".4","MaxSpeed":"55","MinSpeed":"-0"},
        "seats":[{}, {"settings":{"LandingSpeed":"6","PitchRate":"9"}}]}"#,
    );
    let input = HullFacts {
        hull: Ref::new(0, 1),
        member: None,
        def: DefId(0),
        definition: &def,
        name: "flyer",
        base: "flyer",
        target_class: &def,
        category: 3.0,
        bounds: [0.0; 6],
        max_health: 2.0,
        speed: 0.0,
        turn_rate: 0.0,
        slot_desc: None,
        seats: vec![(&Value::Null, 0)],
    };
    let view = vehicle_class_view(&input);
    assert_eq!(view.get("TurnRate").and_then(Value::as_f64), Some(1.5));
    assert_eq!(view.get("PitchRate").and_then(Value::as_f64), Some(1.0));
    assert_eq!(view.get("LandingSpeed").and_then(Value::as_f64), Some(6.0));
    assert_eq!(view.get("MaxSpeed").and_then(Value::as_f64), Some(55.0));
    assert_eq!(
        view.get("MinSpeed")
            .and_then(Value::as_f64)
            .unwrap()
            .to_bits(),
        (-0.0f64).to_bits()
    );
}

#[test]
fn unnamed_post_matches_world_position_before_registry_alias() {
    let desc = value(r#"{"name":"","spawnPath":"fallback","spawnPoints":[]}"#);
    let mission = value(
        r#"{"posts":[{"position":[1,2,3],"order":4,"values":{"atk":[0,1,2,3,4,5],"def":[6,7,8,9,10,11]},
        "bans":4294967295,"allyCount":2,"classSpawnPoints":3,"valueBleed":80,"radius":9,"spawnPath":{"present":false}}]}"#,
    );
    let alias = Value::str("#5");
    let ai = attach_post_ai(
        &desc,
        Some(&alias),
        [1.0, 2.0, 3.0],
        0,
        Some(&mission),
        None,
    )
    .unwrap();
    let mut strings = Strings::new();
    let row = post_facts(
        &mut strings,
        SpawnPost::Post(Ref::new(4, 2)),
        Some(&alias),
        &desc,
        &ai,
    )
    .unwrap();
    assert_eq!(row.order, Some(4.0));
    assert_eq!(row.policy.attack, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!(row.policy.bans, -1.0);
    assert_eq!(row.spawn_present, Some(false));
    assert_eq!(row.ally_present, None);
    assert_eq!(row.value_bleed, 80.0);
    assert!(!row.policy.spawn);
}

#[test]
fn fallback_post_path_is_case_folded_and_capture_half_keeps_scale() {
    let desc = value(
        r#"{"spawnPath":"PATH","spawnPoints":[],"capture":{"type":"box","center":[1,2,3],
        "size":[2,3,4],"matrix":[2,0,0,0,0,3,0,0,0,0,4,0,1,2,3,1]}}"#,
    );
    let spawns = value(r#"{"path":[[1,2,3,4]]}"#);
    let ai = attach_post_ai(&desc, None, [0.0; 3], 5, None, Some(&spawns)).unwrap();
    let row = post_facts(&mut Strings::new(), SpawnPost::default(), None, &desc, &ai).unwrap();
    assert!(row.policy.spawn);
    assert_eq!(row.capture.unwrap().half, [4.0, 9.0, 16.0]);
    assert_eq!(row.raw_capture.unwrap().size, Some([2.0, 3.0, 4.0]));
    assert_eq!(row.spawn_points, 0.0);
    assert_eq!(row.order, Some(5.0));
}

#[test]
fn setup_and_roster_use_different_mission_argument_forms() {
    let mission = value(
        r#"{"teams":{"3":{"name":"local","classes":[["unit",2]],"units":1},"1":{"units":40},"2":{"units":40}},"attacker":1}"#,
    );
    let mut calls = calls_from_map_mission(Some(&mission));
    calls.push(value(
        r#"{"name":"Extra","args":["3junk",false,null,-0,"unit"]}"#,
    ));
    let facts = SetupFacts {
        stream_a: 12,
        stream_b: 13,
        state_a: 0,
        state_b: 0x9e3779b9,
        next_unit_id: 7,
        hull_seats: vec![2, 3],
        buildings: 4,
        classes: 8,
        posts: 6,
        unit_scale: 1.5,
        ..Default::default()
    };
    let mut strings = Strings::new();
    let setup = setup_rows(
        &mut strings,
        &options(),
        1.0,
        &calls,
        &facts,
        &AiAssets::default(),
    )
    .unwrap();
    assert_eq!(setup.settings.capacity.characters, 154);
    assert_eq!(setup.settings.capacity.registry, 385);
    assert_eq!(setup.settings.capacity.controls, 308);
    assert_eq!(setup.settings.capacity.support_keys, 16);
    assert_eq!(setup.settings.streams.as_ref().unwrap().c, 0x51ed270b);
    let last = setup.settings.mission.last().unwrap();
    assert_eq!(last.args, vec![0.0; 5]);
    assert_eq!(last.args[3].to_bits(), 0);
    assert_eq!(last.strings.len(), 2);
    let roster = roster_rows(
        &mut strings,
        &options(),
        1.0,
        1.5,
        &calls,
        None,
        &[],
        vec![],
    )
    .unwrap();
    let last = roster.calls.last().unwrap();
    assert!(matches!(last.args[0], AiClassScalar::Str(_)));
    assert_eq!(last.args[1], AiClassScalar::Bool(false));
    assert_eq!(last.args[2], AiClassScalar::Null);
    assert!(matches!(last.args[3],AiClassScalar::F64(n) if n.to_bits() == (-0.0f64).to_bits()));
    assert_eq!(roster.names.len(), 5);
    assert_eq!(roster.humans.len(), 1);
    assert!(roster.heroes);
}

#[test]
fn encoded_content_distinguishes_explicit_null_and_missing_tags() {
    let null_class = value(r#"{"class":"null","settings":{"Label":null}}"#);
    let missing_class = value(r#"{"class":"missing"}"#);
    let null_item = value(r#"{"base":null,"class":null}"#);
    let facts = ContentFacts {
        classes: vec![
            PreparedClass {
                name: "null",
                definition: &null_class,
                id: DefId(1),
            },
            PreparedClass {
                name: "missing",
                definition: &missing_class,
                id: DefId(2),
            },
        ],
        items: vec![ItemFacts {
            definition: &null_item,
            id: DefId(3),
        }],
        ..Default::default()
    };
    let content = content_rows(&mut Strings::new(), &facts).unwrap();
    let call = encoded_content(&content, &facts).unwrap();
    let raw: Vec<_> = AiWorldContent::classes_rows(&call.row, &call.heap).collect();
    // Probe the generated writer for label, without copying an offset into
    // production code. Only its absent tag differs between the two records.
    let mut missing = content.classes[1].clone();
    missing.def = content.classes[0].def;
    missing.name = content.classes[0].name;
    let mut expected = vec![0.0; AiUnitClass::WORDS];
    missing.put(&mut expected, &mut vec![]);
    let differences: Vec<_> = raw[0]
        .iter()
        .zip(expected)
        .filter(|(a, b)| a.to_bits() != b.to_bits())
        .collect();
    assert_eq!(differences.len(), 1);
    assert_eq!((*differences[0].0, differences[0].1), (1.0, 2.0));
}

#[test]
fn prop_preserves_owner_life_and_target_policy() {
    let desc = value(
        r#"{"class":"post","base":"commandpost","settings":{"MaxHealth":"4","YawLimits":null}}"#,
    );
    let target = value(r#"{"base":"building","MaxHealth":22}"#);
    let input = PropFacts {
        desc: &desc,
        def: DefId(4),
        target: RepairTarget::Building(Ref::new(8, 2)),
        owner: F4Handle {
            slot: 9,
            generation: 4,
            life: -0.0,
            ..Default::default()
        },
        member: None,
        target_name: Some("target"),
        target_class: Some(&target),
        team: 3.0,
        armed: false,
        post: Some(SpawnPost::Post(Ref::new(2, 5))),
    };
    let mut strings = Strings::new();
    let row = prop_facts(&mut strings, &input).unwrap();
    assert_eq!(row.owner.life.to_bits(), (-0.0f64).to_bits());
    assert!(row.command_post);
    assert_eq!(
        text(&strings, row.target_policy.as_ref().unwrap().name),
        "target"
    );
    assert_eq!(
        row.target_policy.unwrap().max_health,
        AiClassScalar::F64(22.0)
    );
    assert!(row.yaw.is_none());
}

#[test]
fn full_program_keeps_native_string_births_between_admission_pieces() {
    let mut strings = Strings::new();
    let facts = SetupFacts {
        buildings: 2,
        classes: 1,
        ..Default::default()
    };
    let mut program = begin(
        &options(),
        1.0,
        &[],
        &facts,
        &AiAssets::default(),
        &mut strings,
    )
    .unwrap();
    assert_eq!(
        program
            .invocations()
            .iter()
            .map(|c| c.1)
            .collect::<Vec<_>>(),
        vec![
            "ai_world_begin",
            "ai_world_boundary",
            "ai_world_prop_owner",
            "ai_world_prop_owner"
        ]
    );
    assert_eq!(program.invocations()[3].2.args, vec![1.0]);
    assert!(program.invocations()[1].2.row.is_empty());
    // begin creates voices inside the owner. These were not host interns,
    // and must not be emitted again as the following content's interns.
    strings.intern_str("native voice");
    let def = value(r#"{"class":"initial","base":"soldier"}"#);
    let content = ContentFacts {
        classes: vec![PreparedClass {
            name: "key",
            definition: &def,
            id: DefId(19),
        }],
        ..Default::default()
    };
    program.content(&content, &mut strings).unwrap();
    assert_eq!(program.content.as_ref().unwrap().classes[0].name, StrId(1));
    assert_eq!(program.invocations()[4].2.strings.len(), 2);
    assert!(program.invocations()[5].2.row.is_empty());
    assert!(program.characters(1).is_err());
    program
        .roster(
            &options(),
            1.0,
            1.0,
            &[],
            None,
            &content.classes,
            vec![],
            &mut strings,
        )
        .unwrap();
    assert!(program.characters(0).is_err());
    program.characters(4).unwrap();
    let births = &program.invocations()[7..10];
    assert_eq!(
        births.iter().map(|c| c.2.args[0]).collect::<Vec<_>>(),
        vec![0.0, 1.0, 2.0]
    );
    assert!(births
        .iter()
        .all(|c| c.1 == "ai_world_roster_character" && c.2.row.is_empty()));
    // Initial roster naming happens inside the native owner too. The late
    // definition must follow that updated table, with no private renumbering.
    strings.intern_str("native Character name");
    let late_id = strings.list.len() as u32;
    let late = value(r#"{"class":"hero"}"#);
    let late_facts = ContentFacts {
        classes: vec![PreparedClass {
            name: "hero",
            definition: &late,
            id: DefId(20),
        }],
        ..Default::default()
    };
    program.activation(&late_facts, &mut strings).unwrap();
    assert_eq!(
        program.late.as_ref().unwrap().classes[0].name,
        StrId(late_id)
    );
    let exports = program
        .invocations()
        .iter()
        .map(|c| c.1)
        .collect::<Vec<_>>();
    assert_eq!(
        &exports[10..],
        &[
            "ai_world_content",
            "ai_world_weapon_policies",
            "ai_world_activate"
        ]
    );
    assert!(program
        .activation(&ContentFacts::default(), &mut strings)
        .is_err());
}

#[test]
fn setup_reserves_large_authored_rosters_and_rejects_unrepresentable_capacity() {
    let calls = vec![value(r#"{"name":"SetUnitCount","args":[3,10000]}"#)];
    let facts = SetupFacts {
        next_unit_id: 100,
        hull_seats: vec![4; 512],
        buildings: 500,
        classes: 1000,
        posts: 300,
        unit_scale: 2.0,
        ..Default::default()
    };
    let row = setup_rows(
        &mut Strings::new(),
        &options(),
        1.0,
        &calls,
        &facts,
        &AiAssets::default(),
    )
    .unwrap();
    assert_eq!(row.settings.capacity.characters, 20065);
    assert_eq!(row.settings.capacity.controls, 40130);
    assert_eq!(row.settings.capacity.hulls, 512);
    assert_eq!(row.settings.capacity.support_keys, 2012);
    assert_eq!(
        row.settings.capacity.repair_jobs,
        row.settings.capacity.registry
    );
    let invalid = vec![value(r#"{"name":"SetUnitCount","args":[3,1e300]}"#)];
    assert!(setup_rows(
        &mut Strings::new(),
        &options(),
        1.0,
        &invalid,
        &facts,
        &AiAssets::default()
    )
    .is_err());
    assert_eq!(streams(0, 0).a, 0);
    assert_eq!(streams(0, 0).b, 0x9e3779b9);
}

fn synthetic_battle() -> bfe_content::battle::Battle {
    use bfe_content::{
        battle::{Battle, SideFiles},
        mission::{Policy, Query},
    };
    let side = SideFiles {
        name: "side".into(),
        dir: "sides/side".into(),
        units_file: value("{}"),
        units: value(r#"{"unit":{"class":"unit","base":"soldier","weapons":[]}}"#),
        items: value("{}"),
        weapons: value("{}"),
        fire_points: value("{}"),
        weapon_buf: vec![].into(),
        weapon_index_buf: vec![].into(),
        fx: None,
    };
    Battle {
        map: "map".into(),
        query_text: String::new(),
        query: Query::default(),
        map_data: value("{}"),
        anim_meta: value("{}"),
        common: value("{}"),
        effects: value("{}"),
        world: value(r#"{"mission":{"teams":{"1":{"units":32},"2":{"units":32}}}}"#),
        policy: Policy {
            screen: false,
            side: "side".into(),
            class_name: "unit".into(),
            team: 1.0,
            post: None,
        },
        side: "side".into(),
        unit: "unit".into(),
        soldier_def: value("{}"),
        anim_buf: vec![].into(),
        coll_buf: vec![].into(),
        coll_index_buf: vec![].into(),
        coll_poly_buf: vec![].into(),
        ground_buf: vec![].into(),
        ground_index_buf: vec![].into(),
        side_fx: None,
        props: None,
        props_buf: None,
        props_index_buf: None,
        props_coll_buf: None,
        props_coll_index_buf: None,
        props_coll_poly_buf: None,
        props_anim_buf: None,
        vehicle_dir: String::new(),
        vehicle_def: None,
        vehicle_bufs: None,
        sides: vec![side],
        bodies: vec![],
        walkers: vec![],
        nav: Some(value(r#"{"nodes":[]}"#)),
        ai_mission: None,
        class_data: vec![],
        nav_bin: Some(vec![0, 255, 8].into()),
        fetched: Default::default(),
    }
}

#[test]
fn shared_context_uses_raw_files_canonical_weapons_and_roster_returns() {
    use bfe_content::files::MemFiles;
    use bfe_sim::world::admit::rows::Cx;
    let battle = synthetic_battle();
    let mut files = MemFiles::default();
    let nav = b" { \"nodes\": [] }\n";
    let weapons = b"  { \"authored\": -0 }\n";
    files.insert("maps/map/ai-nav.json", nav.to_vec());
    files.insert("sides/side/ai.json", weapons.to_vec());
    let options = options();
    let content = bfe_content::content::Content::build(battle).expect("synthetic content");
    let defs = bfe_content::defs::Defs::build(&content).expect("synthetic definitions");
    let battle = &content.battle;
    let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new());
    cx.ids.defs.push(("weapon:canonical".into(), 12.0));
    let calls = begin_cx(
        &mut cx,
        &SetupFacts {
            buildings: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[3].2.args, vec![1.0]);
    let setup = AiWorldSetup::take(&calls[0].2.row, &calls[0].2.heap);
    assert_eq!(setup.navigation.json, nav);
    assert_eq!(setup.navigation.bin, vec![0, 255, 8]);
    let def = value(r#"{"class":"unit"}"#);
    let facts = ContentFacts {
        classes: vec![PreparedClass {
            name: "key",
            definition: &def,
            id: DefId(19),
        }],
        ..Default::default()
    };
    let calls = content_cx(&mut cx, &facts).unwrap();
    let content = AiWorldContent::take(&calls[0].2.row, &calls[0].2.heap);
    assert_eq!(content.weapons, vec![DefId(12)]);
    assert_eq!(content.weapon_data[0].json, weapons);
    assert!(calls[1].2.row.is_empty());
    assert!(roster_characters_cx(&cx).is_err());
    let calls = roster_cx(&mut cx, &facts.classes, 1.0, &[]).unwrap();
    assert_eq!(calls[0].1, "ai_world_roster");
    cx.returned.push(("ai_world_roster", 3.0));
    let births = roster_characters_cx(&cx).unwrap();
    assert_eq!(births.len(), 2);
    assert_eq!(births[1].2.args, vec![1.0]);
    let calls = activate_cx(&mut cx, &ContentFacts::default()).unwrap();
    assert_eq!(
        calls.iter().map(|c| c.1).collect::<Vec<_>>(),
        vec!["ai_world_activate"]
    );
    assert!(calls
        .iter()
        .all(|c| c.2.row.is_empty() && c.2.args.is_empty()));
    cx.returned.push(("ai_world_roster", 0.0));
    assert!(roster_characters_cx(&cx).is_err());
    assert!(AiAssets::read(&battle, &MemFiles::default()).is_err());
}

#[test]
fn ai_item_identity_admission_extends_the_shared_table_in_side_order() {
    use bfe_content::files::MemFiles;
    use bfe_sim::world::admit::rows::Cx;
    let mut battle = synthetic_battle();
    battle.sides[0].items = value(r#"{"second":{"class":"equal"},"first":{"class":"equal"}}"#);
    let files = MemFiles::default();
    let options = options();
    let content = bfe_content::content::Content::build(battle).expect("synthetic content");
    let defs = bfe_content::defs::Defs::build(&content).expect("synthetic definitions");
    let battle = &content.battle;
    let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new());
    let items = items_cx(&mut cx).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, DefId(0));
    assert_eq!(items[1].0, DefId(1));
    assert!(items[0]
        .1
        .same(battle.sides[0].items.get("second").unwrap()));
    assert!(items[1].1.same(battle.sides[0].items.get("first").unwrap()));
    assert_eq!(cx.ids.items.len(), 2);
    assert_eq!(items_cx(&mut cx).unwrap().len(), 2);
    assert!(cx.strings.list.is_empty());
}

#[test]
fn context_receipts_preserve_definition_identity_and_initial_content_cursor() {
    use bfe_content::files::MemFiles;
    use bfe_sim::world::admit::rows::Cx;
    let mut battle = synthetic_battle();
    battle.sides[0].units = value(
        r#"{"first":{"class":"same","base":"soldier","weapons":[]},"second":{"class":"same","base":"soldier","weapons":[]}}"#,
    );
    let content = bfe_content::content::Content::build(battle).expect("synthetic content");
    let defs = bfe_content::defs::Defs::build(&content).expect("synthetic definitions");
    let mut files = MemFiles::default();
    files.insert("maps/map/ai-nav.json", b"{}".to_vec());
    let options = options();
    let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new());
    cx.ids.ai = Some(Receipts {
        prepared: vec!["first".into()],
        ..Default::default()
    });
    let begin = bfe_sim::world::admit::rows::ai::build(&mut cx).unwrap();
    assert_eq!(cx.ids.ai.as_ref().unwrap().part, Part::Content);
    let mut receipts = cx.ids.ai.take().unwrap();
    assert_eq!(
        begin.iter().map(|c| c.1).collect::<Vec<_>>(),
        vec!["ai_world_begin", "ai_world_boundary"]
    );
    let calls = build_with_receipts(&mut cx, &mut receipts, Part::Content).unwrap();
    let initial = AiWorldContent::take(&calls[0].2.row, &calls[0].2.heap);
    assert_eq!(initial.classes.len(), 1);
    assert_eq!(initial.classes[0].def, DefId(0));
    receipts.prepared.push("second".into());
    let calls = build_with_receipts(&mut cx, &mut receipts, Part::Activate).unwrap();
    let late = AiWorldContent::take(&calls[0].2.row, &calls[0].2.heap);
    assert_eq!(late.classes.len(), 1);
    assert_eq!(late.classes[0].def, DefId(1));
    assert_eq!(cx.definitions.classes.len(), 2);
    assert!(!cx.definitions.classes[0]
        .1
        .same(&cx.definitions.classes[1].1));
    assert_eq!(calls.last().unwrap().1, "ai_world_activate");
}
