//! Human choices retain source ordering and the gameplay stream's exact draws.
use bfe_sim::battle::{human_posts::{Choices,Key}, mobile::Pad, world::PostsWorld};
use bfe_sim::gen::*;
use bfe_sim::store::{Ref,StrId};
use three::Vector3;
#[derive(Default)] struct World;
impl PostsWorld for World {
    fn capture_team(&mut self,t:f64)->f64 {t}
    fn building_alive(&mut self,_:Ref<Building>)->bool {true}
    fn pad(&mut self,_:&Store,_:f64)->Pad {panic!("no command hull")}
    fn vehicle_position(&mut self,s:&Store,r:Ref<Vehicle>)->[f64;3] {s.vehicle.get(r).unwrap().position}
    fn class_spawn_world(&mut self,_:&Store,_:Ref<MobilePost>)->Vec<[f64;4]> {panic!("no mobile class path")}
}
fn post(s:&mut Store,name:u32,team:f64,x:f64,points:Option<Vec<[f64;4]>>)->Ref<Post> {
    let r=s.post.insert(Post{name:StrId(name),active:true,team,position:[x,0.0,0.0],
        ai:Some(PostAi{spawn_path:points.map(|points|SpawnPath{present:true,points}),..PostAi::default()}),..Post::default()});
    s.command_posts.posts.push(r);r
}
fn relation(a:f64,b:f64)->f64 {if a==b {1.0} else if a==0.0 || b==0.0 {0.0} else {-1.0}}
#[test]
fn a_named_post_is_retained_and_an_invalid_one_uses_ordered_float32_distance_ties() {
    let mut s=Store::default();
    let a=post(&mut s,1,1.0,-1.0,Some(vec![[1.0,2.0,3.0,4.0]]));
    post(&mut s,2,1.0,1.0,Some(vec![[9.0;4]]));
    post(&mut s,3,2.0,0.0,None);
    let mut choices=Choices::new(3,0,0);choices.begin(&mut s,&mut World,|_,_|None);
    assert_eq!(choices.choose(1.0,Some(StrId(1)),relation,||panic!("retained post drew")),Some(0));
    assert_eq!(choices.choose(1.0,Some(StrId(3)),relation,||panic!("nearest post drew")),Some(0));
    assert_eq!(choices.choose(1.0,None,relation,||panic!("front line drew")),Some(0));
    let mut draws=0;
    assert_eq!(choices.point(&s,0,1.0,&mut World,relation,||{draws+=1;0.7}),Some([1.0,2.0,3.0,4.0]));
    assert_eq!(draws,1);assert_eq!(choices.rows[0].key,Key::Post(SpawnPost::Post(a)));
}
#[test]
fn no_front_line_draws_even_for_one_eligible_post_and_raw_fallback_keeps_world_path() {
    let mut s=Store::default();
    let a=post(&mut s,1,1.0,0.0,Some(vec![[1.0;4]]));
    let enemy=post(&mut s,2,2.0,10.0,None);
    s.post.get_mut(enemy).unwrap().active=false;
    let mut choices=Choices::new(2,0,0);choices.raw_post(a,Some(vec![[7.0;4]]));choices.raw_post(enemy,Some(vec![[8.0;4]]));
    choices.begin(&mut s,&mut World,|_,_|None);
    let mut draws=0;assert_eq!(choices.choose(1.0,None,relation,||{draws+=1;0.0}),Some(0));assert_eq!(draws,1);
    s.post.get_mut(a).unwrap().team=2.0; choices.begin(&mut s,&mut World,|_,_|None);
    assert_eq!(choices.choose(1.0,None,relation,||panic!("no eligible post drew")),None);
    let fallback=choices.fallback(1.0).unwrap();assert_eq!(fallback,0);
    assert_eq!(choices.point(&s,fallback,1.0,&mut World,relation,||0.0),Some([7.0;4]));
}
#[path="support/allocations.rs"] mod allocation;
#[global_allocator] static ALLOCATOR:allocation::Counted=allocation::Counted;
#[test]
fn landed_transport_uses_its_current_quaternion_floor_and_two_uniform_draws_without_allocations() {
    let mut s=Store::default();
    let v=s.vehicle.insert(Vehicle{alive:true,flight_class:Some(FlightClass::Transport),flight_state:Some(FlightState::Landed),
        team:Some(1.0),bounds_max:[3.0,1.0,1.0],position:[10.0,0.0,20.0],quaternion:[0.0,0.0,0.0,1.0],yaw:0.25,..Vehicle::default()});
    s.vehicles.vehicles.push(v);let mut choices=Choices::new(0,1,0);choices.hull(v,StrId(10));
    let mut draws=0;
    allocation::start();
    for _ in 0..1200 {
        choices.begin(&mut s,&mut World,|at,_|{assert_eq!(at,Vector3::new(15.0,1.0,20.0));Some((Vector3::new(15.0,-2.0,20.0),Vector3::new(0.0,1.0,0.0)))});
        let i=choices.choose(1.0,None,relation,||{draws+=1;0.0}).unwrap();
        assert_eq!(choices.rows[i].key,Key::Transport(v));
        assert_eq!(choices.point(&s,i,1.0,&mut World,relation,||{draws+=1;0.0}),Some([15.0,-1.9,20.0,0.25]));
    }
    assert_eq!(allocation::stop(),0);assert_eq!(draws,2400);
    s.vehicle.get_mut(v).unwrap().flight_state=Some(FlightState::Flying);
    choices.begin(&mut s,&mut World,|_,_|panic!("airborne transport cast"));assert!(choices.rows.is_empty());
    s.vehicle.get_mut(v).unwrap().flight_state=Some(FlightState::Landed);
    choices.begin(&mut s,&mut World,|_,_|Some((Vector3::default(),Vector3::new(0.0,0.79,0.0))));assert!(choices.rows.is_empty());
}

#[test]
fn automatic_posts_keep_raw_presence_ownership_order_and_wrap_only_at_realization() {
    let mut s=Store::default();
    let foreign=post(&mut s,1,2.0,0.0,None);
    let absent=post(&mut s,2,1.0,1.0,Some(vec![[2.0;4]]));
    let own=post(&mut s,3,1.0,2.0,Some(vec![[3.0;4]]));
    let mut choices=Choices::new(3,0,1);
    choices.raw_post(foreign,Some(vec![[11.0;4]]));
    choices.raw_post(absent,None);
    choices.raw_post(own,Some(vec![[71.0;4]]));
    allocation::start();
    for _ in 0..1200 {
        assert_eq!(choices.auto_posts(&mut s,1.0,&mut World),1);
        assert_eq!(choices.auto_post(0.0),Some(SpawnPost::Post(own)));
        assert_eq!(choices.auto_post(-1.0),None);
        assert_eq!(choices.auto_post(1.0),None);
        let mut index=-1.0;let mut draws=0;
        assert_eq!(choices.auto_point(&mut index,&[],||{draws+=1;0.5}),Some((Some(StrId(3)),[71.0;4])));
        assert_eq!(draws,1);assert_eq!(index,0.0);
    }
    assert_eq!(allocation::stop(),0);
    s.post.get_mut(own).unwrap().active=false;
    assert_eq!(choices.auto_posts(&mut s,1.0,&mut World),1);
    assert_eq!(choices.auto_post(0.0),Some(SpawnPost::Post(foreign)));
}
#[test]
fn empty_authored_auto_path_stays_present_and_consumes_its_uniform_before_rejection() {
    let mut s=Store::default();let p=post(&mut s,1,1.0,0.0,None);
    let mut choices=Choices::new(1,0,0);choices.raw_post(p,Some(vec![]));
    assert_eq!(choices.auto_posts(&mut s,1.0,&mut World),1);
    let mut draws=0;let mut index=0.0;
    assert_eq!(choices.auto_point(&mut index,&[[91.0;4]],||{draws+=1;0.5}),None);
    assert_eq!(draws,1);
    for invalid in [f64::NAN,f64::INFINITY,0.5] {
        let mut index=invalid;assert_eq!(choices.auto_point(&mut index,&[],||0.0),None);
    }
    assert_eq!(choices.point(&s,usize::MAX,1.0,&mut World,relation,||0.0),None);
}
