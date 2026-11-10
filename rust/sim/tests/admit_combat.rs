//! Cold combat and item marshaling over synthetic content, without game files.
//! Expected words describe the host boundary, including JavaScript's missing
//! values, object identity and scalar bit patterns.
use bfe_content::json::parse;
use bfe_content::value::{Object, Value};
use bfe_sim::gen::{AiWeaponPolicySource, F3CombatField, F3WeaponField};
use bfe_sim::store::{DefId, Strings};
use bfe_sim::world::admit::rows::combat::{combat_settings, new_combat, value_words, weapon_levels, weapon_set, CombatRows, Definitions};
use bfe_sim::world::admit::rows::items::{pools, ItemRows, ITEM_FIELDS};
use bfe_sim::world::admit::rows::ordnance::{OrdnanceRows, ORDNANCE_FIELDS};
use bfe_sim::world::admit::rows::weapon_policies::{call, source_with_data};
use bfe_sim::world::admit::Stage;
use std::rc::Rc;

fn bits(values: &[f64]) -> Vec<u64> { values.iter().map(|v| v.to_bits()).collect() }
fn text(strings: &Strings, id: f64) -> String { String::from_utf16_lossy(&strings.list[id as usize]) }

#[test]
fn value_words_preserve_missing_null_boolean_and_numeric_bits() {
    let mut strings = Strings::new();
    let nan = f64::from_bits(0x7ff8_0000_0000_0345);
    for (value, expected) in [
        (None, vec![0.0]), (Some(Value::Null), vec![1.0]),
        (Some(Value::Bool(false)), vec![2.0]), (Some(Value::Bool(true)), vec![3.0]),
        (Some(Value::Number(-0.0)), vec![4.0, -0.0]),
        (Some(Value::Number(nan)), vec![4.0, nan]),
    ] { assert_eq!(bits(&value_words(value.as_ref(), &mut strings, None)), bits(&expected)); }
}

#[test]
fn object_keys_use_javascript_order_and_depth_limit_keeps_scalars() {
    let value = parse(r#"{"z":1,"9":"nine","2":"two","a":false}"#).unwrap();
    let mut strings = Strings::new();
    let row = value_words(Some(&value), &mut strings, None);
    assert_eq!(row, [7.0,4.0, 0.0,5.0,1.0, 2.0,5.0,3.0, 4.0,4.0,1.0, 5.0,2.0]);
    assert_eq!(strings.list.iter().map(|v| String::from_utf16_lossy(v)).collect::<Vec<_>>(), ["2","two","9","nine","z","a"]);
    let mut deep = Value::Array(Rc::new(vec![Value::Number(-0.0)]));
    for _ in 0..12 { deep = Value::Array(Rc::new(vec![deep])); }
    let row = value_words(Some(&deep), &mut strings, None);
    assert_eq!(row.len(), 25);
    assert_eq!(row[24], 9.0);
    let mut deep = Value::Number(-0.0);
    for _ in 0..12 { deep = Value::Array(Rc::new(vec![deep])); }
    assert_eq!(bits(&value_words(Some(&deep), &mut strings, None)[24..]), bits(&[4.0,-0.0]));
}

#[test]
fn identity_tables_retain_equal_but_distinct_records() {
    let a = parse(r#"{"base":"bullet"}"#).unwrap();
    let b = parse(r#"{"base":"bullet"}"#).unwrap();
    let mut defs = Definitions::default();
    assert_eq!(defs.id(&a, Stage::Combat).unwrap(), DefId(0));
    assert_eq!(defs.id(&b, Stage::Combat).unwrap(), DefId(1));
    assert_eq!(defs.id(&a.clone(), Stage::Combat).unwrap(), DefId(0));
    assert!(defs.id(&Value::Null, Stage::Combat).is_err());
    assert!(defs.id(&Value::Number(1.0), Stage::Combat).is_err());
}

#[test]
fn class_registers_weapons_first_and_references_them_by_identity() {
    let class = parse(r#"{"base":"soldier","weapons":[{"class":"rifle"},{"class":"rifle"}]}"#).unwrap();
    let mut rows = CombatRows::default();
    let mut strings = Strings::new();
    rows.class(&class, &mut strings).unwrap();
    assert_eq!(rows.calls.iter().map(|c| c.name).collect::<Vec<_>>(), ["cmb_weapon","cmb_weapon","cmb_class"]);
    let row = &rows.calls[2].call.row;
    assert_eq!((row[0], row[1]), (7.0,3.0));
    assert_eq!(text(&strings,row[2]), "AddHealth");
    assert_eq!(row[3], 0.0);
    assert_eq!(text(&strings,row[4]), "base");
    assert_eq!(text(&strings,row[7]), "weapons");
    assert_eq!(&row[8..], [6.0,2.0,8.0,0.0,8.0,1.0]);
    rows.class(&class, &mut strings).unwrap();
    assert_eq!(rows.calls.len(), 3);
    // Sending call rows may be drained between construction seams.
    rows.calls.clear();
    rows.class(&class, &mut strings).unwrap();
    rows.weapon(&rows.weapons.values[0].clone(), &mut strings).unwrap();
    assert!(rows.calls.is_empty());
}

#[test]
fn combat_construction_uses_truthy_health_chunks_and_health_scale() {
    let mut strings = Strings::new();
    let class = parse(r#"{"MaxHealth":0,"HealthType":"DrOiD","chunks":[{}],"ChunkFrequency":2.5}"#).unwrap();
    let row = new_combat(42,DefId(7),&class,true,&mut strings).unwrap();
    assert_eq!(row.name,"cmb_new");
    assert_eq!(row.call.args,[42.0,7.0,1.0,300.0,2.5,0.0]);
    assert!(row.call.row.is_empty());
    assert_eq!(text(&strings,0.0),"DroidScale");
    let class = parse(r#"{"MaxHealth":-1,"HealthType":"unknown","ChunkFrequency":2}"#).unwrap();
    let row = new_combat(3,DefId(0),&class,false,&mut strings).unwrap();
    assert_eq!(&row.call.args[..5],[3.0,0.0,0.0,-1.0,0.0]);
    assert_eq!(text(&strings,row.call.args[5]),"PersonScale");
    for chunks in [r#""part""#,r#"{"length":2}"#] {
        let class=parse(&format!("{{\"chunks\":{chunks},\"ChunkFrequency\":2.5}}")).unwrap();
        assert_eq!(new_combat(3,DefId(0),&class,false,&mut strings).unwrap().call.args[4],2.5);
    }
    let invalid = parse(r#"{"HealthType":3}"#).unwrap();
    assert!(new_combat(0,DefId(0),&invalid,false,&mut strings).is_err());
}

#[test]
fn scalar_weapon_and_combat_commands_use_generated_field_values() {
    let row = weapon_set(6,2,F3WeaponField::YawSpreadOverride,-0.0);
    assert_eq!(bits(&row.call.args),bits(&[6.0,2.0,F3WeaponField::YawSpreadOverride as u32 as f64,-0.0]));
    let rows = combat_settings(6,[0.0,0.0,2.0,1.0,1.0,0.0]);
    assert_eq!(rows.len(),6);
    assert_eq!(rows[2].call.args,[6.0,F3CombatField::Team as u32 as f64,2.0]);
}

#[test]
fn ordnance_fields_keep_tags_and_register_explosions_before_parent() {
    let def = parse(r#"{"base":"bullet","Velocity":-0,"LifeSpan":null,"Gravity":false,"StickTerrain":true,"ExplosionImpact":{"Damage":20},"ExplosionExpire":{"Damage":20}}"#).unwrap();
    let mut rows = OrdnanceRows::default();
    let mut strings = Strings::new();
    rows.definition(&def,&mut strings).unwrap();
    assert_eq!(rows.calls.iter().map(|c| c.name).collect::<Vec<_>>(),["ord_explosion","ord_explosion","ord_def"]);
    let row = &rows.calls[2].call.row;
    assert_eq!(row.len(),ORDNANCE_FIELDS.len()*2);
    assert_eq!(bits(&row[..8]),bits(&[5.0,0.0,4.0,-0.0,1.0,0.0,2.0,0.0]));
    assert_eq!(&row[24..],[3.0,0.0,0.0,0.0,6.0,0.0,6.0,1.0]);
    rows.definition(&def,&mut strings).unwrap();
    assert_eq!(rows.calls.len(),3);
}

#[test]
fn reserved_ordnance_handles_are_drained_in_table_order() {
    let a = parse(r#"{"base":"beam"}"#).unwrap();
    let b = parse(r#"{"base":"grenade"}"#).unwrap();
    let mut rows = OrdnanceRows::default();
    rows.definitions.id(&a,Stage::Store).unwrap();
    rows.definitions.id(&b,Stage::Store).unwrap();
    rows.definition(&b,&mut Strings::new()).unwrap();
    assert_eq!(rows.calls[0].call.args,[0.0]);
    assert_eq!(rows.calls[1].call.args,[1.0]);
}

#[test]
fn items_share_explosion_table_and_preserve_all_fields() {
    let explosion = parse(r#"{"Damage":40}"#).unwrap();
    let make = || Value::Object(Rc::new(Object::from_written(vec![
        (Rc::from("base"),Value::str("mine")),
        (Rc::from("LifeSpan"),Value::Number(f64::INFINITY)),
        (Rc::from("ExplosionDeath"),explosion.clone()),
        (Rc::from("ExplosionExpire"),explosion.clone()),
    ])));
    let (a,b) = (make(),make());
    let mut rounds = OrdnanceRows::default();
    let mut items = ItemRows::default();
    let mut strings = Strings::new();
    assert_eq!(items.definition(&a,&mut rounds,&mut strings).unwrap(),DefId(0));
    assert_eq!(items.definition(&b,&mut rounds,&mut strings).unwrap(),DefId(1));
    assert!(rounds.calls.is_empty());
    assert_eq!(items.calls.iter().map(|c|c.name).collect::<Vec<_>>(),["ord_explosion","items_def","items_def"]);
    let row = &items.calls[1].call.row;
    assert_eq!(row.len(),ITEM_FIELDS.len()*2);
    assert_eq!(&row[8..10],[4.0,f64::INFINITY]);
    assert_eq!(&row[14..20],[6.0,0.0,0.0,0.0,6.0,0.0]);
}

#[test]
fn mission_pool_overrides_keep_default_places_and_null_tags() {
    let mission = parse(r#"{"pools":{"mine":-0,"powerupitem":null,"extra":false,"4":7}}"#).unwrap();
    let mut rounds = OrdnanceRows::default();
    let mut strings = Strings::new();
    let row = pools(Some(&mission),&mut rounds,&mut strings).unwrap();
    assert_eq!(row.call.args,[4.0]);
    assert_eq!(strings.list.iter().map(|v| String::from_utf16_lossy(v)).collect::<Vec<_>>(),["4","powerupitem","mine","extra"]);
    assert_eq!(bits(&row.call.row),bits(&[0.0,4.0,7.0,1.0,1.0,0.0,2.0,4.0,-0.0,3.0,2.0,0.0]));
}

#[test]
fn policy_source_preserves_original_bytes_empty_files_and_weapon_order() {
    let source = source_with_data(&[DefId(9),DefId(0)], [Some(b"{ \"x\":-0 }\n".to_vec()),None,Some(Vec::new())]);
    assert_eq!(source.data.len(),2);
    assert_eq!(source.data[0].json,b"{ \"x\":-0 }\n");
    let packed = call(&source);
    assert_eq!(packed.row,[0.0,2.0,2.0,2.0]);
    assert_eq!(&packed.heap[..6],[9.0,0.0,6.0,11.0,17.0,0.0]);
    assert_eq!(AiWeaponPolicySource::take(&packed.row,&packed.heap),source);
}

#[test]
fn charge_levels_merge_shallowly_without_changing_property_order() {
    let weapon = parse(r#"{"class":"charged","Damage":1,"ordnance":{"base":"bullet"},"charges":[{"Damage":2,"newField":true},{}]}"#).unwrap();
    let levels = weapon_levels(&weapon).unwrap();
    assert_eq!(levels.len(),3);
    assert!(levels[0].same(&weapon));
    assert!(!levels[1].same(&weapon));
    assert!(!levels[1].same(&levels[2]));
    assert!(levels[1].get("ordnance").unwrap().same(weapon.get("ordnance").unwrap()));
    assert_eq!(levels[1].get("Damage").and_then(Value::as_f64),Some(2.0));
    assert_eq!(levels[1].as_object().unwrap().keys().collect::<Vec<_>>(),["class","Damage","ordnance","charges","newField"]);
}

#[test]
fn native_service_binding_interleaves_before_parent_fields_resume() {
    use bfe_sim::world::admit::rows::ordnance::Prepared;
    let def=parse(r#"{"base":"bullet","ExplosionImpact":{"Damage":20,"class":"blast"},"ExplosionExpire":"after"}"#).unwrap();
    let mut rounds=OrdnanceRows::default();
    let mut combat=CombatRows::default();
    let mut strings=Strings::new();
    rounds.definition_with(&def,&mut strings,&mut |kind,_,def,strings,calls| {
        if kind==Prepared::Explosion {
            combat.explosion(def,strings)?;
            calls.append(&mut combat.calls);
        }
        Ok(())
    }).unwrap();
    assert_eq!(rounds.calls.iter().map(|c|c.name).collect::<Vec<_>>(),["ord_explosion","cmb_explosion","ord_def"]);
    assert_eq!(strings.list.iter().map(|v|String::from_utf16_lossy(v)).collect::<Vec<_>>(),["bullet","Damage","class","blast","after"]);
    assert_eq!(&rounds.calls[2].call.row[30..],[5.0,4.0]);
}

// Battle::load needs the ordinary asset names, but row marshaling never
// decodes geometry. Empty binary fixtures keep this Cx test safe for CI.
fn fixture()->(bfe_content::files::MemFiles,bfe_sim::world::admit::Options) {
    let mut files=bfe_content::files::MemFiles::default();
    for path in ["maps/test/map.json","common/anims.json","common/common.json","common/effects.json","sides/rep/weapons.json","sides/rep/actors/person.json"] {
        files.insert(path,b"{}".to_vec());
    }
    files.insert("sides/rep/units.json",br#"{"units":{"a":{"model":"person","base":"soldier","weapons":[{"class":"rifle","base":"launcher"}]},"b":{"model":"person","base":"soldier","weapons":[{"class":"rifle","base":"launcher"}]}},"items":{"mine":{"base":"mine","ExplosionExpire":{"Damage":1}}}}"#.to_vec());
    for path in ["sides/rep/actors/person.bin","sides/rep/actors/person_index.bin","common/anims.bin","maps/test/collision.bin","maps/test/collision_index.bin","maps/test/collision_poly.bin","maps/test/terrain_collision.bin","maps/test/terrain_collision_index.bin","sides/rep/weapons.bin","sides/rep/weapons_index.bin","common/fxmodels.bin","common/fxmodels_index.bin"] {
        files.insert(path,Vec::new());
    }
    // loadAIWeaponData forwards fetched bytes even if they are invalid JSON.
    files.insert("sides/rep/ai.json",b"  invalid policy\n".to_vec());
    let options=bfe_sim::world::admit::Options {map:"test".into(),side:"rep".into(),enhanced:false,seed:1,heroes:false,unit:Some("a".into()),name:None};
    (files,options)
}

#[test]
fn shared_context_keeps_equal_named_definitions_distinct_across_seams() {
    use bfe_sim::world::admit::rows::{combat,Cx};
    let (files,options)=fixture();
    let battle=bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files),&options.map,&options.query()).unwrap();
    let content=fixture_content(battle);
    let defs=bfe_content::defs::Defs::build(&content).unwrap();
    let battle=&content.battle;
    let units=&battle.sides[0].units;
    let a=units.get("a").unwrap().get("weapons").unwrap().at(0).unwrap();
    let b=units.get("b").unwrap().get("weapons").unwrap().at(0).unwrap();
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    let (a_id,a_calls)=combat::prepare_weapon(&mut cx,a,Stage::Combat).unwrap();
    let (b_id,b_calls)=combat::prepare_weapon(&mut cx,b,Stage::Units).unwrap();
    assert_eq!((a_id,b_id),(DefId(0),DefId(1)));
    assert!(cx.definitions.weapons[0].1.same(a));
    assert!(cx.definitions.weapons[1].1.same(b));
    assert_eq!(a_calls[0].2.args,[0.0]);
    assert_eq!(b_calls[0].2.args,[1.0]);
    assert!(!a_calls[0].2.strings.is_empty());
    assert!(b_calls[0].2.strings.is_empty());
    let (reused,calls)=combat::prepare_weapon(&mut cx,a,Stage::Units).unwrap();
    assert_eq!(reused,a_id);assert!(calls.is_empty());
    assert_eq!(bfe_sim::world::admit::rows::weapon_policies::weapon_ids(&cx).unwrap(),[DefId(0),DefId(1)]);
    assert_ne!(cx.ids.defs[0].0,cx.ids.defs[1].0);
    cx.ids.defs[0].1=9.0;
    assert!(combat::prepare_weapon(&mut cx,a,Stage::Units).is_err());
}

#[test]
fn context_entry_uses_supplied_viewpoint_id_and_original_policy_bytes() {
    use bfe_sim::world::admit::rows::{combat,items,weapon_policies,Cx};
    let (files,options)=fixture();
    let battle=bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files),&options.map,&options.query()).unwrap();
    let content=fixture_content(battle);
    let defs=bfe_content::defs::Defs::build(&content).unwrap();
    let battle=&content.battle;
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    assert_eq!(items::build(&mut cx).unwrap()[0].1,"items_pools");
    assert!(combat::build(&mut cx).is_err());
    cx.ids.registry.push(("viewpoint".into(),42.0));
    let calls=combat::build(&mut cx).unwrap();
    let constructed=calls.iter().find(|(_,name,_)|*name=="cmb_new").unwrap();
    assert_eq!(constructed.2.args[0],42.0);
    assert_eq!(calls.iter().filter(|(_,name,_)|*name=="cmb_set").count(),6);
    let call=&weapon_policies::build(&mut cx).unwrap()[0].2;
    let source=AiWeaponPolicySource::take(&call.row,&call.heap);
    assert_eq!(source.weapons,[DefId(0)]);
    assert_eq!(source.data[0].json,b"  invalid policy\n");
}

#[test]
fn shared_context_retains_unsent_reservations_until_native_content_is_needed() {
    use bfe_sim::world::admit::rows::{combat,Cx};
    let (files,options)=fixture();
    let battle=bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files),&options.map,&options.query()).unwrap();
    let content=fixture_content(battle);
    let defs=bfe_content::defs::Defs::build(&content).unwrap();
    let battle=&content.battle;
    let class=battle.sides[0].units.get("a").unwrap();
    let weapon=class.get("weapons").unwrap().at(0).unwrap();
    let item=battle.sides[0].items.get("mine").unwrap();
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    let (_,calls)=combat::with_bindings(&mut cx,Stage::Ai,|bindings,_| {
        bindings.combat.classes.id(class,Stage::Ai)?;
        bindings.combat.weapons.id(weapon,Stage::Ai)?;
        bindings.items.definitions.id(item,Stage::Ai)?;
        // A plain definition is sufficient to check F1's prefix draining.
        bindings.ordnance.definitions.id(item,Stage::Ai)?;
        Ok(((),Vec::new()))
    }).unwrap();
    assert!(calls.is_empty());
    let (_,calls)=combat::with_bindings(&mut cx,Stage::Units,|bindings,strings| {
        bindings.ordnance.definition(item,strings)?;
        let mut calls=std::mem::take(&mut bindings.ordnance.calls);
        bindings.items.definition(item,&mut bindings.ordnance,strings)?;
        calls.append(&mut bindings.items.calls);
        bindings.combat.class(class,strings)?;
        calls.append(&mut bindings.combat.calls);
        Ok(((),calls))
    }).unwrap();
    assert_eq!(calls.iter().map(|(_,name,_)|*name).collect::<Vec<_>>(),["ord_explosion","ord_def","items_def","cmb_weapon","cmb_class"]);
    let (_,calls)=combat::with_bindings(&mut cx,Stage::Units,|bindings,strings| {
        bindings.ordnance.definition(item,strings)?;
        bindings.items.definition(item,&mut bindings.ordnance,strings)?;
        bindings.combat.class(class,strings)?;
        assert!(bindings.ordnance.calls.is_empty());assert!(bindings.items.calls.is_empty());
        Ok(((),std::mem::take(&mut bindings.combat.calls)))
    }).unwrap();
    assert!(calls.is_empty());
}

#[test]
fn policy_binding_sends_reserved_weapon_gaps_once_in_original_table_order() {
    use bfe_sim::world::admit::rows::{combat,weapon_policies,Cx};
    let (files,options)=fixture();
    let battle=bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files),&options.map,&options.query()).unwrap();
    let content=fixture_content(battle);
    let defs=bfe_content::defs::Defs::build(&content).unwrap();
    let battle=&content.battle;
    let units=&battle.sides[0].units;
    let a=units.get("a").unwrap().get("weapons").unwrap().at(0).unwrap();
    let b=units.get("b").unwrap().get("weapons").unwrap().at(0).unwrap();
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    combat::with_bindings(&mut cx,Stage::Ai,|bindings,_| {
        bindings.combat.weapons.id(a,Stage::Ai)?;bindings.combat.weapons.id(b,Stage::Ai)?;
        Ok(((),Vec::new()))
    }).unwrap();
    assert_eq!(combat::prepare_weapon(&mut cx,b,Stage::Units).unwrap().1[0].2.args,[1.0]);
    let calls=weapon_policies::build(&mut cx).unwrap();
    assert_eq!(calls.iter().map(|(_,name,_)|*name).collect::<Vec<_>>(),["cmb_weapon","ai_weapon_policies"]);
    assert_eq!(calls[0].2.args,[0.0]);
    let source=AiWeaponPolicySource::take(&calls[1].2.row,&calls[1].2.heap);
    assert_eq!(source.weapons,[DefId(0),DefId(1)]);
    assert_eq!(weapon_policies::build(&mut cx).unwrap().len(),1);
}

// These tests exercise definition admission only. A shared Content can retain
// empty geometry while its Battle and Defs preserve the original Rc objects.
fn fixture_content(battle:bfe_content::battle::Battle)->bfe_content::content::Content {
    bfe_content::content::Content {battle,weapon_packs:Vec::new(),vehicle_models:None,vehicle_extras:None,props_models:None,
        weapon_models:Default::default(),weapon_model_sources:Vec::new(),actors:Vec::new(),body_clips:Vec::new(),walker_clips:Vec::new(),
        collision:bfe_content::collision::MapCollision::load(&[],&[],&[],&[],&[]).unwrap(),vehicle_templates:Vec::new(),vehicle_blockers:None,
        prop_bodies:Vec::new(),prop_clips:Vec::new(),death_regions:Vec::new(),route_words:None,placed_items:Vec::new()}
}

#[test]
fn shared_objects_keep_independent_combat_and_ordnance_explosion_ids() {
    use bfe_sim::world::admit::rows::{combat,Cx};
    use combat::DefinitionKind;
    let (files,options)=fixture();
    let battle=bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files),&options.map,&options.query()).unwrap();
    let content=fixture_content(battle);
    let defs=bfe_content::defs::Defs::build(&content).unwrap();
    let first=content.battle.sides[0].units.get("a").unwrap();
    let explosion=content.battle.sides[0].items.get("mine").unwrap().get("ExplosionExpire").unwrap();
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    combat::with_bindings(&mut cx,Stage::Units,|bindings,strings| {
        bindings.combat.explosion(first,strings)?;
        bindings.combat.explosion(explosion,strings)?;
        let mut calls=std::mem::take(&mut bindings.combat.calls);
        bindings.ordnance.explosion(explosion,strings)?;
        calls.append(&mut bindings.ordnance.calls);
        Ok(((),calls))
    }).unwrap();
    assert_eq!(combat::definition_id(&cx,DefinitionKind::CombatExplosion,explosion).unwrap(),Some(DefId(1)));
    assert_eq!(combat::definition_id(&cx,DefinitionKind::Explosion,explosion).unwrap(),Some(DefId(0)));
    let blasts=combat::registered_definitions(&cx,DefinitionKind::CombatExplosion).unwrap();
    let ordnance=combat::registered_definitions(&cx,DefinitionKind::Explosion).unwrap();
    assert!(blasts[1].1.same(&ordnance[0].1));
    assert!(ordnance[0].1.same(explosion));
}

#[test]
fn tank_guidance_keeps_field_order_types_and_distinct_construction_identity() {
    use bfe_sim::world::admit::rows::combat::vehicle_missile_definition;
    let source=parse(r#"{"class":"rep_weap_hover_fightertank_missile","base":"cannon","ai":{"minRange":0.1,"barrageMin":"3","implicitRange":0,"action":false},"ordnance":{"base":"bullet","Velocity":-0},"child":{}}"#).unwrap();
    let a=vehicle_missile_definition(&source).unwrap();
    let b=vehicle_missile_definition(&source).unwrap();
    assert!(!a.same(&b));assert!(!a.same(&source));
    assert!(a.get("child").unwrap().same(source.get("child").unwrap()));
    assert!(!a.get("ordnance").unwrap().same(b.get("ordnance").unwrap()));
    assert_eq!(a.get("base").and_then(Value::as_str),Some("launcher"));
    let ai=a.get("ai").unwrap();
    assert_eq!(ai.as_object().unwrap().entries().map(|(k,_)|k).collect::<Vec<_>>(),["minRange","optimalRange","maxRange","implicitRange","extremeRange","relationMask","categoryMask","barrageMin","barrageMax","barrageDelay","triggerSingle","zoomMax","action"]);
    assert_eq!(ai.get("minRange").unwrap().as_f64().unwrap(),0.1f32 as f64);
    assert_eq!(ai.get("barrageMin").and_then(Value::as_str),Some("3"));
    assert!(matches!(ai.get("action"),Some(Value::Bool(false))));
    assert!(matches!(ai.get("implicitRange"),Some(Value::Number(0.0))));
    assert_eq!(a.get("ordnance").unwrap().get("Velocity").unwrap().as_f64().unwrap().to_bits(),(-0.0f64).to_bits());
    let ordinary=parse(r#"{"class":"rifle"}"#).unwrap();
    assert!(ordinary.same(&vehicle_missile_definition(&ordinary).unwrap()));
}

#[test]
fn derived_definitions_restore_native_identity_and_authored_network_source() {
    use bfe_sim::world::admit::rows::{combat,Cx};
    use combat::DefinitionKind;
    let (files,options)=fixture();
    let battle=bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files),&options.map,&options.query()).unwrap();
    let content=fixture_content(battle);
    let defs=bfe_content::defs::Defs::build(&content).unwrap();
    let source=content.battle.sides[0].items.get("mine").unwrap();
    let derived=Value::Object(Rc::new(source.as_object().unwrap().clone()));
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    combat::with_bindings(&mut cx,Stage::Fleet,|bindings,strings| {
        bindings.derived_from(&derived,source)?;
        bindings.ordnance.definition(&derived,strings)?;
        Ok(((),std::mem::take(&mut bindings.ordnance.calls)))
    }).unwrap();
    assert_eq!(combat::definition_id(&cx,DefinitionKind::Ordnance,&derived).unwrap(),Some(DefId(0)));
    assert!(combat::registered_definitions(&cx,DefinitionKind::Ordnance).unwrap()[0].1.same(&derived));
    assert!(combat::source_definitions(&cx,DefinitionKind::Ordnance).unwrap()[0].1.same(source));
    let (_,calls)=combat::with_bindings(&mut cx,Stage::Units,|bindings,strings| {
        bindings.ordnance.definition(&derived,strings)?;
        Ok(((),std::mem::take(&mut bindings.ordnance.calls)))
    }).unwrap();
    assert!(calls.is_empty());
}

#[test]
fn vehicle_firing_preparation_retains_fresh_handles_and_authored_sources() {
    use bfe_sim::world::admit::rows::{combat,Cx};
    use combat::DefinitionKind;
    let (mut files,options)=fixture();
    files.insert("sides/rep/units.json",br#"{"units":{"a":{"model":"person","base":"soldier","weapons":[{"class":"rep_weap_hover_fightertank_missile","base":"cannon","ordnance":{"base":"bullet","Velocity":10}}]}},"items":{}}"#.to_vec());
    let battle=bfe_content::battle::Battle::load(&bfe_content::battle::Loader::new(&files),&options.map,&options.query()).unwrap();
    let content=fixture_content(battle);
    let defs=bfe_content::defs::Defs::build(&content).unwrap();
    let source=content.battle.sides[0].units.get("a").unwrap().get("weapons").unwrap().at(0).unwrap();
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    let ((a,weapon,ordnance),calls)=combat::prepare_vehicle_weapon(&mut cx,source,Stage::Fleet).unwrap();
    assert_eq!((weapon,ordnance),(DefId(0),Some(DefId(0))));
    assert_eq!(calls.iter().map(|(_,name,_)|*name).collect::<Vec<_>>(),["cmb_weapon","ord_def"]);
    let ((b,weapon,ordnance),_)=combat::prepare_vehicle_weapon(&mut cx,source,Stage::Fleet).unwrap();
    assert_eq!((weapon,ordnance),(DefId(1),Some(DefId(1))));assert!(!a.same(&b));
    for (_,value) in combat::source_definitions(&cx,DefinitionKind::Weapon).unwrap() {assert!(value.same(source));}
    for (_,value) in combat::source_definitions(&cx,DefinitionKind::Ordnance).unwrap() {assert!(value.same(source.get("ordnance").unwrap()));}
    let ordinary=content.battle.sides[0].units.get("a").unwrap();
    let ((_,_,round),_)=combat::prepare_vehicle_weapon(&mut cx,ordinary,Stage::Fleet).unwrap();
    assert_eq!(round,None);
    // A later derivative follows the original source, as derivedFrom does.
    let child=Value::Object(Rc::new(a.as_object().unwrap().clone()));
    combat::with_bindings(&mut cx,Stage::Units,|bindings,strings| {
        bindings.derived_from(&child,&a)?;bindings.combat.weapon(&child,strings)?;
        Ok(((),std::mem::take(&mut bindings.combat.calls)))
    }).unwrap();
    assert!(combat::source_definitions(&cx,DefinitionKind::Weapon).unwrap()[3].1.same(source));
}
