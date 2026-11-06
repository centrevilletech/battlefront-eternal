//! Station pulses mutate canonical bodies without a target/weapon snapshot.
use bfe_sim::battle::props::{self, Station, Supplies, Supply, SupplyCandidate};
use bfe_sim::gen::*;
use bfe_sim::store::{Ref, StrId};
use three::Vector3;
#[derive(Clone, Copy, Debug, PartialEq)]
enum Key { Unit(Ref<Unit>), Hull(Ref<Vehicle>) }
struct Native { s: Store, units: [Ref<Unit>; 2], hulls: [Ref<Vehicle>; 2], events: Vec<&'static str>, reads: usize }
impl Supplies for Native {
    type Key = Key;
    fn soldiers(&self) -> usize { self.units.len() }
    fn soldier(&mut self, i: usize) -> SupplyCandidate<Key> {
        self.reads += 1; let r = self.units[i]; let u = self.s.unit.get(r).unwrap();
        SupplyCandidate { key: Key::Unit(r), position: u.position(), alive: u.alive, player: i == 0, radius: 1.0 }
    }
    fn vehicles(&self) -> usize { self.hulls.len() }
    fn vehicle(&mut self, i: usize) -> SupplyCandidate<Key> {
        self.reads += 1; let r = self.hulls[i]; let v = self.s.vehicle.get(r).unwrap(); let p = v.position;
        SupplyCandidate { key: Key::Hull(r), position: Vector3::new(p[0], p[1], p[2]), alive: v.alive, player: false, radius: v.radius }
    }
    fn serve(&mut self, key: Key, supply: &Supply) -> u8 {
        match key {
            Key::Unit(r) => {
                let u = self.s.unit.get_mut(r).unwrap(); let mut gave = 0;
                if let Some(next) = props::supply_health(u.combat.health, u.combat.max_health, supply.health) { u.combat.health = next; gave |= 1; }
                for w in &mut u.combat.weapons {
                    if let Some(next) = props::supply_reserve(w.bottomless, 10.0, w.clip, w.per_shot, w.reserve, supply.ammo) { w.reserve = next; gave |= 2; }
                }
                gave
            }
            Key::Hull(r) => {
                let v = self.s.vehicle.get_mut(r).unwrap();
                if let Some(next) = props::supply_health(v.health, v.max_health, supply.vehicle_health) { v.health = next; 1 } else { 0 }
            }
        }
    }
    fn changed(&mut self, key: Key) { self.events.push(match key { Key::Unit(_) => "unit", Key::Hull(_) => "hull" }); }
    fn served(&mut self, station: &Station, key: Key, gave: u8) {
        let Key::Unit(r) = key else { panic!("hulls have no foot gain callback") };
        assert!(station.served_player); assert_eq!(gave, 3);
        let u = self.s.unit.get(r).unwrap(); assert_eq!(u.combat.health, 60.0); assert_eq!(u.combat.weapons[0].reserve, 7.0);
        self.events.push("served");
    }
    fn sound(&mut self, station: &Station, first: Option<Key>) {
        assert_eq!(first, Some(Key::Unit(self.units[0]))); assert!(station.served_health && station.served_ammo);
        self.events.push("sound");
    }
}
fn setup() -> (Station, Supply, Native) {
    let mut s = Store::default();
    let units = [9.0, 10.0].map(|x| s.unit.insert(Unit { alive: true,
        soldier: UnitBody::Soldier(Soldier { position: [x, 0.0, 0.0], ..Soldier::default() }),
        combat: Combat { health: 50.0, max_health: 100.0, weapons: vec![Weapon { clip: 1.0, per_shot: 1.0, reserve: 2.0, ..Weapon::default() }], ..Combat::default() },
        ..Unit::default() }));
    let hulls = [11.0, 12.0].map(|x| s.vehicle.insert(Vehicle { alive: true, radius: 4.0, position: [x, 0.0, 0.0], health: 30.0, max_health: 100.0, name: StrId(0), ..Vehicle::default() }));
    let station = Station { building: props::Building { state: props::State::Built, health: 100.0, add_health: 0.0,
        last_attacker: props::Identity::Undefined, target_generation: None }, active_mode: false, active: 0.0, pulse: 0.0,
        served_player: false, served_health: false, served_ammo: false };
    let supply = Supply { position: Vector3::default(), radius: 10.0, delay: 1.0, health: 10.0, ammo: 5.0, vehicle_health: 20.0, vehicle_ammo: 0.0 };
    (station, supply, Native { s, units, hulls, events: Vec::with_capacity(4), reads: 0 })
}
#[test]
fn canonical_supply_preserves_foot_and_hull_edges_and_notifies_after_writes() {
    let (mut station, supply, mut native) = setup(); station.pulse(1.0, &supply, &mut native);
    assert_eq!(native.events, ["unit", "served", "hull", "sound"]); assert_eq!(native.reads, 4);
    assert_eq!(native.s.unit.get(native.units[1]).unwrap().combat.health, 50.0);
    assert_eq!(native.s.vehicle.get(native.hulls[0]).unwrap().health, 50.0);
    assert_eq!(native.s.vehicle.get(native.hulls[1]).unwrap().health, 30.0);
    assert!(station.active_mode); assert_eq!(station.active, 0.0);
}
#[test]
fn waiting_and_dead_stations_do_not_read_any_target() {
    let (mut station, supply, mut native) = setup(); station.pulse(0.5, &supply, &mut native);
    assert_eq!(native.reads, 0); assert!(native.events.is_empty());
    station.building.state = props::State::Destroyed; station.pulse(1.0, &supply, &mut native);
    assert_eq!(native.reads, 0); assert_eq!(station.pulse, 0.5);
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn first_and_repeated_canonical_pulses_allocate_no_phase_storage() {
    let (mut station, supply, mut native) = setup(); allocation::start();
    for _ in 0..1200 {
        native.events.clear(); let u = native.s.unit.get_mut(native.units[0]).unwrap(); u.combat.health = 50.0; u.combat.weapons[0].reserve = 2.0;
        native.s.vehicle.get_mut(native.hulls[0]).unwrap().health = 30.0;
        station.pulse(1.0, &supply, &mut native);
    }
    assert_eq!(allocation::stop(), 0); assert_eq!(native.events, ["unit", "served", "hull", "sound"]);
}
