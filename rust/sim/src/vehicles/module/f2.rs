//! Persistent fleet scheduler. The generated slab is canonical; input contains
//! foreign facts only. No complete vehicle row is imported by an ordinary tick.
pub mod firing;

use super::*;
use crate::store::Nullable;

const CAPACITY: usize = 262144;
static mut ACTIVE: bool = false;
static mut IO: Vec<f64> = Vec::new();
static mut DESCRIPTOR: [f64; 16] = [0.0; 16];
static mut LIVES: Vec<f64> = Vec::new();
static mut DESCS: Vec<super::super::spawn::SlotDesc> = Vec::new();
static mut ACC: f64 = 0.0;
static mut TIME: f64 = 0.0;
static mut TOUCHED: Option<crate::store::block::Block> = None;
static mut CHANGED: Vec<Ref<Vehicle>> = Vec::new();
fn touched() -> &'static mut crate::store::block::Block { unsafe { (*addr_of_mut!(TOUCHED)).get_or_insert_with(Default::default) } }
fn changed() -> &'static mut Vec<Ref<Vehicle>> { unsafe { &mut *addr_of_mut!(CHANGED) } }

/// Commands and damage hooks expose only the hull they changed. This also
/// retains reciprocal blast writes until the end of the firing phase.
pub fn mark_changed(r: Ref<Vehicle>) { if !changed().contains(&r) { changed().push(r); } }
fn clear_publication() {
    for i in [2, 3, 4, 5, 6, 11, 13, 14] { desc()[i] = 0.0; }
}
fn row_out(r: Ref<Vehicle>, at: usize) {
    let (row, heap) = touched().row(at);
    row[0] = r.slot as f64; row[1] = r.gen as f64;
    sim().store.vehicle.get(r).unwrap().put_export(row, heap);
}
pub fn publish_touched(r: Ref<Vehicle>) {
    mark_changed(r);
    clear_publication();
    touched().begin(1, Vehicle::EXPORT_STRIDE); row_out(r, 0); touched().finish();
    desc()[2] = touched().ptr() as usize as f64;
    desc()[3] = sim().export(KIND_VEHICLES as u32, 0).unwrap().ptr() as usize as f64;
    desc()[4] = veh_changes() as usize as f64;
}
fn firing_output() {
    clear_publication();
    let n = changed().len(); touched().begin(n, Vehicle::EXPORT_STRIDE);
    for i in 0..n { row_out(changed()[i], i); }
    touched().finish(); desc()[2] = touched().ptr() as usize as f64;
    desc()[4] = veh_changes() as usize as f64;
    desc()[13] = firing::publish() as usize as f64;
    desc()[14] = firing::cables_ptr() as usize as f64;
}

pub fn active() -> bool { unsafe { *addr_of_mut!(ACTIVE) } }
fn io() -> &'static mut Vec<f64> { unsafe { &mut *addr_of_mut!(IO) } }
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
    sim().events.reserve(262144, FfShot::WORDS, 1024);
    changed().reserve(256); touched().words.reserve(32768); touched().heap.reserve(8192);
    desc()[0] = io().as_mut_ptr() as usize as f64;
    desc()[1] = CAPACITY as f64;
    desc().as_ptr()
}

/// Publish without a getter crossing from JS, also before a synchronous hook.
/// No reference to the store survives a foreign call.
pub fn publish() {
    let s = sim();
    let lives = unsafe { &mut *addr_of_mut!(LIVES) };
    lives.clear(); lives.push(s.store.vehicles.vehicles.len() as f64);
    for r in &s.store.vehicles.vehicles {
        let v = s.store.vehicle.get(*r).unwrap();
        lives.extend_from_slice(&[r.slot as f64, v.target_generation, if v.visible { 1.0 } else { 0.0 },
            v.flight_class.map_or(-1.0, |c| c as u8 as f64), v.min_speed.unwrap_or(f64::NAN),
            v.mid_speed.unwrap_or(f64::NAN), v.boost_speed.unwrap_or(f64::NAN), v.speed, v.turn_rate]);
    }
    desc()[13] = firing::publish() as usize as f64;
    desc()[14] = firing::cables_ptr() as usize as f64;
    desc()[12] = 9.0;
    desc()[11] = lives.as_ptr() as usize as f64;
    desc()[2] = s.export(KIND_VEHICLE as u32, 0).expect("F2 hull output").ptr() as usize as f64;
    desc()[3] = s.export(KIND_VEHICLES as u32, 0).expect("F2 pad output").ptr() as usize as f64;
    desc()[4] = veh_changes() as usize as f64;
    desc()[5] = unsafe { (&*addr_of_mut!(POSED)).first().map_or(0.0, |_| (&*addr_of_mut!(POSED)).as_ptr() as usize as f64) };
    desc()[6] = unsafe { (&*addr_of_mut!(LOCKS)).first().map_or(0.0, |_| (&*addr_of_mut!(LOCKS)).as_ptr() as usize as f64) };
    desc()[7] = unsafe { *addr_of_mut!(ACC) };
    desc()[8] = unsafe { *addr_of_mut!(FLIGHT_ACC) };
    desc()[9] = unsafe { *addr_of_mut!(TIME) };
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
        if blocked || near { sim().store.vehicles.slots[i].remaining = 1.0; continue; }
        sim().store.vehicles.slots[i].vehicle = Some(r);
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
        publish();
        import_f2_life(&VehHull { hull: index as f64 });
    }
}

pub fn respawn_clock(hull: usize) {
    let r = hull_refs()[hull];
    let pad = sim().store.vehicle.get(r).unwrap().slot_index as usize;
    sim().store.vehicles.slots[pad].remaining = unsafe { (&*addr_of_mut!(DESCS))[pad].respawn_time };
}

/// Consume only foreign state. Layout is deliberately separate from vehicle
/// rows, so adding a hull field cannot silently turn this into a full import.
fn foreign() {
    let start = io()[CAPACITY - 1] as usize;
    let end = io()[CAPACITY - 2] as usize;
    let data = io()[start..end].to_vec();
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
        if data[o] != 0.0 { v.stick_x = Some(data[o + 1]); v.stick_y = Some(data[o + 2]); }
        o += 3;
        for (seat, lease) in v.seats.iter_mut().zip(&mut v.seat_leases) {
            let has = data[o] != 0.0;
            seat.lease = has.then(|| Ref::new(data[o + 1] as u32, data[o + 2] as u32));
            *lease = if has { SeatLeaseView {
                human: Some(data[o + 3] != 0.0), network_human: Some(data[o + 4] != 0.0),
                unit: if data[o + 5] != 0.0 { Nullable::Value(Ref::new(data[o + 6] as u32, data[o + 7] as u32)) } else { Nullable::Null },
                forward: Some(data[o + 8]), right: Some(data[o + 9]), turn: Some(data[o + 10]), pitch: Some(data[o + 11]), brake: Some(data[o + 12] != 0.0),
            }} else { SeatLeaseView { unit: Nullable::Undefined, ..Default::default() } };
            seat.aim_yaw = data[o + 13]; seat.pitch = data[o + 14];
            seat.look_yaw = opt(data[o + 15], data[o + 16]); seat.look_pitch = opt(data[o + 17], data[o + 18]);
            o += 19;
        }

    }
    o += firing::inputs(&data[o..]);
    assert_eq!(o, data.len(), "F2 foreign input length");
}

#[no_mangle]
pub extern "C" fn f2_step(dt: f64, tick: f64) {
    assert!(active(), "F2 must be initialized");
    let dt = three::clamp(dt, 0.0, 0.1);
    unsafe { *addr_of_mut!(TIME) += dt; *addr_of_mut!(CLOCK) = Some(io()[0]); }
    spawns(dt);
    publish();
    import_f2_latch(&VehNothing {});
    foreign();
    let flags = io()[2] as u32;
    let left = veh_ground(dt, unsafe { *addr_of_mut!(ACC) }, unsafe { *addr_of_mut!(FLIGHT_ACC) }, io()[1], io()[0], flags);
    unsafe { *addr_of_mut!(ACC) = left; }
    desc()[10] = tick;
    publish();
    import_f2_posed(&VehNothing {});
    changed().clear();
    firing::step(dt, io()[0]);
    firing_output();
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
    publish_touched(r);
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
    if !sim().store.vehicle.retire(r) { return 0; }
    sim().store.vehicles.vehicles.retain(|&x| x != r);
    for pad in &mut sim().store.vehicles.slots {
        if pad.vehicle == Some(r) { pad.vehicle = None; }
        pad.variants.entries.retain(|(_, v)| *v != r);
    }
    if let Some(p) = poses().get_mut(slot as usize) { *p = None; }
    firing::retire(r);
    // The last pose belongs to its retired life and cannot be reused.
    unsafe { (*addr_of_mut!(POSED)).clear(); (*addr_of_mut!(LOCKS)).clear(); }
    publish();
    1
}
