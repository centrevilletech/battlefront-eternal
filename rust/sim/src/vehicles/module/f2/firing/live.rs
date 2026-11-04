//! Persistent storage and the native calls used by Vehicle.fireSeat.
use super::*;
use crate::abi::sim;
use crate::combat::defs::{K, WeaponClass};
use crate::combat::weapons::{Admission, Aim, Shot, ShotKind, Trigger, WeaponDraws, LockTarget};
use crate::gen::*;
use crate::store::{Ref, StrId};
use crate::store::Slab;
use crate::flight::FlightWorld;
use crate::store::block::Block;
use crate::store::calls::Calls;
use crate::vehicles::{Hull, VehicleWorld};
use crate::vehicles::firing::{spread, WeaponGeometry};
use crate::vehicles::module::{poses, JsWorld, MEMBERS, MemberRow, v3, vec};
use std::ptr::addr_of_mut;

struct Mounted {
    birth: FfWeaponBirth,
    state: Weapon,
    class: Option<WeaponClass>,
    lock_angles: Vec<[f64; 2]>,
    keys: WeaponGeometry,
    input: FfWeaponInput,
    shots: Vec<Shot>,
    dirty: bool,
}
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
struct State {
    emitters: Vec<Option<Emitter>>, weapons: Slab<Mounted>, crafts: Vec<Option<Craft>>, cables: Vec<Cable>,
    phase: [f64; FfPhase::WORDS], supply_result: Calls, output: Block, cable_output: Block, published: Vec<FfHullOut>, locked: StrId, attached: StrId, snapped: StrId, retow: StrId, tripped: StrId, empty: StrId, class_names: [StrId; 4],
}
static mut STATE: Option<State> = None;
fn state() -> &'static mut State {
    unsafe { (*addr_of_mut!(STATE)).get_or_insert_with(|| State { emitters: Vec::with_capacity(1024), weapons: Slab::with_capacity(512), crafts: Vec::with_capacity(128),
        cables: Vec::with_capacity(128), phase: [0.0; FfPhase::WORDS], supply_result: Calls::default(), output: Block::default(), cable_output: Block::default(), published: Vec::with_capacity(128), locked: sim().strings.intern_str("Locked"), attached: sim().strings.intern_str("attached"), snapped: sim().strings.intern_str("snapped"), retow: sim().strings.intern_str("retow"), tripped: sim().strings.intern_str("tripped"), empty: sim().strings.intern_str(""), class_names: FlightClass::NAMES.map(|n| sim().strings.intern_str(n)) }) }
}
fn node(n: f64) -> Option<three::NodeId> { (n >= 0.0).then_some(n as three::NodeId) }

#[no_mangle]
pub extern "C" fn f2_firing_words(which: u32) -> u32 {
    match which { 0 => FfBirth::WORDS as u32, 1 => FfWeaponInput::WORDS as u32, 2 => FfSeatInput::WORDS as u32, 3 => FfSupply::WORDS as u32, 4 => FfSupplyResult::WORDS as u32, _ => 0 }
}

/// Vehicle construction and respawn give mounted weapons a new gameplay life.
#[no_mangle]
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
            let keys = WeaponGeometry { weapon_node: node(birth.weapon_node), fallback_node: node(birth.fallback_node),
                direction_nodes: birth.direction_nodes.iter().map(|&n| node(n)).collect(), outside_limits: birth.outside.clone() };
            let lock_angles = class.levels.iter().map(|d| { let on = d.field("LockOnAngle").or(0.0); [on, d.field("LockOffAngle").or(on)] }).collect();
            let r = state().weapons.insert(Mounted { lock_angles, state: birth.state.clone(), birth, class: Some(class), keys, input: FfWeaponInput::default(), shots: Vec::with_capacity(32), dirty: true });
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

fn reset_weapons(r: Ref<Vehicle>) {
    let st = state();
    if let Some(slot) = st.crafts.get_mut(r.slot as usize) {
        if slot.as_ref().is_some_and(|c| c.id == r) {
            let c = slot.take().unwrap();
            for s in c.seats { for w in s { st.weapons.remove(w); } }
        }
    }
}
pub fn retire(r: Ref<Vehicle>) {
    reset_weapons(r);
    state().cables.retain(|t| t.source != r && t.target != r);
    rebuild_publication();
}
fn rebuild_publication() {
    let st = state();
    st.published.clear();
    for c in st.crafts.iter().flatten() {
        let mut seats = Vec::new();
        for (i, s) in c.seats.iter().enumerate() {
            let mut weapons = Vec::new();
            for &r in s {
                let w = st.weapons.get(r).unwrap();
                weapons.push(FfWeaponOut { state: w.state.clone(), barrel: 0.0, kicks: vec![None; w.birth.muzzles.len()] });
            }
            seats.push(FfSeatOut { consumed: c.consumed[i], weapons });
        }
        st.published.push(FfHullOut { hull: c.id, life: c.life, seats });
    }
    st.cable_output.words.reserve(8192); st.cable_output.heap.reserve(1024);
    st.output.words.reserve(32768); st.output.heap.reserve(32768);
}

/// SeatInput's frozen triggers and seatWeaponPolicy/AI spread modifiers.
/// The generated record is fixed width and contains no variable heap.
pub fn inputs(data: &[f64]) -> usize {
    let mut o = 0;
    let st = state();
    for c in st.crafts.iter_mut().flatten() {
        for used in &mut c.consumed { *used = false; }
    }
    // Follow Vehicles.vehicles, not slab order after retirement and reuse.
    let n = sim().store.vehicles.vehicles.len();
    for i in 0..n {
        let r = sim().store.vehicles.vehicles[i];
        let c = st.crafts[r.slot as usize].as_ref().expect("mounted hull birth");
        for (si, s) in c.seats.iter().enumerate() {
            let aim = FfSeatInput::take(&data[o..o + FfSeatInput::WORDS], &[]); o += FfSeatInput::WORDS;
            let sp = &mut poses()[r.slot as usize].as_mut().unwrap().pose.seats[si];
            sp.aiming = aim.aiming; sp.aim = vec(&aim.aim);
            for &r in s {
            let w = st.weapons.get_mut(r).unwrap();
            w.input = FfWeaponInput::take(&data[o..o + FfWeaponInput::WORDS], &[]);
            o += FfWeaponInput::WORDS;
            w.birth.channel = w.input.channel;
            w.state.random = w.input.has_random;
            w.state.floor = w.input.floor;
            if w.state.kick < w.state.floor { w.state.kick = w.state.floor; }
            w.state.spread_scale = w.input.spread_scale;
            w.state.sniper_scale = w.input.sniper_scale;
            w.state.yaw_spread_override = w.input.yaw_spread_override;
        }}
    }
    o
}

/// Copies just mounted state; physics and leases remain in their canonical slab.
fn dirty(st: &State, c: &Craft) -> bool {
    c.seats.iter().any(|s| s.iter().any(|&r| st.weapons.get(r).unwrap().dirty))
}
/// FfHullOut contains only hulls whose mounted state changed since publication.
pub fn publish() -> *const f64 { publish_for(None) }
fn publish_for(only: Option<Ref<Vehicle>>) -> *const f64 {
    let st = state();
    let n = st.crafts.iter().flatten().filter(|c| only.is_none_or(|r| c.id == r) && dirty(st, c)).count();
    st.output.begin(n, FfHullOut::WORDS);
    let mut at = 0;
    for row in &mut st.published {
        if only.is_some_and(|r| row.hull != r) { continue; }
        let c = st.crafts[row.hull.slot as usize].as_ref().unwrap();
        let changed = c.seats.iter().any(|s| s.iter().any(|&r| st.weapons.get(r).unwrap().dirty));
        if !changed { continue; }
        for (i, seat) in row.seats.iter_mut().enumerate() {
            seat.consumed = c.consumed[i];
            for (j, out) in seat.weapons.iter_mut().enumerate() {
                let w = st.weapons.get_mut(c.seats[i][j]).unwrap();
                out.state.clone_from(&w.state); w.dirty = false;
                if let Some(p) = poses()[c.id.slot as usize].as_ref() {
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
/// The existing F2 descriptor points to a generated firing descriptor.
/// Its event address removes getter/clear exports from the ordinary tick.
pub fn phase_ptr() -> *const f64 {
    let weapons = publish() as usize as u32;
    let st = state();
    let value = FfPhase { weapons, cables: st.cable_output.ptr() as usize as u32, events: sim().events.ptr() as usize as u32 };
    value.put(&mut st.phase, &mut st.cable_output.heap);
    st.phase.as_ptr()
}

struct Draws { hull: Ref<Vehicle>, seat: u32, weapon: u32 }
impl WeaponDraws for Draws {
    fn draw(&mut self, _: u32) -> f64 { import_f2_weapon_draw(&FfDraw { hull: self.hull, seat: self.seat, weapon: self.weapon }).value }
    fn legacy(&mut self, i: u32) -> f64 { self.draw(i) }
    fn seeded(&mut self, _: f64, _: f64) {}
    fn admit(&mut self, _: u32) -> Admission { Admission::Yes }
}

/// Load only fields read by firing geometry. No Hull copy is ever written
/// back after an ordnance callback, so reciprocal blast edits cannot be lost.
fn geometry(r: Ref<Vehicle>) -> &'static mut Hull {
    let c = state().crafts[r.slot as usize].as_mut().unwrap();
    let v = sim().store.vehicle.get(r).unwrap();
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
    std::mem::swap(h.pose.as_mut().unwrap(), &mut poses()[r.slot as usize].as_mut().unwrap().pose);
    h
}
fn keep_geometry(r: Ref<Vehicle>) {
    let c = state().crafts[r.slot as usize].as_mut().unwrap();
    std::mem::swap(c.geometry.pose.as_mut().unwrap(), &mut poses()[r.slot as usize].as_mut().unwrap().pose);
}

/// weapon-targeting.js lockPick. Bot targets bypass the view cone, while
/// human acquisition and retention use the candidate's actual box or sphere.
fn lock_pick(r: Ref<Vehicle>, class: &WeaponClass, w: &Weapon, seat: usize, target: i32, masks: (u32, u32), angles: [f64; 2]) -> Option<LockTarget> {
    let h = geometry(r);
    let origin = h.seat_eye(seat);
    let mut direction = h.reticle_ray(seat, &origin); direction.normalize();
    keep_geometry(r);
    let d = w.level(class);
    let [on, off] = angles;
    let range = d.get(K::LockOnRange).or(0.0);
    if target >= -1 {
        if target < 0 { return None; }
        let facts = target_facts(r, target as usize, false, false);
        if !facts.alive || !allowed(masks, &facts) { return None; }
        return Some(target_key(target as usize, facts.alive));
    }
    if range <= 0.0 || direction.length_sq() == 0.0 { return None; }
    let mut best = None;
    let mut best_angle = f64::INFINITY; let mut best_distance = f64::INFINITY;
    for pass in 0..2 {
        if pass == 0 && !w.lock_complete { continue; }
        let n = unsafe { (&*addr_of_mut!(MEMBERS)).len() };
        let cone = v8math::max(0.0, if pass == 0 { off } else { on }) * std::f64::consts::PI / 180.0;
        for i in 0..n {
            let member = member(i);
            if pass == 0 && (member.id != w.lock_target_id || member.generation.unwrap_or(0.0) != w.lock_generation) { continue; }
            let mut facts = target_facts(r, i, false, false);
            if !facts.eligible || !allowed(masks, &facts) { continue; }
            let hull = if member.kind == 1 || member.kind == 3 { sim().store.vehicle.at(member.hull).map(|(_, v)| v) } else { None };
            if hull.is_none() { facts = target_facts(r, i, true, false); }
            let (a, distance) = if let Some(v) = hull {
                box_span(&origin, &direction, &Matrix4 { elements: v.matrix }, &vec(&v.bounds_min), &vec(&v.bounds_max))
            } else { sphere_span(&origin, &direction, &vec(&facts.center), facts.radius) };
            if a > cone || distance > range { continue; }
            if best.is_some() && (best_angle < a || best_angle == a && best_distance <= distance) { continue; }
            facts = target_facts(r, i, false, true);
            let mut sight = vec(&facts.aim); sight.sub(&origin);
            let length = sight.length();
            let hit = if length < 1e-6 { None } else { JsWorld { craft: r.slot }.cast(&origin, sight.divide_scalar(length), length, Some(sim().store.vehicle.get(r).unwrap().body.slot)) };
            if hit.is_some_and(|h| h.body.map(|b| b as f64) != Some(facts.body)) { continue; }
            best = Some(target_key(i, facts.alive)); best_angle = a; best_distance = distance;
        }
        if best.is_some() { return best; }
    }
    None
}
fn member(i: usize) -> MemberRow { unsafe { (&*addr_of_mut!(MEMBERS))[i] } }
fn target_key(i: usize, alive: bool) -> LockTarget { let m = member(i); LockTarget { id: m.id, generation: m.generation.unwrap_or(0.0), alive } }
fn target_facts(r: Ref<Vehicle>, i: usize, shapes: bool, aim: bool) -> FfTargetFacts {
    import_f2_target_facts(&FfTargetQuery { index: i as u32, viewer: r, shapes, aim })
}
fn allowed(masks: (u32, u32), f: &FfTargetFacts) -> bool {
    masks.0 & (1 << (f.relation as i32 + 1)) != 0 && masks.1 & (1 << f.category) != 0
}

/// Vehicles.fireCrews: hull order, then seat order, then channel order.
pub fn step(dt: f64, clock: f64) {
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
            for weapon in 0..count { fire_seat(r, seat, weapon, dt, clock); }
        }
    }
    update_tethers();
}

fn fire_seat(r: Ref<Vehicle>, seat: usize, weapon: usize, dt: f64, clock: f64) {
    let wr = state().crafts[r.slot as usize].as_ref().unwrap().seats[seat][weapon];
    let (mut w, class, input, mut shots, tow, masks, angles) = {
        let m = state().weapons.get_mut(wr).unwrap();
        let angles = m.lock_angles[m.state.level as usize];
        (std::mem::take(&mut m.state), m.class.take().unwrap(), std::mem::take(&mut m.input), std::mem::take(&mut m.shots), m.birth.tow, (m.birth.relation_mask, m.birth.category_mask), angles)
    };
    let v = sim().store.vehicle.get(r).unwrap();
    if v.flying {
        w.lock_target_id = v.flight_lock_id.unwrap_or(-1.0); w.lock_generation = v.flight_lock_generation.unwrap_or(-1.0);
        w.lock_elapsed = v.flight_lock_timer.unwrap_or(0.0); w.lock_complete = v.flight_lock_state == Some(state().locked);
    } else if class.base().get(K::Base).is_str("launcher") && class.base().get(K::Ordnance).get("base").is_str("missile") {
        let picked = lock_pick(r, &class, &w, seat, input.target, masks, angles);
        w.update_lock(&class, dt, picked);
    }
    let suppressed = tow && sim().store.vehicle.get(r).unwrap().tow_target >= 0.0;
    let trigger = Trigger { pressed: !suppressed && input.pressed, held: !suppressed && (input.held || input.pressed), seed: None, late: 0.0 };
    shots.clear();
    w.update(weapon as u32, &class, dt, true, &trigger, input.reload, &Aim { stick: 0.0, posture: Some(Posture::Stand), zoomed: false, repair_target: false },
        &mut Draws { hull: r, seat: seat as u32, weapon: weapon as u32 }, &mut shots);
    { let m = state().weapons.get_mut(wr).unwrap(); m.state = w; m.class = Some(class); m.dirty = true; }
    for shot in &shots {
        if !sim().store.vehicle.get(r).is_some_and(|v| v.alive) { break; }
        if let ShotKind::Round { yaw, pitch, charge, target_id, target_generation, .. } = shot.kind {
            emit(r, seat, weapon, wr, yaw, pitch, charge, target_id, target_generation, clock);
        }
    }
    if let Some(m) = state().weapons.get_mut(wr) { m.shots = shots; }
}

/// Vehicle.emitShot: geometry, spread, direct ordnance, synchronous fired
/// listeners, then presentation retained in the battle's ordered event buffer.
#[allow(clippy::too_many_arguments)]
fn emit(r: Ref<Vehicle>, seat: usize, weapon: usize, wr: Ref<Mounted>, yaw: f64, pitch: f64, charge: f64, mut target_id: f64, mut target_generation: f64, clock: f64) {
    let h = geometry(r);
    let keys = &state().weapons.get(wr).unwrap().keys;
    let mut muzzle = Vector3::default();
    let m = h.muzzle_of_seat(seat, weapon, keys, |_| clock, &mut muzzle);
    if h.held_by_limits(seat, weapon, m.aimer, keys) { keep_geometry(r); return; }
    let target = h.pose.as_ref().unwrap().seats[seat].target;
    let bot = h.bot_aim(seat);
    keep_geometry(r);
    let tow = state().weapons.get(wr).unwrap().birth.tow;
    let fallback = if tow { tow_aim(r, seat, wr, &muzzle).unwrap_or(bot) } else { bot };
    let h = geometry(r);
    let keys = &state().weapons.get(wr).unwrap().keys;
    let mut direction = h.shot_direction(seat, weapon, m.aimer, keys, &muzzle, target.as_ref(), Some(&fallback));
    keep_geometry(r);
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
        (owner, v.body, v.target_generation, v.flight_class, crate::flight::FlightWorld::tuning(&mut JsWorld { craft: r.slot }, v.flight_class.unwrap_or_default()))
    };
    let (ordnance, channel, emitter) = {
        let b = &state().weapons.get(wr).unwrap().birth;
        (b.ordnance, b.channel, m.emitter.map_or(b.emitter, |i| b.muzzles[i].emitter))
    };
    if tow { launch_tow(r, seat, weapon, wr, &muzzle, &direction); }
    else if let Some(def) = ordnance {
        // This is an internal Rust call. No game Weapon or Ordnance adapter
        // participates in the mounted fire path.
        let (store, strings) = crate::ordnance::module::parts();
        let mut world = crate::ordnance::module::JsWorld::new(store, strings);
        let mut cx = unsafe { crate::ordnance::Cx::new(store, strings, crate::ordnance::module::defs(), &mut world) };
        let round = cx.fire_round(def, v3(&muzzle), v3(&direction), owner, None, Some(body), emitter, channel, Some(target_id), Some(target_generation));
        if flying { if let Some(crate::ordnance::world::Shot::Round(round)) = round {
            if sim().store.round.get(round).is_some_and(|r| r.kind == RoundKind::Missile) {
                cx.flight_missile(round, Some(state().class_names[class.unwrap_or_default() as usize]), config.missile_lifetime, config.missile_speed);
            }
        }}
    }
    let event = FfShot { state: state().weapons.get(wr).unwrap().state.clone(), hull: r, life, seat: seat as u32, weapon: weapon as u32, owner, yaw, pitch, charge, target_id, target_generation, origin: v3(&muzzle), direction: v3(&direction) };
    event.emit(&mut sim().events);
    import_f2_fired(&event);
}

/// flight/targeting.js assistShot, on canonical hull fields and registry facts.
fn assist(r: Ref<Vehicle>, origin: &Vector3, direction: &mut Vector3) {
    let v = sim().store.vehicle.get(r).unwrap();
    let id = v.flight_assist_id.unwrap_or(-1.0);
    let class = v.flight_class.unwrap_or_default();
    let mut world = JsWorld { craft: r.slot };
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

fn hit_hull(body: Option<u32>) -> Option<Ref<Vehicle>> {
    let body = body?;
    let n = sim().store.vehicles.vehicles.len();
    for i in 0..n {
        let r = sim().store.vehicles.vehicles[i];
        if sim().store.vehicle.get(r).unwrap().body.slot == body { return Some(r); }
    }
    None
}
fn command_walker(r: Ref<Vehicle>, source: Ref<Vehicle>, live_enemy: bool) -> bool {
    let Some(c) = state().crafts.get(r.slot as usize).and_then(Option::as_ref) else { return false };
    if !c.command_walker { return false; }
    if !live_enemy { return true; }
    let v = sim().store.vehicle.get(r).unwrap(); let s = sim().store.vehicle.get(source).unwrap();
    v.alive && v.team != s.team
}
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
fn leg_near(r: Ref<Vehicle>, origin: &Vector3) -> Option<Vector3> {
    let c = state().crafts[r.slot as usize].as_ref().unwrap();
    let v = sim().store.vehicle.get(r).unwrap();
    nearest_leg(&c.legs, &Matrix4 { elements: v.matrix }, origin).map(|(_, p)| p)
}
fn leg_hit(r: Ref<Vehicle>, point: &Vector3) -> Option<(usize, Vector3)> {
    let c = state().crafts[r.slot as usize].as_ref().unwrap();
    let v = sim().store.vehicle.get(r).unwrap();
    let mut inverse = Matrix4 { elements: v.matrix }; inverse.invert();
    leg_at(&c.legs, c.leg_top, v.bounds_min[1], &inverse, point)
}

/// Vehicle.towAim: a body hit may choose a command walker even when it is
/// friendly; nearestWalker is the fallback and excludes friends and dead hulls.
fn tow_aim(r: Ref<Vehicle>, seat: usize, wr: Ref<Mounted>, muzzle: &Vector3) -> Option<Vector3> {
    let h = geometry(r); let origin = h.seat_eye(seat); let direction = h.bot_aim(seat); keep_geometry(r);
    let range = state().weapons.get(wr).unwrap().birth.range;
    let body = sim().store.vehicle.get(r).unwrap().body.slot;
    let hit = JsWorld { craft: r.slot }.cast(&origin, &direction, range, Some(body));
    let target = hit.and_then(|h| hit_hull(h.body)).filter(|&t| command_walker(t, r, false)).or_else(|| nearest_walker(r, range))?;
    let mut p = leg_near(target, muzzle)?; p.sub(muzzle).normalize(); Some(p)
}

/// Vehicles.launchTow: the harpoon must hit an enemy command walker's leg.
/// A bot's body hit retries toward the nearest leg, with the original range.
fn launch_tow(r: Ref<Vehicle>, seat: usize, weapon: usize, wr: Ref<Mounted>, origin: &Vector3, direction: &Vector3) -> bool {
    let range = state().weapons.get(wr).unwrap().birth.range;
    let body = sim().store.vehicle.get(r).unwrap().body.slot;
    let Some(hit) = JsWorld { craft: r.slot }.cast(origin, direction, range, Some(body)) else { return false };
    let Some(target) = hit_hull(hit.body).filter(|&t| command_walker(t, r, true)) else { return false };
    let mut leg = leg_hit(target, &hit.point);
    if leg.is_none() {
        if let Some(mut p) = leg_near(target, origin) {
            p.sub(origin); let far = p.length();
            if far > 1e-3 {
                let hit = JsWorld { craft: r.slot }.cast(origin, p.divide_scalar(far), v8math::min(range, far + 2.0), Some(body));
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
fn update_tethers() {
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
        let h = geometry(r);
        let pose = h.pose.as_mut().unwrap();
        let node = pose.seats.get(seat).and_then(|s| s.turret).or_else(|| pose.seats.iter().find_map(|s| s.turret));
        let mut start = h.center;
        if let Some(node) = node { pose.rig.tree.get_world_position(node, &mut start); } else { start.apply_matrix4(&h.matrix); }
        keep_geometry(r);
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
            let (tag, slot, gen) = match owner { OrdnanceOwner::Unit(u) => (2, u.slot, u.gen), _ => (3, r.slot, r.gen) };
            super::super::f2_command(target.slot, target.gen, target_life, 0, f64::INFINITY, tag, slot, gen);
        }
    }
}

fn register_emitter(source: BeamSource, hull: Ref<Vehicle>, life: f64, seat: usize, weapon: usize, node: Option<three::NodeId>, aimer: Option<usize>) {
    if let BeamSource::Emitter(identity) = source {
        let slots = &mut state().emitters;
        if slots.len() <= identity.slot as usize { slots.resize_with(identity.slot as usize + 1, || None); }
        slots[identity.slot as usize] = Some(Emitter { identity, hull, life, seat, weapon, node, aimer });
    }
}
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
pub fn emitter_local(source: BeamSource, origin: [f64; 3]) -> Option<[f64; 3]> {
    let e = emitter(source)?;
    let mut inverse = emitter_matrix(e); inverse.invert();
    let mut p = vec(&origin); p.apply_matrix4(&inverse); Some(v3(&p))
}
/// OrdnanceWorld.beamPose reads the retained rig directly, without a JS detour.
pub fn emitter_pose(source: BeamSource, local: [f64; 3], position: &mut [f64; 3], direction: &mut [f64; 3]) -> bool {
    let Some(e) = emitter(source) else { return false };
    if emitter_gone(source) == Some(true) { return true; }
    let mut p = vec(&local); p.apply_matrix4(&emitter_matrix(e)); *position = v3(&p);
    let wr = state().crafts[e.hull.slot as usize].as_ref().unwrap().seats[e.seat][e.weapon];
    let h = geometry(e.hull);
    let keys = &state().weapons.get(wr).unwrap().keys;
    let d = h.beam_direction(e.seat, e.weapon, e.node, e.aimer, keys, &mut Vector3::default());
    keep_geometry(e.hull); *direction = v3(&d); true
}

/// PowerupStation.serve sends ammunition as one scalar command per hull.
/// It never imports the mirrored weapon clocks back into the mounted slab.
#[no_mangle]
pub extern "C" fn f2_firing_supply() -> *const f64 {
    let a = { let (row, heap) = sim().calls.args(); FfSupply::take(row, heap) };
    let mut changed = false;
    let current = sim().store.vehicle.get(a.hull).is_some_and(|v| v.target_generation == a.life);
    if current {
        let c = state().crafts[a.hull.slot as usize].as_ref().unwrap();
        for s in &c.seats { for &r in s {
            let m = state().weapons.get_mut(r).unwrap();
            let w = &mut m.state; let d = w.level(m.class.as_ref().unwrap());
            let ammo = d.get(K::Ammo).num();
            if w.bottomless || !(ammo > 0.0) { continue; }
            let loaded = if w.clip >= w.per_shot - 1e-6 { 1.0 } else { 0.0 };
            let next = three::clamp(w.reserve + a.amount, 0.0, ammo - loaded);
            if v8math::abs(next - w.reserve) > 0.0001 { w.reserve = next; m.dirty = true; changed = true; }
        }}
    }
    let result = FfSupplyResult { changed, output: publish_for(Some(a.hull)) as usize as u32 };
    state().supply_result.result(FfSupplyResult::WORDS, |row, heap| result.put(row, heap));
    state().supply_result.block.ptr()
}
