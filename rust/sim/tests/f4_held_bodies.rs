//! Native held-body writes are visible to later props in the same live span.
use bfe_sim::battle::{animated::{self, Animated, Body, Settings}, held_bodies::{self, Bodies, Positions}, prepared};
use bfe_sim::gen::*;
use bfe_sim::store::{Nullable, Ref, StrId};
use three::{Matrix4, Quaternion, Vector3};
struct World<'a> { s: &'a mut Store, positions: &'a Positions, original: Ref<Prop> }
impl animated::World for World<'_> {
    fn near(&mut self, _: &Animated, settings: &Settings, _: &[Vector3]) -> bool { self.positions.near(self.s, settings) }
    fn refresh_body(&mut self, p: &Animated, b: &mut Body) { held_bodies::refresh(self.s, p, self.original, b); }
    fn body_changed(&mut self, _: &Animated, b: &Body) { assert!(held_bodies::commit(self.s, self.original, b).is_some()); }
    fn has_rider_clip(&mut self, _: &Animated, _: &Body, _: usize) -> bool { true }
    fn sound(&mut self, _: &Animated, _: StrId, _: Vector3) {}
    fn let_go(&mut self, p: &Animated, b: &mut Body) {
        self.s.unit.get_mut(b.reference.unwrap()).unwrap().let_go(); self.refresh_body(p, b);
    }
    fn damage(&mut self, _: &Animated, _: &mut Body, _: f64) { panic!("unexpected swallow"); }
    fn remove(&mut self, _: &Animated, _: &mut Body) { panic!("unexpected removal"); }
    fn rider_root(&mut self, _: &Animated, _: &Body, _: StrId, _: f64) -> Option<(Vector3, Quaternion)> { Some((Vector3::default(), Quaternion::default())) }
}
fn creature() -> bfe_sim::battle::live_phase::Animated {
    let mut world = Matrix4::default(); world.elements[12] = 30.0;
    prepared::animated(F4AnimatedBirth {
        settings: PaSettings { root: Matrix4::default().elements, parents: vec![-1.0],
            bind: vec![PaBind { p: [30.0, 0.0, 0.0], q: [0.0, 0.0, 0.0, 1.0] }],
            grabs: vec![PaGrab { slot: 1.0, window: 1.0, radius: 100.0, node: Some(0.0), clip: StrId(0) }], ..PaSettings::default() },
        state: PaState { index: 0.0, slot: 1.0, weight: 1.0, r#loop: false, root_world: Matrix4::default().elements,
            stand_q: vec![0.0, 0.0, 0.0, 1.0], stand_t: vec![30.0, 0.0, 0.0],
            pose: Some(PaPose { root: -1.0, q: vec![0.0, 0.0, 0.0, 1.0], t: vec![30.0, 0.0, 0.0], w: vec![0.0] }),
            bones: vec![PaBone { p: [30.0, 0.0, 0.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3], world: world.elements }], ..PaState::default() },
    })
}
fn setup() -> (Store, Ref<Unit>, Bodies) {
    let mut s = Store::default(); let r = s.unit.insert(Unit { alive: true, generation: 1.0, tick_count: 10.0,
        soldier: UnitBody::Soldier(Soldier { air: Some(Air::Jump), dive: 1.0, velocity: [1.0; 3], ..Soldier::default() }), ..Unit::default() });
    s.units.list.push(Member::Unit(r)); s.units.next_id = 1.0;
    let mut bodies = Bodies::new(1, 1); bodies.adopt(r); bodies.refresh_order(&s); bodies.positions.begin(&s, Some(r), &[]);
    (s, r, bodies)
}
#[test]
fn seize_and_carry_commit_before_a_later_door_reads_the_same_position_reference() {
    let (mut s, r, mut bodies) = setup(); let original = Ref::new(7, 1); let mut prop = creature();
    let mut w = World { s: &mut s, positions: &bodies.positions, original };
    prop.step(1.0 / 60.0, 60.0, &[], &mut bodies.frames, &bodies.order, &mut w);
    let u = w.s.unit.get(r).unwrap(); assert_eq!(u.position(), Vector3::new(30.0, 0.0, 0.0)); assert_eq!(u.tick_count, 11.0);
    let Nullable::Value(held) = &u.held else { panic!("missing native hold"); };
    assert_eq!(held.prop, original); assert_eq!(held.since, 60.0); assert!(matches!(held.frame, Nullable::Value(_)));
    assert_eq!(u.combat.mounted, Some(true));
    let UnitBody::Soldier(s) = &u.soldier else { unreachable!() };
    assert!(s.air.is_none()); assert_eq!(s.dive, 0.0); assert_eq!(s.previous, [30.0, 0.0, 0.0]); assert_eq!(s.settled, s.previous);
    let mut door = creature(); door.settings.door = true;
    door.settings.triggers = vec![animated::Trigger { slot: 0, point: Vector3::default(), radius: 5.0, sound: None, opens: true }];
    door.state.body_enabled = Some(true);
    door.step(0.5, 60.0, &[], &mut bodies.frames, &bodies.order, &mut w);
    assert_eq!(door.state.openness, 0.0); assert_eq!(door.state.body_enabled, Some(true));
    // Reusing the same body invalidates the old prop's victim before carry.
    w.s.unit.get_mut(r).unwrap().generation = 2.0;
    assert!(held_bodies::commit(w.s, original, &bodies.frames[0]).is_none());
    prop.state.carry(61.0, &mut bodies.frames, &prop.settings, &mut w); assert!(prop.state.victim.is_none());
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn repeated_canonical_carries_reuse_frames_positions_and_pose_storage() {
    let (mut s, r, mut bodies) = setup(); let mut prop = creature(); let original = Ref::new(7, 1);
    let mut w = World { s: &mut s, positions: &bodies.positions, original };
    allocation::start();
    for tick in 0..1200 { prop.step(0.0, tick as f64, &[], &mut bodies.frames, &bodies.order, &mut w); }
    assert_eq!(allocation::stop(), 0); assert_eq!(w.s.unit.get(r).unwrap().tick_count, 1210.0);
}
