use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct AllocationCounter;
thread_local! {
    pub(super) static COUNTING: Cell<bool> = const { Cell::new(false) };
    pub(super) static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for AllocationCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|active| if active.get() {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout); }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|active| if active.get() {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
        });
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: AllocationCounter = AllocationCounter;

use bfe_sim::gen::{AiLifeHandle, AiPhaseCamera};
use bfe_sim::ai::control::world::CameraView;
use bfe_sim::ai::control::scheduler::camera_accepts;
use bfe_sim::ai::nav::math::Vec3;
use bfe_sim::store::Slab;

#[test]
fn handles_reject_respawn_retirement_reuse_and_other_kinds() {
    let mut units = Slab::with_capacity(1);
    let first = units.insert(());
    let handle = AiLifeHandle { kind: 1, slot: first.slot, generation: first.gen, life: 3. };
    assert!(handle.matches(1, first, 3.));
    assert!(!handle.matches(1, first, 4.));
    assert!(!handle.matches(2, first, 3.));
    units.remove(first).unwrap();
    let second = units.insert(());
    assert_eq!(first.slot, second.slot);
    assert!(!handle.matches(1, second, 3.));
}

#[test]
fn generated_camera_retains_zoom_and_frustum_geometry() {
    let camera = AiPhaseCamera {
        elements: [1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0., 0.,0.,0.,1.],
        fov: 90., aspect: 1., near: 0.1, far: 2000., zoom: 2.,
    };
    let view = CameraView::from(&camera);
    assert!(camera_accepts(&view, Vec3::new(0., 0., -86.)));
    assert!(!camera_accepts(&view, Vec3::new(0., 0., -86.01)));
    assert!(!camera_accepts(&view, Vec3::new(6., 0., -10.)));
    assert!(!camera_accepts(&view, Vec3::new(0., 0., 1.)));
}

#[test]
fn shared_draws_preserve_interleaving_and_empty_integer_ranges() {
    use bfe_sim::ai::control::world::Draws;
    use bfe_sim::ai::phase::RandomState;
    use bfe_sim::ai::random::ClassicRandom;
    let mut state = RandomState::new(17);
    let mut a = ClassicRandom::from_state(17);
    let mut b = ClassicRandom::from_state(17 ^ 0x9e37_79b9);
    let mut c = ClassicRandom::from_state(17 ^ 0x51ed_270b);
    assert_eq!(state.a_uniform().to_bits(), a.uniform().to_bits());
    assert_eq!(state.b_range(-2., 3.).to_bits(), b.range(-2., 3.).to_bits());
    assert_eq!(state.a_int(4., 4.), 4.);
    assert_eq!(state.aim_uniform().to_bits(), c.uniform().to_bits());
    assert_eq!(state.a_range(-1., 2.).to_bits(), a.range(-1., 2.).to_bits());
    assert_eq!(state.a_int(0., 9.).to_bits(), a.int(0., 9.).to_bits());
    assert_eq!(state.seeds(), [a.seed(), b.seed(), c.seed()]);
    assert_eq!(state.draw_counts(), [3, 1, 1]);
}

#[test]
fn classic_lod_keeps_offline_and_network_camera_rules() {
    use bfe_sim::ai::control::scheduler::visible_or_near;
    use bfe_sim::ai::control::world::AiUnit;
    let mut unit = AiUnit::default();
    unit.position = Vec3::new(100., 0., 1.);
    let first = CameraView::from(&AiPhaseCamera {
        elements: [1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0., 0.,0.,0.,1.],
        fov: 90., aspect: 1., near: 0.1, far: 2000., zoom: 1.,
    });
    let mut second = first.clone();
    second.elements[12] = 100.;
    let cameras = [first, second];
    // Behind both views, so only the distance rule can accept it.
    assert!(!visible_or_near(&unit, &cameras, false, false));
    assert!(visible_or_near(&unit, &cameras, true, false));
    unit.velocity = Some(Vec3::new(10., 0., 0.));
    assert!(visible_or_near(&unit, &cameras, false, false));
    unit.velocity = Some(Vec3::new(9.99, 0., 0.));
    assert!(!visible_or_near(&unit, &cameras, false, false));
    assert!(visible_or_near(&unit, &[], false, false));
    assert!(visible_or_near(&unit, &cameras, false, true));
}

#[test]
fn health_type_names_keep_case_insensitive_content_matching() {
    use bfe_sim::ai::control::world::AiUnit;
    use bfe_sim::ai::control::classes::{health_type, HEALTH_TYPES};
    let mut unit = AiUnit::default();
    for (i, name) in HEALTH_TYPES.iter().enumerate() {
        unit.health_type_name = Some(name.to_uppercase().into());
        assert_eq!(health_type(&unit), i as f64);
        unit.health_type_name = Some(name.to_lowercase().into());
        assert_eq!(health_type(&unit), i as f64);
    }
    unit.health_type_name = Some("Buİlding".into());
    assert_eq!(health_type(&unit), 0.);
}

#[test]
fn objective_selection_reuses_rows_and_preserves_fallback_draws() {
    use bfe_sim::ai::control::goals::{select_objective_from, ObjectiveScratch};
    use bfe_sim::ai::control::world::{PostAi, PostSnap};
    use bfe_sim::ai::phase::RandomState;
    let make = |index, team| PostSnap {
        index, name: None, team, active: true, position: Vec3::new(index as f64, 0., 0.),
        building_alive: Some(true), neutralize: 0., radius: 10., speed: 0.,
        ai: PostAi { order: None, atk: [1.; 6], def: [1.; 6], bans: 0., value_bleed: 0.,
            spawn_present: Some(true), ally_present: None, ally_count: 0., class_spawn_points: 0.,
            capture: None, radius: 10., yaw: 0. },
        region: None, desc_capture: None, desc_spawn_path: false, desc_spawn_points: 0., desc_ally_path: false,
    };
    let mut posts = [make(0, 1.), make(1, 2.)];
    let relation = |a, b| if a == b { 1. } else { -1. };
    let mut scratch = ObjectiveScratch::default();
    scratch.reserve(posts.len());
    let mut draws = RandomState::new(1);
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|active| active.set(true));
    for _ in 0..64 {
        assert_eq!(select_objective_from(2, &|i| &posts[i], &mut scratch,
            1., 3., &mut draws, &relation, false, 0., false, None), Some(1));
    }
    COUNTING.with(|active| active.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0, "reserved objective selection must not allocate");
    assert_eq!(draws.draw_counts(), [64, 0, 0]);
    posts[1].active = false;
    // One friendly spawn gives no candidate and no random fallback.
    assert_eq!(select_objective_from(2, &|i| &posts[i], &mut scratch,
        1., 3., &mut draws, &relation, false, 0., false, None), None);
    assert_eq!(draws.draw_counts(), [64, 0, 0]);
}

#[test]
fn retained_hint_queries_keep_cap_and_reservation_quirks() {
    use bfe_sim::ai::nav::hints::{build_hints, HintRecord, HintSet};
    let record = HintRecord { kind: Some("snipe".into()), name: None,
        xfrm: [1.,0.,0., 0.,1.,0., 0.,0.,1., 0.,0.,0.], properties: vec![] };
    let mut hints = HintSet::new(build_hints(&[record.clone(), record], None, 350.));
    let mut found = Vec::with_capacity(hints.list.len());
    hints.reserve(0, Some(7), 10.);
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|active| active.set(true));
    hints.query_into(1, Vec3::default(), 10., 0., None, 10., &mut found);
    // The source checks cap after appending, even when cap is zero.
    assert_eq!(found.as_slice(), &[0]);
    hints.query_into(1, Vec3::default(), 10., 10., Some(7), 10., &mut found);
    // Reservation presence gates availability; matching owners get no exemption.
    assert_eq!(found.as_slice(), &[1]);
    hints.query_into(1, Vec3::default(), 10., 10., Some(7), 14., &mut found);
    assert_eq!(found.as_slice(), &[0, 1]);
    hints.query_into(0, Vec3::default(), 10., 10., None, 14., &mut found);
    assert!(found.is_empty());
    COUNTING.with(|active| active.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn missing_b_service_shares_a_at_each_draw_point() {
    use bfe_sim::ai::control::world::Draws;
    use bfe_sim::ai::phase::RandomState;
    use bfe_sim::ai::random::ClassicRandom;
    let mut state = RandomState::with_random_b(91, false);
    let mut shared = ClassicRandom::from_state(91);
    assert_eq!(state.a_uniform().to_bits(), shared.uniform().to_bits());
    assert_eq!(state.b_uniform().to_bits(), shared.uniform().to_bits());
    assert_eq!(state.b_range(-10., 10.).to_bits(), shared.range(-10., 10.).to_bits());
    assert_eq!(state.a_int(0., 3.).to_bits(), shared.int(0., 3.).to_bits());
    assert_eq!(state.seeds()[0], shared.seed());
    assert_eq!(state.seeds()[1], 91 ^ 0x9e37_79b9);
    assert_eq!(state.draw_counts(), [4, 0, 0]);
}

#[test]
fn canonical_reads_reject_respawns_and_slot_reuse_without_mirrors() {
    use bfe_sim::gen::{Store, Unit, Vehicle, KIND_UNIT, KIND_VEHICLE};
    let mut store = crate::ai::owned::tests::empty_world();
    let unit = store.unit.insert(Unit { generation: 7., ..Unit::default() });
    let hull = store.vehicle.insert(Vehicle { target_generation: 9., ..Vehicle::default() });
    let key = AiLifeHandle { kind: KIND_UNIT.into(), slot: unit.slot, generation: unit.gen, life: 7. };
    let hull_key = AiLifeHandle { kind: KIND_VEHICLE.into(), slot: hull.slot, generation: hull.gen, life: 9. };
    assert!(std::ptr::eq(key.unit(&store).unwrap(), store.unit.get(unit).unwrap()));
    assert!(std::ptr::eq(hull_key.vehicle(&store).unwrap(), store.vehicle.get(hull).unwrap()));
    assert!(key.vehicle(&store).is_none());
    store.unit.get_mut(unit).unwrap().generation = 8.;
    store.vehicle.get_mut(hull).unwrap().target_generation = 10.;
    assert!(key.unit(&store).is_none());
    assert!(hull_key.vehicle(&store).is_none());
    store.unit.remove(unit).unwrap();
    let replacement = store.unit.insert(Unit { generation: 7., ..Unit::default() });
    assert_eq!(replacement.slot, unit.slot);
    assert!(key.unit(&store).is_none());
}

struct NavigationEnvironment { now: f64 }
impl bfe_sim::ai::owned::navigation::Environment for NavigationEnvironment {
    fn time(&self) -> f64 { self.now }
    fn seat_frame(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::vehicles::f2_seats::HullLife,_:usize)->Option<(Vec3,Vec3)> {Some((Vec3::default(),Vec3::new(0.,0.,1.)))}
    fn remembered(&mut self, _: bfe_sim::ai::owned::navigation::Actor) -> Option<Vec3> { None }
    fn clear_above(&mut self,_:&mut crate::world::owner::World,_:Vec3,_:&AiLifeHandle) -> bool { true }
    fn follow_far(&mut self) -> f64 { 3. }
}
fn owned_navigation(graph: bool) -> bfe_sim::ai::owned::navigation::Navigation {
    use bfe_sim::ai::nav::{Nav, PlannerOptions, graph::{NavGraph, GraphRecord, HubRecord, ArcRecord}};
    let mut nav = Nav::empty(PlannerOptions::default());
    if graph {
        let hubs = [0., 20.].into_iter().map(|x| HubRecord { name: None, position: [x,0.,0.], radius: 5., arcs: vec![0.,255.,255.,255.,255.,255.,255.,255.] }).collect();
        let arcs = vec![ArcRecord { name: None, hubs: [0.,1.], mask: 63. }];
        let mut tables = vec![0; 20];
        for from in 0..2 { for table in 0..5 { tables[(from*5+table)*2+(1-from)] = 31 << 3; } }
        nav.graph = Some(NavGraph::new(&GraphRecord { info: [2.,1.], hubs, arcs, tables: Some(5.) }, tables).unwrap());
    }
    bfe_sim::ai::owned::navigation::Navigation::new(nav, 8)
}

#[test]
fn canonical_navigation_runs_two_maps_without_tick_allocations_or_body_imports() {
    let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    use bfe_sim::ai::{owned::navigation::Command, nav::{goto::GotoMode, units::ClassDef}, phase::RandomState};
    use bfe_sim::gen::{Store, Unit, Soldier, UnitBody, KIND_UNIT};
    for graph in [false,true] {
        let mut store = crate::ai::owned::tests::empty_world();
        let reference = store.unit.insert(Unit { alive: true, generation: 1.,
            soldier: UnitBody::Soldier(Soldier { max_speed: 9., max_strafe: 7., max_turn: 2., max_pitch: 0.5, ..Soldier::default() }), ..Unit::default() });
        let key = AiLifeHandle { kind: KIND_UNIT.into(), slot: reference.slot, generation: reference.gen, life: 1. };
        let mut navigation = owned_navigation(graph);
        let class = navigation.class(ClassDef::default());
        let actor = navigation.register(&store,key,class).unwrap();
        let mut random = RandomState::new(19);
        let mut env = NavigationEnvironment { now: 0. };
        navigation.command(&mut store,&mut random,&mut env,actor,Command::Goto(Vec3::new(20.,0.,0.), if graph { GotoMode::Route } else { GotoMode::Direct })).unwrap();
        for _ in 0..8 {
            navigation.nav.service(1.);
            navigation.command(&mut store,&mut random,&mut env,actor,Command::Tick(1./60.)).unwrap();
            navigation.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).unwrap();
            navigation.clear_dirty();
        }
        store.unit.get_mut(reference).unwrap().intent.buttons = [2.,3.];
        ALLOCATIONS.with(|count| count.set(0)); COUNTING.with(|active| active.set(true));
        for _ in 0..120 {
            navigation.command(&mut store,&mut random,&mut env,actor,Command::Tick(1./60.)).unwrap();
            navigation.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).unwrap();
            navigation.clear_dirty(); env.now += 1./60.;
        }
        COUNTING.with(|active| active.set(false));
        assert_eq!(ALLOCATIONS.with(Cell::get),0,"map graph={graph}");
        let unit = store.unit.get(reference).unwrap();
        assert_eq!(unit.intent.buttons,[2.,3.]);
        assert!(unit.intent.right != 0. || unit.intent.forward != 0.);
        // New authoritative movement is observed on the next input call.
        let UnitBody::Soldier(body) = &mut store.unit.get_mut(reference).unwrap().soldier else { unreachable!() };
        body.position = [20.,0.,0.];
        navigation.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).unwrap();
        assert_eq!(store.unit.get(reference).unwrap().intent.forward,0.);
    }
}

#[test]
fn navigation_follow_handles_reject_recycled_actors_and_previous_lives() {
    use bfe_sim::ai::nav::units::ClassDef;
    use bfe_sim::gen::{Store,Unit,KIND_UNIT};
    let mut store=crate::ai::owned::tests::empty_world(); let reference=store.unit.insert(Unit { alive:true,generation:1.,..Unit::default() });
    let key=|| AiLifeHandle { kind:KIND_UNIT.into(),slot:reference.slot,generation:reference.gen,life:1. };
    let mut navigation=owned_navigation(false); let class=navigation.class(ClassDef::default());
    let actor=navigation.register(&store,key(),class).unwrap(); let old=navigation.follow_ref(actor).unwrap();
    assert!(std::ptr::eq(navigation.resolve(&store,old).unwrap(),store.unit.get(reference).unwrap()));
    store.unit.get_mut(reference).unwrap().generation=2.;
    assert!(navigation.resolve(&store,old).is_none());
    let mut next=key();next.life=2.;assert!(navigation.refresh(&store,actor,next,class));
    assert!(navigation.resolve(&store,old).is_none());
    let current=navigation.follow_ref(actor).unwrap();assert!(navigation.resolve(&store,current).is_some());
    assert!(navigation.retire(actor));
    let mut next=key();next.life=2.;let replacement=navigation.register(&store,next,class).unwrap();
    assert_ne!(replacement,actor);assert!(navigation.resolve(&store,current).is_none());
}

#[test]
fn owned_lod_batches_camera_facts_and_observes_current_canonical_motion() {
    use bfe_sim::{gen::{Store,Unit,UnitBody,Soldier,KIND_UNIT},ai::owned::lod::Lod};
    let mut store=crate::ai::owned::tests::empty_world();
    let reference=store.unit.insert(Unit { generation:3.,soldier:UnitBody::Soldier(Soldier { position:[100.,0.,1.],..Soldier::default() }),..Unit::default() });
    let key=AiLifeHandle { kind:KIND_UNIT.into(),slot:reference.slot,generation:reference.gen,life:3. };
    let first=AiPhaseCamera { elements:[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.], fov:90.,aspect:1.,near:0.1,far:2000.,zoom:1. };
    let mut second=first.clone();second.elements[12]=100.;let cameras=[first,second];
    let mut lod=Lod::new(2);
    ALLOCATIONS.with(|count|count.set(0));COUNTING.with(|active|active.set(true));
    lod.update(&cameras,false,false);assert_eq!(lod.visible(&store,&key),Some(false));
    lod.update(&cameras,true,false);assert_eq!(lod.visible(&store,&key),Some(true));
    lod.update(&cameras,false,false);
    let UnitBody::Soldier(body)=&mut store.unit.get_mut(reference).unwrap().soldier else { unreachable!() };
    body.velocity=[10.,0.,0.];assert_eq!(lod.visible(&store,&key),Some(true));
    store.unit.get_mut(reference).unwrap().generation=4.;assert_eq!(lod.visible(&store,&key),None);
    COUNTING.with(|active|active.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

struct PopulationFacts;
impl bfe_sim::ai::owned::population::Facts for PopulationFacts {
    fn building_alive(&self,_:bfe_sim::store::Ref<bfe_sim::gen::Building>)->bool {true}
    fn vehicle_mask(&self,_:bfe_sim::store::Ref<bfe_sim::gen::Vehicle>)->f64 {1.}
}
struct ZeroDraws(u32);
impl bfe_sim::ai::control::world::Draws for ZeroDraws {
    fn a_uniform(&mut self)->f64 {self.0+=1;0.}
    fn b_uniform(&mut self)->f64 {panic!("objective selection must not draw B")}
}
fn policy(post:bfe_sim::gen::SpawnPost)->bfe_sim::gen::AiPostPolicy {
    bfe_sim::gen::AiPostPolicy {post,attack:[1.;6],defend:[1.;6],bans:0.,spawn:false,ally:false,class_spawn_points:0.}
}
#[test]
fn native_population_shares_driver_objectives_and_rejects_old_character_lives() {
    use bfe_sim::{gen::*,battle::commands::Commands,ai::{owned::population::{Population,Stale},control::world::TeamTable}};
    let mut store=crate::ai::owned::tests::empty_world();
    let home=store.post.insert(Post {active:true,team:1.,ai:Some(PostAi {spawn_path:Some(SpawnPath {present:true,points:vec![]}),..PostAi::default()}),..Post::default()});
    let enemy=store.post.insert(Post {active:true,team:2.,position:[20.,0.,0.],..Post::default()});
    let mut population=Population::new(2,3,false);population.add(&store,policy(SpawnPost::Post(home))).unwrap();population.add(&store,policy(SpawnPost::Post(enemy))).unwrap();
    let characters:Vec<_>=(0..3).map(|i|store.character.insert(Character {team:1.,human:i==2,..Character::default()})).collect();
    let units:Vec<_>=characters.iter().map(|&c|store.unit.insert(Unit {alive:true,generation:1.,character:Some(c),..Unit::default()})).collect();
    let vehicle=store.vehicle.insert(Vehicle {alive:true,seats:(0..3).map(|_|VehicleSeat::default()).collect(),..Vehicle::default()});
    for i in 0..3 {
        let lease=store.seat_lease.insert(SeatLease {unit:Some(units[i]),generation:1.,human:i==2,vehicle:Some(vehicle),index:i as f64,..SeatLease::default()});
        store.vehicle.get_mut(vehicle).unwrap().seats[i].lease=Some(lease);
        store.unit.get_mut(units[i]).unwrap().seat=Some(lease);store.character.get_mut(characters[i]).unwrap().unit=Some(units[i]);
    }
    let mut commands=Commands::new(3);let handles:Vec<_>=characters.iter().map(|&r|commands.adopt(&store,r)).collect();
    let mut roster=bfe_sim::ai::owned::roster::Roster::new(3);
    for handle in &handles {roster.add(handle.clone());}
    let mut entities=bfe_sim::ai::owned::entities::Entities::new(8);
    let raw=entities.register(bfe_sim::ai::owned::entities::Entity::Hull(vehicle));
    let wrapped=entities.register(bfe_sim::ai::owned::entities::Entity::HullEntry(vehicle));
    let seats=bfe_sim::vehicles::f2_seats::Seats::default();
    let turrets=bfe_sim::ai::owned::turrets::Turrets::new(0,0);
    let crew_pool=bfe_sim::ai::owned::scratch::Pool::new(2,3);
    ALLOCATIONS.with(|count|count.set(0));COUNTING.with(|active|active.set(true));
    for _ in 0..120 {
        let crew=bfe_sim::ai::owned::crew::read(&store,&entities,&roster,&commands,0).unwrap();
        assert_eq!(crew.vehicle,raw);assert_ne!(crew.vehicle,wrapped);
        assert!(crew.is_driver);assert_eq!(crew.driver,Some(0));assert!(crew.occupants.is_empty());
        assert_eq!(&*bfe_sim::ai::owned::crew::occupants(&store,&seats,&turrets,&roster,&commands,&crew_pool,1),&[0,1,2]);
    }
    COUNTING.with(|active|active.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    let passenger_lease=store.unit.get(units[1]).unwrap().seat.unwrap();
    store.seat_lease.get_mut(passenger_lease).unwrap().released=true;
    assert!(bfe_sim::ai::owned::crew::read(&store,&entities,&roster,&commands,1).is_none());
    assert_eq!(&*bfe_sim::ai::owned::crew::occupants(&store,&seats,&turrets,&roster,&commands,&crew_pool,0),&[0,2]);
    store.seat_lease.get_mut(passenger_lease).unwrap().released=false;
    let teams=TeamTable::default();let mut draws=ZeroDraws(0);
    ALLOCATIONS.with(|count|count.set(0));COUNTING.with(|active|active.set(true));
    let selected=population.request(&mut store,&mut commands,&handles[0],&mut draws,&teams,&PopulationFacts,0.,false,PostRef::Null).unwrap();
    COUNTING.with(|active|active.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(selected,Some(SpawnPost::Post(enemy)));assert_eq!(draws.0,1);
    assert_eq!(population.publications().iter().map(|p|p.character).collect::<Vec<_>>(),characters[..2]);
    assert!(population.publications().iter().all(|p|p.goal));
    assert_eq!(store.character.get(characters[1]).unwrap().objective,PostRef::Post(enemy));
    assert_eq!(store.character.get(characters[2]).unwrap().objective,PostRef::Null);
    population.request(&mut store,&mut commands,&handles[1],&mut draws,&teams,&PopulationFacts,0.,false,PostRef::Null).unwrap();
    assert_eq!(draws.0,1,"passenger inherits without a draw");
    commands.adopt(&store,characters[1]);commands.published();
    assert_eq!(population.request(&mut store,&mut commands,&handles[1],&mut draws,&teams,&PopulationFacts,0.,false,PostRef::Null),Err(Stale::Character));
    assert_eq!(draws.0,1);assert!(commands.dirty.is_empty());
    // Retiring a post cannot redirect a policy to a new post in its slot.
    store.post.remove(enemy).unwrap();let replacement=store.post.insert(Post {active:true,team:2.,..Post::default()});
    assert_eq!(replacement.slot,enemy.slot);
    assert_eq!(population.select(&store,characters[0],&mut draws,&teams,&PopulationFacts,0.,None,false,PostRef::Null),None);
}
#[test]
fn enhanced_population_keeps_threat_window_and_defense_exception() {
    use bfe_sim::{gen::*,battle::commands::Commands,ai::{owned::population::Population,control::world::TeamTable}};
    let mut store=crate::ai::owned::tests::empty_world();
    let home=store.post.insert(Post {active:true,team:1.,ai:Some(PostAi {spawn_path:Some(SpawnPath {present:true,points:vec![]}),..PostAi::default()}),..Post::default()});
    let enemy=store.post.insert(Post {active:true,team:2.,position:[20.,0.,0.],..Post::default()});
    let character=store.character.insert(Character {team:1.,..Character::default()});let mut owner=Commands::new(1);let handle=owner.adopt(&store,character);
    let mut population=Population::new(2,1,true);population.add(&store,policy(SpawnPost::Post(home))).unwrap();population.add(&store,policy(SpawnPost::Post(enemy))).unwrap();
    let teams=TeamTable::default();let mut draws=ZeroDraws(0);
    assert_eq!(population.request(&mut store,&mut owner,&handle,&mut draws,&teams,&PopulationFacts,0.,false,PostRef::Null).unwrap(),Some(SpawnPost::Post(enemy)));
    store.post.get_mut(home).unwrap().neutralize=1.;population.tick(&store,&PopulationFacts,0.);
    store.post.get_mut(home).unwrap().neutralize=0.;
    ALLOCATIONS.with(|count|count.set(0));COUNTING.with(|active|active.set(true));
    assert_eq!(population.request(&mut store,&mut owner,&handle,&mut draws,&teams,&PopulationFacts,7.999,true,PostRef::Post(home)).unwrap(),Some(SpawnPost::Post(home)));
    assert_eq!(population.request(&mut store,&mut owner,&handle,&mut draws,&teams,&PopulationFacts,8.,true,PostRef::Post(home)).unwrap(),Some(SpawnPost::Post(enemy)));
    COUNTING.with(|active|active.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(draws.0,3);
}
#[test]
fn class_jobs_read_native_ammo_and_keep_owned_by_presence() {
    use std::rc::Rc;
    use bfe_sim::{gen::*,store::{DefId,StrId},combat::defs::{Js,JsObj,DefTable,WeaponClass},ai::owned::classes::{Classes,DispenserKeys}};
    fn object(fields:Vec<(&str,Js)>,def:Option<(DefTable,DefId)>)->Rc<JsObj> {Rc::new(JsObj {fields:fields.into_iter().map(|(k,v)|(Rc::from(k),v)).collect(),def})}
    struct Keys(Option<StrId>);impl DispenserKeys for Keys {fn owned_key(&self,_:&crate::world::owner::World,_:&AiLifeHandle,_:DefId,_:u32)->Option<StrId>{self.0}}
    let item=object(vec![("base",Js::Str(Rc::from("mine"))),("LifeSpan",Js::Num(f64::INFINITY)),("PersonHealth",Js::Num(10.))],Some((DefTable::Item,DefId(7))));
    let def=object(vec![("base",Js::Str(Rc::from("dispenser"))),("RoundsPerClip",Js::Num(10.)),("ammo",Js::Num(5.)),("MaxItems",Js::Num(2.)),("item",Js::Obj(item))],None);
    let mut classes=Classes::new();classes.weapon(DefId(3),&WeaponClass::new(def));
    classes.weapon(DefId(4),&WeaponClass::new(object(vec![("base",Js::Str(Rc::from("repair")))],None)));
    let repair=Weapon {def:DefId(4),..Weapon::default()};
    let mut store=crate::ai::owned::tests::empty_world();let unit=store.unit.insert(Unit {alive:true,generation:1.,combat:Combat {health:70.,max_health:100.,weapons:vec![Weapon {def:DefId(3),clip:1.,reserve:0.,..Weapon::default()}],..Combat::default()},..Unit::default()});
    let key=AiLifeHandle {kind:KIND_UNIT.into(),slot:unit.slot,generation:unit.gen,life:1.};let owned=StrId(17);
    for _ in 0..2 {let r=store.item.insert(Item {def:DefId(7),owner:ItemOwner::Unit(unit),..Item::default()});store.items.items.push(r);}
    assert_eq!(classes.mine_weapon(&store,&key,&Keys(None)),None);
    assert_eq!(classes.mine_weapon(&store,&key,&Keys(Some(owned))),Some(0),"present empty ownedBy must not scan all items");
    assert_eq!(classes.needs_ammo(&store,&key),Some(true));assert_eq!(classes.needs_health(&store,&key),Some(true));
    ALLOCATIONS.with(|count|count.set(0));COUNTING.with(|active|active.set(true));
    for _ in 0..120 {
        assert_eq!(classes.service_weapon(&store,&key,0.,false,&Keys(Some(owned))),Some(0));
        assert!(!classes.is_repair(&store.unit.get(unit).unwrap().combat.weapons[0]));assert!(classes.is_repair(&repair));
    }
    COUNTING.with(|active|active.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    store.unit.get_mut(unit).unwrap().combat.weapons[0].clip=0.;
    assert_eq!(classes.mine_weapon(&store,&key,&Keys(Some(owned))),None);
    store.unit.get_mut(unit).unwrap().generation=2.;assert_eq!(classes.needs_ammo(&store,&key),None);
}

#[test]
fn native_class_jobs_read_body_and_holder_arsenals_and_observe_only_eligible_owner_queries() {
    use std::rc::Rc;
    use bfe_sim::{gen::*,store::DefId,combat::defs::{Js,JsObj,WeaponClass,WeaponDefs},
        ai::{owned::{state::{State,Capacity},entities::Entity,arsenals::Mounted,rows::Rows,seats::Source},nav::{Nav,PlannerOptions}},
        battle::{commands::Commands,live_phase::Live},vehicles::f2_seats::Seats};
    struct Holder(Weapon);
    impl Mounted for Holder {
        fn count(&self,_:&crate::world::owner::World,_:Source,_:usize)->Option<usize> {Some(1)}
        fn read<R>(&self,_:&crate::world::owner::World,_:Source,_:usize,index:usize,run:impl FnOnce(&Weapon)->R)->Option<R> {(index==0).then(||run(&self.0))}
        fn edit<R>(&mut self,_:&mut crate::world::owner::World,_:Source,_:usize,_:usize,_:impl FnOnce(&mut bfe_sim::ai::combat::world::WeaponSettings)->R)->Option<R> {panic!("class queries never edit weapons")}
    }
    fn object(fields:Vec<(&str,Js)>)->Rc<JsObj> {Rc::new(JsObj {fields:fields.into_iter().map(|(k,v)|(Rc::from(k),v)).collect(),def:None})}
    let repair=WeaponClass::new(object(vec![("base",Js::Str("repair".into())),("PersonHealth",Js::Num(5.))]));
    let mine=WeaponClass::new(object(vec![("base",Js::Str("dispenser".into())),("RoundsPerClip",Js::Num(1.)),("MaxItems",Js::Num(2.)),
        ("item",Js::Obj(object(vec![("base",Js::Str("mine".into())),("PersonHealth",Js::Num(2.))])))]));
    let mut state=State::new(Nav::empty(PlannerOptions::default()),false,Capacity {registry:8,characters:1,posts:0,hulls:1,buildings:0,controls:1,scopes:2,cameras:1});
    state.classes.weapon(DefId(0),&repair);state.classes.weapon(DefId(1),&mine);
    let mut defs=WeaponDefs::default();defs.set(DefId(0),repair);defs.set(DefId(1),mine);
    let mut store=crate::ai::owned::tests::empty_world();let body=store.unit.insert(Unit {alive:true,generation:1.,..Default::default()});
    store.unit.get_mut(body).unwrap().combat.weapons=vec![Weapon {def:DefId(0),..Default::default()},Weapon {def:DefId(1),clip:1.,..Default::default()}];
    let obj=state.entities.register(Entity::Unit(body));
    let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:1.,seats:vec![Default::default()],..Default::default()});
    let lease=store.seat_lease.insert(SeatLease {unit:Some(body),generation:1.,vehicle:Some(hull),..Default::default()});
    store.unit.get_mut(body).unwrap().seat=Some(lease);store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
    let mut seats=Seats::default();let control=state.births.construct(&store,&mut seats,&mut state.entities,lease,None).unwrap();
    store.seat_lease.get_mut(lease).unwrap().controllable=Some(control);
    let mounted=state.entities.find(Entity::Control(control)).unwrap();assert_ne!(obj,mounted);
    let mut holder=Holder(Weapon {def:DefId(1),clip:1.,..Default::default()});
    let mut changes=bfe_sim::units::ownership::Ownership::default();let mut characters=Commands::new(0);let buildings=Live::new(0,0,0);
    crate::vehicles::module::f2::live_seats::admit_test_seats(&mut store.fleet,std::mem::take(&mut seats));
  let mut rows=Rows {w:&mut store,buildings:&buildings.buildings,characters:&mut characters,state:&mut state,changes:&mut changes,mounted:&mut holder,definitions:&defs};
    let owned=Cell::new(1);let observations=Cell::new(0);let last=Cell::new(None);
    let mut count=|_:&mut Rows<'_,_,_>,owner,weapon:(DefId,u32)| {assert_eq!(weapon.0,DefId(1));last.set(Some(owner));observations.set(observations.get()+1);owned.get()};
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for _ in 0..120 {
        for (owner,mine_index) in [(obj,1),(mounted,0)] {
            assert!(rows.dispenser_available(owner,mine_index,&mut count));assert_eq!(last.get(),Some(owner));
            assert_eq!(rows.mine_weapon(owner,&mut count),Some(mine_index));assert_eq!(last.get(),Some(owner));
            assert_eq!(rows.service_weapon(owner,0.,false,&mut count),Some(0));
        }
        let before=observations.get();owned.set(2);
        assert_eq!(rows.mine_weapon(obj,&mut count),None);assert_eq!(rows.mine_weapon(mounted,&mut count),None);
        assert_eq!(observations.get(),before+2);
        let before=observations.get();assert_eq!(rows.service_weapon(obj,0.,false,&mut count),Some(0));
        assert_eq!(observations.get(),before,"repair selection never observes ownedBy");
        rows.w.sim.store.unit.get_mut(body).unwrap().combat.weapons[1].clip=0.;rows.mounted.0.clip=0.;
        assert!(!rows.dispenser_available(obj,1,&mut count));assert!(!rows.dispenser_available(mounted,0,&mut count));
        assert_eq!(observations.get(),before,"empty weapons do not number an owner");
        rows.w.sim.store.unit.get_mut(body).unwrap().combat.weapons[1].clip=1.;rows.mounted.0.clip=1.;owned.set(1);
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    let before=observations.get();rows.w.sim.store.seat_lease.get_mut(lease).unwrap().released=true;
    assert_eq!(rows.mine_weapon(mounted,&mut count),None);assert_eq!(observations.get(),before);
}

#[test]
fn reserved_query_lists_preserve_nested_order_and_return_on_early_exit() {
    use bfe_sim::ai::owned::scratch::Pool;
    let pool = Pool::new(2, 64);
    ALLOCATIONS.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    {
        for _ in 0..120 {
            let mut outer = pool.take();
            for i in 0..30 { outer.push_reserved(i); }
            for (expected, value) in outer.into_iter().enumerate() {
                assert_eq!(expected, value);
                let mut inner = pool.take();
                inner.push_reserved(value);
                inner.push_reserved(value + 1);
                assert_eq!(inner.into_iter().next(), Some(value));
            }
            let mut after = pool.take();
            assert!(after.is_empty());
            after.push_reserved(9);
            assert_eq!(after.into_iter().next(), Some(9));
        }
    }
    COUNTING.with(|c| c.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn mounted_policy_reads_current_f2_rows_and_preserves_frozen_fire_edges() {
    use bfe_sim::{gen::*,store::Ref,vehicles::f2_seats::Seats,
        seats::{hulls::HullSide,lease::control_key},ai::{owned::{entities::{Entities,Entity},mounts::{Mounts,HullClass}},vehicles::world::{SeatView,Stick}}};
    let mut store=crate::ai::owned::tests::empty_world();let mut seats=Seats::default();let mut entities=Entities::new(16);
    let unit=store.unit.insert(Unit {alive:true,generation:4.,..Unit::default()});
    let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:2.,seats:vec![VehicleSeat::default();2],..Vehicle::default()});
    seats.add_hull(&store,HullSide {vehicle:hull,weapons:vec![1,0],..HullSide::default()});
    let control=Ref::new(0,1);
    let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:4.,vehicle:Some(hull),controllable:Some(control),team:2.,..SeatLease::default()});
    store.unit.get_mut(unit).unwrap().seat=Some(lease);store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
    let mut mounts=Mounts::new(16,3);
    mounts.register(&store,&mut entities,hull,HullClass {name:"craft".into(),seats:vec![SeatView {yaw_limits:Some(" -30 30".into()),..SeatView::default()};2],..HullClass::default()});
    let towed=store.vehicle.insert(Vehicle::default());let tow_object=entities.register(Entity::Hull(towed));store.vehicles.vehicles.extend([hull,towed]);store.vehicle.get_mut(hull).unwrap().tow_target=1.;
    let member=Member::VehicleTarget(Ref::new(9,2));
    let entry=mounts.target(&entities,hull,bfe_sim::ai::owned::mounts::Target {member,kind:6.,category:3.,policy:std::rc::Rc::new(Default::default()),combat:Default::default()}).unwrap();
    let person=entities.register(Entity::Unit(unit));entities.register(Entity::Lease(lease));entities.register(Entity::Control(control));
    assert_eq!(mounts.seat(&store,&entities,hull,0).unwrap().prepared_limits.unwrap()[0],[0.,-30.*std::f64::consts::PI/180.]);
    mounts.control(&mut seats,control,false);
    let intent=&mut seats.hulls.controls.get_mut(&control_key(control)).unwrap().intent;
    intent.buttons=[1.,0.];intent.reload=true;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        store.vehicle.get_mut(hull).unwrap().position[0]=tick as f64;
        let clock=bfe_sim::ai::vehicles::service::Decay {idle:tick as f64,sample:tick as f64+0.5};
        assert!(mounts.write_decay(hull,clock));assert_eq!(mounts.decay(hull),Some(Some(clock)));
        let body=store.vehicle.get_mut(hull).unwrap();body.matrix=three::Matrix4::default().elements;body.matrix[12]=tick as f64;body.center=[0.,2.,0.];body.bound_radius=3.;
        let policy=mounts.policy_target(&store,&entities,hull).unwrap();assert_eq!(policy.key,entry);assert_eq!(policy.id,9.);assert_eq!(policy.team,2.);assert_eq!(policy.kind,6.);assert_eq!(policy.sphere_radius,Some(3.));
        let target=mounts.combat_target(&store,&entities,hull).unwrap();assert_eq!(target.team,2.);assert_eq!(target.category,Some(3.));assert!(target.tow_on);assert_eq!(target.towed,Some(tow_object));assert!(target.body_leases.is_none());assert!(target.arsenal.is_none());
        assert_eq!(mounts.sphere(&store,hull).unwrap().center,three::Vector3::new(tick as f64,2.,0.));
        let h=mounts.hull(&store,&seats,&entities,hull).unwrap();
        assert_eq!(h.position.x,tick as f64);assert_eq!(h.controlling_team,2.);assert_eq!(h.occupants,1.);assert_eq!(h.free_seats,1.);
        assert_eq!(mounts.hulls(&store,&seats,&entities).len(),1);
        let l=mounts.lease(&store,&entities,lease).unwrap();assert!(l.live);assert_eq!(l.unit,Some(person));
        assert!(mounts.analogs(&store,&mut seats,lease,control,&Stick {forward:0.75,up:1.,..Stick::default()}));
        let intent=&seats.hulls.controls.get(&control_key(control)).unwrap().intent;
        assert_eq!(intent.forward,0.75);assert_eq!(intent.buttons,[1.,0.]);assert!(intent.reload);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    store.vehicle.get_mut(hull).unwrap().target_generation+=1.;
    assert_eq!(mounts.decay(hull).unwrap().unwrap().idle,119.);assert_eq!(mounts.policy_target(&store,&entities,hull).unwrap().generation,3.);
    assert!(mounts.decay(Ref::new(hull.slot,hull.gen+1)).is_none());
    assert!(!mounts.write_decay(Ref::new(hull.slot,hull.gen+1),Default::default()));
    let previous=entities.life(&store,person).unwrap();
    store.unit.get_mut(unit).unwrap().generation=5.;
    assert!(previous.unit(&store).is_none());assert!(!mounts.lease(&store,&entities,lease).unwrap().live);
    assert!(!mounts.analogs(&store,&mut seats,lease,control,&Stick::default()));
    store.unit.remove(unit);let replacement=store.unit.insert(Unit::default());assert_eq!(replacement.slot,unit.slot);
    let replacement_id=entities.register(Entity::Unit(replacement));assert_ne!(person,replacement_id);assert_eq!(entities.get(person),None);
    assert_eq!(entities.find(Entity::Unit(unit)),None);
}

#[test]
fn native_weapon_edits_keep_enhanced_floor_life_and_sparse_publication() {
    use bfe_sim::{gen::*,units::ownership::Ownership,ai::owned::weapons,combat::defs::{Js,JsObj}};
    use std::rc::Rc;
    let mut store=crate::ai::owned::tests::empty_world();let mut owner=Ownership::default();
    let reference=store.unit.insert(Unit {alive:true,generation:3.,combat:Combat {weapons:vec![Weapon {clip:0.5,reserve:8.,floor:0.,kick:1.,sniper_scale:0.,spread_scale:1.,..Weapon::default()}],..Combat::default()},..Unit::default()});
    let key=owner.adopt(&store,reference).unwrap();owner.clear_changed();
    let mut def=JsObj::default();def.fields.push((Rc::from("SpreadThreshold"),Js::Num(0.25)));
    weapons::edit(&mut store,&mut owner,&key,0,|w|w.player_spread(&def)).unwrap();
    assert_eq!(owner.changed(),&[reference]);owner.clear_changed();
    let w=&store.unit.get(reference).unwrap().combat.weapons[0];
    assert!(w.enhanced_floor);assert_eq!(w.floor,-0.25);assert_eq!(w.kick,-0.25);assert_eq!(w.clip,0.5);assert_eq!(w.reserve,8.);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {weapons::edit(&mut store,&mut owner,&key,0,|w|w.player_spread(&def)).unwrap();}
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert!(owner.changed().is_empty());
    weapons::edit(&mut store,&mut owner,&key,0,|w|w.sniper_scale=Some(-0.)).unwrap();assert_eq!(owner.changed(),&[reference]);
    weapons::buttons(&mut store,&key,1.,0.).unwrap();weapons::aim(&mut store,&key,Some(three::Vector3::new(1.,2.,3.))).unwrap();
    assert_eq!(store.unit.get(reference).unwrap().intent.aim,Some([1.,2.,3.]));
    store.unit.get_mut(reference).unwrap().generation=4.;
    assert!(weapons::edit(&mut store,&mut owner,&key,0,|_|panic!("stale edit ran")).is_err());
    assert!(weapons::buttons(&mut store,&key,0.,0.).is_err());
}

#[test]
fn native_post_callbacks_keep_team_roster_and_recruitment_budgets() {
    use bfe_sim::{gen::*,store::{Ref,DefId},battle::commands::Commands,ai::{owned::{population::{Population,Recruitment},scratch::{List,Pool}},control::world::TeamTable,nav::math::Vec3}};
    struct Recruit {members:Vec<Member>,pool:Pool<Member>,calls:usize}
    impl Recruitment for Recruit {
        fn query(&mut self,_:&mut Store,_:Vec3,r:f64,team:f64,_:&TeamTable)->List<Member> {
            assert_eq!(r,60.);assert_eq!(team,1.);self.calls+=1;
            let mut out=self.pool.take();for m in &self.members {out.push_reserved(*m);}out
        }
        fn mask(&self,u:&Unit)->f64 {u.class_def.0 as f64}
    }
    let mut store=crate::ai::owned::tests::empty_world();let mut commands=Commands::new(13);
    let old=store.post.insert(Post {active:true,team:0.,..Post::default()});
    let enemy=store.post.insert(Post {active:true,team:2.,..Post::default()});
    let mut population=Population::new(2,13,false);let mut p=policy(SpawnPost::Post(old));p.bans=2.;
    population.add(&store,p).unwrap();population.add(&store,policy(SpawnPost::Post(enemy))).unwrap();
    let mut characters=Vec::new();let mut units=Vec::new();let mut roster=Vec::new();
    for i in 0..13 {
        let c=store.character.insert(Character {team:1.,human:i==0,objective:if i==12 {PostRef::Post(old)}else{PostRef::Null},..Character::default()});
        let u=store.unit.insert(Unit {team:1.,alive:true,kind:if i==1 {5.}else{0.},class_def:DefId(if i==2 {2}else{0}),generation:1.,human:i==0,character:Some(c),controller:Some(Ref::new(i,1)),..Unit::default()});
        store.character.get_mut(c).unwrap().unit=Some(u);characters.push(c);units.push(u);roster.push(commands.adopt(&store,c));
    }
    let mut members=vec![Member::VehicleTarget(Ref::new(0,1))];members.extend(units[..12].iter().copied().map(Member::Unit));
    let mut recruit=Recruit {members,pool:Pool::new(1,13),calls:0};let teams=TeamTable::default();let mut draws=ZeroDraws(0);
    commands.published();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    population.owner_changed(&mut store,&mut commands,&roster,&mut draws,&teams,&PopulationFacts,&mut recruit,0.,SpawnPost::Post(old),1.,0.).unwrap();
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(draws.0,1);assert_eq!(recruit.calls,1);assert_eq!(population.publications().len(),6);
    assert_eq!(population.publications()[0].character,characters[12]);
    assert_eq!(population.publications()[1..].iter().map(|p|p.character).collect::<Vec<_>>(),characters[3..8]);
    assert_eq!(store.character.get(characters[8]).unwrap().objective,PostRef::Null,"fifth recruit ends the scan");
    commands.published();store.post.get_mut(old).unwrap().active=false;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    population.deactivated(&mut store,&mut commands,&roster,&mut draws,&teams,&PopulationFacts,1.,SpawnPost::Post(old)).unwrap();
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(draws.0,7);assert_eq!(population.publications().len(),6);
    for &c in &characters[3..8] {assert_eq!(store.character.get(c).unwrap().objective,PostRef::Post(enemy));}
    assert_eq!(store.character.get(characters[12]).unwrap().objective,PostRef::Post(enemy));
    population.owner_changed(&mut store,&mut commands,&roster,&mut draws,&teams,&PopulationFacts,&mut recruit,2.,SpawnPost::Post(old),0.,0.).unwrap();
    assert!(population.publications().is_empty());assert_eq!(draws.0,7);assert_eq!(recruit.calls,1);
}

#[test]
fn control_observations_read_canonical_scalars_without_copying_arsenals_or_shapes() {
    use bfe_sim::{gen::*,store::DefId,ai::{owned::{units::Units,entities::{Entities,Entity}},control::world::ClassSnap}};
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(2);let mut views=Units::new();
    let r=store.unit.insert(Unit {alive:true,generation:4.,class_def:DefId(2),combat:Combat {health:90.,max_health:100.,weapons:vec![Weapon::default();8],..Combat::default()},soldier:UnitBody::Soldier(Soldier {max_speed:7.,..Soldier::default()}),spheres_tick:-99.,..Unit::default()});
    let actor=entities.register(Entity::Unit(r));
    views.class(DefId(2),ClassSnap::default(),"infantry".into());
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        let UnitBody::Soldier(body)=&mut store.unit.get_mut(r).unwrap().soldier else {unreachable!()};body.position=[tick as f64,2.,3.];
        let observed=views.read(&store,&entities,actor).unwrap();
        assert_eq!(observed.position.x,tick as f64);assert_eq!(observed.soldier.unwrap().max_speed,Some(7.));
        assert_eq!(observed.combat_health,Some(90.));assert!(observed.weapons.is_empty());
        assert!(views.command(&mut store,&entities,actor,bfe_sim::ai::owned::intent::Command::Analogs([0.5,0.,0.,0.])));
        assert_eq!(store.unit.get(r).unwrap().intent.forward,0.5);
        assert_eq!(store.unit.get(r).unwrap().spheres_tick,-99.,"scalar reads must not observe geometry");
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    store.unit.get_mut(r).unwrap().generation=5.;assert_eq!(views.read(&store,&entities,actor).unwrap().generation,5.);
    store.unit.remove(r);assert!(views.read(&store,&entities,actor).is_none());
}

#[test]
fn core_objective_publication_writes_the_f4_character_without_a_roster_mirror() {
    use bfe_sim::{gen::*,battle::commands::Commands,ai::{owned::{roster::Roster,entities::{Entities,Entity},population::Population},control::{Core,Options,controller::Controller,difficulty::{Difficulty,DifficultyOptions},world::{Characters,CharacterSnap,TeamTable}}}};
    struct Battle {store:Box<crate::world::owner::World>,commands:Commands,entities:Entities,population:Population,roster:Roster}
    impl Characters for Battle {
        fn character(&self,index:usize)->CharacterSnap {self.roster.read(&self.store,&self.commands,&self.entities,&self.population,index).unwrap()}
        fn character_set_objective(&mut self,index:usize,post:Option<usize>) {self.roster.assign(&mut self.store,&mut self.commands,&self.population,index,post).unwrap();}
    }
    let mut b=Battle {store:crate::ai::owned::tests::empty_world(),commands:Commands::new(1),entities:Entities::new(2),population:Population::new(1,1,false),roster:Roster::new(1)};
    let post=b.store.post.insert(Post {active:true,..Post::default()});b.population.add(&b.store,policy(SpawnPost::Post(post))).unwrap();
    let character=b.store.character.insert(Character {team:1.,..Character::default()});let handle=b.commands.adopt(&b.store,character);b.roster.add(handle.clone());
    let unit=b.store.unit.insert(Unit {alive:true,generation:1.,..Unit::default()});b.entities.register(Entity::Unit(unit));b.store.character.get_mut(character).unwrap().unit=Some(unit);
    let mut core=Core::new(Options::default(),TeamTable::default(),Difficulty::classic(DifficultyOptions::default()),1);
    core.controllers.push(Controller::new(0));core.by_character[0]=Some(0);
    core.objectives_assign(&mut b,0,Some(0));assert_eq!(b.store.character.get(character).unwrap().objective,PostRef::Post(post));
    assert_eq!(b.commands.dirty,&[character]);b.commands.published();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {core.objectives_republish(&mut b,0);assert_eq!(core.controllers[0].commands.queue.len(),1);}
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert!(b.commands.dirty.is_empty());
    b.store.character.get_mut(character).unwrap().team=2.;assert_eq!(b.character(0).team,2.);
    b.store.unit.get_mut(unit).unwrap().generation=2.;assert!(b.roster.read(&b.store,&b.commands,&b.entities,&b.population,0).is_ok(),"Unit respawn is not a new Character life");
    let replacement=b.commands.adopt(&b.store,character);assert!(b.roster.read(&b.store,&b.commands,&b.entities,&b.population,0).is_err());
    b.roster.replace(0,replacement);core.objectives_assign(&mut b,0,None);assert_eq!(b.store.character.get(character).unwrap().objective,PostRef::Null);
}

#[test]
fn mobile_post_policy_observes_current_class_weights_and_hull_position() {
    use bfe_sim::{gen::*,ai::{owned::{posts::{Posts,Definition},population::Population},control::world::TeamTable}};
    let mut store=crate::ai::owned::tests::empty_world();let mut population=Population::new(2,1,false);let mut posts=Posts::new(2);
    let hull=store.vehicle.insert(Vehicle {alive:true,position:[9.,0.,0.],..Vehicle::default()});
    let mobile=store.mobile_post.insert(MobilePost {active:true,team:2.,vehicle:Some(hull),ai:MobileAi {atk:vec![0.;6],def:vec![100.;6],..MobileAi::default()},..MobilePost::default()});
    let placed=store.post.insert(Post {active:true,team:2.,..Post::default()});
    posts.register(&store,&mut population,policy(SpawnPost::MobilePost(mobile)),Definition {name:Some("transport-0".into()),..Definition::default()}).unwrap();
    posts.register(&store,&mut population,policy(SpawnPost::Post(placed)),Definition::default()).unwrap();
    let character=store.character.insert(Character {team:1.,..Character::default()});let teams=TeamTable::default();struct Quarter;
    impl bfe_sim::ai::control::world::Draws for Quarter {fn a_uniform(&mut self)->f64 {0.25}fn b_uniform(&mut self)->f64 {panic!("unexpected B draw")}}
    let mut draws=Quarter;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for i in 0..120 {
        store.mobile_post.get_mut(mobile).unwrap().ai.atk[0]=if i%2==0 {0.}else{100.};
        store.vehicle.get_mut(hull).unwrap().position[0]=i as f64;
        let observed=posts.read(&store,&population,0,&PopulationFacts).unwrap();assert_eq!(observed.position.x,i as f64);
        assert_eq!(observed.ai.atk[0],if i%2==0 {0.}else{100.});
        assert_eq!(population.select(&store,character,&mut draws,&teams,&PopulationFacts,0.,None,false,PostRef::Null),
            Some(if i%2==0 {SpawnPost::Post(placed)}else{SpawnPost::MobilePost(mobile)}));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    store.mobile_post.get_mut(mobile).unwrap().active=false;store.mobile_post.get_mut(mobile).unwrap().last_position=[42.,0.,0.];
    let observed=posts.read(&store,&population,0,&PopulationFacts).unwrap();assert_eq!(observed.position.x,42.);assert!(!observed.active);
}

#[test]
fn combat_weapon_views_and_selection_use_current_f3_arsenal_rows() {
    use bfe_sim::{gen::*,store::DefId,units::ownership::Ownership,combat::defs::{WeaponDefs,WeaponClass,JsObj,Js},ai::owned::weapons::{self,Definitions}};
    use std::rc::Rc;
    let mut defs=WeaponDefs::default();let mut prepared=Definitions::new();
    for (id,channel) in [(0,0.),(1,0.),(2,1.)] {
        let class=WeaponClass::new(Rc::new(JsObj {fields:vec![("channel".into(),Js::Num(channel)),("RoundsPerClip".into(),Js::Num(10.)),("ammo".into(),Js::Num(5.))],def:None}));
        prepared.register(DefId(id),&class);defs.set(DefId(id),class);
    }
    let mut store=crate::ai::owned::tests::empty_world();let mut owner=Ownership::default();
    let reference=store.unit.insert(Unit {alive:true,generation:1.,combat:Combat {weapons:(0..3).map(|i|Weapon::create(DefId(i),&defs,false)).collect(),..Combat::default()},..Unit::default()});
    let key=owner.adopt(&store,reference).unwrap();owner.clear_changed();
    assert_eq!(prepared.current(&store,&key,0.),Some(0));assert_eq!(prepared.current(&store,&key,1.),Some(2));
    store.unit.get_mut(reference).unwrap().combat.weapons[0].firing=true;
    assert_eq!(weapons::select(&mut store,&mut owner,&key,1,&defs),Ok(false));assert!(owner.changed().is_empty());
    let w=&mut store.unit.get_mut(reference).unwrap().combat.weapons[0];w.firing=false;w.charging=true;w.held=2.;w.reload_left=3.;w.lock_target_id=99.;
    assert_eq!(weapons::select(&mut store,&mut owner,&key,1,&defs),Ok(true));assert_eq!(owner.changed(),&[reference]);
    let old=&store.unit.get(reference).unwrap().combat.weapons[0];assert!(!old.charging);assert_eq!(old.held,0.);assert_eq!(old.reload_left,0.);assert_eq!(old.lock_target_id,-1.);
    owner.clear_changed();let first=prepared.view(&store,&key,1).unwrap();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for i in 0..120 {
        store.unit.get_mut(reference).unwrap().combat.weapons[1].clip=i as f64/120.;
        let observed=prepared.view(&store,&key,1).unwrap();assert_eq!(observed.clip,i as f64/120.);
        assert!(Rc::ptr_eq(first.levels.as_ref().unwrap(),observed.levels.as_ref().unwrap()));
        assert_eq!(prepared.current(&store,&key,0.),Some(1));assert_eq!(weapons::select(&mut store,&mut owner,&key,1,&defs),Ok(true));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert!(owner.changed().is_empty());
    store.unit.get_mut(reference).unwrap().generation=2.;assert!(prepared.view(&store,&key,1).is_none());assert!(weapons::select(&mut store,&mut owner,&key,1,&defs).is_err());
}

#[test]
fn native_vehicle_and_combat_bindings_share_prepared_weapon_policy_and_cluster_state() {
    use bfe_sim::{combat::defs::{Js,JsObj,WeaponClass},ai::combat::weapons_ai::Shared};
    use std::rc::Rc;
    let class=WeaponClass::new(Rc::new(JsObj {fields:vec![("MaxRange".into(),Js::Num(37.))],def:None}));
    let mut shared=Shared::default();let expected=shared.weapons.weapon_ai(&class.levels[0].obj);
    shared.prepare_class(&class);shared.begin(8);let vehicle=shared.share();
    assert!(Rc::ptr_eq(&shared.weapons,&vehicle.weapons));assert!(Rc::ptr_eq(&shared.clusters,&vehicle.clusters));
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for i in 0..120 {
        assert_eq!(vehicle.weapons.weapon_ai(&class.levels[0].obj),expected);
        {let mut clusters=vehicle.clusters.borrow_mut();let key=clusters.state(9,None,1.,true).unwrap();clusters.states.get_mut(&key).unwrap().flagged_at=i as f64;}
        assert_eq!(shared.clusters.borrow().states[&9].flagged_at,i as f64);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    shared.clusters.borrow_mut().state(9,None,2.,false);assert_eq!(vehicle.clusters.borrow().states[&9].flagged_at,f64::NEG_INFINITY);
}

#[test]
fn combat_observations_read_f3_bodies_without_shape_or_arsenal_snapshots() {
    use bfe_sim::{gen::*,store::{DefId,Ref},combat::defs::{Js,JsObj},ai::{owned::{combat::{Units,ControlOwners},entities::{Entities,Entity}},combat::{world::Obj,agent::role_of,weapons_ai::category_of}}};
    struct Controls;impl ControlOwners for Controls {fn controller(&self,_:Ref<SeatUnit>)->Option<Obj>{None}}
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(4);let mut facts=Units::new();
    facts.class(DefId(3),&JsObj {fields:vec![("UnitType".into(),Js::Str("support".into())),("HealthType".into(),Js::Str("Droid".into()))],def:None});
    let r=store.unit.insert(Unit {alive:true,generation:1.,class_def:DefId(3),spheres_tick:-9.,combat:Combat {max_health:100.,weapons:vec![Weapon::default();8],..Combat::default()},..Unit::default()});
    let actor=entities.register(Entity::Unit(r));let lease=store.seat_lease.insert(SeatLease {unit:Some(r),generation:1.,..SeatLease::default()});
    store.unit.get_mut(r).unwrap().seat=Some(lease);entities.register(Entity::Lease(lease));
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for i in 0..120 {
        let UnitBody::Soldier(s)=&mut store.unit.get_mut(r).unwrap().soldier else {unreachable!()};s.position=[i as f64,0.,0.];s.fuel=i as f64;
        let observed=facts.read(&store,&entities,&Controls,actor).unwrap();
        assert_eq!(observed.position.x,i as f64);assert_eq!(observed.soldier.as_ref().unwrap().fuel,Js::Num(i as f64));
        assert!(observed.arsenal.is_none());assert!(observed.body_leases.is_none());assert!(!observed.seat.unwrap().live_false);
        assert_eq!(category_of(&observed),2.);assert_eq!(role_of(observed.class.as_ref()),3.);
        assert_eq!(store.unit.get(r).unwrap().spheres_tick,-9.);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    store.unit.get_mut(r).unwrap().generation=2.;assert!(facts.read(&store,&entities,&Controls,actor).unwrap().seat.unwrap().live_false);
}

#[test]
fn native_seat_controls_read_live_hull_and_reject_released_or_respawned_bindings() {
    use bfe_sim::{gen::*,store::Ref,vehicles::f2_seats::Seats,seats::lease::{SeatControl,control_key},
        ai::{owned::{controls::HullControls,entities::{Entities,Entity}},vehicles::world::ClassView}};
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(8);let mut controls=HullControls::new();let mut seats=Seats::default();
    let unit=store.unit.insert(Unit {alive:true,hero:true,generation:3.,..Unit::default()});
    let hull=store.vehicle.insert(Vehicle {alive:true,health:200.,max_health:400.,size:[4.,0.,6.],target_generation:4.,seats:vec![VehicleSeat::default()],..Vehicle::default()});
    entities.register(Entity::Hull(hull));
    let towed=store.vehicle.insert(Vehicle::default());let tow_object=entities.register(Entity::Hull(towed));store.vehicles.vehicles.extend([hull,towed]);store.vehicle.get_mut(hull).unwrap().tow_target=1.;
    let control=Ref::new(2,8);
    let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:3.,vehicle:Some(hull),controllable:Some(control),team:2.,..SeatLease::default()});
    store.unit.get_mut(unit).unwrap().seat=Some(lease);store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
    assert!(controls.register(&store,&mut entities,control,lease,AiLifeHandle {kind:u32::from(KIND_VEHICLE),slot:hull.slot,generation:hull.gen,life:4.},5.,
        ClassView {forward_speed:Some(75.),..ClassView::default()},"flyer".into(),None,bfe_sim::ai::owned::seats::Occupant::read(&store,lease).unwrap()));
    let class=controls.class(5.,bfe_sim::ai::control::world::ClassSnap {ai_size_type:bfe_sim::ai::nav::units::SizeType::Text("FLYER".into()),max_health:Some(300.),..Default::default()},
        bfe_sim::ai::combat::world::ClassView {fields:std::rc::Rc::new(bfe_sim::ai::combat::world::ClassFields {forward_speed:bfe_sim::combat::defs::Js::Num(75.),..Default::default()}),..Default::default()});
    assert!(controls.set_class(control,class));
    store.unit.get_mut(unit).unwrap().hero=false;
    seats.control(control,SeatControl {driven:true,..SeatControl::default()},false);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        let body=store.vehicle.get_mut(hull).unwrap();body.position[0]=tick as f64;body.velocity[2]=tick as f64;
        assert!(controls.set_class(control,class));
        assert_eq!(controls.edit_steer(control,|s|*s=None),Some(()));
        assert!(controls.steer(control).unwrap().is_none());
        let u=controls.unit(&store,&entities,control).unwrap();assert!(u.alive);assert_eq!(u.generation,1.);assert_eq!(u.kind,5.);
        assert_eq!(u.position.x,tick as f64);assert_eq!(u.velocity.z,tick as f64);assert_eq!(u.soldier_max_speed,Some(75.));assert_eq!(u.up_y,None);
        {let intent=controls.intent(&store,&mut seats,control).unwrap();intent.buttons[0]=1.;intent.reload=true;intent.boost=true;}
        assert!(controls.command(&store,&mut seats,control,bfe_sim::ai::owned::intent::Command::Analogs([0.5,0.25,0.1,0.2])));
        assert!(controls.command(&store,&mut seats,control,bfe_sim::ai::owned::intent::Command::Buttons([1.,0.])));
        assert!(controls.command(&store,&mut seats,control,bfe_sim::ai::owned::intent::Command::Aim(Some([tick as f64,0.,1.]))));
        let policy=controls.policy_unit(&store,&entities,control,&|_|panic!("no Character")).unwrap();
        assert!(policy.hero);assert!(policy.is_seat_unit);assert_eq!(policy.combat_health,Some(200.));
        assert_eq!(policy.class_def.as_ref().unwrap().max_health,Some(300.));assert_eq!(bfe_sim::ai::control::goals::class_travel_mask(policy.class_def.as_deref()),32.);
        let combat=controls.combat_unit(&store,&entities,control).unwrap();assert!(combat.hero);assert!(combat.pilot);assert!(combat.tow_on);assert_eq!(combat.towed,Some(tow_object));assert!(combat.targetable_false);assert!(combat.arsenal.is_none());assert!(combat.body_leases.is_none());
        let intent=&seats.hulls.controls.get(&control_key(control)).unwrap().intent;
        assert_eq!(intent.forward,0.5);assert_eq!(intent.right,0.25);assert_eq!(intent.buttons,[1.,0.]);assert!(intent.reload);assert!(intent.boost);assert_eq!(intent.aim,Some(three::Vector3::new(tick as f64,0.,1.)));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(seats.hulls.controls.get(&control_key(control)).unwrap().intent.buttons,[1.,0.]);
    store.seat_lease.get_mut(lease).unwrap().released=true;
    assert!(!controls.unit(&store,&entities,control).unwrap().alive);assert!(controls.intent(&store,&mut seats,control).is_none());
    store.seat_lease.get_mut(lease).unwrap().released=false;store.vehicle.get_mut(hull).unwrap().target_generation=5.;
    assert!(controls.unit(&store,&entities,control).is_none());assert!(controls.intent(&store,&mut seats,control).is_none());
    assert!(controls.retire(control));assert!(!controls.retire(control));
    assert!(controls.steer(control).is_none());assert!(!controls.set_class(control,class));
    assert!(controls.edit_steer(control,|_|panic!("stale control")).is_none());
}

#[test]
fn native_hint_views_share_live_reservations_without_copying_ring_tables() {
    use bfe_sim::ai::{nav::hints::{build_hints,HintRecord,HintSet},owned::hints::Hints};
    let record=HintRecord {kind:Some("cover".into()),name:None,xfrm:[1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.],properties:vec![]};
    let mut nav=owned_navigation(false).nav;nav.hints=HintSet::new(build_hints(&[record.clone(),record],None,350.));
    assert_eq!(nav.hints.list.len(),2);
    nav.hints.list[0].link=Some(1);nav.hints.list[1].link=Some(0);
    nav.hints.list[0].siblings=vec![0,1];nav.hints.list[1].siblings=vec![0,1];
    let hints=Hints::new(&nav,3,|_|None,|i|i as u64+50);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        let now=tick as f64;nav.mark_hint(0,Some(7),now);
        let view=hints.read(&nav,0);assert_eq!(view.reserved_at,Some(now));assert_eq!(&*view.linked,&[1]);
        assert!(hints.query(&nav,4,Vec3::default(),10.,30.,Some(7),now).is_empty());
        // Cover queries omit a requester and therefore keep reserved hints.
        let cover=hints.cover(&nav,Vec3::default(),10.,now,|i|i as u64+50);
        assert_eq!(cover.len(),2);assert_eq!(cover[0].obj,50);assert_eq!(&*cover[0].siblings,&[50,51]);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn weapon_and_lifecycle_stream_handles_share_each_original_draw_point() {
    use bfe_sim::ai::{phase::RandomState,random::{ClassicRandom,Random},control::world::Draws};
    let mut expected_a=ClassicRandom::from_state(81);let mut expected_b=ClassicRandom::from_state(81^0x9e37_79b9);let mut expected_c=ClassicRandom::from_state(81^0x51ed_270b);
    // Cold roster creation consumed A before native activation. B/C are
    // separate current states, rather than offsets from that advanced A.
    for _ in 0..37 {expected_a.next();}for _ in 0..3 {expected_b.next();}
    let mut phase=RandomState::from_stream_states(81,[expected_a.seed(),expected_b.seed(),expected_c.seed()],true);
    assert_eq!(phase.draw_counts(),[0,0,0]);
    let mut weapon=phase.weapon_stream();let mut nested_weapon=weapon.clone();let mut command=phase.command_stream();let mut aim=phase.aim_stream();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        assert_eq!(phase.a_uniform().to_bits(),expected_a.uniform().to_bits());
        assert_eq!(weapon.uniform().to_bits(),expected_a.uniform().to_bits());
        assert_eq!(command.range(-2.,9.).to_bits(),expected_b.range(-2.,9.).to_bits());
        assert_eq!(bfe_sim::battle::spawning::Draws::next(&mut phase),expected_a.next() as f64);
        assert_eq!(nested_weapon.int(3.,3.),3.); // no draw on a degenerate interval
        assert_eq!(nested_weapon.next(),expected_a.next());
        assert_eq!(phase.b_uniform().to_bits(),expected_b.uniform().to_bits());
        assert_eq!(aim.uniform().to_bits(),expected_c.uniform().to_bits());assert_eq!(phase.aim_uniform().to_bits(),expected_c.uniform().to_bits());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(phase.draw_counts(),[480,240,240]);assert_eq!(phase.seeds()[0],weapon.seed());
    assert_eq!(phase.seeds()[1],command.seed());
}

#[test]
fn native_navigation_drives_current_f2_seat_intents_without_importing_hulls() {
    use bfe_sim::{gen::*,store::Ref,seats::lease::{SeatControl,control_key},vehicles::f2_seats::Seats,
        ai::{owned::navigation::Command,nav::{goto::GotoMode,units::ClassDef,math::Vec3 as NavVec3},phase::RandomState,vehicles::world::ClassView}};
    for kind in [5.,6.] {
        let mut store=crate::ai::owned::tests::empty_world();let mut seats=Seats::default();let mut nav=owned_navigation(false);
        let unit=store.unit.insert(Unit {alive:true,generation:3.,..Unit::default()});
        let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:2.,size:[4.,3.,8.],seats:vec![VehicleSeat::default()],..Vehicle::default()});
        let control=Ref::new(1,2);let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:3.,vehicle:Some(hull),controllable:Some(control),..SeatLease::default()});
        store.unit.get_mut(unit).unwrap().seat=Some(lease);store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
        seats.control(control,SeatControl {driven:true,..SeatControl::default()},false);
        let class=nav.class(ClassDef {max_speed:Some(30.),turn_rate:Some(2.),..ClassDef::default()});
        let actor=nav.register_seat(&store,AiLifeHandle {kind:KIND_VEHICLE.into(),slot:hull.slot,generation:hull.gen,life:2.},lease,control,kind,class,
            &ClassView {max_speed:Some(30.),turn_rate:Some(2.),pitch_rate:Some(1.),..ClassView::default()}).unwrap();
        let mut random=RandomState::new(31);let mut env=NavigationEnvironment {now:0.};
        // Following the control must keep a separate weak identity without
        // giving that target a movement helper or reusing the driver's actor.
        let passive=nav.register_seat_target(&store,AiLifeHandle {kind:KIND_VEHICLE.into(),slot:hull.slot,generation:hull.gen,life:2.},lease,control,kind,class,&ClassView::default()).unwrap();
        assert!(nav.control(passive).is_none());assert_ne!(nav.follow_ref(actor),nav.follow_ref(passive));
        let infantry_class=nav.class(ClassDef {max_speed:Some(7.),max_pitch_speed:Some(1.),..Default::default()});
        let infantry=nav.register(&store,AiLifeHandle {kind:KIND_UNIT.into(),slot:unit.slot,generation:unit.gen,life:3.},infantry_class).unwrap();
        // Unit.origin has already switched to the lease, while Core.sync has
        // not yet replaced this old infantry control. The seat eye is y=0.
        crate::vehicles::module::f2::live_seats::admit_test_seats(&mut store.fleet,seats);
        let analogs=nav.analogs(&mut store,&mut env,infantry,Some(NavVec3::new(0.,0.,30.)),1./60.,false).unwrap();
        assert_eq!(analogs[3],0.);
        nav.command(&mut store,&mut random,&mut env,infantry,Command::Follow(nav.follow_ref(passive).unwrap(),0.,false)).unwrap();
        nav.command(&mut store,&mut random,&mut env,actor,Command::Goto(NavVec3::new(0.,0.,100.),GotoMode::Direct)).unwrap();
        {let intent=&mut store.fleet.seats_owner().hulls.controls.get_mut(&control_key(control)).unwrap().intent;intent.buttons=[1.,1.];intent.reload=true;}
        ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
        for tick in 0..120 {
            store.vehicle.get_mut(hull).unwrap().position[2]=tick as f64*0.1;
            nav.command(&mut store,&mut random,&mut env,actor,Command::Tick(1./60.)).unwrap();
            nav.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).unwrap();
            let intent=&store.fleet.seats_owner().hulls.controls.get(&control_key(control)).unwrap().intent;
            assert_eq!(intent.buttons,[1.,1.]);assert!(intent.reload);assert!(intent.forward.is_finite());
        }
        COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert!(nav.dirty().is_empty());
        store.seat_lease.get_mut(lease).unwrap().released=true;
        assert!(nav.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).is_none());
        store.seat_lease.get_mut(lease).unwrap().released=false;store.vehicle.get_mut(hull).unwrap().target_generation=3.;
        assert!(nav.command(&mut store,&mut random,&mut env,actor,Command::Tick(1./60.)).is_none());
    }
}

#[test]
fn controller_service_requests_keep_bounded_commands_and_shared_enhanced_memory_inline() {
    use bfe_sim::ai::control::{controller::{Controller,EnhancedMemory,SourceGoal,goal},world::{ForeignShared,ResponseState}};
    let mut controller=Controller::new(0);let mut memory=EnhancedMemory::new();memory.reserve(80);
    for i in 0..40 {memory.heard_set((i,1),0.);}
    let captured=memory.clone();controller.enhanced=Some(memory);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        controller.commands.latch=false;controller.commands.overwrite(SourceGoal::new(goal::WAIT));
        controller.commands.latch=false;controller.commands.prepend_two(SourceGoal::new(goal::BOARD),SourceGoal::new(goal::FOLLOW));
        let mut request=ForeignShared::read(&controller);request.commands.remove_kind(goal::BOARD);
        request.enhanced.as_mut().unwrap().heard_set((3,1),tick as f64);
        assert_eq!(controller.enhanced.as_ref().unwrap().heard_get((3,1)),Some(tick as f64));
        request.apply(&mut controller);
        let mut response=ResponseState::read(&controller);response.enhanced.as_mut().unwrap().heard_prune(tick as f64,10.);
        response.apply(&mut controller);assert_eq!(controller.commands.queue.len(),2);
        assert_eq!(controller.commands.queue[0].kind,goal::FOLLOW);assert_eq!(controller.commands.queue[1].kind,goal::WAIT);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(captured.heard_get((3,1)),Some(0.),"capture clone is an independent snapshot");
}

#[test]
fn prepared_phase_buffers_retain_nested_enhanced_objective_and_recruitment_scans() {
    use bfe_sim::ai::owned::buffers::Buffers;
    let mut buffers=Buffers::new(160,8,32,40,4);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        let mut candidates=buffers.indices.take();let mut spawns=buffers.indices.take();
        let mut weights=buffers.weights.take();let mut distances=buffers.weights.take();
        for i in 0..8 {candidates.push(i);weights.push(i as f64);distances.push(20.);}
        spawns.push(0);let mut recruits=buffers.recruits.take();
        for i in 0..160 {recruits.push(((159-i) as f64,i));}
        buffers.sort_recruits(&mut recruits);assert_eq!(recruits[0],(0.,159));assert_eq!(recruits[159],(159.,0));
        assert_eq!(candidates.len(),8);assert_eq!(spawns.len(),1);assert_eq!(weights.len(),distances.len());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn enhanced_aim_keys_preserve_missing_ids_and_reaction_draw_order_without_strings() {
    use bfe_sim::ai::{combat::enhanced::{EnhancedAimer,TargetKey,profile},phase::RandomState};
    let mut random=RandomState::new(93);let mut aimer=EnhancedAimer::new(profile(2.),1.);
    assert_eq!(TargetKey::of(Some(-0.),1.),TargetKey::of(Some(0.),1.));
    assert_ne!(TargetKey::of(None,1.),TargetKey::of(Some(0.),1.));
    assert_eq!(TargetKey::parse("undefined:1"),TargetKey::of(None,1.));
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for i in 0..120 {
        let key=TargetKey::of(Some(i as f64),1.);aimer.observe_key(&mut random,Some(key),i as f64);
        assert_eq!(aimer.key,Some(key));assert_eq!(random.draw_counts()[2],(i+1)*6);
        aimer.observe_key(&mut random,Some(key),i as f64+0.1);assert_eq!(random.draw_counts()[2],(i+1)*6);
        aimer.observe_key(&mut random,None,i as f64+0.2);aimer.observe_key(&mut random,Some(key),i as f64+0.3);
        assert_eq!(random.draw_counts()[2],(i+1)*6,"retained target reacquisition makes no draw");
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(random.draw_counts(),[0,0,720]);
}

#[test]
fn native_combat_controllers_renew_lives_without_reallocating_perception_or_alerts() {
    use bfe_sim::ai::{owned::{controllers::Controllers,entities::Entities},combat::{control::Objects,world::{Obj,Ref}},control::{controller::Controller,world::{UKey,AlertSource,CombatInput,EntRef}}};
    struct Ids;impl Objects for Ids {
        fn unit(&self,k:UKey)->Obj {k} fn character(&self,k:usize)->Obj {1000+k as u64} fn post(&self,k:usize)->Obj {2000+k as u64}
        fn unit_key(&self,k:Obj)->UKey {k} fn alert_source(&self,k:Obj)->AlertSource {AlertSource::Unit(k)}
    }
    let mut entities=Entities::new(8);let mut rows=Controllers::new(2,2);let mut key=rows.adopt(0,&mut entities);
    let mut control=Controller::new(0);control.unit=Some(3);control.generation=1.;control.acoustic.push(EntRef {unit:4,generation:1.});
    let first=rows.fresh_perception(&entities,key).unwrap();let heap=first.heap.items.as_ptr();rows.write(&entities,key).unwrap().perception=Some(first);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        control.generation=tick as f64+1.;assert!(rows.feed(&entities,key,&CombatInput::read(&control,Some(0)),&Ids));
        {let p=rows.write(&entities,key).unwrap().perception.as_mut().unwrap();p.heap.insert(3.,Some(Ref {obj:7,generation:1.}));p.heap.dropped=9.;p.last_scan_count=8.;p.candidate_point.x=5.;}
        let p=rows.fresh_perception(&entities,key).unwrap();assert_eq!(p.heap.items.as_ptr(),heap);assert_eq!(p.heap.count,0);assert_eq!(p.heap.dropped,0.);
        assert_eq!(p.last_scan_count,0.);assert_eq!(p.candidate_point.x,0.);rows.write(&entities,key).unwrap().perception=Some(p);
        let alerts=rows.ingest(&Ids);assert_eq!(alerts.len(),1);assert_eq!(alerts[0].acoustic.len(),1);
        assert_eq!(rows.writes(&entities,key,&Ids).unwrap().target,None);
        let old=key;assert!(rows.retire(&mut entities,key));key=rows.adopt(tick+1000,&mut entities);
        assert_ne!(old,key);assert!(rows.read(&entities,old).is_none());
        let p=rows.fresh_perception(&entities,key).unwrap();assert_eq!(p.heap.items.as_ptr(),heap);rows.write(&entities,key).unwrap().perception=Some(p);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_goal_masks_keep_the_source_distinction_from_navigation_integer_masks() {
    use bfe_sim::{gen::{Store,Unit},store::DefId,ai::{owned::{units::Units,entities::{Entities,Entity}},control::{world::ClassSnap,goals::{travel_mask_of,class_travel_mask},classes::health_type},nav::units::SizeType}};
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(8);let mut classes=Units::new();
    for (i,size,expected) in [(0,SizeType::Text("FLYER".into()),32.),(1,SizeType::Number(32.),1.),(2,SizeType::Text(" HOVER ".into()),1.),(3,SizeType::Text("hover".into()),2.)] {
        assert_eq!(travel_mask_of(&size),expected);
        classes.class(DefId(i),ClassSnap {ai_size_type:size.clone(),health_type_name:Some("Vehicle".into()),..ClassSnap::default()},"class".into());
        let r=store.unit.insert(Unit {alive:true,generation:1.,class_def:DefId(i),..Unit::default()});let key=entities.register(Entity::Unit(r));
        ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
        for _ in 0..120 {
            let view=classes.read(&store,&entities,key).unwrap();let class=view.class_def.as_deref().unwrap();
            assert_eq!(class.ai_size_type,size);assert_eq!(class_travel_mask(Some(class)),expected);assert_eq!(health_type(&view),3.);
        }
        COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    }
}

#[test]
fn native_acceptance_callbacks_keep_classic_and_enhanced_policy_boundaries() {
    use bfe_sim::ai::{control::{policy::{self,Input,Kind,Scene},controller::EnhancedMemory,world::{ResponseState,UKey}},nav::math::Vec3};
    struct World {now:f64,kind:f64,capture:bool,fighters:f64,charges:u32,engagements:u32,memory:Option<EnhancedMemory>}
    impl Scene for World {
        fn time(&self)->f64 {self.now} fn kind(&self,_:UKey)->f64 {self.kind}
        fn position(&self,unit:UKey)->Vec3 {if unit==0 {Vec3::default()}else{Vec3::new(0.,0.,4.)}}
        fn identity(&self,unit:UKey)->(u64,u64) {(unit,1)} fn post_contains(&self,_:usize,_:Vec3)->bool {self.capture}
        fn hint_forward(&self,_:usize)->Vec3 {Vec3::new(0.,0.,1.)}
        fn can_engage(&mut self,_:UKey)->bool {self.engagements+=1;true}
        fn board_policy(&mut self,_:UKey)->bool {true}
        fn near_fighting(&mut self,_:UKey)->f64 {self.fighters}
        fn emit_charge(&mut self,_:UKey) {self.charges+=1;}
        fn new_memory(&mut self)->EnhancedMemory {self.memory.take().unwrap()}
    }
    let mut memory=EnhancedMemory::new();memory.reserve(8);
    let mut world=World {now:1.,kind:0.,capture:false,fighters:0.,charges:0,engagements:0,memory:Some(memory)};
    let mut state=ResponseState {interrupts:Default::default(),enhanced:None,secondary_wait_until:10.};
    let input=Input {kind:Kind::Objective,unit:Some(0),controller_unit:Some(0),has_agent:true,state:3.,enhanced:true,..Input::default()};
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    assert!(policy::accepts_acoustic(input,&mut state,&mut world,1));assert_eq!(world.engagements,1);
    world.now=6.;assert!(!policy::accepts_acoustic(input,&mut state,&mut world,1));assert_eq!(world.engagements,1);
    world.now=11.;assert!(policy::accepts_acoustic(input,&mut state,&mut world,1));assert_eq!(world.engagements,2);
    let far=Vec3::new(30.,0.,0.);assert!(!policy::accepts_alert(input,&mut state,&mut world,true,far));
    assert_eq!(state.enhanced.as_ref().unwrap().look_point,far);assert_eq!(state.enhanced.as_ref().unwrap().look_until,13.5);
    world.capture=true;let capturing=Input {helper:1,patrol_moving:true,patrol_post:Some(0),state:2.,..input};
    assert!(!policy::accepts_target(capturing,&mut state,&mut world,1));
    // The combat subclass owns its tracker: a candidate must not replace
    // its own active machine through the base Agent acceptance fallback.
    assert!(!policy::accepts_target(policy::Input {kind:policy::Kind::Combat,..capturing},&mut state,&mut world,1));
    let sniper=Input {helper:2,snipe_full:true,snipe_ready:true,snipe_hint:Some(0),state:4.,enhanced:false,..input};
    assert!(!policy::accepts_alert(sniper,&mut state,&mut world,true,Vec3::new(0.,0.,10.)));
    assert!(policy::accepts_alert(sniper,&mut state,&mut world,false,Vec3::new(0.,0.,10.)));
    assert!(!policy::accepts_target(sniper,&mut state,&mut world,1));assert!(!policy::accepts_board(sniper,&mut world,1));
    world.kind=5.;assert!(!policy::accepts_hazard(Input {kind:Kind::WaitSecondary,..input},&mut state,&mut world));assert_eq!(state.secondary_wait_until,0.);
    world.kind=0.;world.fighters=3.;world.now=12.;let repair=Input {repair:true,enhanced:false,state:2.,..input};
    assert!(!policy::accepts_hazard(repair,&mut state,&mut world));assert_eq!(world.charges,1);
    assert!(!policy::accepts_hazard(repair,&mut state,&mut world));assert_eq!(world.charges,1,"charge query is cached at the same time");
    world.now=13.;world.fighters=0.;assert!(policy::accepts_hazard(repair,&mut state,&mut world));
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_support_keys_and_selection_keep_rounded_identity_and_route_order_without_allocations() {
    use bfe_sim::ai::{vehicles::{support::{SupportHub,Selection,support_candidates_into},support_key::SupportKey},owned::scratch::Pool};
    let navigation=owned_navigation(true);
    let graph=navigation.nav.graph.as_ref().unwrap();
    let hubs=[SupportHub {hub:0,distance:5.},SupportHub {hub:1,distance:20.},SupportHub {hub:0,distance:20.01}];
    let pool=Pool::new(2,graph.hubs.len());
    let parsed=SupportKey::parse("63/1.1");
    let negative_zero=SupportKey::parse("63/-0.0");
    ALLOCATIONS.with(|n|n.set(0));
    COUNTING.with(|n|n.set(true));
    for _ in 0..120 {
        assert_eq!(SupportKey::new(63.,1.06),parsed);
        assert_eq!(SupportKey::new(63.,1.14),parsed);
        assert_eq!(SupportKey::new(63.,-0.01),negative_zero);
        assert_ne!(SupportKey::new(63.,0.),negative_zero);
        assert_eq!(SupportKey::new(-0.,1.),SupportKey::new(0.,1.));
        let selection=Selection::new(graph,&hubs,&three::Vector3::default(),63).unwrap();
        assert_eq!(selection.count,2);
        assert_eq!(selection.at(graph,&hubs,0),hubs[0]);
        assert_eq!(selection.at(graph,&hubs,1),hubs[1]);
        let mut outer=pool.take();
        support_candidates_into(Some(graph),None,&three::Vector3::default(),0.5,63,&mut outer);
        let mut nested=pool.take();
        support_candidates_into(Some(graph),None,&three::Vector3::default(),0.5,63,&mut nested);
        assert_eq!(outer.len(),2);assert_eq!(nested.len(),2);
        assert_eq!(outer[1].hub,nested[1].hub);
    }
    COUNTING.with(|n|n.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_agent_replacement_invalidates_identity_and_retains_callback_tracker_writes() {
    use bfe_sim::ai::{owned::{agents::Agents,entities::Entities},combat::{agent::{CombatAgent,Patterns},perception::TargetTracker}};
    let mut entities=Entities::new(8);
    let mut agents=Agents::new(2);
    let controller=entities.register(bfe_sim::ai::owned::entities::Entity::Controller(bfe_sim::store::Ref::new(0,1)));
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for core in 1000..1120 {
        let key=agents.begin(&mut entities,core,controller);
        assert_eq!(agents.key(core),Some(key));
        let mut combat=CombatAgent {obj:key,controller,state:1.,pending:2.,tracker:TargetTracker::new(controller),patterns:Patterns::default(),point:three::Vector3::default(),hazard:three::Vector3::default(),align_at:0.,looks:0.,look_angle:0.,aligned:false,hazard_until:0.,prone_until:0.,pattern:0.,stationary:false};
        assert!(agents.sync(&entities,&combat));
        agents.get_mut(&entities,key).unwrap().tracker_mut().point=three::Vector3::new(3.,4.,5.);
        combat=agents.combat(&entities,key).unwrap();
        assert_eq!(combat.tracker.point,three::Vector3::new(3.,4.,5.));
        assert!(agents.retire(&mut entities,key));
        assert!(agents.get(&entities,key).is_none());
        let next=agents.begin(&mut entities,core,controller);
        assert_ne!(next,key);assert!(agents.combat(&entities,next).is_none());
        assert!(!agents.retire(&mut entities,key));
        assert_eq!(agents.key(core),Some(next));
        // A recursive transition replaces the controller binding while the
        // old native stack still owns its agent. Resync targets only that row.
        assert!(agents.enter(&entities,next));assert!(agents.enter(&entities,next));
        assert!(agents.retire(&mut entities,next));assert_eq!(agents.key(core),None);
        let replacement=agents.begin(&mut entities,core,controller);assert_ne!(replacement,next);
        assert_eq!(agents.key(core),Some(replacement));
        assert!(agents.get(&entities,next).is_some());
        agents.get_mut(&entities,next).unwrap().tracker_mut().point=three::Vector3::new(9.,8.,7.);
        assert_eq!(agents.get(&entities,replacement).unwrap().tracker().point,three::Vector3::default());
        agents.leave(&mut entities,next);assert!(agents.get(&entities,next).is_some());
        agents.leave(&mut entities,next);assert!(agents.get(&entities,next).is_none());
        assert!(!agents.retire(&mut entities,next));assert_eq!(agents.key(core),Some(replacement));
        assert!(agents.retire(&mut entities,replacement));
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_two_battles_keep_weapon_registration_and_aircraft_run_but_reset_local_queries() {
    use bfe_sim::{ai::{owned::process::Process,combat::weapons_ai::Registered},combat::defs::{Js,JsObj,WeaponClass}};
    use std::rc::Rc;
    let mut process=Process::default();
    let record=Registered {ai:Js::Obj(Rc::new(JsObj {fields:vec![("maxRange".into(),Js::Num(49.))],def:None})),..Registered::default()};
    let class=WeaponClass::new(Rc::new(JsObj {fields:vec![("class".into(),Js::Str("test-cannon".into()))],def:None}));
    let (mut first,mut vehicles)=process.begin();
    first.register(&[("test-cannon".into(),record)]);first.prepare_class(&class);first.begin(8);
    let expected=first.weapons.weapon_ai(&class.levels[0].obj);assert_eq!(expected.max_range,49.);
    vehicles.run.a=Some(71.);vehicles.run.b=Some(112.);
    first.clusters.borrow_mut().state(9,None,1.,true);
    process.end(first,vehicles);
    let (mut second,vehicles)=process.begin();
    assert_eq!(vehicles.run.a,Some(71.));assert_eq!(vehicles.run.b,Some(112.));
    assert!(vehicles.support_cache.is_empty());assert!(vehicles.reservations.records.is_empty());assert!(second.clusters.borrow().states.is_empty());
    // No registration of this class in battle two: the earlier side's record
    // remains visible, including after a different side registers a new name.
    second.register(&[("other-cannon".into(),Registered {ai:Js::Bool(true),..Registered::default()})]);
    second.prepare_class(&class);second.begin(8);
    assert_eq!(second.weapons.weapon_ai(&class.levels[0].obj),expected);
    process.end(second,vehicles);
}

#[test]
fn native_agent_helpers_renew_without_recreating_the_controller_navigation_state() {
    let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    use bfe_sim::{gen::{Store,Unit,UnitBody,Soldier,KIND_UNIT},ai::{owned::navigation::{Command,Actor},nav::{units::ClassDef,goto::GotoMode},phase::RandomState}};
    let mut store=crate::ai::owned::tests::empty_world();
    let r=store.unit.insert(Unit {alive:true,generation:1.,soldier:UnitBody::Soldier(Soldier {max_speed:7.,..Default::default()}),..Default::default()});
    let mut nav=owned_navigation(true);let class=nav.class(ClassDef::default());
    let actor:Actor=nav.register(&store,AiLifeHandle {kind:u32::from(KIND_UNIT),slot:r.slot,generation:r.gen,life:1.},class).unwrap();
    let mut random=RandomState::new(81);let mut env=NavigationEnvironment {now:1.};
    nav.command(&mut store,&mut random,&mut env,actor,Command::Goto(Vec3::new(20.,0.,0.),GotoMode::Direct)).unwrap();
    nav.command(&mut store,&mut random,&mut env,actor,Command::Stop).unwrap();
    nav.control_mut(actor).unwrap().stance=2.;
    let serial=nav.control(actor).unwrap().serial;let controls=nav.nav.controls.len();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        nav.renew_helper(&store,actor).unwrap();assert_eq!(nav.destination(&store,actor),Some(Vec3::default()));
        assert_eq!(nav.control(actor).unwrap().stance,2.);assert_eq!(nav.control(actor).unwrap().serial,serial);assert_eq!(nav.nav.controls.len(),controls);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn running_navigation_helpers_keep_their_identity_and_do_not_reuse_a_retired_control() {
    use bfe_sim::{ai::{owned::navigation::Command,nav::{units::ClassDef,goto::GotoMode},phase::RandomState},gen::{Store,Unit,UnitBody,Soldier,KIND_UNIT}};
    let mut store=crate::ai::owned::tests::empty_world();let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    let unit=store.unit.insert(Unit {alive:true,generation:1.,soldier:UnitBody::Soldier(Soldier {max_speed:7.,..Default::default()}),..Default::default()});
    let target=AiLifeHandle {kind:KIND_UNIT.into(),slot:unit.slot,generation:unit.gen,life:1.};
    let mut nav=owned_navigation(true);let class=nav.class(ClassDef::default());
    let mut random=RandomState::new(81);let mut env=NavigationEnvironment {now:1.};
    // An old combat call and the successor share the controller's control,
    // but each helper owns its destination, route cursor and arrival state.
    for iteration in 0..122 {
        if iteration==2 {ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));}
        let actor=nav.register(&store,target.clone(),class).unwrap();
        let old=nav.helper_key(actor).unwrap();assert!(nav.retain_helper(old));
        nav.command_for(&mut store,&mut random,&mut env,old,Command::Goto(Vec3::new(20.,0.,0.),GotoMode::Direct)).unwrap();
        nav.renew_helper(&store,actor).unwrap();let next=nav.helper_key(actor).unwrap();assert_ne!(old,next);
        nav.command(&mut store,&mut random,&mut env,actor,Command::Goto(Vec3::new(90.,0.,0.),GotoMode::Direct)).unwrap();
        nav.command_for(&mut store,&mut random,&mut env,old,Command::Stop).unwrap();
        assert_eq!(nav.destination_for(&store,old),Some(Vec3::new(20.,0.,0.)));
        assert_eq!(nav.destination(&store,actor),Some(Vec3::new(90.,0.,0.)));
        assert!(nav.helper_observation_for(&store,old).unwrap().arrived);
        let controls=nav.nav.controls.len();let weak=nav.follow_ref(actor).unwrap();
        assert!(nav.retire(actor));assert!(nav.resolve(&store,weak).is_none());
        nav.command_for(&mut store,&mut random,&mut env,old,Command::Goto(Vec3::new(200.,0.,0.),GotoMode::Direct)).unwrap();
        nav.command_for(&mut store,&mut random,&mut env,old,Command::Stop).unwrap();
        assert_eq!(nav.destination_for(&store,old),Some(Vec3::new(20.,0.,0.)));
        let successor=nav.register(&store,target.clone(),class).unwrap();
        assert!(nav.nav.controls.len()>=controls);assert_ne!(actor,successor);
        nav.control_mut(successor).unwrap().stance=2.;
        assert_eq!(nav.control(actor).unwrap().stance,0.,"retired helper cannot alias the next controller");
        nav.release_helper(old);assert!(nav.helper_observation_for(&store,old).is_none());
        assert!(nav.control(actor).is_none());assert!(nav.retire(successor));
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn retained_helpers_keep_stationary_requests_and_direct_state_with_their_controller() {
    use bfe_sim::ai::control::{agents::{Agent,AgentKind,Helper},controller::Controller,helpers::{self,World},world::{AiUnit,HelperKind,HelperObs,ControlObs,EntRef,GotoMode,UKey}};
    struct Facts;
    impl World for Facts {
        fn unit(&self,key:UKey)->AiUnit {AiUnit {key,position:Vec3::new(key as f64,0.,0.),..Default::default()}}
        fn helper_goto(&mut self,_:usize,_:Vec3,_:GotoMode)->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_stop(&mut self,_:usize)->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_terminate(&mut self,_:usize)->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_tick(&mut self,_:usize,_:f64)->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_destination(&mut self,_:usize)->Vec3 {panic!("direct helper uses no navigation service")}
        fn helper_install_path(&mut self,_:usize,_:&[Vec3])->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_follow(&mut self,_:usize,_:EntRef)->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_follow_trail(&mut self,_:usize,_:bool)->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_hold_path(&mut self,_:usize,_:bool)->HelperObs {panic!("direct helper uses no navigation service")}
        fn helper_set_leader(&mut self,_:usize,_:Option<EntRef>)->HelperObs {panic!("direct helper uses no navigation service")}
        fn nav_control_obs(&mut self,_:usize)->ControlObs {panic!("direct helper has no navigation control")}
    }
    let mut controller=Controller::new(0);controller.unit=Some(1);
    let mut successor=Helper {kind:Some(HelperKind::Direct),..Default::default()};successor.direct.target=Vec3::new(99.,0.,0.);
    controller.agent=Some(Agent {fsm:Default::default(),unit:Some(1),goal:None,interrupts:Default::default(),helper:successor,kind:AgentKind::MissingCombat});
    let mut retained=Helper {kind:Some(HelperKind::Direct),..Default::default()};
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for _ in 0..120 {
        let mut view=helpers::Controller {index:0,unit:controller.unit,request:&mut controller.request,control:&mut controller.nav_control,leader:&mut controller.leader};
        helpers::goto(&mut Facts,&mut view,&mut retained,Vec3::new(20.,0.,0.),GotoMode::Route);
        helpers::tick(&mut Facts,&mut view,&mut retained,1./60.);
        assert!(view.request.valid);assert_eq!(view.request.steering_target,Vec3::new(20.,0.,0.));
        view.unit=Some(20);helpers::tick(&mut Facts,&mut view,&mut retained,1./60.);
        assert!(retained.direct.arrived);assert!(!view.request.valid);
        assert_eq!(controller.agent.as_ref().unwrap().helper,successor);
    }
    // StationaryHelper has present no-op follow/tick and no installPath or
    // terminate. A retained NavControl aliases its request but no new
    // navigation service or pose query is introduced by those calls.
    controller.nav_control=Some(Default::default());
    let mut stationary=Helper {kind:Some(HelperKind::Stationary),obs:HelperObs {arrived:true,..Default::default()},..Default::default()};
    for unit in 0..120 {
        controller.unit=Some(unit);controller.request.valid=true;controller.nav_control.as_mut().unwrap().request.valid=true;
        let mut view=helpers::Controller {index:0,unit:controller.unit,request:&mut controller.request,control:&mut controller.nav_control,leader:&mut controller.leader};
        assert_eq!(helpers::destination(&mut Facts,&mut view,&stationary),Some(Vec3::new(unit as f64,0.,0.)));
        helpers::tick(&mut Facts,&mut view,&mut stationary,1./60.);
        helpers::follow(&mut Facts,&mut view,&mut stationary,EntRef {unit:u64::MAX,generation:1.});
        helpers::terminate(&mut Facts,&mut view,&mut stationary);
        assert!(!helpers::install(&mut Facts,&mut view,&mut stationary,&[Vec3::default()]));
        assert!(view.request.valid);assert!(view.control.as_ref().unwrap().request.valid);
        helpers::goto(&mut Facts,&mut view,&mut stationary,Vec3::new(50.,0.,0.),GotoMode::Direct);
        assert!(!view.request.valid);assert!(!view.control.as_ref().unwrap().request.valid);
        view.request.valid=true;view.control.as_mut().unwrap().request.valid=true;
        helpers::stop(&mut Facts,&mut view,&mut stationary);
        assert!(!view.request.valid);assert!(!view.control.as_ref().unwrap().request.valid);
        assert!(stationary.obs.arrived);assert!(!stationary.obs.failed);assert!(!stationary.obs.has_path);
        view.unit=None;assert!(helpers::destination(&mut Facts,&mut view,&stationary).is_none());
        assert_eq!(controller.agent.as_ref().unwrap().helper,successor);
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_controller_navigation_follows_body_changes_without_retargeting_passive_refs() {
    use bfe_sim::{gen::*,store::{DefId,Strings},ai::{owned::{state::{State,Capacity},entities::Entity,navigation::Command},nav::{Nav,PlannerOptions,goto::GotoMode,math::Vec3},phase::RandomState}};
    let mut state=State::new(Nav::empty(PlannerOptions::default()),false,Capacity {registry:4,characters:1,posts:0,hulls:0,buildings:0,controls:0,scopes:2,cameras:1});
    let mut strings=Strings::default();state.unit_class(&AiUnitClass {def:DefId(0),name:strings.intern_str("trooper"),max_speed:AiClassScalar::F64(7.),..Default::default()},&strings);
    let mut store=crate::ai::owned::tests::empty_world();let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    let bodies=[0.,10.].map(|x|store.unit.insert(Unit {alive:true,generation:1.,soldier:UnitBody::Soldier(Soldier {position:[x,0.,0.],max_speed:7.,..Default::default()}),..Default::default()}));
    let objects=bodies.map(|r|state.entities.register(Entity::Unit(r)));
    state.controllers.adopt(0,&mut state.entities);
    let passive=state.body_actor(&store,objects[0],false).unwrap();
    let target=state.navigation.follow_ref(passive).unwrap();
    let actor=state.controller_body(&store,0,objects[0]).unwrap();assert_ne!(passive,actor);
    let mut random=RandomState::new(81);let mut env=NavigationEnvironment {now:1.};
    state.navigation.command(&mut store,&mut random,&mut env,actor,Command::Goto(Vec3::new(20.,0.,0.),GotoMode::Direct)).unwrap();
    let controls=state.navigation.nav.controls.len();let mut serial=state.navigation.control(actor).unwrap().serial;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for turn in 0..120 {
        let at=(turn+1)%2;
        assert_eq!(state.controller_body(&store,0,objects[at]),Some(actor));
        assert_eq!(state.navigation.control(actor).unwrap().serial,serial,"binding defers NavControl.attach until its original observation");
        state.navigation.renew_helper(&store,actor).unwrap();
        state.navigation.command(&mut store,&mut random,&mut env,actor,Command::Goto(Vec3::new(20.,0.,0.),GotoMode::Direct)).unwrap();
        serial+=1.;assert_eq!(state.navigation.control(actor).unwrap().serial,serial);
        assert_eq!(state.navigation.control(actor).unwrap().unit,Some(objects[at]));
        assert_eq!(state.navigation.nav.controls.len(),controls);
        assert_eq!(state.navigation.resolve(&store,target).unwrap().position().x,0.);
        assert_eq!(state.navigation.position(&store,actor).unwrap().x,at as f64*10.);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    store.unit.get_mut(bodies[0]).unwrap().generation=2.;
    assert!(state.navigation.resolve(&store,target).is_none());
}

#[test]
fn native_path_pool_reuses_slots_and_reclaims_large_tails_without_losing_source_ids_or_counts() {
    use bfe_sim::ai::nav::planner::{PathStore,PathNode};
    let mut store=PathStore::new(3.,128.);
    let mut old=None;
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for round in 0..120 {
        let a=store.make();let b=store.make();let c=store.make();
        for path in [a,b,c] {
            for i in 0..100 {assert!(store.append(path,PathNode {x:i as f64,..Default::default()}));}
            store.drop_front(path,99.);assert_eq!(store.paths[path].nodes[0].x,99.);
        }
        assert_eq!(store.pools.nodes.used,3.);assert_eq!(store.pools.paths.used,3.);
        if let Some(old)=old {assert!(store.paths[old].released);assert!(store.paths[old].nodes.is_empty());store.release(old);assert_eq!(store.pools.paths.used,3.);}
        for path in [a,b,c] {store.release(path);}
        assert_eq!(store.pools.nodes.used,0.);assert_eq!(store.pools.paths.used,0.);
        assert_eq!(a,round*3);old=Some(a);
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_turrets_read_f2_aim_and_f4_life_without_refreshing_rigs_or_copying_buildings() {
    use bfe_sim::{gen::*,store::Ref,battle::{props,live_phase::{Live,Building}},seats::turrets::{TurretSide,TurretDesc},vehicles::f2_seats::Seats,
        ai::owned::{turrets::Turrets,entities::{Entities,Entity},controls::HullControls}};
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(64);let mut seats=Seats::default();let controls=HullControls::new();
    let turret=Ref::<Turret>::new(3,2);
    let control=Ref::<SeatUnit>::new(31,2);
    let unit=store.unit.insert(Unit {alive:true,hero:true,generation:1.,..Default::default()});
    let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:1.,turret:Some(turret),controllable:Some(control),team:2.,..Default::default()});
    store.unit.get_mut(unit).unwrap().seat=Some(lease);entities.register(Entity::Lease(lease));
    seats.add_turret(TurretSide {turret,lease:Some(lease),gunner:Some(unit.slot),desc:TurretDesc {facing:0.4,pitch_limits:[-0.3,0.7],yaw_limits:Some([-0.5,0.5]),..Default::default()},..Default::default()});
    let mut live=Live::new(1,0,0);
    let mut settings=props::Settings::from(PbSettings {models:vec![None;3],..Default::default()});settings.kind=props::Kind::Turret;settings.matrix=three::Matrix4::default();
    settings.matrix.elements[12]=20.;
    settings.models[0]=Some(props::Model {bounds:Some([three::Vector3::new(-1.,0.,-1.),three::Vector3::new(1.,4.,1.)]),..Default::default()});
    let owner=live.adopt(1.,Building {state:props::Station {building:props::Building {state:props::State::Built,health:50.,add_health:0.,last_attacker:props::Identity::Null,target_generation:Some(1.)},active_mode:false,active:0.,pulse:0.,served_player:false,served_health:false,served_ammo:false},settings,supply:None,attacker:Attacker::Null});
    let mut turrets=Turrets::new(4,32);
    turrets.register(&seats,&live.buildings,&mut entities,turret,owner.clone(),Some("-30 30".into()),None);
    let member=Member::TurretTarget(Ref::new(7,2));
    let entry=turrets.target(turret,bfe_sim::ai::owned::turrets::Target {member,team:1.,name:"turret".into(),policy:std::rc::Rc::new(Default::default()),combat:Default::default()}).unwrap();
    let mut queries=bfe_sim::ai::owned::registry::Queries::new(12,2);queries.target(member,entry);
    assert_eq!(queries.object(&entities,member),Some(entry));assert_eq!(queries.member(&entities,entry),Some(member));
    // The first turret can reuse a high slot formerly occupied by hull
    // riders. Measure the first bind, before any controls table has grown.
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    assert!(turrets.bind_control(&store,&seats,&live.buildings,&mut entities,control,lease,turret,None,bfe_sim::ai::owned::seats::Occupant::read(&store,lease).unwrap()));
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    seats.control(control,bfe_sim::seats::lease::SeatControl::default(),true);
    seats.crews.controls.get_mut(&bfe_sim::seats::lease::control_key(control)).unwrap().intent.buttons=[1.,0.];
    store.unit.get_mut(unit).unwrap().hero=false;
    assert_eq!(turrets.life(&seats,&live.buildings,turret),Some(1.));
    assert_eq!(turrets.view(&seats,&live.buildings,&entities,turret).unwrap().prepared_limits.unwrap()[1],[-std::f64::consts::FRAC_PI_4,std::f64::consts::FRAC_PI_4]);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        seats.crews.turrets[0].yaw=tick as f64*0.001;
        live.buildings.command(&owner,|b|b.state.building.health=tick as f64);
        let seated=turrets.policy_control(&store,&seats,&live.buildings,&entities,control,&|_|panic!("no Character")).unwrap();
        assert!(seated.hero);assert!(seated.has_turret);assert_eq!(seated.kind,7.);assert_eq!(seated.generation,1.);assert!(seated.nav_radius.is_none());
        assert_eq!(seated.soldier.unwrap().aim_yaw,Some(0.4+tick as f64*0.001));
        let combat=turrets.combat_control(&store,&seats,&live.buildings,&entities,control).unwrap();assert!(combat.hero);assert!(combat.targetable_false);assert!(!combat.pilot);assert!(combat.arsenal.is_none());assert!(combat.body_leases.is_none());
        assert_eq!(turrets.vehicle_control(&store,&seats,&live.buildings,&entities,control).unwrap().soldier_max_speed,Some(0.));
        assert!(turrets.command(&store,&mut seats,&live.buildings,control,bfe_sim::ai::owned::intent::Command::Analogs([0.,0.,0.3,-0.2])));
        let intent=&seats.crews.controls.get(&bfe_sim::seats::lease::control_key(control)).unwrap().intent;
        assert_eq!(intent.turn,0.3);assert_eq!(intent.pitch,-0.2);assert_eq!(intent.buttons,[1.,0.]);
        assert_eq!(turrets.edit_steer(control,|s|*s=None),Some(()));
        let policy=turrets.policy_target(&store,&seats,&live.buildings,turret).unwrap();
        assert_eq!(policy.key,entry);assert_eq!(policy.id,7.);assert_eq!(policy.team,2.);assert_eq!(policy.combat_health,Some(tick as f64));
        let combat=turrets.combat_target(&store,&seats,&live.buildings,&entities,turret).unwrap();assert_eq!(combat.kind,8.);assert_eq!(combat.category,Some(4.));assert_eq!(combat.team,2.);assert!(combat.arsenal.is_none());assert!(combat.body_leases.is_none());
        let view=turrets.view(&seats,&live.buildings,&entities,turret).unwrap();assert!(view.alive);assert!(view.gunner);assert_eq!(view.lease,entities.find(Entity::Lease(lease)));
        assert_eq!(turrets.position(&seats,&live.buildings,turret).unwrap().x,20.);
        let sphere=turrets.sphere(&seats,&live.buildings,turret).unwrap();assert_eq!(sphere.center,three::Vector3::new(20.,2.,0.));
        assert_eq!(turrets.hold_yaw(&seats,&live.buildings,turret,1.),Some(0.5));
        assert_eq!(turrets.forward(&seats,&live.buildings,turret).unwrap(),seats.crews.turrets[0].view_direction(seats.crews.turrets[0].yaw,0.));
        assert_eq!(turrets.lease_count(&seats,&live.buildings,turret),Some(1));
        assert!(!turrets.lease(&store,&seats,&live.buildings,&entities,&controls,turret).unwrap().unwrap().live_false);
        // A scalar observation did not need to instantiate or update any rig.
        assert!(seats.crews.turrets[0].rig.bones.is_empty());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    let next=live.buildings.rebirth(&owner,2.,|b|b.state.building.target_generation=Some(2.)).unwrap();
    assert!(turrets.policy_control(&store,&seats,&live.buildings,&entities,control,&|_|0).is_none());
    assert!(!turrets.command(&store,&mut seats,&live.buildings,control,bfe_sim::ai::owned::intent::Command::Exit));
    assert!(turrets.retire_control(control));assert!(!turrets.retire_control(control));
    assert!(turrets.edit_steer(control,|_|panic!("old control")).is_none());
    assert!(live.buildings.get(&owner).is_none());assert_eq!(turrets.life(&seats,&live.buildings,turret),Some(2.));
    store.seat_lease.get_mut(lease).unwrap().released=true;
    assert!(turrets.lease(&store,&seats,&live.buildings,&entities,&controls,turret).unwrap().unwrap().live_false);
    seats.crews.turrets[0].lease=None;assert_eq!(turrets.policy_target(&store,&seats,&live.buildings,turret).unwrap().team,1.);assert_eq!(turrets.lease(&store,&seats,&live.buildings,&entities,&controls,turret),Some(None));
    assert!(live.buildings.retire(&next));assert!(turrets.view(&seats,&live.buildings,&entities,turret).is_none());
    assert!(turrets.life(&seats,&live.buildings,Ref::new(turret.slot,turret.gen+1)).is_none());
}

#[test]
fn native_service_candidates_keep_registry_fleet_and_wreck_order_without_allocating() {
    use bfe_sim::{gen::*,store::Ref,ai::owned::{services::{Candidates,Geometry},entities::{Entities,Entity}}};
    use bfe_sim::ai::nav::math::Vec3;
    struct Shape {reads:[u64;8],count:usize}
    impl Geometry for Shape {
        fn position(&mut self,u:u64)->Vec3 {self.reads[self.count]=u;self.count+=1;Vec3::new(if self.count==4 {20.}else{0.},0.,0.)}
        fn radius(&mut self,_:u64)->Option<f64> {Some(f64::NAN)}
    }
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(12);let mut candidates=Candidates::new(12,3,2);
    let first=store.unit.insert(Unit {alive:true,..Default::default()});store.sim.store.units.add(|_|Member::Unit(first));
    let body=entities.register(Entity::Unit(first));
    let seated=store.unit.insert(Unit {alive:true,seat:Some(Ref::new(0,1)),..Default::default()});store.sim.store.units.add(|_|Member::Unit(seated));entities.register(Entity::Unit(seated));
    let dead=store.unit.insert(Unit::default());store.sim.store.units.add(|_|Member::Unit(dead));entities.register(Entity::Unit(dead));
    let hull=store.vehicle.insert(Vehicle::default());store.vehicles.vehicles.push(hull);let vehicle=entities.register(Entity::HullEntry(hull));
    let repair=Ref::<Building>::new(0,1);let wrapper=entities.register(Entity::BuildingService(repair));
    candidates.building(RepairTarget::Building(repair),wrapper,false);
    let post=Ref::<Building>::new(1,1);let excluded=entities.register(Entity::BuildingService(post));candidates.building(RepairTarget::Building(post),excluded,true);
    let turret=Ref::<Turret>::new(0,1);let turret_wrapper=entities.register(Entity::TurretService(turret));candidates.building(RepairTarget::Turret(turret),turret_wrapper,false);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        let snapshot=candidates.snapshot(&store,&entities);assert_eq!(&snapshot[..],&[body,vehicle,wrapper,turret_wrapper]);
        let mut shape=Shape {reads:[0;8],count:0};
        let filtered=Candidates::filter(snapshot,Vec3::default(),10.,&mut shape);
        assert_eq!(&filtered[..],&[body,vehicle,wrapper]);assert_eq!(&shape.reads[..shape.count],&[body,vehicle,wrapper,turret_wrapper]);
        assert!(candidates.matches(wrapper,RepairTarget::Building(repair)));assert!(!candidates.matches(wrapper,RepairTarget::Building(post)));
        let nested=candidates.snapshot(&store,&entities);assert_eq!(nested.len(),4);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    entities.retire(wrapper);assert_eq!(&candidates.snapshot(&store,&entities)[..],&[body,vehicle,turret_wrapper]);
}

#[test]
fn native_combat_objects_distinguish_roster_post_and_body_slots_and_reject_old_roster_lives() {
    use bfe_sim::{gen::*,store::Ref,battle::core::Core as BattleCore,ai::{combat::control::Objects as CombatObjects,control::{controller::Controller,world::{CombatInput,AlertSource}},owned::{objects::Objects,entities::{Entities,Entity},roster::Roster,population::Population,controllers::Controllers}}};
    let mut store=crate::ai::owned::tests::empty_world();let mut commands=BattleCore::new(&store,8);let mut roster=Roster::new(8);let mut entities=Entities::new(32);
    let later=store.character.insert_at(5,2,Character::default());let first=store.character.insert_at(0,1,Character::default());
    let later_life=commands.adopt(&mut store,later);roster.add(later_life);roster.add(commands.adopt(&mut store,first));
    let unit=store.unit.insert_at(0,1,Unit {alive:true,generation:1.,..Default::default()});let body=entities.register(Entity::Unit(unit));
    let post=store.post.insert_at(0,1,Post::default());let mobile=store.mobile_post.insert_at(0,1,MobilePost::default());
    let mut population=Population::new(2,2,false);
    for p in [SpawnPost::MobilePost(mobile),SpawnPost::Post(post)] {population.add(&store,AiPostPolicy {post:p,..Default::default()}).unwrap();}
    commands.flush(&store);
    Objects::prepare(&store,&commands,&mut entities,&roster,&population);
    let mut controllers=Controllers::new(2,2);let controller=controllers.adopt(0,&mut entities);
    let mut input=Controller::new(0);input.unit=Some(body);input.generation=1.;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        let objects=Objects {w:&store,store:&store,commands:&commands,entities:&entities,roster:&roster,population:&population};
        let character=objects.character(0);assert_ne!(body,character);assert_ne!(character,controller);
        assert_ne!(objects.post(0),objects.post(1));assert_ne!(objects.post(1),objects.character(1));
        assert_eq!(objects.alert_source(character),AlertSource::Character(0));assert_eq!(objects.alert_source(body),AlertSource::Unit(body));
        assert!(controllers.feed(&entities,controller,&CombatInput::read(&input,Some(1)),&objects));
        let c=controllers.read(&entities,controller).unwrap();assert_eq!(c.character,Some(character));assert_eq!(c.unit,Some(body));assert_eq!(c.objective,Some(objects.post(1)));
        assert_eq!(roster.index(&store,&commands,later),Some(0));assert_eq!(roster.index(&store,&commands,first),Some(1));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(commands.flush(&store),0);
    roster.assign(&mut store,&mut commands,&population,0,Some(1)).unwrap();
    assert_eq!(store.character.get(later).unwrap().objective,PostRef::Post(post));
    assert_eq!(commands.flush(&store),1);assert_eq!(commands.flush(&store),0);
    let next=commands.adopt(&mut store,later);assert_eq!(roster.index(&store,&commands,later),None);assert_eq!(roster.reference(&store,&commands,0),None);
    roster.replace(0,next);assert_eq!(roster.index(&store,&commands,later),Some(0));
    let reused=Ref::new(later.slot,later.gen+1);assert_eq!(roster.index(&store,&commands,reused),None);
}

#[test]
fn native_building_wrappers_read_f4_rebuild_health_and_keep_registry_and_repair_identity_distinct() {
    use bfe_sim::{gen::*,store::Ref,battle::{props,live_phase::{Live,Building as LiveBuilding}},ai::{control::classes,owned::{buildings::{Buildings,Definition},entities::{Entities,Entity}}}};
    fn native(kind:props::Kind)->LiveBuilding {
        let mut settings=props::Settings::from(PbSettings {models:vec![None;3],..Default::default()});settings.kind=kind;settings.max_health=100.;settings.matrix=three::Matrix4::default();settings.matrix.elements[12]=12.;
        LiveBuilding {state:props::Station {building:props::Building {state:props::State::Built,health:75.,add_health:0.,last_attacker:props::Identity::Undefined,target_generation:None},active_mode:false,active:0.,pulse:0.,served_player:false,served_health:false,served_ammo:false},settings,supply:None,attacker:Attacker::Undefined}
    }
    let mut live=Live::new(2,0,0);let mut entities=Entities::new(20);let mut buildings=Buildings::new(2);
    let repair=RepairTarget::Building(Ref::new(1,2));let objective=RepairTarget::Building(Ref::new(0,2));
    let a=live.adopt(1.,native(props::Kind::Building));let b=live.adopt(1.,native(props::Kind::Objective));
    let post=entities.register(Entity::Post(Ref::new(7,1)));
    let definition=|post|Definition {name:Some("building".into()),policy:std::rc::Rc::new(Default::default()),combat:Default::default(),team:2.,armed:false,post};
    let wrapper=buildings.register(&live.buildings,&mut entities,repair,a.clone(),definition(None));
    let objective_wrapper=buildings.register(&live.buildings,&mut entities,objective,b.clone(),definition(Some(post)));
    let entry=buildings.objective(&mut entities,objective,Member::ObjectiveTarget(Ref::new(42,1))).unwrap();
    let RepairTarget::Building(r)=repair else{unreachable!()};let underlying=entities.find(Entity::Building(r)).unwrap();
    assert_ne!(entry,objective_wrapper);assert_ne!(wrapper,underlying);assert!(buildings.matches(&entities,wrapper,underlying));assert!(!buildings.matches(&entities,wrapper,entry));
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        let state=[props::State::Built,props::State::Destroyed,props::State::Unbuilt][tick%3];
        live.buildings.command(&a,|b|{b.state.building.state=state;b.state.building.health=tick as f64/2.;});
        let repair_view=buildings.policy(&live.buildings,None,repair,false).unwrap();assert!(repair_view.id.is_nan());assert_eq!(repair_view.key,wrapper);
        assert_eq!(repair_view.health_type_num,Some([4.,5.,6.][tick%3]));assert_eq!(classes::unit_health(&repair_view),tick as f64/2.);assert!(classes::service_alive(&repair_view));
        let combat=buildings.combat(&live.buildings,None,repair,false).unwrap();assert_eq!(combat.id,None);assert_eq!(combat.category,Some(4.));assert_eq!(combat.max_health,bfe_sim::combat::defs::Js::Num(100.));assert!(combat.combat_max_health.is_undefined());
        let registry=buildings.policy(&live.buildings,None,objective,true).unwrap();assert_eq!(registry.key,entry);assert_eq!(registry.id,42.);assert_eq!(registry.generation,1.);assert_eq!(registry.combat_health,Some(75.));
        assert_eq!(buildings.combat(&live.buildings,None,objective,true).unwrap().building_post,Some(post));
        assert_eq!(buildings.sphere(&live.buildings,None,repair).unwrap().center,three::Vector3::new(12.,1.,0.));assert_eq!(buildings.forward(&live.buildings,None,repair).unwrap(),three::Vector3::new(0.,0.,1.));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    live.buildings.command(&b,|b|b.state.building.state=props::State::Destroyed);
    assert_eq!(buildings.policy(&live.buildings,None,objective,false).unwrap().health_type_num,Some(4.));
    let current=live.buildings.rebirth(&a,2.,|b|{b.state.building.state=props::State::Built;b.state.building.health=100.;}).unwrap();
    assert_eq!(buildings.policy(&live.buildings,None,repair,false).unwrap().key,wrapper);assert_eq!(buildings.policy(&live.buildings,None,repair,false).unwrap().health,Some(100.));
    assert!(live.buildings.retire(&current));assert!(buildings.policy(&live.buildings,None,repair,false).is_none());
}

#[test]
fn generated_class_rows_load_native_control_combat_and_navigation_with_distinct_role_and_mask_rules() {
    use bfe_sim::{gen::*,store::{Strings,DefId},ai::{owned::{content::UnitClass,units::Units,combat::Units as CombatUnits,entities::{Entities,Entity},controls::HullControls,navigation::Navigation},control::goals,nav::{self,units::SizeType},combat::agent::role_of}};
    let mut strings=Strings::default();
    let row=AiUnitClass {def:DefId(7),name:strings.intern_str("trooper"),bounds:Some([-1.,0.,-2.,1.,2.,2.]),
        label:Some(strings.intern_str("body-label")),no_enter_vehicles:AiClassScalar::Str(strings.intern_str("1suffix")),role:AiClassScalar::F64(3.),ai_role:AiClassScalar::F64(5.),unit_type:AiClassScalar::Str(strings.intern_str("scout")),
        health_type:AiClassScalar::Str(strings.intern_str("Droid")),size_type:AiClassScalar::F64(32.),max_speed:AiClassScalar::F64(7.),max_health:AiClassScalar::F64(100.),
        jet_jump:AiClassScalar::Null,jet_fuel_min_border:AiClassScalar::F64(2.),jet_fuel_recharge_rate:AiClassScalar::F64(1.),..Default::default()};
    let mut words=vec![0.;AiUnitClass::WORDS];let mut heap=Vec::new();row.put(&mut words,&mut heap);let decoded=AiUnitClass::take(&words,&heap);assert_eq!(row,decoded);
    let data=UnitClass::read(&decoded,&strings);assert_eq!(data.policy.ai_size_type,SizeType::Number(32.));assert_eq!(data.navigation.jet_fuel_recharge_rate,Some(1.));assert!(data.combat.jet_jump.is_nullish());
    let mut native=Units::new();let mut combat=CombatUnits::new();let mut navigation=Navigation::new(nav::Nav::empty(Default::default()),4);
    let class=data.install(&mut native,&mut combat,&mut navigation);assert_eq!(class,0);
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(8);let r=store.unit.insert(Unit {alive:true,generation:1.,class_def:DefId(7),network_human:Some(true),..Default::default()});let actor=entities.register(Entity::Unit(r));
    let controls=HullControls::new();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        let vehicle=native.vehicle(&store,&entities,actor).unwrap();assert!(vehicle.network_human);assert!(vehicle.soldier);assert_eq!(vehicle.own_label.as_deref(),Some("body-label"));
        assert_eq!(vehicle.class.as_ref().unwrap().no_enter_vehicles,1.);assert_eq!(vehicle.class.as_ref().unwrap().role,3.);
        let control=native.read(&store,&entities,actor).unwrap();let class=control.class_def.as_ref().unwrap();
        assert_eq!(class.prepared_role,Some(3.));assert_eq!(class.prepared_health_type,Some(2.));assert_eq!(goals::class_travel_mask(Some(class)),1.);
        assert_eq!(nav::units::traversal_mask(&control.nav_unit()),32);
        let unit=combat.read(&store,&entities,&controls,actor).unwrap();assert_eq!(role_of(unit.class.as_ref()),5.);assert_eq!(bfe_sim::ai::combat::weapons_ai::category_of(&unit),2.);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_seat_births_capture_occupants_before_binding_and_reuse_generation_qualified_controls() {
    use bfe_sim::{gen::*,ai::owned::{seats::{Controls,Source},controls::HullControls,entities::{Entities,Entity}},seats::hulls::HullSide,vehicles::f2_seats::Seats};
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(8);let mut seats=Seats::default();let mut pool=Controls::new(1);let mut controls=HullControls::with_capacity(1);
    let unit=store.unit.insert(Unit {alive:true,generation:1.,hero:true,..Default::default()});
    let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:2.,seats:vec![VehicleSeat::default()],..Default::default()});entities.register(Entity::Hull(hull));
    seats.add_hull(&store,HullSide {vehicle:hull,..Default::default()});
    let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:1.,vehicle:Some(hull),..Default::default()});store.unit.get_mut(unit).unwrap().seat=Some(lease);store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
    let class=controls.class(5.,Default::default(),Default::default());let name:std::rc::Rc<str>="craft".into();let mut previous:Option<bfe_sim::store::Ref<SeatUnit>>=None;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        store.unit.get_mut(unit).unwrap().hero=true;
        let control=pool.construct(&store,&mut seats,&mut entities,lease,None).unwrap();
        if let Some(old)=previous {assert_eq!(old.slot,control.slot);assert_ne!(old.gen,control.gen);assert!(pool.read(old).is_none());}
        let birth=pool.read(control).unwrap();assert_eq!(birth.source,Source::Hull(hull,2.));assert_eq!(birth.index,0);
        store.seat_lease.get_mut(lease).unwrap().controllable=Some(control);store.unit.get_mut(unit).unwrap().hero=false;
        assert!(controls.register(&store,&mut entities,control,lease,AiLifeHandle {kind:u32::from(KIND_VEHICLE),slot:hull.slot,generation:hull.gen,life:2.},5.,Default::default(),name.clone(),None,birth.occupant));
        assert!(controls.set_class(control,class));
        assert!(controls.policy_unit(&store,&entities,control,&|_|0).unwrap().hero);assert!(controls.combat_unit(&store,&entities,control).unwrap().hero);
        let object=entities.find(Entity::Control(control)).unwrap();assert!(controls.retire(control));assert!(pool.retire(&mut seats,&mut entities,control));assert!(entities.get(object).is_none());
        assert!(seats.hulls.controls.len()==0);assert!(seats.crews.controls.len()==0);assert!(!pool.retire(&mut seats,&mut entities,control));previous=Some(control);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn mounted_arsenal_preserves_channels_tuning_and_weapon_clocks() {
    use bfe_sim::{ai::owned::{arsenals::{Owner,Mounted},seats::Source,weapons::{self,Definitions}},
        ai::combat::world::WeaponSettings,gen::*,store::{Ref,DefId},units::ownership::Ownership,
        combat::defs::{WeaponDefs,WeaponClass,JsObj,Js}};
    use std::rc::Rc;
    struct Mount {source:Source,weapons:Vec<Weapon>,changed:usize}
    impl Mounted for Mount {
        fn count(&self,_:&crate::world::owner::World,s:Source,seat:usize)->Option<usize> {(s==self.source&&seat==1).then_some(self.weapons.len())}
        fn read<R>(&self,store:&crate::world::owner::World,s:Source,seat:usize,index:usize,read:impl FnOnce(&Weapon)->R)->Option<R> {
            self.count(store,s,seat)?;Some(read(self.weapons.get(index)?))
        }
        fn edit<R>(&mut self,store:&mut crate::world::owner::World,s:Source,seat:usize,index:usize,edit:impl FnOnce(&mut WeaponSettings)->R)->Option<R> {
            self.count(store,s,seat)?;let (r,changed)=weapons::edit_state(self.weapons.get_mut(index)?,edit);
            self.changed+=usize::from(changed);Some(r)
        }
    }
    let mut defs=WeaponDefs::default();let mut prepared=Definitions::new();
    for (id,channel) in [(0,Js::Undefined),(1,Js::Null),(2,Js::Num(9.))] {
        let class=WeaponClass::new(Rc::new(JsObj {fields:vec![("channel".into(),channel)],def:None}));
        prepared.register(DefId(id),&class);defs.set(DefId(id),class);
    }
    let mut store=crate::ai::owned::tests::empty_world();let mut ownership=Ownership::default();
    for source in [Source::Hull(Ref::new(3,2),4.),Source::Turret(Ref::new(5,6),7.)] {
        let owner=Owner::Mount(source,1);
        let mut mount=Mount {source,weapons:(0..3).map(|i|Weapon::create(DefId(i),&defs,false)).collect(),changed:0};
        mount.weapons[0].charging=true;mount.weapons[0].held=2.;mount.weapons[0].reload_left=3.;
        assert_eq!(owner.current(&store,&mount,&prepared,0.),Some(0));
        assert_eq!(owner.current(&store,&mount,&prepared,1.),Some(1));
        for channel in [2.,9.,-1.,0.5,f64::NAN] {assert_eq!(owner.current(&store,&mount,&prepared,channel),None);}
        for (index,channel) in [(0,0.),(1,1.),(2,9.)] {assert_eq!(owner.view(&store,&mount,&prepared,index).unwrap().channel,Js::Num(channel));}
        assert!(owner.select(&mut store,&mut ownership,&mount,&defs,2));
        assert!(!owner.select(&mut store,&mut ownership,&mount,&defs,3));
        assert!(mount.weapons[0].charging);assert_eq!(mount.weapons[0].held,2.);assert_eq!(mount.weapons[0].reload_left,3.);
        owner.edit(&mut store,&mut ownership,&mut mount,0,|w|w.spread_scale=0.5).unwrap();
        assert_eq!(mount.changed,1);assert_eq!(owner.settings(&store,&mount,0).unwrap().spread_scale,0.5);
        ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
        for _ in 0..120 {
            owner.edit(&mut store,&mut ownership,&mut mount,0,|w|w.spread_scale=0.5).unwrap();
            assert!(owner.select(&mut store,&mut ownership,&mount,&defs,0));
            assert!(owner.view(&store,&mount,&prepared,0).is_some());
        }
        COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(mount.changed,1);
        assert!(Owner::Mount(source,0).view(&store,&mount,&prepared,0).is_none());
    }
    assert!(ownership.changed().is_empty());
}

#[test]
fn native_sight_keeps_collision_body_exclusions_and_registry_observation_order() {
    use bfe_sim::{ai::owned::{sight::{Sight,World},entities::{Entities,Entity},registry::Queries,seats::Controls},gen::*,store::Ref};
    use three::Vector3;
    struct Occlusion {hit:Option<Option<Ref<Body>>>,unit:bool,casts:usize,rays:usize,ignored:Option<Ref<Body>>,excluded:Vec<Member>,axis:Vector3,span:f64}
    impl World for Occlusion {
        fn cast(&mut self,_:&mut crate::world::owner::World,from:&Vector3,d:&Vector3,span:f64,ignore:Option<Ref<Body>>)->Option<Option<Ref<Body>>> {
            assert_eq!(*from,Vector3::default());assert_eq!(*d,self.axis);assert_eq!(span,self.span);
            self.casts+=1;self.ignored=ignore;self.hit
        }
        fn raycast(&mut self,_:&mut crate::world::owner::World,_:&Vector3,_:&Vector3,_:f64,exclude:&[Member])->bool {
            assert!(self.casts>self.rays);self.rays+=1;self.excluded.clear();self.excluded.extend_from_slice(exclude);self.unit
        }
    }
    let mut store=crate::ai::owned::tests::empty_world();let mut entities=Entities::new(16);let controls=Controls::new(4);let queries=Queries::new(16,2);
    let hull=store.vehicle.insert(Vehicle {body:Ref::new(21,3),..Vehicle::default()});
    let lease=store.seat_lease.insert(SeatLease {vehicle:Some(hull),..SeatLease::default()});
    let rider=store.unit.insert(Unit {seat:Some(lease),..Unit::default()});let rider_obj=entities.register(Entity::Unit(rider));
    let target_ref=Ref::new(8,2);let target=entities.register(Entity::BuildingService(target_ref));
    let authored=entities.register(Entity::Building(target_ref));
    let mut sight=Sight::new(16);sight.prepare(authored,Ref::new(22,4));
    let from=Vector3::default();let to=Vector3::new(0.,0.,10.);let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    let mut world=Occlusion {hit:None,unit:false,casts:0,rays:0,ignored:None,excluded:Vec::with_capacity(2),axis:Vector3::new(0.,0.,1.),span:10.};
    assert!(sight.los(&mut store,&entities,&controls,&queries,&mut world,&from,&from,Some(rider_obj),Some(target)));
    assert_eq!(world.casts,0);assert_eq!(world.rays,0);
    world.hit=Some(None);
    assert!(!sight.los(&mut store,&entities,&controls,&queries,&mut world,&from,&to,Some(rider_obj),Some(target)));
    assert_eq!(world.ignored,Some(Ref::new(21,3)));assert_eq!(world.rays,0);
    world.hit=Some(Some(Ref::new(22,4)));
    assert!(sight.los(&mut store,&entities,&controls,&queries,&mut world,&from,&to,Some(rider_obj),Some(target)));
    assert_eq!(world.excluded,vec![Member::Unit(rider)]);
    world.unit=true;
    assert!(!sight.los(&mut store,&entities,&controls,&queries,&mut world,&from,&to,Some(rider_obj),Some(target)));
    world.unit=false;world.hit=None;
    store.unit.get_mut(rider).unwrap().seat=None;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {assert!(sight.los(&mut store,&entities,&controls,&queries,&mut world,&from,&to,Some(rider_obj),Some(target)));}
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(world.ignored,None);
    entities.retire(authored);
    let replacement=entities.register(Entity::Building(Ref::new(8,3)));
    assert_ne!(replacement,authored);assert_eq!(sight.body(&store,&entities,&controls,target),None);
    world.axis=Vector3::new(0.,1.,0.);world.span=40.;
    store.unit.get_mut(rider).unwrap().seat=Some(lease);
    let handle=AiLifeHandle {kind:KIND_UNIT.into(),slot:rider.slot,generation:rider.gen,life:0.};
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        assert!(bfe_sim::ai::owned::sight::clear_above(&mut store,&mut world,&from,&handle));
        assert_eq!(world.ignored,Some(Ref::new(21,3)));assert_eq!(&world.excluded,&[Member::Unit(rider)]);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    let rays=world.rays;world.hit=Some(None);
    assert!(!bfe_sim::ai::owned::sight::clear_above(&mut store,&mut world,&from,&handle));assert_eq!(world.rays,rays);
    let casts=world.casts;store.unit.get_mut(rider).unwrap().generation=1.;
    assert!(!bfe_sim::ai::owned::sight::clear_above(&mut store,&mut world,&from,&handle));assert_eq!(world.casts,casts);
}

#[test]
fn native_navigation_reuses_physical_controls_without_reusing_actor_identity() {
    let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    use bfe_sim::{ai::{owned::navigation::Command,nav::{goto::GotoMode,units::ClassDef},phase::RandomState},gen::*};
    let mut store=crate::ai::owned::tests::empty_world();let r=store.unit.insert(Unit {alive:true,generation:1.,..Unit::default()});
    let mut navigation=owned_navigation(false);let class=navigation.class(ClassDef::default());
    let mut random=RandomState::new(37);let mut env=NavigationEnvironment{now:0.};
    let key=|life|AiLifeHandle {kind:KIND_UNIT.into(),slot:r.slot,generation:r.gen,life};
    let first=navigation.register(&store,key(1.),class).unwrap();let old=navigation.follow_ref(first).unwrap();
    assert!(navigation.retire(first));
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for life in 2..122 {
        store.unit.get_mut(r).unwrap().generation=life as f64;
        let actor=navigation.register(&store,key(life as f64),class).unwrap();
        assert_ne!(actor,first);assert!(navigation.resolve(&store,old).is_none());
        assert!(!navigation.control(actor).unwrap().disposed);
        navigation.command(&mut store,&mut random,&mut env,actor,Command::Goto(bfe_sim::ai::nav::math::Vec3::new(10.,0.,0.),GotoMode::Direct)).unwrap();
        assert!(navigation.retire(actor));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(navigation.nav.controls.len(),1);
    assert!(navigation.nav.queue.is_empty());assert!(navigation.nav.current.is_none());
}

#[test]
fn shared_native_state_uses_canonical_controller_links_and_prepares_empty_hint_services() {
    use bfe_sim::{ai::{owned::{state::{State,Capacity},entities::Entity},nav::{Nav,PlannerOptions},control::world::ClassSnap},
        gen::{Store,Unit},store::DefId,units::ownership::Ownership,battle::{commands::Commands,live_phase::Live},vehicles::f2_seats::Seats};
    let mut state=State::new(Nav::empty(PlannerOptions::default()),false,Capacity {registry:8,characters:2,posts:2,hulls:2,buildings:2,controls:4,scopes:2,cameras:1});
    state.prepare_hints(|_|None);
    let mut store=crate::ai::owned::tests::empty_world();let mut owner=Ownership::default();let r=store.unit.insert(Unit {alive:true,generation:1.,..Unit::default()});
    owner.adopt(&store,r).unwrap();owner.clear_changed();
    let obj=state.entities.register(Entity::Unit(r));state.units.class(DefId(0),ClassSnap::default(),"test".into());
    state.combat_units.class_view(DefId(0),Default::default());
    assert!(state.link_controller(&mut store,&mut owner,obj,0,true));let controller=state.controllers.key(0).unwrap();
    assert_eq!(owner.changed(),&[r]);owner.clear_changed();
    let mut seats=Seats::default();let live=Live::new(0,0,0);let commands=Commands::new(0);
    assert_eq!(state.scene(&store,&seats,&live.buildings,&commands).combat(obj).unwrap().controller,Some(controller));
    // Scheduler scalar observations must see the current row immediately,
    // without refreshing lazy bounds or copying unrelated intent/class data.
    for life in [1.,2.,3.] {
        let unit=store.unit.get_mut(r).unwrap();unit.generation=life;unit.team=life+1.;unit.alive=life!=2.;
        let spheres_tick=unit.spheres_tick;
        let scene=state.scene(&store,&seats,&live.buildings,&commands);
        assert_eq!(scene.scalars(obj),bfe_sim::ai::control::world::UnitScalars::of(&scene.policy(obj).unwrap()));
        assert_eq!(scene.motion(obj),bfe_sim::ai::control::world::UnitMotion::of(&scene.policy(obj).unwrap()));
        assert_eq!(scene.combat_scalars(obj),bfe_sim::ai::combat::world::UnitScalars::of(&scene.combat(obj).unwrap()));
        assert_eq!(store.unit.get(r).unwrap().spheres_tick,spheres_tick);
    }
    {let unit=store.unit.get_mut(r).unwrap();unit.generation=1.;unit.team=0.;unit.alive=true;}
    store.unit.get_mut(r).unwrap().intent.reload=true;
    assert!(state.intent(&mut store,&mut seats,&live.buildings,obj,bfe_sim::ai::owned::intent::Command::Buttons([1.,2.])));
    assert!(state.intent(&mut store,&mut seats,&live.buildings,obj,bfe_sim::ai::owned::intent::Command::Aim(Some([1.,2.,3.]))));
    assert_eq!(store.unit.get(r).unwrap().intent.buttons,[1.,2.]);assert_eq!(store.unit.get(r).unwrap().intent.aim,Some([1.,2.,3.]));assert!(store.unit.get(r).unwrap().intent.reload);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        assert!(state.link_controller(&mut store,&mut owner,obj,0,true));
        assert!(state.hints.as_ref().unwrap().cover(&state.navigation.nav,Vec3::default(),10.,0.,|_|unreachable!()).is_empty());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert!(owner.changed().is_empty());
    assert!(state.link_controller(&mut store,&mut owner,obj,1,false));
    assert!(store.unit.get(r).unwrap().controller.is_some());
    assert!(state.link_controller(&mut store,&mut owner,obj,0,false));assert_eq!(store.unit.get(r).unwrap().controller,None);
    assert_eq!(owner.changed(),&[r]);
    let hint=state.entities.register(Entity::Hint(7));assert_ne!(hint,obj);state.entities.retire(hint);
    assert_ne!(state.entities.register(Entity::Hint(7)),hint);assert_eq!(state.entities.get(hint),None);
}

#[test]
fn native_jet_jobs_keep_random_ties_authored_vectors_and_current_launch_position() {
    let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    use bfe_sim::{ai::{owned::{hints::Hints,jets::Jets,navigation::{Navigation,Command}},nav::{hints::{build_hints,HintRecord,HintSet},units::ClassDef,jet_hints::find_hint},phase::RandomState,random::ClassicRandom},gen::{Store,Unit,Soldier,UnitBody,KIND_UNIT}};
    let record=HintRecord {kind:Some("jetjump".into()),name:None,xfrm:[1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.],properties:vec![]};
    let mut raw=owned_navigation(false).nav;raw.hints=HintSet::new(build_hints(&[record.clone(),record.clone(),record],None,350.));
    let hints=Hints::new(&raw,2,|_|None,|i|i as u64+40);
    let mut nav=Navigation::new(raw,2);let mut store=crate::ai::owned::tests::empty_world();
    let r=store.unit.insert(Unit {alive:true,generation:1.,soldier:UnitBody::Soldier(Soldier {max_speed:7.,..Default::default()}),..Default::default()});
    let class=nav.class(ClassDef::default());let actor=nav.register(&store,AiLifeHandle {kind:KIND_UNIT.into(),slot:r.slot,generation:r.gen,life:1.},class).unwrap();
    let mut jets=Jets::new(4);let mut random=RandomState::new(47);let mut expected=ClassicRandom::from_state(47);
    let candidates=[(0,Vec3::default()),(1,Vec3::default()),(2,Vec3::default())];
    let want=find_hint(&candidates,Vec3::default(),&mut expected);
    assert_eq!(jets.find(&nav,&hints,3,Vec3::default(),1.,&mut random),want);
    assert_eq!(random.draw_counts(),[2,0,0]);assert_eq!(random.seeds()[0],expected.seed());
    let mut env=NavigationEnvironment {now:1.};
    for forward in [Vec3::new(2.,0.,0.),Vec3::default()] {
        nav.nav.hints.list[0].forward=forward;
        jets.enter(1,Some(actor),Some(0),&mut nav,&mut store,&mut random,&mut env);
        for _ in 0..8 {
            nav.nav.service(1.);
            nav.command(&mut store,&mut random,&mut env,actor,Command::Tick(1./60.)).unwrap();
            nav.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).unwrap();
            if nav.helper_observation(&store,actor).unwrap().arrived {break;}
        }
        assert!(nav.helper_observation(&store,actor).unwrap().arrived);
        let job_helper=nav.helper_key(actor).unwrap();
        nav.renew_helper(&store,actor).unwrap();
        let successor_destination=nav.destination(&store,actor).unwrap();
        let UnitBody::Soldier(body)=&mut store.unit.get_mut(r).unwrap().soldier else {unreachable!()};body.position=[3.,0.,4.];
        assert!(!jets.tick(1,&mut nav,&mut store,&mut random,&mut env));
        assert_eq!(nav.destination_for(&store,job_helper),Some(Vec3::new(3.+forward.x*80.,0.,4.+forward.z*80.)));
        assert_eq!(nav.destination(&store,actor),Some(successor_destination));
        assert_eq!(nav.control(actor).unwrap().jet.vector,forward);assert!(nav.control(actor).unwrap().jet.pending);
        env.now+=1.;assert!(!jets.tick(1,&mut nav,&mut store,&mut random,&mut env));
        env.now+=0.01;assert!(jets.tick(1,&mut nav,&mut store,&mut random,&mut env));
        jets.exit(1,&mut nav);assert!(jets.tick(1,&mut nav,&mut store,&mut random,&mut env));
        let UnitBody::Soldier(body)=&mut store.unit.get_mut(r).unwrap().soldier else {unreachable!()};body.position=[0.,0.,0.];
    }
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {assert!(jets.find(&nav,&hints,3,Vec3::default(),env.now,&mut random).is_some());jets.enter(1,None,Some(0),&mut nav,&mut store,&mut random,&mut env);jets.exit(1,&mut nav);}
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn native_combat_row_scope_reads_and_writes_one_canonical_body() {
    use bfe_sim::{ai::{owned::{state::{State,Capacity},entities::Entity,arsenals::Mounted,rows::Rows},nav::{Nav,PlannerOptions},combat::world::WeaponSettings},
        gen::{Store,Unit,Weapon},store::DefId,units::ownership::Ownership,battle::{commands::Commands,live_phase::Live},vehicles::f2_seats::Seats,combat::defs::{WeaponDefs,WeaponClass,JsObj,Js}};
    struct NoMount;
    impl Mounted for NoMount {
        fn count(&self,_:&crate::world::owner::World,_:bfe_sim::ai::owned::seats::Source,_:usize)->Option<usize> {Some(0)}
        fn read<R>(&self,_:&crate::world::owner::World,_:bfe_sim::ai::owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&Weapon)->R)->Option<R> {panic!("body arsenal must stay with F3")}
        fn edit<R>(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::ai::owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&mut WeaponSettings)->R)->Option<R> {panic!("body arsenal must stay with F3")}
    }
    let mut state=State::new(Nav::empty(PlannerOptions::default()),false,Capacity {registry:8,characters:2,posts:2,hulls:2,buildings:2,controls:4,scopes:2,cameras:1});
    let mut defs=WeaponDefs::default();let class=WeaponClass::new(std::rc::Rc::new(JsObj {fields:vec![("channel".into(),Js::Num(0.)),("base".into(),Js::Str("towcableweapon".into()))],def:None}));
    state.weapons.register(DefId(0),&class);defs.set(DefId(0),class);
    let mut strings=bfe_sim::store::Strings::default();
    let body_class=bfe_sim::gen::AiUnitClass {def:DefId(0),name:strings.intern_str("test"),..Default::default()};
    state.unit_class(&body_class,&strings);
    let mut store=crate::ai::owned::tests::empty_world();let r=store.unit.insert(Unit {alive:true,generation:1.,..Default::default()});
    store.unit.get_mut(r).unwrap().combat.weapons.push(Weapon::create(DefId(0),&defs,false));
    let obj=state.entities.register(Entity::Unit(r));let mut changes=Ownership::default();changes.adopt(&store,r).unwrap();changes.clear_changed();
    let mut seats=Seats::default();let live=Live::new(0,0,0);let mut commands=Commands::new(0);let mut mounted=NoMount;
    store.sim.strings=std::mem::take(&mut strings);
    crate::vehicles::module::f2::live_seats::admit_test_seats(&mut store.fleet,std::mem::take(&mut seats));
  let mut rows=Rows {w:&mut store,buildings:&live.buildings,characters:&mut commands,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
    assert!(rows.unit(obj).alive);assert!(rows.has_arsenal(obj));assert_eq!(rows.weapon_count(obj),1);
    assert_eq!(rows.unit_life(obj),(rows.unit(obj).alive,rows.unit(obj).generation));
    assert_eq!(rows.unit_priority(obj),bfe_sim::ai::combat::world::PriorityView::of(&rows.unit(obj)));
    assert_eq!(rows.unit_target_status(obj),bfe_sim::ai::combat::world::TargetStatus::of(&rows.unit(obj)));
    assert_eq!(rows.unit_has_origin(obj),rows.unit(obj).has_origin);
    for tick in 0..3 {
        let bfe_sim::gen::UnitBody::Soldier(body)=&mut rows.w.sim.store.unit.get_mut(r).unwrap().soldier else {unreachable!()};
        body.velocity=[tick as f64,-0.0,3.];
        assert_eq!(rows.unit_velocity(obj),rows.unit(obj).velocity);
        assert_eq!(rows.unit_controller(obj),rows.unit(obj).controller);
    }
    rows.w.sim.store.unit.get_mut(r).unwrap().class_def=DefId(999);
    assert_eq!(rows.unit_velocity(obj),None);assert_eq!(rows.unit_controller(obj),None);
    assert_eq!(rows.unit_life(obj),(false,0.));assert!(!rows.unit_has_origin(obj));
    assert_eq!(rows.unit_target_status(obj),Default::default());
    rows.w.sim.store.unit.get_mut(r).unwrap().class_def=DefId(0);
    assert_eq!(rows.unit_velocity(u64::MAX),None);assert_eq!(rows.unit_controller(u64::MAX),None);
    let hull=rows.w.sim.store.vehicle.insert(Default::default());
    let raw_hull=rows.state.entities.register(Entity::Hull(hull));
    let hull_target=rows.state.entities.register(Entity::HullEntry(hull));
    rows.w.sim.store.unit.get_mut(r).unwrap().combat.repair_target=Some(bfe_sim::gen::RepairTarget::Vehicle(hull));
    {
        let row=rows.w.sim.store.vehicle.get_mut(hull).unwrap();row.alive=true;row.target_generation=1.;
        row.seats.push(Default::default());
    }
    let lease=rows.w.sim.store.seat_lease.insert(bfe_sim::gen::SeatLease {unit:Some(r),generation:1.,vehicle:Some(hull),..Default::default()});
    rows.w.sim.store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
    rows.w.sim.store.unit.get_mut(r).unwrap().seat=Some(lease);
    let control=rows.state.births.construct(&rows.w.sim.store,rows.w.fleet.seats_owner(),&mut rows.state.entities,lease,None).unwrap();
    rows.w.sim.store.seat_lease.get_mut(lease).unwrap().controllable=Some(control);
    let seat_class=bfe_sim::gen::AiUnitClass {name:rows.w.sim.strings.intern_str("seat"),..Default::default()};
    let class=rows.state.control_class(&seat_class,&rows.w.sim.strings,6.);
    assert!(rows.state.controls.register(&rows.w.sim.store,&mut rows.state.entities,control,lease,bfe_sim::gen::AiLifeHandle {kind:bfe_sim::gen::KIND_VEHICLE.into(),slot:hull.slot,generation:hull.gen,life:1.},6.,Default::default(),"craft".into(),None,bfe_sim::ai::owned::seats::Occupant {id:0.,hero:false}));
    assert!(rows.state.controls.set_class(control,class));
    let control_obj=rows.state.entities.find(Entity::Control(control)).unwrap();
    let seat_key=bfe_sim::seats::lease::control_key(control);
    rows.w.fleet.seats_owner().hulls.controls.get_mut(&seat_key).unwrap().intent.buttons=[4.,5.];
    rows.w.fleet.seats_owner().hulls.controls.get_mut(&seat_key).unwrap().intent.reload=true;
    rows.w.fleet.seats_owner().add_hull(&rows.w.sim.store,bfe_sim::seats::hulls::HullSide {vehicle:hull,..Default::default()});
    let mut hull_definition=bfe_sim::gen::AiHullDefinition {
        hull,member:Some(bfe_sim::gen::Member::VehicleTarget(bfe_sim::store::Ref::new(0,1))),kind:6.,category:3.,forward_speed:f64::NAN,
        policy:bfe_sim::gen::AiUnitClass {name:rows.w.sim.strings.intern_str("craft"),vehicle_type:bfe_sim::gen::AiClassScalar::Str(rows.w.sim.strings.intern_str("transport")),..Default::default()},
        seats:vec![bfe_sim::gen::AiSeatDefinition {policy:seat_class.clone(),kind:6.,weapons:1,..Default::default()}],..Default::default()
    };
    // A rejected batch leaves both registry and existing construction intact.
    hull_definition.hull=bfe_sim::store::Ref::new(hull.slot,hull.gen+1);
    assert!(!rows.prepare_members(std::slice::from_ref(&hull_definition),&[]));
    assert!(rows.state.queries.object(&rows.state.entities,hull_definition.member.unwrap()).is_none());
    hull_definition.hull=hull;
    assert!(rows.prepare_members(std::slice::from_ref(&hull_definition),&[]));
    assert_eq!(rows.state.queries.object(&rows.state.entities,hull_definition.member.unwrap()),Some(hull_target));
    assert_eq!(rows.scene().vehicle(hull_target).unwrap().class.unwrap().vehicle_type.as_deref(),Some("transport"));
    let variant=rows.w.sim.store.vehicle.insert(bfe_sim::gen::Vehicle {seats:vec![Default::default()],..Default::default()});
    let mut pending=hull_definition.clone();pending.hull=variant;pending.member=None;
    assert!(rows.prepare_members(std::slice::from_ref(&pending),&[]));
    let future=bfe_sim::gen::Member::VehicleTarget(bfe_sim::store::Ref::new(7,1));
    assert!(rows.state.queries.object(&rows.state.entities,future).is_none());
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    assert!(rows.state.mounts.activate_target(&rows.state.entities,bfe_sim::store::Ref::new(variant.slot,variant.gen+1),future).is_none());
    let entry=rows.state.mounts.activate_target(&rows.state.entities,variant,future).unwrap();
    rows.state.queries.target(future,entry);
    assert_eq!(rows.scene().vehicle(entry).unwrap().class.unwrap().vehicle_type.as_deref(),Some("transport"));
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    let controller=rows.state.controllers.adopt(0,&mut rows.state.entities);
    rows.state.controllers.write(&rows.state.entities,controller).unwrap().unit=Some(control_obj);
    rows.state.controls.link_controller(control,controller,true).unwrap();
    let body_actor=rows.state.controller_body(&rows.w.sim.store,0,obj).unwrap();
    let control_count=rows.state.navigation.nav.controls.len();
    assert_eq!(rows.state.controller_hull(&rows.w.sim.store,0,control_obj),Some(body_actor));
    assert_eq!(rows.state.navigation.nav.controls.len(),control_count);
    let agent=rows.state.agents.begin(&mut rows.state.entities,0,controller);
    let mut helper=bfe_sim::ai::control::agents::Helper::default();helper.kind=Some(bfe_sim::ai::control::world::HelperKind::Ground);
    rows.state.controllers.helper_created(0,helper);
    let mut vehicles=bfe_sim::ai::owned::vehicles::Authority::new(bfe_sim::ai::vehicles::VehicleAI::new());
    bfe_sim::ai::vehicles::service::VehicleStorage::write(&mut vehicles,|v|v.prepare_reservations(8,2));
    let vehicle_settings=bfe_sim::ai::vehicles::world::Settings::default();
    let combat_settings=bfe_sim::ai::owned::frame::Settings {difficulty:std::cell::RefCell::new(bfe_sim::ai::control::difficulty::Difficulty::classic(Default::default())),mission_stay_in_turrets:false,mission_no_jet_obstacle_test:false,ai_stay_in_turrets:false,vehicles:true,dense:false,navigation:true};
    let mut combat=bfe_sim::ai::combat::binding::CombatAi::default();let mut random=bfe_sim::ai::phase::RandomState::new(3);
    let mut hooks=canonical_combat_fixture::Callbacks::default();
    rows.w.sim.store.unit.get_mut(r).unwrap().intent.reload=true;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for step in 0..120 {
        assert_eq!(rows.state.controller_body(&rows.w.sim.store,0,obj),Some(body_actor));
        assert_eq!(rows.state.controller_hull(&rows.w.sim.store,0,control_obj),Some(body_actor));
        assert!(rows.state.navigation.renew_helper(&rows.w.sim.store,body_actor).is_some());
        assert_eq!(rows.state.navigation.nav.controls.len(),control_count);
        {
            use bfe_sim::ai::vehicles::{World,service::VehicleServices,notify::VehicleNotify};
            let mut frame=bfe_sim::ai::owned::vehicle_frame::Frame {rows:&mut rows,hooks:&mut hooks,random:&mut random,combat:&mut combat,combat_settings:&combat_settings,settings:&vehicle_settings,time:step as f64/60.,navigation:true,population:true};
            assert!(vehicles.boarded(&frame,Some(controller)));assert_eq!(vehicles.boarded_vehicle(&frame,Some(controller)),Some(hull_target));
            assert_eq!(vehicles.empty_seats(&frame,Some(hull_target)),0.);
            vehicles.reserve(&frame,hull_target,obj,1.);assert_eq!(vehicles.free_seats(&frame,hull_target,0.,1.),0.);
            assert_eq!(vehicles.notify(&mut frame),0.);vehicles.update_decay(&mut frame,1./60.);
            assert_eq!(frame.read_decay(raw_hull).unwrap().idle,0.);
            assert_eq!(frame.tow_weapon(obj),Some((0,0.)));
            assert!(frame.helper_traversal(agent,false)>0.);assert_eq!(frame.helper_traversal(agent,true),0.);
            helper.kind=Some(bfe_sim::ai::control::world::HelperKind::Fly);frame.rows.state.controllers.helper_created(0,helper);
            assert!(frame.helper_traversal(agent,false).is_nan());assert!(frame.helper_traversal(agent,true).is_nan());
            helper.kind=Some(bfe_sim::ai::control::world::HelperKind::Ground);frame.rows.state.controllers.helper_created(0,helper);
        }
        {
            use bfe_sim::ai::combat::world::World;
            let mut frame=bfe_sim::ai::owned::frame::Frame {rows:&mut rows,hooks:&mut hooks,random:&mut random,settings:&combat_settings,time:step as f64/60.,response:None};
            assert!(frame.has_vehicle_dispatch());assert!(frame.has_mount_reach());
            assert!(!frame.agent_board_policy(agent,hull_target),"stationary hull without remembered target");
            frame.rows.w.sim.store.vehicle.get_mut(hull).unwrap().speed=1.;
            assert!(frame.agent_board_policy(agent,hull_target),"moving hull bypasses remembered target");
            frame.rows.w.sim.store.vehicle.get_mut(hull).unwrap().speed=0.;
            assert!(frame.mount_reach(control_obj,&three::Vector3::new(0.,0.,10.)));
            let choice=bfe_sim::ai::combat::agent::dispatch_shared(&mut frame,&mut combat.shared,Some(controller),control_obj,obj,true);
            assert_eq!((choice.pattern,choice.probability,choice.aux,choice.weapon),(0.,0.,false,None));
        }
        assert_eq!(rows.tow_weapon(obj),Some((0,0.)));
        assert!(rows.vehicle_fields(control_obj,bfe_sim::ai::vehicles::world::SeatIntent {up:step as f64,down:2.,brake:true,trick:true,immelmann:false,boost:true}));
        let intent=&rows.w.fleet.seats_owner().hulls.controls.get(&seat_key).unwrap().intent;
        assert_eq!(intent.up,step as f64);assert_eq!(intent.buttons,[4.,5.]);assert!(intent.reload);assert!(intent.boost);
        assert_eq!(rows.vehicle_steer(control_obj,|steer|{let steer=steer.get_or_insert_with(bfe_sim::ai::vehicles::world::Steer::full);steer.land=Some(true);steer.land==Some(true)}),Some(true));
        assert_eq!(rows.repair_target(obj),Some(raw_hull));
        assert!(rows.state.entities.repair_matches(hull_target,raw_hull));
        assert!(rows.state.entities.repair_matches(hull_target,hull_target));
        assert!(!rows.state.entities.repair_matches(raw_hull,hull_target));
        rows.buttons(obj,2.,3.);rows.aim(obj,Some(three::Vector3::new(step as f64,0.,1.)));
        rows.edit_weapon(obj,0,|w|w.spread_scale=0.5);
        assert_eq!(rows.weapon_settings(obj,0).spread_scale,0.5);assert_eq!(rows.current_weapon(obj,0.),Some(0));
        assert!(rows.select(obj,0));assert_eq!(rows.body_lease_count(obj),0);
        assert!(rows.w.sim.store.unit.get(r).unwrap().intent.reload);assert!(rows.weapon(obj,0).is_some());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(rows.changes.changed(),&[r]);
    // Mounted pattern state belongs to this agent. The optional helper is
    // absent here: pattern entries still update canonical steering, tracker
    // delays and the process's attack-run distances without a host callback.
    {
        let mut frame=bfe_sim::ai::owned::frame::Frame {rows:&mut rows,hooks:&mut hooks,random:&mut random,settings:&combat_settings,time:0.,response:None};
        frame.create_combat(agent,Default::default());
        assert_eq!(frame.target_override(&mut combat.shared,agent,obj),Some(false));
        let row=frame.rows.state.agents.get_mut(&frame.rows.state.entities,agent).unwrap();
        let previous=row.behavior.clone();row.behavior=bfe_sim::ai::owned::agents::Behavior::Fly;
        row.vehicle.initialize(bfe_sim::ai::control::world::ForeignKind::Fly,Some(control_obj));
        for state in [2.,5.] {
            use bfe_sim::ai::vehicles::agents::AgentMachine;
            let machine=&mut frame.rows.state.agents.get_mut(&frame.rows.state.entities,agent).unwrap().vehicle;
            machine.request_state(state);machine.transition();
            assert_eq!(frame.target_override(&mut combat.shared,agent,obj),Some(false));
            assert_eq!(frame.board_override(agent,obj),Some(false));
        }
        frame.rows.state.agents.get_mut(&frame.rows.state.entities,agent).unwrap().behavior=previous;
    }
    let draws=random.draw_counts();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        use bfe_sim::ai::combat::world::World;
        let mut frame=bfe_sim::ai::owned::frame::Frame {rows:&mut rows,hooks:&mut hooks,random:&mut random,settings:&combat_settings,time:tick as f64/60.,response:None};
        for pattern in [7.,13.,15.,16.] {
            frame.enter_pattern(agent,pattern);frame.tick_pattern(agent,pattern,1./60.);
            if pattern==15. {assert_eq!(frame.rows.vehicle_steer(control_obj,|state|state.unwrap().orbit.is_some_and(|o|o.is_some())),Some(true));}
            frame.exit_pattern(agent,pattern);
        }
        assert_eq!(frame.rows.vehicle_steer(control_obj,|state|state.unwrap().orbit),Some(Some(None)));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(random.draw_counts()[0]-draws[0],240,"orbit entry draws both raw side bits in A order");
    // A promoted pilot receives a fresh control; the old body now mans a
    // gunner slot. Its shared target comes from the separate human pilot's
    // reticle, and every early gate precedes that lazy query.
    let pilot=rows.w.sim.store.unit.insert(Unit {alive:true,generation:1.,human:true,team:1.,..Default::default()});
    let human_lease=rows.w.sim.store.seat_lease.insert(bfe_sim::gen::SeatLease {unit:Some(pilot),generation:1.,vehicle:Some(hull),human:true,team:1.,..Default::default()});
    rows.state.entities.register(Entity::Unit(pilot));rows.state.entities.register(Entity::Lease(human_lease));
    rows.w.sim.store.unit.get_mut(pilot).unwrap().seat=Some(human_lease);
    rows.w.sim.store.unit.get_mut(r).unwrap().team=1.;
    rows.w.sim.store.seat_lease.get_mut(lease).unwrap().index=1.;rows.w.sim.store.seat_lease.get_mut(lease).unwrap().team=1.;
    rows.w.sim.store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(human_lease);
    rows.w.sim.store.vehicle.get_mut(hull).unwrap().seats.push(Default::default());rows.w.sim.store.vehicle.get_mut(hull).unwrap().seats[1].lease=Some(lease);
    let gun=rows.state.births.construct(&rows.w.sim.store,rows.w.fleet.seats_owner(),&mut rows.state.entities,lease,None).unwrap();
    rows.w.sim.store.seat_lease.get_mut(lease).unwrap().controllable=Some(gun);
    let class=rows.state.control_class(&seat_class,&rows.w.sim.strings,7.);
    assert!(rows.state.controls.register(&rows.w.sim.store,&mut rows.state.entities,gun,lease,bfe_sim::gen::AiLifeHandle {kind:bfe_sim::gen::KIND_VEHICLE.into(),slot:hull.slot,generation:hull.gen,life:1.},7.,Default::default(),"gun".into(),Some(controller),bfe_sim::ai::owned::seats::Occupant {id:r.slot as f64,hero:false}));
    assert!(rows.state.controls.set_class(gun,class));
    let gun_obj=rows.state.entities.find(Entity::Control(gun)).unwrap();rows.state.controllers.write(&rows.state.entities,controller).unwrap().unit=Some(gun_obj);
    let target=rows.w.sim.store.unit.insert(Unit {alive:true,generation:1.,team:2.,..Default::default()});
    if let bfe_sim::gen::UnitBody::Soldier(body)=&mut rows.w.sim.store.unit.get_mut(target).unwrap().soldier {body.position=[0.,0.,10.];}
    let target_obj=rows.state.entities.register(Entity::Unit(target));rows.w.sim.store.units.list.push(bfe_sim::gen::Member::Unit(target));
    rows.state.phase_facts(&bfe_sim::gen::AiPhaseFacts {reticle:bfe_sim::gen::AiPhaseReticle {pilot:Some(hull),pilot_life:1.,target:Some(bfe_sim::gen::Member::Unit(target)),target_life:1.},..Default::default()},&[]);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        use bfe_sim::ai::combat::world::World;
        let mut frame=bfe_sim::ai::owned::frame::Frame {rows:&mut rows,hooks:&mut hooks,random:&mut random,settings:&combat_settings,time:0.,response:None};
        assert_eq!(frame.shared_target(controller),Some(target_obj));
        frame.rows.w.sim.store.vehicle.get_mut(hull).unwrap().flying=true;assert_eq!(frame.shared_target(controller),None);frame.rows.w.sim.store.vehicle.get_mut(hull).unwrap().flying=false;
        frame.rows.w.sim.store.seat_lease.get_mut(human_lease).unwrap().human=false;assert_eq!(frame.shared_target(controller),None);frame.rows.w.sim.store.seat_lease.get_mut(human_lease).unwrap().human=true;
        frame.rows.w.sim.store.unit.get_mut(target).unwrap().alive=false;assert_eq!(frame.shared_target(controller),None);frame.rows.w.sim.store.unit.get_mut(target).unwrap().alive=true;
        frame.rows.w.sim.store.unit.get_mut(target).unwrap().team=1.;assert_eq!(frame.shared_target(controller),None);frame.rows.w.sim.store.unit.get_mut(target).unwrap().team=2.;
        frame.rows.w.sim.store.seat_lease.get_mut(lease).unwrap().released=true;assert_eq!(frame.shared_target(controller),None);frame.rows.w.sim.store.seat_lease.get_mut(lease).unwrap().released=false;
        frame.rows.state.reticle.target_life=2.;assert_eq!(frame.shared_target(controller),None);frame.rows.state.reticle.target_life=1.;
        frame.rows.state.reticle.pilot_life=2.;assert_eq!(frame.shared_target(controller),None);frame.rows.state.reticle.pilot_life=1.;
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    // Outer publication keeps unread updates, then frees released controls
    // only after all agent/helper scopes have left. Reboarding reuses storage
    // with another generation and never grows the warmed native pool.
    let mut output=std::mem::take(&mut rows.state.control_views);
    let first=output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)));
    assert_eq!(first[1],2);assert!(rows.state.births.read(control).is_none());
    assert_eq!(output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle))),first);
    output.acknowledge(first[2]);
    assert_eq!(output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)))[1],0);
    // Steering never leaves the native owner. Structural identities remain
    // immediately visible, independently of the following unit phase.
    let key=bfe_sim::seats::lease::control_key(gun);
    rows.w.fleet.seats_owner().hulls.controls.get_mut(&key).unwrap().intent.forward=0.75;
    let version=output.version();
    assert_eq!(output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)))[1],0);
    assert_eq!(output.version(),version);
    let driven=output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)));
    assert_eq!(driven[1],0);output.acknowledge(driven[2]);
    rows.w.sim.store.seat_lease.get_mut(lease).unwrap().released=true;
    let released=output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)));
    assert_eq!(released[1],1);assert!(rows.state.births.read(gun).is_some());output.acknowledge(released[2]);
    // A physical release publishes controller=None immediately, but the old
    // qualified object survives until Core has completed its unbind hooks.
    assert!(rows.state.link_controller(rows.w,rows.changes,gun_obj,0,false));
    rows.state.controllers.write(&rows.state.entities,controller).unwrap().unit=None;
    assert_eq!(output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)))[1],0);
    assert!(rows.state.births.read(gun).is_none());
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..1000 {
        rows.w.sim.store.seat_lease.get_mut(lease).unwrap().released=false;
        let next=rows.state.births.construct(&rows.w.sim.store,rows.w.fleet.seats_owner(),&mut rows.state.entities,lease,None).unwrap();
        assert!(next.slot<2);assert_ne!(next,gun);
        rows.w.sim.store.seat_lease.get_mut(lease).unwrap().controllable=Some(next);
        let born=output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)));output.acknowledge(born[2]);
        rows.w.sim.store.seat_lease.get_mut(lease).unwrap().released=true;
        // F2 retires steering before the AI publisher sees the released lease.
        rows.w.fleet.seats_owner().hulls.controls.remove(&bfe_sim::seats::lease::control_key(next));
        let retired=output.publish(rows.state,&rows.w.sim.store,rows.w.fleet.seats_owner(),rows.buildings.rows_in(crate::battle::f4::live_ref(&rows.w.battle)));output.acknowledge(retired[2]);
        assert!(rows.state.births.read(next).is_none());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    rows.state.control_views=output;
    rows.w.sim.store.vehicle.get_mut(hull).unwrap().target_generation=2.;
    assert!(rows.state.controller_hull(&rows.w.sim.store,0,control_obj).is_none());
    assert!(!rows.vehicle_fields(control_obj,Default::default()));
    assert!(rows.vehicle_steer(control_obj,|_|panic!("old holder must not edit steering")).is_none());
    rows.w.sim.store.unit.remove(r);let replacement=rows.w.sim.store.unit.insert(Unit {alive:true,..Default::default()});assert_eq!(replacement.slot,r.slot);
    assert!(!rows.unit(obj).alive);assert!(!rows.has_arsenal(obj));assert_eq!(rows.weapon(obj,0),None);
    assert!(!rows.select(obj,0));
}

mod canonical_combat_fixture {
use bfe_sim::{ai::{owned::{frame::Hooks,rows::Rows,weapons::Changes,arsenals::Mounted,entities::Entity,scratch::List},combat::world::*,phase::RandomState},gen::Weapon};
use three::Vector3;
pub struct Callbacks {vehicles:bfe_sim::ai::owned::vehicles::Binding}
impl Default for Callbacks {
    fn default()->Self {Self {vehicles:bfe_sim::ai::owned::vehicles::Binding {authority:bfe_sim::ai::owned::vehicles::Authority::new(bfe_sim::ai::vehicles::VehicleAI::new()),settings:Default::default(),navigation:true,population:true}}}
}
impl bfe_sim::ai::owned::navigation::Environment for Callbacks {
 fn time(&self)->f64 {0.}
 fn remembered(&mut self,_:bfe_sim::ai::owned::navigation::Actor)->Option<bfe_sim::ai::nav::math::Vec3> {panic!("unprepared fixture memory")}
 fn clear_above(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::ai::nav::math::Vec3,_:&bfe_sim::gen::AiLifeHandle)->bool {panic!("unprepared fixture ceiling")}
 fn follow_far(&mut self)->f64 {panic!("unprepared fixture follow")}
 fn seat_frame(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::vehicles::f2_seats::HullLife,_:usize)->Option<(bfe_sim::ai::nav::math::Vec3,bfe_sim::ai::nav::math::Vec3)> {panic!("unprepared fixture seat pose")}
}
#[allow(unused_variables)]
impl<C:Changes+?Sized,M:Mounted> Hooks<C,M> for Callbacks {
fn vehicle_binding(&self)->bfe_sim::ai::owned::vehicles::Binding {self.vehicles.clone()}
fn fire_primary(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,controller:Obj) {rows.state.controllers.write(&rows.state.entities,controller).unwrap().fire_a=true;}
fn flight_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent:Obj,points:&[[f64;3]],over:bool,altitude:bool) {panic!("unprepared native flight path")}

fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,origin:bfe_sim::ai::vehicles::world::Origin)->Option<Vector3> {let bfe_sim::ai::vehicles::world::Origin::Lease(key)=origin else{panic!("fixture lease origin")};let Entity::Lease(reference)=rows.state.entities.get(key).unwrap() else{panic!("fixture lease identity")};let body=rows.w.sim.store.seat_lease.get(reference).unwrap().unit.unwrap();Some(rows.w.sim.store.unit.get(body).unwrap().origin_on_foot())}
fn origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 { {let Entity::Unit(r)=rows.state.entities.get(u).unwrap() else {panic!("body origin")};rows.w.sim.store.unit.get(r).unwrap().origin_on_foot()} }
fn forward(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 { {let Entity::Unit(r)=rows.state.entities.get(u).unwrap() else {panic!("body forward")};rows.w.sim.store.unit.get(r).unwrap().forward_on_foot()} }
fn aim_point(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 { panic!("unbound fixture service aim_point") }
fn sphere(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Sphere {
    struct NoBodies;
    impl bfe_sim::ai::owned::observe::Bodies for NoBodies {
        fn sphere(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::store::Ref<bfe_sim::gen::Unit>)->Option<Sphere> {panic!("only native mounted spheres are prepared")}
    }
    rows.sphere(u,&mut NoBodies).expect("canonical mounted sphere")
}
fn use_classic_random(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj) { {let Entity::Unit(r)=rows.state.entities.get(u).unwrap() else {panic!("body stream")};let unit=rows.w.sim.store.unit.get_mut(r).unwrap();unit.combat.random=true;for weapon in &mut unit.combat.weapons {weapon.random=true;}rows.changes.mark(rows.w,r);} }
fn query(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,center: &Vector3,radius: f64,filter: QueryFilter)-> List<Obj> { assert_eq!(radius,8.,"only the boarding query is prepared");assert!(matches!(filter,QueryFilter::Hostile {..}));rows.state.buffers.units.take() }
fn relation(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,a: f64,b: f64)-> f64 { if a==b {1.}else{-1.} }
fn living(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState)-> List<Obj> { panic!("unbound fixture service living") }
fn los(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,from: &Vector3,to: &Vector3,a: Option<Obj>,b: Option<Obj>)-> bool { panic!("unbound fixture service los") }
fn set_stance(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,c:Obj,stance:f64) {rows.state.controllers.write(&rows.state.entities,c).unwrap().set_stance(stance);}
fn human_target(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,hull:Obj)->Option<Obj> {
    let Some(Entity::Hull(reference))=rows.state.entities.get(hull) else {panic!("native reticle hull")};
    let body=rows.w.sim.store.vehicle.get(reference).unwrap();assert!(!body.flying);
    let pilot=rows.w.sim.store.seat_lease.get(body.seats[0].lease.unwrap()).unwrap();assert!(pilot.human);
    rows.human_reticle_target(hull)
}
fn interrupts_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,combat:&mut bfe_sim::ai::combat::binding::CombatAi)-> bool { panic!("unbound fixture service interrupts_tick") }
fn has_helper(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool {rows.state.agents.get(&rows.state.entities,agent).unwrap().helper.kind.is_some()}
fn helper_arrived(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj)-> bool { panic!("unbound fixture service helper_arrived") }
fn helper_flying(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj)-> bool { panic!("unbound fixture service helper_flying") }
fn helper_has_follow(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool { panic!("unbound fixture service helper_has_follow") }
fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,point: &Vector3,mode: Option<GotoMode>) { panic!("unbound fixture service helper_goto") }
fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj) { panic!("unbound fixture service helper_stop") }
fn helper_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,dt: f64) { panic!("unbound fixture service helper_tick") }
fn helper_follow(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,target: Obj) { panic!("unbound fixture service helper_follow") }
fn helper_terminate(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj) { panic!("unbound fixture service helper_terminate") }
fn request_jet(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,controller: Obj,vector: &Vector3) { panic!("unbound fixture service request_jet") }
fn emit_vo(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,unit: Obj,event: &str,property: &str) { panic!("unbound fixture service emit_vo") }
}
pub struct Mount;
impl Mounted for Mount {
fn count(&self,_:&crate::world::owner::World,_:bfe_sim::ai::owned::seats::Source,_:usize)->Option<usize> {panic!("no mounted fixture arsenal")}
fn read<R>(&self,_:&crate::world::owner::World,_:bfe_sim::ai::owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&Weapon)->R)->Option<R> {panic!("no mounted fixture arsenal")}
fn edit<R>(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::ai::owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&mut WeaponSettings)->R)->Option<R> {panic!("no mounted fixture arsenal")}
 }
}

#[test]
fn native_combat_frame_ingests_alerts_and_aims_before_weapon_fire_without_body_mirrors() {
    use bfe_sim::{ai::{owned::{frame::{Frame,Settings},rows::Rows,state::{State,Capacity},entities::Entity,events::{Record,Journal}},nav::{Nav,PlannerOptions},phase::RandomState,control::{world::ClassSnap,difficulty::{Difficulty,DifficultyOptions}},combat::{binding::{CombatAi,Event,EventRef},world::{World,Draws,Ref as Weak}}},
        gen::{Store,Unit,Character,Weapon},store::DefId,units::ownership::Ownership,battle::{commands::Commands,live_phase::Live},vehicles::f2_seats::Seats,combat::defs::{WeaponDefs,WeaponClass,JsObj,Js}};
    let mut nav=Nav::empty(PlannerOptions::default());
    use bfe_sim::ai::nav::hints::{HintRecord,HintSet,build_hints};
    let jet=HintRecord {kind:Some("jetjump".into()),name:None,xfrm:[1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.],properties:vec![]};
    nav.hints=HintSet::new(build_hints(&[jet],None,350.));
    let mut state=State::new(nav,false,Capacity {registry:8,characters:2,posts:2,hulls:0,buildings:0,controls:0,scopes:2,cameras:1});
    let mut defs=WeaponDefs::default();let class=WeaponClass::new(std::rc::Rc::new(JsObj {fields:vec![("channel".into(),Js::Num(0.))],def:None}));
    state.weapons.register(DefId(0),&class);defs.set(DefId(0),class);
    state.units.class(DefId(0),ClassSnap::default(),"test".into());state.combat_units.class_view(DefId(0),Default::default());state.units.vehicle_class(DefId(0),Default::default(),None);
    state.prepare_hints(|_|None);
    let mut store=crate::ai::owned::tests::empty_world();let mut commands=Commands::new(2);let mut changes=Ownership::default();
    let mut actors=Vec::new();
    for team in [1.,2.] {
        let character=store.character.insert(Character {team,..Default::default()});state.roster.add(commands.adopt(&store,character));
        let unit=store.unit.insert(Unit {alive:true,generation:1.,team,character:Some(character),..Default::default()});
        store.unit.get_mut(unit).unwrap().combat.weapons.push(Weapon::create(DefId(0),&defs,false));
        store.character.get_mut(character).unwrap().unit=Some(unit);
        changes.adopt(&store,unit).unwrap();state.entities.register(Entity::Character(character));
        actors.push((unit,state.entities.register(Entity::Unit(unit))));
    }
    let (victim,obj)=actors[0];let shooter=actors[1].1;
    assert!(state.link_controller(&mut store,&mut changes,obj,0,true));let controller=state.controllers.key(0).unwrap();
    {let ctl=state.controllers.write(&state.entities,controller).unwrap();ctl.unit=Some(obj);ctl.generation=1.;}
    let mut seats=Seats::default();let live=Live::new(0,0,0);let mut mounted=canonical_combat_fixture::Mount;
    crate::vehicles::module::f2::live_seats::admit_test_seats(&mut store.fleet,std::mem::take(&mut seats));
  let mut rows=Rows {w:&mut store,buildings:&live.buildings,characters:&mut commands,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
    let settings=Settings {difficulty:std::cell::RefCell::new(Difficulty::classic(DifficultyOptions::default())),mission_stay_in_turrets:false,mission_no_jet_obstacle_test:false,ai_stay_in_turrets:false,vehicles:false,dense:false,navigation:false};
    let nav_settings=Settings {difficulty:std::cell::RefCell::new(settings.difficulty.borrow().clone()),mission_stay_in_turrets:false,mission_no_jet_obstacle_test:false,ai_stay_in_turrets:false,vehicles:false,dense:false,navigation:true};
    let mut hooks=canonical_combat_fixture::Callbacks::default();let mut random=RandomState::new(11);let mut combat=CombatAi::default();
    let mut frame=Frame {rows:&mut rows,hooks:&mut hooks,random:&mut random,settings:&settings,time:0.,response:None};
    let full=frame.unit(obj);
    assert_eq!(frame.unit_weapon_motion(obj),(full.kind,bfe_sim::ai::combat::weapons_ai::forward_speed(&full)));
    let mut visited=Vec::new();frame.visit_weapon_levels(obj,0,&mut |def|visited.push(def as *const _));
    assert_eq!(visited,frame.weapon(obj,0).unwrap().levels.unwrap().iter().map(|def|&**def as *const _).collect::<Vec<_>>());
    frame.set_weapon_yaw(obj,0,Some(-0.));
    assert_eq!(frame.rows.w.sim.store.unit.get(victim).unwrap().combat.weapons[0].yaw_spread_override.unwrap().to_bits(),(-0.0f64).to_bits());
    frame.set_weapon_yaw(obj,0,None);
    assert_eq!(frame.rows.w.sim.store.unit.get(victim).unwrap().combat.weapons[0].yaw_spread_override,None);
    let observed=frame.unit_sensor(obj);
    assert_eq!((observed.generation,observed.team,observed.controller),(1.,1.,Some(controller)));
    assert_eq!(frame.unit_bound_posture(obj),Some(bfe_sim::ai::combat::world::BoundPosture::Stand));
    frame.rows.w.sim.store.unit.get_mut(victim).unwrap().class_def=DefId(999);
    assert_eq!(frame.unit_sensor(obj),bfe_sim::ai::combat::world::UnitSensor::default());
    assert_eq!(frame.unit_weapon_motion(obj),(0.,0.));
    assert_eq!(frame.unit_bound_posture(obj),None,"missing classes retain the full reader's sphere fallback");
    frame.rows.w.sim.store.unit.get_mut(victim).unwrap().class_def=DefId(0);
    assert!(!frame.has_jet_hints());
    combat.create_perception(&mut frame,controller);combat.attach_controller(&mut frame,controller);
    assert!(frame.rows.w.sim.store.unit.get(victim).unwrap().combat.random);
    assert!(frame.rows.w.sim.store.unit.get(victim).unwrap().combat.weapons[0].random);
    let journal=Journal::new(4096);let mut reader=journal.reader();
    for _ in 0..8 {combat.aim(&mut frame,controller,1./60.);}
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        frame.time=tick as f64/60.;
        assert!(frame.cover_hints(&three::Vector3::default(),10.).is_empty());
        journal.emit(Record::Other);
        journal.emit(Record::Combat(Event::Shot {source:EventRef::Unit(shooter),target:EventRef::Ref(Weak {obj,generation:1.})}));
        reader.ingest(&journal,&mut combat,&mut frame);
        assert!(frame.ctl(controller).shot.pending);assert!(frame.ctl(controller).shot.known);
        combat.aim(&mut frame,controller,1./60.);
        let unit=frame.rows.w.sim.store.unit.get(victim).unwrap();assert!(unit.intent.aim.is_some());assert_eq!(unit.combat.weapons[0].wait,0.);
        frame.ctl_mut(controller).shot.pending=false;
        assert!((0. ..1.).contains(&frame.uniform_a()));assert!((0. ..1.).contains(&frame.uniform_b()));assert!((0. ..1.).contains(&frame.uniform_c()));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(frame.random.draw_counts(),[120,120,120]);assert_eq!(reader.dropped,0);
    // The subclass machine runs in the same canonical scope. Ranking can
    // query native perception without importing a controller or body table.
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        let agent=frame.rows.state.agents.begin(&mut frame.rows.state.entities,0,controller);
        frame.create_combat(agent,Default::default());
        frame.run_combat(agent,&mut combat,|frame,machine,combat| {
            assert!(!machine.tick(frame,combat,1./60.,bfe_sim::ai::combat::agent::Phase::Rank));
            // A synchronous transition retires the running object. Its
            // callback-visible tracker remains separate from the successor.
            assert!(frame.rows.state.agents.retire(&mut frame.rows.state.entities,agent));
            let next=frame.rows.state.agents.begin(&mut frame.rows.state.entities,0,controller);
            assert_ne!(next,agent);
            frame.agent_sync(machine);
            frame.rows.state.agents.get_mut(&frame.rows.state.entities,agent).unwrap().tracker_mut().point=three::Vector3::new(9.,8.,7.);
            frame.agent_resync(machine);assert_eq!(machine.tracker.point,three::Vector3::new(9.,8.,7.));
            assert_eq!(frame.rows.state.agents.get(&frame.rows.state.entities,next).unwrap().tracker().point,three::Vector3::default());
        });
        assert!(frame.rows.state.agents.get(&frame.rows.state.entities,agent).is_none());
        let next=frame.rows.state.agents.key(0).unwrap();assert_ne!(next,agent);
        assert!(frame.rows.state.agents.retire(&mut frame.rows.state.entities,next));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    // The Core feeds only its requests. Acceptance uses canonical Frame
    // facts and an explicit response scope while preserving perception.
    use bfe_sim::ai::control::{controller::Controller,policy::{Input,Kind},world::{CombatInput,ResponseState}};
    let agent=frame.rows.state.agents.begin(&mut frame.rows.state.entities,0,controller);
    let mut control=Controller::new(0);control.unit=Some(obj);control.generation=1.;
    // The retained aim target can differ from the weapon target. Returning
    // only CombatWrites would lose the old look when the target changes.
    assert!(frame.rows.combat_feed(0,&CombatInput::read(&control,None)));
    frame.ctl_mut(controller).aim_at(Some(Weak {obj:shooter,generation:1.}));
    frame.ctl_mut(controller).target=Some(Weak {obj,generation:1.});
    control.aim_target=frame.rows.combat_aim_target(0);
    assert_eq!(control.aim_target,Some(bfe_sim::ai::control::world::EntRef {unit:shooter,generation:1.}));
    assert!(frame.rows.combat_feed(0,&CombatInput::read(&control,None)));
    assert_eq!(frame.ctl(controller).aim_target,Some(Weak {obj:shooter,generation:1.}));
    let mut input=CombatInput::read(&control,None);
    input.policy=Input {kind:Kind::Follow,unit:Some(obj),controller_unit:Some(obj),has_agent:true,..Default::default()};
    let mut response=ResponseState::read(&control);
    let class=frame.rows.state.navigation.class(bfe_sim::ai::nav::units::ClassDef::default());
    let actor=frame.rows.state.navigation.register(&frame.rows.w.sim.store,AiLifeHandle {kind:bfe_sim::gen::KIND_UNIT.into(),slot:victim.slot,generation:victim.gen,life:1.},class).unwrap();
    frame.rows.state.actors[0]=Some(actor);
    input.helper_kind=Some(bfe_sim::ai::control::world::HelperKind::Ground);input.helper_has_path=true;
    assert!(frame.rows.combat_feed(0,&input));
    let view=frame.rows.state.vehicle_controller(&frame.rows.w.sim.store,controller).unwrap();
    assert!(!view.nav_control,"preallocated physical control does not create navControl presence");
    assert!(!view.executor);assert_eq!(view.executor_status,0.);assert!(view.helper);assert!(!view.helper_has_path,"GroundGotoHelper has no hasPath property");
    assert_eq!(view.helper_state,Some(1.));assert_eq!(view.helper_destination,Some(three::Vector3::default()));
    assert_eq!(view.agent,Some(agent));assert_eq!(view.fire_channel,Some(0.));
    input.nav_stance=Some(2.);
    let heap=frame.ctl(controller).perception.as_ref().unwrap().heap.items.as_ptr();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        input.policy.kind=Kind::Follow;assert!(frame.rows.combat_feed(0,&input));
        assert!(frame.rows.state.vehicle_controller(&frame.rows.w.sim.store,controller).unwrap().nav_control);
        frame.response=Some(bfe_sim::ai::owned::frame::ResponseScope::take(&mut response,combat.shared.share()));
        assert!(!frame.agent_accepts_alert(agent,true,&three::Vector3::default()));
        assert!(!frame.agent_accepts_hazard(agent));
        assert!(!frame.agent_accepts_acoustic(agent,shooter));
        frame.response.take().unwrap().finish(&mut response);
        assert!(!frame.base_accepts_board(&combat.shared,agent,shooter));
        input.policy.kind=Kind::Objective;input.policy.enhanced=true;input.policy.state=3.;
        assert!(frame.rows.combat_feed(0,&input));
        let point=Vec3::new(100.,0.,0.);
        frame.response=Some(bfe_sim::ai::owned::frame::ResponseScope::take(&mut response,combat.shared.share()));
        assert!(!frame.agent_accepts_alert(agent,true,&three::Vector3::new(point.x,point.y,point.z)));
        frame.response.take().unwrap().finish(&mut response);
        assert_eq!(response.enhanced.as_ref().unwrap().look_point,point);
        assert_eq!(response.enhanced.as_ref().unwrap().look_until,frame.time+2.5);
        assert_eq!(frame.ctl(controller).perception.as_ref().unwrap().heap.items.as_ptr(),heap);
        let alerts=frame.rows.combat_alerts();assert_eq!(alerts.len(),1);
        assert!(frame.rows.combat_writes(0).is_some());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(frame.random.draw_counts(),[120,120,120]);
    frame.settings=&nav_settings;assert!(frame.has_jet_hints());
    let draws=frame.random.draw_counts();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        let hint=frame.jet_hints_find(controller).expect("authored jet hint");
        assert!(!frame.jet_hints_excluded(controller));
        frame.jet_hints_enter(controller,Some(hint));
        assert_eq!(frame.rows.state.navigation.destination(&frame.rows.w.sim.store,actor),Some(Vec3::default()));
        frame.jet_hints_exit(controller,Some(hint));
        assert!(frame.jet_hints_tick(controller,Some(hint),1./60.));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(frame.random.draw_counts(),draws,"one candidate needs no tie draw");
    // The same response loan dispatches to the combat subclass's rules.
    // Follow's base refuses hazards above; a fighting foot soldier accepts.
    frame.create_combat(agent,Default::default());
    frame.response=Some(bfe_sim::ai::owned::frame::ResponseScope::take(&mut response,combat.shared.share()));
    assert!(frame.agent_accepts_hazard(agent));
    assert!(!frame.agent_accepts_alert(agent,true,&three::Vector3::default()));
    assert!(!frame.agent_accepts_acoustic(agent,shooter));
    frame.response.take().unwrap().finish(&mut response);
    // Boarding uses the same native clear-path policy as VehicleAI. A
    // stale candidate is contested; flyers and leaving agents refuse these
    // interrupts even when the base policy would accept them.
    use bfe_sim::ai::{owned::agents::Behavior,vehicles::{agents::{BoardAgent,UnBoardAgent},world::Ref as VehicleRef}};
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for _ in 0..120 {
        frame.rows.state.agents.get_mut(&frame.rows.state.entities,agent).unwrap().behavior=Behavior::Board(BoardAgent {vehicle:Some(VehicleRef {obj:shooter,generation:1.})});
        frame.response=Some(bfe_sim::ai::owned::frame::ResponseScope::take(&mut response,combat.shared.share()));
        assert!(!frame.agent_accepts_alert(agent,false,&three::Vector3::default()));
        assert!(!frame.agent_accepts_hazard(agent));assert!(!frame.agent_accepts_acoustic(agent,shooter));
        frame.rows.state.agents.get_mut(&frame.rows.state.entities,agent).unwrap().behavior=Behavior::Board(BoardAgent {vehicle:Some(VehicleRef {obj:shooter,generation:2.})});
        assert!(frame.agent_accepts_alert(agent,false,&three::Vector3::default()));assert!(frame.agent_accepts_hazard(agent));
        for behavior in [Behavior::Unboard(UnBoardAgent),Behavior::Fly] {
            frame.rows.state.agents.get_mut(&frame.rows.state.entities,agent).unwrap().behavior=behavior;
            assert!(!frame.agent_accepts_alert(agent,false,&three::Vector3::default()));
            assert!(!frame.agent_accepts_hazard(agent));assert!(!frame.agent_accepts_acoustic(agent,shooter));
        }
        frame.response.take().unwrap().finish(&mut response);
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    frame.rows.w.sim.store.unit.get_mut(victim).unwrap().generation=2.;
    journal.emit(Record::Combat(Event::Shot {source:EventRef::Unit(shooter),target:EventRef::Ref(Weak {obj,generation:1.})}));
    reader.ingest(&journal,&mut combat,&mut frame);assert!(!frame.ctl(controller).shot.pending);
    // A retired seat/control token can remain in feedback until Core.sync.
    // Feeding it must neither panic nor resolve the reused entity slot.
    assert!(frame.rows.state.entities.retire(obj));
    let replacement=frame.rows.state.entities.register(Entity::Unit(victim));
    assert_ne!(replacement,obj);assert!(frame.rows.combat_feed(0,&input));
    assert_eq!(frame.ctl(controller).unit,Some(obj));assert!(!frame.unit(obj).alive);
}

#[test]
fn navigation_follows_humans_without_creating_controls_and_qualifies_their_lives() {
    let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    use bfe_sim::{ai::{owned::{state::{State,Capacity},entities::Entity,navigation::Command},nav::{Nav,PlannerOptions},phase::RandomState},gen::{Store,Unit,UnitBody,Soldier,AiUnitClass},store::Strings};
    let mut state=State::new(Nav::empty(PlannerOptions::default()),false,Capacity {registry:8,characters:2,posts:2,hulls:0,buildings:0,controls:0,scopes:2,cameras:1});
    let mut strings=Strings::default();let row=AiUnitClass {name:strings.intern_str("soldier"),..Default::default()};state.unit_class(&row,&strings);
    let mut store=crate::ai::owned::tests::empty_world();let human=store.unit.insert(Unit {alive:true,human:true,generation:1.,soldier:UnitBody::Soldier(Soldier {position:[0.,0.,10.],max_speed:7.,..Default::default()}),..Default::default()});
    let person=state.entities.register(Entity::Unit(human));
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    let target=state.body_actor(&store,person,false).unwrap();
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(state.navigation.nav.controls.len(),0);assert!(state.navigation.control(target).is_none());assert!(state.navigation.observation(&store,target).is_none());
    let old=state.navigation.follow_ref(target).unwrap();
    let bot=store.unit.insert(Unit {alive:true,generation:1.,soldier:UnitBody::Soldier(Soldier {max_speed:7.,max_strafe:5.,max_turn:2.,..Default::default()}),..Default::default()});
    let follower=state.entities.register(Entity::Unit(bot));let actor=state.body_actor(&store,follower,true).unwrap();
    let mut random=RandomState::new(13);let mut env=NavigationEnvironment {now:0.};
    state.navigation.command(&mut store,&mut random,&mut env,actor,Command::Follow(old,3.,false)).unwrap();
    for _ in 0..8 {state.navigation.nav.service(1.);state.navigation.command(&mut store,&mut random,&mut env,actor,Command::Tick(1./60.)).unwrap();state.navigation.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).unwrap();}
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        let UnitBody::Soldier(body)=&mut store.unit.get_mut(human).unwrap().soldier else {unreachable!()};body.position[2]=10.+tick as f64*0.1;
        assert_eq!(state.body_actor(&store,person,false),Some(target));
        state.navigation.command(&mut store,&mut random,&mut env,actor,Command::Tick(1./60.)).unwrap();state.navigation.input(&mut store,&mut random,&mut env,actor,1./60.,None,false).unwrap();
        state.navigation.clear_dirty();env.now+=1./60.;
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(state.navigation.nav.controls.len(),1);
    assert!(store.unit.get(bot).unwrap().intent.forward>0.);
    store.unit.get_mut(human).unwrap().generation=2.;assert_eq!(state.body_actor(&store,person,false),Some(target));assert!(state.navigation.resolve(&store,old).is_none());
    let current=state.navigation.follow_ref(target).unwrap();assert!(state.navigation.resolve(&store,current).is_some());
    assert_eq!(state.body_actor(&store,person,true),Some(target));assert_eq!(state.navigation.nav.controls.len(),2);assert!(state.navigation.control(target).is_some());
    store.unit.remove(human);let next=store.unit.insert(Unit {alive:true,generation:1.,..Default::default()});let replacement=state.entities.register(Entity::Unit(next));
    let new_actor=state.body_actor(&store,replacement,false).unwrap();assert_ne!(new_actor,target);assert!(state.navigation.resolve(&store,current).is_none());
}

#[test]
fn native_stationary_seat_helpers_keep_requests_and_observe_current_position_without_queries() {
    let mut seats=bfe_sim::vehicles::f2_seats::Seats::default();
    use bfe_sim::{gen::*,store::Ref,ai::{owned::navigation::{Actor,Command,Environment},
        nav::{goto::GotoMode,units::{ClassDef,UnitRef},math::Vec3},phase::RandomState,vehicles::world::ClassView}};
    struct NoQueries;
    impl Environment for NoQueries {
        fn time(&self)->f64 {panic!("stationary helper must not observe time")}
        fn remembered(&mut self,_:Actor)->Option<Vec3> {panic!("stationary helper must not resolve memories")}
        fn clear_above(&mut self,_:&mut crate::world::owner::World,_:Vec3,_:&AiLifeHandle)->bool {panic!("stationary helper must not cast")}
        fn follow_far(&mut self)->f64 {panic!("stationary helper must not read formation")}
        fn seat_frame(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::vehicles::f2_seats::HullLife,_:usize)->Option<(Vec3,Vec3)> {panic!("stationary helper must not pose the rig")}
    }
    let mut store=crate::ai::owned::tests::empty_world();let mut nav=owned_navigation(false);
    let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:2.,seats:vec![VehicleSeat::default(),VehicleSeat::default()],..Default::default()});
    let unit=store.unit.insert(Unit {alive:true,generation:3.,..Default::default()});
    let control=Ref::new(3,4);
    let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:3.,vehicle:Some(hull),index:1.,controllable:Some(control),..Default::default()});
    store.unit.get_mut(unit).unwrap().seat=Some(lease);
    store.vehicle.get_mut(hull).unwrap().seats[1].lease=Some(lease);
    let class=nav.class(ClassDef::default());
    let actor=nav.register_seat(&store,AiLifeHandle {kind:KIND_VEHICLE.into(),slot:hull.slot,generation:hull.gen,life:2.},lease,control,7.,class,&ClassView::default()).unwrap();
    let mut random=RandomState::new(37);let mut env=NoQueries;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for tick in 0..120 {
        store.vehicle.get_mut(hull).unwrap().position=[tick as f64,2.,3.];
        assert_eq!(nav.destination(&store,actor),Some(Vec3::new(tick as f64,2.,3.)));
        nav.control_mut(actor).unwrap().request.valid=true;
        let obs=nav.renew_helper(&store,actor).unwrap();
        assert!(obs.arrived);assert!(!obs.failed);assert!(!obs.has_path);assert_eq!(obs.status,0.);
        assert!(nav.control(actor).unwrap().request.valid,"new helper does not reset controller request");
        for command in [Command::Tick(1./60.),Command::Follow(UnitRef {unit:u64::MAX,generation:None},1.,true),Command::Stance(2.),Command::Aim(Vec3::new(5.,2.,3.)),Command::AimTarget(None),Command::AimPath] {
            assert!(nav.command(&mut store,&mut random,&mut env,actor,command).unwrap().arrived);
            assert!(nav.control(actor).unwrap().request.valid,"present no-op keeps request");
        }
        nav.command(&mut store,&mut random,&mut env,actor,Command::Goto(Vec3::new(100.,0.,100.),GotoMode::Route)).unwrap();
        assert!(!nav.control(actor).unwrap().request.valid);
        nav.control_mut(actor).unwrap().request.valid=true;
        nav.command(&mut store,&mut random,&mut env,actor,Command::Stop).unwrap();
        assert!(!nav.control(actor).unwrap().request.valid);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(random.draw_counts(),[0,0,0]);assert!(nav.dirty().is_empty());assert!(nav.nav.planner_idle());
    store.seat_lease.get_mut(lease).unwrap().released=true;
    assert!(nav.helper_observation(&store,actor).is_none());
    assert!(nav.destination(&store,actor).is_none());
    assert!(nav.command(&mut store,&mut random,&mut env,actor,Command::Stop).is_none());
    store.seat_lease.get_mut(lease).unwrap().released=false;
    store.vehicle.get_mut(hull).unwrap().target_generation=3.;
    assert!(nav.renew_helper(&store,actor).is_none());
}

#[test]
fn native_registry_observes_team_after_lazy_sphere_and_skips_filtered_geometry_in_source_order() {
    use bfe_sim::{ai::owned::{registry::{Queries,World},entities::{Entities,Entity}},gen::{Member,Bounds,VehicleTarget},store::Ref};
    use std::cell::Cell;
    struct Observations {members:[Member;4],team:f64,spheres:usize,team_reads:Cell<usize>}
    impl World for Observations {
        fn len(&self)->usize {4}
        fn member(&self,index:usize)->Member {self.members[index]}
        fn alive(&mut self,member:Member)->bool {member!=self.members[1]}
        fn sphere(&mut self,member:Member)->Bounds {
            self.spheres+=1;
            if member==self.members[0] {self.team=2.;}
            Bounds {center:if member==self.members[2] {[100.;3]}else{[0.;3]},radius:1.}
        }
        fn team(&self,member:Member)->f64 {
            assert_ne!(member,self.members[1]);assert_ne!(member,self.members[2]);
            self.team_reads.set(self.team_reads.get()+1);self.team
        }
    }
    let mut queries=Queries::new(4,3);let mut entities=Entities::new(8);
    let members=std::array::from_fn(|i|Member::VehicleTarget(Ref::<VehicleTarget>::new(i as u32,1)));
    let keys:Vec<_>=members.iter().enumerate().map(|(i,&member)|{
        let obj=entities.register(Entity::HullEntry(Ref::new(i as u32,4)));queries.target(member,obj);obj
    }).collect();
    let mut world=Observations {members,team:1.,spheres:0,team_reads:Cell::new(0)};
    let mut filters=0;
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        world.team=1.;
        let found=queries.query_world(&mut world,&entities,&three::Vector3::default(),5.,&mut |obj,team|{
            filters+=1;assert_eq!(team,2.,"team is observed after the sphere");obj!=keys[3]
        });
        assert_eq!(&*found,&[keys[0]]);
        let living=queries.living_world(&mut world,&entities);assert_eq!(&*living,&[keys[0],keys[2],keys[3]]);
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(world.spheres,360);assert_eq!(world.team_reads.get(),240);assert_eq!(filters,240);
}

#[test]
fn native_barrier_queries_reuse_candidates_and_named_barriers_without_tick_allocations() {
    use bfe_sim::ai::nav::barriers::{BarrierRecord,BarrierSet,barrier_from_record,Work};
    let barriers=(0..32).map(|i|barrier_from_record(&BarrierRecord {name:Some(format!("wall{i}")),
        xfrm:[1.,0.,0.,0.,1.,0.,0.,0.,1.,i as f64*8.,0.,0.],size:[2.,2.,1.],flags:1.},i).unwrap()).collect();
    let mut barriers=BarrierSet::new(barriers);let mut work=Work::default();let mut normal=Vec3::default();
    let start=Vec3::new(-10.,0.,0.);let end=Vec3::new(270.,0.,0.);
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for _ in 0..120 {
        assert_eq!(barriers.point_clear(Vec3::default(),0.,1),(false,Some(0)));
        assert_eq!(barriers.point_clear(Vec3::new(-10.,0.,0.),0.,1),(true,None));
        assert!(barriers.segment_blocked(start,end,0.,1,Some(&mut work)));
        assert_eq!(barriers.nearest(start,end,0.,1,Some(&mut work)),(true,Some(0),63.99999999999999));
        // This winding ejects inward in the source, so all four candidates fail.
        let mut point=Vec3::default();assert!(!barriers.correct(&mut point,0.,1));
        assert_eq!(point,Vec3::default());
        let mut clear=start;assert!(barriers.correct(&mut clear,0.,1));
        assert_eq!(clear,start);
        let mut finish=end;assert!(barriers.obstruct(start,&mut finish,0.,1,Some(&mut work),Some(&mut normal)));
        assert_eq!(finish,Vec3::new(-2.000000000000001,0.,0.));assert_eq!(normal,Vec3::new(-1.,0.,0.));
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert!(work.count>0.);
}

#[path="common/vehicle_reentry.rs"]
mod vehicle_reentry;

#[path="common/vehicle_frame_hooks.rs"]
mod vehicle_frame_hooks;

#[test]
fn native_control_factory_uses_committed_lease_index_and_prepared_classes_without_allocations() {
 use bfe_sim::{gen::*,ai::{owned::{state::{State,Capacity},entities::Entity},nav::Nav},seats::hulls::HullSide,vehicles::f2_seats::Seats,store::Strings};
 let mut store=crate::ai::owned::tests::empty_world();let mut seats=Seats::default();let mut strings=Strings::default();
 let buildings=bfe_sim::battle::live_phase::Live::new(0,0,0);
 let mut state=State::new(Nav::empty(Default::default()),false,Capacity {registry:4,characters:0,posts:0,hulls:1,buildings:0,controls:2,scopes:2,cameras:0});
 let pilot=state.control_class(&AiUnitClass {name:strings.intern_str("craft"),forward_speed:AiClassScalar::F64(17.),..Default::default()},&strings,5.);
 let gunner=state.control_class(&AiUnitClass {name:strings.intern_str("craft"),forward_speed:AiClassScalar::F64(0.),turn_rate:AiClassScalar::F64(0.5),..Default::default()},&strings,7.);
 let unit=store.unit.insert(Unit {alive:true,generation:2.,..Default::default()});state.entities.register(Entity::Unit(unit));
 let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:3.,seats:vec![Default::default(),Default::default()],..Default::default()});
 seats.add_hull(&store,HullSide {vehicle:hull,..Default::default()});
 state.prepare_hull_controls(hull,vec![pilot,gunner]);
 let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:2.,vehicle:Some(hull),..Default::default()});
 let mut previous=None;
 ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
 for turn in 0..120 {
  let index=turn%2;
  // F2 assigns the body's seat after controllable returns. Construction
  // must not require that later assignment or resample a subsequent hero.
  store.unit.get_mut(unit).unwrap().seat=None;store.unit.get_mut(unit).unwrap().hero=true;
  store.seat_lease.get_mut(lease).unwrap().index=index as f64;
  let seats=&mut seats;
  for (at,seat) in store.vehicle.get_mut(hull).unwrap().seats.iter_mut().enumerate() {seat.lease=(at==index).then_some(lease);}
  let control=state.construct_control(&mut store,seats,&buildings.buildings,lease).unwrap();
  let key=state.entities.find(Entity::Control(control)).unwrap();
  assert_eq!(store.seat_lease.get(lease).unwrap().controllable,Some(control));
  let observed=state.controls.policy_unit(&store,&state.entities,control,&|_|panic!("no Character")).unwrap();
  assert!(!observed.alive);assert!(observed.hero);assert_eq!(observed.kind,if index==0 {5.}else{7.});
  assert_eq!(observed.soldier.unwrap().max_speed,Some(if index==0 {17.}else{0.}));
  store.unit.get_mut(unit).unwrap().hero=false;store.unit.get_mut(unit).unwrap().seat=Some(lease);
  assert!(state.controls.policy_unit(&store,&state.entities,control,&|_|0).unwrap().alive);
  assert!(state.controls.combat_unit(&store,&state.entities,control).unwrap().hero);
  if let Some(old)=previous {assert_ne!(control,old);assert!(state.births.read(old).is_none());}
  assert!(state.controls.retire(control));assert!(state.births.retire(seats,&mut state.entities,control));assert!(state.entities.get(key).is_none());
  store.seat_lease.get_mut(lease).unwrap().controllable=None;previous=Some(control);
 }
 COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
 store.seat_lease.get_mut(lease).unwrap().index=2.;assert!(state.construct_control(&mut store,&mut seats,&buildings.buildings,lease).is_none());
}

#[test]
fn generated_cold_navigation_preserves_authored_numbers_and_maps_hint_posts_to_canonical_order() {
 use bfe_sim::{ai::owned::{load::Navigation,population::Population},gen::*,store::{Ref,Strings}};
 let mut strings=Strings::default();let first=SpawnPost::Post(Ref::new(7,2));let second=SpawnPost::Post(Ref::new(1,4));
 let pools=AiNavPools {paths:8.,path_nodes:64.,requests:8.,search_nodes:None,heap:None};
 let input=AiNavSetup {json:br#"{"schema":1,"barriers":[],"hints":[{"kind":"cover","name":"hint","xfrm":[1,0,0,0,1,0,0,0,1,-0,2,3],"properties":[["844ccdb4",0,"same"]]}]}"#.to_vec(),bin:vec![],
  posts:vec![AiNavPost {post:first,name:strings.intern_str("same"),position:[0.,0.,0.]},AiNavPost {post:second,name:strings.intern_str("same"),position:[10.,0.,0.]}],pools:pools.clone(),hint_capacity:None};
 let mut words=vec![0.;AiNavSetup::WORDS];let mut heap=Vec::new();input.put(&mut words,&mut heap);
 let decoded=AiNavSetup::take(&words,&heap);assert_eq!(input,decoded);
 let loaded=Navigation::read(&decoded,&strings).unwrap();
 let (nav,posts)=loaded.into_parts();assert_eq!(nav.hints.list.len(),1);
 assert_eq!(nav.hints.list[0].position.x.to_bits(),(-0.0f64).to_bits());
 assert_eq!(nav.hints.list[0].command_post,None,"the source post array has no byName method");
 let mut store=crate::ai::owned::tests::empty_world();
 // Population order is independent from the loader's temporary indices.
 let a=store.post.insert(Post::default());let b=store.post.insert(Post::default());
 let mut population=Population::new(2,0,false);
 population.add(&store,AiPostPolicy {post:SpawnPost::Post(b),..Default::default()}).unwrap();
 population.add(&store,AiPostPolicy {post:SpawnPost::Post(a),..Default::default()}).unwrap();
 assert_eq!(Navigation::hint_posts(&[SpawnPost::Post(a),SpawnPost::Post(b)],&population),vec![Some(1),Some(0)]);
 assert_eq!(posts,vec![first,second]);
 let land=AiNavSetup {json:br#"{"schema":1,"barriers":[],"hints":[{"kind":"land","name":"land","xfrm":[1,0,0,0,1,0,0,0,1,0,0,0],"properties":[]}]}"#.to_vec(),..input.clone()};
 let (nav,_)=Navigation::read(&land,&strings).unwrap().into_parts();
 assert_eq!(nav.hints.list[0].command_post,None,"the source post array has no list for nearest landing posts");
 let empty=Navigation::read(&AiNavSetup {pools,..Default::default()},&strings).unwrap();
 assert!(empty.nav.graph.is_none());assert!(empty.nav.hints.list.is_empty());
 let invalid=AiNavSetup {json:b"{broken".to_vec(),..Default::default()};assert!(Navigation::read(&invalid,&strings).is_err());
}

#[test]
fn native_weapon_record_loading_retains_side_order_truthiness_and_authored_charge_levels() {
 use bfe_sim::{ai::{owned::{load::weapon_records,process::Process},combat::weapons_ai::Registered},combat::defs::{Js,JsObj,WeaponClass}};
 use std::rc::Rc;
 let first=weapon_records(br#"{"weapons":{"test":{"ai":{"maxRange":49},"levels":[{"at":0.6,"ai":{"maxRange":77}}]},"skip":{"ai":false},"empty":{"ai":{}}}}"#).unwrap();
 assert_eq!(first.iter().map(|row|row.0.as_str()).collect::<Vec<_>>(),vec!["test","empty"]);
 let second=weapon_records(br#"{"weapons":{"test":{"ai":{"maxRange":61},"levels":[{"at":0.6,"ai":{"maxRange":88}}]}}}"#).unwrap();
 let mut process=Process::default();let (mut shared,vehicles)=process.begin();shared.register(&first);shared.register(&second);
 let class=WeaponClass::new(Rc::new(JsObj {fields:vec![("class".into(),Js::Str("test".into())),("at".into(),Js::Num(0.6))],def:None}));
 shared.prepare_class(&class);shared.begin(4);
 assert_eq!(shared.weapons.weapon_ai(&class.levels[0].obj).max_range,88.);
 process.end(shared,vehicles);
 let (mut shared,vehicles)=process.begin();shared.register(&[("other".into(),Registered {ai:Js::Bool(true),..Default::default()})]);shared.prepare_class(&class);shared.begin(4);
 assert_eq!(shared.weapons.weapon_ai(&class.levels[0].obj).max_range,88.);process.end(shared,vehicles);
 assert!(weapon_records(b"null").unwrap().is_empty());assert!(weapon_records(b"{").is_err());
}

#[test]
fn native_pilot_checks_the_ordered_live_rounds_and_exact_evasion_radius() {
 use bfe_sim::{ai::owned::pilot,gen::{Store,Round}};
 let mut store=crate::ai::owned::tests::empty_world();let center=three::Vector3::new(1.,2.,3.);
 let boundary=store.round.insert(Round {target_id:Some(0.),position:[301.,2.,3.],..Default::default()});
 let dead=store.round.insert(Round {target_id:Some(0.),position:[2.,2.,3.],dead:true,..Default::default()});
 let other=store.round.insert(Round {target_id:Some(4.),position:[2.,2.,3.],..Default::default()});
 store.sim.store.ordnance.rounds.extend([boundary,dead,other]);
 ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
 for _ in 0..120 {
  assert!(!pilot::incoming(&store,0.,&center,300.));
  store.round.get_mut(boundary).unwrap().position[0]=300.;
  assert!(pilot::incoming(&store,-0.,&center,300.));
  store.round.get_mut(boundary).unwrap().position[0]=301.;
  store.round.get_mut(dead).unwrap().dead=false;
  assert!(pilot::incoming(&store,0.,&center,300.));
  store.round.get_mut(dead).unwrap().dead=true;
 }
 COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
 store.round.remove(boundary).unwrap();let replacement=store.round.insert(Round {target_id:Some(0.),position:[2.,2.,3.],..Default::default()});
 assert_eq!(replacement.slot,boundary.slot);assert!(!pilot::incoming(&store,0.,&center,300.));
}

#[test]
fn generated_whole_phase_exchange_reuses_camera_and_journal_storage() {
    use bfe_sim::{ai::owned::exchange::Exchange,gen::{AiPhaseHeader,AiJournalEvent,AiJournalRef,Member}};
    let mut exchange=Exchange::new(2,4096);
    let header=AiPhaseHeader {cameras:2,events:1,sequence:7.,time:1.25,..Default::default()};
    let camera=AiPhaseCamera {fov:46.826,zoom:1.5,aspect:4./3.,near:0.2,far:500.,..Default::default()};
    let event=AiJournalEvent {sequence:6.,source:AiJournalRef {member:Some(Member::Unit(bfe_sim::store::Ref::new(3,2))),generation:Some(9.),direct:false},..Default::default()};
    let mut heap=Vec::new();header.put(&mut exchange.header,&mut heap);
    camera.put(&mut exchange.camera_words,&mut heap);camera.put(&mut exchange.camera_words[AiPhaseCamera::WORDS..],&mut heap);
    event.put(&mut exchange.journal_words,&mut heap);
    let cameras=exchange.cameras.as_ptr();let records=exchange.journal.records.as_ptr();
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for _ in 0..120 {
        let decoded=exchange.decode();assert_eq!(decoded.time,1.25);
        assert_eq!(exchange.cameras[0],camera);assert_eq!(exchange.journal.records[0],event);assert_eq!(exchange.journal.sequence,7.);
        let _=exchange.descriptor(&Default::default());
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    assert_eq!(exchange.cameras.as_ptr(),cameras);assert_eq!(exchange.journal.records.as_ptr(),records);
    let mut invalid=header.clone();invalid.cameras=3;invalid.put(&mut exchange.header,&mut heap);
    assert_eq!(exchange.decode().cameras,2);assert_eq!(exchange.journal.records[0],event);
    invalid.cameras=2;invalid.events=4097;invalid.put(&mut exchange.header,&mut heap);
    assert_eq!(exchange.decode().events,4096);assert_eq!(exchange.journal.records[0],event);
    invalid.cameras=u32::MAX;invalid.events=u32::MAX;invalid.put(&mut exchange.header,&mut heap);
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for _ in 0..120 {let input=exchange.decode();assert_eq!(input.cameras,2);assert_eq!(input.events,4096);}
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn cooked_vehicle_class_keeps_zero_false_and_landing_fields_at_cold_load() {
    use bfe_sim::{ai::owned::content::UnitClass,gen::{AiUnitClass,AiClassScalar as Scalar},store::Strings};
    let mut strings=Strings::default();
    let row=AiUnitClass {name:strings.intern_str("transport"),moving_turn_only:Scalar::Bool(false),min_speed:Scalar::F64(0.),
        max_yaw_speed:Scalar::F64(2.),vehicle_type:Scalar::Str(strings.intern_str("transport")),landing_time:Scalar::F64(3.),
        landing_speed:Scalar::F64(0.),landed_height:Scalar::F64(-0.),..Default::default()};
    let class=UnitClass::read(&row,&strings).vehicle;
    assert_eq!(class.moving_turn_only,Some(false));assert_eq!(class.min_speed,Some(0.));assert_eq!(class.max_yaw_speed,Some(2.));
    assert_eq!(class.vehicle_type.as_deref(),Some("transport"));assert_eq!(class.landing_time,Some(3.));assert_eq!(class.landing_speed,Some(0.));
    assert!(class.landed_height.unwrap().is_sign_negative());
    let missing=UnitClass::read(&AiUnitClass::default(),&strings).vehicle;
    assert_eq!(missing.moving_turn_only,None);assert_eq!(missing.min_speed,None);assert_eq!(missing.vehicle_type,None);assert_eq!(missing.landing_time,None);
}

#[test]
fn native_grenade_warning_state_renews_on_storage_generation_and_keeps_contact_minimum() {
    use bfe_sim::{ai::owned::danger::Dangers,gen::Grenade,store::Ref};
    let mut dangers=Dangers::default();dangers.reserve(2);let first:Ref<Grenade>=Ref::new(1,1);
    let (state,contact)=dangers.begin(first);assert_eq!(contact,f64::INFINITY);assert_eq!(state.first_contact,f64::INFINITY);
    dangers.contact(first,7.);let (mut state,contact)=dangers.begin(first);state.first_contact=contact;state.last_notify=8.;dangers.finish(first,state);
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
    for time in 9..129 {
        dangers.contact(first,time as f64);let (mut state,contact)=dangers.begin(first);
        if contact<state.first_contact {state.first_contact=contact;}
        assert_eq!(state.first_contact,7.);assert_eq!(state.last_notify,8.);dangers.finish(first,state);
    }
    let replacement=Ref::new(first.slot,2);let (state,contact)=dangers.begin(replacement);
    assert_eq!(state.first_contact,f64::INFINITY);assert_eq!(state.last_notify,-1.);assert_eq!(contact,f64::INFINITY);
    // Excess or stale loan returns neither panic nor overwrite another life.
    dangers.finish(first,state);
    for slot in 2..10002 {
        let excess=Ref::new(slot,1);dangers.contact(excess,0.);
        let (state,contact)=dangers.begin(excess);assert_eq!(contact,f64::INFINITY);
        dangers.finish(excess,state);
    }
    COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn vehicle_objective_traversal_uses_authored_settings_and_navigation_coercion() {
    use bfe_sim::{ai::owned::content::traversal_size,gen::AiClassScalar as Scalar,store::Strings};
    let mut strings=Strings::default();let padded=Scalar::Str(strings.intern_str(" FLYER "));
    assert_eq!(traversal_size(padded,&strings),32);assert_eq!(traversal_size(Scalar::F64(34.),&strings),34);
    for value in [Scalar::Undefined,Scalar::Null,Scalar::Bool(true),Scalar::F64(2.5)] {assert_eq!(traversal_size(value,&strings),1);}
}

#[test]
fn initial_mission_arguments_keep_numeric_coercion_separate_from_numeric_class_quotas() {
    use bfe_sim::{gen::*,store::Strings,ai::owned::initial::Program};
    let mut strings=Strings::default();let soldier=strings.intern_str("soldier");let hex=strings.intern_str("0x10");
    let count=strings.intern_str("SetUnitCount");let class=strings.intern_str("AddUnitClass");let delay=strings.intern_str("SetSpawnDelay");
    let input=AiRosterSetup {unit_scale:1.,capacity:200.,prepared:vec![soldier],calls:vec![
        AiRosterCall {name:class,args:vec![AiClassScalar::F64(1.),AiClassScalar::Str(soldier),AiClassScalar::Bool(true)],..Default::default()},
        AiRosterCall {name:count,args:vec![AiClassScalar::F64(1.),AiClassScalar::Bool(true)],..Default::default()},
        AiRosterCall {name:delay,args:vec![AiClassScalar::Str(hex),AiClassScalar::Null],..Default::default()},
    ],..Default::default()};
    let program=Program::read(&input,&mut strings);
    assert_eq!(program.spawning.teams[1].unit_count,1.);
    assert_eq!(program.spawning.teams[1].classes[0].target,0.,"AddUnitClass accepts only a number for its quota");
    assert_eq!(program.spawning.a,16.);assert_eq!(program.spawning.b,0.);assert!(program.spawning.q.is_infinite());
}

#[test]
fn authored_item_services_keep_empty_tables_and_canonical_order_without_allocating() {
    use bfe_sim::{ai::{owned::{content,items::Items,entities::Entities},control::classes::quantity},
        gen::{AiClassScalar as Scalar,AiItemDefinition,Item,Store},store::{DefId,Strings}};
    let mut strings=Strings::default();
    let name=strings.intern_str("supply");let base=strings.intern_str("powerupitem");
    let row=AiItemDefinition {def:DefId(11),base:Some(base),class_name:Some(name),
        service_health:Some(Vec::new()),service_ammo:None,
        health:vec![Scalar::F64(50.)],ammo:vec![Scalar::Str(strings.intern_str("7"))],
        bare:vec![Scalar::Undefined,Scalar::Undefined,Scalar::Undefined,Scalar::Undefined,Scalar::Undefined,Scalar::F64(12.)]};
    let mut items=Items::new();content::item(&row,&strings,&mut items);
    let mut store=crate::ai::owned::tests::empty_world();let reference=store.item.insert(Item {def:row.def,position:[1.,2.,3.],..Default::default()});
    store.items.items.push(reference);let entities=Entities::new(1);
    let view=items.read(&store,&entities,0);
    assert_eq!(view.def_class.as_deref(),Some("supply"));assert_eq!(view.def_base.as_deref(),Some("powerupitem"));
    assert_eq!(view.position,Vec3::new(1.,2.,3.));assert_eq!(quantity(&view.qty,0.,false),0.);
    assert_eq!(quantity(&view.qty,0.,true),7.);assert_eq!(quantity(&view.qty,5.,false),0.);
    let mut named=row;named.service_health=None;content::item(&named,&strings,&mut items);
    assert_eq!(quantity(&items.read(&store,&entities,0).qty,5.,false),12.);
    ALLOCATIONS.with(|count|count.set(0));COUNTING.with(|active|active.set(true));
    for _ in 0..120 {let view=items.read(&store,&entities,0);assert_eq!(quantity(&view.qty,0.,false),50.);}
    COUNTING.with(|active|active.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

#[test]
fn late_network_body_admission_precedes_roster_reads_and_rejects_stale_units() {
    use bfe_sim::{gen::*,store::Ref,battle::commands::Commands,ai::{nav::Nav,owned::{state::{State,Capacity},entities::Entity}}};
    let mut store=crate::ai::owned::tests::empty_world();
    let mut commands=Commands::new(64);
    let mut state=State::new(Nav::empty(Default::default()),false,Capacity {registry:64,characters:64,posts:0,hulls:0,buildings:0,controls:0,scopes:1,cameras:0});
    let characters:Vec<_>=(0..64).map(|_|store.character.insert(Character {human:true,..Default::default()})).collect();
    let handles:Vec<_>=characters.iter().map(|&r|commands.adopt(&store,r)).collect();
    state.prepare_roster(&store,&commands,&handles).unwrap();
    // These bodies are realized after initial AI activation, as network
    // admission exhausts the warm class pool. No aim callback has run yet.
    let bodies:Vec<_>=(0..64).map(|_|store.unit.insert(Unit::default())).collect();
    ALLOCATIONS.with(|c|c.set(0));COUNTING.with(|c|c.set(true));
    for (index,&body) in bodies.iter().enumerate() {
        assert!(state.admit_body(&store,body));
        store.character.get_mut(characters[index]).unwrap().unit=Some(body);
        let snap=state.roster.read(&store,&commands,&state.entities,&state.population,index).unwrap();
        assert_eq!(snap.unit,state.entities.find(Entity::Unit(body)));
        assert!(snap.unit.is_some());
    }
    COUNTING.with(|c|c.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
    let old=bodies[0];store.unit.remove(old).unwrap();
    let next=store.unit.insert(Unit::default());assert_eq!(old.slot,next.slot);assert_ne!(old.gen,next.gen);
    assert!(!state.admit_body(&store,old));assert!(!state.admit_body(&store,Ref::new(u32::MAX,1)));
    assert_eq!(state.roster.read(&store,&commands,&state.entities,&state.population,0).unwrap().unit,None);
    store.character.get_mut(characters[0]).unwrap().unit=Some(next);
    assert_eq!(state.roster.read(&store,&commands,&state.entities,&state.population,0).unwrap().unit,None);
    assert!(state.admit_body(&store,next));
    assert_eq!(state.entities.find(Entity::Unit(old)),None);
    assert_eq!(state.roster.read(&store,&commands,&state.entities,&state.population,0).unwrap().unit,state.entities.find(Entity::Unit(next)));
}

#[test]
fn seat_birth_rejects_invalid_holder_index_and_occupant_generations() {
    use bfe_sim::{gen::*,store::Ref,ai::owned::{seats::Controls,entities::Entities},vehicles::f2_seats::Seats};
    let mut store=crate::ai::owned::tests::empty_world();let mut pool=Controls::new(1);let mut entities=Entities::new(8);let mut seats=Seats::default();
    let unit=store.unit.insert(Unit {generation:4.,..Default::default()});
    let hull=store.vehicle.insert(Vehicle {seats:vec![Default::default()],..Default::default()});
    let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:4.,vehicle:Some(hull),..Default::default()});
    store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
    for index in [-1.,0.5,f64::NAN,f64::INFINITY,u32::MAX as f64,1.] {
        store.seat_lease.get_mut(lease).unwrap().index=index;
        assert!(pool.construct(&store,&mut seats,&mut entities,lease,None).is_none());
    }
    store.seat_lease.get_mut(lease).unwrap().index=0.;
    store.seat_lease.get_mut(lease).unwrap().unit=Some(Ref::new(unit.slot,unit.gen+1));
    assert!(pool.construct(&store,&mut seats,&mut entities,lease,None).is_none());
    store.seat_lease.get_mut(lease).unwrap().unit=Some(unit);
    store.seat_lease.get_mut(lease).unwrap().generation=5.;
    assert!(pool.construct(&store,&mut seats,&mut entities,lease,None).is_none());
    store.seat_lease.get_mut(lease).unwrap().generation=4.;
    store.seat_lease.get_mut(lease).unwrap().vehicle=Some(Ref::new(hull.slot,hull.gen+1));
    assert!(pool.construct(&store,&mut seats,&mut entities,lease,None).is_none());
    store.seat_lease.get_mut(lease).unwrap().vehicle=Some(hull);
    let control=pool.construct(&store,&mut seats,&mut entities,lease,None).unwrap();
    assert!(pool.construct(&store,&mut seats,&mut entities,lease,None).is_none());
    assert!(pool.read(control).is_some());
}
