use bfe_sim::gen::*;
use bfe_sim::store::{Nullable, Ref, StrId};
use bfe_sim::units::{ownership::{Ownership, Stale}, registry::Exclude, separate::{self, Pushers, Scope}, shapes::{Bounds, HitShape}, world::{CastHit, UnitsWorld}};
use three::{Matrix4, Vector3};

#[derive(Default)]
struct World { exposed: bool, rider_samples: usize, held_samples: usize, releases: usize }
impl World {
    fn shapes(&mut self, out: &mut [HitShape]) -> usize {
        let points = bfe_sim::rig::rider::RiderPoints {
            head: Vector3::new(0.0, 1.6, 0.0), neck: Vector3::new(0.0, 1.4, 0.0),
            pelvis: Vector3::new(0.0, 0.9, 0.0), ..Default::default()
        };
        let mut shapes = [bfe_sim::rig::rider::HitShape::default(); 10];
        let n = bfe_sim::rig::rider::rider_hit_shapes(&points, &Matrix4::default(), Some(1.0), &mut shapes);
        for i in 0..n { out[i].set_rider(&shapes[i]); }
        n
    }
}
impl UnitsWorld for World {
    fn cast(&mut self, _: &Vector3, _: &Vector3, _: f64) -> Option<CastHit> { None }
    fn visit_gathered(&mut self, _: u32, _: &Vector3, _: f64, _: usize,
        _: &mut dyn FnMut(bool, &[Vector3; 3])) {}
    fn seat_exposed(&mut self, _: u32, _: &Unit) -> bool { self.exposed }
    fn rider_shapes(&mut self, _: u32, _: &Unit, out: &mut [HitShape]) -> usize { self.rider_samples += 1; self.shapes(out) }
    fn held_shapes(&mut self, _: u32, _: &Unit, out: &mut [HitShape]) -> usize { self.held_samples += 1; self.shapes(out) }
    fn target_alive(&mut self, _: u32) -> bool { true }
    fn target_dead(&mut self, _: u32) -> bool { false }
    fn target_sphere(&mut self, _: u32) -> Bounds { Bounds { center: [100.0; 3], radius: 1.0 } }
    fn move_soldier(&mut self, _: u32, u: &mut Unit, h: f64) {
        if let UnitBody::Soldier(s) = &mut u.soldier { s.position[2] += u.input.forward * h; }
    }
    fn damage(&mut self, _: u32, u: &mut Unit, _: f64) { u.dead = true; u.alive = false; }
    fn aim(&mut self, _: u32, _: &mut Unit, _: f64) {}
    fn has_remote_step(&mut self, _: u32, _: &Unit) -> bool { false }
    fn remote_step(&mut self, _: u32, _: &mut Unit, _: f64, _: bool) {}
    fn combat_reset(&mut self, _: u32, u: &mut Unit) { u.combat.dead = false; }
    fn combat_update(&mut self, _: u32, _: &mut Unit, _: f64, _: f64) {}
    fn flush_muzzles(&mut self, _: u32, _: &mut Unit) {}
    fn clear_charge(&mut self, _: u32, _: &mut Unit) {}
    fn death_blast(&mut self, _: u32, _: &mut Unit) {}
    fn blow_apart(&mut self, _: u32, _: &mut Unit, _: f64) {}
    fn let_go_of_character(&mut self, _: u32, _: &mut Unit) {}
    fn release(&mut self, _: u32, _: &Unit) { self.releases += 1; }
}

fn soldier() -> Unit {
    Unit {
        class_name: StrId(7), root_scale: 1.0, alive: true,
        spheres_tick: -1.0, shapes: vec![HitShape::default(); 2],
        controls: Controls { was: [0.0; 2], pressed: vec![false; 2] },
        soldier: UnitBody::Soldier(Soldier { max_turn: 1.0, tick_length: 1.0 / 60.0, pitch_limits: [-1.4, 1.4], ..Default::default() }),
        ..Default::default()
    }
}
fn store() -> Store { let mut s = Store::default(); s.units.next_id = 1.0; s }
fn body(u: &mut Unit) -> &mut Soldier { let UnitBody::Soldier(s) = &mut u.soldier else { panic!() }; s }

#[test]
fn publish_only_changed_rows_without_observing_shapes() {
    let mut store = store(); let mut owner = Ownership::default();
    let a = owner.build(&mut store, soldier()); let b = owner.build(&mut store, soldier());
    let mut out = Vec::new(); owner.publish(&store, &mut out);
    assert_eq!(out.len(), 2);
    owner.team(&mut store, &b, 2.0).unwrap(); owner.team(&mut store, &b, 3.0).unwrap();
    assert_eq!(store.unit.get(b.reference()).unwrap().combat.team, 3.0);
    owner.publish(&store, &mut out);
    assert_eq!(out.len(), 1); assert_eq!(out[0].key, b); assert_eq!(out[0].team, 3.0);
    assert_eq!(store.unit.get(a.reference()).unwrap().spheres_tick, -1.0);
    owner.publish(&store, &mut out); assert!(out.is_empty());
}

#[test]
fn pooled_respawn_keeps_the_slot_and_rejects_the_previous_life() {
    let mut store = store(); let mut owner = Ownership::default(); let mut world = World::default();
    let first = owner.build(&mut store, soldier());
    let life = owner.spawn(&mut store, &F3SpawnCommand { key: first.clone(), position: [1.0, 2.0, 3.0], yaw: 0.0 }, &mut world).unwrap();
    assert_eq!(owner.team(&mut store, &first, 9.0), Err(Stale::Life));
    owner.remove(&mut store, &life, &mut world).unwrap(); owner.removed(&store, &life).unwrap();
    assert_eq!(world.releases, 1);
    let free = owner.acquire(&store, StrId(7)).unwrap(); assert_eq!(free, life);
    assert!(owner.acquire(&store, StrId(7)).is_none());
    let next = owner.spawn(&mut store, &F3SpawnCommand { key: free, position: [4.0, 5.0, 6.0], yaw: 0.0 }, &mut world).unwrap();
    assert_eq!(next.reference(), life.reference()); assert_eq!(next.life, life.life + 1.0);
    assert_eq!(owner.team(&mut store, &life, 9.0), Err(Stale::Life));
    assert_eq!(store.units.list, vec![Member::Unit(next.reference())]);
}

#[test]
fn retired_slot_reuse_rejects_old_commands_and_keeps_build_order() {
    let mut store = store(); let mut owner = Ownership::default();
    let old = owner.build(&mut store, soldier()); let kept = owner.build(&mut store, soldier());
    owner.retire(&mut store, &old).unwrap();
    let replacement = store.unit.insert(soldier());
    assert_eq!(old.slot, replacement.slot); assert_ne!(old.generation, replacement.gen);
    let new = owner.adopt(&store, replacement).unwrap();
    assert_eq!(owner.built(), &[kept.reference(), new.reference()]);
    assert_eq!(owner.team(&mut store, &old, 99.0), Err(Stale::Slot));
    assert_eq!(store.unit.get(new.reference()).unwrap().team, 0.0);
}

#[test]
fn ordered_ties_use_actual_slab_generations_and_corpses_stop_rounds() {
    let mut store = store(); let mut owner = Ownership::default(); let mut world = World::default();
    let a = owner.build(&mut store, soldier());
    store.units.list.push(Member::VehicleTarget(Ref::new(90, 7)));
    let b = owner.build(&mut store, soldier());
    let origin = Vector3::new(0.0, 0.8, -5.0); let direction = Vector3::new(0.0, 0.0, 1.0);
    let hit = store.units.raycast(&mut store.unit, &origin, &direction, 10.0, Exclude::Nobody, None, false, &mut world).unwrap();
    assert_eq!(hit.unit, a.slot);
    store.units.list.swap(0, 2);
    let hit = store.units.raycast(&mut store.unit, &origin, &direction, 10.0, Exclude::Nobody, None, false, &mut world).unwrap();
    assert_eq!(hit.unit, b.slot);
    let u = store.unit.get_mut(b.reference()).unwrap(); u.alive = false; u.dead = true;
    let mut near = Vec::new();
    store.units.query(&mut store.unit, &Vector3::default(), 10.0, &mut near, None, &mut world);
    assert_eq!(near, vec![Member::Unit(a.reference())]);
    let hit = store.units.raycast(&mut store.unit, &origin, &direction, 10.0, Exclude::Nobody, None, false, &mut world).unwrap();
    assert_eq!(hit.unit, b.slot);
    store.unit.get_mut(b.reference()).unwrap().removed = true;
    let hit = store.units.raycast(&mut store.unit, &origin, &direction, 10.0, Exclude::Nobody, None, false, &mut world).unwrap();
    assert_eq!(hit.unit, a.slot);
}

#[test]
fn death_and_blast_keep_shapes_stale_until_the_original_tick_invalidation() {
    let mut u = soldier(); let mut world = World::default();
    u.update_spheres(1, &mut world); let standing = u.shapes[0].center;
    u.dead = true; u.alive = false; body(&mut u).position[0] = 10.0;
    u.sphere(1, &mut world); assert_eq!(u.shapes[0].center, standing);
    u.tick_count += 1.0; u.sphere(1, &mut world);
    assert_ne!(u.shapes[0].center, standing); assert_eq!(u.shapes[0].r#type, ShapeType::Box);
}

#[test]
fn rider_and_held_shapes_are_sampled_lazily_and_hidden_seats_are_skipped() {
    let mut store = store(); let mut owner = Ownership::default(); let mut world = World::default();
    let key = owner.build(&mut store, soldier());
    let u = store.unit.get_mut(key.reference()).unwrap(); u.seat = Some(Ref::new(1, 4));
    let origin = Vector3::new(0.0, 1.6, -5.0); let direction = Vector3::new(0.0, 0.0, 1.0);
    assert!(store.units.raycast(&mut store.unit, &origin, &direction, 10.0, Exclude::Nobody, None, true, &mut world).is_none());
    world.exposed = true;
    assert!(store.units.raycast(&mut store.unit, &origin, &direction, 10.0, Exclude::Nobody, None, true, &mut world).is_some());
    assert_eq!(world.rider_samples, 1);
    let u = store.unit.get_mut(key.reference()).unwrap(); u.sphere(key.slot, &mut world); assert_eq!(world.rider_samples, 1);
    u.seat = None; u.held = Nullable::Value(Held { frame: Nullable::Value(Matrix4::default().elements), ..Default::default() });
    u.tick_count += 1.0; u.sphere(key.slot, &mut world); u.sphere(key.slot, &mut world);
    assert_eq!(world.held_samples, 1); assert_eq!(u.shape_count, 6.0);
}

#[test]
fn separation_uses_mixed_registry_order_and_invalidates_only_pushed_bodies() {
    let mut store = store(); let mut owner = Ownership::default();
    let a = owner.build(&mut store, soldier()); let b = owner.build(&mut store, soldier());
    let c = owner.build(&mut store, soldier()); body(store.unit.get_mut(c.reference()).unwrap()).position[0] = 50.0;
    let mut pushers = Pushers::default();
    pushers.reserve(3);
    separate::separate(&store.units, &mut store.unit, &Scope { player: None, player_out: false, global: true, camera: Vector3::default() }, &mut pushers);
    assert_eq!(pushers.changed().collect::<Vec<_>>(), [a.reference(), b.reference()]);
    let a = store.unit.get(a.reference()).unwrap(); let b = store.unit.get(b.reference()).unwrap();
    assert_ne!(a.position(), b.position()); assert_eq!(a.tick_count, 1.0); assert_eq!(b.tick_count, 1.0);
    assert_eq!(store.unit.get(c.reference()).unwrap().tick_count, 0.0);
}

#[test]
fn native_ai_queries_keep_mixed_registry_order_filtering_and_lazy_spheres() {
    use bfe_sim::ai::{owned::{entities::{Entities,Entity},registry::{Queries,TargetTeams}},combat::world::QueryFilter};
    struct Teams;
    impl TargetTeams for Teams {fn team(&self,_:Member)->f64 {2.}}
    let mut store=store();let mut owner=Ownership::default();let mut world=World::default();
    let mut queries=Queries::new(4,2);let mut entities=Entities::new(8);
    let first=owner.build(&mut store,soldier());store.unit.get_mut(first.reference()).unwrap().team=1.;
    let a=entities.register(Entity::Unit(first.reference()));
    let target_id=store.units.add(|id|Member::VehicleTarget(bfe_sim::store::Ref::new(id,1)));
    let target=Member::VehicleTarget(bfe_sim::store::Ref::new(target_id,1));
    let hull=store.vehicle.insert(bfe_sim::gen::Vehicle::default());let h=entities.register(Entity::HullEntry(hull));queries.target(target,h);
    let second=owner.build(&mut store,soldier());store.unit.get_mut(second.reference()).unwrap().team=2.;
    let b=entities.register(Entity::Unit(second.reference()));
    let filter=QueryFilter::Hostile {except:a,team:1.};
    let result=queries.query(&mut store,&mut world,&Teams,&entities,&Vector3::default(),500.,&mut |obj,team|filter.keeps(obj,team,&mut |a,b|if a==b {1.}else{-1.}));
    assert_eq!(&result[..],&[h,b]);
    // A nested living scan takes a different reserved scope and leaves the
    // outer result intact. Living does not refresh either body's geometry.
    let before=store.unit.get(first.reference()).unwrap().spheres_tick;
    let nested=queries.living(&store,&mut world,&entities);assert_eq!(&nested[..],&[a,h,b]);assert_eq!(&result[..],&[h,b]);
    assert_eq!(store.unit.get(first.reference()).unwrap().spheres_tick,before);
    drop(nested);drop(result);
    store.unit.get_mut(second.reference()).unwrap().alive=false;
    assert_eq!(&queries.living(&store,&mut world,&entities)[..],&[a,h]);
    entities.retire(h);assert_eq!(queries.object(&entities,target),None);
    let other_life=Member::VehicleTarget(bfe_sim::store::Ref::new(target_id,2));assert_eq!(queries.object(&entities,other_life),None);
}

#[test]
fn native_ai_geometry_observes_body_spheres_and_current_seat_rigs_at_distinct_points() {
    use bfe_sim::ai::{owned::{self,arsenals::Mounted,entities::Entity,rows::Rows,state::{State,Capacity}},nav::{Nav,PlannerOptions},combat::world::WeaponSettings};
    struct NoMount;
    impl Mounted for NoMount {
        fn count(&self,_:&bfe_sim::gen::Store,_:owned::seats::Source,_:usize)->Option<usize> {panic!("geometry must not read weapons")}
        fn read<R>(&self,_:&bfe_sim::gen::Store,_:owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&Weapon)->R)->Option<R> {panic!("geometry must not read weapons")}
        fn edit<R>(&mut self,_:&bfe_sim::gen::Store,_:owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&mut WeaponSettings)->R)->Option<R> {panic!("geometry must not edit weapons")}
    }
    #[derive(Default)]struct Hulls {reads:usize}
    impl owned::observe::Hulls for Hulls {
        fn origin(&mut self,s:&mut Store,_:&mut bfe_sim::vehicles::f2_seats::Seats,h:Ref<Vehicle>,seat:usize)->Option<Vector3> {self.reads+=1;let p=s.vehicle.get(h)?.position;Some(Vector3::new(p[0],20.+seat as f64,p[2]))}
        fn forward(&mut self,s:&mut Store,_:&mut bfe_sim::vehicles::f2_seats::Seats,h:Ref<Vehicle>,seat:usize)->Option<Vector3> {self.reads+=1;s.vehicle.get(h)?;Some(Vector3::new(seat as f64,0.,1.))}
    }
    let mut state=State::new(Nav::empty(PlannerOptions::default()),false,Capacity {registry:8,characters:1,posts:0,hulls:1,buildings:0,controls:2,scopes:2,cameras:1});
    let mut store=Store::default();let body=store.unit.insert(soldier());
    store.unit.get_mut(body).unwrap().generation=3.;
    store.unit.get_mut(body).unwrap().combat.pivot=[5.,6.,7.];
    let unit=state.entities.register(Entity::Unit(body));
    let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:4.,position:[10.,0.,30.],center:[0.,2.,0.],bound_radius:9.,matrix:Matrix4::default().elements,seats:vec![VehicleSeat::default(),VehicleSeat::default()],..Default::default()});
    state.mounts.register(&store,&mut state.entities,hull,owned::mounts::HullClass {seats:vec![Default::default(),Default::default()],..Default::default()});
    let target=state.entities.find(Entity::HullEntry(hull)).unwrap();
    let control=Ref::new(2,3);
    let lease=store.seat_lease.insert(SeatLease {unit:Some(body),generation:3.,vehicle:Some(hull),index:1.,controllable:Some(control),..Default::default()});
    store.vehicle.get_mut(hull).unwrap().seats[1].lease=Some(lease);
    assert!(state.controls.register(&store,&mut state.entities,control,lease,AiLifeHandle {kind:KIND_VEHICLE.into(),slot:hull.slot,generation:hull.gen,life:4.},7.,Default::default(),"gunner".into(),None,owned::seats::Occupant::read(&store,lease).unwrap()));
    let gunner=state.entities.find(Entity::Control(control)).unwrap();
    let mut seats=Default::default();let live=bfe_sim::battle::live_phase::Live::new(0,0,0);
    let mut characters=bfe_sim::battle::commands::Commands::new(0);let mut changes=Ownership::default();let mut mounted=NoMount;let defs=Default::default();
    let mut rows=Rows {store:&mut store,seats:&mut seats,buildings:&live.buildings,characters:&mut characters,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
    let mut hulls=Hulls::default();let mut world=World {exposed:true,..Default::default()};
    assert_eq!(rows.origin(unit,&mut hulls),Some(Vector3::new(5.,6.,7.)));assert_eq!(hulls.reads,0);
    let original=rows.aim_point(unit,&mut owned::observe::Shapes(&mut world)).unwrap();assert_ne!(original,Vector3::new(5.,6.,7.));
    rows.store.unit.get_mut(body).unwrap().seat=Some(lease);
    for tick in 1..=120 {
        rows.store.vehicle.get_mut(hull).unwrap().position[0]=tick as f64;
        assert_eq!(rows.origin(unit,&mut hulls),Some(Vector3::new(tick as f64,21.,30.)));
        assert_eq!(rows.origin(gunner,&mut hulls),Some(Vector3::new(tick as f64,21.,30.)));
        assert_eq!(rows.origin(target,&mut hulls),Some(Vector3::new(tick as f64,20.,30.)));
        assert_eq!(rows.forward(unit,&mut hulls),Some(Vector3::new(1.,0.,1.)));
        let before=hulls.reads;
        assert_eq!(rows.aim_point(target,&mut owned::observe::Shapes(&mut world)),Some(Vector3::new(0.,2.,0.)));
        assert_eq!(rows.aim_point(gunner,&mut owned::observe::Shapes(&mut world)),Some(Vector3::new(0.,2.,0.)));
        assert_eq!(rows.aim_point(unit,&mut owned::observe::Shapes(&mut world)),Some(original),"same body tick retains its lazy sphere");
        assert_eq!(hulls.reads,before);assert_eq!(world.rider_samples,0);
    }
    rows.store.unit.get_mut(body).unwrap().tick_count=1.;
    rows.aim_point(unit,&mut owned::observe::Shapes(&mut world)).unwrap();assert_eq!(world.rider_samples,1);
    rows.aim_point(unit,&mut owned::observe::Shapes(&mut world)).unwrap();assert_eq!(world.rider_samples,1);
    rows.store.vehicle.get_mut(hull).unwrap().target_generation=5.;
    let before=hulls.reads;
    assert!(rows.origin(gunner,&mut hulls).is_none());assert!(rows.forward(gunner,&mut hulls).is_none());assert!(rows.aim_point(gunner,&mut owned::observe::Shapes(&mut world)).is_none());
    assert_eq!(hulls.reads,before);
    rows.store.unit.remove(body).unwrap();let replacement=rows.store.unit.insert(soldier());assert_eq!(replacement.slot,body.slot);
    assert!(rows.origin(unit,&mut hulls).is_none());assert!(rows.aim_point(unit,&mut owned::observe::Shapes(&mut world)).is_none());
}
