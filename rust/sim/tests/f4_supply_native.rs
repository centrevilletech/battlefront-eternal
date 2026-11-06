//! Canonical station phase readers, independent of game content.
use bfe_sim::battle::{props, supply::{Key, Prepared, Targets, World}};
use bfe_sim::gen::*;
use bfe_sim::store::{DefId, Nullable, Ref};
use three::Vector3;
#[derive(Default)]
struct Notices { units: usize, hulls: usize, served: usize, sounds: usize, gave: u8 }
impl World for Notices {
    fn hull_health(&mut self, s: &mut Store, r: Ref<Vehicle>, life: f64, health: f64) {
        let v = s.vehicle.get_mut(r).unwrap(); assert_eq!(v.target_generation, life); v.health = health;
    }
    fn hull_ammo(&mut self, _: &mut Store, _: Ref<Vehicle>, _: f64, _: f64) -> bool { false }
    fn changed(&mut self, _: &mut Store, key: Key) { match key { Key::Unit {..} => self.units += 1, Key::Hull {..} => self.hulls += 1 } }
    fn served(&mut self, _: &props::Station, _: Key, _: DefId, position: Vector3, gave: u8) {
        assert_eq!(position, Vector3::default()); self.served += 1; self.gave = gave;
    }
    fn sound(&mut self, _: &props::Station, _: Option<Key>) { self.sounds += 1; }
}
fn station() -> props::Station {
    props::Station { building: props::Building { state: props::State::Built, health: 100.0, add_health: 0.0,
        last_attacker: props::Identity::Null, target_generation: None }, active_mode: false, active: 0.0, pulse: 0.0,
        served_player: false, served_health: false, served_ammo: false }
}
fn supply() -> props::Supply {
    props::Supply { position: Vector3::default(), radius: 10.0, delay: 1.0,
        health: 10.0, ammo: 5.0, vehicle_health: 20.0, vehicle_ammo: 3.0 }
}
fn unit(s: &mut Store) -> Ref<Unit> {
    s.unit.insert(Unit { generation: 1.0, alive: true, class_def: DefId(0),
        combat: Combat { health: 50.0, max_health: 100.0, weapons: vec![Weapon { def: DefId(0), level: 1.0,
            clip: 1.0, per_shot: 1.0, reserve: 2.0, ..Weapon::default() }], ..Combat::default() },
        ..Unit::default() })
}
fn prepared() -> Prepared { let mut p = Prepared::default(); p.class(DefId(0), true); p.weapon(DefId(0), vec![2.0, 10.0]); p }
#[test]
fn local_viewpoint_keeps_soldier_rates_while_held_and_stationary_mounted() {
    let mut s = Store::default(); let r = unit(&mut s);
    s.unit.get_mut(r).unwrap().held = Nullable::Value(Held::default());
    s.unit.get_mut(r).unwrap().seat = Some(Ref::new(0, 1));
    let mut targets = Targets::new(1, 0); targets.begin(&s, Some(r), false, &[r], &[]);
    let mut station = station(); let mut notices = Notices::default();
    targets.pulse(&mut station, 1.0, &supply(), &mut s, &prepared(), &mut notices);
    assert_eq!(s.unit.get(r).unwrap().combat.health, 60.0);
    assert_eq!(s.unit.get(r).unwrap().combat.weapons[0].reserve, 7.0);
    assert_eq!(notices.units, 1); assert_eq!(notices.served, 1); assert_eq!(notices.gave, 3); assert!(station.served_player);
    targets.begin(&s, Some(r), true, &[r], &[]);
    targets.pulse(&mut station, 1.0, &supply(), &mut s, &prepared(), &mut notices);
    assert_eq!(s.unit.get(r).unwrap().combat.health, 60.0); assert_eq!(notices.served, 1);
}
#[test]
fn frozen_membership_reads_current_positions_and_rejects_reused_lives() {
    let mut s = Store::default(); let r = unit(&mut s);
    s.unit.get_mut(r).unwrap().soldier = UnitBody::Soldier(Soldier { position: [100.0, 0.0, 0.0], ..Soldier::default() });
    let mut targets = Targets::new(1, 0); targets.begin(&s, None, false, &[r], &[]);
    s.unit.get_mut(r).unwrap().soldier = UnitBody::Soldier(Soldier::default());
    let mut station = station(); let mut notices = Notices::default();
    targets.pulse(&mut station, 1.0, &supply(), &mut s, &prepared(), &mut notices);
    assert_eq!(s.unit.get(r).unwrap().combat.health, 70.0); // Factory vehicle-health class.
    assert_eq!(s.unit.get(r).unwrap().combat.weapons[0].reserve, 5.0);
    s.unit.get_mut(r).unwrap().generation += 1.0;
    targets.pulse(&mut station, 1.0, &supply(), &mut s, &prepared(), &mut notices);
    assert_eq!(s.unit.get(r).unwrap().combat.health, 70.0); assert_eq!(notices.served, 1);
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn first_and_repeated_native_membership_and_pulses_allocate_nothing() {
    let mut s = Store::default(); let r = unit(&mut s);
    let v = s.vehicle.insert(Vehicle { alive: true, target_generation: 1.0, radius: 1.0, health: 50.0, max_health: 100.0, ..Vehicle::default() });
    let mut targets = Targets::new(1, 1); let mut station = station(); let mut prepared = prepared(); prepared.hull(v,true); let supply = supply(); let mut notices = Notices::default();
    allocation::start();
    for _ in 0..1200 {
        s.unit.get_mut(r).unwrap().combat.health = 50.0; s.unit.get_mut(r).unwrap().combat.weapons[0].reserve = 2.0;
        s.vehicle.get_mut(v).unwrap().health = 50.0;
        targets.begin(&s, Some(r), false, &[], &[v]);
        targets.pulse(&mut station, 1.0, &supply, &mut s, &prepared, &mut notices);
    }
    assert_eq!(allocation::stop(), 0); assert_eq!(notices.units, 1200); assert_eq!(notices.hulls, 1200);
}

#[test]
fn animal_hulls_keep_soldier_rates_in_the_vehicle_pass() {
    let mut s=Store::default();
    let v=s.vehicle.insert(Vehicle{alive:true,target_generation:1.,radius:1.,health:50.,max_health:100.,..Vehicle::default()});
    let mut targets=Targets::new(0,1);targets.begin(&s,None,false,&[],&[v]);
    let mut prepared=prepared();prepared.hull(v,false);
    let mut rates=supply();rates.health=0.;rates.ammo=0.;
    targets.pulse(&mut station(),1.,&rates,&mut s,&prepared,&mut Notices::default());
    assert_eq!(s.vehicle.get(v).unwrap().health,50.);
    rates.health=10.;
    targets.pulse(&mut station(),1.,&rates,&mut s,&prepared,&mut Notices::default());
    assert_eq!(s.vehicle.get(v).unwrap().health,60.);
}
