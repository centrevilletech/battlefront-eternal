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
