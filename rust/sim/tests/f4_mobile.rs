//! Command-post refresh borrows prepared hull facts and retains class arrays.
use bfe_sim::battle::{mobile::{self, ClassRecord, Hull, Pad, MobileEvent}, world::PostsWorld};
use bfe_sim::gen::*;
use bfe_sim::store::{Ref, StrId, Strings};
struct Native { pad: Pad }
impl PostsWorld for Native {
    fn capture_team(&mut self, team: f64) -> f64 { team }
    fn building_alive(&mut self, _: Ref<Building>) -> bool { true }
    fn pad(&mut self, _: &Store, _: f64) -> Pad { panic!("native refresh must borrow the prepared pad"); }
    fn with_pad(&mut self, s: &mut Store, _: f64, visit: &mut dyn FnMut(&mut Store, &Pad)) { visit(s, &self.pad); }
    fn vehicle_position(&mut self, s: &Store, vehicle: Ref<Vehicle>) -> [f64; 3] {
        s.vehicle.get(vehicle).unwrap().position
    }
    fn class_spawn_world(&mut self, _: &Store, _: Ref<MobilePost>) -> Vec<[f64; 4]> { panic!("unexpected point query"); }
}
fn setup() -> (Store, Ref<MobilePost>, Native) {
    let mut s = Store::default();
    let a = s.vehicle.insert(Vehicle::default()); let b = s.vehicle.insert(Vehicle::default());
    s.vehicle.get_mut(b).unwrap().position = [1.0, 2.0, 3.0];
    let hull = |id| Hull { id, alive: true, command: true, generation: 1.0, team: 1.0, name: StrId(1), label: StrId(2),
        record: Some(ClassRecord { atk: Some(vec![3.0; 6]), def: Some(vec![4.0; 6]), value_bleed: Some(10.0),
            spawn_points: vec![[1.0, 2.0, 3.0, 4.0]; 8] }), ..Hull::default() };
    let pad = Pad { variants: vec![hull(a), hull(b)], vehicle: Some(hull(a)) };
    let mut p = mobile::new_mobile_post(0.0, &mut Strings::default()); mobile::reserve_class(&mut p, 6, 6, 8);
    let p = s.mobile_post.insert(p); s.mobile_posts.posts.push(p);
    (s, p, Native { pad })
}
#[test]
fn last_live_command_variant_wins_and_occupation_keeps_its_owner_after_exit() {
    let (mut s, p, mut native) = setup(); mobile::report_all(&mut s, &mut native);
    assert_eq!(s.mobile_post.get(p).unwrap().vehicle, Some(native.pad.variants[1].id));
    let mut events = Vec::with_capacity(2);
    native.pad.variants[1].seated = true; native.pad.variants[1].controlling_team = 2.0;
    mobile::events_into(&mut s, &mut native, &mut events);
    assert_eq!(events, [MobileEvent::Owner { post: p, from: 1.0, to: 2.0 }]);
    native.pad.variants[1].seated = false; mobile::events_into(&mut s, &mut native, &mut events);
    assert!(events.is_empty()); assert_eq!(s.mobile_post.get(p).unwrap().team, 2.0);
    for h in &mut native.pad.variants { h.alive = false; } native.pad.vehicle = None;
    mobile::events_into(&mut s, &mut native, &mut events);
    assert_eq!(events, [MobileEvent::Lost { post: p, vehicle: Some(native.pad.variants[1].id), from: 2.0 }]);
    let p = s.mobile_post.get(p).unwrap(); assert!(!p.active); assert_eq!(p.last_position, [1.0, 2.0, 3.0]);
}
#[test]
fn replacing_a_living_hull_reports_loss_before_reporting_the_new_life() {
    let (mut s, p, mut native) = setup(); mobile::report_all(&mut s, &mut native);
    let mut events = Vec::with_capacity(2); native.pad.variants[1].generation = 2.0;
    mobile::events_into(&mut s, &mut native, &mut events);
    assert_eq!(events, [MobileEvent::Lost { post: p, vehicle: Some(native.pad.variants[1].id), from: 1.0 }]);
    let current = s.mobile_post.get(p).unwrap(); assert_eq!(current.life, Some(2.0)); assert!(current.active);
    assert_eq!(current.ai.points.len(), 8); assert_eq!(current.desc.value_bleed, 10.0);
    mobile::events_into(&mut s, &mut native, &mut events); assert!(events.is_empty());
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn repeated_hull_lives_reuse_pad_facts_class_paths_and_event_storage() {
    let (mut s, p, mut native) = setup(); mobile::report_all(&mut s, &mut native);
    let mut events = Vec::with_capacity(2); allocation::start();
    for _ in 0..1200 {
        native.pad.variants[1].generation += 1.0;
        mobile::events_into(&mut s, &mut native, &mut events); assert_eq!(events.len(), 1);
    }
    assert_eq!(allocation::stop(), 0); assert_eq!(s.mobile_post.get(p).unwrap().life, Some(1201.0));
}
#[test]
fn a_class_spawn_point_retracts_toward_the_transformed_collision_center() {
    use three::{Matrix4, Vector3};
    let mut matrix = Matrix4::default(); matrix.elements[12] = 10.0; matrix.elements[13] = 20.0; matrix.elements[14] = 30.0;
    let point = mobile::command_spawn_point(&matrix, Vector3::new(1.0, 2.0, 3.0), 0.5, [4.0, 6.0, 3.0, 90.0], |from, direction, span| {
        assert_eq!(from, Vector3::new(11.0, 22.0, 33.0)); assert_eq!(span, 5.0);
        assert_eq!(direction, Vector3::new(0.6000000000000001, 0.8, 0.0)); Some(Vector3::new(12.0, 23.0, 33.0))
    });
    assert_eq!(point, [12.0, 23.0, 33.0, 0.5 + std::f64::consts::FRAC_PI_2]);
    let point = mobile::command_spawn_point(&matrix, Vector3::default(), 0.0, [0.0; 4], |_, _, _| panic!("zero-span point cannot cast"));
    assert_eq!(point, [10.0, 20.0, 30.0, 0.0]);
}
