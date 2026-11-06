//! Command posts observe canonical fleet changes through a whole mobile pass.
use bfe_sim::battle::{fleet_posts::{self, Pads}, mobile::{self, MobileEvent, Pad}, world::PostsWorld};
use bfe_sim::gen::*;
use bfe_sim::store::{Ref, StrId, Strings};
struct Native { pads: Pads }
impl PostsWorld for Native {
    fn capture_team(&mut self, team: f64) -> f64 { team }
    fn building_alive(&mut self, _: Ref<Building>) -> bool { unreachable!() }
    fn pad(&mut self, _: &Store, _: f64) -> Pad { panic!("native refresh must lend the prepared pad"); }
    fn with_pad(&mut self, s: &mut Store, slot: f64, visit: &mut dyn FnMut(&mut Store, &Pad)) { let pad = self.pads.get(s, slot as usize); visit(s, pad); }
    fn vehicle_position(&mut self, s: &Store, r: Ref<Vehicle>) -> [f64; 3] { s.vehicle.get(r).unwrap().position }
    fn class_spawn_world(&mut self, _: &Store, _: Ref<MobilePost>) -> Vec<[f64; 4]> { panic!("use retained points"); }
}
fn setup() -> (Store, Ref<MobilePost>, [Ref<Vehicle>; 2], Native) {
    let mut s = Store::default();
    let hulls = [1.0, 2.0].map(|x| s.vehicle.insert(Vehicle { alive: true, target_generation: 1.0, team: Some(1.0),
        position: [x, 0.0, 0.0], size: [3.0, 4.0, 0.0], ..Vehicle::default() }));
    let mut slot = SpawnSlot::default();
    slot.variants.entries.push((StrId(1), hulls[0])); slot.variants.entries.push((StrId(2), hulls[1])); slot.vehicle = Some(hulls[0]);
    s.vehicles.slots.push(slot);
    let birth = |vehicle| PsHull { vehicle, command: true, label: StrId(1), name: StrId(2), record: Some(PsClass {
        atk: Some(vec![3.0; 6]), def: Some(vec![4.0; 6]), value_bleed: Some(10.0), points: vec![[1.0, 0.0, 0.0, 0.0]; 8] }), ..PsHull::default() };
    let mut pads = Pads::default(); pads.adopt(0, PsPad { variants: hulls.into_iter().map(birth).collect(), vehicle: Some(birth(hulls[0])) });
    let mut p = mobile::new_mobile_post(0.0, &mut Strings::default()); pads.reserve_post(0, &mut p);
    let p = s.mobile_post.insert(p); s.mobile_posts.posts.push(p);
    (s, p, hulls, Native { pads })
}
#[test]
fn map_order_and_first_lease_are_read_from_the_canonical_fleet() {
    let (mut s, p, hulls, mut w) = setup(); mobile::report_all(&mut s, &mut w);
    assert_eq!(s.mobile_post.get(p).unwrap().vehicle, Some(hulls[1])); assert_eq!(s.mobile_post.get(p).unwrap().radius, 2.5);
    let lease = s.seat_lease.insert(SeatLease { team: 2.0, ..SeatLease::default() });
    s.vehicle.get_mut(hulls[1]).unwrap().seats.push(VehicleSeat { lease: Some(lease), ..VehicleSeat::default() });
    let mut events = Vec::with_capacity(2); mobile::events_into(&mut s, &mut w, &mut events);
    assert_eq!(events, [MobileEvent::Owner { post: p, from: 1.0, to: 2.0 }]);
    s.vehicle.get_mut(hulls[1]).unwrap().seats[0].lease = None;
    mobile::events_into(&mut s, &mut w, &mut events); assert!(events.is_empty()); assert_eq!(s.mobile_post.get(p).unwrap().team, 2.0);
    s.vehicles.slots[0].variants.entries.swap(0, 1);
    mobile::events_into(&mut s, &mut w, &mut events);
    assert_eq!(events, [MobileEvent::Lost { post: p, vehicle: Some(hulls[1]), from: 2.0 }]);
    assert_eq!(s.mobile_post.get(p).unwrap().vehicle, Some(hulls[0]));
    // A removed variant cannot win even if its hull remains alive in the slab.
    s.vehicles.slots[0].variants.entries.pop(); s.vehicles.slots[0].vehicle = None;
    mobile::events_into(&mut s, &mut w, &mut events); assert_eq!(s.mobile_post.get(p).unwrap().vehicle, Some(hulls[1]));
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn first_refresh_and_repeated_lives_reuse_prepared_pad_storage() {
    let (mut s, p, hulls, mut w) = setup(); let mut events = Vec::with_capacity(2);
    allocation::start();
    mobile::report_all(&mut s, &mut w);
    for _ in 0..1200 { s.vehicle.get_mut(hulls[1]).unwrap().target_generation += 1.0; mobile::events_into(&mut s, &mut w, &mut events); }
    assert_eq!(allocation::stop(), 0); assert_eq!(s.mobile_post.get(p).unwrap().life, Some(1201.0));
}
#[test]
fn native_class_points_use_the_current_hull_matrix_and_exclude_its_body() {
    let (mut s, p, hulls, mut w) = setup(); mobile::report_all(&mut s, &mut w);
    let v = s.vehicle.get_mut(hulls[1]).unwrap(); v.matrix = three::Matrix4::default().elements; v.matrix[12] = 10.0; v.yaw = 0.5;
    let body = v.body; let mut points = Vec::with_capacity(8); let mut casts = 0;
    fleet_posts::spawn_points(&s, p, &mut points, |from, direction, span, ignored| {
        assert_eq!(ignored, body); assert_eq!(from.x, 10.0); assert_eq!(direction.x, 1.0); assert_eq!(span, 1.0); casts += 1; None
    });
    assert_eq!(casts, 8); assert_eq!(points, [[11.0, 0.0, 0.0, 0.5]; 8]);
    s.mobile_post.get_mut(p).unwrap().active = false;
    fleet_posts::spawn_points(&s, p, &mut points, |_, _, _, _| panic!("inactive post cannot cast")); assert!(points.is_empty());
}
