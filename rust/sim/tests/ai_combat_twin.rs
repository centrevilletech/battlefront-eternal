//! Source-oracle twins from tools/ai-combat-twin.mjs. The input file is
//! synthetic and requires no retail content; set BFE_AI_COMBAT_TWIN.
use bfe_content::{json, Value};
use bfe_sim::ai::combat::{aim, binding::{CombatAi, Event, EventRef}, enhanced, perception, weapons_ai};
use bfe_sim::ai::combat::world::*;
use bfe_sim::ai::random::ClassicRandom;
use bfe_sim::combat::defs::{Js, JsObj};
use std::rc::Rc;
use three::Vector3;

fn field<'a>(v: &'a Value, k: &str) -> &'a Value { v.get(k).unwrap_or_else(|| panic!("missing {k}")) }
fn number(v: &Value) -> f64 { f64::from_bits(u64::from_str_radix(field(v,"$f64").as_str().unwrap(),16).unwrap()) }
fn n(v: &Value,k: &str) -> f64 { number(field(v,k)) }
fn boolean(v: &Value) -> bool { let Value::Bool(b)=v else { panic!("not boolean {v:?}") }; *b }
fn array(v: &Value) -> &[Value] { v.as_array().unwrap() }
fn vec(v: &Value) -> Vector3 { Vector3::new(number(&array(v)[0]),number(&array(v)[1]),number(&array(v)[2])) }
fn same(a: f64,b: f64) -> bool { a.to_bits()==b.to_bits() || a.is_nan() && b.is_nan() }
fn eq(a: f64,b: f64) { assert!(same(a,b), "native {a:?} {:016x}; JS {b:?} {:016x}",a.to_bits(),b.to_bits()); }
fn eqv(a: Vector3,b: Vector3) { eq(a.x,b.x); eq(a.y,b.y); eq(a.z,b.z); }
fn js(v: &Value) -> Js {
    if v.get("$f64").is_some() { return Js::Num(number(v)); }
    match v {
        Value::Null => Js::Null, Value::Bool(b) => Js::Bool(*b), Value::String(s) => Js::Str(s.clone()),
        Value::Array(a) => Js::Arr(Rc::new(a.iter().map(js).collect())),
        Value::Object(o) => Js::Obj(Rc::new(JsObj { fields:o.entries().map(|(k,v)|(k.into(),js(v))).collect(),def:None })),
        Value::Number(n) => Js::Num(*n),
    }
}
fn obj(v:&Value)->Rc<JsObj> { let Js::Obj(o)=js(v) else { panic!("not object") };o }
fn fixture()->Option<Value> {
    let path=std::env::var_os("BFE_AI_COMBAT_TWIN")?;
    let v=json::parse_bytes(&std::fs::read(path).unwrap()).unwrap();
    assert!(field(&v,"v8").as_str().unwrap().starts_with("13.6."));
    for (key,count) in [("heaps",6),("filter",2048),("aimer",8),("ballistic",2048),("weapons",256),("buttons",2048),("priorities",144),("enhanced",9),("ingest",128)] { assert_eq!(array(field(&v,key)).len(),count,"{key} cases"); }
    for c in array(field(&v,"enhanced")) {assert_eq!(array(field(c,"steps")).len(),256);}
    for c in array(field(&v,"aimer")) {assert_eq!(array(c).len(),128);}
    Some(v)
}

#[test]
fn priority_heaps_match_the_source() {
    let Some(data)=fixture() else { return };
    let mut count=0;
    for run in array(field(&data,"heaps")) {
        let mut heap=perception::PriorityHeap::new(n(run,"capacity") as usize,20000.0);
        for op in array(field(run,"ops")) {
            let item=n(op,"item") as u32;
            let result=match field(op,"op").as_str().unwrap() {
                "clear"=>{heap.clear();None}, "pop"=>heap.pop().flatten(),
                "insert"=>match heap.insert(n(op,"priority"),Some(item)) {
                    perception::Inserted::Placed=>None,perception::Inserted::Displaced(old)=>old,perception::Inserted::Dropped=>Some(item),
                }, _=>unreachable!(),
            };
            let want=match field(op,"result") { Value::Null=>None,v=>Some(number(v) as u32) };
            assert_eq!(result,want);eq(heap.count as f64,n(op,"count"));eq(heap.dropped,n(op,"dropped"));
            for (i,v) in array(field(op,"priorityAfter")).iter().enumerate() { eq(heap.priority[i] as f64,number(v)); }
            for (i,v) in array(field(op,"items")).iter().enumerate() { assert_eq!(heap.items[i+1],match v {Value::Null=>None,v=>Some(number(v) as u32)}); }
            count+=1;
        }
    }
    assert_eq!(count,7200);
}

#[test]
fn classic_aim_and_ballistic_leads_match_the_source() {
    let Some(data)=fixture() else { return };
    for c in array(field(&data,"filter")) {
        let mut v=vec(field(c,"v"));
        let out=aim::filter_direction(&vec(field(c,"d")),&vec(field(c,"q")),&mut v,n(c,"dt"),n(c,"A"),n(c,"D"));
        eqv(out,vec(field(c,"out")));eqv(v,vec(field(c,"velocity")));
    }
    for run in array(field(&data,"aimer")) {
        let mut aimer=aim::Aimer::default();
        for c in array(run) {
            let d=field(c,"curves");let curves=Curves{a:n(d,"A"),d:n(d,"D"),b:Some(n(d,"B")),s:Some(n(d,"S"))};
            let (out,scales)=aimer.update(&vec(field(c,"desired")),n(c,"dt"),Some(&curves),Some(&obj(field(c,"def"))),boolean(field(c,"hero")));
            eqv(out,vec(field(c,"out")));eqv(aimer.velocity,vec(field(c,"velocity")));assert_eq!(aimer.started,boolean(field(c,"started")));
            let k=field(c,"keptCurves");eq(aimer.curves.a,n(k,"A"));eq(aimer.curves.d,n(k,"D"));eq(aimer.curves.b,n(k,"B"));eq(aimer.curves.s,n(k,"S"));
            eq(scales.as_ref().map_or(-1.0,|s|s.spread_scale),n(c,"spread"));eq(scales.as_ref().map_or(-1.0,|s|s.sniper_scale),n(c,"sniper"));
        }
    }
    for c in array(field(&data,"ballistic")) {
        let (out,valid)=aim::ballistic_lead(&vec(field(c,"p")),&vec(field(c,"v")),&vec(field(c,"origin")),n(c,"speed"),n(c,"gravity"));
        eqv(out,vec(field(c,"out")));assert_eq!(valid,boolean(field(c,"valid")));
    }
    assert_eq!(array(field(&data,"filter")).len(),2048);assert_eq!(array(field(&data,"ballistic")).len(),2048);
}

#[test]
fn weapon_records_triggers_and_priorities_match_the_source() {
    let Some(data)=fixture() else { return };
    let mut table=weapons_ai::WeaponAiTable::default();
    for c in array(field(&data,"weapons")) {
        let def=obj(field(c,"def"));let registered=field(c,"registered");
        if !matches!(registered,Value::Null) { table.register(&[(def.get("class").str().unwrap().into(),weapons_ai::Registered {ai:js(field(registered,"ai")),levels:vec![]})]); }
        let a=table.weapon_ai(&def);let e=field(c,"expected");
        for (x,key) in [(a.min_range,"minRange"),(a.optimal_range,"optimalRange"),(a.max_range,"maxRange"),(a.extreme_range,"extremeRange"),
            (a.relation_mask as f64,"relationMask"),(a.category_mask as f64,"categoryMask"),(a.barrage_min,"barrageMin"),(a.barrage_max,"barrageMax"),
            (a.barrage_delay,"barrageDelay"),(a.zoom_max,"zoomMax"),(a.action,"action")] { eq(x,n(e,key)); }
        assert_eq!(a.implicit_range,boolean(field(e,"implicitRange")));assert_eq!(a.trigger_single,boolean(field(e,"triggerSingle")));
    }
    let mut b=weapons_ai::Button::default();
    for c in array(field(&data,"buttons")) {
        let prev=b.count;weapons_ai::press_button(&mut b,boolean(field(c,"down")));eq(b.count,n(c,"count"));eq(b.code,n(c,"code"));
        let (held,pressed)=weapons_ai::trigger_from_counts(prev,b.count);let t=field(c,"trigger");assert_eq!(held,boolean(field(t,"held")));assert_eq!(pressed,boolean(field(t,"pressed")));
    }
    for c in array(field(&data,"priorities")) { eq(perception::matchup_priority(n(c,"source"),n(c,"target")),n(c,"expected")); }
}

#[test]
fn enhanced_reaction_noise_and_missing_target_ids_match_the_source() {
    let Some(data)=fixture() else { return };
    for run in array(field(&data,"enhanced")) {
        let mut w=AimWorld {random:ClassicRandom::new(n(run,"seed")),calls:0,unit:UnitView::default(), units:Vec::new(), ctls:Vec::new(), fallback_team:1.0};
        let mut aimer=enhanced::EnhancedAimer::new(enhanced::profile(n(run,"tier")),n(run,"skill"));
        for c in array(field(run,"steps")) {
            let target=field(c,"target");let target=if matches!(target,Value::Null) {None} else {
                w.unit.id=target.get("id").map(number);w.unit.generation=n(target,"generation");Some(1)
            };
            aimer.observe(&mut w,target,n(c,"time"));
            let out=aimer.update(&mut w,&vec(field(c,"desired")),n(c,"dt"),n(c,"distance"),n(c,"speedFraction"),boolean(field(c,"scoped")),n(c,"time"),boolean(field(c,"hero")));
            eqv(out,vec(field(c,"out")));assert_eq!(aimer.started,boolean(field(c,"started")));
            for i in 0..2 {eq(aimer.error[i],number(&array(field(c,"error"))[i]));eq(aimer.bias[i],number(&array(field(c,"bias"))[i]));}
            assert_eq!(aimer.key.as_deref(),field(c,"key").as_str());assert_eq!(aimer.last_key.as_deref(),field(c,"lastKey").as_str());
            eq(aimer.lost_at,n(c,"lostAt"));eq(aimer.acquired_at,n(c,"acquiredAt"));eq(aimer.ready_at,n(c,"readyAt"));
            assert_eq!(aimer.reacting(n(c,"time")),boolean(field(c,"reacting")));eq(w.random.state as f64,n(c,"randomState"));eq(w.calls as f64,n(c,"calls"));
        }
    }
}

struct AimWorld { random:ClassicRandom,calls:usize,unit:UnitView,units:Vec<UnitView>,ctls:Vec<Ctl>,fallback_team:f64 }
impl Draws for AimWorld {
    fn uniform_a(&mut self)->f64 {panic!("Enhanced aim drew from A")}
    fn uniform_b(&mut self)->f64 {panic!("Enhanced aim drew from B")}
    fn uniform_c(&mut self)->f64 {self.calls+=1;self.random.uniform()}
}

#[allow(unused_variables)]
impl World for AimWorld {
    fn time(&self) -> f64 { panic!("unexpected world call in the aim twin") }
    fn difficulty(&self) -> DifficultyView { panic!("unexpected world call in the aim twin") }
    fn mission_stay_in_turrets(&self) -> bool { panic!("unexpected world call in the aim twin") }
    fn mission_no_jet_obstacle_test(&self) -> bool { panic!("unexpected world call in the aim twin") }
    fn ai_stay_in_turrets(&mut self) -> bool { panic!("unexpected world call in the aim twin") }
    fn unit(&self, u: Obj) -> &UnitView { if self.units.is_empty() { &self.unit } else { &self.units[(u-1) as usize] } }
    fn unit_mut(&mut self, u: Obj) -> &mut UnitView { panic!("unexpected world call in the aim twin") }
    fn origin(&mut self, u: Obj) -> Vector3 { panic!("unexpected world call in the aim twin") }
    fn forward(&mut self, u: Obj) -> Vector3 { panic!("unexpected world call in the aim twin") }
    fn aim_point(&mut self, u: Obj) -> Vector3 { panic!("unexpected world call in the aim twin") }
    fn sphere(&mut self, u: Obj) -> Sphere { panic!("unexpected world call in the aim twin") }
    fn set_intent_buttons(&mut self, u: Obj, b0: f64, b1: f64) { panic!("unexpected world call in the aim twin") }
    fn set_intent_aim(&mut self, u: Obj, aim: Option<Vector3>) { panic!("unexpected world call in the aim twin") }
    fn arsenal_select(&mut self, u: Obj, index: usize) -> bool { panic!("unexpected world call in the aim twin") }
    fn use_classic_random(&mut self, u: Obj) { panic!("unexpected world call in the aim twin") }
    fn query(&mut self, center: &Vector3, radius: f64, filter: QueryFilter) -> Vec<Obj> { panic!("unexpected world call in the aim twin") }
    fn relation(&mut self, a: f64, b: f64) -> f64 { if a==b {1.0} else {-1.0} }
    fn living(&mut self) -> Vec<Obj> { panic!("unexpected world call in the aim twin") }
    fn los(&mut self, from: &Vector3, to: &Vector3, a: Option<Obj>, b: Option<Obj>) -> bool { panic!("unexpected world call in the aim twin") }
    fn curves_for(&mut self, team: f64) -> Option<Curves> { panic!("unexpected world call in the aim twin") }
    fn character_team(&self, character: Obj) -> f64 { if character==201 {1.0} else {assert_eq!(character,202);self.fallback_team} }
    fn ctl(&self, c: Obj) -> &Ctl { &self.ctls[(c-101) as usize] }
    fn ctl_mut(&mut self, c: Obj) -> &mut Ctl { &mut self.ctls[(c-101) as usize] }
    fn agent_controller(&self, agent: Obj) -> Obj { panic!("unexpected world call in the aim twin") }
    fn tracker(&mut self, agent: Obj) -> Option<&mut bfe_sim::ai::combat::perception::TargetTracker> { panic!("unexpected world call in the aim twin") }
    fn is_controller(&self, c: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn agent_accepts_alert(&mut self, agent: Obj, known: bool, point: &Vector3) -> bool { panic!("unexpected world call in the aim twin") }
    fn agent_accepts_hazard(&mut self, agent: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn agent_accepts_acoustic(&mut self, agent: Obj, source: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn shared_target(&mut self, controller: Obj) -> Option<Obj> { panic!("unexpected world call in the aim twin") }
    fn has_vehicle_dispatch(&self) -> bool { panic!("unexpected world call in the aim twin") }
    fn vehicle_dispatch(&mut self, controller: Obj, unit: Obj, target: Obj, switch_allowed: bool) -> Engagement { panic!("unexpected world call in the aim twin") }
    fn has_mount_reach(&self) -> bool { panic!("unexpected world call in the aim twin") }
    fn mount_reach(&mut self, unit: Obj, point: &Vector3) -> bool { panic!("unexpected world call in the aim twin") }
    fn current_controllable(&mut self, character: Obj) -> (bool, Option<Obj>) { panic!("unexpected world call in the aim twin") }
    fn character_remote(&self, character: Obj) -> Option<Obj> { panic!("unexpected world call in the aim twin") }
    fn character_unit(&self, character: Obj) -> Option<Obj> { panic!("unexpected world call in the aim twin") }
    fn is_unit(&self, o: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn enter_pattern(&mut self, agent: Obj, state: f64) { panic!("unexpected world call in the aim twin") }
    fn exit_pattern(&mut self, agent: Obj, state: f64) { panic!("unexpected world call in the aim twin") }
    fn tick_pattern(&mut self, agent: Obj, state: f64, dt: f64) { panic!("unexpected world call in the aim twin") }
    fn sidestep(&mut self, agent: Obj) { panic!("unexpected world call in the aim twin") }
    fn cover_hints(&mut self, center: &Vector3, half: f64) -> Vec<HintView> { panic!("unexpected world call in the aim twin") }
    fn has_jet_hints(&self) -> bool { panic!("unexpected world call in the aim twin") }
    fn jet_hints_excluded(&mut self, controller: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn jet_hints_find(&mut self, controller: Obj) -> Option<Obj> { panic!("unexpected world call in the aim twin") }
    fn jet_hints_enter(&mut self, controller: Obj, hint: Option<Obj>) { panic!("unexpected world call in the aim twin") }
    fn jet_hints_exit(&mut self, controller: Obj, hint: Option<Obj>) { panic!("unexpected world call in the aim twin") }
    fn jet_hints_tick(&mut self, controller: Obj, hint: Option<Obj>, dt: f64) -> bool { panic!("unexpected world call in the aim twin") }
    fn nav_point_clear(&mut self, point: &Vector3, unit: Obj) -> Option<bool> { panic!("unexpected world call in the aim twin") }
    fn nav_segment_blocked(&mut self, from: &Vector3, to: &Vector3, unit: Obj) -> Option<bool> { panic!("unexpected world call in the aim twin") }
    fn nav_direct_clear(&mut self, unit: Obj, point: &Vector3) -> Option<bool> { panic!("unexpected world call in the aim twin") }
    fn nav_in_danger_region(&mut self, unit: Obj) -> Option<bool> { panic!("unexpected world call in the aim twin") }
    fn agent_sync(&mut self, agent: &bfe_sim::ai::combat::agent::CombatAgent) { panic!("unexpected world call in the aim twin") }
    fn agent_resync(&mut self, agent: &mut bfe_sim::ai::combat::agent::CombatAgent) { panic!("unexpected world call in the aim twin") }
    fn interrupts_tick(&mut self, agent: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn agent_board_policy(&mut self, agent: Obj, candidate: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn has_helper(&self, agent: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn helper_arrived(&mut self, agent: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn helper_flying(&mut self, agent: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn helper_has_follow(&self, agent: Obj) -> bool { panic!("unexpected world call in the aim twin") }
    fn helper_goto(&mut self, agent: Obj, point: &Vector3, mode: Option<GotoMode>) { panic!("unexpected world call in the aim twin") }
    fn helper_stop(&mut self, agent: Obj) { panic!("unexpected world call in the aim twin") }
    fn helper_tick(&mut self, agent: Obj, dt: f64) { panic!("unexpected world call in the aim twin") }
    fn helper_follow(&mut self, agent: Obj, target: Obj) { panic!("unexpected world call in the aim twin") }
    fn helper_terminate(&mut self, agent: Obj) { panic!("unexpected world call in the aim twin") }
    fn request_jet(&mut self, controller: Obj, vector: &Vector3) { panic!("unexpected world call in the aim twin") }
    fn emit_vo(&mut self, unit: Obj, event: &str, property: &str) { panic!("unexpected world call in the aim twin") }
}

#[test]
fn attributed_events_and_crew_alerts_match_the_source() {
    let Some(data)=fixture() else { return };
    let optional = |v:&Value| if matches!(v,Value::Null) {None} else {Some(number(v) as Obj)};
    let lease = |v:&Value| if matches!(v,Value::Null) {None} else {Some(LeaseView {
        obj:999,human:boolean(field(v,"human")),released:boolean(field(v,"released")),live_false:boolean(field(v,"liveFalse")),
        controllable_controller:optional(field(v,"controller")),unit_controller:optional(field(v,"unitController")),
    })};
    let event_ref = |v:&Value| if matches!(v,Value::Null) {EventRef::None} else if let Some(d)=v.get("direct") {EventRef::Unit(number(d) as Obj)}
        else {EventRef::Ref(Ref {obj:n(v,"unit") as Obj,generation:n(v,"generation")})};
    for run in array(field(&data,"ingest")) {
        let units=vec![
            UnitView {obj:1,generation:3.0,alive:boolean(&array(field(run,"alive"))[0]),team:1.0,character:Some(201),controller:Some(105),position:Vector3::new(3.0,4.0,-5.0),..UnitView::default()},
            UnitView {obj:2,generation:5.0,alive:boolean(&array(field(run,"alive"))[1]),team:2.0,human:boolean(field(run,"human")),controller:Some(101),seat:lease(field(run,"seat")),
                vehicle:(!matches!(field(run,"leases"),Value::Null)).then_some(999),
                body_leases:field(run,"leases").as_array().map(|list|list.iter().map(lease).collect()),..UnitView::default()},
        ];
        let mut ctls:Vec<_>=(101..106).map(|obj|Ctl {obj,shot:ShotAlert {pending:false,source:None,known:boolean(field(run,"known")),point:Vector3::new(7.0,-0.0,9.0)},..Ctl::default()}).collect();
        ctls[4].perception=Some(perception::Perception::default());ctls[4].target=Some(Ref {obj:2,generation:5.0});
        let mut w=AimWorld {random:ClassicRandom::new(1.0),calls:0,unit:UnitView::default(),units,ctls,fallback_team:n(run,"fallbackTeam")};
        let events:Vec<_>=array(field(run,"events")).iter().map(|e|if field(e,"kind").as_str()==Some("shot") {
            Event::Shot {source:event_ref(field(e,"source")),target:event_ref(field(e,"target"))}
        } else {Event::Damage {target:event_ref(field(e,"target")),attacker:event_ref(field(e,"attacker")),attacker_character:optional(field(e,"attackerCharacter")),
            amount:Some(n(e,"amount")),generic:Some(n(e,"generic")),typed:Some(n(e,"typed")),known:boolean(field(e,"known")),
            point:(!matches!(field(e,"point"),Value::Null)).then(||vec(field(e,"point")))}}).collect();
        CombatAi::default().ingest(&mut w,&events);
        for (ctl,want) in w.ctls.iter().zip(array(field(run,"after"))) {
            assert_eq!(ctl.shot.pending,boolean(field(want,"pending")));assert_eq!(ctl.shot.known,boolean(field(want,"known")));
            assert_eq!(ctl.shot.source,optional(field(want,"source")));eqv(ctl.shot.point,vec(field(want,"point")));
        }
    }
}
