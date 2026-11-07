//! Persistent storage and the native calls used by Vehicle.fireSeat.
use super::*;
use crate::abi::sim;
use crate::units::f3::{self, WorldServices};
use crate::combat::defs::{K, WeaponClass};
use crate::combat::weapons::{Admission, Shot, ShotKind, WeaponDraws, LockTarget};
use crate::gen::*;
use crate::store::{Ref, StrId};
use crate::store::Slab;
use crate::flight::FlightWorld;
use crate::store::block::Block;
use crate::store::calls::Calls;
use crate::vehicles::{Hull, VehicleWorld};
use crate::vehicles::firing::{spread, WeaponGeometry};
use crate::vehicles::module::{poses, JsWorld, MemberRow, v3, vec};
use std::ptr::addr_of_mut;

// Vehicle.createSeats resolves geometry and definitions once per weapon life.
// Mutable weapon state is detached briefly while update calls the draw import,
// so no borrowed slab row survives a synchronous callback.
struct Mounted {
    birth: FfWeaponBirth,
    state: Weapon,
    class: Option<WeaponClass>,
    random:Option<crate::ai::random::SharedRandom>,
    missile_lock: bool,
    lock_angles: Vec<[f64; 2]>,
    keys: WeaponGeometry,
    input: FfWeaponInput,
    shots: Vec<Shot>,
    dirty: bool,
    random_binding: u32,
}
// Vehicle.fireSeat borrows the retained pose for geometry only. Physics and
// leases stay in their canonical slabs; this scratch Hull must never be stored
// back as an authoritative snapshot after another callback changed the hull.
struct Craft {
    id: Ref<Vehicle>,
    life: f64,
    command_walker: bool,
    legs: Vec<TowLeg>,
    leg_names: Vec<StrId>,
    leg_top: f64,
    // Geometry's Hull has no mutable physics authority. It borrows the
    // retained pose only while doing native geometry, with no foreign call.
    geometry: Hull,
    seats: Vec<Vec<Ref<Mounted>>>,
    consumed: Vec<bool>,
}
struct Cable {
    source: Ref<Vehicle>, target: Ref<Vehicle>, source_life: f64, target_life: f64,
    seat: usize, weapon: usize, length: f64, start: Vector3, end: Vector3,
}
#[derive(Clone, Copy)]
struct Emitter { identity: Ref<crate::gen::Emitter>, hull: Ref<Vehicle>, life: f64, seat: usize, weapon: usize, node: Option<three::NodeId>, aimer: Option<usize> }
// Vehicles.fireCrews iterates the ordered fleet, not this slab's slot order.
// The publication objects retain their nested lists between ticks. Cables and
// emitters also carry gameplay lives so respawns cannot adopt old effects.
pub(crate) struct State {
    emitters: Vec<Option<Emitter>>, weapons: Slab<Mounted>, crafts: Vec<Option<Craft>>, cables: Vec<Cable>,
    phase: [f64; FfPhase::WORDS], supply_result: Calls, output: Block, cable_output: Block, published: Vec<FfHullOut>, locked: StrId, attached: StrId, snapped: StrId, retow: StrId, tripped: StrId, empty: StrId, class_names: [StrId; 4],
}
fn state() -> &'static mut State {
    unsafe { (*addr_of_mut!(crate::abi::cold().fleet.firing)).get_or_insert_with(|| State { emitters: Vec::with_capacity(1024), weapons: Slab::with_capacity(512), crafts: Vec::with_capacity(128),
        cables: Vec::with_capacity(128), phase: [0.0; FfPhase::WORDS], supply_result: Calls::default(), output: Block::default(), cable_output: Block::default(), published: Vec::with_capacity(128), locked: sim().strings.intern_str("Locked"), attached: sim().strings.intern_str("attached"), snapped: sim().strings.intern_str("snapped"), retow: sim().strings.intern_str("retow"), tripped: sim().strings.intern_str("tripped"), empty: sim().strings.intern_str(""), class_names: FlightClass::NAMES.map(|n| sim().strings.intern_str(n)) }) }
}
/// captureMountedControl reads both exact weapon clocks and their next
/// barrels. The pose owns barrel rotation; the firing slab owns Weapon state.
pub(crate) fn wire_weapons<T>(hull: Ref<Vehicle>, life: f64, seat: usize,
    capture: impl FnOnce(&[(&Weapon, f64)]) -> Result<T, &'static str>) -> Result<T, &'static str> {
    let st = unsafe { (&*addr_of_mut!(crate::abi::cold().fleet.firing)).as_ref() }.ok_or("mounted control lease prerequisite missing")?;
    let craft = st.crafts.get(hull.slot as usize).and_then(Option::as_ref)
        .filter(|c| c.id == hull && c.life == life).ok_or("mounted control lease prerequisite missing")?;
    let refs = craft.seats.get(seat).ok_or("mounted control lease prerequisite missing")?;
    if refs.len() > 2 { return Err("mounted weapon count exceeds correction budget"); }
    let posed = poses().get(hull.slot as usize).and_then(Option::as_ref)
        .and_then(|p| p.pose.seats.get(seat)).ok_or("mounted control lease prerequisite missing")?;
    let unused = Weapon::default(); let mut values = [(&unused, 0.); 2];
    for (i, &reference) in refs.iter().enumerate() {
        let weapon = st.weapons.get(reference).ok_or("mounted weapon content prerequisite missing")?;
        let pose = posed.weapons.get(i).ok_or("mounted weapon content prerequisite missing")?;
        values[i] = (&weapon.state, pose.barrel);
    }
    capture(&values[..refs.len()])
}
fn node(n: f64) -> Option<three::NodeId> { (n >= 0.0).then_some(n as three::NodeId) }

#[no_mangle]
pub extern "C" fn f2_firing_words(which: u32) -> u32 {
    match which { 0 => FfBirth::WORDS as u32, 1 => FfWeaponInput::WORDS as u32, 3 => FfSupply::WORDS as u32, 4 => FfSupplyResult::WORDS as u32, _ => 0 }
}

/// Vehicle construction and respawn give mounted weapons a new gameplay life.
#[no_mangle]
// Vehicle.spawn resets weapon clocks and ammo for this hull alone. Keep the
// ordinary JS objects as mirrors, but resolve class keys, lock angles and beam
// source nodes before any per-tick weapon work can run.
pub extern "C" fn f2_firing_adopt() {
    let a = { let (row, heap) = sim().calls.args(); FfBirth::take(row, heap) };
    reset_weapons(a.hull);
    let mut craft = Craft { id: a.hull, life: a.life, command_walker: a.command_walker, legs: Vec::new(), leg_names: Vec::new(), leg_top: a.leg_top,
        geometry: Hull::default(), seats: Vec::new(), consumed: vec![false; a.seats.len()] };
    craft.geometry.pose = Some(poses()[a.hull.slot as usize].as_ref().unwrap().pose.clone());
    craft.geometry.seats.resize(a.seats.len(), Default::default());
    for leg in a.legs {
        craft.leg_names.push(leg.name);
        craft.legs.push(TowLeg { name: String::new(), thigh: leg.thigh.as_ref().map(vec), calf: leg.calf.as_ref().map(vec), foot: leg.foot.as_ref().map(vec) });
    }
    let defs = crate::combat::module::defs();
    for seat in a.seats {
        let mut refs = Vec::new();
        for birth in seat.weapons {
            let class = defs.weapons.class(birth.state.def).clone();
            // hasMissileLock uses immutable definition fields. Resolve it
            // once, alongside the model's fire points and lock cone angles.
            let missile_lock = class.base().get(K::Base).is_str("launcher") && class.base().get(K::Ordnance).get("base").is_str("missile");
            let keys = WeaponGeometry { weapon_node: node(birth.weapon_node), fallback_node: node(birth.fallback_node),
                direction_nodes: birth.direction_nodes.iter().map(|&n| node(n)).collect(), outside_limits: birth.outside.clone() };
            let lock_angles = class.levels.iter().map(|d| { let on = d.field("LockOnAngle").or(0.0); [on, d.field("LockOffAngle").or(on)] }).collect();
            // A new mount starts with human view acquisition, like reset_life.
            // Zero is a real registry target: the enclosing tick protects native
            // hints and would otherwise reject its first foreign view input.
            let r = state().weapons.insert(Mounted { random:None, lock_angles, state: birth.state.clone(), birth, class: Some(class), missile_lock, keys, input: FfWeaponInput {target:-2,..Default::default()}, shots: Vec::with_capacity(32), dirty: true, random_binding:u32::MAX });
            refs.push(r);
        }
        craft.seats.push(refs);
    }
    let i = a.hull.slot as usize;
    if state().crafts.len() <= i { state().crafts.resize_with(i + 1, || None); }
    state().crafts[i] = Some(craft);
    let seats = state().crafts[i].as_ref().unwrap().seats.len();
    for seat in 0..seats {
        let n = state().crafts[i].as_ref().unwrap().seats[seat].len();
        for weapon in 0..n {
            let wr = state().crafts[i].as_ref().unwrap().seats[seat][weapon];
            let b = &state().weapons.get(wr).unwrap().birth;
            register_emitter(b.emitter, a.hull, a.life, seat, weapon, node(b.emitter_node), node(b.emitter_aimer).map(|n| n as usize));
            let n = state().weapons.get(wr).unwrap().birth.muzzles.len();
            for k in 0..n {
                let m = &state().weapons.get(wr).unwrap().birth.muzzles[k];
                register_emitter(m.emitter, a.hull, a.life, seat, weapon, node(m.node), node(m.aimer).map(|n| n as usize));
            }
        }
    }
    // All variable publication storage is sized at construction.
    rebuild_publication();
}

// Retiring mounted rows invalidates old weapon handles before a new life is
// inserted. Emitter identities are retained separately because a beam may still
// ask whether the original occupancy has gone away.
fn reset_weapons(r: Ref<Vehicle>) { state().reset_weapons(r) }
// Vehicle.spawn constructs fresh Weapon state for this gameplay life. Keep
// its native class, geometry and work buffers; only the scalar birth state
// resets. The previous phase's foreign modifiers cannot enter the new life.
pub(crate) fn reset_life(r: Ref<Vehicle>, life: f64) { state().reset_life(r, life) }
// Vehicle retirement releases both ends of cable ownership. It cannot leave
// an anchor attached to a different vehicle that later occupies the same slot,
// even when a release occurs inside reciprocal explosion damage.
pub fn retire(r: Ref<Vehicle>) {
    reset_weapons(r);
    state().cables.retain(|t| t.source != r && t.target != r);
    rebuild_publication();
}
// Birth is where variable publication storage is sized. In particular every
// barrel kick has a reusable output slot; an idle weapon update must not build
// new per-seat arrays merely to publish unchanged rig metadata.
fn rebuild_publication() { state().rebuild_publication() }

/// SeatInput's frozen triggers and seatWeaponPolicy/AI spread modifiers.
/// The generated record is fixed width and contains no variable heap.
// Only submitted changed facts replace the retained foreign input. Reapply
// AI modifiers before latch so a later seatWeaponPolicy call can override them
// at its original slot. Triggers always come from the canonical lease later.
pub fn inputs(data: &[f64]) -> usize { state().inputs(data, crate::world::deferred()) }

/// Bind a bot seat to the same native generator its controller consumes.
/// Retain that handle in each fire invocation so recursive retirement cannot
/// redirect an outer weapon's remaining draws to a replacement mount.
pub fn bind_ai_stream(hull:crate::vehicles::f2_seats::HullLife,seat:usize,stream:crate::ai::random::SharedRandom)->bool {
    // A rejected life must not initialize the mounted bank or intern its IDs.
    if !sim().store.vehicle.get(hull.vehicle).is_some_and(|h| h.target_generation == hull.life) { return false; }
    state().bind_ai_stream(&sim().store, hull, seat, stream)
}

/// F3.phase prepares mounted lock hints before the first Unit.tick. Native
/// AI owns the same fire controls; freeze their scalar registry indices here
/// rather than observing a controller replaced by a later shot or death.
pub fn freeze_ai_targets(ai:&dyn crate::ai::owned::callbacks::Callbacks) {
    if sim().store.vehicles.vehicles.is_empty() { return; }
    state().freeze_ai_targets(&sim().store, ai)

}


/// Native AI reads the mounted owner's current weapon without a mirror.
/// A detached fire update cannot expose its temporary empty weapon state.
pub fn weapon_count(hull:crate::vehicles::f2_seats::HullLife,seat:usize)->Option<usize> {
    weapon_count_owned(&sim().store,hull,seat)
}
/// The whole native AI phase supplies its loaned canonical Store here.
pub fn weapon_count_owned(store:&crate::gen::Store,hull:crate::vehicles::f2_seats::HullLife,seat:usize)->Option<usize> {
    if store.vehicle.get(hull.vehicle)?.target_generation != hull.life { return None; }
    state().weapon_count_owned(store, hull, seat)
}
pub fn with_weapon<R>(hull:crate::vehicles::f2_seats::HullLife,seat:usize,index:usize,
    read:impl FnOnce(&Weapon)->R)->Option<R> {
    with_weapon_owned(&sim().store,hull,seat,index,read)
}
/// The whole native AI phase supplies its loaned canonical Store here.
pub fn with_weapon_owned<R>(store:&crate::gen::Store,hull:crate::vehicles::f2_seats::HullLife,seat:usize,index:usize,
    read:impl FnOnce(&Weapon)->R)->Option<R> {
    if store.vehicle.get(hull.vehicle)?.target_generation != hull.life { return None; }
    state().with_weapon_owned(store, hull, seat, index, read)
}
/// AI tuning shares the exact edit operation with ordinary F3 weapons. These
/// callbacks must only edit the supplied scalar settings, without world calls.
pub fn edit_weapon<R>(hull:crate::vehicles::f2_seats::HullLife,seat:usize,index:usize,
    edit:impl FnOnce(&mut crate::ai::combat::world::WeaponSettings)->R)->Option<R> {
    edit_weapon_owned(&sim().store,hull,seat,index,edit)
}
/// The whole native AI phase supplies its loaned canonical Store here.
pub fn edit_weapon_owned<R>(store:&crate::gen::Store,hull:crate::vehicles::f2_seats::HullLife,seat:usize,index:usize,
    edit:impl FnOnce(&mut crate::ai::combat::world::WeaponSettings)->R)->Option<R> {
    if store.vehicle.get(hull.vehicle)?.target_generation != hull.life { return None; }
    state().edit_weapon_owned(store, hull, seat, index, edit)
}

// Vehicle.setSeatPolicy can run during promotion or boarding after input was
// imported. Apply the native reset immediately, rather than waiting for a JS
// mirror to send the same values in the following phase.
pub fn policy(r: Ref<Vehicle>, seat: usize, player: bool, random: bool) { state().policy(r, seat, player, random) }

// ClassicCombatBinding.attachController changes every weapon on that seat
// to its shared AI generator before later native fire can consume a draw.
pub(crate) fn bind_random(hull: Ref<Vehicle>, seat: usize, binding: u32) { state().bind_random(hull, seat, binding) }

/// Copies just mounted state; physics and leases remain in their canonical slab.
fn dirty(st: &State, c: &Craft) -> bool {
    c.seats.iter().any(|s| s.iter().any(|&r| st.weapons.get(r).unwrap().dirty))
}
/// FfHullOut contains only hulls whose mounted state changed since publication.
pub fn publish() -> *const f64 { publish_for(None) }
// Power-up supply publishes one hull; ordinary phase publication selects dirty
// mounted rows. Neither operation copies another craft's mutable weapon state
// back into JS and risks replacing an unlatched AI edit.
fn publish_for(only: Option<Ref<Vehicle>>) -> *const f64 { state().publish_for(poses(), only) }
/// The existing F2 descriptor points to a generated firing descriptor.
/// Its event address removes getter/clear exports from the ordinary tick.
pub fn phase_ptr() -> *const f64 {
    let weapons = publish() as usize as u32;
    let st = state();
    let value = FfPhase { weapons, cables: st.cable_output.ptr() as usize as u32, events: sim().events.ptr() as usize as u32 };
    value.put(&mut st.phase, &mut st.cable_output.heap);
    st.phase.as_ptr()
}

// Weapon.update chooses draws in the same order as its JS implementation.
// Seed/admission hooks remain no-ops for mounted weapons, matching the original
// Vehicle.fireSeat path rather than the infantry prediction path.
struct Draws { hull: Ref<Vehicle>, seat: u32, weapon: u32, random:Option<crate::ai::random::SharedRandom>, binding:u32 }
impl WeaponDraws for Draws {
    fn draw(&mut self, _: u32) -> f64 {
            if let Some(random)=self.random.as_mut() {return crate::ai::random::Random::uniform(random);}
            if f3::active() { return f3::random_binding(self.binding); }
            import_f2_weapon_draw(&FfDraw { hull: self.hull, seat: self.seat, weapon: self.weapon }).value
    }
    fn legacy(&mut self, i: u32) -> f64 { self.draw(i) }
    fn seeded(&mut self, _: f64, _: f64) {}
    fn admit(&mut self, _: u32) -> Admission { Admission::Yes }
}

/// Load only fields read by firing geometry. No Hull copy is ever written
/// back after an ordnance callback, so reciprocal blast edits cannot be lost.
// Vehicle.muzzleOfSeat and shotDirection need current physics and the current
// retained pose together. Move the pose into scratch only for this native
// calculation, returning it before any target, RNG or gameplay import.
fn geometry<R>(r: Ref<Vehicle>, run: impl FnOnce(&mut Hull, &Slab<Mounted>) -> R) -> R {
    state().with_geometry(&sim().store, poses(), r, run)
}


/// weapon-targeting.js lockPick. Bot targets bypass the view cone, while
/// human acquisition and retention use the candidate's actual box or sphere.
// weapon-targeting.js uses a bot hint directly, while human acquisition and
// retention search actual target volumes. Keep the translated seat eye as the
// origin; using the hull root changes both range and the visible lock cone.
fn lock_pick(r: Ref<Vehicle>, class: &WeaponClass, w: &Weapon, seat: usize, target: i32, masks: (u32, u32), angles: [f64; 2]) -> Option<LockTarget> {
    let (origin, direction) = geometry(r, |h, _| {
        let origin = h.seat_eye(seat);
        let mut direction = h.reticle_ray(seat, &origin); direction.normalize();
        (origin, direction)
    });
    let d = w.level(class);
    let [on, off] = angles;
    let range = d.get(K::LockOnRange).or(0.0);
    if target >= -1 {
        if target < 0 { return None; }
        let facts = target_facts(r, target as usize, false, false);
        if !facts.alive || !allowed(masks.0,masks.1,facts.relation,facts.category) { return None; }
        return Some(target_key(target as usize, facts.alive));
    }
    pick_view(&mut HullQuery(r), LockView { range, on, off, relations: masks.0, categories: masks.1,
        complete: w.lock_complete, id: w.lock_target_id, generation: w.lock_generation }, &origin, &direction)
}
// The shared lock search asks lazy foreign shapes only as candidates survive
// filters. Registry keys stay frozen for this phase, while eligibility, bounds
// and sight points are queried at their original read slots.
struct HullQuery(Ref<Vehicle>);
impl LockQuery for HullQuery {
    fn count(&mut self) -> usize { super::super::target_count() }
    fn identity(&mut self, index: usize) -> (f64, f64) { super::super::target_identity(index) }
    fn hull_bounds(&mut self, hull: Ref<Vehicle>) -> Option<(Matrix4, Vector3, Vector3)> {
            let v = sim().store.vehicle.get(hull)?;
            Some((Matrix4 { elements: v.matrix }, vec(&v.bounds_min), vec(&v.bounds_max)))
    }

    fn facts(&mut self, i: usize, shapes: bool, aim: bool) -> LockFacts {
            let f = target_facts(self.0,i,shapes,aim); let m = member(i);
            LockFacts { id: m.id, generation: m.generation.unwrap_or(0.0), alive: f.alive, eligible: f.eligible, relation: f.relation, category: f.category,
                hull: if m.kind == 1 || m.kind == 3 { sim().store.vehicle.at(m.hull).map(|(r,_)| r) } else { None },
                body: (f.body >= 0.0).then_some(f.body as u32), center: vec(&f.center), radius: f.radius, aim: vec(&f.aim) }
    }
    fn clear(&mut self, origin: &Vector3, sight: &mut Vector3, length: f64, body: Option<u32>) -> bool {
            if length < 1e-6 { return true; }
            let ignore = sim().store.vehicle.get(self.0).unwrap().body.slot;
            !JsWorld::new(self.0.slot).cast(origin,sight.divide_scalar(length),length,Some(ignore)).is_some_and(|h| h.body != body || h.body.is_none())
    }
}

fn member(i: usize) -> MemberRow {
    if f3::active() {
        let facts = crate::units::f3_fleet::flight(i, u32::MAX);
        let hull = crate::units::f3_hulls::for_member(crate::units::f3_fleet::key(i));
        return MemberRow { kind: if facts.unit { 0 } else if hull.is_some() { 1 } else { 2 },
            id: facts.id, generation: facts.generation, alive: facts.alive, team: facts.team,
            hull: hull.map_or(u32::MAX, |r| r.slot), point: facts.point };
    }
    unsafe { (&*addr_of_mut!(crate::abi::cold().fleet.members))[i] }
}
fn target_key(i: usize, alive: bool) -> LockTarget { let m = member(i); LockTarget { id: m.id, generation: m.generation.unwrap_or(0.0), alive } }
fn target_facts(r: Ref<Vehicle>, i: usize, shapes: bool, aim: bool) -> FfTargetFacts {
    if f3::active() { return crate::units::f3_fleet::hull_target(r, i, shapes, aim); }
    import_f2_target_facts(&FfTargetQuery { index: i as u32, viewer: r, shapes, aim })
}

/// Vehicles.fireCrews: hull order, then seat order, then channel order.
// Flight targeting writes lock progress even on unoccupied weapon mounts.
// Publish those changes before any fired listener can inspect the weapon, rather
// than sending a second legacy lock stream over the same state.
pub fn flight_locks() {
    // updateFlightTarget leaves lock progress on every mounted weapon,
    // including weapons in unoccupied seats.
    let n = sim().store.vehicles.vehicles.len();
    for i in 0..n {
        let r = sim().store.vehicles.vehicles[i];
        let v = sim().store.vehicle.get(r).unwrap();
        if !v.flying { continue; }
        let (id, generation, elapsed, complete) = (v.flight_lock_id.unwrap_or(-1.0), v.flight_lock_generation.unwrap_or(-1.0), v.flight_lock_timer.unwrap_or(0.0), v.flight_lock_state == Some(state().locked));
        let c = state().crafts[r.slot as usize].as_ref().unwrap();
        for s in &c.seats { for &wr in s {
            let m = state().weapons.get_mut(wr).unwrap();
            let w = &mut m.state;
            m.dirty |= w.lock_target_id != id || w.lock_generation != generation || w.lock_elapsed != elapsed || w.lock_complete != complete;
            w.lock_target_id = id; w.lock_generation = generation; w.lock_elapsed = elapsed; w.lock_complete = complete;
        }}
    }
}

/// Vehicles.fireCrews keeps hull, seat and channel order.
// Vehicle.fireCrews takes hulls, seats and channels in that order. A network
// human uses the crew path too; only the world's dormant local driver is skipped.
// Cable updates remain after every hull has had its firing opportunity.
pub(crate) fn step(dt: f64, clock: f64, mut services: Option<WorldServices<'_>>) {
    let n = sim().store.vehicles.vehicles.len();
    for i in 0..n {
        let r = sim().store.vehicles.vehicles[i];
        let Some(v) = sim().store.vehicle.get(r) else { continue };
        if !v.alive { continue; }
        let seats = v.seats.len();
        for seat in 0..seats {
            let v = sim().store.vehicle.get(r).unwrap();
            if v.seats[seat].lease.is_none() || v.seat_leases[seat].human == Some(true) { continue; }
            if v.flying && v.flight_state != Some(FlightState::Flying) { continue; }
            let count = state().crafts[r.slot as usize].as_ref().unwrap().seats[seat].len();
            if count == 0 { continue; }
            state().crafts[r.slot as usize].as_mut().unwrap().consumed[seat] = true;
            for weapon in 0..count { fire_seat(r, seat, weapon, dt, clock, services.as_mut().map(WorldServices::reborrow)); }
            if let Some(lr) = sim().store.vehicle.get(r).unwrap().seats[seat].lease {
                let l = sim().store.seat_lease.get_mut(lr).unwrap();
                if l.input.reload { l.input.reload = false; super::super::live_seats::changed_lease(lr); }
            }
        }
    }
    update_tethers(services);
}

// Vehicle.fireSeat reads the just-latched canonical edges. Consume each press
// before weapon execution, retaining reload until every weapon on the seat has
// updated. A synchronous shot death cannot give those edges to a replacement.
fn fire_seat(r: Ref<Vehicle>, seat: usize, weapon: usize, dt: f64, clock: f64, mut services: Option<WorldServices<'_>>) {
    let wr = state().crafts[r.slot as usize].as_ref().unwrap().seats[seat][weapon];
    // RNG and fired hooks can reenter the simulation. Take only the weapon's
    // mutable working state and reusable shot buffer, leaving no reference into
    // the slab across imports. Restore it before emitting any resulting round.
    let random=state().weapons.get(wr).unwrap().random.clone();
    let binding=state().weapons.get(wr).unwrap().random_binding;
    let (mut w, class, input, mut shots, tow, missile_lock, masks, angles) = {
        let m = state().weapons.get_mut(wr).unwrap();
        let angles = m.lock_angles[m.state.level as usize];
        (std::mem::take(&mut m.state), m.class.take().unwrap(), std::mem::take(&mut m.input), std::mem::take(&mut m.shots), m.birth.tow, m.missile_lock, (m.birth.relation_mask, m.birth.category_mask), angles)
    };
    let (mut pressed, mut held, mut reload) = (false, false, false);
    if let Some(lr) = sim().store.vehicle.get(r).unwrap().seats[seat].lease {
        let l = sim().store.seat_lease.get_mut(lr).unwrap();
        pressed = l.input.pressed.get(weapon).copied().unwrap_or(false); held = l.input.held.get(weapon).copied().unwrap_or(false); reload = l.input.reload;
        if let Some(pressed) = l.input.pressed.get_mut(weapon) { *pressed = false; }
        if pressed { super::super::live_seats::changed_lease(lr); }
    }
    let v = sim().store.vehicle.get(r).unwrap();
    if v.flying {
        w.lock_target_id = v.flight_lock_id.unwrap_or(-1.0); w.lock_generation = v.flight_lock_generation.unwrap_or(-1.0);
        w.lock_elapsed = v.flight_lock_timer.unwrap_or(0.0); w.lock_complete = v.flight_lock_state == Some(state().locked);
    } else if missile_lock {
        let picked = lock_pick(r, &class, &w, seat, input.target, masks, angles);
        w.update_lock(&class, dt, picked);
    }
    let suppressed = tow && sim().store.vehicle.get(r).unwrap().tow_target >= 0.0;
    update_mounted(&mut w, weapon as u32, &class, dt, !suppressed && (held || pressed), !suppressed && pressed, reload,
        &mut Draws { hull: r, seat: seat as u32, weapon: weapon as u32, random, binding }, &mut shots);
    { let m = state().weapons.get_mut(wr).unwrap(); m.state = w; m.input = input; m.class = Some(class); m.dirty = true; }
    for shot in &shots {
        if !sim().store.vehicle.get(r).is_some_and(|v| v.alive) { break; }
        if let ShotKind::Round { yaw, pitch, charge, target_id, target_generation, .. } = shot.kind {
            emit(r, seat, weapon, wr, yaw, pitch, charge, target_id, target_generation, clock, services.as_mut().map(WorldServices::reborrow));
        }
    }
    if let Some(m) = state().weapons.get_mut(wr) { m.shots = shots; }
}

/// Vehicle.emitShot: geometry, spread, direct ordnance, synchronous fired
/// listeners, then presentation retained in the battle's ordered event buffer.
#[allow(clippy::too_many_arguments)]
// Vehicle.emitShot consumes the muzzle/barrel before the limits test. Preserve
// that ordering, then apply spread and any flyer assist. Tow launches share the
// same geometry but do not create an ordinary projectile.
fn emit(r: Ref<Vehicle>, seat: usize, weapon: usize, wr: Ref<Mounted>, yaw: f64, pitch: f64, charge: f64, mut target_id: f64, mut target_generation: f64, clock: f64, services: Option<WorldServices<'_>>) {
    let Some((m, muzzle, target, bot)) = geometry(r, |h, weapons| {
        let keys = &weapons.get(wr).unwrap().keys;
        let mut muzzle = Vector3::default();
        let m = h.muzzle_of_seat(seat, weapon, keys, |_| clock, &mut muzzle);
        if h.held_by_limits(seat, weapon, m.aimer, keys) { return None; }
        Some((m, muzzle, h.pose.as_ref().unwrap().seats[seat].target, h.bot_aim(seat)))
    }) else { return; };
    let tow = state().weapons.get(wr).unwrap().birth.tow;
    let fallback = if tow { tow_aim(r, seat, wr, &muzzle).unwrap_or(bot) } else { bot };
    let mut direction = geometry(r, |h, weapons| {
        h.shot_direction(seat, weapon, m.aimer, &weapons.get(wr).unwrap().keys, &muzzle, target.as_ref(), Some(&fallback))
    });
    spread(yaw, pitch, &mut direction, &mut Vector3::default());
    let v = sim().store.vehicle.get(r).unwrap();
    let flying = v.flying;
    if flying {
        target_id = if v.flight_lock_state == Some(state().locked) { v.flight_lock_id.unwrap_or(-1.0) } else { -1.0 };
        target_generation = v.flight_lock_generation.unwrap_or(-1.0);
        if seat == 0 { assist(r, &muzzle, &mut direction); }
    }
    let (owner, body, life, class, config) = {
        let v = sim().store.vehicle.get(r).unwrap();
        let owner = match v.seat_leases[seat].unit { crate::store::Nullable::Value(u) => OrdnanceOwner::Unit(u), _ => OrdnanceOwner::Vehicle(r) };
        (owner, v.body, v.target_generation, v.flight_class, crate::flight::FlightWorld::tuning(&mut JsWorld::new(r.slot), v.flight_class.unwrap_or_default()))
    };
    let (ordnance, channel, emitter) = {
        let weapon = state().weapons.get(wr).unwrap();
        let b = &weapon.birth;
        (b.ordnance, weapon.input.channel, m.emitter.map_or(b.emitter, |i| b.muzzles[i].emitter))
    };
    if tow { launch_tow(r, seat, weapon, wr, &muzzle, &direction); }
    else if let Some(def) = ordnance {
        fire_round(def, v3(&muzzle), v3(&direction), owner, Some(body), emitter, channel, target_id, target_generation,
            flying.then_some((state().class_names[class.unwrap_or_default() as usize], config.missile_lifetime, config.missile_speed)), services);
    }
    // Vehicle.emitShot captures its weapon state and muzzle before another
    // barrel advances. Game's fired observer is a journal/presentation notice;
    // owned worlds drain it in the shared stream after native work returns.
    let sample = if f3::active() {
        crate::units::f3_fleet::mounted_sample(owner,crate::units::f3_hulls::member(r))
    } else { F3MountedSample::default() };
    if f3::active() { crate::units::f3_fleet::wire_shot(&sample); }
    let event = FfShot {
        state: state().weapons.get(wr).unwrap().state.clone(), hull: r, life,
        seat: seat as u32, weapon: weapon as u32, owner, yaw, pitch, charge,
        target_id, target_generation, origin: v3(&muzzle), direction: v3(&direction), sample,
    };
    event.emit(&mut sim().events);
    if !f3::active() {
        import_f2_fired(&FfFired { shot: event, events: sim().events.ptr() as usize as u32 });
    }
}

/// Shared Ordnance.fire seam for hulls and stationary crews. Beam emitters
/// retain their original identity; only hull flight missiles get flight keys.
#[allow(clippy::too_many_arguments)]
// Both stationary crews and hull weapons execute Ordnance.fire natively.
// The ordnance scope records its own execution allocations separately from
// steady F2 scratch; the total allocator counter still includes them.
pub(crate) fn fire_round(def: crate::store::DefId, origin: Vec3, direction: Vec3, owner: OrdnanceOwner, body: Option<Ref<Body>>,
    emitter: BeamSource, channel: f64, target: f64, generation: f64, flight: Option<(StrId, f64, f64)>, services: Option<WorldServices<'_>>) -> Option<crate::ordnance::world::Shot> {
    let _scope = crate::vehicles::module::f2::allocation::enter(crate::vehicles::module::f2::allocation::ORDNANCE);
    let mut world = crate::ordnance::module::JsWorld::with_services(crate::abi::world(), services);
    let mut cx = crate::ordnance::Cx::new(&mut world);
    let round = cx.fire_round(def, origin, direction, owner, None, body, emitter, channel, Some(target), Some(generation));
    if let Some((class, lifetime, speed)) = flight { if let Some(crate::ordnance::world::Shot::Round(r)) = round {
        if cx.s().round.get(r).is_some_and(|r| r.kind == RoundKind::Missile) { cx.flight_missile(r, Some(class), lifetime, speed); }
    } }
    round

}

/// flight/targeting.js assistShot, on canonical hull fields and registry facts.
// flight/targeting.js assistShot uses the canonical lock and craft class.
// An assist changes direction only after the muzzle and spread are known; it
// must not replace the seat's stored target or the next weapon's bot aim.
fn assist(r: Ref<Vehicle>, origin: &Vector3, direction: &mut Vector3) {
    let v = sim().store.vehicle.get(r).unwrap();
    let id = v.flight_assist_id.unwrap_or(-1.0);
    let class = v.flight_class.unwrap_or_default();
    let mut world = JsWorld::new(r.slot);
    if !world.environment().assists || id < 0.0 { return; }
    let Some(i) = world.member_by_id(id) else { return };
    let facts = target_facts(r, i, false, true);
    if !facts.alive { return; }
    let point = vec(&facts.aim);
    let t = world.tuning(class);
    let mut d = point; d.sub(origin).normalize();
    if direction.dot(&d) > v8math::cos(t.assist_cone * std::f64::consts::PI / 180.0) {
        direction.lerp(&d, t.assist_strength).normalize();
    }
}

// launchTow starts from a collision body's owner. Resolve the canonical hull
// handle in fleet order rather than treating a scene body index as a vehicle
// slot, which would attach cables to unrelated moving objects.
fn hit_hull(body: Option<u32>) -> Option<Ref<Vehicle>> {
    let body = body?;
    let n = sim().store.vehicles.vehicles.len();
    for i in 0..n {
        let r = sim().store.vehicles.vehicles[i];
        if sim().store.vehicle.get(r).unwrap().body.slot == body { return Some(r); }
    }
    None
}
// nearestWalker excludes the source and, when requested, non-enemies and dead
// walkers. Keep that filter separate from a direct body hit, whose leg test
// has its own original acceptance rules.
fn command_walker(r: Ref<Vehicle>, source: Ref<Vehicle>, live_enemy: bool) -> bool {
    let Some(c) = state().crafts.get(r.slot as usize).and_then(Option::as_ref) else { return false };
    if !c.command_walker { return false; }
    if !live_enemy { return true; }
    let v = sim().store.vehicle.get(r).unwrap(); let s = sim().store.vehicle.get(source).unwrap();
    v.alive && v.team != s.team
}
// Vehicle.nearestWalker keeps the first equally near candidate. Iterate the
// fleet's construction order, not slab order after retirement/reuse, so AI and
// human tow assistance make the same selection as JS.
fn nearest_walker(source: Ref<Vehicle>, range: f64) -> Option<Ref<Vehicle>> {
    let p = vec(&sim().store.vehicle.get(source).unwrap().position);
    let mut best = None; let mut span = range * range;
    let n = sim().store.vehicles.vehicles.len();
    for i in 0..n {
        let r = sim().store.vehicles.vehicles[i];
        if !command_walker(r, source, true) { continue; }
        let d = vec(&sim().store.vehicle.get(r).unwrap().position).distance_to_squared(&p);
        if d < span { span = d; best = Some(r); }
    }
    best
}
// Vehicle.towAim chooses an anchor in the walker's bind-space leg columns.
// The animated feet do not redefine cable attachment, even while gameplay
// rendering eases those legs for the next physics substep.
fn leg_near(r: Ref<Vehicle>, origin: &Vector3) -> Option<Vector3> {
    let c = state().crafts[r.slot as usize].as_ref().unwrap();
    let v = sim().store.vehicle.get(r).unwrap();
    nearest_leg(&c.legs, &Matrix4 { elements: v.matrix }, origin).map(|(_, p)| p)
}
// Vehicle.legAt keeps the hit below the pelvis and clamps its height to the
// selected leg. Store the returned local anchor, so future cable endpoints
// follow that walker without another collision query.
fn leg_hit(r: Ref<Vehicle>, point: &Vector3) -> Option<(usize, Vector3)> {
    let c = state().crafts[r.slot as usize].as_ref().unwrap();
    let v = sim().store.vehicle.get(r).unwrap();
    let mut inverse = Matrix4 { elements: v.matrix }; inverse.invert();
    leg_at(&c.legs, c.leg_top, v.bounds_min[1], &inverse, point)
}

/// Vehicle.towAim: a body hit may choose a command walker even when it is
/// friendly; nearestWalker is the fallback and excludes friends and dead hulls.
fn tow_aim(r: Ref<Vehicle>, seat: usize, wr: Ref<Mounted>, muzzle: &Vector3) -> Option<Vector3> {
    let (origin, direction) = geometry(r, |h, _| (h.seat_eye(seat), h.bot_aim(seat)));
    let range = state().weapons.get(wr).unwrap().birth.range;
    let body = sim().store.vehicle.get(r).unwrap().body.slot;
    let hit = JsWorld::new(r.slot).cast(&origin, &direction, range, Some(body));
    let target = hit.and_then(|h| hit_hull(h.body)).filter(|&t| command_walker(t, r, false)).or_else(|| nearest_walker(r, range))?;
    let mut p = leg_near(target, muzzle)?; p.sub(muzzle).normalize(); Some(p)
}

/// Vehicles.launchTow: the harpoon must hit an enemy command walker's leg.
/// A bot's body hit retries toward the nearest leg, with the original range.
// Vehicles.launchTow first considers the body actually hit, then assistance.
// Attach both canonical endpoints before publishing an event; an immediate
// release or reciprocal blast must observe the new relationship.
fn launch_tow(r: Ref<Vehicle>, seat: usize, weapon: usize, wr: Ref<Mounted>, origin: &Vector3, direction: &Vector3) -> bool {
    let range = state().weapons.get(wr).unwrap().birth.range;
    let body = sim().store.vehicle.get(r).unwrap().body.slot;
    let Some(hit) = JsWorld::new(r.slot).cast(origin, direction, range, Some(body)) else { return false };
    let Some(target) = hit_hull(hit.body).filter(|&t| command_walker(t, r, true)) else { return false };
    let mut leg = leg_hit(target, &hit.point);
    if leg.is_none() {
        if let Some(mut p) = leg_near(target, origin) {
            p.sub(origin); let far = p.length();
            if far > 1e-3 {
                let hit = JsWorld::new(r.slot).cast(origin, p.divide_scalar(far), v8math::min(range, far + 2.0), Some(body));
                if let Some(hit) = hit.filter(|h| hit_hull(h.body) == Some(target)) { leg = leg_hit(target, &hit.point); }
            }
        }
    }
    let Some((leg, local)) = leg else { return false };
    if sim().store.vehicle.get(r).unwrap().tow_target >= 0.0 { release(r, state().retow); }
    let target_life = sim().store.vehicle.get(target).unwrap().target_generation;
    let source = sim().store.vehicle.get(r).unwrap();
    let source_life = source.target_generation;
    let p = source.position; let t = sim().store.vehicle.get(target).unwrap().position;
    let around = v8math::atan2(p[0] - t[0], p[2] - t[2]);
    let length = state().weapons.get(wr).unwrap().birth.cable_length;
    let slot = state().cables.iter().position(|t| t.source == r);
    let cable = Cable { source: r, target, source_life, target_life, seat, weapon, length, start: Vector3::default(), end: Vector3::default() };
    if let Some(i) = slot { state().cables[i] = cable; } else { state().cables.push(cable); }
    let target_index = sim().store.vehicles.vehicles.iter().position(|&t| t == target).unwrap();
    let name = state().crafts[target.slot as usize].as_ref().unwrap().leg_names[leg];
    let v = sim().store.vehicle.get_mut(r).unwrap();
    super::super::mark_changed(r);
    v.tow_target = target_index as f64; v.tow_local = v3(&local); v.tow_leg = name; v.tow_angle = around; v.tow_winding = 0.0;
    FfTow { source: r, life: source_life, seat: seat as u32, weapon: weapon as u32, reason: state().attached }.emit(&mut sim().events);
    true
}
// Vehicles.releaseTow clears the source before notifying presentation.
// Generation-qualified events can still name that historical life without
// letting a later respawn inherit its cable or winding angle.
fn release(r: Ref<Vehicle>, reason: StrId) {
    let Some(v) = sim().store.vehicle.get_mut(r) else { return };
    if v.tow_target < 0.0 { return; }
    super::super::mark_changed(r);
    v.tow_target = -1.0; v.tow_winding = 0.0; v.tow_leg = state().empty;
    if reason == state().empty { return; }
    if let Some(t) = state().cables.iter().find(|t| t.source == r) {
        FfTow { source: r, life: t.source_life, seat: t.seat as u32, weapon: t.weapon as u32, reason }.emit(&mut sim().events);
    }
}

/// Vehicles.updateTethers. Commit the source's release before damage, and
/// never store a saved source or target hull after synchronous blast damage.
// Vehicles.updateTethers follows the existing cable list order. Opposite
// turns unwind the accumulated angle, and a trip damages the walker before
// reciprocal explosion callbacks can inspect or destroy the towing craft.
fn update_tethers(mut services: Option<WorldServices<'_>>) {
    let n = state().cables.len();
    for i in 0..n {
        let (r, target, life, target_life, seat, length) = {
            let t = &state().cables[i]; (t.source, t.target, t.source_life, t.target_life, t.seat, t.length)
        };
        let Some(v) = sim().store.vehicle.get(r) else { continue };
        if v.target_generation != life || v.tow_target < 0.0 { continue; }
        let local = vec(&v.tow_local);
        let target_alive = sim().store.vehicle.get(target).is_some_and(|v| v.alive && v.target_generation == target_life);
        let matrix = Matrix4 { elements: sim().store.vehicle.get(target).unwrap().matrix };
        let start = geometry(r, |h, _| {
            let pose = h.pose.as_mut().unwrap();
            let node = pose.seats.get(seat).and_then(|s| s.turret).or_else(|| pose.seats.iter().find_map(|s| s.turret));
            let mut start = h.center;
            if let Some(node) = node { pose.rig.tree.get_world_position(node, &mut start); } else { start.apply_matrix4(&h.matrix); }
            start
        });
        let mut end = local; end.apply_matrix4(&matrix);
        state().cables[i].start = start; state().cables[i].end = end;
        if !sim().store.vehicle.get(r).unwrap().alive || !target_alive { release(r, state().empty); continue; }
        if start.distance_to(&end) > length * 1.2 { release(r, state().snapped); continue; }
        let s = sim().store.vehicle.get(r).unwrap(); let t = sim().store.vehicle.get(target).unwrap();
        let (around, wound, trip) = winding(s.tow_angle, s.tow_winding, &vec(&s.position), &vec(&t.position));
        super::super::mark_changed(r);
        let s = sim().store.vehicle.get_mut(r).unwrap(); s.tow_angle = around; s.tow_winding = wound;
        if trip {
            let owner = match s.seat_leases[seat].unit { crate::store::Nullable::Value(u) => OrdnanceOwner::Unit(u), _ => OrdnanceOwner::Vehicle(r) };
            release(r, state().tripped);
            if let Some(services) = &mut services {
                // Vehicles.updateTethers finishes the tripped walker's crew
                // deaths and blast before continuing the next retained cable.
                crate::units::f3_hulls::mutate(target, f64::INFINITY, Some(owner), services.reborrow());
            } else {
                let (tag, slot, gen) = match owner { OrdnanceOwner::Unit(u) => (2, u.slot, u.gen), _ => (3, r.slot, r.gen) };
                super::super::f2_command(target.slot, target.gen, target_life, 0, f64::INFINITY, tag, slot, gen);
            }
        }
    }
}

// BeamSource identity is created at weapon birth. Keep the actual node and
// aimer selected by JS setup, including fallback emitters that do not share a
// barrel's frame. No live JS geometry adapter is needed for following beams.
fn register_emitter(source: BeamSource, hull: Ref<Vehicle>, life: f64, seat: usize, weapon: usize, node: Option<three::NodeId>, aimer: Option<usize>) {
    if let BeamSource::Emitter(identity) = source {
        let slots = &mut state().emitters;
        if slots.len() <= identity.slot as usize { slots.resize_with(identity.slot as usize + 1, || None); }
        slots[identity.slot as usize] = Some(Emitter { identity, hull, life, seat, weapon, node, aimer });
    }
}
// A source is valid only for the same hull handle and gameplay life. The
// stationary-crew emitter namespace is handled separately, so these lookups
// cannot accidentally reinterpret a released turret lease as a hull weapon.
fn emitter(source: BeamSource) -> Option<Emitter> {
    if !super::super::active() { return None; }
    let BeamSource::Emitter(r) = source else { return None };
    state().emitters.get(r.slot as usize).copied().flatten().filter(|e| e.identity == r)
}
/// OrdnanceWorld.gone for mounted emitters, qualified by hull and gameplay life.
pub fn emitter_gone(source: BeamSource) -> Option<bool> {
    let e = emitter(source)?;
    Some(!sim().store.vehicle.get(e.hull).is_some_and(|v| v.alive && v.target_generation == e.life))
}
fn emitter_matrix(e: Emitter) -> Matrix4 {
    match e.node {
        Some(node) => poses()[e.hull.slot as usize].as_ref().unwrap().pose.rig.tree.node(node).matrix_world,
        None => Matrix4 { elements: sim().store.vehicle.get(e.hull).unwrap().matrix },
    }
}
/// OrdnanceWorld.beamLocalOrigin: retain the actual emitter's local start.
// Ordnance.fireBeam keeps its origin in the emitter's original local frame.
// Use the retained native node matrix here, before any future hull or gun
// movement, and preserve that frame when the beam asks for its next pose.
pub fn emitter_local(source: BeamSource, origin: [f64; 3]) -> Option<[f64; 3]> {
    let e = emitter(source)?;
    let mut inverse = emitter_matrix(e); inverse.invert();
    let mut p = vec(&origin); p.apply_matrix4(&inverse); Some(v3(&p))
}
/// OrdnanceWorld.beamPose reads the retained rig directly, without a JS detour.
// Ordnance.updateBeam reads the original emitter, not the current seat owner.
// This native intercept and the stationary lease intercept must both remain:
// their lifetime and fallback frame contracts are intentionally distinct.
pub fn emitter_pose(source: BeamSource, local: [f64; 3], position: &mut [f64; 3], direction: &mut [f64; 3]) -> bool {
    let Some(e) = emitter(source) else { return false };
    if emitter_gone(source) == Some(true) { return true; }
    let mut p = vec(&local); p.apply_matrix4(&emitter_matrix(e)); *position = v3(&p);
    let wr = state().crafts[e.hull.slot as usize].as_ref().unwrap().seats[e.seat][e.weapon];
    let d = geometry(e.hull, |h, weapons| {
        h.beam_direction(e.seat, e.weapon, e.node, e.aimer, &weapons.get(wr).unwrap().keys, &mut Vector3::default())
    });
    *direction = v3(&d); true
}

/// PowerupStation.serve sends ammunition as one scalar command per hull.
/// It never imports the mirrored weapon clocks back into the mounted slab.
#[no_mangle]
pub extern "C" fn f2_firing_supply() -> *const f64 {
    let a = { let (row, heap) = sim().calls.args(); FfSupply::take(row, heap) };
    let changed = supply_station(a.hull, a.life, a.amount);

    let result = FfSupplyResult { changed, output: publish_for(Some(a.hull)) as usize as u32 };
    state().supply_result.result(FfSupplyResult::WORDS, |row, heap| result.put(row, heap));
    state().supply_result.block.ptr()
}

// givePowerup uses the same canonical mounted ammunition as Station.serve.
// The caller owns publication; no nested export or second hull copy is needed.
pub(crate) fn supply(hull: Ref<Vehicle>, amount: f64, powerup: bool) -> bool {
    let c = state().crafts[hull.slot as usize].as_ref().expect("prepared supply hull");
    if c.id != hull { return false; }
    let mut changed = false;
    for seat in &c.seats { for &r in seat {
        let m = state().weapons.get_mut(r).unwrap();
        let level = m.state.level(m.class.as_ref().unwrap());
        if crate::combat::weapons::supply(&mut m.state,level,amount,powerup) {
            m.dirty = true; changed = true;
        }
    }}
    changed
}

/// Native stations reject an old hull life before changing its retained guns.
pub(crate) fn supply_station(hull: Ref<Vehicle>,life:f64,amount:f64)->bool{
    if !sim().store.vehicle.get(hull).is_some_and(|v|v.target_generation==life){return false;}
    supply(hull,amount,false)
}
/// Statistics observe the seat's first weapon, independently of selection.
pub(crate) fn first_weapon(hull:Ref<Vehicle>,seat:usize)->Option<(crate::store::DefId,f64)>{
    let st=state();let craft=st.crafts.get(hull.slot as usize)?.as_ref()?;
    if craft.id!=hull{return None;}let weapon=*craft.seats.get(seat)?.first()?;
    let w=&st.weapons.get(weapon)?.state;Some((w.def,w.level))
}

impl State {
    fn reset_weapons(&mut self, r: Ref<Vehicle>) {
        let st = self;
        if let Some(slot) = st.crafts.get_mut(r.slot as usize) {
            if slot.as_ref().is_some_and(|c| c.id == r) {
                let c = slot.take().unwrap();
                for s in c.seats { for w in s { st.weapons.remove(w); } }
            }
        }
    }
    fn reset_life(&mut self, r: Ref<Vehicle>, life: f64) {
        let st = self;
        let craft = st.crafts[r.slot as usize].as_mut().expect("prepared mounted hull");
        craft.life = life; craft.consumed.fill(false);
        for seat in &craft.seats { for &reference in seat {
            let weapon = st.weapons.get_mut(reference).unwrap();
            weapon.state.clone_from(&weapon.birth.state);
            weapon.random_binding = u32::MAX;
            weapon.shots.clear(); weapon.dirty = true;
            weapon.input = FfWeaponInput {
                channel: weapon.birth.channel, target: -2, has_random: weapon.state.random,
                random_binding: u32::MAX, floor: weapon.state.floor,
                spread_scale: weapon.state.spread_scale, sniper_scale: weapon.state.sniper_scale,
                yaw_spread_override: weapon.state.yaw_spread_override,
            };
        } }
        for emitter in st.emitters.iter_mut().flatten() {
            if emitter.hull == r { emitter.life = life; }
        }
    }
    fn rebuild_publication(&mut self, ) {
        let st = self;
        st.published.clear();
        for c in st.crafts.iter().flatten() {
            let mut seats = Vec::new();
            for (i, s) in c.seats.iter().enumerate() {
                let mut weapons = Vec::new();
                for &r in s {
                    let w = st.weapons.get(r).unwrap();
                    weapons.push(FfWeaponOut { state: w.state.clone(), random_binding:w.random_binding, barrel: 0.0, kicks: vec![None; w.birth.muzzles.len()] });
                }
                seats.push(FfSeatOut { consumed: c.consumed[i], weapons });
            }
            st.published.push(FfHullOut { hull: c.id, life: c.life, seats });
        }
        st.cable_output.words.reserve(8192); st.cable_output.heap.reserve(1024);
        st.output.words.reserve(32768); st.output.heap.reserve(32768);
    }
    fn inputs(&mut self, data: &[f64], whole_tick:bool) -> usize {
        let mut o = 0;
        let st = self;
        for c in st.crafts.iter_mut().flatten() {
            for used in &mut c.consumed { *used = false; }
        }
        let header = FfInputs::take(data, &[]); o += FfInputs::WORDS;
        for _ in 0..header.count {
            let a = FfWeaponChange::take(&data[o..], &[]); o += FfWeaponChange::WORDS;
            let c = st.crafts[a.hull.slot as usize].as_ref().unwrap();
            if c.id != a.hull || c.life != a.life { continue; }
            let wr = c.seats[a.seat as usize][a.weapon as usize];
            // A newly boarded native bot supersedes the earlier unoccupied
            // seat input. Its target hint was frozen after AI at the source slot.
            if whole_tick && st.weapons.get(wr).unwrap().input.target != -2 {continue;}
            st.weapons.get_mut(wr).unwrap().input = a.input;
        }
        for c in st.crafts.iter().flatten() { for seat in &c.seats { for &wr in seat {
            let w = st.weapons.get_mut(wr).unwrap();
            w.state.random = w.input.has_random;
            if f3::active() { w.random_binding = w.input.random_binding; }
            w.state.floor = w.input.floor;
            if w.state.kick < w.state.floor { w.state.kick = w.state.floor; }
            w.state.spread_scale = w.input.spread_scale; w.state.sniper_scale = w.input.sniper_scale;
            w.state.yaw_spread_override = w.input.yaw_spread_override;
        } } }
        o
    }
    fn bind_random(&mut self, hull: Ref<Vehicle>, seat: usize, binding: u32) {
        let craft = self.crafts[hull.slot as usize].as_ref().unwrap();
        for &reference in &craft.seats[seat] {
            let weapon = self.weapons.get_mut(reference).unwrap();
            weapon.random_binding = binding; weapon.state.random = binding != u32::MAX;
            weapon.input.random_binding = binding; weapon.input.has_random = weapon.state.random;
            weapon.dirty = true;
        }
    }
    fn policy(&mut self, r: Ref<Vehicle>, seat: usize, player: bool, random: bool) {
        let c = self.crafts[r.slot as usize].as_ref().unwrap();
        for &wr in &c.seats[seat] {
            let w = self.weapons.get_mut(wr).unwrap(); let d = w.state.level(w.class.as_ref().unwrap());
            w.random=None;
            w.state.floor = if player { -d.get(K::SpreadThreshold).or(0.0) } else { 0.0 };
            if w.state.kick < w.state.floor { w.state.kick = w.state.floor; }
            w.state.spread_scale = 1.0; w.state.sniper_scale = 0.0; w.state.yaw_spread_override = None; w.state.random = random; w.dirty = true;
            if f3::active() {
                w.random_binding = if random { f3::seat_binding() } else { u32::MAX };
                w.input.random_binding = w.random_binding; w.input.has_random = random;
                w.input.floor = w.state.floor; w.input.spread_scale = 1.0;
                w.input.sniper_scale = 0.0; w.input.yaw_spread_override = None;
            }
        }
    }
    fn bind_ai_stream(&mut self, store: &Store, hull:crate::vehicles::f2_seats::HullLife,seat:usize,stream:crate::ai::random::SharedRandom)->bool {
        if !(*store).vehicle.get(hull.vehicle).is_some_and(|h|h.target_generation==hull.life) {return false;}
        let st=self;let Some(craft)=st.crafts.get(hull.vehicle.slot as usize).and_then(Option::as_ref) else {return false};
        if craft.id!=hull.vehicle||craft.life!=hull.life {return false;}
        let Some(weapons)=craft.seats.get(seat) else {return false};
        for &r in weapons {
            let weapon=st.weapons.get_mut(r).unwrap();assert!(weapon.class.is_some(),"AI bind outside a detached fire update");
            // SeatUnit.initialize's seatArsenal assigns a missing authored
            // channel from the weapon's seat index. Its cold cooked value must
            // reach projectile attribution before the first native bot shot.
            weapon.input.channel=weapon.birth.channel;
            weapon.random=Some(stream.clone());weapon.dirty|=!weapon.state.random;weapon.state.random=true;
        }
        true
    }
    fn freeze_ai_targets(&mut self, store: &Store, ai:&dyn crate::ai::owned::callbacks::Callbacks) {
        let count=(*store).vehicles.vehicles.len();
        for index in 0..count {
            let hull=(*store).vehicles.vehicles[index];
            let Some(craft)=self.crafts.get(hull.slot as usize).and_then(Option::as_ref) else{continue};
            if craft.id!=hull {continue;}
            let seats=craft.seats.len();
            for seat in 0..seats {
                let hint=(*store).vehicle.get(hull).and_then(|hull|hull.seats.get(seat)).and_then(|seat|seat.lease)
                    .and_then(|lease|(*store).seat_lease.get(lease)).and_then(|lease|lease.unit.map(|unit|(unit,lease.controllable)))
                    .map(|(unit,control)|ai.weapon_hint(&(*store),unit,control));
                let target=match hint {
                    Some(hint) if hint.bot=>hint.target.and_then(|target| {
                        // Ordinary factory order matches native slots. Check the
                        // complete qualified reference before using that index;
                        // sparse or reordered registries retain source lookup.
                        let list=&store.units.list;
                        if let crate::gen::Member::Unit(unit)=target {
                            let slot=unit.slot as usize;if list.get(slot)==Some(&target) {return Some(slot);}
                        }
                        list.iter().position(|member|*member==target)
                    }).map_or(-1,|index|index as i32),
                    _=>-2,
                };
                let count=self.crafts[hull.slot as usize].as_ref().unwrap().seats[seat].len();
                for weapon in 0..count {
                    let reference=self.crafts[hull.slot as usize].as_ref().unwrap().seats[seat][weapon];
                    self.weapons.get_mut(reference).unwrap().input.target=target;
                }
            }
        }
    }
    fn weapon_count_owned(&mut self, store:&crate::gen::Store,hull:crate::vehicles::f2_seats::HullLife,seat:usize)->Option<usize> {
        if store.vehicle.get(hull.vehicle)?.target_generation!=hull.life {return None;}
        let craft=self.crafts.get(hull.vehicle.slot as usize)?.as_ref()?;
        if craft.id!=hull.vehicle||craft.life!=hull.life {return None;}
        Some(craft.seats.get(seat)?.len())
    }
    fn with_weapon_owned<R>(&mut self, store:&crate::gen::Store,hull:crate::vehicles::f2_seats::HullLife,seat:usize,index:usize,
    read:impl FnOnce(&Weapon)->R)->Option<R> {
        if store.vehicle.get(hull.vehicle)?.target_generation!=hull.life {return None;}
        let st=self;let craft=st.crafts.get(hull.vehicle.slot as usize)?.as_ref()?;
        if craft.id!=hull.vehicle||craft.life!=hull.life {return None;}
        let weapon=st.weapons.get(*craft.seats.get(seat)?.get(index)?)?;
        weapon.class.as_ref()?;
        Some(read(&weapon.state))
    }
    fn edit_weapon_owned<R>(&mut self, store:&crate::gen::Store,hull:crate::vehicles::f2_seats::HullLife,seat:usize,index:usize,
    edit:impl FnOnce(&mut crate::ai::combat::world::WeaponSettings)->R)->Option<R> {
        if store.vehicle.get(hull.vehicle)?.target_generation!=hull.life {return None;}
        let st=self;let craft=st.crafts.get(hull.vehicle.slot as usize)?.as_ref()?;
        if craft.id!=hull.vehicle||craft.life!=hull.life {return None;}
        let weapon=st.weapons.get_mut(*craft.seats.get(seat)?.get(index)?)?;
        weapon.class.as_ref()?;
        let (result,changed)=crate::ai::owned::weapons::edit_state(&mut weapon.state,edit);
        // prepareStep reapplies retained foreign settings before firing. Native
        // Aimer.update/prepareSpread now own these edits, so retain them there too;
        // a later explicit seatWeaponPolicy input still overrides at its old slot.
        weapon.input.floor=weapon.state.floor;
        weapon.input.spread_scale=weapon.state.spread_scale;
        weapon.input.sniper_scale=weapon.state.sniper_scale;
        weapon.input.yaw_spread_override=weapon.state.yaw_spread_override;
        weapon.dirty|=changed;Some(result)
    }
    fn publish_for(&mut self, poses: &[Option<crate::vehicles::module::Posed>], only: Option<Ref<Vehicle>>) -> *const f64 {
        let st = self;
        let n = st.crafts.iter().flatten().filter(|c| only.is_none_or(|r| c.id == r) && dirty(st, c)).count();
        st.output.begin(n, FfHullOut::WORDS);
        let mut at = 0;
        // The shape of each nested record was created at birth. Clone_from here
        // copies scalar Weapon state into that retained shape; it does not clone class
        // tables, definitions, geometry or animation clips during the phase.
        for row in &mut st.published {
            if only.is_some_and(|r| row.hull != r) { continue; }
            let c = st.crafts[row.hull.slot as usize].as_ref().unwrap();
            let changed = c.seats.iter().any(|s| s.iter().any(|&r| st.weapons.get(r).unwrap().dirty));
            if !changed { continue; }
            row.life = c.life;
            for (i, seat) in row.seats.iter_mut().enumerate() {
                seat.consumed = c.consumed[i];
                for (j, out) in seat.weapons.iter_mut().enumerate() {
                    let w = st.weapons.get_mut(c.seats[i][j]).unwrap();
                    out.state.clone_from(&w.state); out.random_binding = w.random_binding; w.dirty = false;
                    if let Some(p) = poses[c.id.slot as usize].as_ref() {
                        let wp = &p.pose.seats[i].weapons[j]; out.barrel = wp.barrel;
                        for (k, kick) in out.kicks.iter_mut().enumerate() { *kick = wp.muzzles[k].kick_at; }
                    }
                }
            }
            let (words, heap) = st.output.row(at); row.put(words, heap); at += 1;
        }
        st.output.finish();
        st.cable_output.begin(st.cables.len(), FfCableOut::WORDS);
        for (i, t) in st.cables.iter().enumerate() {
            let out = FfCableOut { source: t.source, target: t.target, seat: t.seat as u32, weapon: t.weapon as u32,
                length: t.length, start: v3(&t.start), end: v3(&t.end) };
            let (words, heap) = st.cable_output.row(i); out.put(words, heap);
        }
        st.cable_output.finish();
        st.output.ptr()
    }
}

impl State {
    // The authoritative rig has one owner even though fire geometry uses Hull
    // methods. Swapping it back also retains barrel turns consumed by a muzzle
    // that is later rejected for being outside its limits.
    fn with_geometry<R>(&mut self, store: &Store, poses: &mut [Option<crate::vehicles::module::Posed>], r: Ref<Vehicle>,
        run: impl FnOnce(&mut Hull, &Slab<Mounted>) -> R) -> R {
        let c = self.crafts[r.slot as usize].as_mut().unwrap();
        let v = store.vehicle.get(r).unwrap();
        let h = &mut c.geometry;
        h.id = r; h.alive = v.alive; h.flying = v.flying;
        h.position = vec(&v.position); h.yaw = v.yaw;
        h.center = vec(&v.center); h.size = vec(&v.size); h.matrix.elements = v.matrix;
        h.quaternion = three::Quaternion::new(v.quaternion[0], v.quaternion[1], v.quaternion[2], v.quaternion[3]);
        h.seat_index = v.seat_index as usize;
        for (i, s) in h.seats.iter_mut().enumerate() {
            s.aim_yaw = v.seats[i].aim_yaw; s.pitch = v.seats[i].pitch;
            s.lease = v.seats[i].lease.map(|_| crate::vehicles::hull::Lease { human: v.seat_leases[i].human == Some(true), ..Default::default() });
        }
        std::mem::swap(h.pose.as_mut().unwrap(), &mut poses[r.slot as usize].as_mut().unwrap().pose);
        let result = run(h, &self.weapons);
        std::mem::swap(h.pose.as_mut().unwrap(), &mut poses[r.slot as usize].as_mut().unwrap().pose);
        result
    }
}

/// Mounted lock acquisition shares the cooked bot masks after cold loading.
/// Updating policy must not respawn mounted rows or replace spread streams.
pub(crate) fn refresh_weapon_masks(table:&crate::ai::combat::weapons_ai::WeaponAiTable) {
    let Some(firing)=crate::abi::world().fleet.firing.as_mut() else {return};
    for slot in 0..firing.weapons.slots() {
        let Some((_,weapon))=firing.weapons.at_mut(slot) else {continue};
        let Some(class)=weapon.class.as_ref() else {continue};
        let ai=table.weapon_ai(&class.levels[0].obj);
        weapon.birth.relation_mask=ai.relation_mask;weapon.birth.category_mask=ai.category_mask;
    }
}
