//! Synthetic admission inputs exercise the host's cold fleet boundary without
//! game files. Hex/bit checks distinguish signed zero and binary NaN payloads.
use bfe_content::battle::{Battle, Loader};
use bfe_content::files::MemFiles;
use bfe_content::json::parse;
use bfe_content::value::Value;
use bfe_sim::gen::{F2Bodies, F2BodyRef, F2ColliderHeader, FfSupply, Ref};
use bfe_sim::store::Strings;
use bfe_sim::world::admit::rows::{fleet, vehicle_pack::{self, Pack}};
use bfe_sim::world::admit::{Options};

fn value(text: &str) -> Value { parse(text).expect("synthetic JSON") }
fn options() -> Options {
    Options { map: "test".into(), side: "rep".into(), unit: Some("rep_test".into()), enhanced: false, seed: 1, heroes: false }
}

fn battle() -> Battle {
    let mut files = MemFiles::default();
    for (path, text) in [
        ("maps/test/map.json", r#"{"vehicles":"vehicles/pack.json","commandPosts":[{"name":"home","team":1}]}"#),
        ("common/anims.json", "{}"), ("common/common.json", "{}"), ("common/effects.json", "{}"),
        ("sides/rep/units.json", r#"{"units":{"rep_test":{"model":"body"}}}"#),
        ("sides/rep/weapons.json", "{}"), ("sides/rep/actors/body.json", "{}"),
        ("maps/test/vehicles/pack.json", r#"{"models":{"m":{"bones":[],"parts":[]}},"collision":{"m":{"positions":[-1,-2,-3,1,2,3,0,0,0],"indices":[0,1,2]}},"classes":{"hover":{"model":"m","settings":{"TurnRate":"-0"},"seats":[]}},"spawns":[{"post":"home","classes":{"1":"hover","2":"hover"}}]}"#),
    ] { files.insert(path, text.as_bytes().to_vec()); }
    for path in ["common/anims.bin", "common/fxmodels.bin", "common/fxmodels_index.bin", "maps/test/collision.bin",
        "maps/test/collision_index.bin", "maps/test/collision_poly.bin", "maps/test/terrain_collision.bin",
        "maps/test/terrain_collision_index.bin", "sides/rep/actors/body.bin", "sides/rep/actors/body_index.bin",
        "sides/rep/weapons.bin", "sides/rep/weapons_index.bin", "maps/test/vehicles/models.bin",
        "maps/test/vehicles/models_index.bin", "maps/test/vehicles/extras.bin", "maps/test/vehicles/extras_index.bin"] {
        files.insert(path, Vec::new());
    }
    Battle::load(&Loader::new(&files), "test", &options().query()).expect("synthetic battle loads")
}

// Cx borrows a single Content/Defs graph. Synthetic records do not need an
// actor decoder, so keep their shared Battle and empty resource catalogs.
fn content(battle: Battle) -> bfe_content::content::Content {
    let vehicle_templates = battle.vehicle_def.as_ref().map(|data| bfe_content::content::VehicleTemplate {
        name: "m".into(), rig: data.get("models").unwrap().get("m").unwrap().clone(),
        parts: Default::default(), shape: bfe_content::collision::json_shape(data.get("collision").unwrap().get("m").unwrap()),
        clips: None, wheel_radii: vec![], muzzle_tips: vec![], gear_foot: None,
    }).into_iter().collect();
    bfe_content::content::Content {
        battle, weapon_packs: vec![], vehicle_models: None, vehicle_extras: None,
        props_models: None, weapon_models: Default::default(), weapon_model_sources: vec![],
        actors: vec![], body_clips: vec![], walker_clips: vec![],
        collision: bfe_content::collision::MapCollision::load(&[],&[],&[],&[],&[]).unwrap(),
        vehicle_templates, vehicle_blockers: None, prop_bodies: vec![],
        prop_clips: vec![], death_regions: vec![], route_words: None, placed_items: vec![],
    }
}

#[test]
fn canonical_json_keeps_negative_zero_number_format_and_property_order() {
    let input = value(r#"{"z":-0,"10":10,"2":2,"small":1e-7,"fixed":1e-6,"large":1e21,"float":1.0000000000000002,"text":"é\n\u0001\\\""}"#);
    assert_eq!(vehicle_pack::json(&input).unwrap(), "{\"2\":2,\"10\":10,\"z\":-0,\"small\":1e-7,\"fixed\":0.000001,\"large\":1e+21,\"float\":1.0000000000000002,\"text\":\"é\\n\\u0001\\\\\\\"\"}");
    let nonfinite = vehicle_pack::array([Value::Number(f64::NAN), Value::Number(f64::INFINITY), Value::Number(-0.0)]);
    assert_eq!(vehicle_pack::json(&nonfinite).unwrap(), "[null,null,-0]");
}

#[test]
fn pack_picks_four_properties_aligns_utf8_and_copies_binary_bits() {
    let data = value(r#"{"classes":{"é":-0},"models":{},"collision":{},"extras":{"unused":true},"spawns":[]}"#);
    let floats = [0x00, 0x00, 0x00, 0x80, 0x45, 0x23, 0xc1, 0x7f];
    let indices = [0xff, 0xff, 0xff, 0xff];
    let clips = [0xab, 0xcd, 0xef, 0x7f];
    let pack = Pack::new(&data, &floats, &indices, &clips).unwrap();
    assert_eq!(pack.text, r#"{"models":{},"collision":{},"classes":{"é":-0}}"#);
    assert_eq!(pack.text_bytes as usize, pack.text.len());
    let head = (pack.text.len() + 3) & !3;
    assert_eq!(&pack.bytes[..pack.text.len()], pack.text.as_bytes());
    assert!(pack.bytes[pack.text.len()..head].iter().all(|&b| b == 0));
    assert_eq!(&pack.bytes[head..], [floats.as_slice(), indices.as_slice(), clips.as_slice()].concat());
    assert_eq!(pack.calls()[1].call.args, [pack.text_bytes as f64, 2.0, 1.0, 1.0]);
    assert!(Pack::new(&data, &[1], &[], &[]).is_err());
}

#[test]
fn class_keys_are_projected_and_turn_rate_uses_body_and_hashed_settings() {
    let def = value(r#"{"settings":{"MaxTurnSpeed":"2.5tail"},"springs":null,"explosion":{},"finalExplosion":0,"seats":[{"settings":null},{"settings":{"TurnRate":8}}],"weapons":[1]}"#);
    assert_eq!(vehicle_pack::json(&fleet::class_keys(&def)).unwrap(), r#"{"settings":{"MaxTurnSpeed":"2.5tail"},"springs":[],"fx":null,"explosion":true,"finalExplosion":false,"seats":[{"settings":null},{"settings":{"TurnRate":8}}]}"#);
    assert_eq!(fleet::turn_rate(&def), 2.5);
    assert_eq!(fleet::turn_rate(&value(r#"{"base":"flyer","settings":{"TurnRate":9}}"#)), 0.0);
    assert_eq!(fleet::turn_rate(&value(r#"{"base":"walker","settings":{"TurnRate":9,"MaxTurnSpeed":3}}"#)), 3.0);
    let hashed = format!(r#"{{"settings":{{"{}":"-0"}}}}"#, bfe_content::odf::fnv("TurnRate"));
    assert_eq!(fleet::turn_rate(&value(&hashed)).to_bits(), (-0.0f64).to_bits());
}

#[test]
fn map_rules_preserve_missing_keys_and_water_set_insertion_order() {
    assert_eq!(fleet::map_rules(&value("{}")).unwrap(), r#"{"water":null,"mission":{}}"#);
    let world = value(r#"{"water":{"cell":32,"offset":[-0,2],"grid":4,"cells":[[2,1],[1,0],[2,1],[-0,-0]]},"deathRegions":[],"mission":{"deathRegions":["pit"]}}"#);
    assert_eq!(fleet::map_rules(&world).unwrap(), r#"{"water":{"cell":32,"offset":[-0,2],"grid":4,"cells":[6,1,0]},"deathRegions":[],"mission":{"deathRegions":["pit"]}}"#);
    assert!(fleet::map_rules(&value(r#"{"water":{"cells":[1]}}"#)).is_err());
}

#[test]
fn initial_hulls_precede_unique_dormant_variants_in_pad_order() {
    let data = value(r#"{"classes":{"a":{},"b":{},"c":{}},"spawns":[{"post":"owned","classes":{"2":"b","1":"a","3":"b"}},{"team":0,"classes":{"1":"c","2":"a"}},{"classes":{"2":"c"}}]}"#);
    let world = value(r#"{"commandPosts":[{"name":"Owned","team":2}]}"#);
    let hulls = fleet::hulls(&data, &world).unwrap();
    assert_eq!(hulls.iter().map(|h| (h.pad,h.name.as_str(),h.prepared)).collect::<Vec<_>>(),
        [(0,"b",false),(2,"c",false),(0,"a",true),(1,"c",true),(1,"a",true)]);
    assert!(fleet::hulls(&value(r#"{"classes":{},"spawns":[{"classes":{"1":"missing"}}]}"#), &value("{}")).is_err());
}

#[test]
fn generated_collision_and_supply_rows_keep_handles_and_signed_values() {
    let header = F2ColliderHeader { body: 9, kernel: 12, min: [-0.0, -2.0, -3.0], max: [1.0, 2.0, 3.0] };
    let export = fleet::collider(header, &[-1, 77], &[88]).unwrap();
    assert_eq!(export.name, "veh_collider"); assert_eq!(export.call.args, [2.0,1.0]);
    let packed = export.call;
    let scratch: Vec<f64> = packed.scratch.chunks_exact(8).map(|v| f64::from_le_bytes(v.try_into().unwrap())).collect();
    let decoded = F2ColliderHeader::take(&scratch, &[]);
    assert_eq!(decoded.body, 9); assert_eq!(decoded.kernel, 12);
    assert_eq!(decoded.min[0].to_bits(), (-0.0f64).to_bits());
    assert_eq!(&scratch[F2ColliderHeader::WORDS..], &[-1.0,77.0,88.0]);
    let refs = vec![F2BodyRef { body: Ref::new(4,7), kernel:12 }, F2BodyRef { body: Ref::new(3,8), kernel:11 }];
    let packed = fleet::bodies(refs.clone()).call;
    assert_eq!(F2Bodies::take(&packed.row, &packed.heap).bodies, refs);
    let packed = fleet::firing_supply(Ref::new(4,7), 3.0, -0.0).call;
    let decoded = FfSupply::take(&packed.row,&packed.heap);
    assert_eq!(decoded.hull, Ref::new(4,7)); assert_eq!(decoded.life, 3.0);
    assert_eq!(decoded.amount.to_bits(), (-0.0f64).to_bits());
    assert_eq!(fleet::register(Ref::new(4,7), true).call.args, [4.0,7.0,1.0]);
    assert_eq!(fleet::collision(2,-1).call.args, [2.0,-1.0]);
    assert_eq!(fleet::prepare_body(12).call.args, [12.0]);
}

#[test]
fn cold_body_mapping_handles_a_census_larger_than_any_map() {
    let refs: Vec<_> = (0..8192).rev().map(|i| F2BodyRef { body: Ref::new(i,2), kernel:i+40 }).collect();
    let packed = fleet::bodies(refs.clone()).call;
    assert_eq!(F2Bodies::take(&packed.row,&packed.heap).bodies, refs);
}

#[test]
fn full_synthetic_battle_builds_pack_map_tuning_classes_and_pad_json() {
    let battle = battle();
    let calls = fleet::content_calls(&battle, &options()).unwrap();
    let names: Vec<_> = calls.iter().map(|call| call.name).collect();
    assert_eq!(&names[..4], ["veh_map","veh_bytes","veh_pack","veh_flight_env"]);
    assert_eq!(names.iter().filter(|&&n| n == "veh_flight_tune").count(), 152);
    assert_eq!(&names[names.len()-3..], ["veh_class","f2_init","veh_map"]);
    assert!(calls.iter().all(|call| call.call.row.is_empty() && call.call.heap.is_empty()));
    assert_eq!(calls.iter().find(|call| call.name == "veh_class").unwrap().call.args[2].to_bits(), (-0.0f64).to_bits());
    // Earlier owners have already interned names: cold calls must reuse the
    // same table, without allocating a second id or re-interning that string.
    let mut strings = Strings::new(); strings.intern_str("fighter");
    let prefix = fleet::prefix(&battle, &mut strings).unwrap();
    let fighter = prefix.iter().find(|call| call.name == "veh_flight_tune").unwrap();
    assert_eq!(fighter.call.args[0], 0.0);
    let (env,height) = fleet::flight_environment(&battle.world,&bfe_content::collision::MapCollision::load(&[],&[],&[],&[],&[]).unwrap()).unwrap();
    assert_eq!((env.as_str(),height), (r#"{"ceiling":200}"#,200.0));
}

#[test]
fn flight_environment_uses_padded_bvh_bounds_and_authored_key_positions() {
    let positions: Vec<_> = [0f32,0.,0., 10.,0.,0., 0.,0.,20.].into_iter().flat_map(f32::to_le_bytes).collect();
    let indices: Vec<_> = [0u32,1,2].into_iter().flat_map(u32::to_le_bytes).collect();
    let col = bfe_content::collision::MapCollision::load(&positions,&indices,&[0,0,0,0],&[],&[]).unwrap();
    let world = value(r#"{"mission":{"maxFlyHeight":250},"flight":{"ceiling":300,"extra":-0}}"#);
    let (text,height) = fleet::flight_environment(&world,&col).unwrap();
    assert_eq!(height,250.0);
    assert!(text.starts_with(r#"{"ceiling":300,"bounds":{"min":[-"#));
    assert!(text.ends_with(r#"},"extra":-0}"#));
    let env = value(&text);
    let min_x = env.get("bounds").unwrap().get("min").unwrap().as_array().unwrap()[0].as_f64().unwrap();
    assert!(min_x < 0.0);
    // The raw JSON value stays authored even though the scalar export
    // converts the same height to a number at the boundary.
    let (text,height) = fleet::flight_environment(&value(r#"{"mission":{"maxFlyHeight":"250"}}"#),&col).unwrap();
    assert_eq!(height,250.0); assert!(text.starts_with(r#"{"ceiling":"250","bounds":"#));
}

#[test]
fn resolved_fleet_orders_registration_body_mapping_colliders_and_init() {
    let battle = battle();
    let binding = fleet::HullBinding { hull: Ref::new(7,3), spawned: true, collider: F2ColliderHeader {
        body: 12, kernel: 21, min: [-1.0,-2.0,-3.0], max: [1.0,2.0,3.0] }, walls: vec![33,34], springs: vec![-1] };
    let bindings = fleet::Bindings { world: 2, blockers: -1, bodies: vec![F2BodyRef { body: Ref::new(12,1), kernel:21 }], hulls:vec![binding] };
    let calls = fleet::resolved(&battle, &mut Strings::new(), &bindings).unwrap();
    let tail = &calls[calls.len()-7..];
    assert_eq!(tail.iter().map(|call| call.name).collect::<Vec<_>>(),
        ["veh_class","f2_register","veh_collision","f2_bodies","veh_collider","f2_init","veh_map"]);
    assert_eq!(tail[1].call.args, [7.0,3.0,0.0]);
    let mut bad = bindings; bad.hulls.clear();
    assert!(fleet::resolved(&battle, &mut Strings::new(), &bad).is_err());
}

#[test]
fn collider_bounds_round_authored_corners_before_cache_sizing() {
    let data = value(r#"{"classes":{"a":{"model":"m","springs":[{},{}]}},"collision":{"m":{"positions":[-5.0000001,-0,-40,5,2,40,0,0,0],"indices":[0,1,2]}}}"#);
    let shape = fleet::collider_shape(&data,"a").unwrap();
    assert!(shape.min[0] < -5.0); assert!(shape.min[1] <= 0.0);
    assert_eq!((shape.walls,shape.springs), (13,2));
    assert!(fleet::collider_shape(&value(r#"{"classes":{"a":{"model":"m"}},"collision":{"m":{"positions":[]}}}"#),"a").is_err());
}

#[test]
fn extracted_corpus_fleet_content_has_complete_packs_and_collider_shapes() {
    let Ok(root) = std::env::var("BFE_CONTENT_DIR") else { return; };
    let files = bfe_content::files::DirFiles::new(root);
    let loader = Loader::new(&files);
    // The checked-in matrix is only option text; no game-derived fixtures
    // enter the repository. CI runs the synthetic cases above without files.
    let matrix = include_str!("../../../tools/world-hash-matrix.txt");
    let mut battles = 0;
    for line in matrix.lines() {
        let args: Vec<_> = line.split('#').next().unwrap_or("").split_whitespace().collect();
        if args.is_empty() { continue; }
        let get = |name| args.windows(2).find(|a| a[0] == name).map(|a| a[1]);
        let options = Options { map: get("--map").unwrap_or("nab2").into(), side: get("--side").unwrap_or("rep").into(),
            enhanced: get("--ai") == Some("enhanced"), seed: get("--seed").unwrap_or("1").parse().unwrap(),
            heroes: args.contains(&"--heroes"), unit: get("--unit").map(String::from) };
        let battle = Battle::load(&loader,&options.map,&options.query()).unwrap();
        let pack = Pack::from_battle(&battle).unwrap();
        let calls = fleet::content_calls(&battle,&options).unwrap();
        if let Some(data) = &battle.vehicle_def {
            assert!(pack.is_some()); assert!(!calls.is_empty());
            for hull in fleet::hulls(data,&battle.world).unwrap() {
                let shape = fleet::collider_shape(data,&hull.name).unwrap();
                assert!((3..=39).contains(&shape.walls));
            }
        } else { assert_eq!(calls.iter().map(|c|c.name).collect::<Vec<_>>(), ["veh_map"]); }
        battles += 1;
    }
    assert_eq!(battles,61);
}

#[test]
fn shared_context_keeps_pack_scalars_separate_and_records_interns() {
    let content = content(battle()); let battle = &content.battle;
    let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let files = MemFiles::default(); let options = options();
    let mut strings = Strings::new(); strings.intern_str("earlier owner");
    let mut cx = bfe_sim::world::admit::rows::Cx::new(&content,&defs,&files,&options,strings);
    let calls = vehicle_pack::build(&mut cx).unwrap();
    assert_eq!(calls.iter().map(|(_,name,_)| *name).collect::<Vec<_>>(), ["veh_map","veh_bytes","veh_pack"]);
    assert_eq!(calls[0].2.args[0],1.0);
    assert!(calls.iter().all(|(_,_,call)| call.row.is_empty() && call.heap.is_empty()));
    assert_eq!(String::from_utf16(&calls[0].2.strings[0]).unwrap(),fleet::map_rules(&battle.world).unwrap());
    assert!(calls[1].2.scratch.is_empty());
    assert_eq!(calls[1].2.args[0] as usize,calls[2].2.scratch.len());
    assert_eq!(calls[2].2.args.len(),4);
    let again = vehicle_pack::build(&mut cx).unwrap();
    assert!(again.iter().all(|(_,_,call)| call.strings.is_empty()));
    assert_eq!(calls[0].2.args, again[0].2.args);
}

#[test]
fn initial_flyers_import_spawned_flight_turn_rate_and_dormant_flyers_keep_zero() {
    let data = value(r#"{"classes":{"f":{"base":"flyer","settings":{"TurnRate":"0.5"}}},"spawns":[{"classes":{"1":"f"}}]}"#);
    let hull = fleet::hulls(&data,&value("{}")).unwrap().remove(0);
    assert_eq!(hull.turn_rate,1.5);
    let live = fleet::bound_class(&data,&hull,true,&mut Strings::new(),&mut Vec::new()).unwrap();
    let blocked = fleet::bound_class(&data,&hull,false,&mut Strings::new(),&mut Vec::new()).unwrap();
    assert_eq!(live[0].call.args[2],1.5); assert_eq!(blocked[0].call.args[2],0.0);
    assert_eq!(fleet::spawned_turn_rate(&value(r#"{"base":"flyer"}"#)),2.0);
}

#[test]
fn context_fleet_consumes_receipts_and_retains_ordered_holders_once() {
    use bfe_sim::world::admit::{bodies::Holder, rows::Cx};
    let content = content(battle()); let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let files = MemFiles::default(); let options = options();
    let mut cx = Cx::new(&content,&defs,&files,&options,Strings::new());
    cx.ids.kernel = Some(bfe_content::install::Installed {world:2,grid:-1,buildings:0,terrain:-1,statics:0});
    cx.ids.fleet = Some(fleet::Bindings {world:2,blockers:-1,
        bodies:vec![F2BodyRef {body:Ref::new(12,1),kernel:21}], hulls:vec![fleet::HullBinding {
            hull:Ref::new(7,3),spawned:true,collider:F2ColliderHeader {body:12,kernel:21,min:[-1.0,-2.0,-3.0],max:[1.0,2.0,3.0]},
            walls:vec![33,34],springs:vec![] }] });
    cx.holders.push(Holder::Turret {turret:Ref::new(9,1),rider:None});
    let prefix = vehicle_pack::build(&mut cx).unwrap();
    let map_id = prefix[0].2.args[0];
    let calls = fleet::build(&mut cx).unwrap();
    assert!(calls.iter().any(|(_,name,call)| *name=="veh_flight_env" && !call.strings.is_empty()));
    let repeated_map = calls.iter().find(|(_,name,_)| *name=="veh_map").unwrap();
    assert_eq!(repeated_map.2.args[0],map_id); assert!(repeated_map.2.strings.is_empty());
    assert_eq!(cx.ids.vehicles,[("hover".into(),7.0)]);
    assert_eq!(cx.ids.vehicle_bodies,[("hover".into(),21.0)]);
    assert_eq!(cx.ids.bodies,[(Ref::new(12,1),21)]);
    assert!(matches!(&cx.holders[0],Holder::Hull {hull,seats} if *hull==Ref::new(7,3)&&seats.is_empty()));
    assert!(matches!(&cx.holders[1],Holder::Turret {turret,..} if *turret==Ref::new(9,1)));
    let again = fleet::build(&mut cx).unwrap();
    assert!(again.iter().all(|(_,_,call)| call.strings.is_empty()));
    assert_eq!(cx.ids.vehicles.len(),1); assert_eq!(cx.holders.len(),2);
}

#[test]
fn context_without_a_pack_installs_shared_rules_before_unit_init() {
    use bfe_sim::world::admit::rows::Cx;
    let mut battle = battle(); battle.vehicle_def=None; battle.vehicle_bufs=None;
    let content = content(battle); let defs = bfe_content::defs::Defs::build(&content).unwrap();
    let files = MemFiles::default(); let options = options();
    let mut cx = Cx::new(&content,&defs,&files,&options,Strings::new());
    cx.ids.kernel = Some(bfe_content::install::Installed {world:4,grid:-1,buildings:0,terrain:-1,statics:0});
    assert!(vehicle_pack::build(&mut cx).unwrap().is_empty());
    let calls = fleet::build(&mut cx).unwrap();
    assert_eq!(calls.iter().map(|(_,name,_)|*name).collect::<Vec<_>>(),["veh_map","veh_collision"]);
    assert_eq!(calls[1].2.args,[4.0,-1.0]);
    assert!(calls.iter().all(|(stage,_,_)|*stage==bfe_sim::world::admit::Stage::Units));
}
