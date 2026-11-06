//! Human commands run the real spawn state machine, selectors and native
//! Character queues around a preconstructed body service.
use bfe_sim::battle::{human::{self,Command,Event},human_posts::Choices,human_world::{Human,Prepared,PlayerServices},
    mobile::Pad,spawning::{self,Draws},world::{PostsWorld,SpawningWorld}};
use bfe_sim::gen::*;
use bfe_sim::store::{Nullable,Ref,StrId,Strings};
use three::Vector3;
struct Bodies {local:Ref<Unit>,character:Ref<Character>,spawns:usize,point:[f64;4]}
impl PostsWorld for Bodies {
    fn capture_team(&mut self,t:f64)->f64 {t}
    fn building_alive(&mut self,_:Ref<Building>)->bool {true}
    fn pad(&mut self,_:&Store,_:f64)->Pad {panic!("no hull")}
    fn vehicle_position(&mut self,_:&Store,_:Ref<Vehicle>)->[f64;3] {panic!("no hull")}
    fn class_spawn_world(&mut self,_:&Store,_:Ref<MobilePost>)->Vec<[f64;4]> {panic!("no hull")}
}
impl SpawningWorld for Bodies {
    fn health(&mut self,s:&Store,r:Ref<Unit>)->f64 {s.unit.get(r).unwrap().combat.health}
    fn force_kill(&mut self,_:&mut Store,_:Ref<Character>,_:Ref<Unit>) {panic!("no forced class kill")}
    fn request_objective(&mut self,_:&mut Store,_:Ref<Character>) {panic!("human needs no objective")}
    fn republish(&mut self,_:&mut Store,_:Ref<Character>) {}
    fn hero_name(&mut self,_:StrId)->Option<StrId> {None}
    fn spawn_unit(&mut self,_:&mut Store,_:Ref<Character>,_:Option<StrId>,_:SpawnPost,_:[f64;3],_:Option<f64>)->Option<Ref<Unit>> {panic!("human body uses its factory")}
}
impl PlayerServices for Bodies {
    fn floor(_:Vector3,_:Ref<Body>)->Option<(Vector3,Vector3)> {None}
    fn relation(a:f64,b:f64)->f64 {if a==b {1.0} else {-1.0}}
    fn local(&self)->Option<Ref<Unit>> {Some(self.local)}
    fn spawn_player(&mut self,s:&mut Store,_:StrId,_:Option<StrId>,point:[f64;4])->Option<Ref<Unit>> {
        let u=s.unit.get_mut(self.local).unwrap();u.alive=true;u.removed=false;u.combat.dead=false;u.combat.health=100.0;u.generation+=1.0;
        self.spawns+=1;self.point=point;Some(self.local)
    }
    fn linked(&mut self,_:&mut Store,_:Ref<Unit>,_:Option<Ref<Character>>) {}
    fn retarget(&mut self,_:&mut Store,_:f64) {}
    fn kill_player(&mut self,s:&mut Store,events:&mut Vec<Event>) {
        let u=s.unit.get_mut(self.local).unwrap();u.alive=false;u.combat.dead=true;u.combat.health=0.0;
        s.character.get_mut(self.character).unwrap().unit=None;
        s.human_spawn.phase=HumanPhase::Dying;s.human_spawn.dead_time=0.0;
        events.push(Event::PlayerDied{team:s.battle.own,class_name:s.human_spawn.class_name});
    }
    fn changed(&mut self,_:&Store,_:Option<Ref<Character>>) {}
    fn rejected(&mut self,_:StrId,_:f64) {panic!("prepared class rejected")}
}
struct Random {draws:usize}
impl Draws for Random {fn next(&mut self)->f64 {self.draws+=1;0.0}}
fn setup(classic:bool)->(Store,Human,Bodies,Strings,[StrId;2],[StrId;2]) {
    let mut strings=Strings::default();let classes=[strings.intern_str("a"),strings.intern_str("b")];
    let names=[strings.intern_str("front-a"),strings.intern_str("front-b")];
    let mut s=Store::default();s.battle.own=1.0;s.battle.foe=2.0;s.battle.team_ids=vec![1.0,2.0];
    let character=s.character.insert(Character{human:true,team:1.0,class_index:Nullable::Value(0.0),..Character::default()});
    s.battle.characters.push(character);
    let local=s.unit.insert(Unit{character:Some(character),team:1.0,combat:Combat{dead:true,max_health:100.0,..Combat::default()},..Unit::default()});
    for (i,&name) in names.iter().enumerate() {
        let r=s.post.insert(Post{name,team:i as f64+1.0,active:true,ai:Some(PostAi{spawn_path:Some(SpawnPath{present:true,
            points:vec![[10.0*(i as f64+1.0),0.0,20.0,90.0*(i as f64+1.0)]]}),..PostAi::default()}),..Post::default()});
        s.command_posts.posts.push(r);
    }
    s.human_spawn=human::new(classes[0],None,None);
    if classic {
        s.spawning.teams=spawning::make_teams(32.0);
        for (i,&name) in classes.iter().enumerate() {s.spawning.teams[i+1].classes.push(SpawnClass{name,target:1.0,hero:false});}
        s.spawning.teams[1].characters.push(character);
        s.character.get_mut(character).unwrap().spawn=Some(Spawn{post:PostRef::Null,..Spawn::default()});
        spawning::prepare_runtime(&mut s,2,2);
    }
    let h=Human::new(Prepared{character:Some(character),classic,rosters:vec![vec![],vec![classes[0]],vec![classes[1]]],classes:classes.to_vec()},Choices::new(2,0,1),4,2,1);
    (s,h,Bodies{local,character,spawns:0,point:[0.0;4]},strings,classes,names)
}
#[test]
fn nonclassic_team_spawn_suicide_and_respawn_keep_native_links_and_notice_order() {
    let (mut s,mut human,mut bodies,strings,classes,names)=setup(false);let mut ai=Random{draws:0};let mut game=Random{draws:0};
    human.step(&mut s,0.0,&[Command::Spawn(HumanPick{class_name:classes[0],post:Some(names[0])})],&mut bodies,&strings,&mut ai,&mut game);
    assert_eq!(s.human_spawn.phase,HumanPhase::Alive);assert_eq!(bodies.point,[10.0,0.0,20.0,90.0]);assert_eq!(game.draws,1);
    assert_eq!(s.character.get(bodies.character).unwrap().unit,Some(bodies.local));human.published();
    human.step(&mut s,0.0,&[Command::Suicide],&mut bodies,&strings,&mut ai,&mut game);
    assert_eq!(s.human_spawn.phase,HumanPhase::Dying);assert!(matches!(human.events(),[Event::PlayerDied{..}]));human.published();
    human.step(&mut s,4.0,&[],&mut bodies,&strings,&mut ai,&mut game);assert_eq!(s.human_spawn.phase,HumanPhase::Awaiting);human.published();
    human.step(&mut s,0.0,&[Command::Team(2.0),Command::Spawn(HumanPick{class_name:classes[1],post:Some(names[1])})],&mut bodies,&strings,&mut ai,&mut game);
    assert_eq!(s.battle.own,2.0);assert_eq!(s.unit.get(bodies.local).unwrap().team,2.0);
    assert_eq!(bodies.point,[20.0,0.0,20.0,180.0]);assert_eq!(bodies.spawns,2);assert_eq!(game.draws,2);assert_eq!(ai.draws,0);
    assert!(matches!(human.events(),[Event::TeamChanged{team:2.0,..},Event::Spawned{team:2.0,..}]));
}
#[path="support/allocations.rs"] mod allocation;
#[global_allocator] static ALLOCATOR:allocation::Counted=allocation::Counted;
#[test]
fn first_classic_ready_cancel_and_team_switch_reuse_cold_queue_and_roster_storage() {
    let (mut s,mut human,mut bodies,strings,classes,names)=setup(true);let mut ai=Random{draws:0};let mut game=Random{draws:0};
    allocation::start();
    human.step(&mut s,0.0,&[Command::Spawn(HumanPick{class_name:classes[0],post:Some(names[0])})],&mut bodies,&strings,&mut ai,&mut game);
    assert!(s.character.get(bodies.character).unwrap().spawn.as_ref().unwrap().ready);
    human.published();human.step(&mut s,0.0,&[Command::Cancel,Command::Team(2.0)],&mut bodies,&strings,&mut ai,&mut game);
    assert_eq!(allocation::stop(),0);assert_eq!(bodies.spawns,0);assert_eq!(s.character.get(bodies.character).unwrap().team,2.0);
    assert!(!s.character.get(bodies.character).unwrap().spawn.as_ref().unwrap().ready);assert_eq!(ai.draws,0);assert_eq!(game.draws,0);
}

#[test]
fn repeated_native_choices_spawn_death_and_screen_transitions_allocate_nothing() {
    let (mut s,mut human,mut bodies,strings,classes,names)=setup(false);let mut ai=Random{draws:0};let mut game=Random{draws:0};
    allocation::start();
    for _ in 0..1200 {
        human.step(&mut s,0.0,&[Command::Spawn(HumanPick{class_name:classes[0],post:Some(names[0])})],&mut bodies,&strings,&mut ai,&mut game);
        human.published();human.step(&mut s,0.0,&[Command::Suicide],&mut bodies,&strings,&mut ai,&mut game);
        human.published();human.step(&mut s,4.0,&[],&mut bodies,&strings,&mut ai,&mut game);human.published();
    }
    assert_eq!(allocation::stop(),0);assert_eq!(bodies.spawns,1200);assert_eq!(game.draws,1200);
}

#[test]
fn auto_respawn_places_raw_world_path_at_the_combat_slot_and_keeps_the_url_index() {
    let (mut s,mut human,mut bodies,strings,classes,names)=setup(false);
    s.human_spawn.auto=Some(HumanAuto{class_name:classes[0],..Default::default()});
    let first=s.command_posts.posts[0];
    human.choices.raw_post(first,Some(vec![[71.0,2.0,3.0,4.0]]));
    let mut index=-1.0;let mut game=Random{draws:0};
    allocation::start();
    human.auto_respawn(&mut s,&mut index,&[[91.0;4]],&mut bodies,&strings,&mut game);
    assert_eq!(allocation::stop(),0);
    assert_eq!(index,0.0);assert_eq!(game.draws,1);assert_eq!(bodies.spawns,1);
    assert_eq!(bodies.point,[71.0,2.0,3.0,4.0]);
    assert_eq!(s.human_spawn.phase,HumanPhase::Alive);
    assert!(matches!(human.events(),[Event::Spawned{post:Some(p),..}] if *p==names[0]));
}
#[test]
fn auto_classic_ready_leaves_an_out_of_range_post_unchanged_and_does_not_place_a_body() {
    let (mut s,mut human,mut bodies,strings,classes,_)=setup(true);
    s.human_spawn.auto=Some(HumanAuto{class_name:classes[0],..Default::default()});
    let first=s.command_posts.posts[0];
    human.choices.raw_post(first,Some(vec![[71.0;4]]));
    s.character.get_mut(bodies.character).unwrap().spawn.as_mut().unwrap().post=PostRef::Post(first);
    let mut index=9.0;let mut game=Random{draws:0};
    human.auto_respawn(&mut s,&mut index,&[],&mut bodies,&strings,&mut game);
    let spawn=s.character.get(bodies.character).unwrap().spawn.as_ref().unwrap();
    assert!(spawn.ready);assert_eq!(spawn.post,PostRef::Post(first));
    assert_eq!(index,9.0);assert_eq!(game.draws,0);assert_eq!(bodies.spawns,0);assert!(human.events().is_empty());
    // The callback must not confirm again while this queue is already ready.
    s.character.get_mut(bodies.character).unwrap().class_name=Some(classes[1]);
    human.auto_respawn(&mut s,&mut index,&[],&mut bodies,&strings,&mut game);
    assert_eq!(s.character.get(bodies.character).unwrap().class_name,Some(classes[1]));
}
#[test]
fn auto_without_placed_paths_uses_the_prepared_world_fallback_and_no_policy_is_a_noop() {
    let (mut s,mut human,mut bodies,strings,classes,_)=setup(false);
    let mut index=12.0;let mut game=Random{draws:0};
    human.auto_respawn(&mut s,&mut index,&[],&mut bodies,&strings,&mut game);
    assert_eq!(bodies.spawns,0);assert_eq!(game.draws,0);
    s.human_spawn.auto=Some(HumanAuto{class_name:classes[0],..Default::default()});
    human.auto_respawn(&mut s,&mut index,&[[91.0;4]],&mut bodies,&strings,&mut game);
    assert_eq!(bodies.point,[91.0;4]);assert_eq!(index,12.0);assert_eq!(game.draws,1);
    assert!(matches!(human.events(),[Event::Spawned{post:None,..}]));
}
