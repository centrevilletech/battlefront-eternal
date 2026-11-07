//! Live Vehicles seat ownership. Foreign unit facts enter once before latching;
//! leases and hull links are never imported from JavaScript. Command-local Store
//! shells transfer ownership across hooks, so no canonical borrow survives JS.
//!
//! Vehicles.board/vacate and TurretSeats.man/leave share the same lease slab.
//! Both slab generation and soldier life matter: a respawn can reuse a unit
//! object while a released beam still holds its earlier seat reference.
//! Publication follows touched identities rather than walking the fleet.
//! A synchronous callback may publish again before its outer solver resumes.
use super::*;
use super::firing::{LockFacts, LockQuery, LockView, allowed, pick_view};
use crate::seats::{self, Attacker as By, Hit as SeatHit, Holder, Reason, SeatWorld, Seating};
use crate::seats::bodies::{CombatFlags, soldier, soldier_mut};
use crate::seats::hulls::{Hulls, HullSide, RiderFacts, Vacate};
use crate::seats::lease::{control_key, SeatControl, SeatIntent};
use crate::store::block::Block;
use crate::seats::turrets::{CrewWorld, Crews, Let, Shot};
use crate::combat::defs::WeaponClass;
use crate::combat::weapons::{Admission, LockTarget, Shot as WeaponShot, ShotKind, WeaponDraws};

// TurretSeats.drive keeps one weapon per turret, independent of its occupant.
// The reusable shot and lock buffers preserve interleaving without allocating
// a Vec for each crew tick; only construction resolves weapon definitions.
struct TurretWeapon {
    state: Weapon, class: Option<WeaponClass>, ordnance: Option<crate::store::DefId>,
    channel: f64, random_binding:u32, shots: Vec<WeaponShot>, locks: Vec<F2TurretLockLevel>,
}
// Weapon.update draws only when a shot needs spread. Owned worlds share the
// numeric seat-weapons channel with hull guns, preserving their interleaving.
// Standalone fleet fixtures retain their explicit foreign generator.
struct Draws<'w> { world_ctx: &'w mut crate::world::owner::World, turret: usize }
impl WeaponDraws for Draws<'_> {
    fn draw(&mut self, _: u32) -> f64 { let world_ctx = &mut *self.world_ctx;
            if crate::units::f3::active() {
                return crate::units::f3::random_binding(seats_state(&mut world_ctx.fleet.seats).weapons[self.turret].as_ref().unwrap().random_binding);
            }
            import_f2_turret_draw(&F2TurretDraw { turret: self.turret as u32 }).value
    }
    fn legacy(&mut self, i: u32) -> f64 { self.draw(i) }
    fn seeded(&mut self, _: f64, _: f64) {}
    fn admit(&mut self, _: u32) -> Admission { Admission::Yes }
}

const INPUT_CAPACITY: usize = 262144;
// Vehicles and TurretSeats formerly held these lists in JavaScript. Slabs
// now own identities; these sidecars retain class facts and reusable exchange
// storage. A mirror cache never decides whether a lease is live.
pub(crate) struct State {
    owner: crate::vehicles::f2_seats::Seats,
    prepared: Vec<Option<HullSide>>,
    prepared_seats: usize,
    depth: usize, command_depth: usize,
    input: Vec<f64>,
    io: [f64; F2SeatIo::WORDS],
    lease_cache: Vec<super::delta::Cache>, unit_cache: Vec<super::delta::Cache>,
    lease_inputs: crate::seats::lease::InputPool,
    leases: Block, units: Block, controls: Block, effects: Block,
    body_effects: Vec<F2SeatEffect>,
    ordnance_output: Block, ordnance_dirty: bool,
    command_relations: Vec<F2SeatRelation>,
    actors: Vec<bool>, rider_sides: Vec<bool>, rider_posed: Vec<bool>,
    turret_lives: Vec<f64>, turret_indices: Vec<Option<(Ref<Turret>,usize)>>, posed: Vec<bool>, timed: Vec<bool>, weapons: Vec<Option<TurretWeapon>>, turret_rows: Vec<F2TurretOut>, turret_output: Block,
    event_rows: Vec<F2TurretEvent>, event_output: Block, rounds: Vec<Option<crate::ordnance::world::Shot>>,
}
fn seats_state(seats: &mut Option<State>) -> &mut State {
    { seats.get_or_insert_with(|| State {
        owner: Default::default(), prepared: Vec::new(), prepared_seats: 0, depth: 0, command_depth: 0, input: vec![0.0; INPUT_CAPACITY], io: [0.0; F2SeatIo::WORDS],
        lease_cache: Vec::with_capacity(512), unit_cache: Vec::with_capacity(512),
        lease_inputs: Default::default(),
        leases: Block::default(), units: Block::default(), controls: Block::default(), effects: Block::default(),
        ordnance_output: Block { words: Vec::with_capacity(32768), heap: Vec::with_capacity(32768) }, ordnance_dirty: false,
        body_effects: Vec::with_capacity(512), command_relations: Vec::with_capacity(256),
        actors: Vec::with_capacity(512), rider_sides: Vec::with_capacity(512), rider_posed: Vec::with_capacity(512),
        turret_lives: Vec::with_capacity(128), turret_indices: Vec::with_capacity(128), posed: Vec::with_capacity(128), timed: Vec::with_capacity(128), weapons: Vec::with_capacity(128), turret_rows: Vec::with_capacity(128), turret_output: Block::default(),
        event_rows: Vec::with_capacity(512), event_output: Block::default(), rounds: Vec::with_capacity(32),
    }) }
}
pub(in crate::vehicles::module) fn seats_owner(fleet: &mut super::super::Fleet) -> &mut crate::vehicles::f2_seats::Seats {
    &mut seats_state(&mut fleet.seats).owner
}
/// An outer native phase borrows the seat owner once. Narrow services loan
/// it back before F3 observes a rider or F2 performs a recursive command.
pub(crate) fn with_owned_seats_in<R>(world_ctx: &mut crate::world::owner::World, run:impl FnOnce(&mut crate::vehicles::f2_seats::Seats)->R)->R {
    let mut owner=std::mem::take(&mut *world_ctx.fleet.seats_owner());
    let result=run(&mut owner);
    *world_ctx.fleet.seats_owner()=owner;
    result
}
pub(crate) fn loan_seats_in<R>(world_ctx: &mut crate::world::owner::World, owner:&mut crate::vehicles::f2_seats::Seats,run:impl FnOnce()->R)->R {
    std::mem::swap(owner,&mut *world_ctx.fleet.seats_owner());
    let result=run();
    std::mem::swap(owner,&mut *world_ctx.fleet.seats_owner());
    result
}
// SeatUnit refers to the foreign registry slot, whose gameplay generation
// is carried separately. Lease validation checks that generation before
// allowing a reused soldier record to steer or release an old occupancy.
fn unit_ref(id: u32) -> Ref<Unit> {
    if crate::units::f3::active() { crate::units::f3::unit_reference(id) }
    else { Ref::new(id, 0) }
}
// Vehicles.syncRider dirties only the rider it placed. Keep the first touch
// order, since hooks can publish midway through a crew operation. Repeated
// touches before that hook require only one body row.
fn touch_unit_in(fleet: &mut crate::vehicles::module::Fleet, r: Ref<Unit>) {
    super::pending_in(fleet);
    if crate::units::f3::active() { crate::units::f3::changed(r); }
    let list = &mut seats_state(&mut fleet.seats).owner.changed.units;
    if !list.contains(&r) { list.push(r); }
}
// Vehicles.move and vacate can touch the same lease several times in one
// command. Retain its identity once; the eventual delta represents the
// latest canonical state, not an intermediate JavaScript snapshot.
fn touch_lease_in(fleet: &mut crate::vehicles::module::Fleet, r: Ref<SeatLease>) {
    super::pending_in(fleet);
    let list = &mut seats_state(&mut fleet.seats).owner.changed.leases;
    if !list.contains(&r) { list.push(r); }
}
// Vehicles.board and promote change both the hull link and derived lease
// views. Mark its current occupants too, so synchronous hooks see the new
// seat indices before a later physics store publishes the hull.
fn touch_hull_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>) {
    super::mark_in(&mut world_ctx.fleet, r);
    let list = &mut seats_state(&mut world_ctx.fleet.seats).owner.changed.hulls;
    if !list.contains(&r) { list.push(r); }
    if let Some(v) = world_ctx.sim.store.vehicle.get(r) {
        for s in &v.seats { if let Some(l) = s.lease { touch_lease_in(&mut world_ctx.fleet, l); } }
    }
}
// Vehicles.destroyedCrew carries the original attacker through nested deaths.
// Do not resolve an owner to whichever occupant now holds a seat: attribution
// uses the captured owner reference, including owner snapshots.
fn by(owner: OrdnanceOwner) -> By {
    By::Record(match owner { OrdnanceOwner::Null => Attacker::Null, OrdnanceOwner::Player => Attacker::Player,
        OrdnanceOwner::Unit(r) => Attacker::Unit(r), OrdnanceOwner::Vehicle(r) => Attacker::Vehicle(r),
        OrdnanceOwner::OwnerSnapshot(r) => Attacker::OwnerSnapshot(r) })
}
// SeatsWorld.kill accepts the same owner union as combat. Preserve the
// distinction between no attacker, the local player, a unit and a hull;
// changing that union would change kill credit in synchronous listeners.
fn attacker_in(world_ctx: &mut crate::world::owner::World, owner: By) -> Attacker {
    match owner { By::Nobody => Attacker::Null, By::Player => Attacker::Player,
        By::Unit(u) => Attacker::Unit(unit_ref(u)), By::Hull(i) => Attacker::Vehicle(world_ctx.sim.store.vehicles.vehicles[i]), By::Record(a) => a }
}
// Vehicles.vacate forwards its reason to onUnboard after body placement.
// Keep stale, displaced, killed and destroyed distinct: AI handlers decide
// whether and when they may board again from this value.
fn why(r: Reason) -> SeatReason { match r { Reason::Exit => SeatReason::Exit, Reason::Stale => SeatReason::Stale,
    Reason::Killed => SeatReason::Killed, Reason::Destroyed => SeatReason::Destroyed, Reason::Displaced => SeatReason::Displaced } }
// SeatsWorld hooks use an index in the appropriate system, not a common
// hull/turret index. Keep the discriminator alongside it so identical indices
// in the two registries cannot name the wrong listener source.
fn holder(h: Holder) -> (f64, bool) { match h { Holder::Hull(i) => (i as f64, false), Holder::Turret(i) => (i as f64, true) } }
// SeatInput.latch reads the soldier or driven SeatUnit intent exactly once.
// Translate optional aim without normalizing or consuming edge counters here;
// those decisions stay in the lease solver at its original latch slot.
fn intent(i: SeatAiIntent) -> SeatIntent {
    SeatIntent { forward: i.forward, right: i.right, turn: i.turn, pitch: i.pitch, jump: i.jump, crouch: i.crouch, prone: i.prone,
        r#use: i.r#use, exit: i.exit, buttons: i.buttons, reload: i.reload, next_primary: i.next_primary, previous_primary: i.previous_primary,
        next_secondary: i.next_secondary, zoom: i.zoom, aim: i.aim.as_ref().map(vec), brake: i.brake, up: i.up, down: i.down,
        trick: i.trick, immelmann: i.immelmann, boost: i.boost }
}

pub(crate) fn f2_seats_init_in(world_ctx: &mut crate::world::owner::World) -> *const f64 { publish_in(world_ctx); seats_state(&mut world_ctx.fleet.seats).io.as_ptr() }
#[no_mangle]
pub extern "C" fn f2_seats_adopt_words() -> u32 { SeatHullFacts::WORDS as u32 }
// poseRider and poseTurret choose whether the world shows the occupant.
// A manual foreign frame has no native lease and keeps its own body view;
// fleet riders read only the retained rig's node and the turret's class fact.
// TurretSeats.vacate restores visibility according to the mount being left.
// An earlier turret must not show a corpse leaving an enclosed hull.
// The current lease and native blow-apart flag decide this observation.
// It does not ask a stale JavaScript holder cache to infer visibility.
pub(crate) fn visible_in(fleet: &mut super::super::Fleet, store: &Store, unit: &Unit) -> Option<bool> {
    let lease = store.seat_lease.get(unit.seat?)?;
    if let Some(hull) = lease.vehicle {
        let posed = fleet.poses.get(hull.slot as usize)?.as_ref()?;
        return Some(posed.pose.rig.seats.get(lease.index as usize)
            .and_then(|seat| seat.rider.as_ref()).and_then(|rider| rider.node).is_some());
    }
    let turret = lease.turret?;
    seats_state(&mut fleet.seats).owner.crews.turrets.iter().find(|t| t.turret == turret).map(|t| t.desc.rider)
}

/// Vehicle construction supplies fixed class facts once, without importing it again.

pub(crate) fn f2_seats_adopt_in(world_ctx: &mut crate::world::owner::World, prepared: u32) {
    let a = { let (row, heap) = world_ctx.sim.calls.args(); SeatHullFacts::take(row, heap) };
    let v = world_ctx.sim.store.vehicle.get(a.vehicle).unwrap();
    let side = HullSide { index: a.index as usize, vehicle: a.vehicle, hover: a.hover, animal: a.animal,
        turret_nodes: a.turret_nodes, weapons: a.weapons.iter().map(|&n| n as usize).collect(), last_attacker: by(v.last_attacker) };
    if prepared != 0 {
        let s = seats_state(&mut world_ctx.fleet.seats);
        let slot = a.vehicle.slot as usize;
        s.prepared.resize_with(s.prepared.len().max(slot + 1), || None);
        s.prepared_seats += v.seats.len();
        s.prepared[slot] = Some(side);
    } else { seats_state(&mut world_ctx.fleet.seats).owner.add_hull(&world_ctx.sim.store, side); }
    let pending = seats_state(&mut world_ctx.fleet.seats).prepared.iter().flatten().count();
    let n = seats_state(&mut world_ctx.fleet.seats).owner.hulls.hulls.len() + pending;
    let seats = seats_state(&mut world_ctx.fleet.seats).owner.hulls.hulls.iter().map(|h| world_ctx.sim.store.vehicle.get(h.vehicle).unwrap().seats.len()).sum::<usize>() + seats_state(&mut world_ctx.fleet.seats).prepared_seats;
    seats_state(&mut world_ctx.fleet.seats).owner.reserve_hulls(n,seats);
    let st = seats_state(&mut world_ctx.fleet.seats);
    st.leases.words.reserve(n * 512); st.leases.heap.reserve(n * 64);
    st.units.words.reserve(n * 1024); st.units.heap.reserve(n * 32);
    st.controls.words.reserve(n * 32); st.effects.words.reserve(n * 256);
    super::geometry::register_in(world_ctx, a.vehicle);
}

// Vehicles.variant appends the hull and its crew side together. Prepared
// sides carry no live index until this point; existing indices never move.
pub(crate) fn activate_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, index: usize) {
    let s = seats_state(&mut world_ctx.fleet.seats);
    let mut side = s.prepared[r.slot as usize].take().expect("prepared hull seats");
    side.index = index;
    s.owner.hulls.hulls.push(side);
}

/// Cold UnitFactory capacity covers later structural births. No rider is
/// inserted until its Unit actually joins the live registry.
// UnitFactory's future mixed ids need rider sidecar capacity too.
// Reserve it when admitting construction capacity, outside the live phase.
// Later births can occupy those slots without resizing actors or leases.
// The reservation never creates an occupant or changes registry order.
// Acquisitions can retain a previous lease until the outer publication.
// Reserve both live/retired generations and bounded recursive command room.
fn prepare_leases_in(store: &mut Store, s:&mut State,units:usize) {
    let leases=units.saturating_mul(2).saturating_add(128);
    s.lease_inputs.reserve(leases);
    s.lease_cache.resize_with(leases,||super::delta::Cache::new(128));
    s.unit_cache.resize_with(units,||super::delta::Cache::new(128));
    store.seat_lease.reserve(leases);
    let list=&mut store.seats.leases;
    list.reserve(leases.saturating_sub(list.len()));
}

pub(crate) fn reserve_unit_slots_in(world_ctx: &mut crate::world::owner::World, count: usize) {
    let s=seats_state(&mut world_ctx.fleet.seats);
    let count=count.max(s.actors.len());
    prepare_leases_in(&mut world_ctx.sim.store, s,count);
    s.actors.resize(count,false);
    s.rider_sides.resize(count,false);
    s.rider_posed.resize(count,false);
    world_ctx.fleet.riders.reserve(count);
    s.owner.hulls.riders.reserve(count);
    s.owner.crews.riders.reserve(count);
    world_ctx.sim.store.seat_lease.reserve(count);
    let leases=&mut world_ctx.sim.store.seats.leases;
    leases.reserve(count.saturating_sub(leases.len()));
}

/// Foreign actor facts keep one row per registry unit, including units that
/// board late. Their ordered table must not allocate a tree node on insertion.
// Vehicles.attachUnits grows cold rider storage before the warm phase.
// Reserve both sides of the unit boundary: later board and kill hooks can
// publish during a shot without growing an output buffer.
pub(crate) fn reserve_riders_in(world_ctx: &mut crate::world::owner::World, count: usize) { seats_state(&mut world_ctx.fleet.seats).reserve_riders(count) }

// UnitFactory.build prepares immutable seat facts once. Actor and rider
// storage is sized here, before a command or the first fleet phase.
// UnitFactory construction supplies actor presence and boarding policy once.
// The seat owner retains those immutable facts beside the canonical Unit.
// Later latches read the body and combat directly after movement.
// No whole-body JavaScript snapshot is needed when a rider changes seats.
pub(crate) fn adopt_unit_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, actor: bool, no_enter: f64) {
    let s = seats_state(&mut world_ctx.fleet.seats);
    let count = (unit.slot as usize + 1).max(s.actors.len());
    prepare_leases_in(&mut world_ctx.sim.store, s,count);
    // Seats.published may retire every occupant during a hull death. The
    // rider's cold adoption reserves both free slots and final lease rows.
    world_ctx.sim.store.seat_lease.reserve(count);
    let leases=&mut world_ctx.sim.store.seats.leases;
    leases.reserve(count.saturating_sub(leases.len()));
    s.actors.resize(count, false); s.actors[unit.slot as usize] = actor;
    s.rider_sides.resize(count, false); s.rider_posed.resize(count, false);
    s.owner.hulls.riders.reserve(count.saturating_sub(s.owner.hulls.riders.len()));
    s.owner.crews.riders.reserve(count.saturating_sub(s.owner.crews.riders.len()));
    let facts = RiderFacts { no_enter, ..Default::default() };
    s.owner.hulls.riders.insert(unit.slot, facts);
    s.owner.crews.riders.insert(unit.slot, facts);
    refresh_unit_in(world_ctx, unit);
}
// SeatsWorld reads changing flags from canonical combat at its entry slot.
// No body, intent or combat row comes back from an owned JavaScript mirror.
fn refresh_unit_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>) {
    if !crate::units::f3::active() { return; }
    let Some(u) = world_ctx.sim.store.unit.get_mut(unit) else { return };
    let zoom = crate::combat::module::zoom(&mut u.combat);
    let flags = CombatFlags { zoom_step:u.combat.zoom_step, zoom,
        first_person:u.combat.first_person, blow_apart:u.combat.blow_apart };
    if let Some(f) = seats_state(&mut world_ctx.fleet.seats).owner.hulls.riders.get_mut(&unit.slot) { f.combat = flags; }
    if let Some(f) = seats_state(&mut world_ctx.fleet.seats).owner.crews.riders.get_mut(&unit.slot) { f.combat = flags; }
}
// Vehicles' latch and crew passes visit occupied seats in their own order.
// A preceding shot may have changed zoom/death flags since input latching.
fn refresh_hull_units_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>) {
    let count = world_ctx.sim.store.vehicle.get(r).map_or(0, |v| v.seats.len());
    for index in 0..count {
        let unit = world_ctx.sim.store.vehicle.get(r).and_then(|v| v.seats[index].lease)
            .and_then(|lease| world_ctx.sim.store.seat_lease.get(lease)).and_then(|lease| lease.unit);
        if let Some(unit) = unit { refresh_unit_in(world_ctx, unit); }
    }
}
fn refresh_crews_in(world_ctx: &mut crate::world::owner::World) {
    if !crate::units::f3::active() { return; }
    for i in 0..world_ctx.sim.store.vehicles.vehicles.len() { refresh_hull_units_in(world_ctx, world_ctx.sim.store.vehicles.vehicles[i]); }
    for i in 0..seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.len() {
        let unit = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[i].lease
            .and_then(|lease| world_ctx.sim.store.seat_lease.get(lease)).and_then(|lease| lease.unit);
        if let Some(unit) = unit { refresh_unit_in(world_ctx, unit); }
    }
}
// Commands refresh only participants. Promotion may read the other riders
// of that hull; it cannot import or republish unrelated Unit bodies.
// Vehicles.board and exit commands affect only their participants.
// Refresh the touched holder and riders before entering the seat solver.
// Other hulls keep pending controls until the ordinary phase latch.
// A nested board command must not import or restore every crew member.
fn refresh_command_in(world_ctx: &mut crate::world::owner::World, a: &F2SeatCommand) {
    if !crate::units::f3::active() { return; }
    if let Some(unit) = a.unit { refresh_unit_in(world_ctx, unit); }
    if let Some(hull) = a.hull { refresh_hull_units_in(world_ctx, hull); }
    let turret = a.lease.and_then(|lease| world_ctx.sim.store.seat_lease.get(lease)).and_then(|lease| lease.turret)
        .and_then(|turret| seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t| t.turret == turret))
        .or_else(|| matches!(a.op,F2SeatOp::Man | F2SeatOp::TurretLeave | F2SeatOp::TurretRequestExit | F2SeatOp::TurretDestroyed).then_some(a.index as usize));
    if let Some(index) = turret.filter(|&index| index < seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.len()) {
        let unit = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[index].lease
            .and_then(|lease| world_ctx.sim.store.seat_lease.get(lease)).and_then(|lease| lease.unit);
        if let Some(unit) = unit { refresh_unit_in(world_ctx, unit); }
    }
}

/// Foreign units remain JS-owned until their phase moves. All these records are
/// fixed width: decoding neither clones a table nor allocates a temporary list.
fn inputs_in(world_ctx: &mut crate::world::owner::World, native_ai:bool) {
    let h = F2SeatHeader::take(&seats_state(&mut world_ctx.fleet.seats).input, &[]);
    // Scalar commands may precede the first phase or introduce a new team.
    // Retain just their affected relation pairs; phase latching uses its own
    // complete team table, sampled with the foreign registry at phase entry.
    for i in 0..h.relation_count as usize {
        let a = F2SeatRelation::take(&seats_state(&mut world_ctx.fleet.seats).input[h.relations as usize + i * F2SeatRelation::WORDS..], &[]);
        if let Some(old) = seats_state(&mut world_ctx.fleet.seats).command_relations.iter_mut().find(|v| v.a == a.a && v.b == a.b) { *old = a; }
        else { seats_state(&mut world_ctx.fleet.seats).command_relations.push(a); }
    }

    for i in 0..h.unit_count as usize {
        let start = h.units as usize + i * F2SeatUnitIn::WORDS;
        let a = F2SeatUnitIn::take(&seats_state(&mut world_ctx.fleet.seats).input[start..start + F2SeatUnitIn::WORDS], &[]);
        let u = world_ctx.sim.store.unit.ensure(a.unit);
        if u.generation != a.generation {
            u.seat = None;
            if let Some(c) = seats_state(&mut world_ctx.fleet.seats).unit_cache.get_mut(a.unit.slot as usize) { c.invalidate(); }
        }
        u.generation = a.generation; u.alive = a.alive; u.dead = a.dead; u.human = a.human;
        u.network_human = a.network_human; u.walker = a.walker; u.team = a.team; u.removed = a.removed;
        u.tick_count = a.tick_count; u.intent = a.intent; u.combat.dead = a.combat_dead;
        u.targetable = a.targetable; u.combat.mounted = a.mounted; u.combat.gun_underwater = a.gun_underwater; u.combat.aim_override = a.aim_override;
        let id = a.unit.slot as usize;
        if seats_state(&mut world_ctx.fleet.seats).actors.len() <= id { seats_state(&mut world_ctx.fleet.seats).actors.resize(id + 1, false); seats_state(&mut world_ctx.fleet.seats).rider_sides.resize(id + 1, false); }
        seats_state(&mut world_ctx.fleet.seats).actors[id] = a.has_actor;
        let f = RiderFacts { no_enter: a.no_enter, combat: CombatFlags { zoom_step: a.zoom_step, zoom: a.zoom, first_person: a.first_person, blow_apart: a.blow_apart } };
        seats_state(&mut world_ctx.fleet.seats).owner.hulls.riders.insert(a.unit.slot, f);
        seats_state(&mut world_ctx.fleet.seats).owner.crews.riders.insert(a.unit.slot, f);
    }
    for i in 0..h.soldier_count as usize {
        let a = F2SeatSoldier::take(&seats_state(&mut world_ctx.fleet.seats).input[h.soldiers as usize + i * F2SeatSoldier::WORDS..], &[]);
        world_ctx.sim.store.unit.get_mut(a.unit).unwrap().soldier = UnitBody::Soldier(a.soldier);
    }
    // Stationary gun modifiers follow the same generated changed-row input
    // as hull guns. Their bodies, clocks, ammunition and heat stay native.
    for i in 0..h.turret_count as usize {
        let a = F2TurretInput::take(&seats_state(&mut world_ctx.fleet.seats).input[h.turrets as usize + i * F2TurretInput::WORDS..], &[]);
        if let Some(w) = seats_state(&mut world_ctx.fleet.seats).weapons[a.index as usize].as_mut() {
            w.random_binding = a.input.random_binding; w.state.random = a.input.has_random;
            w.state.floor = a.input.floor;
            if w.state.kick < w.state.floor { w.state.kick = w.state.floor; }
            w.state.spread_scale = a.input.spread_scale; w.state.sniper_scale = a.input.sniper_scale;
            w.state.yaw_spread_override = a.input.yaw_spread_override; w.channel = a.input.channel;
            { let turret = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[a.index as usize].turret; touch_turret_in(&mut world_ctx.fleet, turret); }
        }
    }
    for i in 0..if native_ai {0}else{h.control_count as usize} {
        let start = h.controls as usize + i * SeatAiControl::WORDS;
        let a = SeatAiControl::take(&seats_state(&mut world_ctx.fleet.seats).input[start..start + SeatAiControl::WORDS], &[]);
        let c = SeatControl { driven: a.driven, intent: intent(a.intent) };
        seats_state(&mut world_ctx.fleet.seats).owner.crews.controls.insert(control_key(a.seat_unit), c);
        seats_state(&mut world_ctx.fleet.seats).owner.hulls.controls.insert(control_key(a.seat_unit), c);
    }
}
// Vehicles.destroyedCrew reads each hull's latest attacker. A nested native
// health command can change it after the sidecar was constructed, so refresh
// from the canonical row before moving the owner into a solver.
fn refresh_hulls(hulls: &mut Hulls, st: &Store) {
    for side in &mut hulls.hulls {
        if let Some(v) = st.vehicle.get(side.vehicle) { side.last_attacker = by(v.last_attacker); }
    }
}

/// Vehicles.latchCrews runs between native spawn clocks and hull capture.
/// Freeze JS-owned facts once, before spawn callbacks can issue commands.
/// Commands import their touched actors separately; they cannot replace the
/// other crews' pending intent by reusing the command input buffer.
pub(crate) fn import_phase_input_in(world_ctx: &mut crate::world::owner::World, native_ai:bool) { inputs_in(world_ctx, native_ai); }

// Vehicles.latchCrews preserves the starting crew order, including exits
// and promotion before driving input is frozen. Move the Store shell rather
// than clone rows; publish_hulls makes it available again around hooks.
// Vehicles.update latches bot and network controls after seat transitions.
// Promotion can change which occupant supplies a hull's pilot keys.
// The canonical lease therefore selects the source at this point.
// Held input and one-shot edges keep their separate consumption rules.
pub(crate) fn latch_in(world_ctx: &mut crate::world::owner::World, time: f64, services: Option<crate::units::f3::WorldServices<'_>>) {
    refresh_crews_in(world_ctx);
    seats_state(&mut world_ctx.fleet.seats).depth += 1;
    let n = seats_state(&mut world_ctx.fleet.seats).owner.hulls.hulls.len();
    for i in 0..n {
        let r = seats_state(&mut world_ctx.fleet.seats).owner.hulls.hulls[i].vehicle;
        if let Some(v) = world_ctx.sim.store.vehicle.get(r) {
            if !v.seats.iter().any(|s| s.lease.is_some()) { continue; }
            touch_hull_in(world_ctx, r);

        }
    }
    let mut st = std::mem::take(&mut world_ctx.sim.store);
    let mut hulls = std::mem::take(&mut seats_state(&mut world_ctx.fleet.seats).owner.hulls);
    hulls.time = time; hulls.latched.clear(); hulls.made.clear(); refresh_hulls(&mut hulls, &st);
    let mut world = World::with_services_in(world_ctx, services);
    hulls.latch_crews(&mut st, &mut world);
    let world_ctx = world.world_ctx;
    seats_state(&mut world_ctx.fleet.seats).owner.hulls = hulls; world_ctx.sim.store = st; seats_state(&mut world_ctx.fleet.seats).depth -= 1;
    // Flight reads the newly frozen controls, never last tick's JS mirror.
    for i in 0..world_ctx.sim.store.vehicles.vehicles.len() {
        let r = world_ctx.sim.store.vehicles.vehicles[i];
        let v = world_ctx.sim.store.vehicle.get(r).unwrap();
        if let Some(l) = v.seats.first().and_then(|s| s.lease).and_then(|l| world_ctx.sim.store.seat_lease.get(l)) {
            if v.flying {
                // Flight uses the lease just frozen by latchCrews. Network
                // pilot keys retain their separate sampled row; copying the
                // old mirrored pressed flags here would replay consumed edges.
                let a = &l.input;
                let given = &mut world_ctx.fleet.flight_inputs;
                if given.len() <= r.slot as usize { given.resize(r.slot as usize + 1, None); }
                let keys = given[r.slot as usize].and_then(|(_, keys)| keys);
                given[r.slot as usize] = Some((FlightInput { forward: a.forward, right: a.right, turn: a.turn, pitch: a.pitch,
                    up: a.up, down: a.down, takeoff: f64::NAN, stick_x: None, stick_y: None,
                    boost: a.boost.map_or(f64::NAN, |b| if b { 1.0 } else { 0.0 }), trick: a.trick.map_or(f64::NAN, |b| if b { 1.0 } else { 0.0 }),
                    immelmann: a.immelmann.map_or(f64::NAN, |b| if b { 1.0 } else { 0.0 }), roll_lock: f64::NAN, target_lock: f64::NAN,
                    bot: f64::NAN, response_curve: f64::NAN, no_assist: false }, keys));
            }
        }
        if let Some(p) = world_ctx.fleet.poses.get_mut(r.slot as usize).and_then(Option::as_mut) {
            for (s, seat) in p.pose.seats.iter_mut().zip(&v.seats) {
                if let Some(l) = seat.lease.and_then(|l| world_ctx.sim.store.seat_lease.get(l)) {
                    s.aiming = l.input.aiming; s.aim = vec(&l.input.aim);
                } else { s.aiming = false; }
            }
        }
    }
    // Latching writes canonical seat links and controls without a physics
    // store. Reload each working row once before its first native substep.
    for i in 0..world_ctx.sim.store.vehicles.vehicles.len() { super::mark_in(&mut world_ctx.fleet, world_ctx.sim.store.vehicles.vehicles[i]); }
}

// Vehicles.syncRider publishes only the Soldier fields a seat owns. The
// remaining soldier state stays foreign. Copying a full old Soldier here
// would overwrite movement or combat writes made earlier in the tick.
fn body(s: &Soldier) -> F2SeatBody {
    F2SeatBody { position: s.position, previous: s.previous, settled: s.settled, velocity: s.velocity, normal: s.normal,
        aim_yaw: s.aim_yaw, aim_pitch: s.aim_pitch, air: s.air, posture: s.posture, grounded: s.grounded,
        dive: s.dive, dive_from: s.dive_from, lock: s.lock, look_x: s.look_x, look_y: s.look_y,
        fuel: s.fuel, state_time: s.state_time, pass_through: s.pass_through }
}
// SeatInput.latch consumes action edges on their source intent. Send only
// those edge fields back; held analogs remain the AI's pending input for
// its next update and must not be restored from a stale mirror.
fn edges(i: &Intent) -> F2SeatEdges { F2SeatEdges { reload: i.reload, r#use: i.r#use, exit: i.exit, jump: i.jump, crouch: i.crouch, prone: i.prone,
    next_primary: i.next_primary, previous_primary: i.previous_primary, next_secondary: i.next_secondary, zoom: i.zoom } }

/// Lease and body publication contains only rows touched since the last hook.
// Publication observes lazy rider geometry before later rounds and AI.
// Deferring facade bytes must retain that gameplay observation point.
pub(crate) fn sample_changed_in(world_ctx: &mut crate::world::owner::World) {
    for i in 0..seats_state(&mut world_ctx.fleet.seats).owner.changed.hulls.len() { { let hull = seats_state(&mut world_ctx.fleet.seats).owner.changed.hulls[i]; super::riders::hull_in(world_ctx, hull, false); } }
    for at in 0..seats_state(&mut world_ctx.fleet.seats).owner.changed.turrets.len() {
        let r=seats_state(&mut world_ctx.fleet.seats).owner.changed.turrets[at];
        if let Some(index)=seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t|t.turret==r) {sample_turret_rider_in(&world_ctx.sim.store, &world_ctx.pose.clips, &mut world_ctx.fleet.riders, world_ctx.fleet.seats.as_mut().unwrap(),index);}
    }
}
pub(crate) fn publish_in(world_ctx: &mut crate::world::owner::World) -> *const f64 {publish_sampled_in(world_ctx, false)}
pub(super) fn publish_sampled_in(world_ctx: &mut crate::world::owner::World, sampled:bool) -> *const f64 {
    if crate::world::deferred() {sample_changed_in(world_ctx);return seats_state(&mut world_ctx.fleet.seats).io.as_ptr();}

    if !sampled {
        for i in 0..seats_state(&mut world_ctx.fleet.seats).owner.changed.hulls.len() { { let hull = seats_state(&mut world_ctx.fleet.seats).owner.changed.hulls[i]; super::riders::hull_in(world_ctx, hull, false); } }
    }
    let state = world_ctx.fleet.seats.as_mut().unwrap();
    state.ordnance_output.begin(if state.ordnance_dirty { 1 } else { 0 }, Ordnance::EXPORT_STRIDE);
    if state.ordnance_dirty {
        let (row, heap) = state.ordnance_output.row(0); world_ctx.sim.store.ordnance.put_export(row, heap);
    }
    state.ordnance_output.finish(); state.ordnance_dirty = false;
    let st = &mut world_ctx.sim.store;
    for &r in &state.owner.changed.hulls {
        world_ctx.fleet.scheduler.mark(&mut world_ctx.fleet.geometry, r);
        if let Some(v) = st.vehicle.get(r) { for s in &v.seats { if let Some(r) = s.lease {
            if !state.owner.changed.leases.contains(&r) { state.owner.changed.leases.push(r); }
        } } }
    }
    // SeatLease.emitters can still refer to retired rows. Send their final
    // generation first, then changed live rows; never reinterpret a retired
    // reference as the new occupant of the same slot.
    let retired = st.seat_lease.retired.len();
    let live = state.owner.changed.leases.iter().filter(|&&r| st.seat_lease.contains(r)).count();
    state.leases.begin(retired + live, F2Delta::WORDS); state.leases.set_retired(retired);
    for (i, &(r, ref l)) in st.seat_lease.retired.iter().enumerate() {
        if state.lease_cache.len() <= r.slot as usize { state.lease_cache.resize_with(r.slot as usize + 1, || super::delta::Cache::new(128)); }
        let c = &mut state.lease_cache[r.slot as usize];
        c.encode(r.slot,r.gen,SeatLease::EXPORT_STRIDE, |row,heap| { row[0] = r.slot as f64; row[1] = r.gen as f64; l.put_export(row,heap); });
        let (row,heap) = state.leases.row(i); c.delta.value.put(row,heap);
    }
    let mut index = retired;
    for &r in &state.owner.changed.leases { if let Some(l) = st.seat_lease.get(r) {
        if state.lease_cache.len() <= r.slot as usize { state.lease_cache.resize_with(r.slot as usize + 1, || super::delta::Cache::new(128)); }
        let c = &mut state.lease_cache[r.slot as usize];
        if c.encode(r.slot,r.gen,SeatLease::EXPORT_STRIDE, |row,heap| { row[0] = r.slot as f64; row[1] = r.gen as f64; l.put_export(row,heap); }) {
            let (row,heap) = state.leases.row(index); index += 1; c.delta.value.put(row,heap);
        }
    } }
    state.leases.words[0] = index as f64; state.leases.finish();
    for (_,lease) in st.seat_lease.retired.drain(..) {state.lease_inputs.recycle(lease.input);}
    // syncCrews preserves the foreign Soldier fields it does not own. These
    // compact results update body and lease readers without importing a whole
    // old unit over movement or combat writes made by a nested listener.
    let owned_units = crate::units::f3::active();
    state.units.begin(if owned_units { 0 } else { state.owner.changed.units.len() }, F2Delta::WORDS);
    let mut unit_count = 0;
    for &r in &state.owner.changed.units {
        // F3 publishes the complete touched body and combat together. A second
        // seat mirror would overwrite it and run combat setters during a hook.
        if owned_units { continue; }
        let u = st.unit.get(r).expect("changed seat unit"); let s = soldier(u).expect("seated soldier");
        let riders = if state.rider_sides.get(r.slot as usize) == Some(&true) { &state.owner.crews.riders } else { &state.owner.hulls.riders };
        let zoom = riders.get(&r.slot).map_or(0.0, |r| r.combat.zoom_step);
        let out = F2SeatUnitOut { unit: r, seat: u.seat, targetable: u.targetable, tick_count: u.tick_count,
            soldier: body(s), intent: edges(&u.intent), mounted: u.combat.mounted, gun_underwater: u.combat.gun_underwater,
            aim_override: u.combat.aim_override, zoom_step: zoom, rider_posed: state.rider_posed.get(r.slot as usize).copied().unwrap_or(false) };
        if let Some(flag) = state.rider_posed.get_mut(r.slot as usize) { *flag = false; }
        if state.unit_cache.len() <= r.slot as usize { state.unit_cache.resize_with(r.slot as usize + 1, || super::delta::Cache::new(128)); }
        let c = &mut state.unit_cache[r.slot as usize];
        if c.encode(r.slot,r.gen,F2SeatUnitOut::WORDS,|row,heap| out.put(row,heap)) {
            let (row,heap) = state.units.row(unit_count); unit_count += 1; c.delta.value.put(row,heap);
        }
    }
    state.units.words[0] = unit_count as f64; state.units.finish();
    state.controls.begin(state.owner.hulls.latched.len() + state.owner.crews.latched.len(), F2Latched::WORDS);
    for (i, &control) in state.owner.hulls.latched.iter().enumerate() {
        let (row, heap) = state.controls.row(i); F2Latched { control }.put(row, heap);
    }
    let base = state.owner.hulls.latched.len();
    for (i, &control) in state.owner.crews.latched.iter().enumerate() {
        let (row, heap) = state.controls.row(base + i); F2Latched { control }.put(row, heap);
    }
    state.controls.finish(); state.owner.hulls.latched.clear(); state.owner.crews.latched.clear();
    state.effects.begin(state.body_effects.len(), F2SeatEffect::WORDS);
    for (i, e) in state.body_effects.iter().enumerate() { let (row, heap) = state.effects.row(i); e.put(row, heap); }
    state.effects.finish(); state.body_effects.clear();
    publish_turrets_in(st, &world_ctx.pose.clips, &mut world_ctx.fleet.riders, state,!sampled);
    state.owner.changed.clear();
    let riders = world_ctx.fleet.riders.publish();
    F2SeatIo { emitter_mask: EMITTER_MASK, ordnance: state.ordnance_output.ptr() as usize as u32, descriptor_words: F2SeatIo::WORDS as u32, input: state.input.as_mut_ptr() as usize as u32, capacity: INPUT_CAPACITY as u32,
        header_words: F2SeatHeader::WORDS as u32, unit_words: F2SeatUnitIn::WORDS as u32, control_words: SeatAiControl::WORDS as u32,
        look_words: F2SeatSample::WORDS as u32, leases: state.leases.ptr() as usize as u32, units: state.units.ptr() as usize as u32,
        controls: state.controls.ptr() as usize as u32, effects: state.effects.ptr() as usize as u32,
        turrets: state.turret_output.ptr() as usize as u32, events: state.event_output.ptr() as usize as u32, riders: riders as usize as u32, ticks: state.owner.hulls.ticks, turret_ticks: st.turret_seats.ticks, turret_time: st.turret_seats.time }.put(&mut state.io, &mut state.effects.heap);
    state.io.as_ptr()
}

struct World<'w, 'a> { world_ctx: &'w mut crate::world::owner::World, turret: bool, services: Option<crate::units::f3::WorldServices<'a>> }
impl<'w, 'a> World<'w, 'a> {
    fn with_services_in(world_ctx: &'w mut crate::world::owner::World, services: Option<crate::units::f3::WorldServices<'a>>) -> Self {
            Self { world_ctx, turret: false, services }
    }
    fn action(native: bool, a: F2SeatAction) -> F2SeatAnswer {
            // Controller-state and controllable callbacks are foreign AI queries.
            // Publish their touched readers at the actual import, not native commits.
            if native { crate::units::f3::foreign_barrier(); }
            import_f2_seat_action(&a)
    }
    // SeatsWorld.kill must publish before entering foreign combat. Its death
    // listener can synchronously board or unseat another unit; import the
    // affected foreign facts afterward without replacing unrelated crews.
    fn killed(&mut self, unit: u32, owner: By) { let world_ctx = &mut *self.world_ctx;
            if let Some(services) = &mut self.services {
                // SeatsWorld.kill follows lease removal and body placement. It
                // must finish native death and its blast before the next occupant.
                crate::combat::live::damage(unit_ref(unit), f64::INFINITY, attacker_in(world_ctx, owner), services.reborrow());
                return;
            }
            touch_unit_in(&mut world_ctx.fleet, unit_ref(unit)); super::publish_in(world_ctx);
            Self::action(self.services.is_some(), F2SeatAction { kind: F2SeatActionKind::Kill, unit: Some(unit_ref(unit)), attacker: attacker_in(world_ctx, owner), ..Default::default() });
            inputs_in(world_ctx, false);
    }
    // seatBody, unseatBody and placeOnFoot also have actor presentation hooks.
    // Queue those hooks with the native body result; the mirror is current
    // before JavaScript applies them, even during a nested death.
    fn emit_body(&mut self, kind: SeatEffectKind, unit: u32, yaw: f64, point: [f64;3]) { let world_ctx = &mut *self.world_ctx;
            seats_state(&mut world_ctx.fleet.seats).rider_sides[unit as usize] = self.turret;
            touch_unit_in(&mut world_ctx.fleet, unit_ref(unit)); seats_state(&mut world_ctx.fleet.seats).body_effects.push(F2SeatEffect { kind, unit: unit_ref(unit), yaw, point });
    }
    // Game's onBoard/onUnboard/onMove listeners only journal the transition.
    // Sample their refs and position now, before another crew can move or kill
    // this body. Controller-state and control construction remain AI imports.
    fn hook(&mut self, kind: F2SeatActionKind, unit: Option<u32>, h: Holder, index: usize, post: Option<Ref<Post>>, reason: Reason) { let world_ctx = &mut *self.world_ctx;
            let (holder, turret) = holder(h);
            if !turret { touch_hull_in(world_ctx, world_ctx.sim.store.vehicles.vehicles[holder as usize]); } else { { let turret = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[holder as usize].turret; touch_turret_in(&mut world_ctx.fleet, turret); } }
            if let Some(unit) = unit {
                let reference = unit_ref(unit);
                touch_unit_in(&mut world_ctx.fleet, reference);
                if self.services.is_some() && !turret && kind == F2SeatActionKind::Unboarded && reason == Reason::Destroyed {
                    let hull = world_ctx.sim.store.vehicles.vehicles[holder as usize];
                    let shown = world_ctx.fleet.poses[hull.slot as usize].as_ref().unwrap().pose.rig.seats[index]
                        .rider.as_ref().and_then(|rider| rider.node).is_some();
                    if !shown { crate::units::f3::hide_destroyed_crew(reference); }
                }
            }
            if self.services.is_some() {
                let mut event = F3SeatNotice {
                    kind, unit: unit.map(unit_ref), index: index as u32, post,
                    reason: why(reason), ..Default::default()
                };
                let member = if turret {
                    let r = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[holder as usize].turret;
                    event.turret = Some(r);
                    crate::units::f3_props::member(RepairTarget::Turret(r))
                } else {
                    let r = world_ctx.sim.store.vehicles.vehicles[holder as usize];
                    event.hull = Some(r);
                    Some(crate::units::f3_hulls::member(r))
                };
                if let Some(member) = member {
                    event.has_target = true;
                    event.target = F3LifeRef {
                        id: crate::units::registry::target_id(member) as f64,
                        generation: crate::units::f3_fleet::life(member),
                    };
                }
                if let Some(reference) = event.unit {
                    let u = world_ctx.sim.store.unit.get(reference).unwrap();
                    event.has_source = true;
                    event.source = F3LifeRef { id: reference.slot as f64, generation: u.generation };
                    event.team = u.team;
                    event.position = v3(&u.position());
                }
                if kind == F2SeatActionKind::Boarded {
                    event.position = if let Some(r) = event.hull {
                        world_ctx.sim.store.vehicle.get(r).unwrap().position
                    } else { v3(&seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[holder as usize].desc.position) };
                }
                event.emit(&mut world_ctx.sim.events);
                return;
            }
            super::publish_in(world_ctx);
            Self::action(self.services.is_some(), F2SeatAction { kind, unit: unit.map(unit_ref), holder, turret, index: index as f64, post, reason: why(reason), ..Default::default() });
    }
}
impl SeatWorld for World<'_, '_> {
    fn lease_input(&mut self)->Option<crate::gen::SeatInput> { let world_ctx = &mut *self.world_ctx;seats_state(&mut world_ctx.fleet.seats).lease_inputs.acquire()}
    // TurretSeats.update keeps its starting lease snapshot across callbacks.
    // Exchange the Store and crew shells around a hook so reentrant commands
    // operate on the live owner, with no outstanding borrow into JavaScript.
    fn publish_crews(&mut self, st: &mut Store, crews: &mut Crews) { let world_ctx = &mut *self.world_ctx;
            std::mem::swap(st, &mut world_ctx.sim.store); std::mem::swap(crews, &mut seats_state(&mut world_ctx.fleet.seats).owner.crews);
    }
    fn resume_crews(&mut self, st: &mut Store, crews: &mut Crews) { let world_ctx = &mut *self.world_ctx;
            std::mem::swap(st, &mut world_ctx.sim.store); std::mem::swap(crews, &mut seats_state(&mut world_ctx.fleet.seats).owner.crews);
    }
    // TurretSeats.man allocates a generation-qualified lease once at boarding.
    // Old emitters may retain the retired reference; slab reuse must create a
    // new generation rather than retarget that old JavaScript lease object.
    fn crew_lease(&mut self, _: &mut Store, lease: SeatLease) -> Ref<SeatLease> { let world_ctx = &mut *self.world_ctx;
            if let Some(unit) = lease.unit { super::riders::life_in(world_ctx, unit,lease.generation); }
            let r = world_ctx.sim.store.seat_lease.insert(lease);
            if let Some(ai)=self.services.as_mut().and_then(|services|services.ai.as_mut()) {ai.lease_created(r);}
            touch_lease_in(&mut world_ctx.fleet, r); r
    }
    // Human hull riders also have leases, although they never construct the
    // bot-only SeatUnit. Publish that identity at the same acquisition slot.
    fn hull_lease(&mut self, store: &mut Store, lease: SeatLease) -> Ref<SeatLease> { let world_ctx = &mut *self.world_ctx;
            // Hull acquisition lends the active Store directly; unlike the crew
            // foreign callback, it has not exchanged that Store with world_ctx.sim.
            if let Some(unit) = lease.unit { super::riders::life_in(world_ctx, unit,lease.generation); }
            let r = store.seat_lease.insert(lease);
            if let Some(ai)=self.services.as_mut().and_then(|services|services.ai.as_mut()) {ai.lease_created(r);}
            touch_lease_in(&mut world_ctx.fleet, r); r
    }
    // TurretSeats.pose changes the occupant and turret together. Mark both
    // reader rows before a shot can enter its launch hook. The posed flag
    // controls the existing animation hook without another JS rig sample.
    fn crew_posed(&mut self, r: Ref<SeatLease>) { let world_ctx = &mut *self.world_ctx;
            touch_lease_in(&mut world_ctx.fleet, r);
            if let Some(unit) = world_ctx.sim.store.seat_lease.get(r).and_then(|l| l.unit) { touch_unit_in(&mut world_ctx.fleet, unit); }
            if let Some(t) = world_ctx.sim.store.seat_lease.get(r).and_then(|l| l.turret) {
                let i = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|v| v.turret == t).unwrap(); seats_state(&mut world_ctx.fleet.seats).posed[i] = true; touch_turret_in(&mut world_ctx.fleet, t);
            }
    }

    // Vehicles seat operations use the same ownership exchange as turrets.
    // The outer solver keeps its lists, while synchronous hooks reach the live
    // Store; resume_hulls takes back any nested command's changes.
    fn publish_hulls(&mut self, st: &mut Store, hulls: &mut Hulls) { let world_ctx = &mut *self.world_ctx;
            std::mem::swap(st, &mut world_ctx.sim.store); std::mem::swap(hulls, &mut seats_state(&mut world_ctx.fleet.seats).owner.hulls);
    }
    fn resume_hulls(&mut self, st: &mut Store, hulls: &mut Hulls) { let world_ctx = &mut *self.world_ctx;
            std::mem::swap(st, &mut world_ctx.sim.store); std::mem::swap(hulls, &mut seats_state(&mut world_ctx.fleet.seats).owner.hulls);
    }
    // SeatsWorld.cast and overlapsSphere run against the shared native world.
    // Keeping them here lets each exit observe the preceding crew's placement.
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<Ref<Body>>) -> Option<SeatHit> { let world_ctx = &mut *self.world_ctx;
            let collision = world_ctx.fleet.collision.as_mut()?;
            if let Some(body) = ignore {
                if collision.body_reference(body.slot) != Some(body) { return None; }
            }
            let hit = collision.cast_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), origin, direction, far, ignore.map(|b| b.slot))??;
            let body = match hit.body { Some(slot) => Some(collision.body_reference(slot)?), None => None };
            Some(SeatHit { point: hit.point, normal: hit.normal, distance: hit.distance, body })
    }
    fn overlaps_sphere(&mut self, center: &Vector3, radius: f64, ignore: Option<Ref<Body>>) -> bool { let world_ctx = &mut *self.world_ctx;
            let Some(collision)=world_ctx.fleet.collision.as_mut() else {return true};
            if ignore.is_some_and(|body|collision.body_reference(body.slot)!=Some(body)) {return true;}
            collision.overlaps_sphere_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), center, radius, ignore.map(|b| b.slot))
    }
    // turretAimPoint still queries the foreign unit registry. Publish native
    // body samples first because its lazy hit-shape reader can inspect them.
    // Solid-world casts beside this query remain entirely in the native kernel.
    fn raycast_units(&mut self, from: &Vector3, direction: &Vector3, far: f64, unit: Option<u32>) -> Option<f64> { let world_ctx = &mut *self.world_ctx;
            if self.services.is_some() { return crate::units::f3_fleet::raycast(from,direction,far,unit); }
            super::publish_in(world_ctx);
            Self::action(self.services.is_some(), F2SeatAction { kind: F2SeatActionKind::Raycast, unit: unit.map(unit_ref), point: v3(from),
                direction: v3(direction), yaw: far, ..Default::default() }).value
    }
    fn relation(&mut self, a: f64, b: f64) -> f64 { let world_ctx = &mut *self.world_ctx;
            if self.services.is_some() { return crate::units::f3::relation(a,b); }
            if seats_state(&mut world_ctx.fleet.seats).command_depth > 0 || world_ctx.fleet.teams.is_empty() {
                if let Some(r) = seats_state(&mut world_ctx.fleet.seats).command_relations.iter().find(|r| r.a == a && r.b == b) { return r.value; }
            }
            FlightWorld::relation(&mut super::JsWorld::new(u32::MAX), Some(a), Some(b))
    }
    // placeOnFoot retains Soldier.spawn's reset and ground snap. The spawn
    // world now casts directly into the same native kernel as the exit search;
    // it cannot call back through the removed JavaScript seat query handlers.
    fn stand_up(&mut self, u: &mut Unit, x: f64, y: f64, z: f64, yaw: f64) {
            crate::seats::module::stand_up(u, x, y, z, yaw);
    }
    // Vehicles.destroyedCrew clears seat links before killing the occupants.
    // Temporarily publish the proper solver owner so a combat listener sees
    // all cleared leases and can safely enter another seat command.
    fn kill(&mut self, st: &mut Store, seating: Seating<'_>, unit: u32, owner: By) {
            match seating {
                Seating::Hulls(hulls) => {
                    self.publish_hulls(st, hulls); self.killed(unit, owner); self.resume_hulls(st, hulls);
                }
                // TurretSeats.vacate already transfers its command before kill;
                // Hulls.destroyedCrew enters the hook with its local rows.
                Seating::Crews(_) => self.killed(unit, owner),
            }
    }
    // Vehicles.vacate uses the completed riderDropPoint at kill entry. Reuse
    // the native sample, including the animal pelvis adjustment, rather than
    // reading a scene node whose presentation frame may already differ.
    fn rider_drop(&mut self, _h: Holder, _index: usize, unit: u32) -> Option<Vector3> { let world_ctx = &mut *self.world_ctx;
            super::riders::drop_in(world_ctx, unit_ref(unit))
    }

    fn sync_rider_rows(&mut self, rows: crate::seats::RiderRows<'_>, r: Ref<SeatLease>, unit: &mut Unit) { let world_ctx = &mut *self.world_ctx;
            // The solver lends lease and hull rows separately from its mutable
            // Unit row, so rider sampling cannot alias the occupant.
            let lease = rows.leases.get(r).expect("current rider lease");
            let Some(id) = lease.unit else { return };
            if !seats_state(&mut world_ctx.fleet.seats).actors.get(id.slot as usize).copied().unwrap_or(false) { return; }
            let vref = lease.vehicle.unwrap(); let index = lease.index as usize;
            let v = rows.vehicles.get(vref).expect("current rider hull");
            let p = world_ctx.fleet.poses[vref.slot as usize].as_mut().unwrap();
            let node = p.pose.rig.seats.get(index).and_then(|s| s.rider.as_ref()).and_then(|r| r.node);
            let mut point = vec(&v.position);
            if let Some(node) = node { p.pose.rig.tree.get_world_position(node, &mut point); }
            else { point.y += v.size[1] * 0.5; }
            if let Some(s) = soldier_mut(unit) {
                s.position = v3(&point); s.previous = s.position; s.settled = s.position;
                s.velocity = v.velocity; s.aim_yaw = v.seats[index].aim_yaw; s.aim_pitch = v.seats[index].pitch;
            }
            unit.tick_count += 1.0; rider_posed_in(&mut world_ctx.fleet, id);
    }

    // Vehicles.promote leaves a bot alone when its controller is boarding
    // something else. This foreign AI fact is read at the decision slot;
    // turning it into an earlier cached value would change takeover order.
    fn controller_state(&mut self, unit: Option<u32>) -> Option<f64> { let world_ctx = &mut *self.world_ctx;
            if let Some(ai)=self.services.as_ref().and_then(|services|services.ai.as_ref()) {
                return ai.controller_state(&world_ctx.sim.store,unit.map(unit_ref));
            }
            Self::action(self.services.is_some(), F2SeatAction { kind: F2SeatActionKind::Controller, unit: unit.map(unit_ref), ..Default::default() }).value
    }
    // SeatsWorld.controllable constructs the AI's SeatUnit facade on boarding.
    // Only that facade remains foreign: canonical seat identity and input
    // latching belong to the native owner, including a newly promoted pilot.
    // Classic can construct a seat controller at the original boarding hook.
    // Publish the current lease before asking that foreign AI query.
    // Its constructor may bind a shared random stream for mounted fire.
    // Resume native work with the accepted binding, not an older mirror value.
    fn controllable(&mut self, lease: Ref<SeatLease>, index: usize, _: SeatRole) -> Option<Ref<SeatUnit>> { let world_ctx = &mut *self.world_ctx;
            if let Some(ai)=self.services.as_mut().and_then(|services|services.ai.as_mut()) {
                let control=ai.controllable(&mut world_ctx.sim.store,&mut seats_state(&mut world_ctx.fleet.seats).owner,lease)?;
                touch_lease_in(&mut world_ctx.fleet, lease);return Some(control);
            }
            touch_lease_in(&mut world_ctx.fleet, lease); super::publish_in(world_ctx);
            let answer = Self::action(self.services.is_some(), F2SeatAction { kind: F2SeatActionKind::Control, lease: Some(lease),
                turret:self.turret, index: index as f64, ..Default::default() });
            // Controller attachment can replace the seat stream before a later
            // weapon fires in this same span. Import only that numeric binding.
            if self.services.is_some() && answer.bound {
                let l = world_ctx.sim.store.seat_lease.get(lease).unwrap();
                let (hull, index, turret) = (l.vehicle, l.index as usize, l.turret);
                if let Some(hull) = hull { super::firing::bind_random_in(world_ctx, hull,index,answer.random_binding); }
                if let Some(turret) = turret {
                    let index = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t| t.turret == turret).unwrap();
                    if let Some(w) = seats_state(&mut world_ctx.fleet.seats).weapons[index].as_mut() {
                        w.random_binding = answer.random_binding; w.state.random = answer.random_binding != u32::MAX;
                    }
                }
            }
            let r = answer.control?;
            seats_state(&mut world_ctx.fleet.seats).owner.hulls.controls.insert(control_key(r), SeatControl::default());
            seats_state(&mut world_ctx.fleet.seats).owner.crews.controls.insert(control_key(r), SeatControl::default()); Some(r)
    }
    fn boarded(&mut self, unit: u32, h: Holder, index: usize) { self.hook(F2SeatActionKind::Boarded, Some(unit), h, index, None, Reason::Exit); }
    fn unboarded(&mut self, unit: Option<u32>, h: Holder, index: usize, post: Option<Ref<Post>>, reason: Reason) { self.hook(F2SeatActionKind::Unboarded, unit, h, index, post, reason); }
    fn moved(&mut self, unit: Option<u32>, h: Holder, index: usize) { self.hook(F2SeatActionKind::Moved, unit, h, index, None, Reason::Exit); }
    fn seated(&mut self, unit: u32) { self.emit_body(SeatEffectKind::Seated, unit, 0.0, [0.0;3]); }
    fn unseated(&mut self, unit: u32, yaw: f64) { self.emit_body(SeatEffectKind::Unseated, unit, yaw, [0.0;3]); }
    fn placed(&mut self, unit: u32, yaw: f64) { self.emit_body(SeatEffectKind::Placed, unit, yaw, [0.0;3]); }
    fn dropped(&mut self, unit: u32, p: &Vector3) { self.emit_body(SeatEffectKind::Dropped, unit, 0.0, v3(p)); }
    // seatWeaponPolicy resets the shared weapon when the occupant changes.
    // Keep the spread floor, kick clamp and RNG assignment order: a human
    // may take a weapon a bot already modified earlier in the same tick.
    fn seat_policy(&mut self, h: Holder, index: usize, player: bool, random: bool) { let world_ctx = &mut *self.world_ctx;
            let (holder, turret) = holder(h); if !turret { touch_hull_in(world_ctx, world_ctx.sim.store.vehicles.vehicles[holder as usize]); } else { { let turret = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[holder as usize].turret; touch_turret_in(&mut world_ctx.fleet, turret); } }
            if turret {
                if let Some(w) = seats_state(&mut world_ctx.fleet.seats).weapons[holder as usize].as_mut() {
                    let d = w.state.level(w.class.as_ref().unwrap());
                    w.state.floor = if player { -d.get(crate::combat::defs::K::SpreadThreshold).or(0.0) } else { 0.0 };
                    if w.state.kick < w.state.floor { w.state.kick = w.state.floor; }
                    w.state.spread_scale = 1.0; w.state.sniper_scale = 0.0; w.state.yaw_spread_override = None;
                }
            }
            if self.services.is_some() {
                if turret { if let Some(w) = seats_state(&mut world_ctx.fleet.seats).weapons[holder as usize].as_mut() {
                    w.state.random = random;
                    w.random_binding = if random { crate::units::f3::seat_binding() } else { u32::MAX };
                } }
                else { super::firing::policy_in(world_ctx, world_ctx.sim.store.vehicles.vehicles[holder as usize], index, player, random); }
                return;
            }
            super::publish_in(world_ctx);
            let answer = Self::action(self.services.is_some(), F2SeatAction { kind: F2SeatActionKind::Policy, holder, turret, index: index as f64, flag: player, second: random, ..Default::default() });
            if turret { if let Some(w) = seats_state(&mut world_ctx.fleet.seats).weapons[holder as usize].as_mut() { w.state.random = answer.flag; } }
            else { super::firing::policy_in(world_ctx, world_ctx.sim.store.vehicles.vehicles[holder as usize], index, player, answer.flag); }
    }
}

// ClassicCombatBinding attaches after the board command returns. Its numeric
// stream is a qualified scalar write, not a weapon table or body import.
// Classic attaches its mounted weapon generator after boarding returns.
// Validate both the Unit life and its current qualified lease first.
// Bind the numeric stream before any later publication can restore defaults.
// A released occupancy cannot retarget the new occupant's weapon stream.
pub(crate) fn bind_random_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, lease: Ref<SeatLease>, binding: u32) -> bool {
    let Some(l) = world_ctx.sim.store.seat_lease.get(lease) else { return false };
    if l.released || l.unit != Some(unit)
        || world_ctx.sim.store.unit.get(unit).is_none_or(|u| {
            u.seat != Some(lease) || u.generation != l.generation
        })
    { return false; }
    if let Some(hull) = l.vehicle {
        super::firing::bind_random_in(world_ctx, hull,l.index as usize,binding);
    } else if let Some(turret) = l.turret {
        let index = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter()
            .position(|t| t.turret == turret).unwrap();
        if let Some(w) = seats_state(&mut world_ctx.fleet.seats).weapons[index].as_mut() {
            w.random_binding = binding;
            w.state.random = binding != u32::MAX;
        }
        touch_turret_in(&mut world_ctx.fleet, turret);
    }
    super::pending_in(&mut world_ctx.fleet);
    true
}

/// The synchronous commands used by AI, network input and death/disconnect.
/// Scalar arguments are decoded before taking native ownership from the instance.
pub(crate) fn command_in(world_ctx: &mut crate::world::owner::World, a: F2SeatCommand) -> F2SeatResult {
    let _external = crate::units::f3::external();
    inputs_in(world_ctx, world_ctx.ai.battle().is_some_and(|battle|battle.ready()));
    seats_state(&mut world_ctx.fleet.seats).command_depth += 1;
    let result = crate::units::f3::with_services(|services| run_in(world_ctx, a, services));
    seats_state(&mut world_ctx.fleet.seats).command_depth -= 1;
    result
}
// Vehicles scalar commands publish before returning to their caller.
// Delay slab reclamation while an outer crew solver still holds its
// starting lease list; a nested death must not recycle one of its handles.
fn run_in(world_ctx: &mut crate::world::owner::World, a: F2SeatCommand, services: Option<crate::units::f3::WorldServices<'_>>) -> F2SeatResult {
    seats_state(&mut world_ctx.fleet.seats).depth += 1;
    let result = run_command_in(world_ctx, a, services);
    super::publish_in(world_ctx); seats_state(&mut world_ctx.fleet.seats).depth -= 1;
    // TurretSeats.update keeps its starting lease list across nested deaths.
    // Retire only when no solver can still hold one of those references.
    if seats_state(&mut world_ctx.fleet.seats).depth == 0 { finish_in(world_ctx); }
    result
}
// Vehicles.board, exit, move, displace and network commands share one
// validated entry. Check soldier life, hull life and lease generation
// before any mutation, so old input can never act on a reused occupancy.
fn run_command_in(world_ctx: &mut crate::world::owner::World, a: F2SeatCommand, services: Option<crate::units::f3::WorldServices<'_>>) -> F2SeatResult {
    refresh_command_in(world_ctx, &a);
    let mut result = F2SeatResult::default(); result.value = -1.0;
    let look = matches!(a.op, F2SeatOp::Look | F2SeatOp::Network | F2SeatOp::ResetSticks);
    if let Some(u) = a.unit {
        if !world_ctx.sim.store.unit.get(u).is_some_and(|u| u.generation == a.unit_life) { return result; }
        if !look { touch_unit_in(&mut world_ctx.fleet, u); }
    }
    if let Some(v) = a.hull {
        if !world_ctx.sim.store.vehicle.get(v).is_some_and(|v| v.target_generation == a.hull_life) { return result; }
        if look { super::mark_in(&mut world_ctx.fleet, v); } else { touch_hull_in(world_ctx, v); }
    }
    if let Some(r) = a.lease {
        let Some(l) = world_ctx.sim.store.seat_lease.get(r) else { return result };
        if !l.released && l.unit.is_some_and(|u| !world_ctx.sim.store.unit.get(u).is_some_and(|u| u.generation == l.generation && u.seat == Some(r))) { return result; }
        if !look { touch_lease_in(&mut world_ctx.fleet, r); }
    }
    if matches!(a.op, F2SeatOp::Man | F2SeatOp::TurretLeave | F2SeatOp::TurretDestroyed | F2SeatOp::TurretRequestExit) ||
        a.lease.and_then(|r| world_ctx.sim.store.seat_lease.get(r)).is_some_and(|l| l.turret.is_some()) {
        return turret_command_in(world_ctx, a, services);
    }
    let mut st = std::mem::take(&mut world_ctx.sim.store);
    let mut hulls = std::mem::take(&mut seats_state(&mut world_ctx.fleet.seats).owner.hulls);
    refresh_hulls(&mut hulls, &st);
    let mut world = World::with_services_in(world_ctx, services);
    let h = a.hull.and_then(|v| hulls.hulls.iter().position(|h| h.vehicle == v));
    let unit = a.unit.map(|u| u.slot);
    let how = Vacate { placement: a.placement, promote: a.promote, post: a.post, safe: a.safe, force: a.force,
        reason: match a.reason { SeatReason::Exit => Reason::Exit, SeatReason::Killed => Reason::Killed, SeatReason::Destroyed => Reason::Destroyed, SeatReason::Stale => Reason::Stale, SeatReason::Displaced => Reason::Displaced } };
    match a.op {
        F2SeatOp::Board => if let Some((u,h)) = unit.zip(h) { result.accepted = hulls.board(&mut st, &mut world, u, h, a.index as i32); result.lease = hulls.seat_of(&st, u); },
        F2SeatOp::RequestExit => if let Some(u) = unit { result.accepted = hulls.request_exit(&mut st, u, a.post, a.promote, a.placement); },
        F2SeatOp::Unboard => if let Some(u) = unit { result.accepted = hulls.unboard(&mut st, &mut world, u, how); },
        F2SeatOp::Vacate => if let Some(r) = a.lease { result.accepted = hulls.vacate(&mut st, &mut world, r, how); },
        F2SeatOp::Move => if let Some(r) = a.lease {
            if let Some(v) = st.seat_lease.get(r).and_then(|l| l.vehicle).and_then(|v| st.vehicle.get(v)) {
                if a.index >= 0.0 && a.index.fract()==0.0 && a.index < v.seats.len() as f64 {
                    result.lease = hulls.move_seat(&mut st, &mut world, r, a.index as usize); result.accepted = result.lease.is_some();
                }
            }
        },
        F2SeatOp::Displace => if let Some(r) = a.lease { result.lease = hulls.displace(&mut st, &mut world, r); result.accepted = true; },
        F2SeatOp::Promote => if let Some(h) = h { result.lease = hulls.promote(&mut st, &mut world, h, a.index as usize); result.accepted = result.lease.is_some(); },
        F2SeatOp::Cycle => if let Some(u) = unit { result.accepted = seats::network::cycle_network_seat(&mut hulls, &mut st, &mut world, u, a.direction as i32); },
        F2SeatOp::ResetSticks => if let Some(r) = a.hull { if let Some(v) = st.vehicle.get_mut(r) { v.stick_x = Some(0.0); v.stick_y = Some(0.0); result.accepted = true; } },
        F2SeatOp::Look | F2SeatOp::Network => if let Some(r) = a.hull {
            let header = F2SeatHeader::take(&seats_state(&mut world_ctx.fleet.seats).input, &[]);
            let scales = seats::boarding::LookScales { scale: header.mouse_scale, invert: header.invert, flight_scale: header.flight_scale, flight_invert: header.flight_invert };
            if let Some(v) = st.vehicle.get_mut(r) { if a.index >= 0.0 && a.index < v.seats.len() as f64 {
                for i in 0..header.look_count as usize {
                    let at = header.looks as usize + i * F2SeatSample::WORDS;
                    let sample = F2SeatSample::take(&seats_state(&mut world_ctx.fleet.seats).input[at..at + F2SeatSample::WORDS], &[]);
                    seats::boarding::seat_look(v, a.index as usize, sample.dx, sample.dy, &scales);
                }
                if a.op == F2SeatOp::Network && seats::boarding::seat_control(v, a.index as usize) != Some(seats::boarding::SeatControl::Flight) {
                    let s = &mut v.seats[a.index as usize]; s.aim_yaw = s.look_yaw.unwrap_or(s.aim_yaw); s.pitch = s.look_pitch.unwrap_or(s.pitch);
                    seats::hulls::clamp_aim(v, a.index as usize);
                }
                result.accepted = true;
            } }
        },
        F2SeatOp::Destroyed => if let Some(h) = h { hulls.destroyed_crew(&mut st, &mut world, h); result.accepted = true; },
        F2SeatOp::Release => if let Some(h) = h {
            let v = st.vehicle.get_mut(hulls.hulls[h].vehicle).unwrap();
            for seat in &mut v.seats { if let Some(r) = seat.lease.take() {
                if let Some(l) = st.seat_lease.get_mut(r) { l.released = true; if let Some(u) = l.unit { if let Some(u) = st.unit.get_mut(u) { if u.seat == Some(r) { u.seat = None; } } } }
            } }
            result.accepted = true;
        },
        _ => {},
    }
    if let Some(u) = a.unit { if let Some(l) = st.unit.get(u).and_then(|u| u.seat).and_then(|r| st.seat_lease.get(r)) { result.value = l.index; } }
    if !look { if let Some(v) = a.hull { seats::hulls::lease_views(&mut st, v); } }
    seats_state(&mut world_ctx.fleet.seats).owner.hulls = hulls; world_ctx.sim.store = st;
    if !look {
        if let Some(v) = a.hull { touch_hull_in(world_ctx, v); }
        if let Some(r) = a.lease { touch_lease_in(&mut world_ctx.fleet, r); }
        if let Some(u) = a.unit { touch_unit_in(&mut world_ctx.fleet, u); }
    }
    result
}

/// Hull::setHealth and the wreck countdown enter these directly, not via JS.
pub(crate) fn destroyed_in(world_ctx: &mut crate::world::owner::World, index: usize) {
    destroyed_with_in(world_ctx, index, None);
}
pub(crate) fn destroyed_with_in(world_ctx: &mut crate::world::owner::World, index: usize, services: Option<crate::units::f3::WorldServices<'_>>) {
    let r = world_ctx.sim.store.vehicles.vehicles[index];
    let life = world_ctx.sim.store.vehicle.get(r).unwrap().target_generation;
    internal_in(world_ctx, F2SeatCommand { op: F2SeatOp::Destroyed, hull: Some(r), hull_life: life, ..Default::default() }, services);
}
pub(crate) fn release_in(world_ctx: &mut crate::world::owner::World, index: usize) {
    release_with_in(world_ctx, index, None);
}
pub(crate) fn release_with_in(world_ctx: &mut crate::world::owner::World, index: usize, services: Option<crate::units::f3::WorldServices<'_>>) {
    let r = world_ctx.sim.store.vehicles.vehicles[index];
    let life = world_ctx.sim.store.vehicle.get(r).unwrap().target_generation;
    internal_in(world_ctx, F2SeatCommand { op: F2SeatOp::Release, hull: Some(r), hull_life: life, ..Default::default() }, services);
}

/// Native AI enters with its current Core and cross-owner service loans.
/// The outer phase already imported foreign inputs; replaying that boundary
/// here could replace an intent written by an earlier controller decision.
/// Dirty rows publish at the outer return, after recursive hooks complete.
pub(crate) fn ai_command_in(world_ctx: &mut crate::world::owner::World, a:F2SeatCommand,services:crate::units::f3::WorldServices<'_>)->F2SeatResult {
    seats_state(&mut world_ctx.fleet.seats).depth+=1;
    let result=run_command_in(world_ctx, a,Some(services));
    // Vehicles.board/man completes its rider samples before returning. Later
    // AI observers can read those shapes in the same owning phase, while
    // facade publication still waits for the outer boundary.
    for i in 0..seats_state(&mut world_ctx.fleet.seats).owner.changed.hulls.len() {
        { let hull = seats_state(&mut world_ctx.fleet.seats).owner.changed.hulls[i]; super::riders::hull_in(world_ctx, hull, false); }
    }
    for i in 0..seats_state(&mut world_ctx.fleet.seats).owner.changed.turrets.len() {
        let turret=seats_state(&mut world_ctx.fleet.seats).owner.changed.turrets[i];
        if let Some(index)=seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t|t.turret==turret) {sample_turret_rider_in(&world_ctx.sim.store, &world_ctx.pose.clips, &mut world_ctx.fleet.riders, world_ctx.fleet.seats.as_mut().unwrap(),index);}
    }
    seats_state(&mut world_ctx.fleet.seats).depth-=1;
    result
}

// Hull::setHealth continues natively after crew changes. Its next foreign
// hook or command return publishes them together with the current hull.
fn internal_in(world_ctx: &mut crate::world::owner::World, a: F2SeatCommand, services: Option<crate::units::f3::WorldServices<'_>>) {
    seats_state(&mut world_ctx.fleet.seats).depth += 1;
    run_command_in(world_ctx, a, services);
    seats_state(&mut world_ctx.fleet.seats).depth -= 1;
}

/// Vehicle.fireSeat consumed canonical edges; keep the lease mirror current.
pub(crate) fn changed_lease_in(world_ctx: &mut crate::world::owner::World, lease: Ref<SeatLease>) { touch_lease_in(&mut world_ctx.fleet, lease); }

/// SeatLease.exposed reads the canonical holder and its prepared rider node.
/// A hull's visible rider and a turret's shootable gunner use distinct rules.
/// Unit.origin and SeatUnit.origin observe the current native seat rig.
pub(crate) fn lease_origin_in(world_ctx: &mut crate::world::owner::World, reference:Ref<SeatLease>)->Option<Vector3> {
    let lease=world_ctx.sim.store.seat_lease.get(reference)?;let vehicle=lease.vehicle;let turret=lease.turret;let index=lease.index as usize;
    if let Some(vehicle)=vehicle {
        let life=world_ctx.sim.store.vehicle.get(vehicle)?.target_generation;
        return super::seat_origin_in(world_ctx, super::seats::HullLife {vehicle,life},index);
    }
    let turret=turret?;let side=seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter_mut().find(|t|t.turret==turret)?;Some(side.origin())
}
pub(crate) fn lease_forward_in(world_ctx: &mut crate::world::owner::World, reference:Ref<SeatLease>)->Option<Vector3> {
    let lease=world_ctx.sim.store.seat_lease.get(reference)?;let vehicle=lease.vehicle;let turret=lease.turret;let index=lease.index as usize;
    if let Some(vehicle)=vehicle {
        let life=world_ctx.sim.store.vehicle.get(vehicle)?.target_generation;
        return super::seat_forward_in(world_ctx, super::seats::HullLife {vehicle,life},index);
    }
    let turret=turret?;let side=seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().find(|t|t.turret==turret)?;Some(side.view_direction(side.yaw,side.pitch))
}

pub(crate) fn exposed_in(world_ctx: &mut crate::world::owner::World, lease: &SeatLease) -> bool {
    if lease.released { return false; }
    if let Some(r) = lease.vehicle {
        if !world_ctx.sim.store.vehicle.get(r).is_some_and(|v| v.alive) { return false; }
        return world_ctx.fleet.poses.get(r.slot as usize).and_then(Option::as_ref)
            .and_then(|p| p.pose.rig.seats.get(lease.index as usize))
            .and_then(|s| s.rider.as_ref()).is_some_and(|r| r.node.is_some() && r.shootable);
    }
    lease.turret.and_then(|r| seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().find(|t| t.turret == r))
        .is_some_and(|t| t.exposed_seat())
}
// A stationary lease's collision owner comes from its retained turret row.
pub(crate) fn turret_body_in(world_ctx: &mut crate::world::owner::World, r: Ref<Turret>) -> Option<Ref<Body>> { seats_state(&mut world_ctx.fleet.seats).turret_body(r) }

pub(crate) fn wire_turret_count_in(world_ctx: &mut crate::world::owner::World) -> usize {
    world_ctx.fleet.seats.as_ref().map_or(0, |s| s.owner.crews.turrets.len())
}
/// captureMountedControl keeps the stationary gun's aim rates and barrel
/// beside its canonical weapon. No turret publication or pose work runs here.
pub(crate) fn wire_own_turret_in(world_ctx: &mut crate::world::owner::World, store: &Store, reference: Ref<Turret>, own: &mut crate::wire::store_own::Capture,
    defs: &crate::combat::defs::WeaponDefs) -> Result<(), &'static str> {
    let st = world_ctx.fleet.seats.as_ref().ok_or("mounted control lease prerequisite missing")?;
    let t = st.owner.crews.turrets.iter().find(|t| t.turret == reference).ok_or("mounted control lease prerequisite missing")?;
    let (_, member, frame, _) = crate::units::f3_props::wire_turret(reference).ok_or("mounted control lease prerequisite missing")?;
    let aim = [t.yaw, t.pitch, t.yaw_vel, t.pitch_vel, t.gun_yaw, t.gun_pitch];
    let id = crate::units::registry::target_id(member) as f64;
    let life = frame.target_generation.unwrap_or(0.);
    if !t.lease.and_then(|r| store.seat_lease.get(r)).is_some_and(|l| l.turret == Some(reference) && !l.released) {
        return Err("mounted control lease prerequisite missing");
    }
    if let Some(weapon) = st.weapons.get(t.index).and_then(Option::as_ref) {
        own.mount(3, id, life, 0., &aim, 0, &[(&weapon.state, t.barrel)], defs)
    } else { own.mount(3, id, life, 0., &aim, 0, &[], defs) }
}
/// Borrow completed crew angles beside the health owner's scalar sample.
/// No posed barrel, collision or lazy rider-shape reader runs here.
pub(crate) fn wire_turret_in(world_ctx: &mut crate::world::owner::World, index: usize, store: &Store, states: [crate::store::StrId; 3])
    -> Option<crate::wire::store_capture::Turret> {
    let st = world_ctx.fleet.seats.as_ref()?;
    let t = st.owner.crews.turrets.get(index)?;
    let (building, member, frame, team) = crate::units::f3_props::wire_turret(t.turret)?;
    let state = match frame.state {
        crate::battle::props::State::Unbuilt => states[0],
        crate::battle::props::State::Built => states[1],
        crate::battle::props::State::Destroyed => states[2],
    };
    Some(crate::wire::store_capture::Turret {
        reference: t.turret, id: crate::units::registry::target_id(member) as f64,
        life: frame.target_generation.unwrap_or(0.), building: building as f64, state,
        team: t.lease.and_then(|r| store.seat_lease.get(r)).map_or(team, |lease| lease.team),
        health: frame.health, yaw: t.yaw, pitch: t.pitch, gun_yaw: t.gun_yaw, gun_pitch: t.gun_pitch,
    })
}
/// leaveKilled vacates the live occupancy before Character death bookkeeping.
/// The explicit service borrow lets an exposed gunner's hit stay native.
// PlayerCombat death releases the canonical occupancy immediately.
// An exposed rider uses the native sampled drop before its corpse falls.
// Crew promotion and any nested damage finish within this same service.
// Pure seat notifications retain their original observation samples.
pub(crate) fn leave_killed_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, services: crate::units::f3::WorldServices<'_>) {
    let row = world_ctx.sim.store.unit.get(unit).expect("killed seated unit");
    let Some(lease) = row.seat else { return };
    let life = row.generation;
    // Game.leaveKilled keeps a corpse hidden when its current hull has no
    // rider node. A former turret's visibility rule cannot override this hull.
    if world_ctx.sim.store.seat_lease.get(lease).is_some_and(|l| l.vehicle.is_some())
        && visible_in(&mut world_ctx.fleet, &world_ctx.sim.store, row)==Some(false) { crate::units::f3::hide_destroyed_crew(unit); }
    internal_in(world_ctx, F2SeatCommand { op:F2SeatOp::Vacate, unit:Some(unit), unit_life:life,
        lease:Some(lease), placement:false, reason:SeatReason::Killed, ..Default::default() }, Some(services));
}
// Building.destroyed keeps a turret's current gunner until crew destruction
// completes. Its health owner updates the same retained crew row directly.
/// Game's spawn/suicide path releases a seat without choosing a foot exit.
/// Its subsequent placement or death supplies the final body position.
pub(crate) fn leave_voluntary_in(world_ctx: &mut crate::world::owner::World, unit: Ref<Unit>, services: crate::units::f3::WorldServices<'_>) {
    let row=world_ctx.sim.store.unit.get(unit).expect("native seat occupant");
    let Some(lease)=row.seat else{return};let life=row.generation;
    internal_in(world_ctx, F2SeatCommand{op:F2SeatOp::Vacate,unit:Some(unit),unit_life:life,lease:Some(lease),
        placement:false,reason:SeatReason::Exit,..Default::default()},Some(services));
}
pub(crate) fn turret_gunner_in(world_ctx: &mut crate::world::owner::World, r: Ref<Turret>) -> Option<Ref<Unit>> {
    seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().find(|t| t.turret == r).and_then(|t| t.gunner).map(unit_ref)
}
/// TurretTarget.team observes the captured lease team, rather than the
/// current occupant's team after a later team command.
pub(crate) fn battle_team_in(world_ctx: &mut crate::world::owner::World, s: &Store, r: Ref<Turret>) -> Option<f64> {
    let st = world_ctx.fleet.seats.as_ref()?;
    let lease = st.owner.crews.turrets.iter().find(|t| t.turret == r)?.lease?;
    Some(s.seat_lease.get(lease).expect("current turret lease").team)
}
/// TurretTarget.team observes the current lease, including an exit or crew
/// death reached during another native owner's phase. Unmanned guns use
/// the building's authored team; a foreign-only target has no crew row here.
pub(crate) fn turret_team_in(world_ctx: &mut crate::world::owner::World, r: Ref<Turret>, fallback: f64) -> Option<f64> {
    if !super::active_in(&world_ctx.fleet) { return None; }
    let turret = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().find(|t| t.turret == r)?;
    Some(turret.lease.and_then(|lease| world_ctx.sim.store.seat_lease.get(lease))
        .map_or(fallback, |lease| lease.team))
}
pub(crate) fn turret_life_in(world_ctx: &mut crate::world::owner::World, r: Ref<Turret>, alive: bool, attacker: Attacker, life: f64, center: Vector3, radius: f64) {
    let Some(index) = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t| t.turret == r) else { return };
    seats_state(&mut world_ctx.fleet.seats).turret_lives[index] = life;
    let t = &mut seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[index];
    t.alive = alive; t.last_attacker = By::Record(attacker); t.sphere = (center, radius);
    touch_turret_in(&mut world_ctx.fleet, r);
}
pub(crate) fn turret_destroyed_in(world_ctx: &mut crate::world::owner::World, r: Ref<Turret>, services: crate::units::f3::WorldServices<'_>) {
    let Some(index) = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t| t.turret == r) else { return };
    let life = seats_state(&mut world_ctx.fleet.seats).turret_lives[index];
    internal_in(world_ctx, F2SeatCommand { op:F2SeatOp::TurretDestroyed, index:index as f64,
        hull_life:life, ..Default::default() }, Some(services));
}

/// Retirement removes class sidecars before a reused slab slot can be latched.
pub(crate) fn retire_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>) {
    if let Some(index) = world_ctx.sim.store.vehicles.vehicles.iter().position(|&v| v == r) { release_in(world_ctx, index); }
    let hulls = &mut seats_state(&mut world_ctx.fleet.seats).owner.hulls.hulls;
    hulls.retain(|h| h.vehicle != r);
    for (index, hull) in hulls.iter_mut().enumerate() { hull.index = index; }
}
// Vehicles.syncCrews ends the last outer seat read of this phase. Only
// now may released slab rows be reclaimed; readers from earlier hooks
// continue to carry the old generation and cannot target a replacement.
pub(crate) fn finish_in(world_ctx: &mut crate::world::owner::World) {
    if crate::world::deferred() {seats_state(&mut world_ctx.fleet.seats).owner.retire_released(&mut world_ctx.sim.store);}
    else {seats_state(&mut world_ctx.fleet.seats).owner.published(&mut world_ctx.sim.store);}
}

// TurretSeats.drive changes its turret and current lease together.
// Track the touched turret once, including lifecycle commands between
// phases, so publishing one gun does not reset another gunner's input.
fn touch_turret_in(fleet: &mut crate::vehicles::module::Fleet, r: Ref<Turret>) {
    super::pending_in(fleet);
    let list = &mut seats_state(&mut fleet.seats).owner.changed.turrets;
    if !list.contains(&r) { list.push(r); }
    if let Some(l) = seats_state(&mut fleet.seats).owner.crews.turrets.iter().find(|t| t.turret == r).and_then(|t| t.lease) { touch_lease_in(fleet, l); }
}

// SeatUnit's arsenal keeps the turret's weapon, including its current ammo
// and tuning. Resolve both generations before exposing a scalar observation.
fn weapon_index_in(world_ctx: &mut crate::world::owner::World, r:Ref<Turret>,life:f64)->Option<usize> { seats_state(&mut world_ctx.fleet.seats).weapon_index(r, life) }
pub(crate) fn weapon_count_in(world_ctx: &mut crate::world::owner::World, r:Ref<Turret>,life:f64)->Option<usize> { seats_state(&mut world_ctx.fleet.seats).weapon_count(r, life) }
pub(crate) fn with_weapon_in<R>(world_ctx: &mut crate::world::owner::World, r:Ref<Turret>,life:f64,index:usize,read:impl FnOnce(&Weapon)->R)->Option<R> { if index != 0 { return None; } seats_state(&mut world_ctx.fleet.seats).with_weapon(r, life, index, read) }
pub(crate) fn edit_weapon_in<R>(world_ctx: &mut crate::world::owner::World, r:Ref<Turret>,life:f64,index:usize,
    edit:impl FnOnce(&mut crate::ai::combat::world::WeaponSettings)->R)->Option<R> {
    if index!=0 {return None;}
    let i=weapon_index_in(world_ctx, r,life)?;let w=seats_state(&mut world_ctx.fleet.seats).weapons[i].as_mut()?;
    w.class.as_ref()?;
    let (result,changed)=crate::ai::owned::weapons::edit_state(&mut w.state,edit);
    if changed {touch_turret_in(&mut world_ctx.fleet, r);}
    Some(result)
}
/// The binding refers to the retained native AI stream in F3. Turret fire and
/// ordinary unit fire therefore consume the same authority in source order.
pub(crate) fn bind_ai_stream_in(world_ctx: &mut crate::world::owner::World, r:Ref<Turret>,life:f64,binding:u32)->bool {
    let Some(i)=weapon_index_in(world_ctx, r,life) else {return false};
    if let Some(w)=seats_state(&mut world_ctx.fleet.seats).weapons[i].as_mut() {
        assert!(w.class.is_some(),"AI bind outside a detached turret update");
        let changed=!w.state.random;w.state.random=true;w.random_binding=binding;
        if changed {touch_turret_in(&mut world_ctx.fleet, r);}
    }
    true
}

#[no_mangle]
pub extern "C" fn f2_turret_words(which: u32) -> u32 {
    match which { 0 => F2TurretBirth::WORDS as u32, _ => F2TurretLife::WORDS as u32 }
}

/// Turret construction keeps its geometry and weapon in the native instance.

pub(crate) fn f2_turret_adopt_in(world_ctx: &mut crate::world::owner::World) {
    let a = { let (row, heap) = world_ctx.sim.calls.args(); F2TurretBirth::take(row, heap) };
    let v = a.turret;
    let mut t = crate::battle::turrets::side(v.state, &a.settings);
    t.turret = v.turret; t.alive = v.alive; t.last_attacker = By::Record(v.attacker);
    t.sphere = (vec(&v.center), v.radius); t.weapon = v.weapon; t.missile_lock = v.missile_lock;
    t.desc.body = v.body; t.desc.rider = v.rider; t.desc.shootable = v.shootable;
    let out = F2TurretOut { header: F2TurretHeader { turret: t.turret }, state: F2TurretPose { yaw: t.yaw, pitch: t.pitch, yaw_vel: t.yaw_vel, pitch_vel: t.pitch_vel, gun_yaw: t.gun_yaw, gun_pitch: t.gun_pitch, barrel: t.barrel }, weapon: a.weapon.clone(), ..Default::default() };
    seats_state(&mut world_ctx.fleet.seats).owner.crews.collide = a.collide;
    seats_state(&mut world_ctx.fleet.seats).owner.add_turret(t);
    let w = a.weapon.map(|weapon| {
        let class = crate::combat::module::defs().weapons.class(weapon.def).clone();
        TurretWeapon { state: weapon, class: Some(class), ordnance: a.ordnance, channel: a.channel,
            shots: Vec::with_capacity(32), locks: a.locks, random_binding:u32::MAX }
    });
    seats_state(&mut world_ctx.fleet.seats).turret_lives.push(a.life); seats_state(&mut world_ctx.fleet.seats).posed.push(false); seats_state(&mut world_ctx.fleet.seats).timed.push(a.timed); seats_state(&mut world_ctx.fleet.seats).weapons.push(w); seats_state(&mut world_ctx.fleet.seats).turret_rows.push(out);
    let s = seats_state(&mut world_ctx.fleet.seats);
    s.turret_output.words.reserve(5 + s.turret_rows.len() * F2TurretOut::WORDS);
    s.event_output.words.reserve(32768);
    let s=seats_state(&mut world_ctx.fleet.seats);let index=s.owner.crews.turrets.len()-1;let turret=s.owner.crews.turrets[index].turret;
    let slot=turret.slot as usize;
    if s.turret_indices.len()<=slot {s.turret_indices.resize(slot+1,None);}
    s.turret_indices[slot]=Some((turret,index));
    let leases=&mut world_ctx.sim.store.turret_seats.leases;
    leases.reserve(s.owner.crews.turrets.len().saturating_sub(leases.len()));
    touch_turret_in(&mut world_ctx.fleet, turret);super::publish_in(world_ctx);
}

/// Building health and rebuilding remain foreign lifecycle facts.

pub(crate) fn f2_turret_life_in(world_ctx: &mut crate::world::owner::World) {
    let a = { let (row, heap) = world_ctx.sim.calls.args(); F2TurretLife::take(row, heap) };
    if let Some(index) = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t| t.turret == a.turret) {
        if a.generation < seats_state(&mut world_ctx.fleet.seats).turret_lives[index] { return; }
        seats_state(&mut world_ctx.fleet.seats).turret_lives[index] = a.generation;
        let t = &mut seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[index];
        t.alive = a.alive; t.last_attacker = By::Record(a.attacker); t.sphere = (vec(&a.center), a.radius);
    }
}

// The command and publication use one completed native rider sample. Do not
// advance Unit.tickCount or its lazy hit-shape cache at this observation.
fn sample_turret_rider_in(store: &Store, clips: &[crate::pose::Clip], riders: &mut super::riders::Riders, s:&mut State,index:usize) {
    let t=&mut s.owner.crews.turrets[index];
    let Some(unit)=t.lease.and_then(|r|store.seat_lease.get(r)).and_then(|l|l.unit) else{return};
    // origin updates the rig exactly where publish_turrets first observes it.
    t.origin();let point=t.seat_point();let mut turn=three::Quaternion::default();
    let node=t.rig.seat.unwrap_or(t.rig.root);t.rig.tree.get_world_quaternion(node,&mut turn);
    let pitch=if s.timed[index] {0.}else{-t.pitch/(45.*(std::f64::consts::PI/180.))};
    let yaw=if s.timed[index]||!seats::truthy(t.desc.turn_rate) {0.}else{t.yaw_vel/t.desc.turn_rate};
    super::riders::turret_sample(riders, store, clips, unit, t.turret, t.desc.rider, point, turn, pitch, yaw);
}

// TurretSeats.pose publishes compact frames and samples for existing
// readers. The retained rig itself stays native; no JavaScript world
// reader rebuilds the gun, seat, eye or muzzle from scene nodes.
fn publish_turrets_in(store: &Store, clips: &[crate::pose::Clip], riders: &mut super::riders::Riders, s: &mut State,sample:bool) {
    s.turret_output.begin(s.owner.changed.turrets.len(), F2TurretOut::WORDS);
    for at in 0..s.owner.changed.turrets.len() {
        let r=s.owner.changed.turrets[at];
        let i = s.owner.crews.turrets.iter().position(|t| t.turret == r).unwrap();
        let t = &mut s.owner.crews.turrets[i]; let out = &mut s.turret_rows[i];
        out.posed = s.posed[i]; s.posed[i] = false;
        out.lease = t.lease; out.gunner = t.gunner.map(unit_ref);
        let p = &mut out.state;
        p.yaw = t.yaw; p.pitch = t.pitch; p.yaw_vel = t.yaw_vel; p.pitch_vel = t.pitch_vel;
        p.gun_yaw = t.gun_yaw; p.gun_pitch = t.gun_pitch; p.barrel = t.barrel;
        out.origin = v3(&t.origin()); out.direction = v3(&t.view_direction(t.yaw,t.pitch));
        out.eye = v3(&t.aim_origin(true,t.yaw,t.pitch)); out.chase = v3(&t.aim_origin(false,t.yaw,t.pitch));
        out.seat_point = v3(&t.seat_point()); out.seat_yaw = t.seat_yaw();
        let mut along = Vector3::default(); out.muzzle = v3(&t.muzzle(false,Some(&mut along))); out.muzzle_direction = v3(&along);
        let mut turn = three::Quaternion::default();
        let node = t.rig.seat.unwrap_or(t.rig.root); t.rig.tree.get_world_quaternion(node, &mut turn);
        out.seat_turn = [turn.x, turn.y, turn.z, turn.w];
        out.sample_pitch = if s.timed[i] { 0.0 } else { -t.pitch / (45.0 * (std::f64::consts::PI / 180.0)) };
        out.sample_turn = if s.timed[i] || !seats::truthy(t.desc.turn_rate) { 0.0 } else { t.yaw_vel / t.desc.turn_rate };
        out.sample_time = s.timed[i].then_some(0.0);
        if let Some(w) = &s.weapons[i] {
            out.weapon.as_mut().unwrap().clone_from(&w.state); out.random_binding = w.random_binding;
        }
        let (row, heap) = s.turret_output.row(at); out.put(row, heap);
        if sample {sample_turret_rider_in(store, clips, riders, s,i);}
    }
    s.turret_output.finish();
    s.event_output.begin(s.event_rows.len(), F2TurretEvent::WORDS);
    for (i, event) in s.event_rows.iter().enumerate() { let (row, heap) = s.event_output.row(i); event.put(row, heap); }
    s.event_output.finish(); s.event_rows.clear();
}

/// TurretSeats.update stays immediately after hull riders in the vehicle phase.
pub(crate) fn turret_tick_in(world_ctx: &mut crate::world::owner::World, dt: f64, services: Option<crate::units::f3::WorldServices<'_>>) {
    refresh_crews_in(world_ctx);
    seats_state(&mut world_ctx.fleet.seats).depth += 1;
    let mut st = std::mem::take(&mut world_ctx.sim.store);
    let mut crews = std::mem::take(&mut seats_state(&mut world_ctx.fleet.seats).owner.crews);
    let mut world = World::with_services_in(world_ctx, services);
    world.turret = true;
    crews.update(&mut st, &mut world, dt);
    let world_ctx = world.world_ctx;
    seats_state(&mut world_ctx.fleet.seats).owner.crews = crews; world_ctx.sim.store = st; seats_state(&mut world_ctx.fleet.seats).depth -= 1;
}

impl CrewWorld for World<'_, '_> {
    // updateWeaponLock must see body placement from this crew and all prior
    // crews. Flush state-sensitive foreign readers before choosing a target;
    // the shared native picker preserves the original lazy query order.
    fn update_lock(&mut self, turret: usize, dt: f64, origin: &Vector3, direction: &Vector3, lease: Ref<SeatLease>) { let world_ctx = &mut *self.world_ctx;
            if self.services.is_none() {
                super::publish_in(world_ctx); Self::action(self.services.is_some(), F2SeatAction { kind: F2SeatActionKind::Flush, ..Default::default() });
            }
            let hint=self.services.as_ref().and_then(|services|services.ai.as_ref()).and_then(|ai| {
                let lease=world_ctx.sim.store.seat_lease.get(lease)?;
                Some(if lease.human {F2TurretHint::default()}else{ai.weapon_hint(&world_ctx.sim.store,lease.unit?,lease.controllable)})
            });
            let picked = turret_lock_in(world_ctx, turret, origin, direction, lease,hint);
            let w = seats_state(&mut world_ctx.fleet.seats).weapons[turret].as_mut().unwrap();
            w.state.update_lock(w.class.as_ref().unwrap(), dt, picked);
    }
    /// TurretSeats.drive's Weapon.update, with the same draw stream as JS.
    fn weapon_update(&mut self, turret: usize, dt: f64, held: bool, pressed: bool, reload: bool, out: &mut Vec<Shot>) { let world_ctx = &mut *self.world_ctx;
            let (mut weapon, class, mut shots) = {
                let w = seats_state(&mut world_ctx.fleet.seats).weapons[turret].as_mut().unwrap();
                (std::mem::take(&mut w.state), w.class.take().unwrap(), std::mem::take(&mut w.shots))
            };
            shots.clear(); seats_state(&mut world_ctx.fleet.seats).rounds.clear();
            super::firing::update_mounted(&mut weapon, 0, &class, dt, held, pressed, reload, &mut Draws { world_ctx, turret }, &mut shots);
            for shot in &shots {
                if let ShotKind::Round { yaw, pitch, .. } = shot.kind { out.push(Shot { yaw, pitch }); }
                else { unreachable!("turret weapons fire rounds"); }
            }
            let w = seats_state(&mut world_ctx.fleet.seats).weapons[turret].as_mut().unwrap(); w.state = weapon; w.class = Some(class); w.shots = shots;
            { let turret = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[turret].turret; touch_turret_in(&mut world_ctx.fleet, turret); }
    }
    /// TurretSeats.drive calls Ordnance.fire directly, before the next crew.
    fn fire(&mut self, turret: usize, origin: &Vector3, direction: &Vector3, owner: Option<u32>, lease: Ref<SeatLease>, shot: usize) -> usize { let world_ctx = &mut *self.world_ctx;
            if self.services.is_none() {
                super::publish_in(world_ctx);
                Self::action(self.services.is_some(), F2SeatAction { kind: F2SeatActionKind::Flush, ..Default::default() });
            }
            let (def, channel, target_id, target_generation) = {
                let w = seats_state(&mut world_ctx.fleet.seats).weapons[turret].as_ref().unwrap();
                let ShotKind::Round { target_id, target_generation, .. } = w.shots[shot].kind else { unreachable!("turret round") };
                (w.ordnance, w.channel, target_id, target_generation)
            };
            let body = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[turret].desc.body;
            let round = if let Some(def) = def {
                super::firing::fire_round_in(world_ctx, def, v3(origin), v3(direction), owner.map_or(OrdnanceOwner::Null, |u| OrdnanceOwner::Unit(unit_ref(u))),
                    body, BeamSource::Emitter(Ref::new(lease.slot, lease.gen | EMITTER_MASK)), channel, target_id, target_generation, None, self.services.as_mut().map(crate::units::f3::WorldServices::reborrow))
            } else { None };
            seats_state(&mut world_ctx.fleet.seats).ordnance_dirty = true;
            let at = seats_state(&mut world_ctx.fleet.seats).rounds.len(); seats_state(&mut world_ctx.fleet.seats).rounds.push(round); at
    }
    // bindMountedRound changes only a round's presentation attachment. Keep
    // its event beside this crew's sound and fired events; it must identify
    // the actual barrel chosen before the turret advances to the next crew.
    fn bind_round(&mut self, round: usize, turret: usize, barrel: f64) { let world_ctx = &mut *self.world_ctx;
            let mut e = F2TurretEvent { kind: F2TurretEventKind::Bind, turret: seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[turret].turret, barrel, ..Default::default() };
            match seats_state(&mut world_ctx.fleet.seats).rounds[round] { Some(crate::ordnance::world::Shot::Bolt(r)) => e.bolt = Some(r),
                Some(crate::ordnance::world::Shot::Round(r)) => e.round = Some(r), _ => {} }
            self.turret_event(e);
    }
    fn sound(&mut self, turret: usize, shot: usize) { self.shot_event(F2TurretEventKind::Sound, turret, shot, None, &Vector3::default(), &Vector3::default()); }
    fn fired(&mut self, turret: usize, shot: usize, owner: Option<u32>, origin: &Vector3, direction: &Vector3) {
            self.shot_event(F2TurretEventKind::Fired, turret, shot, owner, origin, direction);
    }
}
impl World<'_, '_> {
    // TurretSeats.drive shares the Unit notice stream in an owned world.
    // Keeping bind, sound and fired beside their round preserves ordering
    // across hull shots, immediate deaths and the following turret crew.
    fn turret_event(&mut self, event: F2TurretEvent) { let world_ctx = &mut *self.world_ctx;
            if self.services.is_some() { event.emit(&mut world_ctx.sim.events); }
            else { seats_state(&mut world_ctx.fleet.seats).event_rows.push(event); }
    }
    // TurretSeats.drive emits sound and fired after the native round call.
    // Copy the shot scalars while that shot buffer is still current, since
    // the next crew reuses scratch before JavaScript drains ordered events.
    fn shot_event(&mut self, kind: F2TurretEventKind, turret: usize, shot: usize, owner: Option<u32>, origin: &Vector3, direction: &Vector3) { let world_ctx = &mut *self.world_ctx;
            let w = seats_state(&mut world_ctx.fleet.seats).weapons[turret].as_ref().unwrap();
            let ShotKind::Round { yaw, pitch, charge, target_id, target_generation, .. } = w.shots[shot].kind else { unreachable!("turret round") };
            let reference = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[turret].turret;
            let sample = if self.services.is_some() {
                let Some(member) = crate::units::f3_props::member(RepairTarget::Turret(reference)) else { crate::units::f3::fail(); return; };
                crate::units::f3_fleet::mounted_sample(owner.map_or(OrdnanceOwner::Null, |u| OrdnanceOwner::Unit(unit_ref(u))),member)
            } else { F3MountedSample::default() };
            if kind == F2TurretEventKind::Fired && self.services.is_some() { crate::units::f3_fleet::wire_shot(&sample); }
            self.turret_event(F2TurretEvent {
                kind, turret: reference, owner: owner.map(unit_ref), origin: v3(origin), direction: v3(direction),
                yaw, pitch, charge, target_id, target_generation, sample, ..Default::default()
            });
    }
}

// TurretSeats.man, leave and destroyed use the same generation checks
// as hull commands. A rebuilding building supplies a new lifecycle value;
// it cannot resurrect the lease of the turret's previous occupant.
fn turret_command_in(world_ctx: &mut crate::world::owner::World, a: F2SeatCommand, services: Option<crate::units::f3::WorldServices<'_>>) -> F2SeatResult {
    let mut result = F2SeatResult { value: -1.0, ..Default::default() };
    let index = a.lease.and_then(|r| world_ctx.sim.store.seat_lease.get(r)).and_then(|l| l.turret)
        .and_then(|r| seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().position(|t| t.turret == r)).unwrap_or(a.index as usize);
    if index >= seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.len() { return result; }
    if a.lease.is_none() && seats_state(&mut world_ctx.fleet.seats).turret_lives[index] != a.hull_life { return result; }
    let r = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[index].turret; touch_turret_in(&mut world_ctx.fleet, r);
    let mut st = std::mem::take(&mut world_ctx.sim.store);
    let mut crews = std::mem::take(&mut seats_state(&mut world_ctx.fleet.seats).owner.crews);
    let mut world = World::with_services_in(world_ctx, services); world.turret = true;
    match a.op {
        F2SeatOp::Man => if let Some(unit) = a.unit { result.accepted = crews.man(&mut st, &mut world, unit.slot, index); },
        F2SeatOp::TurretLeave => if let Some(unit) = a.unit { result.accepted = crews.leave(&mut st, &mut world, unit.slot, a.placement, a.post); },
        F2SeatOp::TurretRequestExit => if let Some(unit) = a.unit { result.accepted = crews.request_exit(&mut st, unit.slot, a.post, a.placement); },
        F2SeatOp::TurretDestroyed => { crews.destroyed(&mut st, &mut world, index); result.accepted = true; },
        F2SeatOp::Vacate => if let Some(lease) = a.lease {
            crews.vacate(&mut st, &mut world, lease, Let { placement: a.placement, post: a.post, reason: match a.reason {
                SeatReason::Exit => Reason::Exit, SeatReason::Killed => Reason::Killed, SeatReason::Destroyed => Reason::Destroyed,
                SeatReason::Stale => Reason::Stale, SeatReason::Displaced => Reason::Displaced }, ..Default::default() }); result.accepted = true;
        },
        _ => {},
    }
    if let Some(unit) = a.unit { result.lease = st.unit.get(unit).and_then(|u| u.seat); }
    seats_state(&mut world_ctx.fleet.seats).owner.crews = crews; world_ctx.sim.store = st;
    touch_turret_in(&mut world_ctx.fleet, r); if let Some(unit) = a.unit { touch_unit_in(&mut world_ctx.fleet, unit); } if let Some(lease) = a.lease { touch_lease_in(&mut world_ctx.fleet, lease); }
    result
}

/// LiveObjects.update eases an unmanned gun at its original scheduler point.

// LiveObjects.update also eases an unmanned gun toward rest. Preserve
// that earlier lifecycle slot, before crews steer and fire. Mark only
// guns whose numeric aim changed so idle turrets need no publication.
pub(crate) fn f2_turrets_idle_in(world_ctx: &mut crate::world::owner::World, dt: f64) {
    idle_turrets_in(world_ctx, dt);
    super::publish_in(world_ctx);
}

// The live owner joins the surrounding publication barrier. Empty maps do
// not construct a fleet merely to discover that it has no idle turrets.
pub(crate) fn idle_turrets_in(world_ctx: &mut crate::world::owner::World, dt: f64) {
    if world_ctx.fleet.seats.is_none() { return; }
    for i in 0..seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.len() {
        let t = &mut seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[i];
        if !t.alive || t.gunner.is_some() { continue; }
        let before = [t.gun_yaw.to_bits(), t.gun_pitch.to_bits()];
        t.aim_gun(dt, None);
        if before != [t.gun_yaw.to_bits(), t.gun_pitch.to_bits()] { let turret = t.turret; touch_turret_in(&mut world_ctx.fleet, turret); }
    }
}

// Native lease emitters use a disjoint generation namespace. A beam keeps its
// original lease generation even when another gunner occupies the same turret.
const EMITTER_MASK: u32 = 1 << 31;
pub fn emitter_lease(source: BeamSource) -> Option<Ref<SeatLease>> {
    if let BeamSource::Emitter(r) = source {
        if r.gen & EMITTER_MASK != 0 { return Some(Ref::new(r.slot, r.gen & !EMITTER_MASK)); }
    }
    None
}
// SeatLease.emitter.alive follows the original occupant and holder life.
// A later gunner at the same turret does not keep an earlier beam alive;
// released and stale generations both end that source.
pub(crate) fn beam_gone_in(world_ctx: &mut crate::world::owner::World, r: Ref<SeatLease>) -> bool {
    let Some(l) = world_ctx.sim.store.seat_lease.get(r) else { return true };
    if !seats::lease::live(l, r, &world_ctx.sim.store.unit) { return true; }
    !seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets.iter().any(|t| Some(t.turret) == l.turret && t.alive)
}
// SeatLease.emitter resolves through the captured lease, never the turret
// currently selected by a player. A missing or reused lease must not
// silently bind an existing beam to a different gun.
fn beam_turret<'a>(st: &'a mut State, store: &Store, r: Ref<SeatLease>) -> Option<&'a mut seats::turrets::TurretSide> {
    let turret = store.seat_lease.get(r)?.turret?;
    st.owner.crews.turrets.iter_mut().find(|t| t.turret == turret)
}
/// SeatLease.emitter.matrix, using the retained gun frame without a JS rig call.
pub(crate) fn beam_local_in(world_ctx: &mut crate::world::owner::World, r: Ref<SeatLease>, origin: [f64; 3]) -> Option<[f64; 3]> {
    let t = beam_turret(seats_state(&mut world_ctx.fleet.seats), &world_ctx.sim.store, r)?; let node = t.rig.gun.unwrap_or(t.rig.root);
    t.rig.tree.update_matrix_world(t.rig.root, true);
    let mut frame = t.rig.tree.node(node).matrix_world; frame.invert();
    let mut p = vec(&origin); p.apply_matrix4(&frame); Some(v3(&p))
}
/// SeatLease.emitter.viewDirection and its gun-local beam origin.
pub(crate) fn beam_pose_in(world_ctx: &mut crate::world::owner::World, r: Ref<SeatLease>, local: [f64; 3], position: &mut [f64; 3], direction: &mut [f64; 3]) {
    let Some(t) = beam_turret(seats_state(&mut world_ctx.fleet.seats), &world_ctx.sim.store, r) else { return };
    let node = t.rig.gun.unwrap_or(t.rig.root);
    t.rig.tree.update_matrix_world(t.rig.root, true);
    let mut p = vec(&local); p.apply_matrix4(&t.rig.tree.node(node).matrix_world); *position = v3(&p);
    let mut dir = Vector3::default(); t.muzzle(false, Some(&mut dir)); *direction = v3(&dir);
}

/// weapon-targeting.js lockPick, preserving lazy shape and sight queries.
fn turret_lock_in(world_ctx: &mut crate::world::owner::World, turret: usize, origin: &Vector3, direction: &Vector3, lease: Ref<SeatLease>,native_hint:Option<F2TurretHint>) -> Option<LockTarget> {
    let w = seats_state(&mut world_ctx.fleet.seats).weapons[turret].as_ref().unwrap();
    let l = &w.locks[w.state.level as usize];
    let (range, on, off, relations, categories) = (l.range, l.on, l.off, l.relation_mask, l.category_mask);
    let (complete, id, generation) = (w.state.lock_complete, w.state.lock_target_id, w.state.lock_generation);
    if crate::units::f3::active() {
        // seatLockTarget is a foreign AI hint, not a geometry callback. A
        // pooled target must still be in that gameplay life when reached.
        let hint = native_hint.unwrap_or_else(|| {
            crate::units::f3::foreign_barrier();import_f2_turret_hint(&F2TurretHintQuery { lease })
        });
        if hint.bot {
            let target = hint.target?;
            if crate::units::f3_fleet::life(target) != hint.life { return None; }
            let turret = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[turret].turret;
            let viewer = crate::units::f3_props::member(RepairTarget::Turret(turret));
            let f = crate::units::f3_fleet::target(target,viewer,false,false);
            return (f.alive && allowed(relations,categories,f.relation,f.category))
                .then_some(LockTarget { id:f.id, generation:f.generation, alive:f.alive });
        }
        let mut direction = *direction; direction.normalize();
        let ignore = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[turret].desc.body;
        return pick_view(&mut TurretQuery { world_ctx, turret, lease, ignore }, LockView { range,on,off,relations,categories,complete,id,generation }, origin,&direction);
    }
    let query = |index, shapes, aim| import_f2_turret_target(&F2TurretTargetQuery { turret: turret as u32, index, lease, shapes, aim });
    let first = query(-1.0, false, false);
    match first.mode {
        F2TurretTargetMode::None | F2TurretTargetMode::Unavailable => return None,
        F2TurretTargetMode::Unit => return (first.alive && allowed(relations,categories,first.relation,first.category)).then_some(LockTarget { id: first.id, generation: first.generation, alive: first.alive }),
        F2TurretTargetMode::Human => {},
    }
    let mut direction = *direction; direction.normalize();
    let ignore = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[turret].desc.body;
    pick_view(&mut TurretQuery { world_ctx, turret, lease, ignore }, LockView { range,on,off,relations,categories,complete,id,generation }, origin,&direction)
}
struct TurretQuery<'w> { world_ctx: &'w mut crate::world::owner::World, turret: usize, lease: Ref<SeatLease>, ignore: Option<Ref<Body>> }
impl LockQuery for TurretQuery<'_> {
    fn count(&mut self) -> usize { let world_ctx = &mut *self.world_ctx; super::target_count_in(world_ctx) }
    fn identity(&mut self, index: usize) -> (f64, f64) { let world_ctx = &mut *self.world_ctx; super::target_identity_in(world_ctx, index) }
    fn hull_bounds(&mut self, hull: Ref<Vehicle>) -> Option<(Matrix4, Vector3, Vector3)> { let world_ctx = &mut *self.world_ctx;
            let v = world_ctx.sim.store.vehicle.get(hull)?;
            Some((Matrix4 { elements: v.matrix }, vec(&v.bounds_min), vec(&v.bounds_max)))
    }

    // lockPick requests expensive unit aim or shape facts only after its
    // cheaper eligibility checks. Keep those requests separate so native
    // targeting preserves Unit.updateSpheres' lazy invalidation timing.
    fn facts(&mut self, index: usize, shapes: bool, aim: bool) -> LockFacts { let world_ctx = &mut *self.world_ctx;
            let f = if crate::units::f3::active() {
                let turret = seats_state(&mut world_ctx.fleet.seats).owner.crews.turrets[self.turret].turret;
                let viewer = crate::units::f3_props::member(RepairTarget::Turret(turret));
                crate::units::f3_fleet::target(crate::units::f3_fleet::key(index),viewer,shapes,aim)
            } else { import_f2_turret_target(&F2TurretTargetQuery { turret: self.turret as u32, index: index as f64, lease: self.lease, shapes, aim }) };
            LockFacts { id: f.id, generation: f.generation, alive: f.alive, eligible: f.eligible, relation: f.relation, category: f.category,
                hull: f.hull, body: f.body.map(|r| r.slot), center: vec(&f.center), radius: f.radius, aim: vec(&f.aim) }
    }
    // lockPick accepts sight to the candidate's own collision body, while
    // terrain or any different body blocks it. Preserve the short-ray cutoff
    // and normalize only at the original solid-world query slot.
    fn clear(&mut self, origin: &Vector3, sight: &mut Vector3, length: f64, body: Option<u32>) -> bool { let world_ctx = &mut *self.world_ctx;
            if length >= 1e-6 {
                let hit = World::with_services_in(world_ctx, None).cast(origin,sight.divide_scalar(length),length,self.ignore);
                if hit.is_some_and(|h| h.body.is_none() || h.body.map(|b| b.slot) != body) { return false; }
            }
            true
    }
}

/// Collision body identities arrive with topology changes, never per crew.
#[no_mangle]
pub extern "C" fn f2_body_words() -> u32 { F2Bodies::WORDS as u32 }

pub(crate) fn f2_bodies_in(world_ctx: &mut crate::world::owner::World) {
    let a = { let (row, heap) = world_ctx.sim.calls.args(); F2Bodies::take(row, heap) };
    for row in a.bodies {
        if row.kernel as usize >= world_ctx.collision.state.bodies.len() {continue;}
        if let Some(collision) = world_ctx.fleet.collision.as_mut() { collision.bind_body(row.body, row.kernel); }
    }
    if super::active_in(&world_ctx.fleet) { super::super::prepare_collision_in(world_ctx); }
}

/// riderBody chooses the animal frame by HealthType, not the movement base.
pub(crate) fn animal_in(fleet: &mut crate::vehicles::module::Fleet, hull: Ref<Vehicle>) -> bool { seats_state(&mut fleet.seats).animal(hull) }
// Vehicles.syncRider skips body writeback when no actor exists. Keep
// that foreign construction fact separate from whether a clip is visible;
// a hidden cockpit can still have a soldier body to carry.
pub(crate) fn has_actor_in(fleet: &mut crate::vehicles::module::Fleet, unit: Ref<Unit>) -> bool { seats_state(&mut fleet.seats).has_actor(unit) }
// Vehicles.syncRider increments tickCount at body writeback, then marks
// its presentation hook. Publication clears this transient marker; the
// underlying rider sample remains retained for lazy shapes and deaths.
pub(crate) fn rider_posed_in(fleet: &mut crate::vehicles::module::Fleet, unit: Ref<Unit>) {
    let flags = &mut seats_state(&mut fleet.seats).rider_posed;
    if flags.len() <= unit.slot as usize { flags.resize(unit.slot as usize + 1, false); }
    flags[unit.slot as usize] = true; touch_unit_in(fleet, unit);
}

/// Statistics use Building.position and the turret's own weapon. The gun's
/// animated muzzle/seat frame and a released rider do not supply these facts.
pub(crate) fn battle_mount_in(world_ctx: &mut crate::world::owner::World, turret: Ref<Turret>) -> Option<([f64; 3], Option<(crate::store::DefId, f64)>)> {
    let st = seats_state(&mut world_ctx.fleet.seats); let index = st.owner.crews.turrets.iter().position(|t| t.turret == turret)?;
    let position = st.owner.crews.turrets[index].desc.position;
    let weapon = st.weapons[index].as_ref().map(|w| (w.state.def, w.state.level));
    Some(([position.x, position.y, position.z], weapon))
}

impl State {
    fn reserve_riders(&mut self, count: usize) {
        let s = self;
        s.owner.hulls.riders.reserve(count.saturating_sub(s.owner.hulls.riders.len()));
        s.owner.crews.riders.reserve(count.saturating_sub(s.owner.crews.riders.len()));
    }
    fn weapon_index(&mut self, r:Ref<Turret>,life:f64)->Option<usize> {
        let st=self;
        let (current,i)=st.turret_indices.get(r.slot as usize).copied().flatten()?;
        if current!=r {return None;}
        (st.turret_lives[i]==life).then_some(i)
    }
    fn weapon_count(&mut self, r:Ref<Turret>,life:f64)->Option<usize> {
        let i=self.weapon_index(r,life)?;
        Some(usize::from(self.weapons[i].is_some()))
    }
    fn with_weapon<R>(&mut self, r:Ref<Turret>,life:f64,index:usize,read:impl FnOnce(&Weapon)->R)->Option<R> {
        if index!=0 {return None;}
        let i=self.weapon_index(r,life)?;let w=self.weapons[i].as_ref()?;
        w.class.as_ref()?;
        Some(read(&w.state))
    }
    fn animal(&mut self, hull: Ref<Vehicle>) -> bool {
        self.owner.hulls.hulls.iter().find(|h| h.vehicle == hull).is_some_and(|h| h.animal)
    }
    fn has_actor(&mut self, unit: Ref<Unit>) -> bool { self.actors.get(unit.slot as usize).copied().unwrap_or(false) }
    fn turret_body(&mut self, r: Ref<Turret>) -> Option<Ref<Body>> {
        self.owner.crews.turrets.iter().find(|t| t.turret == r).and_then(|t| t.desc.body)
    }
}

/// Turret charge levels retain their own content targeting masks. Loading
/// policy changes no live crew, timer, weapon state or shared random binding.
pub(crate) fn refresh_weapon_masks_in(world_ctx: &mut crate::world::owner::World, table:&crate::ai::combat::weapons_ai::WeaponAiTable) {
    let Some(seats)=world_ctx.fleet.seats.as_mut() else {return};
    for weapon in seats.weapons.iter_mut().flatten() {
        let Some(class)=weapon.class.as_ref() else {continue};
        for (lock,level) in weapon.locks.iter_mut().zip(&class.levels) {
            let ai=table.weapon_ai(&level.obj);
            lock.relation_mask=ai.relation_mask;lock.category_mask=ai.category_mask;
        }
    }
}

// facade shims: callers outside the fleet bindings still enter through WASM.
#[cfg(target_arch = "wasm32")]
pub(crate) fn with_owned_seats<R>(run:impl FnOnce(&mut crate::vehicles::f2_seats::Seats)->R)->R { with_owned_seats_in(crate::abi::world(), run) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn loan_seats<R>(owner:&mut crate::vehicles::f2_seats::Seats,run:impl FnOnce()->R)->R { loan_seats_in(crate::abi::world(), owner, run) }
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn f2_seats_init() -> *const f64 { f2_seats_init_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn visible(unit: &Unit) -> Option<bool> { let w = crate::abi::world(); visible_in(&mut w.fleet, &w.sim.store,unit) }
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn f2_seats_adopt(prepared: u32) { f2_seats_adopt_in(crate::abi::world(), prepared) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn reserve_unit_slots(count: usize) { reserve_unit_slots_in(crate::abi::world(), count) }
#[cfg(target_arch = "wasm32")]
pub fn reserve_riders(count: usize) { reserve_riders_in(crate::abi::world(), count) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn adopt_unit(unit: Ref<Unit>, actor: bool, no_enter: f64) { adopt_unit_in(crate::abi::world(), unit, actor, no_enter) }
#[cfg(target_arch = "wasm32")]
pub fn import_phase_input(native_ai:bool) { import_phase_input_in(crate::abi::world(), native_ai) }
#[cfg(target_arch = "wasm32")]
pub fn publish() -> *const f64 { publish_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn bind_random(unit: Ref<Unit>, lease: Ref<SeatLease>, binding: u32) -> bool { bind_random_in(crate::abi::world(), unit, lease, binding) }
#[cfg(target_arch = "wasm32")]
pub fn command(a: F2SeatCommand) -> F2SeatResult { command_in(crate::abi::world(), a) }
#[cfg(target_arch = "wasm32")]
pub fn destroyed(index: usize) { destroyed_in(crate::abi::world(), index) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn destroyed_with(index: usize, services: Option<crate::units::f3::WorldServices<'_>>) { destroyed_with_in(crate::abi::world(), index, services) }
#[cfg(target_arch = "wasm32")]
pub fn release(index: usize) { release_in(crate::abi::world(), index) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn release_with(index: usize, services: Option<crate::units::f3::WorldServices<'_>>) { release_with_in(crate::abi::world(), index, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn ai_command(a:F2SeatCommand,services:crate::units::f3::WorldServices<'_>)->F2SeatResult { ai_command_in(crate::abi::world(), a, services) }
#[cfg(target_arch = "wasm32")]
pub fn changed_lease(lease: Ref<SeatLease>) { changed_lease_in(crate::abi::world(), lease) }
#[cfg(target_arch = "wasm32")]
pub fn lease_origin(reference:Ref<SeatLease>)->Option<Vector3> { lease_origin_in(crate::abi::world(), reference) }
#[cfg(target_arch = "wasm32")]
pub fn lease_forward(reference:Ref<SeatLease>)->Option<Vector3> { lease_forward_in(crate::abi::world(), reference) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn exposed(lease: &SeatLease) -> bool { exposed_in(crate::abi::world(), lease) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn turret_body(r: Ref<Turret>) -> Option<Ref<Body>> { turret_body_in(crate::abi::world(), r) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_turret_count() -> usize { wire_turret_count_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_own_turret(store: &Store, reference: Ref<Turret>, own: &mut crate::wire::store_own::Capture,
    defs: &crate::combat::defs::WeaponDefs) -> Result<(), &'static str> { wire_own_turret_in(crate::abi::world(), store, reference, own, defs) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_turret(index: usize, store: &Store, states: [crate::store::StrId; 3])
    -> Option<crate::wire::store_capture::Turret> { wire_turret_in(crate::abi::world(), index, store, states) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn leave_killed(unit: Ref<Unit>, services: crate::units::f3::WorldServices<'_>) { leave_killed_in(crate::abi::world(), unit, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn leave_voluntary(unit: Ref<Unit>, services: crate::units::f3::WorldServices<'_>) { leave_voluntary_in(crate::abi::world(), unit, services) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn turret_gunner(r: Ref<Turret>) -> Option<Ref<Unit>> { turret_gunner_in(crate::abi::world(), r) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn battle_team(s: &Store, r: Ref<Turret>) -> Option<f64> { battle_team_in(crate::abi::world(), s, r) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn turret_team(r: Ref<Turret>, fallback: f64) -> Option<f64> { turret_team_in(crate::abi::world(), r, fallback) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn turret_life(r: Ref<Turret>, alive: bool, attacker: Attacker, life: f64, center: Vector3, radius: f64) { turret_life_in(crate::abi::world(), r, alive, attacker, life, center, radius) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn turret_destroyed(r: Ref<Turret>, services: crate::units::f3::WorldServices<'_>) { turret_destroyed_in(crate::abi::world(), r, services) }
#[cfg(target_arch = "wasm32")]
pub fn retire(r: Ref<Vehicle>) { retire_in(crate::abi::world(), r) }
#[cfg(target_arch = "wasm32")]
pub fn finish() { finish_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub fn weapon_count(r:Ref<Turret>,life:f64)->Option<usize> { weapon_count_in(crate::abi::world(), r, life) }
#[cfg(target_arch = "wasm32")]
pub fn with_weapon<R>(r:Ref<Turret>,life:f64,index:usize,read:impl FnOnce(&Weapon)->R)->Option<R> { with_weapon_in(crate::abi::world(), r, life, index, read) }
#[cfg(target_arch = "wasm32")]
pub fn edit_weapon<R>(r:Ref<Turret>,life:f64,index:usize,
    edit:impl FnOnce(&mut crate::ai::combat::world::WeaponSettings)->R)->Option<R> { edit_weapon_in(crate::abi::world(), r, life, index, edit) }
#[cfg(target_arch = "wasm32")]
pub fn bind_ai_stream(r:Ref<Turret>,life:f64,binding:u32)->bool { bind_ai_stream_in(crate::abi::world(), r, life, binding) }
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn f2_turret_adopt() { f2_turret_adopt_in(crate::abi::world()) }
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn f2_turret_life() { f2_turret_life_in(crate::abi::world()) }
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn f2_turrets_idle(dt: f64) { f2_turrets_idle_in(crate::abi::world(), dt) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn idle_turrets(dt: f64) { idle_turrets_in(crate::abi::world(), dt) }
#[cfg(target_arch = "wasm32")]
pub fn beam_gone(r: Ref<SeatLease>) -> bool { beam_gone_in(crate::abi::world(), r) }
#[cfg(target_arch = "wasm32")]
pub fn beam_local(r: Ref<SeatLease>, origin: [f64; 3]) -> Option<[f64; 3]> { beam_local_in(crate::abi::world(), r, origin) }
#[cfg(target_arch = "wasm32")]
pub fn beam_pose(r: Ref<SeatLease>, local: [f64; 3], position: &mut [f64; 3], direction: &mut [f64; 3]) { beam_pose_in(crate::abi::world(), r, local, position, direction) }
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn f2_bodies() { f2_bodies_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub fn animal(hull: Ref<Vehicle>) -> bool { animal_in(&mut crate::abi::world().fleet,hull) }
#[cfg(target_arch = "wasm32")]
pub fn has_actor(unit: Ref<Unit>) -> bool { has_actor_in(&mut crate::abi::world().fleet,unit) }
#[cfg(target_arch = "wasm32")]
pub fn rider_posed(unit: Ref<Unit>) { rider_posed_in(&mut crate::abi::world().fleet,unit) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn battle_mount(turret: Ref<Turret>) -> Option<([f64; 3], Option<(crate::store::DefId, f64)>)> { battle_mount_in(crate::abi::world(), turret) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn refresh_weapon_masks(table:&crate::ai::combat::weapons_ai::WeaponAiTable) { refresh_weapon_masks_in(crate::abi::world(), table) }
