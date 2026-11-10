//! Synthetic admission inputs run without prepared game files. Assertions
//! cover the marshaling quirks at the boundary, not a second battle engine.
use bfe_content::{json::parse, mission::Policy, value::Value};
use bfe_sim::{gen::*, store::Strings, world::admit::rows::{battle, cameras, startup, teams}};

fn value(text: &str) -> Value { parse(text).expect("synthetic JSON") }
fn policy(class_name: &str, team: f64) -> Policy {
    Policy { screen: false, side: "rep".into(), class_name: class_name.into(), team, post: None }
}

#[test]
fn startup_keeps_team_zero_hero_and_duplicate_classes() {
    let mission = value(r#"{"teams":{"0":{"classes":[["local",1]]},"1":{"hero":"hero","classes":[["hero",1],["trooper",2],["trooper",3]]}}}"#);
    let mut strings = Strings::new();
    let input = startup::from_policy(&policy("hero", 1.0), Some(&mission), &mut |s| strings.intern_str(s)).unwrap();
    assert_eq!(strings.list.iter().map(|s| String::from_utf16_lossy(s)).collect::<Vec<_>>(), ["hero", "local", "trooper"]);
    assert_eq!(input.rosters.iter().map(|r| r.classes.len()).collect::<Vec<_>>(), [1, 3, 0]);
    let mut row = vec![0.0; F4StartupClass::WORDS]; let mut heap = Vec::new(); input.put(&mut row, &mut heap);
    assert_eq!(F4StartupClass::take(&row, &heap), input);
}

#[test]
fn startup_reports_bad_content_without_panicking() {
    let mission = value(r#"{"teams":{"1":{"classes":[[]]}}}"#);
    assert!(startup::from_policy(&policy("bad", 1.0), Some(&mission), &mut |_| StrId(0)).is_err());
}

#[test]
fn team_names_have_eight_entries_and_share_empty_string() {
    let mission = value(r#"{"teams":{"1":{"name":"Republic"},"2":{"name":null},"7":{"name":"locals"}}}"#);
    let mut strings = Strings::new();
    let input = teams::names_from_mission(Some(&mission), &mut |s| strings.intern_str(s));
    assert_eq!(input.roster, 0); assert!(input.input.calls.is_empty());
    assert_eq!(input.input.names, [StrId(0), StrId(1), StrId(0), StrId(0), StrId(0), StrId(0), StrId(0), StrId(2)]);
}

#[test]
fn mission_calls_keep_scalar_tags_order_and_signed_zero() {
    let ai = value(r#"{"scripts":{"test":{"calls":[{"name":"SetTeamName","args":[1,"rep",null,true,-0],"faction":0,"mask":-0}]}}}"#);
    let mission = value(r#"{"name":"test","teams":{"1":{"name":"fallback"}}}"#);
    let mut strings = Strings::new();
    let calls = teams::mission_calls(Some(&ai), Some(&mission), &mut |s| strings.intern_str(s)).unwrap();
    assert_eq!(calls.len(), 1); assert_eq!(calls[0].faction, Some(0.0));
    assert_eq!(calls[0].mask.unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(calls[0].args[2], AiClassScalar::Null); assert_eq!(calls[0].args[3], AiClassScalar::Bool(true));
    assert!(matches!(calls[0].args[4], AiClassScalar::F64(v) if v.to_bits() == (-0.0f64).to_bits()));
    let empty = value(r#"{"scripts":{"test":{"calls":[]}}}"#);
    assert!(teams::mission_calls(Some(&empty), Some(&mission), &mut |s| strings.intern_str(s)).unwrap().is_empty());
}

#[test]
fn map_call_fallback_preserves_zero_counts_and_numeric_team_order() {
    let mission = value(r#"{"teams":{"2":{"name":"cis","units":0},"1":{"name":"rep","classes":[["unit",0]],"hero":"hero","reinforcements":0}},"spawnDelay":[0,1],"attacker":0}"#);
    let calls = teams::calls_from_map(Some(&mission));
    let names = calls.iter().map(|c| c.get("name").and_then(Value::as_str).unwrap()).collect::<Vec<_>>();
    assert_eq!(names, ["SetTeamName", "AddUnitClass", "SetHeroClass", "SetReinforcementCount", "SetTeamName", "SetUnitCount", "SetSpawnDelay", "SetAttackingTeam"]);
    assert_eq!(calls[0].get("args").unwrap().at(0).and_then(Value::as_f64), Some(1.0));
}

#[test]
fn camera_definition_has_posture_and_zoom_fallbacks() {
    let def = value(r#"{"ThirdPersonFOV":0,"FirstPersonFOV":null,"cameras":{"STAND":{"EyePointOffset":[-0,2,3]},"CROUCH":{"TrackCenter":[4,5,6]},"PRONE":{},"STANDZOOM":{"TiltValue":-0}}}"#);
    let definition = cameras::definition(&def);
    let sections = definition.get("sections").unwrap().as_array().unwrap();
    assert_eq!(sections.len(), 6);
    assert_eq!(sections[0].get("center").unwrap().at(1).and_then(Value::as_f64), Some(2.0));
    assert_eq!(sections[1].get("eye").unwrap().at(0).and_then(Value::as_f64), Some(4.0));
    assert_eq!(sections[2].get("eye").unwrap().at(1).and_then(Value::as_f64), Some(1.8));
    assert_eq!(sections[3].get("tilt").and_then(Value::as_f64).unwrap().to_bits(), (-0.0f64).to_bits());
    let encoded = String::from_utf8(cameras::encode(&definition)).unwrap();
    assert!(encoded.starts_with("{\"third\":0,\"first\":1.2217304763960306,\"sections\":"));
    assert!(encoded.contains("[-0.0,2,3]"));
}

#[test]
fn camera_init_uses_generated_motion_order_and_preserves_raw_properties() {
    let def = value("{}");
    let motions = value(r#"{"Land":{"Time":[1],"Vec":[[-0,0,0]],"extra":7},"RollLeft":{"Time":[]}}"#);
    let p = cameras::payloads(&def, Some(&motions), None);
    let init = parse(std::str::from_utf8(&p.init).unwrap()).unwrap();
    let keys = init.get("motions").unwrap().as_array().unwrap();
    assert_eq!(keys.len(), bfe_presentation::layout::camera_limits::MOTIONS);
    assert!(keys[0].get("Slope").is_none());
    assert_eq!(keys[6].get("extra").and_then(Value::as_f64), Some(7.0));
    assert!(String::from_utf8(p.init).unwrap().contains("[-0.0,0,0]"));
    let empty = cameras::payloads(&def, None, None);
    assert!(String::from_utf8(empty.init).unwrap().starts_with("{\"definitions\":[],\"motions\":[{\"Vec\":[],\"Slope\":[],\"Ore\":[],\"RSlope\":[],\"Time\":[]}"));
}

#[test]
fn battle_initial_tables_keep_authored_and_sorted_bleed_separate() {
    let mission = value(r#"{"teams":{"3":{},"2":{"reinforcements":-0},"1":{"units":22,"bleed":[[10,2],[0,1],[10,3]]}}}"#);
    let input = battle::from_mission(Some(&mission), 1.0, vec![]).unwrap();
    assert_eq!(input.foe, 2.0); assert_eq!(input.team_ids, [1.0, 2.0, 3.0]);
    assert_eq!(input.reinforcements.at(2.0).unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(input.reinforcements.at(3.0), Some(2147483647.0));
    assert_eq!(input.teams.get(1.0).unwrap().bleed.as_ref().unwrap()[0], [10.0, 2.0]);
    assert_eq!(input.bleed_rules.get(1.0).unwrap(), &[[0.0, 1.0], [10.0, 2.0], [10.0, 3.0]]);
    assert_eq!(input.stats.ring.killers.len(), 100); assert_eq!(input.victory_clock, Nullable::Null);
    assert!(battle::from_mission(Some(&value(r#"{"teams":{"bad":{}}}"#)), 1.0, vec![]).is_err());
}

#[test]
fn live_and_tail_capacities_cover_future_registry_and_fleet() {
    let facts = battle::Facts { local: Some(Ref::new(5, 2)), next_id: 20, built: 120,
        construction_limit: Some(600), fleet: 9, prepared: 3, point_capacity: 7, spawn_index: -0.0, random: 6, gameplay: 2, ..Default::default() };
    let live = battle::live(&facts).unwrap();
    assert_eq!((live.unit_capacity, live.registry_capacity, live.hull_capacity), (600, 600, 12));
    let world = value(r#"{"spawns":{"late":[],"first":[[0,10,0,0]]},"mission":{"teams":{"1":{"hero":"hero","classes":[["hero",1],["u",2],["u",3]]},"2":{"classes":[["v",2],["u",1]]}}}}"#);
    let mut strings = Strings::new();
    let tail = battle::tail_from_content(&world, None, true, &facts, &mut |s| strings.intern_str(s)).unwrap();
    assert_eq!((tail.registry_capacity, tail.victim_capacity, tail.point_capacity), (600, 612, 7));
    assert!(tail.classic && tail.enhanced); assert!(tail.fallback_path.is_empty());
    assert_eq!(tail.rosters.iter().map(|r| r.classes.len()).collect::<Vec<_>>(), [0, 2, 2]);
    assert_eq!(tail.classes.len(), 2); assert_eq!(tail.spawn_index.to_bits(), (-0.0f64).to_bits());
    let mut row = vec![0.0; F4TailSetup::WORDS]; let mut heap = Vec::new(); tail.put(&mut row, &mut heap);
    assert_eq!(F4TailSetup::take(&row, &heap), tail);
}

#[test]
fn missing_construction_limit_differs_from_zero_and_overflow_is_an_error() {
    let facts = battle::Facts { next_id: 4, built: 2, ..Default::default() };
    assert_eq!(battle::live(&facts).unwrap().registry_capacity, 68);
    assert_eq!(battle::live(&battle::Facts { construction_limit: Some(0), ..facts.clone() }).unwrap().registry_capacity, 0);
    assert!(battle::live(&battle::Facts { next_id: u32::MAX, ..facts }).is_err());
    assert_eq!(battle::setup(300).unwrap().capacity, 300);
    assert_eq!(battle::connections(0).unwrap(), 256);
}

#[test]
fn boundary_keeps_closing_edge_and_negative_zero_normal() {
    let nav = value(r#"{"boundary":{"references":[{"path":0},{"path":1.5},{"path":9},{"path":"0"}]},"paths":[{"points":[[0,0,0],[1,0,0],[1,0,1]]}]}"#);
    let b = battle::boundary(Some(&nav)).unwrap();
    assert_eq!(b.polygons.len(), 1); let edges = &b.polygons[0].edges;
    assert_eq!(edges[0], [1.0, 1.0, 1.0, -1.0]);
    assert_eq!(edges[1][2].to_bits(), (-0.0f64).to_bits());
    assert_eq!(edges[2], [1.0, 0.0, -1.0, 0.0]);
}

#[test]
fn roster_preserves_full_lists_without_bot_generator_truncation() {
    let names = value(r#"{"0":{"first":["neutral"],"last":[]},"3":{"first":["DELTA","THETA"],"last":["123"]},"local":"LOCAL NAME"}"#);
    let mut strings = Strings::new();
    let row = battle::roster_from_names(Some(&names), &[0.0, 3.0, 5.0], Some(Ref::new(2, 1)), None, &mut |s| strings.intern_str(s)).unwrap();
    assert_eq!(row.names[1].first.len(), 2); assert_eq!(row.names[2].first, row.names[0].first);
    assert!(row.names.iter().all(|n| n.state.is_none()));
    assert!(battle::roster_from_names(None, &[], None, Some(33.0), &mut |_| StrId(0)).is_err());
    assert!(battle::roster_from_names(None, &[], None, Some(f64::NAN), &mut |_| StrId(0)).is_err());
}

#[test]
fn capacity_queries_follow_mirror_order_and_generated_kinds() {
    let b = battle::battle_capacities(3); assert_eq!(b.len(), 4);
    assert_eq!(b[0].arguments, [0.0]); assert!(b[1..].iter().all(|q| q.arguments == [1.0]));
    let t = battle::tail_capacities(2, 1);
    assert_eq!(t.iter().map(|q| q.arguments.clone()).collect::<Vec<_>>(), [
        vec![F4ObjectKind::Post as u8 as f64, 0.0], vec![F4ObjectKind::Post as u8 as f64, 1.0],
        vec![F4ObjectKind::MobilePost as u8 as f64, 0.0], vec![F4ObjectKind::Human as u8 as f64, 0.0], vec![F4ObjectKind::Spawning as u8 as f64, 0.0],
    ]);
}

#[test]
fn tail_skips_roster_classes_without_ready_bodies() {
    let world = value(r#"{"mission":{"teams":{"1":{"classes":[["missing",1],["ready",2]]}}}}"#);
    let mut strings = Strings::new();
    let row = battle::tail_with_ready(&world, None, false, &battle::Facts::default(), &|name| name == "ready", &mut |s| strings.intern_str(s)).unwrap();
    assert_eq!(row.rosters[1].classes.len(), 1);
    assert_eq!(strings.list, vec!["ready".encode_utf16().collect::<Vec<_>>()]);
}

#[test]
fn bound_calls_keep_scalar_arguments_outside_record_blocks() {
    let rows = battle::BoundRows { live: F4LiveSetup::default(), battle: battle::setup(0).unwrap(), tail: F4TailSetup::default(),
        connections: 256, roster: F4RosterSetup::default(), battle_capacities: battle::battle_capacities(0), tail_capacities: battle::tail_capacities(0, 0) };
    let calls = rows.calls();
    assert_eq!(calls.iter().map(|c| c.1).collect::<Vec<_>>(), ["f4_live_init", "f4_battle_init", "f4_battle_capacity", "f4_tail_init", "f4_tail_capacity", "f4_tail_capacity", "f4_connections_init", "f4_roster_init"]);
    assert_eq!(calls[1].2.row, [256.0]); assert!(calls[1].2.args.is_empty());
    assert_eq!(calls[6].2.args, [256.0]); assert!(calls[6].2.row.is_empty());
    assert_eq!(F4LiveSetup::take(&calls[0].2.row, &calls[0].2.heap), rows.live);
}

#[test]
fn large_rosters_and_unusual_numbers_keep_generated_layout() {
    use std::rc::Rc;
    let classes = (0..1024).map(|i| Value::Array(Rc::new(vec![Value::str(&format!("unit_{i}")), Value::Number(1.0)]))).collect();
    let team = bfe_content::value::Object::from_written(vec![(Rc::from("classes"), Value::Array(Rc::new(classes)))]);
    let teams = bfe_content::value::Object::from_written(vec![(Rc::from("1"), Value::Object(Rc::new(team)))]);
    let mission = Value::Object(Rc::new(bfe_content::value::Object::from_written(vec![(Rc::from("teams"), Value::Object(Rc::new(teams)))])));
    let mut strings = Strings::new();
    let input = startup::from_policy(&policy("unit_0", f64::from_bits(0x7ff8_0000_0000_002a)), Some(&mission), &mut |s| strings.intern_str(s)).unwrap();
    let mut row = vec![0.0; F4StartupClass::WORDS]; let mut heap = Vec::new(); input.put(&mut row, &mut heap);
    assert_eq!(input.rosters[1].classes.len(), 1024);
    assert_eq!(row[F4StartupClass::TEAM].to_bits(), 0x7ff8_0000_0000_002a);
    assert_eq!(F4StartupClass::take(&row, &heap).rosters, input.rosters);
}

#[test]
fn pad_point_capacity_covers_sparse_future_variants() {
    let hull = |n| PsHull { record: Some(PsClass { points: vec![[0.0; 4]; n], ..Default::default() }), ..Default::default() };
    let pad = PsPad { variants: vec![hull(4), PsHull::default(), hull(1024)], vehicle: Some(hull(3)) };
    assert_eq!(battle::point_capacity(16, [None, Some(&pad), None]).unwrap(), 1024);
    assert_eq!(battle::point_capacity(2048, [Some(&pad)]).unwrap(), 2048);
}

// The content loader accepts empty binary assets here because these tests
// marshal definitions only; none installs collision geometry or posed rigs.
fn context_fixture() -> (bfe_content::files::MemFiles, bfe_sim::world::admit::Options) {
    let mut files = bfe_content::files::MemFiles::default();
    for (path, bytes) in [
        ("maps/synthetic/map.json", r#"{"mission":{"sides":["rep"],"teams":{"1":{"name":"rep","classes":[["rep_inf_a",1]]},"2":{"name":"cis","classes":[]}}}}"#),
        ("common/anims.json", r#"{"clips":{}}"#), ("common/common.json", "{}"), ("common/effects.json", "{}"),
        ("sides/rep/units.json", r#"{"units":{"rep_inf_a":{"base":"soldier","model":"body"}}}"#),
        ("sides/rep/weapons.json", "{}"), ("sides/rep/actors/body.json", r#"{"bones":[],"parts":[]}"#),
    ] { files.insert(path, bytes.as_bytes().to_vec()); }
    for path in ["sides/rep/actors/body.bin", "sides/rep/actors/body_index.bin", "common/anims.bin", "maps/synthetic/collision.bin",
        "maps/synthetic/collision_index.bin", "maps/synthetic/collision_poly.bin", "maps/synthetic/terrain_collision.bin",
        "maps/synthetic/terrain_collision_index.bin", "sides/rep/weapons.bin", "sides/rep/weapons_index.bin", "common/fxmodels.bin", "common/fxmodels_index.bin"] {
        files.insert(path, vec![]);
    }
    (files, bfe_sim::world::admit::Options { map: "synthetic".into(), side: "rep".into(), enhanced: false, seed: 1, heroes: false, unit: None, name: None })
}

#[test]
fn context_builders_share_existing_string_ids_and_team_call_scalars() {
    let (files, options) = context_fixture();
    let battle = bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files), &options.map, &options.query()).unwrap();
    let content = bfe_content::content::Content::build(battle).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let mut strings = Strings::new(); strings.intern_str("before"); let class = strings.intern_str("rep_inf_a");
    let mut cx = bfe_sim::world::admit::rows::Cx::new(&content, &defs, &files, &options, strings);
    let startup = startup::build(&mut cx).unwrap();
    assert_eq!(startup[0].2.row[F4StartupClass::CLASS_NAME], class.0 as f64);
    let first = teams::build(&mut cx).unwrap(); assert_eq!(first[0].2.args, [0.0]);
    let count = cx.strings.list.len();
    let second = teams::after_build(&mut cx, true).unwrap(); assert_eq!(second[0].2.args, [1.0]);
    assert_eq!(second[0].2.row, [0.0; AiMissionTeamsInput::WORDS]); assert!(second[0].2.strings.is_empty());
    assert_eq!(cx.strings.list.len(), count);
}

#[test]
fn context_camera_construction_reads_each_scratch_and_owner_return() {
    let (files, options) = context_fixture();
    let battle = bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files), &options.map, &options.query()).unwrap();
    let content = bfe_content::content::Content::build(battle).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let mut cx = bfe_sim::world::admit::rows::Cx::new(&content, &defs, &files, &options, Strings::new());
    // Live construction has no module memory: the address word is zero and
    // the body reads the call's scratch bytes.
    let live = cameras::build_init(&mut cx).unwrap();
    assert_eq!(live[0].2.args, [0.0, live[0].2.scratch.len() as f64]);
    cx.returned.push(("sim_scratch", 128.0));
    let init = cameras::build_init(&mut cx).unwrap();
    assert_eq!(init[0].2.args, [128.0, init[0].2.scratch.len() as f64]);
    assert!(cameras::build_definition(&mut cx).is_err());
    cx.returned.push(("camera_init", 2.0)); cx.returned.push(("sim_scratch", 256.0));
    let def = cameras::build_definition(&mut cx).unwrap();
    assert_eq!(def[0].2.args, [2.0, 256.0, def[0].2.scratch.len() as f64]);
    cx.returned.push(("camera_add_definition", 0.0));
    cx.returned.push(("sim_scratch", 999.0));
    let whole = cameras::build(&mut cx).unwrap();
    assert_eq!(whole[0].2.args[0], 128.0);
    assert_eq!(whole[1].2.args[1], 256.0);
}

#[test]
fn context_battle_calls_keep_qualified_references_and_shared_class_ids() {
    let (files, options) = context_fixture();
    let battle = bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files), &options.map, &options.query()).unwrap();
    let content = bfe_content::content::Content::build(battle).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let mut strings = Strings::new(); strings.intern_str("before"); let class = strings.intern_str("rep_inf_a");
    let mut cx = bfe_sim::world::admit::rows::Cx::new(&content, &defs, &files, &options, strings);
    cx.ids.posts = vec![0.0, 1.0];
    let facts = battle::Facts { local: Some(Ref::new(7, 3)), character: Some(Ref::new(9, 4)), characters: vec![Ref::new(9, 4)],
        next_id: 15, built: 2, construction_limit: Some(527), ..Default::default() };
    cx.ids.battle = Some(facts.clone()); cx.ids.factions = vec![0.0, 3.0, 4.0];
    let calls = battle::build(&mut cx).unwrap();
    let tail = calls.iter().find(|(_, name, _)| *name == "f4_tail_init").unwrap();
    let input = F4TailSetup::take(&tail.2.row, &tail.2.heap);
    assert_eq!(input.local, facts.local); assert_eq!(input.character, facts.character);
    assert_eq!(input.classes, [class]); assert_eq!(input.registry_capacity, 527);
    let queries = calls.iter().filter(|(_, name, _)| *name == "f4_tail_capacity").count();
    assert_eq!(queries, 4);
}

// The diagnostic consumes the same generated records as the builders.
// A synthetic predecessor stream keeps its dependency inference in CI.
#[allow(dead_code)]
#[path = "../examples/admission-rows-battle.rs"]
mod admission_dump;
#[test]
fn predecessor_facts_are_taken_from_earlier_owners_not_target_rows() {
    use serde_json::json;
    fn hex(v: &[f64]) -> Vec<String> { v.iter().map(|v|format!("{:016x}",v.to_bits())).collect() }
    fn event(name: &str, args: &[f64], row: &[f64], heap: &[f64]) -> serde_json::Value {
        json!({"export":name,"arguments":hex(args),"args":{"words":hex(row),"heap":hex(heap)},"returned":{"scalar":null,"block":null}})
    }
    let mut p=admission_dump::Predecessors::default();
    p.observe(&event("f2_register", &[0.0,1.0,0.0], &[], &[])).unwrap();
    p.observe(&event("f2_register", &[1.0,1.0,1.0], &[], &[])).unwrap();
    let mut birth=event("f3_registry_add", &[KIND_UNIT as f64,0.0], &[], &[]); birth["returned"]["scalar"]=json!(format!("{:016x}",10.0f64.to_bits()));
    p.observe(&birth).unwrap();
    let mut setup=AiWorldSetup::default();setup.settings.stream_a=7;
    setup.navigation.posts.push(AiNavPost {post:SpawnPost::Post(Ref::new(5,1)),..Default::default()});
    let mut row=vec![0.0;AiWorldSetup::WORDS];let mut heap=Vec::new();setup.put(&mut row,&mut heap);
    p.observe(&event("ai_world_begin", &[], &row, &heap)).unwrap();
    let mut program=AiRosterSetup::default();program.pads.push(F4PadSetup {pad:PsPad {vehicle:Some(PsHull {record:Some(PsClass {points:vec![[0.0;4];5],..Default::default()}),..Default::default()}),..Default::default()},..Default::default()});
    let mut row=vec![0.0;AiRosterSetup::WORDS];let mut heap=Vec::new();program.put(&mut row,&mut heap);
    p.observe(&event("ai_world_roster", &[], &row, &heap)).unwrap();
    let resident=F3ColdStage {adoption:F3Adoption {key:F3UnitKey {slot:3,generation:2,life:1.0},..Default::default()},..Default::default()};
    let mut row=vec![0.0;F3ColdStage::WORDS];let mut heap=Vec::new();resident.put(&mut row,&mut heap);
    p.observe(&event("f3_factory_resident", &[], &row, &heap)).unwrap();
    let command=F3Change {key:resident.adoption.key,action:F3Action::RandomBinding,index:0,value:6.0,..Default::default()};
    let mut row=vec![0.0;F3Change::WORDS];let mut heap=Vec::new();command.put(&mut row,&mut heap);
    p.observe(&event("f3_command", &[], &row, &heap)).unwrap();
    let character=Character {human:true,..Default::default()};let mut row=vec![0.0;Character::FULL_STRIDE];let mut heap=Vec::new();character.put_full(&mut row,&mut heap);AiCharacterBirth {slot:4,generation:1}.put(&mut row,&mut Vec::new());
    let mut export=event("sim_export", &[KIND_CHARACTER as f64,0.0], &[], &[]);export["returned"]["block"]=json!({"words":hex(&row),"heap":hex(&heap)});p.observe(&export).unwrap();
    let teams=AiMissionTeams {teams:vec![AiMissionTeam {faction:3.0,..Default::default()}],..Default::default()};
    let mut row=vec![0.0;AiMissionTeams::WORDS];let mut heap=Vec::new();teams.put(&mut row,&mut heap);
    let mut export=event("ai_mission_teams", &[1.0], &[], &[]);export["returned"]["block"]=json!({"words":hex(&row),"heap":hex(&heap)});p.observe(&export).unwrap();
    p.observe(&event("f4_live_init", &[], &[999.0;F4LiveSetup::WORDS], &[])).unwrap();
    let facts=p.resolved().unwrap();
    assert_eq!((facts.next_id,facts.built,facts.fleet,facts.prepared,facts.point_capacity,facts.random,facts.gameplay),(11,1,1,1,5,7,6));
    assert_eq!(facts.local,Some(Ref::new(3,2)));assert_eq!(facts.character,Some(Ref::new(4,1)));assert_eq!(p.posts,[Ref::new(5,1)]);
}

#[test]
fn new_roster_character_queries_capacity_once_per_qualified_identity() {
    use serde_json::json;
    let mut queries=admission_dump::CharacterQueries::default();
    queries.seed(&[Ref::new(4,1)]);
    for (slot,generation,life,expected) in [(4,1,2.0,false),(8,1,1.0,true),(8,1,2.0,false),(8,2,1.0,true)] {
        let result=F4RosterResult {handle:Some(F4Handle {kind:F4ObjectKind::Character,slot,generation,life}),..Default::default()};
        let mut row=vec![0.0;F4RosterResult::WORDS];let mut heap=Vec::new();result.put(&mut row,&mut heap);
        let hex=|v:&[f64]|v.iter().map(|v|format!("{:016x}",v.to_bits())).collect::<Vec<_>>();
        queries.observe(&json!({"export":"f4_roster_command","returned":{"block":{"words":hex(&row),"heap":hex(&heap)}}})).unwrap();
        assert_eq!(queries.take(),expected);assert!(!queries.take());
    }
    assert_eq!(battle::character_capacity().args,[1.0]);
}

#[test]
fn roster_publication_birth_precedes_its_capacity_query() {
    use serde_json::json;
    let birth=F4Handle {kind:F4ObjectKind::Character,slot:8,generation:1,life:1.0};
    let mut row=vec![0.0;F4Handle::WORDS];birth.put(&mut row,&mut Vec::new());
    let words:Vec<_>=row.iter().map(|v|format!("{:016x}",v.to_bits())).collect();
    let mut queries=admission_dump::CharacterQueries::default();
    queries.observe(&json!({"export":"f4_roster_init","returned":{"characterBirths":[words]}})).unwrap();
    assert!(queries.take());assert!(!queries.take());
}
