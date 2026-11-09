use bfe_content::{actor::build_actor, json::parse, Value};
use bfe_sim::{gen::*, world::admit::rows::units};

fn json(text: &str) -> Value { parse(text).expect("synthetic JSON") }

#[test]
fn init_and_missile_vocabulary_keep_absence_and_alias() {
    let row = units::Init::new(9, None, 12).call().args;
    assert_eq!(row[0], 9.0); assert!(row[1].is_nan()); assert_eq!(row[2], 12.0);
    assert_eq!(units::Init::new(0, Some(-0.0), 0).call().args[1].to_bits(), (-0.0f64).to_bits());
    let mut strings = Vec::new();
    let rows = units::missile_classes(|name| { strings.push(name.to_owned()); StrId(strings.len() as u32 - 1) });
    assert_eq!(strings, ["", "fighter", "scout", "bomber", "transport"]);
    assert_eq!(rows[0].class, rows[1].class);
    assert_eq!(rows[4].class, FlightClass::Transport);
}

#[test]
fn skeleton_uses_decomposed_bind_and_actual_root_parent() {
    let def = json(r#"{"bones":[{"name":"root","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,2,3,4,1]},{"name":"child","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#);
    let actor = build_actor(&def, &[]).expect("actor");
    let skeleton = units::body_skeleton(&actor, F3UnitKey {slot:7,generation:2,life:3.0}, |_| StrId(4)).expect("skeleton");
    assert_eq!(skeleton.bones[0].parent, -1);
    assert_eq!(skeleton.bones[0].position, [2.0,3.0,4.0]);
    let call = units::call(F3BodySkeleton::WORDS, |row, heap| skeleton.put(row, heap));
    assert_eq!(call.heap.len(), 2 * F3BodyBone::WORDS);
    assert_eq!(F3BodySkeleton::take(&call.row, &call.heap), skeleton);
}

#[test]
fn droideka_keeps_mode_holes_null_tags_and_negative_zero() {
    let class = units::droideka_class(&json(r#"{"MaxPitchSpeed":-0,"BallTurnSpeed":null,"BallMaxLean":"2.5"}"#), |name| if name == "fold" {Some(17)} else {None});
    assert_eq!(class.max_pitch_speed.map(f64::to_bits), Some((-0.0f64).to_bits()));
    assert_eq!(class.ball_turn_speed, None); assert_eq!(class.ball_max_lean, Some(2.5));
    assert_eq!(class.clips[11], 17.0); assert_eq!(class.clips[4], -1.0); assert_eq!(class.clips[6], -1.0);
}

#[test]
fn impact_defaults_distinguish_truthy_damage_from_nullish_scales() {
    let policy = units::hit_policy(DefId(8),&json(r#"{"Damage":"bad","PersonScale":-0,"AnimalScale":null,"DroidScale":"2","ShieldScale":false}"#));
    assert!(policy.damage.is_nan());
    assert_eq!(policy.person.to_bits(),(-0.0f64).to_bits());
    assert_eq!(policy.animal,1.0); assert_eq!(policy.droid,2.0); assert_eq!(policy.shield,0.0);
    assert!(!units::impact_rules(Some(&Value::Bool(false))).friendly_fire);
    assert!(units::impact_rules(Some(&Value::Number(0.0))).friendly_fire);
}

#[test]
fn charge_services_keep_base_bank_and_channel_with_level_lock_masks() {
    let def = json(r#"{"channel":1,"AnimationBank":"pistol","LockOnAngle":3,"TargetAll":0,"TargetDroid":1,"charges":[{"AnimationBank":"rifle","channel":9,"LockOffAngle":0,"LockOnRange":"30"}]}"#);
    let levels = units::weapon_levels(&def).expect("levels");
    let mut names = Vec::new();
    let row = units::weapon_service(&def,&levels[1],Some(DefId(9)),None,|n| {names.push(n.to_owned());StrId(names.len() as u32)},false);
    assert_eq!(row.channel,1.0); assert_eq!(names,["pistol",""]);
    assert_eq!(row.lock.off_angle,3.0); assert_eq!(row.lock.range,30.0); assert_eq!(row.lock.category_mask,4);
    let ai = units::lock_rule(&json(r#"{"TargetAll":0,"TargetEnemy":0,"ai":{"relationMask":-1}}"#));
    assert_eq!(ai.relation_mask,u32::MAX); assert_eq!(ai.category_mask,0xffff_ff9f);
}

#[test]
fn item_groups_and_missing_drops_keep_first_use_order_and_tags() {
    let mut groups = units::ItemGroups::default();
    let mine = groups.prepare(&json(r#"{"base":"mine","LifeSpan":-0,"PersonHealth":"bad"}"#),DefId(9),Some(0.0),ModelAnswer::default());
    let powerup = groups.prepare(&json(r#"{"base":"powerup"}"#),DefId(2),None,ModelAnswer::default());
    let again = groups.prepare(&json(r#"{"base":"mine"}"#),DefId(3),None,ModelAnswer::default());
    assert!(mine.mine); assert_eq!(mine.base,again.base); assert_eq!(powerup.base,1);
    assert_eq!(mine.life.to_bits(),(-0.0f64).to_bits()); assert!(mine.health_gain[0].is_nan());
    assert!(powerup.pool.is_infinite());
    let drops = units::unit_items(&json(r#"{"drops":[{"class":"missing","probability":"0.25"},{"class":"known","probability":false},{"class":"known"}]}"#),
        DefId(4),None,|name|Ok(if name=="known" {Some(DefId(7))} else {None}),|_|StrId(12)).expect("drops");
    assert_eq!(drops.drops.len(),3); assert_eq!(drops.drops[0].def,None);
    assert_eq!(drops.drops[0].probability_tag,5); assert_eq!(drops.drops[1].probability_tag,2);
    assert_eq!(drops.drops[2].probability_tag,0); assert!(drops.add_health.is_nan());
}

#[test]
fn hull_bounds_use_sqrt_diagonal_and_separate_explosion_ids() {
    let identity = units::HullIdentity {hull:Ref::new(7,3),member:None,category:3,kind:2.0,key:StrId(2),health:3};
    let hull = units::hull(identity,[6.0,8.0,0.0],[1.0,2.0,3.0],Some([9.0,8.0,7.0,4.0]),Some(2),Some(3));
    assert_eq!(hull.blast_radius,5.0); assert_eq!(hull.blast_center,[9.0,8.0,7.0]);
    assert_eq!(hull.explosion,Some(2)); assert_eq!(hull.final_explosion,Some(3));
    assert!(units::registry_add(KIND_ITEM).is_err());
}

#[test]
fn building_birth_reuses_life_and_keeps_nested_look_slots() {
    use bfe_sim::world::admit::rows::props;
    let desc = json(r#"{"base":"powerupstation","model":"station","unbuilt":{"model":"pad"},"settings":{"MaxHealth":"200hp","PowerupDelay":["0.25s"],"SoldierHealth":"5hp"},"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,3,4,5,1]}"#);
    let models = json(r#"{"station":{"sphere":[0,1,0,2]},"pad":{"box":[[-1,0,-1],[1,1,1]]}}"#);
    let binding = props::Binding {index:3,target:RepairTarget::Building(Ref::new(2,0)),member:None,bodies:[None;3],
        generation:None,team:0.0,target_team:0.0,category:0,kind:0.0,damaged:false,crew:false,trap:None};
    let row = props::building(&desc,&models,binding,None,|_|StrId(1)).expect("building");
    assert_eq!(row.state.state,PbStateName::Unbuilt); assert_eq!(row.state.health,0.0);
    assert_eq!(row.settings.max_health,200.0); assert_eq!(row.settings.models.len(),3);
    assert!(row.settings.models[1].is_none()); assert_eq!(row.live.state.building.health,row.state.health);
    let supply = row.live.supply.as_ref().expect("supply");
    assert_eq!(supply.position,[3.0,4.0,5.0]); assert_eq!(supply.delay,0.25); assert_eq!(supply.health,5.0);
    let encoded = units::call(F3BuildingSetup::WORDS,|r,h|row.put(r,h));
    let decoded = F3BuildingSetup::take(&encoded.row,&encoded.heap);
    assert_eq!(decoded.live,row.live);
    assert!(decoded.state.trap_state.health.is_nan());
}

#[test]
fn recon_calls_preserve_strike_level_class_order_and_shared_identity() {
    use bfe_sim::world::admit::{AdmitError,rows::recon};
    #[derive(Default)]
    struct Definitions { strings: Vec<String>, weapons: Vec<Value>, ordnance: Vec<Value> }
    fn id(values: &mut Vec<Value>, def: &Value) -> DefId {
        let at = values.iter().position(|v|bfe_content::defs::identity(v)==bfe_content::defs::identity(def));
        DefId(at.unwrap_or_else(|| { values.push(def.clone());values.len()-1 }) as u32)
    }
    impl recon::Definitions for Definitions {
        fn intern(&mut self, text: &str) -> StrId {
            let at = self.strings.iter().position(|v|v==text).unwrap_or_else(|| {self.strings.push(text.to_owned());self.strings.len()-1});
            StrId(at as u32)
        }
        fn weapon(&mut self,def: &Value)->Result<DefId,AdmitError> {Ok(id(&mut self.weapons,def))}
        fn ordnance(&mut self,def: &Value)->Result<DefId,AdmitError> {Ok(id(&mut self.ordnance,def))}
        fn explosion(&mut self,_def: &Value)->Result<u32,AdmitError> {Ok(6)}
    }
    let weapon = json(r#"{"SwitchImmediately":1,"droid":{"weapons":[{"strike":{"Damage":2},"charges":[{"ShotDelay":0.5}]}],"explosion":{"Damage":5}}}"#);
    let mut classes = recon::Classes::default(); let mut defs = Definitions::default();
    let calls = classes.prepare(&weapon,&mut defs,Some([1.0,2.0,3.0])).expect("recon");
    assert_eq!(calls.iter().map(|(_,name,_)|*name).collect::<Vec<_>>(),
        ["f3_recon_strike","f3_recon_level","f3_recon_strike","f3_recon_level","f3_recon_class","f3_recon_weapon"]);
    let class = F3ReconClass::take(&calls[4].2.row,&calls[4].2.heap);
    assert_eq!(class.muzzle,[1.0,2.0,3.0]); assert_eq!(class.explosion,Some(6));
    let level = F3ReconLevel::take(&calls[3].2.row,&calls[3].2.heap);
    assert_eq!(level.level,1); assert_eq!(level.strike,Some(1));
    let repeat = classes.prepare(&weapon,&mut defs,None).expect("shared class");
    assert_eq!(repeat.len(),1); assert_eq!(repeat[0].1,"f3_recon_weapon");
    assert_eq!(classes.definitions().len(),1);
    assert!(classes.definitions()[0].same(weapon.get("droid").expect("droid definition")));
    assert_eq!(classes.strikes().len(),2);
    let binding = recon::binding(F3ReconAction::Attach,4,F3UnitKey {life:3.0,..Default::default()},recon::seed(991,"owner"));
    assert_eq!(binding.sensitivity,1.0); assert!(binding.droid.is_none());
}

#[test]
fn building_scratch_retains_trap_words_and_member_category() {
    use bfe_sim::world::admit::rows::props;
    let matrix = r#""matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]"#;
    let trap = json(&format!(r#"{{"base":"trap","model":"m","settings":{{}},"trap":{{}},{matrix}}}"#));
    let station = json(&format!(r#"{{"base":"powerupstation","model":"m","settings":{{}},{matrix}}}"#));
    let models = json(r#"{"m":{}}"#);
    let mut buildings = props::Buildings::default();
    let binding = props::Binding {index:0,target:RepairTarget::Building(Ref::new(0,0)),member:Some(Member::ObjectiveTarget(Ref::new(2,0))),
        bodies:[None;3],generation:Some(0.0),team:0.0,target_team:0.0,category:4,kind:9.0,damaged:false,crew:false,
        trap:Some(props::Trap {pad_sphere:Bounds::default(),point:[0.0;3],effect_at:[0.0;3],build_frames:None,trigger_frames:None})};
    let first = buildings.prepare(&trap,&models,binding.clone(),None,|_|StrId(0)).expect("trap");
    let mut next = binding; next.index=1;next.member=None;next.trap=None;next.category=0;next.kind=0.0;
    let second = buildings.prepare(&station,&models,next,None,|_|StrId(0)).expect("station");
    assert_eq!(second.category,4); assert_eq!(second.kind,9.0);
    assert_eq!(second.state.trap_state,first.state.trap_state); assert!(!second.state.trap_mode);
}

#[test]
fn export_arguments_do_not_shift_generated_rows_and_relations_are_row_major() {
    let row = units::Entry::WeaponServices {weapon:DefId(19),levels:vec![F3WeaponService::default();2]}.encode().expect("export");
    assert_eq!(row.arguments,[19.0,2.0]); assert_eq!(row.call.row.len(),2*F3WeaponService::WORDS);
    let scalar = units::Entry::Init(units::Init::new(4,None,11)).encode().expect("init");
    assert!(scalar.call.row.is_empty()); assert!(scalar.arguments[1].is_nan());
    let relations = units::relations(2,|a,b|a*10.0+b).expect("relations");
    assert_eq!(relations.len(),36); assert_eq!(relations[7].value,11.0);
    assert!(units::relations(usize::MAX,|_,_|0.0).is_err());
}

#[test]
fn turret_birth_keeps_null_attacker_in_both_life_frames() {
    use bfe_sim::world::admit::rows::props;
    let desc=json(r#"{"base":"armedbuilding","model":"m","settings":{},"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}"#);
    let models=json(r#"{"m":{}}"#);
    let binding=props::Binding {index:0,target:RepairTarget::Turret(Ref::new(0,0)),member:None,bodies:[None;3],
        generation:None,team:0.0,target_team:0.0,category:0,kind:0.0,damaged:false,crew:false,trap:None};
    let row=props::building(&desc,&models,binding,None,|_|StrId(0)).expect("turret");
    assert_eq!(row.state.attacker,Attacker::Null);assert_eq!(row.live.attacker,Attacker::Null);
    assert_eq!(row.live.state.building.last_attacker.kind,1.0);
}

#[test]
fn lightning_library_uses_last_assignment_even_when_empty() {
    let first=json(r#"{"bolt":[{"lightning":{}}]}"#);let empty=json(r#"{"bolt":[]}"#);
    let unrelated=json(r#"{"other":[]}"#);
    assert!(units::lightning_in("bolt",[&first,&unrelated]));
    assert!(!units::lightning_in("bolt",[&first,&empty]));
    assert!(units::lightning_in("bolt",[&empty,&first]));
}

// Exercise the real shared-identity seam with a small synthetic asset set.
// No simulation return or definition id is supplied by a recording here.
fn service_fixture() -> (bfe_content::files::MemFiles,bfe_sim::world::admit::Options) {
    let mut files=bfe_content::files::MemFiles::default();
    for path in ["maps/test/map.json","common/anims.json","common/common.json","common/effects.json","sides/rep/weapons.json"] {
        files.insert(path,b"{}".to_vec());
    }
    files.insert("common/anims.json",br#"{"clips":{}}"#.to_vec());
    files.insert("sides/rep/actors/person.json",br#"{"bones":[],"parts":[]}"#.to_vec());
    // This impact is separate from the object-valued drop probability:
    // both create blasts, but only the impact must precede its hit policy.
    let impact_round=r#"{"Damage":4,"ExplosionImpact":{"Damage":2}}"#;
    let unit_source=r#"{"units":{"a":{"model":"person","base":"soldier","weapons":[{"class":"dispenser","base":"launcher","droid":{"weapons":[{"ordnance":{"Damage":8},"strike":{"Damage":10},"charges":[{"ShotDelay":0.4}]}]},"ordnance":__IMPACT_ROUND__,"item":{"base":"mine","MaxHealth":30}}],"drops":[{"class":"mine","probability":{"Damage":9}},{"class":"absent","probability":"0.2"}]},"b":{"model":"person","base":"soldier","weapons":[{"class":"dispenser","base":"launcher","droid":{"weapons":[{"strike":{"Damage":11}}]},"item":{"base":"mine"}}]}},"items":{"mine":{"base":"mine"}}}"#.replace("__IMPACT_ROUND__",impact_round);
    files.insert("sides/rep/units.json",unit_source.into_bytes());
    for path in ["sides/rep/actors/person.bin","sides/rep/actors/person_index.bin","common/anims.bin","maps/test/collision.bin","maps/test/collision_index.bin","maps/test/collision_poly.bin","maps/test/terrain_collision.bin","maps/test/terrain_collision_index.bin","sides/rep/weapons.bin","sides/rep/weapons_index.bin","common/fxmodels.bin","common/fxmodels_index.bin"] {
        files.insert(path,Vec::new());
    }
    let options=bfe_sim::world::admit::Options {map:"test".into(),side:"rep".into(),enhanced:false,seed:1,heroes:false,unit:Some("a".into())};
    (files,options)
}

#[test]
fn context_services_preserve_lazy_hooks_groups_and_per_class_caches() {
    use bfe_content::{battle::{Battle,Loader},content::Content,defs::Defs};
    use bfe_sim::{store::Strings,world::admit::{Stage,rows::{combat,Cx}}};
    let (files,options)=service_fixture();
    let content=Content::build(Battle::load(&Loader::new(&files),&options.map,&options.query()).unwrap()).unwrap();
    let defs=Defs::build(&content).unwrap();
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    let a=&defs.classes[0];let b=&defs.classes[1];
    // Combat owns these allocations before the body's F3 service seam.
    combat::attach_unit(&mut cx,0,&a.record,false,Stage::Player).unwrap();
    let key=F3UnitKey {slot:0,generation:0,life:0.0};
    let first=units::prepare_unit_services(&mut cx,a,key.clone()).unwrap();
    let names=first.iter().map(|(_,name,_)|*name).collect::<Vec<_>>();
    let policy=first.iter().position(|(_,name,call)|*name=="f3_hit_policy"&&F3HitPolicy::take(&call.row,&call.heap).damage==4.0).unwrap();
    assert_eq!(names[policy-1],"ord_def");
    let impact=a.record.get("weapons").unwrap().at(0).unwrap().get("ordnance").unwrap()
        .get("ExplosionImpact").expect("fixture must use F1's ExplosionImpact field");
    let impact_key=&cx.definitions.explosions.iter().find(|(key,value)|key.starts_with("explosion:")&&value.same(impact))
        .expect("impact retains its original definition identity").0;
    let impact_id=cx.ids.ordnance.iter().find(|(key,_)|key==impact_key).expect("impact has its assigned F1 id").1 as u32;
    let blast=first.iter().position(|(_,name,call)|*name=="f3_blast_binding"
        &&F3BlastBinding::take(&call.row,&call.heap).explosion==impact_id).expect("parent impact has its own blast binding");
    assert_eq!(&names[blast-2..blast],&["ord_explosion","cmb_explosion"]);
    assert!(blast<policy,"parent impact binding must precede its ordnance hit policy");
    let classes=first.iter().filter(|(_,name,_)|*name=="f3_item_class")
        .map(|(_,_,call)|F3ItemClass::take(&call.row,&call.heap)).collect::<Vec<_>>();
    assert_eq!(classes.len(),2);assert!(classes.iter().all(|row|row.base==0));
    let drops=first.iter().find(|(_,name,_)|*name=="f3_unit_items").unwrap();
    let drops=F3UnitItems::take(&drops.2.row,&drops.2.heap);
    assert_eq!(drops.drops[0].probability_tag,6);
    assert_eq!(drops.drops[1].def,None);assert_eq!(drops.drops[1].probability_tag,5);
    let values=bfe_sim::world::admit::session::DefinitionValues::from_context(&cx).unwrap();
    assert_eq!(values.remotes.len(),1);
    assert!(values.remotes[0].same(a.record.get("weapons").unwrap().at(0).unwrap().get("droid").unwrap()));
    assert!(units::prepare_unit_services(&mut cx,a,F3UnitKey {slot:1,..key.clone()}).unwrap().is_empty());
    combat::attach_unit(&mut cx,2,&b.record,false,Stage::Player).unwrap();
    let second=units::prepare_unit_services(&mut cx,b,F3UnitKey {slot:2,..key.clone()}).unwrap();
    let strike=second.iter().find(|(_,name,_)|*name=="f3_recon_strike").unwrap();
    assert_eq!(F3ReconStrike::take(&strike.2.row,&strike.2.heap).id,2);
    assert_eq!(cx.definitions.recon.len(),2);
    let item=second.iter().find(|(_,name,_)|*name=="f3_item_class").unwrap();
    assert_eq!(F3ItemClass::take(&item.2.row,&item.2.heap).base,0);
    let dispensers=units::dispenser_calls(&mut cx,a,key).unwrap();
    assert_eq!(dispensers.len(),1);assert_eq!(dispensers[0].1,"f3_dispenser_binding");
}
