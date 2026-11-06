//! Canonical statistics readers, including mount and corpse distinctions.
use bfe_sim::battle::observation::{Mount, Mounts, Observations};
use bfe_sim::gen::*;
use bfe_sim::store::{DefId, Ref, StrId};
#[derive(Default)]
struct Native { mount: Option<Mount>, calls: usize }
impl Mounts for Native {
    fn mount(&mut self, s: &Store, r: Ref<SeatLease>) -> Option<Mount> {
        assert!(!s.seat_lease.get(r).unwrap().released); self.calls += 1; self.mount
    }
}
fn setup() -> (Store, Observations, Ref<Unit>, Native) {
    let mut s = Store::default(); let u = s.unit.insert(Unit { alive: true, class_def: DefId(1),
        soldier: UnitBody::Soldier(Soldier { position: [1.0, 2.0, 3.0], ..Soldier::default() }),
        combat: Combat { health: 750000.0, max_health: 1000000.0, weapons: vec![
            Weapon { def: DefId(0), ..Weapon::default() }, Weapon { def: DefId(1), ..Weapon::default() },
            Weapon { def: DefId(2), ..Weapon::default() }], selected: [1.0, 0.0], ..Combat::default() }, ..Unit::default() });
    let mut facts = Observations::default(); facts.class(F4StatsClass { def: DefId(1), label: Some(StrId(10)) });
    for i in 0..3 { facts.weapon(F4StatsWeapon { def: DefId(i), primary: i != 1, key: Some(StrId(20 + i)), labels: vec![Some(StrId(30 + i)), Some(StrId(90 + i))] }); }
    (s, facts, u, Native::default())
}
#[test]
fn on_foot_reads_the_selected_primary_and_full_precision_hero_health() {
    let (mut s, facts, u, mut native) = setup();
    let seen = facts.observe(&s, Some(u), &mut native).unwrap(); assert_eq!(seen.health, Some(0.75));
    assert_eq!(seen.position, Some([1.0, 2.0, 3.0])); assert_eq!(seen.class_label, Some(StrId(10)));
    assert_eq!(seen.weapon, Some((Some(StrId(22)), Some(StrId(32))))); assert_eq!(native.calls, 0);
    for selected in [-1.0, 0.5, f64::NAN, f64::INFINITY, 2.0] {
        s.unit.get_mut(u).unwrap().combat.selected[0] = selected;
        assert_eq!(facts.observe(&s, Some(u), &mut native).unwrap().weapon, None);
    }
}
#[test]
fn active_mount_supplies_the_place_and_first_weapon_and_released_lease_uses_the_hand() {
    let (mut s, facts, u, mut native) = setup(); let lease = s.seat_lease.insert(SeatLease::default()); s.unit.get_mut(u).unwrap().seat = Some(lease);
    native.mount = Some(Mount { position: [7.0, 8.0, 9.0], weapon: Some((DefId(1), 0.0)), vehicle: Some((Some(StrId(40)), Some(StrId(41)))) });
    let seen = facts.observe(&s, Some(u), &mut native).unwrap(); assert_eq!(seen.position, Some([7.0, 8.0, 9.0]));
    assert_eq!(seen.weapon, Some((Some(StrId(21)), Some(StrId(31))))); assert_eq!(seen.vehicle, native.mount.unwrap().vehicle);
    native.mount.as_mut().unwrap().weapon = None;
    assert_eq!(facts.observe(&s, Some(u), &mut native).unwrap().weapon, None);
    s.seat_lease.get_mut(lease).unwrap().released = true;
    assert_eq!(facts.observe(&s, Some(u), &mut native).unwrap().weapon, Some((Some(StrId(22)), Some(StrId(32)))));
    assert_eq!(native.calls, 2);
}
#[test]
fn a_corpse_keeps_weapon_and_place_but_removed_and_absent_bodies_have_no_observation() {
    let (mut s, facts, u, mut native) = setup(); s.unit.get_mut(u).unwrap().combat.dead = true;
    let seen = facts.observe(&s, Some(u), &mut native).unwrap(); assert_eq!(seen.health, None); assert!(seen.weapon.is_some());
    s.unit.get_mut(u).unwrap().removed = true; assert_eq!(facts.observe(&s, Some(u), &mut native), None);
    assert_eq!(facts.observe(&s, None, &mut native), None);
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn canonical_body_and_weapon_reads_allocate_nothing() {
    let (s, facts, u, mut native) = setup(); allocation::start();
    for _ in 0..1200 { assert_eq!(facts.observe(&s, Some(u), &mut native).unwrap().health, Some(0.75)); }
    assert_eq!(allocation::stop(), 0);
}
#[test]
fn a_charged_weapon_keeps_its_root_key_and_observes_its_current_level_label() {
    let (mut s, facts, u, mut native) = setup(); s.unit.get_mut(u).unwrap().combat.weapons[2].level = 1.0;
    let seen = facts.observe(&s, Some(u), &mut native).unwrap();
    assert_eq!(seen.weapon, Some((Some(StrId(22)), Some(StrId(92)))));
}
#[test]
fn hull_favourite_facts_cannot_follow_a_recycled_slab_slot() {
    let (mut s, mut facts, _, _) = setup(); let old = s.vehicle.insert(Vehicle { name: StrId(40), ..Vehicle::default() });
    facts.hull(F4StatsHull { hull: old, label: Some(StrId(41)) });
    assert_eq!(facts.vehicle_favourite(&s, old), Some((Some(StrId(40)), Some(StrId(41)))));
    s.vehicle.remove(old); let new = s.vehicle.insert(Vehicle { name: StrId(50), ..Vehicle::default() });
    assert_eq!(old.slot, new.slot); assert_eq!(facts.vehicle_favourite(&s, new), None);
}
