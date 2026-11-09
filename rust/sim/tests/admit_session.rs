//! CI fixtures for the native registry, connection and publication admission.
//! Authored test objects retain aliases, so identical-looking definitions can
//! prove that source binding uses identity rather than a name or list index.
#![cfg(not(target_arch = "wasm32"))]
use bfe_content::{battle::{Battle, FetchLog, SideFiles}, files::MemFiles, mission::{Policy, Query}, Value as ContentValue};
use bfe_sim::{gen::*, store::Strings, world::{admit::{rows::{Calls, Cx}, session::*, Options}, owner::World}};
use serde_json::{json, Value};
use std::rc::Rc;

fn value(text: &str) -> ContentValue { bfe_content::json::parse(text).unwrap() }
fn empty() -> ContentValue { value("{}") }
fn options() -> Options { Options { map: "test".into(), side: "a".into(), enhanced: false, seed: 1, heroes: false, unit: None } }
fn side(name: &str, units: &str) -> SideFiles {
    SideFiles { name: name.into(), dir: format!("/sides/{name}"), units_file: empty(), units: value(units), items: empty(),
        weapons: empty(), fire_points: empty(), weapon_buf: Rc::from([]), weapon_index_buf: Rc::from([]), fx: None }
}
fn battle() -> Battle {
    Battle { map: "test".into(), query_text: String::new(), query: Query::parse(""), map_data: empty(), anim_meta: empty(),
        common: empty(), effects: empty(), world: value(r##"{"mission":{"sides":["a","b"]},"commandPosts":[{"name":"home"},{}]}"##),
        policy: Policy { screen: false, side: "a".into(), class_name: "a_soldier".into(), team: 1.0, post: None },
        side: "a".into(), unit: "a_soldier".into(), soldier_def: empty(), anim_buf: Rc::from([]), coll_buf: Rc::from([]),
        coll_index_buf: Rc::from([]), coll_poly_buf: Rc::from([]), ground_buf: Rc::from([]), ground_index_buf: Rc::from([]),
        side_fx: None, props: None, props_buf: None, props_index_buf: None, props_coll_buf: None, props_coll_index_buf: None,
        props_coll_poly_buf: None, props_anim_buf: None, vehicle_dir: String::new(), vehicle_def: None, vehicle_bufs: None,
        sides: vec![side("a", r#"{"a_soldier":{"weapons":[{"class":"rifle"},{"class":"rifle"} ]}}"#),
            side("b", r#"{"b_soldier":{"weapons":[{"class":"pistol"}]}}"#), side("unfielded", r#"{"hidden":{"weapons":[{"class":"hidden"}]}}"#)],
        bodies: Vec::new(), walkers: Vec::new(), nav: None, ai_mission: None, class_data: Vec::new(), nav_bin: None, fetched: FetchLog::default() }
}

#[test]
fn native_registry_validates_and_orders_the_actual_input_block() {
    let content = bfe_content::content::Content::build(battle()).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap(); let battle = &content.battle;
    let files = MemFiles::default(); let options = options();
    let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new()); let mut world = World::new();
    let mut input = RegistryInput::build(&battle).unwrap();
    assert_eq!(input.classes, ["a_soldier", "b_soldier"]);
    assert_eq!(input.weapons, ["rifle", "rifle", "pistol"]);
    assert_eq!(input.posts, ["home", "#1"]);
    // UTF-16 order differs from scalar-value order above the BMP.
    input.classes.extend(["\u{e000}".into(), "\u{10000}".into(), "a_soldier".into()]);
    let mut calls = Calls::new(); let registry = create_registry(&mut world, &mut cx, &input, &mut calls).unwrap();
    assert_eq!(registry.classes, ["a_soldier", "b_soldier", "\u{10000}", "\u{e000}"]);
    assert_eq!(registry.weapons, ["pistol", "rifle"]);
    assert_eq!(registry.posts, ["#1", "home"]);
    assert_eq!(calls.len(), 4);
    let (_, name, call) = &calls[1]; assert_eq!(*name, "wire_registry_run"); assert_eq!(call.args, [1.0]);
    let row = WrTables::take(&call.row, &call.heap); assert_eq!(row.classes.len(), 5);
    let input = RegistryInput { classes: vec!["".into()], ..Default::default() };
    assert!(create_registry(&mut world, &mut cx, &input, &mut Calls::new()).is_err());
}

#[test]
fn source_definitions_keep_owner_order_and_object_identity() {
    let first = value(r#"{"base":"mine","class":"same"}"#); let twin = value(r#"{"base":"mine","class":"same"}"#);
    let other_side = value(r#"{"base":"mine","class":"hidden"}"#);
    let input = RegistryInput { combat: vec![("first".into(), first.clone()), ("twin".into(), twin.clone())], ..Default::default() };
    let registry = Registry { weapons: vec!["rifle".into()], ..Default::default() };
    let values = DefinitionValues { weapons: vec![value(r#"{"class":"hidden"}"#), value(r#"{"class":"rifle"}"#)],
        items: vec![twin.clone(), other_side, first.clone()], remotes: vec![first], ordnance: vec![twin] };
    let links = SourceDefinitions::bind(&input, &registry, &values).unwrap();
    assert_eq!(links.weapons, [WireStoreDefinition { definition: 1, id: 1 }]);
    assert_eq!(links.items, [WireStoreDefinition { definition: 0, id: 2 }, WireStoreDefinition { definition: 2, id: 1 }]);
    assert_eq!(links.remotes, [WireStoreDefinition { definition: 0, id: 1 }]);
    assert_eq!(links.ordnance, [WireStoreDefinition { definition: 0, id: 2 }]);
}

#[test]
fn welcome_text_matches_property_order_and_escaping() {
    let registry = Registry { classes: vec!["quote\"\\\n".into()], weapons: Vec::new(), posts: Vec::new(), combat_paths: vec!["[\"sides\",\"a\"]".into()] };
    let player = LocalPlayer { unit_id: 7, generation: 2.0, team: 1.0, class_name: "quote\"\\\n".into(), team_select: true, ..Default::default() };
    let packets = Session::welcome(&registry, &player).unwrap();
    assert_eq!(packets[1], r#"{"type":"welcome","epoch":1,"snapshotMode":"snapshot-delta-v1","unitId":7,"playerId":7,"generation":2,"team":1,"className":"quote\"\\\n","spectator":false,"teamSelect":true,"tick":0}"#);
    assert!(packets[0].starts_with(r#"{"type":"entity-registry","registry":{"combatDefinitions":["[\"sides\",\"a\"]"],"contentEpoch":1,"classes":[[1,"quote\"\\\n"]],"weapons":[],"posts":[],"inputCodes":["KeyA""#));
    let parsed: Value = serde_json::from_str(&packets[0]).unwrap(); assert_eq!(parsed["registry"], registry.descriptor());
    assert_eq!(input_codes().len(), 104); assert_eq!(input_codes().last().unwrap(), "WheelDown");
}

#[test]
fn mailbox_player_and_publication_are_owned_by_the_supplied_world() {
    let content = bfe_content::content::Content::build(battle()).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap(); let battle = &content.battle;
    let files = MemFiles::default(); let options = options();
    let input = RegistryInput::build(&battle).unwrap();
    let mut previous = None;
    for _ in 0..2 {
        let mut world = World::new(); let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new()); let mut calls = Calls::new();
        assert!(create_player(&mut world, &mut cx, &PlayerSetup::default(), &mut calls).is_err());
        let mailbox = create_mailbox(&mut world, &mut cx, &mut calls).unwrap();
        let player = create_player(&mut world, &mut cx, &PlayerSetup { mailbox, reliable_fire: true, separate_actions: true,
            ordinal: 1.0, team: 1.0, team_select: true, ..Default::default() }, &mut calls).unwrap();
        let local = LocalPlayer { mailbox, player, unit_id: 7, generation: 1.0, team: 1.0, class_name: "a_soldier".into(), team_select: true, ..Default::default() };
        let session = create_host(&mut world, &mut cx, &local, &input, &DefinitionValues::default(), 0.0).unwrap();
        assert_eq!(session.handles.mailbox, (15 << 26) | 1); assert_eq!(session.handles.player, (16 << 26) | 1);
        assert_eq!(session.handles.host, (2 << 26) | 1); assert_eq!(session.handles.publication, (12 << 26) | 1);
        assert_eq!(session.handles.edges, (5 << 26) | 1); assert_eq!(session.handles.client, 1);
        let names: Vec<_> = session.calls.iter().map(|(_, name, _)| *name).collect();
        assert!(names.iter().position(|n| *n == "wire_bind_store").unwrap() < names.iter().position(|n| *n == "wire_publish_create").unwrap());
        assert!(names.iter().position(|n| *n == "wire_store_source_configure").unwrap() < names.iter().position(|n| *n == "wire_publish_bind").unwrap());
        assert_eq!(session.player_id, 7.0); assert_eq!(session.registry["contentEpoch"], json!(1));
        if let Some(previous) = &previous { assert_eq!(previous, &session); } previous = Some(session);
    }
}

#[test]
fn missing_unit_owner_refuses_before_a_factory_or_connection_is_mutated() {
    let content = bfe_content::content::Content::build(battle()).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let files = MemFiles::default(); let options = options();
    let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new()); let mut world = World::new(); let mut prepared = false;
    let error = add_player(&mut world, &mut cx, &PlayerBindings::default(), 0.0, |_, _, _, _| { prepared = true; Ok(()) }).unwrap_err();
    assert!(matches!(error, bfe_sim::world::admit::AdmitError::Refused { call, .. } if call == "f3_init"));
    assert!(!prepared); assert!(cx.returned.is_empty());
}

#[test]
fn shared_definition_tables_retain_the_earlier_owners_exact_objects() {
    let content = bfe_content::content::Content::build(battle()).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap(); let battle = &content.battle;
    let files = MemFiles::default(); let options = options();
    let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new());
    let expected = battle.sides[0].units.get("a_soldier").unwrap().get("weapons").unwrap().at(1).unwrap();
    cx.definitions.weapons.push(("second loadout".into(), expected.clone()));
    let values = DefinitionValues::from_context(&cx).unwrap();
    assert!(values.weapons[0].same(expected));
    // A duplicate spelling does not collapse two separately authored objects.
    let first = battle.sides[0].units.get("a_soldier").unwrap().get("weapons").unwrap().at(0).unwrap();
    cx.definitions.weapons.push(("second loadout".into(), first.clone()));
    let values = DefinitionValues::from_context(&cx).unwrap();
    assert_eq!(values.weapons.len(), 2); assert!(values.weapons[1].same(first));
    assert!(!values.weapons[0].same(&values.weapons[1]));
}

/// An external packet oracle can exercise every content configuration without
/// committing authored data. The reference's player facts come from its
/// welcome; the registry and complete packet bytes must be rebuilt natively.
#[test]
fn registry_and_welcome_match_external_packet_reference() {
    let Ok(reference) = std::env::var("BFE_SESSION_REFERENCE") else { return };
    let root = std::env::var("BFE_CONTENT_DIR").expect("packet reference needs content");
    let files = bfe_content::files::DirFiles::new(root); let loader = bfe_content::battle::Loader::new(&files);
    let reference = std::fs::read_to_string(reference).unwrap(); let mut count = 0;
    for line in reference.lines().filter(|line| !line.is_empty()) {
        let row: Value = serde_json::from_str(line).unwrap();
        let label = row["label"].as_str().unwrap(); let map = row["map"].as_str().unwrap(); let query = row["query"].as_str().unwrap();
        let content = bfe_content::content::Content::build(Battle::load(&loader, map, query).unwrap()).unwrap();
        let defs = bfe_content::defs::Defs::build(&content).unwrap(); let battle = &content.battle;
        let mut options = options(); options.map = map.into();
        let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new()); let mut world = World::new();
        let input = RegistryInput::build(&battle).unwrap(); let registry = create_registry(&mut world, &mut cx, &input, &mut Calls::new()).unwrap();
        let player = &row["player"];
        assert_eq!(player["className"], battle.policy.class_name, "{label} class");
        assert_eq!(player["team"].as_f64().unwrap(), battle.policy.team, "{label} team");
        let player = LocalPlayer { unit_id: player["unitId"].as_u64().unwrap() as u32,
            generation: player["generation"].as_f64().unwrap(), team: battle.policy.team, class_name: battle.policy.class_name.clone(),
            team_select: player["teamSelect"].as_bool().unwrap(), tick: player["tick"].as_f64().unwrap(), ..Default::default() };
        let packets = Session::welcome(&registry, &player).unwrap();
        for i in 0..2 { assert_eq!(packets[i], row["packets"][i].as_str().unwrap(), "{label} packet {i}"); }
        count += 1;
    }
    assert!(count > 0, "packet reference must contain a battle");
    println!("session packet reference: {count} exact registries and welcome pairs");
}

#[test]
fn session_entry_uses_the_shared_context_and_keeps_holders_after_refusal() {
    let _: fn(&mut World, &mut Cx) -> Result<Session, bfe_sim::world::admit::AdmitError> = run;
    let content = bfe_content::content::Content::build(battle()).unwrap();
    let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let files = MemFiles::default(); let options = options();
    let mut cx = Cx::new(&content, &defs, &files, &options, Strings::new());
    cx.holders.push(bfe_sim::world::admit::bodies::Holder::Hull { hull: bfe_sim::store::Ref::new(3, 1), seats: Vec::new() });
    let mut world = World::new();
    assert!(run(&mut world, &mut cx).is_err());
    assert_eq!(cx.holders.len(), 1);
}

// Expanded only in the crate's unit-test module, where the cold World bank
// can be seeded with the preceding owners' recorded string table. The prior
// definition paths come from the JS game's actual definition tables, rather
// than being reconstructed from the source row under test.
#[allow(unused_macros)]
macro_rules! session_host_oracle {
    () => {
        #[test]
        fn host_rows_and_handles_match_recorded_calls() {
            use bfe_content::{battle::{Battle, Loader}, content::Content, defs::Defs,
                files::DirFiles, value::{Object, Value as CV}};
            use crate::{gen::*, world::{owner::World, admit::{rows::{Calls, Cx}, session::*}}};
            use serde_json::Value;
            use std::rc::Rc;
            let Ok(path) = std::env::var("BFE_SESSION_HOST_REFERENCE") else { return };
            let files = DirFiles::new(std::env::var("BFE_CONTENT_DIR").expect("host reference needs content"));
            let loader = Loader::new(&files);
            let data = std::fs::read_to_string(path).unwrap(); let mut battles = 0; let mut count = 0;
            let (mut player_rows, mut missing_direct) = (0, 0);
            let decode = |value: &Value| u64::from_str_radix(value.as_str().unwrap(), 16).unwrap();
            let assert_words = |actual: &[f64], expected: &Value, label: &str| {
                let expected = expected.as_array().map(Vec::as_slice).unwrap_or(&[]);
                assert_eq!(actual.len(), expected.len(), "{label} length");
                for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                    assert_eq!(a.to_bits(), decode(b), "{label} word {i}");
                }
            };
            for line in data.lines().filter(|line| !line.is_empty()) {
                let row: Value = serde_json::from_str(line).unwrap();
                assert!(row["source"].is_object(), "host reference needs prior owner facts");
                assert_eq!(row["prior"].as_array().map(Vec::len), Some(2), "mailbox and player call receipts");
                assert_eq!(row["packets"].as_array().map(Vec::len), Some(2), "both actual welcome events");
                let label = row["label"].as_str().unwrap(); let query = row["query"].as_str().unwrap();
                let content = Content::build(Battle::load(&loader, row["map"].as_str().unwrap(), query).unwrap()).unwrap();
                let defs = Defs::build(&content).unwrap();
                let mut world = World::new();
                for (i, units) in row["strings"].as_array().unwrap().iter().enumerate() {
                    let units: Vec<u16> = units.as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u16).collect();
                    assert_eq!(world.sim.strings.intern(&units).0 as usize, i, "{label} prior interner");
                }
                crate::players::module::players_init_in(&mut world);
                let options = crate::world::admit::Options { map: row["map"].as_str().unwrap().into(), side: content.battle.side.clone(),
                    enhanced: false, seed: 1, heroes: false, unit: None };
                let mut cx = Cx::new(&content, &defs, &files, &options, world.sim.strings.clone());
                let mut prior = Calls::new(); let mailbox = create_mailbox(&mut world, &mut cx, &mut prior).unwrap();
                let p = &row["player"]; let c = &p["character"];
                let character = F4Handle { kind: F4ObjectKind::Character, slot: c["slot"].as_u64().unwrap() as u32,
                    generation: c["generation"].as_u64().unwrap() as u32, life: c["life"].as_f64().unwrap() };
                let player = create_player(&mut world, &mut cx, &PlayerSetup { mailbox, character: Some(character.clone()),
                    reliable_fire: true, separate_actions: true, team_select: p["teamSelect"].as_bool().unwrap(),
                    ordinal: 1.0, team: p["team"].as_f64().unwrap(), recon_key: p["reconKey"].as_u64().map(|v| v as u32),
                    at: 1000.0, ..Default::default() }, &mut prior).unwrap();
                assert_eq!(mailbox as u64, p["mailbox"].as_u64().unwrap(), "{label} mailbox");
                assert_eq!(player as u64, p["handle"].as_u64().unwrap(), "{label} player");
                if let Some(expected) = row["prior"].as_array() {
                    assert_eq!(prior.len(), expected.len(), "{label} connection owner calls");
                    for ((_, name, call), expected) in prior.iter().zip(expected) {
                        assert_eq!(*name, expected["export"].as_str().unwrap());
                        assert_words(&call.args, &expected["arguments"], &format!("{label} {name} args"));
                        assert_words(&call.row, &expected["args"]["words"], &format!("{label} {name} row"));
                        assert_words(&call.heap, &expected["args"]["heap"], &format!("{label} {name} heap"));
                        assert_eq!(cx.returned(name).unwrap().to_bits(), decode(&expected["returned"]));
                        player_rows += 1;
                    }
                }
                // The factory/roster bodies have their own gates. Feed their
                // returned Character and stream identities into the exact
                // serializers used by add_player, without a second registry.
                if let Some(source) = row.get("source") {
                    let ids = &source["bindings"];
                    let bindings = PlayerBindings { combat_random: ids["combat"].as_u64().unwrap() as u32,
                        weapon_random: ids["weapon"].as_u64().unwrap() as u32, recon_key: ids["recon"].as_u64().map(|v|v as u32),
                        recon_seed: ids["reconSeed"].as_u64().unwrap() as u32, ..Default::default() };
                    let character_ref = crate::store::Ref::new(source["characterRef"][0].as_u64().unwrap() as u32,
                        source["characterRef"][1].as_u64().unwrap() as u32);
                    let class = content.battle.policy.class_name.as_str();
                    let mut connection_index = 0;
                    let key = |v: &Value| F3UnitKey { slot: v["slot"].as_u64().unwrap() as u32,
                        generation: v["generation"].as_u64().unwrap() as u32, life: v["life"].as_f64().unwrap() };
                    let mut mirror = F3Change { vector: std::array::from_fn(|i|f64::from_bits(decode(&source["mirror"]["vector"][i]))),
                        on_shield: f64::from_bits(decode(&source["mirror"]["onShield"])), ..Default::default() };
                    for expected in row["playerRows"].as_array().unwrap() {
                        let name = expected["export"].as_str().unwrap();
                        let call = match name {
                            "f4_roster_command" => reserve_call(&mut cx, content.battle.policy.team),
                            "f3_factory_player_bind" => binding_call(&bindings, character_ref),
                            "f4_connection_command" => {
                                let op = match connection_index { 0 => F4ConnectionOp::Admit, 1 => F4ConnectionOp::Spawn,
                                    2 if content.battle.policy.screen => F4ConnectionOp::Unspawn, _ => panic!("{label} unexpected connection command") };
                                connection_index += 1;
                                connection_call(&mut cx, op, character.clone(), if op == F4ConnectionOp::Unspawn { None } else { Some(class) })
                            }
                            "players_auto" => scalar(&[player as f64, u8::from(!content.battle.policy.screen) as f64]),
                            "f3_command" => {
                                let words: Vec<f64> = expected["args"]["words"].as_array().unwrap().iter().map(|v|f64::from_bits(decode(v))).collect();
                                if words.len() < F3Change::WORDS { missing_direct += 1; continue; }
                                let recorded = F3Change::take(&words, &[]);
                                let change = match recorded.action {
                                    F3Action::Unspawn if recorded.flag => F3Change { key: key(&source["viewpoint"]), action: F3Action::Unspawn, flag: true, ..Default::default() },
                                    F3Action::BodySpawn => F3Change { key: key(&source["key"]), action: F3Action::BodySpawn,
                                        vector: std::array::from_fn(|i|source["position"][i].as_f64().unwrap()), value: source["yaw"].as_f64().unwrap(), ..Default::default() },
                                    _ => continue, // Body instrumentation has its own construction gate.
                                };
                                change_call(&mut mirror, change)
                            }
                            _ => panic!("{label} unexpected player export {name}"),
                        };
                        assert_words(&call.args, &expected["arguments"], &format!("{label} {name} args"));
                        assert_words(&call.row, &expected["args"]["words"], &format!("{label} {name} row"));
                        assert_words(&call.heap, &expected["args"]["heap"], &format!("{label} {name} heap"));
                        player_rows += 1;
                    }
                    assert_eq!(connection_index, if content.battle.policy.screen {3} else {2});
                }
                let local = LocalPlayer { mailbox, player, character: Some(character), unit_id: p["unitId"].as_u64().unwrap() as u32,
                    generation: p["generation"].as_f64().unwrap(), team: p["team"].as_f64().unwrap(),
                    class_name: p["className"].as_str().unwrap().into(), team_select: p["teamSelect"].as_bool().unwrap(), tick: 0.0 };
                let input = RegistryInput::build(&content.battle).unwrap();
                let resolve = |values: &Value| values.as_array().unwrap().iter().map(|path| match path.as_str() {
                    Some(path) => input.combat.iter().find(|(key, _)| key == path).unwrap_or_else(|| panic!("{label} prior definition {path}")).1.clone(),
                    None => CV::Null,
                }).collect();
                let d = &row["definitions"];
                let values = DefinitionValues { weapons: d["weapons"].as_array().unwrap().iter().map(|name| match name.as_str() {
                    Some(name) => CV::Object(Rc::new(Object::from_written(vec![(Rc::from("class"), CV::str(name))]))), None => CV::Null,
                }).collect(), items: resolve(&d["items"]), remotes: resolve(&d["remotes"]), ordnance: resolve(&d["ordnance"]) };
                let session = create_host(&mut world, &mut cx, &local, &input, &values, 1000.0).unwrap();
                if let Some(packets) = row["packets"].as_array() {
                    assert_eq!(packets.len(), 2, "{label} welcome event count");
                    for (packet, expected) in session.packets.iter().zip(packets) {
                        assert_eq!(packet, expected.as_str().unwrap(), "{label} welcome bytes");
                    }
                }
                let expected = row["expected"].as_array().unwrap();
                assert_eq!(session.calls.len(), expected.len(), "{label} call count");
                for ((_, name, call), expected) in session.calls.iter().zip(expected) {
                    assert_eq!(*name, expected["export"].as_str().unwrap(), "{label} call {count}");
                    assert_words(&call.args, &expected["arguments"], &format!("{label} {name} arguments"));
                    assert_words(&call.row, &expected["args"]["words"], &format!("{label} {name} row"));
                    assert_words(&call.heap, &expected["args"]["heap"], &format!("{label} {name} heap"));
                    if matches!(*name, "wire_create" | "wire_presentation_create" | "wire_host_create" | "wire_stagger_create" |
                        "wire_shots_create" | "wire_combat_create" | "wire_store_source_create" | "wire_publish_create" | "wire_edges_create") {
                        assert_eq!(cx.returned(name).unwrap().to_bits(), decode(&expected["returned"]), "{label} {name} handle");
                    }
                    count += 1;
                }
                battles += 1;
            }
            assert!(battles > 0);
            assert_eq!(missing_direct, 0, "recording must include the generated direct F3 command rows");
            println!("recorded session inputs: {battles} battles, {count} native host calls/handles, {player_rows} player rows; {missing_direct} direct rows absent from recording");
        }
    };
}
