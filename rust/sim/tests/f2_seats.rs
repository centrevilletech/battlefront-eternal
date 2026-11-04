//! Synthetic vehicle-phase fixtures run without the retail game files.
use bfe_sim::vehicles::f2_seats as phase;

use bfe_sim::gen::*;
use bfe_sim::seats::bodies::soldier_mut;
use bfe_sim::seats::hulls::{HullSide, Vacate};
use bfe_sim::seats::lease::{SeatControl, SeatIntent};
use bfe_sim::seats::network::{SeatCommand, SeatKeys};
use bfe_sim::seats::turrets::{CrewWorld, Let, Shot, TurretRig, TurretSide};
use bfe_sim::seats::{Attacker as By, Hit, Holder, Reason, SeatWorld, Seating};
use bfe_sim::store::Ref;
use phase::{HullLife, Seats as Phase, UnitLife};
use three::{Matrix4, Vector3};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event { Board(u32), Move(u32, usize), Leave(u32, Reason), Kill(u32), Drop(u32) }
#[derive(Default)]
struct World {
    events: Vec<Event>,
    casts: usize,
    overlaps: usize,
    // 0 clear floor, 1 no floor, 2 floor but every path out is blocked.
    collision: u8,
    controls: u32,
    kill_entries: usize,
    nested_destroy: Option<usize>,
    expected_empty: Option<Ref<Vehicle>>,
}
impl SeatWorld for World {
    fn publish_hulls(&mut self, st: &Store, hulls: &bfe_sim::seats::hulls::Hulls) {
        for side in &hulls.hulls {
            let hull = st.vehicle.get(side.vehicle).unwrap();
            for (seat, view) in hull.seats.iter().zip(&hull.seat_leases) {
                if let Some(r) = seat.lease {
                    assert_eq!(view.forward, Some(st.seat_lease.get(r).unwrap().input.forward));
                }
            }
        }
    }
    fn cast(&mut self, p: &Vector3, d: &Vector3, far: f64, _: Option<Ref<Body>>) -> Option<Hit> {
        self.casts += 1;
        if self.collision == 1 { return None; }
        if d.y < -0.99 {
            return Some(Hit { point: Vector3::new(p.x, 0.0, p.z), normal: Vector3::new(0.0, 1.0, 0.0), distance: p.y, body: None });
        }
        (self.collision == 2).then_some(Hit { point: *p, normal: Vector3::new(0.0, 1.0, 0.0), distance: far * 0.5, body: None })
    }
    fn overlaps_sphere(&mut self, _: &Vector3, _: f64, _: Option<Ref<Body>>) -> bool { self.overlaps += 1; false }
    fn raycast_units(&mut self, _: &Vector3, _: &Vector3, _: f64, _: Option<u32>) -> Option<f64> { None }
    fn relation(&mut self, a: f64, b: f64) -> f64 { if a == b { 1.0 } else { -1.0 } }
    fn stand_up(&mut self, u: &mut Unit, x: f64, y: f64, z: f64, yaw: f64) {
        let s = soldier_mut(u).unwrap();
        s.position = [x, y, z]; s.previous = s.position; s.settled = s.position;
        s.aim_yaw = yaw; s.grounded = true; s.air = None;
    }
    fn kill(&mut self, st: &mut Store, seating: Seating<'_>, id: u32, _: By) {
        self.kill_entries += 1;
        // This is the synchronous kill-entry contract, before death hooks can
        // reenter. Mutating the seat link here must be detected by the test.
        assert!(st.unit.get(Ref::new(id, 0)).unwrap().seat.is_none());
        if let Some(v) = self.expected_empty {
            assert!(st.vehicle.get(v).unwrap().seats.iter().all(|s| s.lease.is_none()));
        }
        if let Seating::Hulls(h) = seating {
            if let Some(other) = self.nested_destroy.take() { h.destroyed_crew(st, self, other); }
        }
        let u = st.unit.get_mut(Ref::new(id, 0)).unwrap();
        u.alive = false; u.dead = true; u.combat.dead = true;
        self.events.push(Event::Kill(id));
    }
    fn rider_drop(&mut self, _: Holder, _: usize, _: u32) -> Option<Vector3> { Some(Vector3::new(3.0, 4.0, 5.0)) }
    fn sync_rider(&mut self, _: Ref<SeatLease>, u: &mut Unit) { u.tick_count += 1.0; }
    fn controller_state(&mut self, _: Option<u32>) -> Option<f64> { None }
    fn controllable(&mut self, _: Ref<SeatLease>, _: usize, _: SeatRole) -> Ref<SeatUnit> {
        self.controls += 1; Ref::new(self.controls, 1)
    }
    fn boarded(&mut self, id: u32, _: Holder, _: usize) { self.events.push(Event::Board(id)); }
    fn unboarded(&mut self, id: Option<u32>, _: Holder, _: usize, _: Option<Ref<Post>>, reason: Reason) {
        self.events.push(Event::Leave(id.unwrap(), reason));
    }
    fn moved(&mut self, id: Option<u32>, _: Holder, index: usize) { self.events.push(Event::Move(id.unwrap(), index)); }
    fn dropped(&mut self, id: u32, _: &Vector3) { self.events.push(Event::Drop(id)); }
}
impl CrewWorld for World {
    fn update_lock(&mut self, _: usize, _: f64, _: &Vector3, _: &Vector3, _: Ref<SeatLease>) {}
    fn weapon_update(&mut self, _: usize, _: f64, _: bool, _: bool, _: bool) -> Vec<Shot> { Vec::new() }
    fn fire(&mut self, _: usize, _: &Vector3, _: &Vector3, _: Option<u32>, _: Ref<SeatLease>, _: usize) -> usize { unreachable!() }
    fn bind_round(&mut self, _: usize, _: usize, _: f64) { unreachable!() }
    fn sound(&mut self, _: usize, _: usize) { unreachable!() }
    fn fired(&mut self, _: usize, _: usize, _: Option<u32>, _: &Vector3, _: &Vector3) { unreachable!() }
}

fn unit(st: &mut Store, id: u32, network: bool) -> UnitLife {
    let r = Ref::new(id, 0);
    let mut u = Unit { generation: 7.0, alive: true, team: 1.0, network_human: Some(network), ..Unit::default() };
    let s = Soldier { pitch_limits: [-1.0, 1.0], ..Soldier::default() };
    u.soldier = UnitBody::Soldier(s);
    *st.unit.ensure(r) = u;
    UnitLife { unit: r, life: 7.0 }
}
fn hull(st: &mut Store, p: &mut Phase, n: usize) -> HullLife {
    let mut v = Vehicle { alive: true, target_generation: 2.0, team: Some(1.0), bound_radius: 2.0, size: [4.0, 2.0, 4.0], ..Vehicle::default() };
    v.matrix = Matrix4::default().elements;
    for _ in 0..n {
        v.seats.push(VehicleSeat::default());
        v.seat_views.push(SeatView { pitch_limits: [-1.0, 1.0], ..SeatView::default() });
        v.seat_leases.push(SeatLeaseView::default());
    }
    let r = st.vehicle.insert(v);
    st.vehicles.vehicles.push(r);
    p.add_hull(st, HullSide { vehicle: r, index: p.hulls.hulls.len(), hover: true, turret_nodes: vec![false; n], weapons: vec![0; n], ..HullSide::default() });
    HullLife { vehicle: r, life: 2.0 }
}
fn seat(st: &Store, u: UnitLife) -> Ref<SeatLease> { st.unit.get(u.unit).unwrap().seat.unwrap() }
fn setup(n: usize) -> (Store, Phase, World, HullLife) {
    let mut st = Store::default(); let mut p = Phase::default();
    let h = hull(&mut st, &mut p, n);
    (st, p, World::default(), h)
}

#[test]
fn takeover_promotes_and_refreshes_the_derived_controls() {
    let (mut st, mut p, mut w, h) = setup(3);
    let a = unit(&mut st, 1, false); let b = unit(&mut st, 2, false); let c = unit(&mut st, 3, true);
    assert!(p.board(&mut st, &mut w, a, h, 0));
    assert!(p.board(&mut st, &mut w, b, h, 2));
    let ar = seat(&st, a); let br = seat(&st, b);
    assert!(p.board(&mut st, &mut w, c, h, 0));
    assert_eq!(st.seat_lease.get(ar).unwrap().index, 1.0);
    assert_eq!(st.seat_lease.get(br).unwrap().index, 2.0);
    assert!(p.unboard(&mut st, &mut w, c, Vacate { placement: false, ..Vacate::default() }));
    assert_eq!(st.seat_lease.get(br).unwrap().index, 0.0, "last bot in a later seat is promoted");
    assert_eq!(st.vehicle.get(h.vehicle).unwrap().seat_leases[0].unit, bfe_sim::store::Nullable::Value(b.unit));
}

#[test]
fn latch_freezes_once_takes_edges_and_honors_requested_exits() {
    let (mut st, mut p, mut w, h) = setup(2);
    let a = unit(&mut st, 1, false);
    assert!(p.board(&mut st, &mut w, a, h, 0));
    let r = seat(&st, a); let c = st.seat_lease.get(r).unwrap().controllable.unwrap();
    p.control(c, SeatControl { driven: true, intent: SeatIntent { forward: 0.75, buttons: [1.0, 0.0], reload: true, ..SeatIntent::default() } }, false);
    p.latch(&mut st, &mut w, 1.0);
    assert_eq!(st.seat_lease.get(r).unwrap().input.forward, 0.75);
    assert!(st.seat_lease.get(r).unwrap().input.pressed[0]);
    assert!(st.seat_lease.get(r).unwrap().input.reload);
    assert_eq!(st.vehicle.get(h.vehicle).unwrap().seat_leases[0].forward, Some(0.75));
    p.latch(&mut st, &mut w, 2.0);
    assert!(!st.seat_lease.get(r).unwrap().input.pressed[0]);
    assert!(!st.seat_lease.get(r).unwrap().input.reload);
    assert!(p.request_exit(&mut st, a, None, None, false));
    p.latch(&mut st, &mut w, 3.0);
    assert!(st.unit.get(a.unit).unwrap().seat.is_none());
    assert!(st.seat_lease.get(r).unwrap().released);
}

#[test]
fn stale_generations_and_retired_slots_do_not_retarget() {
    let (mut st, mut p, mut w, h) = setup(1);
    let a = unit(&mut st, 1, false);
    assert!(p.board(&mut st, &mut w, a, h, 0)); let old = seat(&st, a);
    st.unit.get_mut(a.unit).unwrap().generation += 1.0;
    assert!(!p.killed(&mut st, &mut w, old), "an old rider life cannot drop the new body");
    p.latch(&mut st, &mut w, 1.0);
    assert!(st.seat_lease.get(old).unwrap().released);
    p.published(&mut st);
    let b = unit(&mut st, 2, true);
    assert!(p.board(&mut st, &mut w, b, h, 0)); let next = seat(&st, b);
    assert_eq!(old.slot, next.slot); assert_ne!(old.gen, next.gen);
    assert!(!p.killed(&mut st, &mut w, old));
    assert!(!p.move_seat(&mut st, &mut w, old, 0));
    assert!(!p.request_exit(&mut st, a, None, None, false));
    assert_eq!(seat(&st, b), next);
    st.vehicle.get_mut(h.vehicle).unwrap().target_generation += 1.0;
    let c = unit(&mut st, 3, true);
    assert!(!p.board(&mut st, &mut w, c, h, 0));
}

#[test]
fn blocked_human_exit_uses_all_twenty_four_casts_and_keeps_the_lease() {
    let (mut st, mut p, mut w, h) = setup(1);
    let a = unit(&mut st, 1, true);
    assert!(p.board(&mut st, &mut w, a, h, 0)); let r = seat(&st, a);
    w.collision = 2;
    assert!(!p.unboard(&mut st, &mut w, a, Vacate::default()));
    assert_eq!(w.casts, 24); assert_eq!(w.overlaps, 12);
    assert_eq!(seat(&st, a), r);
    assert!(p.unboard(&mut st, &mut w, a, Vacate { force: true, ..Vacate::default() }));
    assert!(st.seat_lease.get(r).unwrap().released);
}

#[test]
fn classic_exit_checks_ninety_eight_rays_before_its_fallback() {
    let (mut st, mut p, mut w, h) = setup(1);
    let a = unit(&mut st, 1, false);
    assert!(p.board(&mut st, &mut w, a, h, 0));
    w.collision = 1;
    assert!(p.unboard(&mut st, &mut w, a, Vacate::default()));
    // Nine candidates each ask ten support casts; the first eight also test
    // their line out. The documented 99-point search uses 98 casts here.
    assert_eq!(w.casts, 98);
    assert_eq!(soldier_mut(st.unit.get_mut(a.unit).unwrap()).unwrap().position, [0.0; 3]);
}

#[test]
fn killed_rider_uses_the_drop_point_and_promotes() {
    let (mut st, mut p, mut w, h) = setup(2);
    let a = unit(&mut st, 1, false); let b = unit(&mut st, 2, false);
    assert!(p.board(&mut st, &mut w, a, h, 0)); assert!(p.board(&mut st, &mut w, b, h, 1));
    let r = seat(&st, a);
    assert!(p.killed(&mut st, &mut w, r));
    let u = st.unit.get_mut(a.unit).unwrap();
    assert_eq!(soldier_mut(u).unwrap().position, [3.0, 4.0, 5.0]);
    assert_eq!(st.seat_lease.get(seat(&st, b)).unwrap().index, 0.0);
    assert!(w.events.contains(&Event::Drop(1)));
}

#[test]
fn destroyed_crews_are_all_released_at_each_kill_entry() {
    let (mut st, mut p, mut w, h) = setup(2);
    let a = unit(&mut st, 1, false); let b = unit(&mut st, 2, false);
    assert!(p.board(&mut st, &mut w, a, h, 0)); assert!(p.board(&mut st, &mut w, b, h, 1));
    w.expected_empty = Some(h.vehicle);
    assert!(p.destroyed(&mut st, &mut w, h));
    assert_eq!(w.kill_entries, 2);
    assert!(!st.unit.get(a.unit).unwrap().alive); assert!(!st.unit.get(b.unit).unwrap().alive);
    assert!(st.vehicle.get(h.vehicle).unwrap().seat_leases.iter().all(|l| l.unit == bfe_sim::store::Nullable::Undefined));
}

#[test]
fn network_cycle_swaps_identities_and_look_is_immediate() {
    let (mut st, mut p, mut w, h) = setup(2);
    let a = unit(&mut st, 1, true); let b = unit(&mut st, 2, false);
    assert!(p.board(&mut st, &mut w, a, h, 0)); assert!(p.board(&mut st, &mut w, b, h, 1));
    let ar = seat(&st, a); let br = seat(&st, b);
    assert!(p.cycle(&mut st, &mut w, a, 1));
    assert_eq!(seat(&st, a), ar); assert_eq!(seat(&st, b), br);
    assert_eq!(st.seat_lease.get(ar).unwrap().index, 1.0); assert_eq!(st.seat_lease.get(br).unwrap().index, 0.0);
    let before = st.vehicle.get(h.vehicle).unwrap().seats[1].aim_yaw;
    p.network_input(&mut st, ar, &SeatKeys::default(), &SeatCommand { look: vec![(10.0, 5.0), (-3.0, -2.0)], ..SeatCommand::default() }).unwrap();
    let s = &st.vehicle.get(h.vehicle).unwrap().seats[1];
    assert_ne!(s.aim_yaw, before); assert_eq!(s.look_yaw, Some(s.aim_yaw));
}

#[test]
fn a_command_marks_only_its_hull_for_publication() {
    let (mut st, mut p, mut w, h) = setup(2);
    let other = hull(&mut st, &mut p, 2);
    let a = unit(&mut st, 1, false);
    assert!(p.board(&mut st, &mut w, a, h, 0));
    assert_eq!(p.changed.hulls, [h.vehicle]);
    assert!(!p.changed.hulls.contains(&other.vehicle));
    assert_eq!(p.changed.units, [a.unit]);
}

#[test]
fn turret_rebuild_keeps_its_rig_and_aim_but_not_its_dead_crew() {
    let mut st = Store::default(); let mut p = Phase::default(); let mut w = World::default();
    let matrix = Matrix4::default().elements;
    let rig = TurretRig::new(&matrix, &[], None);
    let t = Ref::new(4, 1);
    p.add_turret(TurretSide { index: 0, turret: t, alive: true, yaw: 0.25, rig, ..TurretSide::default() });
    let a = unit(&mut st, 1, false);
    assert!(p.man(&mut st, &mut w, a, t)); let r = seat(&st, a);
    assert!(p.turret_life(&mut st, &mut w, t, false));
    assert_eq!(w.kill_entries, 1); assert!(st.seat_lease.get(r).unwrap().released);
    assert!(p.turret_life(&mut st, &mut w, t, true));
    assert_eq!(p.crews.turrets[0].yaw, 0.25);
    assert!(p.crews.turrets[0].lease.is_none());
    let b = unit(&mut st, 2, true);
    assert!(p.man(&mut st, &mut w, b, t));
    assert!(!p.leave_turret(&mut st, &mut w, r, Let::default()));
    let br = seat(&st, b);
    assert!(p.leave_turret(&mut st, &mut w, br, Let { placement: false, ..Let::default() }));
    p.turret_tick(&mut st, &mut w, 1.0 / 60.0);
    assert_eq!(st.turret_seats.ticks, 1.0);
}

// Count allocations on this test's thread, so unrelated parallel tests cannot
// turn the warmed phase's allocation check into a scheduling-dependent result.
mod allocation {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    thread_local! {
        static ENABLED: Cell<bool> = const { Cell::new(false) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }
    pub struct Counted;
    unsafe impl GlobalAlloc for Counted {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ENABLED.with(|e| { if e.get() { COUNT.with(|n| n.set(n.get() + 1)); } });
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) { System.dealloc(p, layout); }
        unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            ENABLED.with(|e| { if e.get() { COUNT.with(|n| n.set(n.get() + 1)); } });
            System.realloc(p, layout, size)
        }
    }
    pub fn start() { COUNT.with(|n| n.set(0)); ENABLED.with(|e| e.set(true)); }
    pub fn stop() -> usize { ENABLED.with(|e| e.set(false)); COUNT.with(Cell::get) }
}
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;

#[test]
fn warmed_hull_and_turret_phases_allocate_nothing() {
    let (mut st, mut p, mut w, h) = setup(2);
    let a = unit(&mut st, 1, false);
    assert!(p.board(&mut st, &mut w, a, h, 0));
    let matrix = Matrix4::default().elements;
    let rig = TurretRig::new(&matrix, &[], None);
    let t = Ref::new(4, 1);
    p.add_turret(TurretSide { index: 0, turret: t, alive: true, rig, ..TurretSide::default() });
    let b = unit(&mut st, 2, false);
    assert!(p.man(&mut st, &mut w, b, t));
    p.crews.collide = false;
    p.latch(&mut st, &mut w, 1.0);
    p.turret_tick(&mut st, &mut w, 1.0 / 60.0);
    p.published(&mut st);
    allocation::start();
    for tick in 0..1200 {
        p.latch(&mut st, &mut w, tick as f64 / 60.0);
        p.turret_tick(&mut st, &mut w, 1.0 / 60.0);
        p.published(&mut st);
    }
    let allocations = allocation::stop();
    assert_eq!(allocations, 0, "warmed seat phases must reuse their storage");
}

#[test]
fn a_nested_crew_death_preserves_the_outer_pending_deaths() {
    let (mut st, mut p, mut w, h) = setup(2);
    let other = hull(&mut st, &mut p, 1);
    let a = unit(&mut st, 1, false); let b = unit(&mut st, 2, false); let c = unit(&mut st, 3, false);
    assert!(p.board(&mut st, &mut w, a, h, 0));
    assert!(p.board(&mut st, &mut w, b, h, 1));
    assert!(p.board(&mut st, &mut w, c, other, 0));
    w.nested_destroy = Some(1);
    assert!(p.destroyed(&mut st, &mut w, h));
    assert_eq!(w.kill_entries, 3);
    assert!(!st.unit.get(a.unit).unwrap().alive);
    assert!(!st.unit.get(b.unit).unwrap().alive);
    assert!(!st.unit.get(c.unit).unwrap().alive);
    assert_eq!(w.events[w.events.len() - 3..], [Event::Kill(3), Event::Kill(1), Event::Kill(2)]);
}

#[test]
fn another_hull_birth_keeps_existing_crew_look_and_pending_ai_input() {
    let (mut st, mut p, mut w, h) = setup(2);
    let human = unit(&mut st, 1, true); let bot = unit(&mut st, 2, false);
    assert!(p.board(&mut st, &mut w, human, h, 0));
    assert!(p.board(&mut st, &mut w, bot, h, 1));
    let human_lease = seat(&st, human); let bot_lease = seat(&st, bot);
    let control = st.seat_lease.get(bot_lease).unwrap().controllable.unwrap();
    p.control(control, SeatControl { driven: true, intent: SeatIntent { forward: 0.75, ..SeatIntent::default() } }, false);
    p.network_input(&mut st, human_lease, &SeatKeys::default(), &SeatCommand { look: vec![(10.0, 5.0)], ..SeatCommand::default() }).unwrap();
    let yaw = st.vehicle.get(h.vehicle).unwrap().seats[0].aim_yaw;
    let pitch = st.vehicle.get(h.vehicle).unwrap().seats[0].pitch;
    let other = hull(&mut st, &mut p, 1);
    assert_ne!(other.vehicle, h.vehicle);
    p.latch(&mut st, &mut w, 1.0);
    assert_eq!(st.vehicle.get(h.vehicle).unwrap().seats[0].aim_yaw, yaw);
    assert_eq!(st.vehicle.get(h.vehicle).unwrap().seats[0].pitch, pitch);
    assert_eq!(st.seat_lease.get(bot_lease).unwrap().input.forward, 0.75);
}
