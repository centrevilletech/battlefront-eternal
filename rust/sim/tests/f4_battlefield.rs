//! Battlefield deaths read canonical unit/hull state after each synchronous
//! service call, including a hull kill that already killed its passenger.
use bfe_sim::ai::nav::{boundary::Boundary, math::Vec3};
use bfe_sim::battle::{battlefield::{Battlefield, BattlefieldWorld}, world::Body};
use bfe_sim::gen::{Character, SeatLease, Soldier, Store, Unit, UnitBody, Vehicle};
use bfe_sim::store::Ref;
#[derive(Default)]
struct World { droid: Option<[f64; 3]>, events: Vec<&'static str> }
impl BattlefieldWorld for World {
    fn body(&mut self, s: &Store, r: Ref<Unit>) -> Body {
        let Some(u) = s.unit.get(r) else { return Body { removed: true, ..Body::default() } };
        let position = match &u.soldier { UnitBody::Soldier(b) => b.position, UnitBody::Droideka(b) => b.position };
        let vehicle = self.vehicle(s, r);
        Body { removed: u.removed, alive: u.alive, combat_dead: u.combat.dead,
            place: vehicle.and_then(|v| s.vehicle.get(v)).map_or(position, |v| v.position),
            vehicle, droid: self.droid }
    }
    fn vehicle(&mut self, s: &Store, r: Ref<Unit>) -> Option<Ref<Vehicle>> {
        s.unit.get(r)?.seat.and_then(|l| s.seat_lease.get(l)).and_then(|l| l.vehicle)
    }
    fn combat_dead(&mut self, s: &Store, r: Ref<Unit>) -> bool { s.unit.get(r).unwrap().combat.dead }
    fn kill(&mut self, s: &mut Store, r: Ref<Unit>) {
        self.events.push("unit"); s.unit.get_mut(r).unwrap().combat.dead = true;
    }
    fn kill_droid(&mut self, _: &mut Store, _: Ref<Unit>) { self.events.push("droid"); self.droid = None; }
    fn wreck(&mut self, s: &mut Store, r: Ref<Vehicle>) {
        self.events.push("hull"); s.vehicle.get_mut(r).unwrap().health = 0.0;
        for slot in 0..s.unit.slots() { if let Some((_, u)) = s.unit.at_mut(slot) { u.combat.dead = true; } }
    }
}
fn field() -> Battlefield {
    Battlefield { boundary: Boundary::new(&[vec![Vec3 { x: -5.0, y: 0.0, z: -5.0 },
        Vec3 { x: -5.0, y: 0.0, z: 5.0 }, Vec3 { x: 5.0, y: 0.0, z: 5.0 }, Vec3 { x: 5.0, y: 0.0, z: -5.0 }]]) }
}
fn setup() -> (Store, Ref<Character>, Ref<Unit>) {
    let mut s = Store::default();
    let u = s.unit.insert(Unit { alive: true, soldier: UnitBody::Soldier(Soldier { position: [20.0, 0.0, 0.0], ..Soldier::default() }), ..Unit::default() });
    let c = s.character.insert(Character { unit: Some(u), out_of_bounds: -1.0, ..Character::default() });
    s.battle.characters.push(c); (s, c, u)
}
#[test]
fn countdown_expires_strictly_below_zero_and_kills_droid_before_body() {
    let (mut s, c, _) = setup(); let f = field();
    let mut w = World { droid: Some([20.0, 0.0, 0.0]), ..World::default() };
    f.update(10.0, &mut s, &mut w);
    assert_eq!(s.character.get(c).unwrap().out_of_bounds, 0.0); assert!(w.events.is_empty());
    f.update(1.0 / 60.0, &mut s, &mut w); assert_eq!(w.events, ["droid", "unit"]);
}
#[test]
fn hull_death_is_visible_before_testing_whether_to_kill_its_passenger() {
    let (mut s, _, u) = setup(); let v = s.vehicle.insert(Vehicle { position: [20.0, 0.0, 0.0], health: 100.0, ..Vehicle::default() });
    let lease = s.seat_lease.insert(SeatLease { vehicle: Some(v), ..SeatLease::default() }); s.unit.get_mut(u).unwrap().seat = Some(lease);
    let mut w = World::default(); field().update(11.0, &mut s, &mut w);
    assert_eq!(w.events, ["hull"]); assert_eq!(s.vehicle.get(v).unwrap().health, 0.0);
}
#[test]
fn inside_hull_uses_its_position_and_no_boundary_has_no_countdown() {
    let (mut s, c, u) = setup(); let v = s.vehicle.insert(Vehicle { position: [0.0; 3], ..Vehicle::default() });
    let lease = s.seat_lease.insert(SeatLease { vehicle: Some(v), ..SeatLease::default() }); s.unit.get_mut(u).unwrap().seat = Some(lease);
    let mut w = World::default(); field().update(11.0, &mut s, &mut w);
    assert_eq!(s.character.get(c).unwrap().out_of_bounds, -1.0); assert!(w.events.is_empty());
    s.unit.get_mut(u).unwrap().seat = None;
    Battlefield::default().update(11.0, &mut s, &mut w);
    assert_eq!(s.character.get(c).unwrap().out_of_bounds, -1.0); assert!(w.events.is_empty());
}
