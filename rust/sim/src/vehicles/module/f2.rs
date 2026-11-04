//! Persistent fleet scheduler. The generated slab is canonical; input contains
//! foreign facts only. No complete vehicle row is imported by an ordinary tick.
pub use crate::vehicles::f2_seats as seats;
pub mod live_seats;
pub mod firing;
mod allocation;
pub mod riders;
mod geometry;
pub use crate::vehicles::f2_delta as delta;

use super::*;

const CAPACITY: usize = 262144;
static mut ACTIVE: bool = false;
static mut IO: Vec<f64> = Vec::new();
static mut DESCRIPTOR: [f64; F2Phase::WORDS] = [0.0; F2Phase::WORDS];
static mut PUBLICATION: Option<crate::vehicles::f2_publication::Publication> = None;
static mut DESCS: Vec<super::super::spawn::SlotDesc> = Vec::new();
static mut PAD_CLASSES: Vec<Vec<(f64, usize, StrId)>> = Vec::new();
const FLAG_POSES: u32 = 1;
const FLAG_COSMETIC: u32 = 2;
const FLAG_FLIGHT: u32 = 4;
const FLAG_ASSISTS: u32 = 8;
static mut ACC: f64 = 0.0;
static mut TIME: f64 = 0.0;
struct HullRow { revision: u64, pool: Vec<Hull>, config: Config, gear: Option<GearClip> }
static mut HULL_ROWS: Vec<Option<HullRow>> = Vec::new();
fn hull_rows() -> &'static mut Vec<Option<HullRow>> { unsafe { &mut *addr_of_mut!(HULL_ROWS) } }

/// Vehicle construction resolves its class and flight keys once. Spare working
/// rows let a synchronous damage hook reenter without borrowing the outer hull.
fn register_hull(r: Ref<Vehicle>) {
    let s = sim(); let v = s.store.vehicle.get(r).unwrap();
    let name = String::from_utf16_lossy(s.strings.units(v.name));
    let class = classes().get(&name).expect("registered hull class");
    let (config, gear) = if v.flying { flight_keys(&name) } else { (Config::default(), None) };
    let mut pool = Vec::with_capacity(4);
    for _ in 0..4 { pool.push(Hull::from_store(r, v, class, 0, &s.strings)); }
    let rows = hull_rows();
    if rows.len() <= r.slot as usize { rows.resize_with(r.slot as usize + 1, || None); }
    rows[r.slot as usize] = Some(HullRow { revision: 1, pool, config, gear });
}
pub fn flight_keys_for(r: Ref<Vehicle>) -> (Config, Option<GearClip>) {
    let row = hull_rows()[r.slot as usize].as_ref().unwrap(); (row.config, row.gear.clone())
}
pub fn take_hull(r: Ref<Vehicle>, index: usize) -> Hull {
    let row = hull_rows()[r.slot as usize].as_mut().unwrap();
    let mut hull = row.pool.pop().expect("reserved reentrant hull row");
    hull.index = index;
    if hull.f2_revision.get() != row.revision {
        let s = sim(); hull.load(s.store.vehicle.get(r).unwrap(), &s.strings); hull.f2_revision.set(row.revision);
    }
    hull
}
pub fn recycle(hull: Hull) { hull_rows()[hull.id.slot as usize].as_mut().unwrap().pool.push(hull); }


pub fn active() -> bool { unsafe { *addr_of_mut!(ACTIVE) } }
fn io() -> &'static mut Vec<f64> { unsafe { &mut *addr_of_mut!(IO) } }
fn publication() -> &'static mut crate::vehicles::f2_publication::Publication {
    unsafe { (*addr_of_mut!(PUBLICATION)).get_or_insert_with(Default::default) }
}

pub fn mark(r: Ref<Vehicle>) {
    if let Some(Some(row)) = hull_rows().get_mut(r.slot as usize) { row.revision += 1; }
    publication().mark(r); geometry::mark(r);
}
/// A completed native substep already has the canonical dynamic fields. Keep
/// that working row current; other spare rows reload only after a real write.
pub fn stored(hull: &Hull) {
    let row = hull_rows()[hull.id.slot as usize].as_mut().unwrap();
    row.revision += 1; hull.f2_revision.set(row.revision);
    publication().mark(hull.id); geometry::mark(hull.id);
}
pub fn mark_changed(r: Ref<Vehicle>) { mark(r); }

/// Vehicle construction registers its row without rewriting other pads or hulls.
#[no_mangle]
pub extern "C" fn f2_register(slot: u32, generation: u32) {
    let r = Ref::new(slot, generation);
    let s = sim();
    let v = s.store.vehicle.get(r).expect("registered F2 hull");
    let pad = v.slot_index as usize;
    let name = v.name;
    if !s.store.vehicles.vehicles.contains(&r) { s.store.vehicles.vehicles.push(r); }
    if !s.store.vehicles.slots[pad].variants.entries.iter().any(|&(n, _)| n == name) {
        s.store.vehicles.slots[pad].variants.entries.push((name, r));
    }
    publication().register(&s.store, r);
    register_hull(r);
}

fn desc() -> &'static mut [f64; F2Phase::WORDS] { unsafe { &mut *addr_of_mut!(DESCRIPTOR) } }

/// Allocate once while loading. The stable descriptor points at input and at
/// each publication; output blocks may move, so their pointers are republished.
#[no_mangle]
pub extern "C" fn f2_init(json: u32, time: f64, acc: f64) -> *const f64 {
    let text = String::from_utf16_lossy(sim().strings.units(StrId(json)));
    let value = bfe_content::json::parse(&text).expect("F2 pad descriptions");
    let ds: Vec<_> = value.as_array().expect("F2 pads").iter().map(super::super::spawn::SlotDesc::of).collect();
    let names = ds.iter().map(|d| d.classes.iter().enumerate().filter_map(|(i, (key, name))| {
        let team = bfe_content::js::string_to_number(key);
        (bfe_content::js::number_to_string(team) == *key && !name.is_empty()).then(|| (team, i, sim().strings.intern_str(name)))
    }).collect()).collect();
    unsafe {
        *addr_of_mut!(PAD_CLASSES) = names;
        *addr_of_mut!(DESCS) = ds;
        *addr_of_mut!(ACTIVE) = true;
        *addr_of_mut!(TIME) = time;
        *addr_of_mut!(ACC) = acc;
    }
    io().resize(CAPACITY, 0.0);
    sim().events.reserve(262144, FfShot::WORDS, 1024);
    desc()[F2Phase::INPUT] = io().as_mut_ptr() as usize as f64;
    desc()[F2Phase::CAPACITY] = CAPACITY as f64;
    desc().as_ptr()
}

/// Publish without a getter crossing from JS, also before a synchronous hook.
/// No reference to the store survives a foreign call.
pub fn publish() {
    desc()[F2Phase::FIRING] = firing::phase_ptr() as usize as f64;
    desc()[F2Phase::SEATS] = live_seats::publish() as usize as f64;
    let p = publication();
    p.flush(&mut sim().store);
    desc()[F2Phase::HULLS] = p.hulls.ptr() as usize as f64;
    desc()[F2Phase::PADS] = p.pads.ptr() as usize as f64;
    desc()[F2Phase::LIVES] = p.lives.ptr() as usize as f64;
    desc()[F2Phase::BODIES] = veh_changes() as usize as f64;
    desc()[F2Phase::POSES] = geometry::publish() as usize as f64;
    desc()[F2Phase::LOCKS] = 0.0;
    desc()[F2Phase::ACC] = unsafe { *addr_of_mut!(ACC) };
    desc()[F2Phase::FLIGHT_ACC] = unsafe { *addr_of_mut!(FLIGHT_ACC) };
    desc()[F2Phase::TIME] = unsafe { *addr_of_mut!(TIME) };
}

/// Fresh pose output exists only after spawn rendering or the complete phase.
fn publish_posed() {
    publish();
    desc()[F2Phase::LOCKS] = unsafe { (&*addr_of_mut!(LOCKS)).first().map_or(0.0, |_| (&*addr_of_mut!(LOCKS)).as_ptr() as usize as f64) };
}

/// Only pad clocks are changed here. The JS registry/weapons receive a life
/// notification; they never construct or step the hull's new mutable state.
fn spawns(dt: f64) {
    let descs = unsafe { &*addr_of_mut!(DESCS) };
    let header = F2Input::take(io(), &[]);
    let occupant = header.occupant.as_ref().map(vec);
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
        let names = unsafe { &(&*addr_of_mut!(PAD_CLASSES))[i] };
        let Some(&(_, class, name_id)) = names.iter().find(|(n, _, _)| *n == team || n.is_nan() && team.is_nan()) else { continue };
        let name = &d.classes[class].1;
        let find = || sim().store.vehicles.slots[i].variants.entries.iter().find(|(n, _)| *n == name_id).map(|(_, r)| *r);
        let r = match find() {
            Some(r) => r,
            None => {
                publish();
                import_f2_variant(&F2Variant { pad: i as f64, name: name_id });
                find().expect("F2 construction must register its handle")
            }
        };
        let v = sim().store.vehicle.get(r).expect("F2 variant");
        let (center, size, radius, flying) = (v.center, v.size, v.radius, v.flying);
        let m = d.matrix;
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
        let def = unsafe { (*addr_of_mut!(DEFS)).as_ref().unwrap().get(name).unwrap() };
        hull.spawn(&mut JsWorld { craft: u32::MAX }, team, &m, def, name);
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
        hull.flight.to_store(v, &mut s.strings);
        // Spawn's render is observable by boarding in the following latch.
        let out = unsafe { &mut *addr_of_mut!(POSED) };
        if out.is_empty() { out.push(0.0); }
        out[0] += 1.0; out.push(r.slot as f64);
        let length = out.len(); out.push(0.0); let start = out.len();
        geometry::mark(r);
        out[length] = (out.len() - start) as f64;
        publish_posed();
        recycle(hull);
        import_f2_life(&VehHull { hull: index as f64 });
    }
}

pub fn respawn_clock(hull: usize) {
    let r = hull_refs()[hull];
    let pad = sim().store.vehicle.get(r).unwrap().slot_index as usize;
    publication().pad(pad);
    sim().store.vehicles.slots[pad].remaining = unsafe { (&*addr_of_mut!(DESCS))[pad].respawn_time };
}

/// Consume only foreign state. Layout is deliberately separate from vehicle
/// rows, so adding a hull field cannot silently turn this into a full import.
fn foreign() {
    let h = F2Input::take(io(), &[]);
    let members = unsafe { &mut *addr_of_mut!(MEMBERS) };
    members.resize(h.member_length as usize, MemberRow { kind: 0, id: 0.0, generation: None, alive: false, team: None, hull: u32::MAX, point: Vector3::default() });
    for i in 0..h.member_count as usize {
        let a = F2Member::take(&io()[h.members as usize + i * F2Member::WORDS..], &[]);
        members[a.index as usize] = MemberRow { kind: a.kind as u8, id: a.id, generation: (!a.generation.is_nan()).then_some(a.generation), alive: a.alive,
            team: team(if a.has_team { 1.0 } else { 0.0 }, a.team), hull: a.hull as u32, point: vec(&a.point) };
    }
    let cache = unsafe { &mut *addr_of_mut!(AIM_CACHE) }; cache.resize(h.member_length as usize, None); cache.fill(None);
    let teams = unsafe { &mut *addr_of_mut!(TEAMS) }; teams.clear();
    for i in 0..h.team_count as usize { let t = F2PadTeam::take(&io()[h.teams as usize + i * F2PadTeam::WORDS..], &[]); teams.push(team(if t.has_team { 1.0 } else { 0.0 }, t.team)); }
    let rel = unsafe { &mut *addr_of_mut!(RELATIONS) }; rel.clear();
    rel.extend_from_slice(&io()[h.relations as usize..h.relations as usize + teams.len() * teams.len()]);
    let given = unsafe { &mut *addr_of_mut!(FLIGHT_INPUTS) };
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
pub extern "C" fn f2_step(dt: f64, tick: f64) {
    assert!(active(), "F2 must be initialized");
    let dt = three::clamp(dt, 0.0, 0.1);
    unsafe { *addr_of_mut!(TIME) += dt; *addr_of_mut!(CLOCK) = Some(io()[F2Input::CLOCK]); }
    allocation::begin();
    sim().events.clear();
    spawns(dt);
    foreign();
    live_seats::latch(unsafe { *addr_of_mut!(TIME) });
    let flags = io()[F2Input::FLAGS] as u32 & (FLAG_POSES | FLAG_COSMETIC | FLAG_FLIGHT | FLAG_ASSISTS);
    let left = veh_ground(dt, unsafe { *addr_of_mut!(ACC) }, unsafe { *addr_of_mut!(FLIGHT_ACC) }, io()[F2Input::LEVEL], io()[F2Input::CLOCK], flags);
    unsafe { *addr_of_mut!(ACC) = left; }
    desc()[F2Phase::TICK] = tick;
    publish_posed();
    import_f2_posed(&VehNothing {});
    firing::step(dt, io()[F2Input::CLOCK]);
    // fireCrews changed weapons and cables. Publish those writes before the
    // presentation callback, replacing the already-consumed pose publication.
    publish();
    import_f2_foreign(&VehNumber { value: dt });
    for i in 0..sim().store.vehicles.vehicles.len() { riders::hull(sim().store.vehicles.vehicles[i], true); }
    live_seats::turret_tick(dt);
    publish();
    live_seats::finish();
    allocation::end();
}

/// Synchronous authoritative writes. Both slab generation and gameplay life
/// are checked; no old reference can write into a replacement's state.
#[no_mangle]
pub extern "C" fn f2_command(slot: u32, generation: u32, life: f64, code: u32, value: f64, tag: u32, owner_slot: u32, owner_gen: u32) -> u32 {
    let r = Ref::new(slot, generation);
    let Some(v) = sim().store.vehicle.get(r) else { return 0 };
    if v.target_generation != life { return 0; }
    let flying = v.flying;
    let index = sim().store.vehicles.vehicles.iter().position(|&x| x == r).expect("F2 ordered hull");
    let mut hull = if flying { load_flyer(r, index) } else { load(r, index) };
    let owner = match tag { 0 => OrdnanceOwner::Null, 1 => OrdnanceOwner::Player, 2 => OrdnanceOwner::Unit(Ref::new(owner_slot, owner_gen)), 3 => OrdnanceOwner::Vehicle(Ref::new(owner_slot, owner_gen)), 4 => OrdnanceOwner::OwnerSnapshot(Ref::new(owner_slot, owner_gen)), _ => return 0 };
    let mut world = JsWorld { craft: u32::MAX };
    match code {
        0 => hull.damage(&mut world, value, owner),
        1 => hull.set_health(&mut world, value),
        2 => hull.sleeping = Some(false),
        3 => hull.last_attacker = owner,
        _ => return 0,
    }
    // A callback may have retired the slot; never resurrect it.
    if !sim().store.vehicle.contains(r) { return 0; }
    store_hull(&hull);
    recycle(hull);
    publish();
    1
}

#[no_mangle]
pub extern "C" fn f2_retire(slot: u32, generation: u32) -> u32 {
    let r = Ref::new(slot, generation);
    let Some(v) = sim().store.vehicle.get(r) else { return 0 };
    let body = v.body.slot;
    JsWorld { craft: u32::MAX }.enable_body(body, false);
    let v = sim().store.vehicle.get_mut(r).unwrap();
    v.alive = false; v.visible = false;
    firing::retire(r);
    live_seats::retire(r);
    if !sim().store.vehicle.retire(r) { return 0; }
    sim().store.vehicles.vehicles.retain(|&x| x != r);
    for (index, pad) in sim().store.vehicles.slots.iter_mut().enumerate() {
        if pad.vehicle == Some(r) { publication().pad(index); }
        if pad.vehicle == Some(r) { pad.vehicle = None; }
        pad.variants.entries.retain(|(_, v)| *v != r);
    }
    if let Some(p) = poses().get_mut(slot as usize) { *p = None; }
    // The last pose belongs to its retired life and cannot be reused.
    unsafe { (*addr_of_mut!(POSED)).clear(); (*addr_of_mut!(LOCKS)).clear(); }
    publish();
    1
}
