//! Persistent fleet scheduler. The generated slab is canonical; input contains
//! foreign facts only. No complete vehicle row is imported by an ordinary tick.
pub use crate::vehicles::f2_seats as seats;
pub mod live_seats;
pub mod firing;
pub(crate) mod allocation;
pub mod riders;
pub(super) mod geometry;
pub use crate::vehicles::f2_delta as delta;

use super::*;

const CAPACITY: usize = 262144;
const FLAG_POSES: u32 = 1 << F2PhaseFlag::Poses as u32;
const FLAG_COSMETIC: u32 = 1 << F2PhaseFlag::Cosmetic as u32;
const FLAG_FLIGHT: u32 = 1 << F2PhaseFlag::Flight as u32;
const FLAG_ASSISTS: u32 = 1 << F2PhaseFlag::Assists as u32;
// Vehicle.load resolves class data once. Working rows keep dynamic solver
// scratch, while revisions avoid reloading the row just stored by a substep.
// Spare rows permit synchronous damage hooks without aliasing the outer hull.
struct HullRow { leg_side: Vec<f64>, revision: u64, pool: Vec<Hull>, flight_class: crate::flight::FlightClass, config: Config, gear: Option<GearClip>, texts: crate::flight::store::FlightTextIds }
fn scheduler() -> &'static mut Scheduler { unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler) } }

pub(crate) struct Scheduler {
    active: bool,
    io: Vec<f64>,
    descriptor: [f64; F2Phase::WORDS],
    publication: Option<crate::vehicles::f2_publication::Publication>,
    descs: Vec<super::super::spawn::SlotDesc>,
    pad_classes: Vec<Vec<(f64, StrId)>>,
    acc: f64,
    time: f64,
    pending: bool,
    publication_pending: bool,
    hull_rows: Vec<Option<HullRow>>,
}
impl Scheduler {
    pub(super) const fn new() -> Self {
        Self {
            active: false,
            io: Vec::new(),
            descriptor: [0.0; F2Phase::WORDS],
            publication: None,
            descs: Vec::new(),
            pad_classes: Vec::new(),
            acc: 0.0,
            time: 0.0,
            pending: false,
            publication_pending: false,
            hull_rows: Vec::new(),
        }
    }
}

/// Vehicle construction resolves its class and flight keys once. Spare working
/// rows let a synchronous damage hook reenter without borrowing the outer hull.
fn register_hull(r: Ref<Vehicle>) {
    let s = sim(); let v = s.store.vehicle.get(r).unwrap();
    let name = String::from_utf16_lossy(s.strings.units(v.name));
    let class = classes().get(&name).expect("registered hull class");
    let (config, gear) = if v.flying { flight_keys(&name) } else { (Config::default(), None) };
    let flight_class = if v.flying { crate::flight::config::flight_class(&name) } else { crate::flight::FlightClass::default() };
    let leg_count = poses().get(r.slot as usize).and_then(|p|p.as_ref())
        .and_then(|p|p.pose.leg_pairs.as_ref()).map_or(0,|legs|legs.at.len());
    let mut pool = Vec::with_capacity(4);
    for _ in 0..4 {
        let mut hull = Hull::from_store(r,v,class,0);
        hull.leg_side_scratch.reserve(leg_count);
        hull.support_heights.reserve(leg_count);
        if v.flying { hull.target_scratch.reserve(if crate::units::f3::active() { s.store.units.list.len() } else { unsafe { (&*addr_of_mut!(crate::abi::cold().fleet.members)).len() } }); }
        pool.push(hull);
    }
    let texts = crate::flight::store::FlightTextIds::new(&mut sim().strings);
    let rows = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.hull_rows) };
    if rows.len() <= r.slot as usize { rows.resize_with(r.slot as usize + 1, || None); }
    rows[r.slot as usize] = Some(HullRow { leg_side:Vec::with_capacity(leg_count), revision: 1, pool, flight_class, config, gear, texts });
    let inputs = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.flight_inputs) };
    if inputs.len() <= r.slot as usize { inputs.resize(r.slot as usize + 1,None); }
    reserve_phase(unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.hull_rows) }.iter().flatten().count());
}
fn reserve_phase(count: usize) {
    // Vehicles.update keeps these five phase lists between calls. Reserve
    // them at hull construction too; warming a tick would hide their first
    // allocation from ordinary scheduler tests.
    unsafe {
        let refs = &mut *addr_of_mut!(crate::abi::cold().fleet.phase_refs); refs.reserve(count.saturating_sub(refs.len()));
        let mine = &mut *addr_of_mut!(crate::abi::cold().fleet.phase_mine); mine.reserve(count.saturating_sub(mine.len()));
        let posed = &mut *addr_of_mut!(crate::abi::cold().fleet.phase_posed); posed.reserve(count.saturating_sub(posed.len()));
        let phase_locks = &mut *addr_of_mut!(crate::abi::cold().fleet.phase_locks); phase_locks.reserve(count.saturating_sub(phase_locks.len()));
        let locks = &mut *addr_of_mut!(crate::abi::cold().fleet.locks);
        locks.reserve((F2LockHeader::WORDS + count * F2Lock::WORDS).saturating_sub(locks.len()));
    }
}
// Units.add grows the canonical registry at construction. Reserve every
// flight target buffer there, so a late pilot or a first lock cannot grow it
// during Vehicles.update. Cache entries reset without changing their storage.
pub(crate) fn reserve_registry(count: usize) {
    if !active() { return; }
    for row in unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.hull_rows) }.iter_mut().flatten() {
        for hull in &mut row.pool {
            if hull.flying { hull.target_scratch.reserve(count); }
        }
    }
    let wanted = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.aim_wanted) };
    wanted.reserve(count.saturating_sub(wanted.len()));
    let cache = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.aim_cache) };
    cache.resize(count.max(cache.len()),None);
    live_seats::reserve_riders(count);
}
/// SeatLease.origin/forward read the retained simulation rig at their source
/// observation point. Native AI never asks a facade to update that geometry.
pub fn seat_origin(life:crate::vehicles::f2_seats::HullLife,index:usize)->Option<three::Vector3> {
    seat_origin_owned(&sim().store,life,index)
}
pub fn seat_forward(life:crate::vehicles::f2_seats::HullLife,index:usize)->Option<three::Vector3> {
    seat_forward_owned(&sim().store,life,index)
}
// Vehicle.seatEye observes retained seat geometry and a few current scalars.
// AI already owns Store here: exchanging the entire Store and loading a
// solver Hull for every sensor poll needlessly moves unrelated state.
pub(crate) fn seat_origin_owned(store:&Store,life:crate::vehicles::f2_seats::HullLife,index:usize)->Option<three::Vector3> {
    let row=store.vehicle.get(life.vehicle)?;
    if row.target_generation!=life.life||index>=row.seats.len() {return None;}
    let pose=&poses().get(life.vehicle.slot as usize)?.as_ref()?.pose;
    let view=pose.seats.get(index)?;
    let q=three::Quaternion::new(row.quaternion[0],row.quaternion[1],row.quaternion[2],row.quaternion[3]);
    let seat=&row.seats[index];let aim=(index!=0).then_some((seat.aim_yaw,seat.pitch,view));
    let frame=crate::vehicles::pose::seat_frame_values(&vec(&row.position),&q,row.flying,row.yaw,aim);
    let mut eye=view.eye;eye.apply_matrix4(&frame);Some(eye)
}
// Vehicle.seatForward keeps the flyer's pilot quaternion special case and
// the source aim-vector arithmetic for every other seat. Preserve the same
// pose-presence gate even though this observation needs no rig mutation.
pub(crate) fn seat_forward_owned(store:&Store,life:crate::vehicles::f2_seats::HullLife,index:usize)->Option<three::Vector3> {
    let row=store.vehicle.get(life.vehicle)?;
    if row.target_generation!=life.life||index>=row.seats.len() {return None;}
    poses().get(life.vehicle.slot as usize)?.as_ref()?;
    if row.flying&&index==0 {
        let mut out=three::Vector3::new(0.,0.,1.);
        out.apply_quaternion(&three::Quaternion::new(row.quaternion[0],row.quaternion[1],row.quaternion[2],row.quaternion[3]));Some(out)
    }else {let seat=&row.seats[index];Some(crate::vehicles::pose::aim_vector(seat.aim_yaw,seat.pitch))}
}

// initFlight uses immutable class/gear handles. GearClip shares its clip by
// reference count; copying this handle allocates no animation or key table.
// Mutable flight state continues to come from the canonical vehicle row.
pub fn flight_keys_for(r: Ref<Vehicle>) -> (Config, Option<GearClip>) { scheduler().flight_keys_for(r) }
// Pilot observations need class handling only, not the retained gear clip or
// a mutable solver hull. The caller validates the canonical hull generation.
pub(crate) fn flight_config(r:Ref<Vehicle>)->Option<Config> {
    Some(scheduler().hull_rows.get(r.slot as usize)?.as_ref()?.config)
}
/// Write flight enums through the construction-time text handles.
pub fn store_flight(hull: &Hull) { scheduler().store_flight(&mut sim().store, hull) }
// Vehicles.update can revisit a hull several times in one tick. Reload after
// an external canonical write, but reuse the current working row across native
// substeps. A callback never receives a borrow into this pool.
pub fn take_hull(r: Ref<Vehicle>, index: usize) -> Hull { scheduler().take_hull(&sim().store, r, index) }
// Hull.to_store keeps the canonical optional leg list absent until the
// original standOnLegs observation. Its cold buffer survives that absence.
pub(crate) fn prepare_leg_store(hull: &Hull, vehicle: &mut Vehicle) { scheduler().prepare_leg_store(hull, vehicle) }
pub fn recycle(hull: Hull) { scheduler().recycle(hull) }


pub fn active() -> bool { unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.active) } }
fn io() -> &'static mut Vec<f64> { unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.io) } }
/// WaterState.step precedes the native fleet span. The enclosing world sets
/// this generated scalar after advancing water, before F3 calls F2's entry.
pub(crate) fn world_level(level:f64) {
    if active() {io()[F2Input::LEVEL]=level;}
}
fn publication() -> &'static mut crate::vehicles::f2_publication::Publication {
    unsafe { (*addr_of_mut!(crate::abi::cold().fleet.scheduler.publication)).get_or_insert_with(Default::default) }
}

// Seat commands and cable writes can change canonical fields outside physics.
// Invalidate working copies as well as publication, since the next native
// substep must see those changes even if a mirror was already published.
pub fn mark(r: Ref<Vehicle>) { scheduler().mark(unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.geometry) }, r) }
/// A completed native substep already has the canonical dynamic fields. Keep
/// that working row current; other spare rows reload only after a real write.
pub fn stored(hull: &Hull) { scheduler().stored(unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.geometry) }, hull) }
pub fn mark_changed(r: Ref<Vehicle>) { mark(r); }
pub(crate) fn has_pending()->bool { unsafe { (&*addr_of_mut!(crate::abi::cold().fleet.scheduler)).has_pending() } }
pub(crate) fn pending() { unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.pending) = true; } }
// VehicleTarget queries reuse the same invalidation stamp as working hulls.
// Physics stores and seat/damage commands already advance it before readers.
pub(crate) fn revision(r: Ref<Vehicle>) -> u64 { scheduler().revision(r) }
// F3's Unit.onMoved barrier need not republish an untouched fleet. Damage or
// seat work marks this bit; its next mirror barrier flushes those rows once.
pub(crate) fn publish_pending() {
    let deferred=crate::world::deferred();
    if active() && (scheduler().pending || (!deferred && scheduler().publication_pending)) { publish(); }
}

/// Vehicle construction registers its row without rewriting other pads or hulls.
#[no_mangle]
pub extern "C" fn f2_register(slot: u32, generation: u32, prepared: u32) {
    let r = Ref::new(slot, generation);
    let s = sim();
    let v = s.store.vehicle.get(r).expect("registered F2 hull");
    let pad = v.slot_index as usize;
    let name = v.name;
    if prepared == 0 && !s.store.vehicles.vehicles.contains(&r) { s.store.vehicles.vehicles.push(r); }
    if !s.store.vehicles.slots[pad].variants.entries.iter().any(|&(n, _)| n == name) {
        s.store.vehicles.slots[pad].variants.entries.push((name, r));
    }
    let legs = poses().get(slot as usize).and_then(|p|p.as_ref())
        .and_then(|p|p.pose.leg_pairs.as_ref()).map_or(0,|legs|legs.at.len());
    publication().register(&s.store, r, legs);
    register_hull(r);
    let count = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.hull_rows) }.iter().flatten().count();
    let fleet = &mut sim().store.vehicles.vehicles;
    fleet.reserve(count.saturating_sub(fleet.len()));
}

// CollisionWorld keeps the active body order separate from cold variant
// geometry. Triangle and broadphase storage must cover both before a tick.
#[no_mangle]
pub extern "C" fn f2_prepare_body(kernel: u32) {
    let c = collision_world().expect("prepared fleet collision");
    let state = crate::collision::state();
    let w = &mut state.worlds[c.world];
    w.order.retain(|&slot| slot != kernel);
    assert!(!w.prepared.contains(&kernel));
    w.prepared.push(kernel);
    w.order.reserve(w.prepared.len());
    w.version += 1;
    crate::collision::broad::prepare(state,c.world);
    prepare_collision();
}

// Vehicles.variant becomes a native activation when a prepared pad first
// chooses this class. Its facade is bound by the structural birth journal.
fn activate(r: Ref<Vehicle>) {
    let index = sim().store.vehicles.vehicles.len();
    sim().store.vehicles.vehicles.push(r);
    live_seats::activate(r,index);
    crate::units::f3_hulls::activate(r,index);
    // Cold publication may have reused a working row. Reload the new target
    // id before spawn stores its first active life back into the slab.
    mark(r);
}

fn desc() -> &'static mut [f64; F2Phase::WORDS] { unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.descriptor) } }

/// Allocate once while loading. The stable descriptor points at input and at
/// each publication; output blocks may move, so their pointers are republished.
#[no_mangle]
// Vehicles construction supplies pad descriptions and fixed class names.
// The descriptor is a generated record; buffers remain battle-local because
// one WASM instance owns one battle and its callback stack.
pub extern "C" fn f2_init(json: u32, time: f64, acc: f64) -> *const f64 {
    let text = String::from_utf16_lossy(sim().strings.units(StrId(json)));
    let value = bfe_content::json::parse(&text).expect("F2 pad descriptions");
    let ds: Vec<_> = value.as_array().expect("F2 pads").iter().map(super::super::spawn::SlotDesc::of).collect();
    let names = ds.iter().map(|d| d.classes.iter().filter_map(|(key, name)| {
        let team = bfe_content::js::string_to_number(key);
        (bfe_content::js::number_to_string(team) == *key && !name.is_empty()).then(|| (team, sim().strings.intern_str(name)))
    }).collect()).collect();
    unsafe {
        *addr_of_mut!(crate::abi::cold().fleet.scheduler.pad_classes) = names;
        *addr_of_mut!(crate::abi::cold().fleet.scheduler.descs) = ds;
        *addr_of_mut!(crate::abi::cold().fleet.scheduler.active) = true;
        *addr_of_mut!(crate::abi::cold().fleet.scheduler.time) = time;
        *addr_of_mut!(crate::abi::cold().fleet.scheduler.acc) = acc;
    }
    io().resize(CAPACITY, 0.0);
    // An empty fleet can still own stationary crews. Its first phase writes
    // the lock header even without a hull, so reserve that header here too.
    reserve_phase(sim().store.vehicles.vehicles.len());
    sim().events.reserve(262144, FfShot::WORDS, 1024);
    prepare_collision();
    desc()[F2Phase::INPUT] = io().as_mut_ptr() as usize as f64;
    desc()[F2Phase::CAPACITY] = CAPACITY as f64;
    // Vehicles construction owns the initial complete mirror. Later incoming
    // damage can then send a sparse delta even before Vehicles.update runs.
    publish();
    desc().as_ptr()
}

/// Publish without a getter crossing from JS, also before a synchronous hook.
/// No reference to the store survives a foreign call.
// Flush the pending touched rows before exposing addresses. Native callbacks
// can publish recursively, so every block represents this flush rather than
// an append-only fleet snapshot that a later hook might read again.
pub fn publish() {
    if crate::world::deferred() {
        live_seats::sample_changed();
        // Pose readers observed this logical barrier. Only facade encoding
        // stays pending; later empty native barriers must not resample rigs.
        scheduler().pending=false;scheduler().publication_pending=true;return;
    }

    // A Unit barrier can read this descriptor again without any fleet write.
    // Readers consume each publication once, retaining later foreign AI edits.
    desc()[F2Phase::VERSION] = (desc()[F2Phase::VERSION] as u32).wrapping_add(1) as f64;
    desc()[F2Phase::FIRING] = firing::phase_ptr() as usize as f64;
    // The final world flush encodes the sample already observed by rounds.
    // A new native write still requires its ordinary observation first.
    let sampled=scheduler().publication_pending&&!scheduler().pending;
    desc()[F2Phase::SEATS] = live_seats::publish_sampled(sampled) as usize as f64;
    let p = publication();
    p.flush(&mut sim().store);
    desc()[F2Phase::HULLS] = p.hulls.ptr() as usize as f64;
    desc()[F2Phase::PADS] = p.pads.ptr() as usize as f64;
    desc()[F2Phase::LIVES] = p.lives.ptr() as usize as f64;
    desc()[F2Phase::BODIES] = veh_changes() as usize as f64;
    desc()[F2Phase::POSES] = geometry::publish() as usize as f64;
    desc()[F2Phase::ACC] = unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.acc) };
    desc()[F2Phase::FLIGHT_ACC] = unsafe { *addr_of_mut!(crate::abi::cold().fleet.flight_acc) };
    desc()[F2Phase::TIME] = unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.time) };
    scheduler().pending=false;scheduler().publication_pending=false;
}

/// Fresh pose output exists only after spawn rendering or the complete phase.
// Vehicle.aimTick/render already completed the retained rig. Publish compact
// camera and scalar pose changes at the pre-fire slot, allowing unit raycasts
// and synchronous fired listeners to read current state without JS rig math.
fn publish_posed() {
    publish();
}

/// Only pad clocks are changed here. The JS registry/weapons receive a life
/// notification; they never construct or step the hull's new mutable state.
// Vehicles.spawnSlots runs before latching. Pad choice and obstruction order
// are observable: keep pad order, first matching class, occupant exclusion and
// the delayed retry without resetting another hull's pending seat samples.
fn spawns(dt: f64,mut services:Option<&mut crate::units::f3::WorldServices<'_>>) {
    let descs = unsafe { &*addr_of_mut!(crate::abi::cold().fleet.scheduler.descs) };
    let header = F2Input::take(io(), &[]);
    let occupant = if crate::units::f3::active() {
        crate::units::f3::spawn_occupant()
    } else { header.occupant.as_ref().map(vec) };
    for (i, d) in descs.iter().enumerate() {
        let slot = &sim().store.vehicles.slots[i];
        let (vehicle, left) = (slot.vehicle, slot.remaining);
        if vehicle.and_then(|r| sim().store.vehicle.get(r)).is_some_and(|v| v.alive) { continue; }
        let remaining = v8math::max(0.0, left - dt);
        if remaining.to_bits() != sim().store.vehicles.slots[i].remaining.to_bits() { publication().pad(i); }
        sim().store.vehicles.slots[i].remaining = remaining;
        if remaining > 0.0 || vehicle.and_then(|r| sim().store.vehicle.get(r)).is_some_and(|v| v.dying > 0.0) { continue; }
        let at = header.pads as usize + i * F2PadTeam::WORDS;
        let pad_team = F2PadTeam::take(&io()[at..], &[]);
        let team = if pad_team.has_team { pad_team.team }
            else if d.team.is_some_and(super::super::hull::truthy) { d.team.unwrap() }
            else if d.classes.len() == 1 { bfe_content::js::string_to_number(&d.classes[0].0) } else { 0.0 };
        let names = unsafe { &(&*addr_of_mut!(crate::abi::cold().fleet.scheduler.pad_classes))[i] };
        let Some(&(_, name_id)) = names.iter().find(|(n, _)| *n == team || n.is_nan() && team.is_nan()) else { continue };
        let find = || sim().store.vehicles.slots[i].variants.entries.iter().find(|(n, _)| *n == name_id).map(|(_, r)| *r);
        let r = match find() {
            Some(r) => r,
            None => {
                publish();
                import_f2_variant(&F2Variant { pad: i as f64, name: name_id });
                find().expect("F2 construction must register its handle")
            }
        };
        if !sim().store.vehicles.vehicles.contains(&r) {
            activate(r);
            // Prepared policy follows the mixed-registry birth at this source
            // slot, before any later AI/seat query can observe the new hull.
            if let Some(ai)=services.as_mut().and_then(|services|services.ai.as_mut()) {ai.hull_born(r,crate::units::f3_hulls::member(r));}
        }
        let v = sim().store.vehicle.get(r).expect("F2 variant");
        let (center, size, radius, flying) = (v.center, v.size, v.radius, v.flying);
        let m = d.matrix;
        // Footprints use canonical hull matrices, including variants retained on
        // other pads. Do not test the scene's presented rig or rebuild collision
        // bounds through JS while deciding whether this life may spawn.
        let blocked = sim().store.vehicles.vehicles.iter().any(|&other| {
            if other == r { return false; }
            let b = sim().store.vehicle.get(other).expect("F2 hull");
            b.alive && super::super::spawn::footprints_overlap(&vec(&center), &vec(&size), &m, &vec(&b.center), &vec(&b.size), &b.matrix)
        });
        let near = occupant.is_some_and(|p| p.distance_to_squared(&Vector3::new(m[12], m[13], m[14])) < v8math::pow(radius + 1.0, 2.0));
        if blocked || near { sim().store.vehicles.slots[i].remaining = 1.0; publication().pad(i); continue; }
        sim().store.vehicles.slots[i].vehicle = Some(r);
        publication().pad(i);
        let index = sim().store.vehicles.vehicles.iter().position(|&x| x == r).unwrap();
        let mut hull = if flying { load_flyer(r, index) } else { load(r, index) };
        let mut p = poses()[r.slot as usize].take().expect("F2 retained rig");
        hull.pose = Some(p.pose);
        let row = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.hull_rows) }[r.slot as usize].as_ref().unwrap();
        hull.spawn_prepared(&mut JsWorld::new(u32::MAX),team,&m,row.flight_class,row.config);
        p.generation = hull.target_generation;
        p.pose = hull.pose.take().unwrap();
        poses()[r.slot as usize] = Some(p);
        // Ground spawn initializes flight fields too.
        store(&hull);
        let s = sim();
        let v = s.store.vehicle.get_mut(r).unwrap();
        if hull.flying {
            v.flight_class = Some(hull.flight.class);
            v.min_speed = Some(hull.flight.min_speed); v.mid_speed = Some(hull.flight.mid_speed);
            v.boost_speed = Some(hull.flight.boost_speed); v.speed = hull.speed; v.turn_rate = hull.turn_rate;
        }
        store_flight(&hull);
        // Spawn's render is observable by boarding in the following latch.
        geometry::mark(r);
        if crate::units::f3::active() {
            // Vehicle.spawn resets retained mounted rows before latching.
            // It must not rebuild JS weapons or reenter module exports here.
            firing::reset_life(r,sim().store.vehicle.get(r).unwrap().target_generation);
        } else { publish_posed(); }
        recycle(hull);
        if !crate::units::f3::active() { import_f2_life(&VehHull { hull: index as f64 }); }
    }
}

// Vehicles' insertion sequence is independent of slab slot reuse.
pub(crate) fn hull_reference(index: usize) -> Ref<Vehicle> {
    sim().store.vehicles.vehicles[index]
}

pub fn respawn_clock(hull: usize) {
    let r = sim().store.vehicles.vehicles[hull];
    let pad = sim().store.vehicle.get(r).unwrap().slot_index as usize;
    publication().pad(pad);
    sim().store.vehicles.slots[pad].remaining = unsafe { (&*addr_of_mut!(crate::abi::cold().fleet.scheduler.descs))[pad].respawn_time };
}

/// Consume only foreign state. Layout is deliberately separate from vehicle
/// rows, so adding a hull field cannot silently turn this into a full import.
// The input records contain retained registry changes and foreign control
// facts. They cannot overwrite physics, lease or weapon ownership. Reset lazy
// aim-query scratch for this phase without allocating a new registry vector.
fn foreign() {
    let h = F2Input::take(io(), &[]);
    if !crate::units::f3::active() {
    let members = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.members) };
    members.resize(h.member_length as usize, MemberRow { kind: 0, id: 0.0, generation: None, alive: false, team: None, hull: u32::MAX, point: Vector3::default() });
    for i in 0..h.member_count as usize {
        let a = F2Member::take(&io()[h.members as usize + i * F2Member::WORDS..], &[]);
        members[a.index as usize] = MemberRow { kind: a.kind as u8, id: a.id, generation: (!a.generation.is_nan()).then_some(a.generation), alive: a.alive,
            team: team(if a.has_team { 1.0 } else { 0.0 }, a.team), hull: a.hull as u32, point: vec(&a.point) };
    }
    // Candidate storage belongs to fleet/registry construction, even when a
    // craft stays empty for many ticks before its first pilot takes control.
    for row in unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.scheduler.hull_rows) }.iter_mut().flatten() {
        for hull in &mut row.pool { if hull.flying { hull.target_scratch.reserve(h.member_length as usize); } }
    }
    let wanted = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.aim_wanted) };
    if wanted.capacity() < h.member_length as usize { wanted.reserve((h.member_length as usize).saturating_sub(wanted.len())); }
    live_seats::reserve_riders(h.member_length as usize);
    // FlightWorld caches canonical Unit aim points even when the foreign
    // member block is empty. Its capacity was reserved at registry creation.
    let count=if crate::units::f3::active() { sim().store.units.list.len() } else { h.member_length as usize };
    let cache = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.aim_cache) };
    // A later native birth can be observed before the next fleet input.
    // Retain the admitted future range as well as the current registry.
    cache.resize(count.max(cache.len()), None); cache.fill(None);
    let teams = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.teams) }; teams.clear();
    for i in 0..h.team_count as usize { let t = F2PadTeam::take(&io()[h.teams as usize + i * F2PadTeam::WORDS..], &[]); teams.push(team(if t.has_team { 1.0 } else { 0.0 }, t.team)); }
    let rel = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.relations) }; rel.clear();
    rel.extend_from_slice(&io()[h.relations as usize..h.relations as usize + teams.len() * teams.len()]);
    } else {
        unsafe { (&mut *addr_of_mut!(crate::abi::cold().fleet.aim_cache)).fill(None); }
    }
    let given = unsafe { &mut *addr_of_mut!(crate::abi::cold().fleet.flight_inputs) };
    for i in 0..h.flight_count as usize {
        let a = F2FlightIn::take(&io()[h.flights as usize + i * F2FlightIn::WORDS..], &[]);
        if given.len() <= a.hull as usize { given.resize(a.hull as usize + 1, None); }
        let k = a.remote;
        given[a.hull as usize] = Some((idle_input(), a.network.then_some(FlightInput {
            forward: k.forward, right: k.right, turn: k.turn, pitch: k.pitch, up: k.up, down: k.down, takeoff: k.takeoff,
            stick_x: k.has_x.then_some(k.stick_x), stick_y: k.has_y.then_some(k.stick_y), boost: k.boost, trick: k.trick, immelmann: k.immelmann,
            roll_lock: k.roll_lock, target_lock: k.target_lock, bot: k.bot, response_curve: k.response_curve, no_assist: k.no_assist,
        })));
    }
    let n = firing::inputs(&io()[h.firing as usize..h.end as usize]);
    assert_eq!(h.firing as usize + n, h.end as usize, "F2 foreign input length");
}

#[no_mangle]
// Vehicles.update and the adjacent TurretSeats slot retain their ordering.
// All aim passes precede gameplay rendering, then hull fire/tow, rider body
// writeback and the interleaved turret crew phase. Effects cannot move a
// state-changing hook past a later read in that sequence.
pub extern "C" fn f2_step(dt: f64, tick: f64) {
    // Vehicles.writeRiders consumes mounted controls too. Keep this guard
    // outside the publication guard so its final mirrors include those edges.
    let _controls = crate::units::f3::control_span(true);
    let _external = crate::units::f3::external();
    crate::units::f3::with_services(|services| step(dt, tick, services));
}

// Vehicles.update lends its authority to hull, mounted and crew work. The
// outer unit coordinator calls this directly with the borrow it already owns.
pub(crate) fn step(dt: f64, tick: f64, mut services: Option<crate::units::f3::WorldServices<'_>>) {
    assert!(active(), "F2 must be initialized");
    let dt = three::clamp(dt, 0.0, 0.1);
    unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.time) += dt; *addr_of_mut!(crate::abi::cold().fleet.clock) = Some(io()[F2Input::CLOCK]); }
    allocation::begin();
    if services.is_none() { sim().events.clear(); }
    live_seats::import_phase_input(services.as_ref().is_some_and(|services|services.ai.is_some()));
    { let _scope = allocation::enter(allocation::LIFECYCLE); spawns(dt,services.as_mut()); }
    allocation::stage(allocation::Stage::Foreign);
    foreign();
    allocation::stage(allocation::Stage::Latch);
    live_seats::latch(unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.time) }, services.as_mut().map(crate::units::f3::WorldServices::reborrow));
    let flags = io()[F2Input::FLAGS] as u32 & (FLAG_POSES | FLAG_COSMETIC | FLAG_FLIGHT | FLAG_ASSISTS);
    allocation::stage(allocation::Stage::Hulls);
    let left = ground(dt, unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.acc) }, unsafe { *addr_of_mut!(crate::abi::cold().fleet.flight_acc) }, io()[F2Input::LEVEL], io()[F2Input::CLOCK], flags, services.as_mut().map(crate::units::f3::WorldServices::reborrow));
    unsafe { *addr_of_mut!(crate::abi::cold().fleet.scheduler.acc) = left; }
    desc()[F2Phase::TICK] = tick;
    allocation::stage(allocation::Stage::Publish);
    firing::flight_locks();
    if services.is_some() {
        // Vehicle.poseRider samples exposed geometry before mounted fire.
        // Its later syncCrews body write advances tickCount separately.
        for i in 0..sim().store.vehicles.vehicles.len() { riders::hull(sim().store.vehicles.vehicles[i], false); }
    } else {
        publish_posed();
        import_f2_posed(&VehNothing {});
    }
    allocation::stage(allocation::Stage::Fire);
    firing::step(dt, io()[F2Input::CLOCK], services.as_mut().map(crate::units::f3::WorldServices::reborrow));
    // fireCrews changed weapons and cables. Publish those writes before the
    // presentation callback, replacing the already-consumed pose publication.
    if services.is_none() {
        publish();
        import_f2_foreign(&VehNumber { value: dt });
    }
    allocation::stage(allocation::Stage::Riders);
    for i in 0..sim().store.vehicles.vehicles.len() { riders::hull(sim().store.vehicles.vehicles[i], true); }
    allocation::stage(allocation::Stage::Turrets);
    live_seats::turret_tick(dt, services);
    allocation::stage(allocation::Stage::Publish);
    publish();
    allocation::stage(allocation::Stage::Finish);
    live_seats::finish();
    allocation::end();
}

/// Synchronous authoritative writes. Both slab generation and gameplay life
/// are checked; no old reference can write into a replacement's state.
#[no_mangle]
// Vehicle.damage/setHealth/wake/attribution validate the command before taking
// a working row. Reentrant hooks may retire it, in which case the outer write
// must not recreate the slot or publish into a replacement's life.
pub extern "C" fn f2_command(slot: u32, generation: u32, life: f64, code: u32, value: f64, tag: u32, owner_slot: u32, owner_gen: u32) -> u32 {
    let r = Ref::new(slot, generation);
    let Some(v) = sim().store.vehicle.get(r) else { return 0 };
    if v.target_generation != life { return 0; }
    if code > 3 || tag > 4 { return 0; }
    let flying = v.flying;
    let owner = match tag { 0 => OrdnanceOwner::Null, 1 => OrdnanceOwner::Player, 2 => OrdnanceOwner::Unit(Ref::new(owner_slot, owner_gen)), 3 => OrdnanceOwner::Vehicle(Ref::new(owner_slot, owner_gen)), 4 => OrdnanceOwner::OwnerSnapshot(Ref::new(owner_slot, owner_gen)), _ => return 0 };
    if crate::units::f3::active() && matches!(F2HullOp::from_u8(code as u8).unwrap(), F2HullOp::Damage | F2HullOp::Health) {
        // Vehicle.damage between phases joins the same native impact service
        // as an instant round. A recursive crew death cannot call a JS adapter.
        let _external = crate::units::f3::external();
        crate::units::f3::with_services(|services| crate::units::f3_hulls::mutate(r, value,
            (code == F2HullOp::Damage as u32).then_some(owner), services.expect("native world services")));
        publish();
        return 1;
    }
    let index = sim().store.vehicles.vehicles.iter().position(|&x| x == r).expect("F2 ordered hull");
    let mut hull = if flying { load_flyer(r, index) } else { load(r, index) };
    let mut world = JsWorld::new(u32::MAX);
    match F2HullOp::from_u8(code as u8).unwrap() {
        F2HullOp::Damage => hull.damage(&mut world, value, owner),
        F2HullOp::Health => hull.set_health(&mut world, value),
        F2HullOp::Wake => hull.sleeping = Some(false),
        F2HullOp::Attacker => hull.last_attacker = owner,
    }
    // A callback may have retired the slot; never resurrect it.
    if !sim().store.vehicle.contains(r) { return 0; }
    store_hull(&hull);
    recycle(hull);
    publish();
    1
}

#[no_mangle]
// Vehicle retirement disables collision and releases seats, weapons and cables
// while the old handle still names its final rows. Remove retained pose data
// before the slab can reuse the slot for another vehicle.
pub extern "C" fn f2_retire(slot: u32, generation: u32) -> u32 {
    let r = Ref::new(slot, generation);
    let Some(v) = sim().store.vehicle.get(r) else { return 0 };
    let body = v.body.slot;
    JsWorld::new(u32::MAX).enable_body(body, false);
    let v = sim().store.vehicle.get_mut(r).unwrap();
    v.alive = false; v.visible = false;
    // Commit the old registry target while this generation still resolves.
    if crate::units::f3::active() { crate::units::f3_hulls::refresh(v.target_id as u32); }
    firing::retire(r);
    live_seats::retire(r);
    // A retired registry member outlives its hull slot. Commit its dead facts
    // before releasing the generation; a later query must not retain alive.
    if crate::units::f3::active() { crate::units::f3_hulls::refresh(sim().store.vehicle.get(r).unwrap().target_id as u32); }
    crate::battle::f4_tail::refresh_mobile_views();
    if !sim().store.vehicle.retire(r) { return 0; }
    sim().store.vehicles.vehicles.retain(|&x| x != r);
    for (index, pad) in sim().store.vehicles.slots.iter_mut().enumerate() {
        if pad.vehicle == Some(r) { publication().pad(index); }
        if pad.vehicle == Some(r) { pad.vehicle = None; }
        pad.variants.entries.retain(|(_, v)| *v != r);
    }
    if let Some(p) = poses().get_mut(slot as usize) { *p = None; }
    // The last pose belongs to its retired life and cannot be reused.
    unsafe { (*addr_of_mut!(crate::abi::cold().fleet.locks)).clear(); }
    publish();
    1
}

// lockPick asks for identity only in its retained-target pass. Keep that
// observation separate from expensive lazy hit-shape and sight capabilities.
fn target_count() -> usize {
    if crate::units::f3::active() { crate::units::f3_fleet::count() }
    else { unsafe { (&*addr_of_mut!(crate::abi::cold().fleet.members)).len() } }
}
fn target_identity(index: usize) -> (f64, f64) {
    if crate::units::f3::active() {
        let key = crate::units::f3_fleet::key(index);
        (crate::units::registry::target_id(key) as f64, crate::units::f3_fleet::life(key))
    } else {
        let m = unsafe { (&*addr_of_mut!(crate::abi::cold().fleet.members))[index] };
        (m.id, m.generation.unwrap_or(0.0))
    }
}

impl Scheduler {
    fn has_pending(&self) -> bool { self.active && self.pending }
    fn flight_keys_for(&self, r: Ref<Vehicle>) -> (Config, Option<GearClip>) {
        let row = self.hull_rows[r.slot as usize].as_ref().unwrap(); (row.config, row.gear.clone())
    }
    fn store_flight(&self, store: &mut Store, hull: &Hull) {
        let texts = &self.hull_rows[hull.id.slot as usize].as_ref().unwrap().texts;
        hull.flight.to_store_cached((*store).vehicle.get_mut(hull.id).unwrap(),texts);
    }
    fn take_hull(&mut self, store: &Store, r: Ref<Vehicle>, index: usize) -> Hull {
        let _allocation = allocation::nested(allocation::Stage::Load);
        let row = self.hull_rows[r.slot as usize].as_mut().unwrap();
        let mut hull = row.pool.pop().expect("reserved reentrant hull row");
        hull.index = index;
        if hull.f2_revision.get() != row.revision {
            hull.load(store.vehicle.get(r).unwrap()); hull.f2_revision.set(row.revision);
        }
        hull
    }
    fn prepare_leg_store(&mut self, hull: &Hull, vehicle: &mut Vehicle) {
        let spare = &mut self.hull_rows[hull.id.slot as usize].as_mut().unwrap().leg_side;
        if hull.leg_side.is_some() && vehicle.leg_side.is_none() {
            vehicle.leg_side = Some(std::mem::take(spare));
        } else if hull.leg_side.is_none() {
            if let Some(mut old) = vehicle.leg_side.take() { old.clear(); *spare = old; }
        }
    }
    fn recycle(&mut self, hull: Hull) { self.hull_rows[hull.id.slot as usize].as_mut().unwrap().pool.push(hull); }
    fn mark(&mut self, geometry: &mut geometry::Geometry, r: Ref<Vehicle>) {
        self.pending = true;
        if let Some(Some(row)) = self.hull_rows.get_mut(r.slot as usize) { row.revision += 1; }
        self.publication.get_or_insert_with(Default::default).mark(r); geometry.mark(r);
    }
    fn stored(&mut self, geometry: &mut geometry::Geometry, hull: &Hull) {
        self.pending = true;
        let row = self.hull_rows[hull.id.slot as usize].as_mut().unwrap();
        row.revision += 1; hull.f2_revision.set(row.revision);
        self.publication.get_or_insert_with(Default::default).mark(hull.id); geometry.mark(hull.id);
    }
    fn revision(&self, r: Ref<Vehicle>) -> u64 {
        self.hull_rows[r.slot as usize].as_ref().expect("registered target hull").revision
    }
}
