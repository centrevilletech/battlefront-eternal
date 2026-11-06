//! Whole-tail fixtures use canonical rows for every observation. Services
//! represent synchronous owner actions, rather than supplying query snapshots.
use bfe_sim::battle::{battlefield::Battlefield, core::Core, owned_world::{Owner, Services},
    observation::{Mount, Mounts}, registry::Targets, rules, stats::Text, human::Command, mobile::MobileEvent};
use bfe_sim::gen::*;
use bfe_sim::store::{Nullable, Ref, StrId, Strings};
use three::Vector3;
struct Owners { core:Core, notices:Vec<&'static str>, record_notices:bool }
impl Mounts for Owners {
    fn mount(&mut self, s:&Store, r:Ref<SeatLease>) -> Option<Mount> {
        let r=s.seat_lease.get(r)?.vehicle?; let v=s.vehicle.get(r)?;
        Some(Mount{position:v.position,..Mount::default()})
    }
}
impl Targets for Owners {
    fn member(&mut self,_:&Store,_:Member)->bfe_sim::battle::world::Member { panic!("unexpected non-soldier") }
}
impl Owners { fn notice(&mut self,name:&'static str) { if self.record_notices { self.notices.push(name); } } }
impl Services for Owners {
    fn changed_battle(&mut self,_:&Store) {self.core.mark_battle();}
    fn building_alive(&mut self,_:Ref<Building>)->bool { panic!("no building") }
    fn building_attacker(&mut self,_:&Store,_:Ref<Building>)->Option<Ref<Character>> { None }
    fn vehicle_attacker(&mut self,_:&Store,_:Ref<Vehicle>)->Option<Ref<Character>> { None }
    fn piloted(&mut self,_:&Store,_:Ref<Unit>)->Option<[f64;3]> { None }
    fn relation(&mut self,a:f64,b:f64)->f64 { if a==b {1.0} else {-1.0} }
    fn prepare_record(&mut self,s:&mut Store,r:Ref<Character>) { self.core.records.ensure(s,r); self.core.mark(r); }
    fn point(&mut self,s:&mut Store,r:Option<Ref<Character>>) { rules::point(s,r); if let Some(r)=r { self.core.mark(r); } }
    fn kill(&mut self,s:&mut Store,r:Ref<Unit>) {
        let u=s.unit.get_mut(r).unwrap(); u.alive=false; u.combat.dead=true; u.combat.health=0.0;
        let c=u.character; if let Some(c)=c { s.character.get_mut(c).unwrap().unit=None; }
        self.core.credit(s,c,None,0.0); self.notice("kill");
    }
    fn kill_droid(&mut self,_:&mut Store,_:Ref<Unit>) { panic!("no droid") }
    fn wreck(&mut self,_:&mut Store,_:Ref<Vehicle>) { panic!("no hull") }
    fn set_hull_health(&mut self,_:&mut Store,_:Ref<Vehicle>,_:f64) { panic!("no hull") }
    fn spawn(&mut self,_:&mut Store,_:Ref<Character>,_:Option<StrId>,_:SpawnPost,_:[f64;3],_:Option<f64>)->Option<Ref<Unit>> { panic!("no pending spawn") }
    fn request_objective(&mut self,_:&mut Store,_:Ref<Character>) { panic!("no objective request") }
    fn republish(&mut self,_:&mut Store,_:Ref<Character>) { panic!("no objective publication") }
    fn cast_spawn(&mut self,_:Vector3,_:Vector3,_:f64,_:Ref<Body>)->Option<Vector3> { panic!("no command hull") }
    fn effect(&mut self,_:&mut Store,_:&str,_:[f64;3]) { self.notice("effect"); }
    fn post_event(&mut self,_:&mut Store,_:&PostEvent) { self.notice("post"); }
    fn mobile_event(&mut self,_:&mut Store,_:&MobileEvent) { self.notice("mobile"); }
    fn battle_over(&mut self,_:&mut Store,_:f64) { self.notice("over"); }
    fn human_tail(&mut self,_:&mut Store,_:f64,_:&[Command],_:&mut bfe_sim::battle::owned_world::Prepared) { self.notice("human"); }
    fn corpse_tail(&mut self,_:&mut Store) { self.notice("corpse"); }
}
fn setup() -> (Store,Owner,Owners,Strings,[Ref<Character>;2]) {
    let mut strings=Strings::default(); let name=strings.intern_str("trooper");
    let mut s=Store::default();
    s.battle=rules::new_battle([(1.0,TeamDesc{reinforcements:Some(100.0),..TeamDesc::default()}),
        (2.0,TeamDesc{reinforcements:Some(100.0),..TeamDesc::default()})].into_iter().collect(),1.0,2.0);
    let chars=[s.character.insert(Character{team:1.0,class_name:Some(name),..Character::default()}),
        s.character.insert(Character{team:1.0,class_name:Some(name),..Character::default()})];
    s.battle.characters.extend(chars);
    for (i,&c) in chars.iter().enumerate() {
        let r=s.unit.insert(Unit{class_def:DefId(i as u32),alive:true,generation:1.0,team:1.0,character:Some(c),
            combat:Combat{health:100.0,max_health:100.0,..Combat::default()},..Unit::default()});
        s.units.list.push(Member::Unit(r)); s.units.next_id+=1.0; s.character.get_mut(c).unwrap().unit=Some(r);
    }
    let p=s.post.insert(Post{active:true,neutralize_time:1.0,capture_time:1.0,
        capture:Some(Built{r#type:RegionType::Sphere,radius:10.0,..Built::default()}),..Post::default()});
    s.command_posts.posts.push(p);
    let mut owner=Owner::new(&mut s,Battlefield::default(),None,false,2,2);
    for i in 0..2 { owner.prepared.registry.class(DefId(i),i==0); owner.prepared.observations.class(F4StatsClass{def:DefId(i),label:Some(name)}); }
    let mut core=Core::new(&s,2); for &c in &chars {core.adopt(&mut s,c);} core.flush(&s);
    (s,owner,Owners{core,notices:Vec::with_capacity(16),record_notices:true},strings,chars)
}
struct Silent;
impl bfe_sim::battle::spawning::Draws for Silent {fn next(&mut self)->f64 {panic!("unexpected draw");}}
#[test]
fn canonical_tail_counts_only_capturers_but_awards_the_hero_before_local_tail() {
    let (mut s,mut owner,mut services,strings,chars)=setup();
    owner.step(&F4TailInput{dt:1.0,..F4TailInput::default()},&mut s,&strings,&[],&mut services,&mut Silent,&mut Silent);
    assert_eq!(s.post.get(s.command_posts.posts[0]).unwrap().team,1.0);
    assert_eq!(s.character.get(chars[0]).unwrap().captures,1.0);
    assert_eq!(s.character.get(chars[1]).unwrap().captures,1.0);
    assert_eq!(services.notices,["post","human","corpse"]);
    assert!(matches!(s.character.get(chars[1]).unwrap().record,Nullable::Value(_)));
    assert!(strings.nonempty(s.character.get(chars[0]).unwrap().class_name.unwrap()));
}
#[path="support/allocations.rs"] mod allocation;
#[global_allocator] static ALLOCATOR:allocation::Counted=allocation::Counted;
#[test]
fn first_and_repeated_owned_tail_reads_rules_awards_and_stats_allocate_nothing() {
    let (mut s,mut owner,mut services,strings,_)=setup(); services.record_notices=false;
    allocation::start();
    for tick in 0..1200 {
        owner.step(&F4TailInput{dt:1.0/60.0,tick:tick as f64,server:true,..F4TailInput::default()},
            &mut s,&strings,&[],&mut services,&mut Silent,&mut Silent);
        services.core.flush(&s);
    }
    assert_eq!(allocation::stop(),0);
}

#[test]
fn classic_post_choice_stays_random_while_enhanced_choice_observes_the_objective() {
    use bfe_sim::battle::world::SpawningWorld;
    let (mut s,mut owner,mut services,_,chars)=setup();
    let post=s.command_posts.posts[0];s.post.get_mut(post).unwrap().position=[7.0,0.0,9.0];
    s.character.get_mut(chars[0]).unwrap().objective=PostRef::Post(post);
    let mut world=bfe_sim::battle::owned_world::World{prepared:&mut owner.prepared,services:&mut services,local:None,classic:true};
    assert_eq!(world.spawn_goal(&s,chars[0]),None);
    world.prepared.enhanced=true;
    assert_eq!(world.spawn_goal(&s,chars[0]),Some([7.0,0.0,9.0]));
}
