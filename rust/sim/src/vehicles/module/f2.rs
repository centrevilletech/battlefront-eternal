//! Persistent fleet scheduler. The generated slab is canonical; input contains
//! foreign facts only. No complete vehicle row is imported by an ordinary tick.
pub use crate::vehicles::f2_seats as seats;
pub mod live_seats;

use super::*;

const CAPACITY: usize = 262144;
static mut ACTIVE: bool = false;
static mut IO: Vec<f64> = Vec::new();
static mut DESCRIPTOR: [f64; 16] = [0.0; 16];
static mut PUBLICATION: Option<crate::vehicles::f2_publication::Publication> = None;
static mut DESCS: Vec<super::super::spawn::SlotDesc> = Vec::new();
static mut ACC: f64 = 0.0;
static mut TIME: f64 = 0.0;

pub fn active() -> bool { unsafe { *addr_of_mut!(ACTIVE) } }
fn io() -> &'static mut Vec<f64> { unsafe { &mut *addr_of_mut!(IO) } }
fn publication() -> &'static mut crate::vehicles::f2_publication::Publication {
    unsafe { (*addr_of_mut!(PUBLICATION)).get_or_insert_with(Default::default) }
}

pub fn mark(r: Ref<Vehicle>) { publication().mark(r); }

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
}

fn desc() -> &'static mut [f64; 16] { unsafe { &mut *addr_of_mut!(DESCRIPTOR) } }

/// Allocate once while loading. The stable descriptor points at input and at
/// each publication; output blocks may move, so their pointers are republished.
#[no_mangle]
pub extern "C" fn f2_init(json: u32, time: f64, acc: f64) -> *const f64 {
    let text = String::from_utf16_lossy(sim().strings.units(StrId(json)));
    let value = bfe_content::json::parse(&text).expect("F2 pad descriptions");
    let ds = value.as_array().expect("F2 pads").iter().map(super::super::spawn::SlotDesc::of).collect();
    unsafe {
        *addr_of_mut!(DESCS) = ds;
        *addr_of_mut!(ACTIVE) = true;
        *addr_of_mut!(TIME) = time;
        *addr_of_mut!(ACC) = acc;
    }
    io().resize(CAPACITY, 0.0);
    desc()[0] = io().as_mut_ptr() as usize as f64;
    desc()[1] = CAPACITY as f64;
    desc().as_ptr()
}

/// Publish without a getter crossing from JS, also before a synchronous hook.
/// No reference to the store survives a foreign call.
pub fn publish() {
    desc()[13] = live_seats::publish() as usize as f64;
    let p = publication();
    p.flush(&mut sim().store);
    desc()[2] = p.hulls.ptr() as usize as f64;
    desc()[3] = p.pads.ptr() as usize as f64;
    desc()[11] = p.lives.ptr() as usize as f64;
    desc()[12] = 0.0;
    desc()[4] = veh_changes() as usize as f64;
    desc()[5] = 0.0;
    desc()[6] = 0.0;
    desc()[7] = unsafe { *addr_of_mut!(ACC) };
    desc()[8] = unsafe { *addr_of_mut!(FLIGHT_ACC) };
    desc()[9] = unsafe { *addr_of_mut!(TIME) };
}

/// Fresh pose output exists only after spawn rendering or the complete phase.
fn publish_posed() {
    live_seats::posed_nodes();
    publish();
    desc()[5] = unsafe { (&*addr_of_mut!(POSED)).first().map_or(0.0, |_| (&*addr_of_mut!(POSED)).as_ptr() as usize as f64) };
    desc()[6] = unsafe { (&*addr_of_mut!(LOCKS)).first().map_or(0.0, |_| (&*addr_of_mut!(LOCKS)).as_ptr() as usize as f64) };
}

/// Only pad clocks are changed here. The JS registry/weapons receive a life
/// notification; they never construct or step the hull's new mutable state.
fn spawns(dt: f64) {
    let descs = unsafe { (&*addr_of_mut!(DESCS)).clone() };
    // Input prefix: clock, level, flags, pad team pairs, occupant flag/xyz.
    let teams = io()[3..3 + descs.len() * 2].to_vec();
    let o = 3 + descs.len() * 2;
    let occupant = (io()[o] != 0.0).then(|| Vector3::new(io()[o + 1], io()[o + 2], io()[o + 3]));
    for (i, d) in descs.iter().enumerate() {
        let slot = sim().store.vehicles.slots[i].clone();
        if slot.vehicle.and_then(|r| sim().store.vehicle.get(r)).is_some_and(|v| v.alive) { continue; }
        let remaining = v8math::max(0.0, slot.remaining - dt);
        if remaining.to_bits() != sim().store.vehicles.slots[i].remaining.to_bits() { publication().pad(i); }
        sim().store.vehicles.slots[i].remaining = remaining;
        if remaining > 0.0 || slot.vehicle.and_then(|r| sim().store.vehicle.get(r)).is_some_and(|v| v.dying > 0.0) { continue; }
        let team = if teams[i * 2] != 0.0 { teams[i * 2 + 1] }
            else if d.team.is_some_and(super::super::hull::truthy) { d.team.unwrap() }
            else if d.classes.len() == 1 { bfe_content::js::string_to_number(&d.classes[0].0) } else { 0.0 };
        let Some(name) = d.class_for(team).map(str::to_owned) else { continue };
        let name_id = sim().strings.intern_str(&name);
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
        let blocked = hull_refs().iter().any(|&other| {
            if other == r { return false; }
            let b = sim().store.vehicle.get(other).expect("F2 hull");
            b.alive && super::super::spawn::footprints_overlap(&vec(&center), &vec(&size), &m, &vec(&b.center), &vec(&b.size), &b.matrix)
        });
        let near = occupant.is_some_and(|p| p.distance_to_squared(&Vector3::new(m[12], m[13], m[14])) < v8math::pow(radius + 1.0, 2.0));
        if blocked || near { sim().store.vehicles.slots[i].remaining = 1.0; publication().pad(i); continue; }
        sim().store.vehicles.slots[i].vehicle = Some(r);
        publication().pad(i);
        let index = hull_refs().iter().position(|&x| x == r).unwrap();
        let mut hull = if flying { load_flyer(r, index) } else { load(r, index) };
        let mut p = poses()[r.slot as usize].take().expect("F2 retained rig");
        hull.pose = Some(p.pose);
        let def = unsafe { (*addr_of_mut!(DEFS)).as_ref().unwrap().get(&name).unwrap().clone() };
        hull.spawn(&mut JsWorld { craft: u32::MAX }, team, &m, &def, &name);
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
        let mut numbers = Vec::new();
        posing::out(&mut poses()[r.slot as usize].as_mut().unwrap().pose, &mut numbers);
        let out = unsafe { &mut *addr_of_mut!(POSED) };
        if out.is_empty() { out.push(0.0); }
        out[0] += 1.0;
        out.extend_from_slice(&[r.slot as f64, numbers.len() as f64]);
        out.extend_from_slice(&numbers);
        publish_posed();
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
    let start = io()[CAPACITY - 1] as usize;
    let end = io()[CAPACITY - 2] as usize;
    let data = &io()[start..end];
    let mut o = 0;
    let mut section = |stride: usize| {
        let n = data[o] as usize; o += 1;
        let end = o + n * stride;
        input().clear(); input().extend_from_slice(&data[o..end]); o = end;
        n as u32
    };
    let n = section(10); veh_members(n);
    let n = data[o] as usize; o += 1;
    let end = o + n * 2 + n * n;
    input().clear(); input().extend_from_slice(&data[o..end]); o = end; veh_relations(n as u32);
    let n = data[o] as usize; o += 1;
    let end = o + n * 40;
    input().clear(); input().extend_from_slice(&data[o..end]); o = end; veh_flight_inputs(n as u32);
    let n = data[o] as usize; o += 1;
    for _ in 0..n {
        let r = Ref::new(data[o] as u32, data[o + 1] as u32); o += 2;
        let Some(v) = sim().store.vehicle.get_mut(r) else { panic!("stale F2 foreign row") };
        // Tow state is foreign through step 5.
        v.tow_target = data[o]; v.tow_local.copy_from_slice(&data[o + 1..o + 4]);
        v.tow_leg = StrId(data[o + 4] as u32); v.tow_angle = data[o + 5]; v.tow_winding = data[o + 6]; o += 7;
        let p = poses()[r.slot as usize].as_mut().expect("F2 rig");
        o += posing::inputs(&mut p.pose, &data[o..]);
    }
}

#[no_mangle]
pub extern "C" fn f2_step(dt: f64, tick: f64) {
    assert!(active(), "F2 must be initialized");
    let dt = three::clamp(dt, 0.0, 0.1);
    unsafe { *addr_of_mut!(TIME) += dt; *addr_of_mut!(CLOCK) = Some(io()[0]); }
    spawns(dt);
    foreign();
    live_seats::latch(unsafe { *addr_of_mut!(TIME) });
    let flags = io()[2] as u32;
    let left = veh_ground(dt, unsafe { *addr_of_mut!(ACC) }, unsafe { *addr_of_mut!(FLIGHT_ACC) }, io()[1], io()[0], flags);
    unsafe { *addr_of_mut!(ACC) = left; }
    desc()[10] = tick;
    live_seats::prepare_foreign_fire();
    publish_posed();
    import_f2_foreign(&VehNumber { value: dt });
    live_seats::turret_tick(dt);
    publish();
    live_seats::finish();
}

/// Synchronous authoritative writes. Both slab generation and gameplay life
/// are checked; no old reference can write into a replacement's state.
#[no_mangle]
pub extern "C" fn f2_command(slot: u32, generation: u32, life: f64, code: u32, value: f64, tag: u32, owner_slot: u32, owner_gen: u32) -> u32 {
    let r = Ref::new(slot, generation);
    let Some(v) = sim().store.vehicle.get(r) else { return 0 };
    if v.target_generation != life { return 0; }
    let flying = v.flying;
    let index = hull_refs().iter().position(|&x| x == r).expect("F2 ordered hull");
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
