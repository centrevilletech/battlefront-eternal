//! LiveObjects.update's owned module entry. The phase takes its Store shell
//! and retained work buffers once. Owner services temporarily lend the Store
//! back around recursive hits; no container borrow spans those services.
use super::{animated, f4, held_bodies::{self, Bodies, Positions}, live_phase, prepared, props, supply, traps};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{Ref, StrId};
use crate::units::f3::{self, WorldServices};
use std::ptr::addr_of_mut;
use three::{Quaternion, Vector3};

struct Io { live: Vec<f64>, tail: Vec<f64>, descriptor: Vec<f64>, handle: Vec<f64>, counters: Vec<f64>, counts: F4Counters, version: u32, pending_publication: bool, dirty_counters: bool }
static mut IO: Option<Io> = None;
fn io() -> &'static mut Io { unsafe { (*addr_of_mut!(IO)).as_mut().expect("live phase initialized") } }
#[derive(Clone, Copy)]
struct Gain { health: StrId, ammo: StrId }
struct Animation { handle: F4Handle, original: Ref<Prop>, flag: Option<usize>, index: usize }
struct State {
    shell: Option<Store>, local: Option<Ref<Unit>>, body_revision: u64, bodies: Bodies, targets: supply::Targets, supply: supply::Prepared,
    stations: Vec<usize>, regenerators: Vec<usize>, traps: Vec<(usize, F4Handle)>,
    hulls: Vec<Ref<Vehicle>>,
}
struct Metadata {
    animations: Vec<Animation>, gains: Vec<Option<Gain>>, grips: Vec<Vec<Vec<Option<u32>>>>,
    sides: Vec<Option<(Ref<Vehicle>, Option<StrId>, Option<StrId>)>>,
}
static mut METADATA: Option<Metadata> = None;
fn metadata() -> &'static Metadata { unsafe { (*addr_of_mut!(METADATA)).as_ref().expect("prepared live metadata") } }
fn metadata_mut() -> &'static mut Metadata {
    assert!(unsafe { (*addr_of_mut!(STATE)).is_some() }, "prepare content outside the live phase");
    unsafe { (*addr_of_mut!(METADATA)).as_mut().expect("prepared live metadata") }
}
static mut STATE: Option<State> = None;
fn state() -> &'static mut State { unsafe { (*addr_of_mut!(STATE)).as_mut().expect("idle live owner") } }
pub(crate) fn reset() { unsafe { *addr_of_mut!(STATE) = None; *addr_of_mut!(IO) = None; *addr_of_mut!(METADATA) = None; } }
fn args<T>(take: impl FnOnce(&[f64], &[f64]) -> T) -> T { let (row, heap) = sim().calls.args(); take(row, heap) }
fn return_handle(handle: F4Handle) -> *const f64 {
    handle.put(&mut io().handle, &mut Vec::new()); io().handle.as_ptr()
}
#[no_mangle]
pub extern "C" fn f4_live_init() -> *const f64 {
    assert!(f3::active(), "canonical unit owner required");
    let setup = args(F4LiveSetup::take);
    let store = &sim().store;
    let mut bodies = Bodies::new((store.unit.slots() as usize).max(setup.registry_capacity as usize), setup.registry_capacity as usize);
    for (r, _) in store.unit.iter() { bodies.adopt(r); }
    let mut stations = Vec::new(); let mut regenerators = Vec::new(); let mut traps = Vec::new();
    for index in 0..crate::units::f3_props::count() {
        let handle = crate::units::f3_props::owner(index);
        if handle.kind == F4ObjectKind::Trap { traps.push((index, handle)); continue; }
        let (_, settings, supply) = f4::station(&handle);
        if supply.is_some() { stations.push(index); }
        else if settings.kind != props::Kind::Objective && settings.regenerates { regenerators.push(index); }
    }
    let live = vec![0.0; F4LiveInput::WORDS]; let tail = vec![0.0; F4TailInput::WORDS];
    let phase_row = vec![0.0; F4Phase::WORDS]; let handle = vec![0.0; F4Handle::WORDS]; let counters = vec![0.0; F4Counters::WORDS];
    unsafe {
        *addr_of_mut!(IO) = Some(Io { live, tail, descriptor: phase_row, handle, counters, counts: F4Counters::default(), version: 0, pending_publication: false, dirty_counters: true });
        *addr_of_mut!(STATE) = Some(State { shell: Some(Store::default()), local: setup.local, body_revision: f3::unit_revision(), bodies,
            targets: supply::Targets::new(setup.unit_capacity as usize, setup.hull_capacity as usize), supply: supply::Prepared::default(),
            stations, regenerators, traps, hulls: Vec::with_capacity(setup.hull_capacity as usize) });
        *addr_of_mut!(METADATA) = Some(Metadata { animations: Vec::new(), gains: Vec::new(), grips: Vec::new(), sides: Vec::new() });
    }
    descriptor()
}
#[no_mangle]
pub extern "C" fn f4_building_handle(index: u32) -> *const f64 { return_handle(crate::units::f3_props::owner(index as usize)) }
#[no_mangle]
pub extern "C" fn f4_unit_class() {
    let c = args(F4UnitClass::take); let slot = c.def.0 as usize;
    state().supply.class(c.def, c.vehicle); let s = metadata_mut(); s.gains.resize(s.gains.len().max(slot + 1), None);
    s.gains[slot] = Some(Gain { health: c.health_effect, ammo: c.ammo_effect });
}
#[no_mangle]
pub extern "C" fn f4_weapon_class() { let w = args(F4WeaponClass::take); state().supply.weapon(w.def, w.ammunition); }
#[no_mangle]
pub extern "C" fn f4_hull_class() {
    let h = args(F4HullClass::take); state().supply.hull(h.hull,h.vehicle); let s = metadata_mut(); let slot = h.hull.slot as usize;
    s.sides.resize(s.sides.len().max(slot + 1), None); s.sides[slot] = Some((h.hull, h.side, h.label));
    let capacity = s.sides.len(); let s = state(); s.targets.reserve(0, capacity);
    s.hulls.reserve(capacity.saturating_sub(s.hulls.len())); f4::live().prepare_hulls(capacity);
}
#[no_mangle]
pub extern "C" fn f4_grip_class() {
    let g = args(F4GripClass::take); let s = metadata_mut();
    let index = s.animations.iter().position(|a| a.handle == g.animated).expect("prepared animated handle");
    s.grips.resize_with(s.grips.len().max(g.def.0 as usize + 1), Vec::new);
    let class = &mut s.grips[g.def.0 as usize]; class.resize_with(class.len().max(index + 1), Vec::new); class[index] = g.clips;
}
#[no_mangle]
pub extern "C" fn f4_animated_birth() -> *const f64 {
    let input = args(F4AnimationSetup::take); let mut animation = prepared::animated(input.birth);
    let index = metadata().animations.len(); animation.state.index = index;
    let handle = f4::live().animated.adopt(1.0, animation);
    // Body order is fixed at construction. Flags share F2/F3's reservation;
    // door posing writes its byte directly, without a collision export.
    let flag = input.body.and_then(|body| {
        let collision = crate::vehicles::module::collision_world().expect("prepared animated collision");
        let kernel = collision.kernel_slot(body.slot)?;
        crate::collision::state().worlds[f3::collision_world()].order.iter().position(|&slot| slot as usize == kernel)
    });
    metadata_mut().animations.push(Animation { handle: handle.clone(), original: input.prop, flag, index });
    return_handle(handle)
}
/// AnimatedProp.state's scalar publication. The body key retained by the
/// native holder is the Unit id, even after that body changes gameplay life.
pub(crate) fn wire_animation_count() -> usize { metadata().animations.len() }
pub(crate) fn wire_animation(index: usize) -> Option<[f64; 7]> {
    let info = metadata().animations.get(index)?;
    let handle = f4::live().animated.current(&info.handle)?;
    let prop = f4::live().animated.get(&handle)?;
    if prop.settings.grabs.is_empty() || prop.state.slot < 0 { return None; }
    let value = &prop.state;
    let (id, life, grab, since) = value.victim.as_ref().map_or((0., 0., -1., 0.),
        |v| (v.unit as f64, v.generation, v.grab as f64, v.since));
    Some([value.slot as f64, value.started, value.start_time, id, life, grab, since])
}
/// LogTrap.stateRecord refreshes on any phase/start/direction change, with
/// the authored trap order independent of its position in buildings.
pub(crate) fn wire_trap_count() -> usize { state().traps.len() }
pub(crate) fn wire_trap(index: usize) -> Option<[f64; 3]> {
    let (_, handle) = state().traps.get(index)?; let value = f4::trap(handle)?;
    let phase = match value.phase { traps::Phase::Down => 0., traps::Phase::Reset => 1., traps::Phase::Armed => 2., traps::Phase::Swing => 3. };
    Some([phase, value.started.unwrap_or(value.tick), u8::from(value.sprung) as f64])
}

pub(crate) fn descriptor() -> *const f64 {
    let phase = F4Phase { version: io().version, live_input: io().live.as_ptr() as u32, tail_input: io().tail.as_ptr() as u32,
        input_capacity: F4LiveInput::WORDS.max(F4TailInput::WORDS) as u32,
        buildings: f4::live().buildings.publication.ptr() as u32, animated: f4::live().animated.publication.ptr() as u32,
        traps: f4::live().traps.publication.ptr() as u32, counters: io().counters.as_ptr() as u32,
        battle: super::f4_core::publication_ptr(), events: sim().events.ptr() as u32, ..F4Phase::default() };
    let io = io(); io.dirty_counters = false; io.counts.put(&mut io.counters, &mut Vec::new()); phase.put(&mut io.descriptor, &mut Vec::new()); io.descriptor.as_ptr()
}
/// Only actual foreign barriers and outer returns publish. Native setters mark
/// their rows; an intermediate commit must not overwrite an unread delta.
pub(crate) fn publish() {
    if unsafe { (*addr_of_mut!(IO)).is_none() } { return; }
    let rows = f4::live().flush();
    let words = { let live = f4::live(); live.buildings.publication.words.len() + live.animated.publication.words.len() + live.traps.publication.words.len() };
    record_publication(rows, words - 3 * crate::store::block::HEADER);
    let io = io();
    // Counters can advance without any changed reader row. Only actual live
    // or battle payloads invalidate the facade epoch at a foreign barrier.
    if io.pending_publication {
        io.version = io.version.wrapping_add(1); io.pending_publication = false;
    } else if !io.dirty_counters {
        // Empty observer barriers keep the same pointers, epoch and counters.
        // Rewriting this descriptor for every aimer would scale with bodies.
        return;
    }
    descriptor();
}
pub(crate) fn record_publication(rows: usize, words: usize) {
    if unsafe { (*addr_of_mut!(IO)).is_none() } {return;}
    if rows != 0 { io().pending_publication = true; }
    let counts = &mut io().counts; counts.published_rows += rows as f64; counts.output_words += words as f64;
}
pub(crate) fn count_command() {count_external(F4Command::WORDS);}
pub(crate) fn count_external(words:usize) {
    if unsafe { (*addr_of_mut!(IO)).is_some() } {io().dirty_counters = true;io().counts.commands += 1.0;io().counts.input_words += words as f64;}
}
#[no_mangle]
pub extern "C" fn f4_live_publish() -> *const f64 { super::f4_core::publish(); publish(); descriptor() }
pub(crate) fn with_store<T>(store: &mut Store, run: impl FnOnce() -> T) -> T {
    std::mem::swap(store, &mut sim().store); let result = run(); std::mem::swap(store, &mut sim().store); result
}
struct Text;
impl super::stats::Text for Text { fn nonempty(&self, id: StrId) -> bool { !sim().strings.units(id).is_empty() } }

struct Supplies<'a> { station: F4Handle, gains: &'a [Option<Gain>], services: WorldServices<'a> }
impl supply::World for Supplies<'_> {
    fn hull_health(&mut self, s: &mut Store, r: Ref<Vehicle>, _: f64, value: f64) {
        with_store(s, || crate::units::f3_hulls::mutate(r, value, None, self.services.reborrow()));
    }
    fn hull_ammo(&mut self, s: &mut Store, r: Ref<Vehicle>, life: f64, amount: f64) -> bool {
        with_store(s, || crate::vehicles::module::f2::firing::supply_station(r, life, amount))
    }
    fn changed(&mut self, _: &mut Store, key: supply::Key) {
        match key { supply::Key::Unit { reference, .. } => f3::changed(reference),
            supply::Key::Hull { reference, .. } => crate::vehicles::module::f2::mark_changed(reference) }
    }
    fn served(&mut self, _: &props::Station, _: supply::Key, class: crate::store::DefId, p: Vector3, gave: u8) {
        let gain = self.gains[class.0 as usize].expect("prepared gain effects");
        if gave & 1 != 0 { F4Effect { name: gain.health, position: array(p) }.emit(&mut sim().events); }
        if gave & 2 != 0 { F4Effect { name: gain.ammo, position: array(p) }.emit(&mut sim().events); }
    }
    fn sound(&mut self, _: &props::Station, first: Option<supply::Key>) {
        // PowerupStation.update identifies the first soldier actually supplied.
        // A hull-only pulse has no soldier and must preserve its null recipient.
        let unit=match first {Some(supply::Key::Unit {reference,..})=>Some(reference),_=>None};
        F4Serve { station: self.station.clone(), unit }.emit(&mut sim().events);
    }
}
struct Animator<'a, 'b> { store: &'a mut Store, positions: &'a Positions, binding: &'a Animation, grips: &'a [Vec<Vec<Option<u32>>>], services: WorldServices<'b> }
impl Animator<'_, '_> {
    fn stage(&mut self, prop: &animated::Animated) {
        f4::live().animated.stage_with(&self.binding.handle, |row, heap| live_phase::write_animated(prop, row, heap));
        if let (Some(order), Some(enabled)) = (self.binding.flag, prop.body_enabled) {
            unsafe { *crate::movement::movement_flags_ptr().add(order) = u8::from(enabled); }
        }
    }
    fn clip(&self, body: &animated::Body, grab: usize) -> Option<u32> {
        let u = self.store.unit.get(body.reference?)?;
        self.grips.get(u.class_def.0 as usize)?.get(self.binding.index)?.get(grab).copied().flatten()
    }
}
impl animated::World for Animator<'_, '_> {
    fn near(&mut self, _: &animated::Animated, settings: &animated::Settings, _: &[Vector3]) -> bool { self.positions.near(self.store, settings) }
    fn refresh_body(&mut self, p: &animated::Animated, b: &mut animated::Body) { held_bodies::refresh(self.store, p, self.binding.original, b); }
    fn body_changed(&mut self, p: &animated::Animated, b: &animated::Body) {
        self.stage(p);
        let Some(key) = held_bodies::commit(self.store, self.binding.original, b) else { return };
        f3::changed(key.reference());
        F4Held { key, prop: b.held.as_ref().map(|_| self.binding.handle.clone()), grab: b.held.as_ref().map_or(0, |h| h.grab as u32),
            since: b.held.as_ref().map_or(0.0, |h| h.since), frame: b.held.as_ref().and_then(|h| h.frame.as_ref()).map(|m| m.elements),
            time: b.held.as_ref().and_then(|h| h.time) }.emit(&mut sim().events);
    }
    fn has_rider_clip(&mut self, _: &animated::Animated, b: &animated::Body, grab: usize) -> bool { self.clip(b, grab).is_some() }
    fn sound(&mut self, p: &animated::Animated, name: StrId, position: Vector3) { self.stage(p); F4Sound { name, position: array(position) }.emit(&mut sim().events); }
    fn let_go(&mut self, p: &animated::Animated, b: &mut animated::Body) {
        self.store.unit.get_mut(b.reference.expect("held Unit")).unwrap().let_go();
        self.refresh_body(p, b); self.body_changed(p, b);
    }
    fn damage(&mut self, p: &animated::Animated, b: &mut animated::Body, amount: f64) {
        self.stage(p); let r = b.reference.expect("swallowed Unit");
        with_store(self.store, || crate::combat::live::damage(r, amount, Attacker::Null, self.services.reborrow()));
    }
    fn remove(&mut self, p: &animated::Animated, b: &mut animated::Body) {
        self.stage(p); let r = b.reference.expect("removed Unit");
        with_store(self.store, || f3::remove_native(r, self.services.reborrow()));
    }
    fn rider_root(&mut self, _: &animated::Animated, b: &animated::Body, _: StrId, time: f64) -> Option<(Vector3, Quaternion)> {
        crate::units::live_pose::held_root(b.reference?, self.clip(b, b.held.as_ref()?.grab)?, time)
    }
}
struct Logs<'a, 'b> { store: &'a mut Store, handle: F4Handle, index: usize, sides: &'a [Option<(Ref<Vehicle>, Option<StrId>, Option<StrId>)>], services: WorldServices<'b> }
impl Logs<'_, '_> { fn commit(&self, t: &traps::Trap) { f4::put_trap(&self.handle, *t); } }
impl traps::World for Logs<'_, '_> {
    type Vehicle = Ref<Vehicle>;
    fn alive(&mut self, r: Self::Vehicle) -> bool { self.store.vehicle.get(r).is_some_and(|v| v.alive) }
    fn sphere(&mut self, r: Self::Vehicle) -> ([f64; 3], f64) {
        with_store(self.store, || { let (p, radius) = crate::units::f3_hulls::sphere(r, true); (array(p), radius) })
    }
    fn side(&mut self, r: Self::Vehicle) -> Option<StrId> { self.sides.get(r.slot as usize).copied().flatten().filter(|(key, _, _)| *key == r).and_then(|(_, side, _)| side) }
    fn clear_attacker(&mut self, t: &mut traps::Trap, r: Self::Vehicle) { self.commit(t); self.store.vehicle.get_mut(r).unwrap().last_attacker = OrdnanceOwner::Null; crate::vehicles::module::f2::mark_changed(r); }
    fn set_health(&mut self, t: &mut traps::Trap, r: Self::Vehicle, value: f64) {
        self.commit(t); with_store(self.store, || crate::units::f3_hulls::mutate(r, value, None, self.services.reborrow()));
        *t = f4::trap(&self.handle).expect("live trap after hull death");
    }
    fn set_dying(&mut self, _: &mut traps::Trap, r: Self::Vehicle, value: f64) { self.store.vehicle.get_mut(r).unwrap().dying = value; crate::vehicles::module::f2::mark_changed(r); }
    fn hide_root(&mut self, _: &mut traps::Trap, r: Self::Vehicle) { self.store.vehicle.get_mut(r).unwrap().visible = false; crate::vehicles::module::f2::mark_changed(r); }
    fn show(&mut self, t: &mut traps::Trap, built: bool) {
        self.commit(t); with_store(self.store, || crate::units::f3_props::trap_show(self.index, t, built, self.services.reborrow()));
        *t = f4::trap(&self.handle).expect("shown trap");
    }
    fn sound(&mut self, t: &mut traps::Trap, name: StrId, at: [f64; 3]) { self.commit(t); F4Sound { name, position: at }.emit(&mut sim().events); }
    fn effect(&mut self, t: &mut traps::Trap, name: StrId, at: [f64; 3]) { self.commit(t); F4Effect { name, position: at }.emit(&mut sim().events); }
}
fn array(p: Vector3) -> [f64; 3] { [p.x, p.y, p.z] }

#[no_mangle]
pub extern "C" fn f4_live() -> *const f64 {
    let before = crate::allocations::snapshot(); crate::allocations::begin();
    let input = F4LiveInput::take(&io().live, &[]);
    let outer = f3::external();
    f3::with_services(|services| {
        let mut services = services.expect("native live services");
        let mut state = unsafe { (*addr_of_mut!(STATE)).take().expect("nonrecursive live phase") };
        let mut store = std::mem::replace(&mut sim().store, state.shell.take().expect("retained store shell"));
        let metadata = metadata();
        // Ordinary ticks retain structural membership. A new native body or
        // class replacement admits its integer slot before a prop can see it.
        let revision = f3::unit_revision();
        if state.body_revision != revision {
            for (r, _) in store.unit.iter() { if state.bodies.frames.get(r.slot as usize).and_then(|b| b.reference) != Some(r) { state.bodies.adopt(r); } }
            state.body_revision = revision;
        }
        if !state.stations.is_empty() {
            let driving = state.local.and_then(|r| store.unit.get(r)).and_then(|u| u.seat).and_then(|r| store.seat_lease.get(r)).is_some_and(|l| l.vehicle.is_some());
            state.targets.begin(&store, state.local, driving, f3::built(), &store.vehicles.vehicles);
        }
        for &index in &state.stations {
            if crate::units::f3_props::needs_regeneration(index) {
                with_store(&mut store, || crate::units::f3_props::regenerate(index, input.dt, services.reborrow()));
            }
            let handle = crate::units::f3_props::owner(index); let (mut station, _, supply) = f4::station(&handle);
            state.targets.pulse(&mut station, input.dt, &supply.expect("station supply"), &mut store, &state.supply,
                &mut Supplies { station: handle.clone(), gains: &metadata.gains, services: services.reborrow() });
            f4::put_station(&handle, station);
        }
        for &index in &state.regenerators {
            if crate::units::f3_props::needs_regeneration(index) {
                with_store(&mut store, || crate::units::f3_props::regenerate(index, input.dt, services.reborrow()));
            }
        }
        with_store(&mut store, || crate::vehicles::module::f2::live_seats::idle_turrets(input.dt));
        if !metadata.animations.is_empty() { state.bodies.positions.begin(&store, state.local, f3::built()); }
        for binding in &metadata.animations {
            state.bodies.refresh_order(&store);
            let Some(mut prop) = f4::live().animated.checkout(&binding.handle) else { continue };
            let mut world = Animator { store: &mut store, positions: &state.bodies.positions, binding, grips: &metadata.grips, services: services.reborrow() };
            prop.step(input.dt, input.tick, &[], &mut state.bodies.frames, &state.bodies.order, &mut world); world.stage(&prop.state);
            assert!(f4::live().animated.restore(&binding.handle, prop).is_ok(), "live animated row");
        }
        for (index, handle) in &state.traps {
            state.hulls.clear(); state.hulls.extend_from_slice(&store.vehicles.vehicles);
            let (mut trap, settings, mut victims) = f4::take_trap(handle);
            trap.step_with_scratch(input.dt, input.tick, &settings, &Text, &state.hulls,
                &mut Logs { store: &mut store, handle: handle.clone(), index: *index, sides: &metadata.sides, services: services.reborrow() }, &mut victims);
            f4::put_trap(handle, trap); f4::return_victims(handle, victims);
        }
        state.shell = Some(std::mem::replace(&mut sim().store, store)); unsafe { *addr_of_mut!(STATE) = Some(state); }
    });
    { let io = io(); io.counts.live += 1.0; io.counts.input_words += F4LiveInput::WORDS as f64; }
    drop(outer);
    crate::allocations::end(); let used = crate::allocations::snapshot().since(before);
    io().counts.allocations = used.calls as u32; io().counts.reallocations = used.reallocs as u32; io().counts.allocation_bytes = used.bytes as u32;
    descriptor()
}

/// External writes validate the complete F4 identity before any owner service.
/// Native combat calls the same services directly inside the phase.
#[no_mangle]
pub extern "C" fn f4_live_command() -> u32 {
    let command = args(F4Command::take); count_command();
    let current = match command.target.kind {
        F4ObjectKind::Building => f4::live().buildings.current(&command.target),
        F4ObjectKind::Trap => f4::live().traps.current(&command.target),
        F4ObjectKind::Animated => f4::live().animated.current(&command.target), _ => None,
    };
    if current.as_ref() != Some(&command.target) { return 0; }
    if command.op == F4CommandOp::Play {
        let accepted = f4::live().play(&command.target, command.value as i32, command.flag, command.tick);
        if accepted {
            let enabled = f4::live().animated.get(&command.target).and_then(|a| a.state.body_enabled);
            let flag = metadata().animations.iter().find(|a| a.handle == command.target).and_then(|a| a.flag);
            if let (Some(flag), Some(enabled)) = (flag, enabled) { unsafe { *crate::movement::movement_flags_ptr().add(flag) = u8::from(enabled); } }
            super::f4_core::publish(); publish();
        }
        return u32::from(accepted);
    }
    if !matches!(command.op, F4CommandOp::Health | F4CommandOp::Damage) { return 0; }
    let index = (0..crate::units::f3_props::count()).find(|&i| crate::units::f3_props::owner(i) == command.target);
    let Some(index) = index else { return 0 };
    let outer = f3::external();
    f3::with_services(|services| crate::units::f3_props::command_hit(index, command.value,
        (command.op == F4CommandOp::Damage).then_some(command.attacker), command.damage_info, services.expect("native prop command")));
    drop(outer); 1
}

/// Prepared hull display names are shared with the tail's mounted statistics.
pub(crate) fn hull_label(r: Ref<Vehicle>) -> Option<StrId> {
    metadata().sides.get(r.slot as usize).copied().flatten().filter(|(h,_,_)| *h == r).and_then(|(_,_,label)|label)
}

/// The shape reader can run during recursive damage while an animated rig
/// is checked out. Its staged scalar victim remains readable at that point;
/// immutable clip bindings live outside the phase's movable work buffers.
pub(crate) fn held_clip(r: Ref<Unit>, unit: &Unit) -> Option<u32> {
    let Nullable::Value(held) = &unit.held else { return None };
    let metadata = unsafe { (*addr_of_mut!(METADATA)).as_ref()? };
    let binding = metadata.animations.iter().find(|a| a.original == held.prop)?;
    let handle = f4::live().animated.current(&binding.handle)?;
    let grab = if let Some(actor) = f4::live().animated.get(&handle) {
        let v = actor.state.victim.as_ref()?;
        if v.unit != r.slot as usize || v.generation != unit.generation { return None; }
        v.grab
    } else {
        let row = f4::live().animated.staged(&handle)?;
        let view = F4AnimatedView::take(row, &[]);
        if !view.has_victim || view.victim.unit != r.slot as f64 || view.victim.generation != unit.generation { return None; }
        view.victim.grab as usize
    };
    metadata.grips.get(unit.class_def.0 as usize)?.get(binding.index)?.get(grab).copied().flatten()
}

pub(crate) fn tail_input() -> F4TailInput {F4TailInput::take(&io().tail,&[])}
pub(crate) fn count_tail(commands:usize,before:crate::allocations::Counts) {
    let used=crate::allocations::snapshot().since(before);let counts=&mut io().counts;
    counts.tail+=1.0;counts.input_words+=(F4TailInput::WORDS+commands*HsCommand::WORDS) as f64;
    counts.allocations=used.calls as u32;counts.reallocations=used.reallocs as u32;counts.allocation_bytes=used.bytes as u32;
    descriptor();
}

pub(crate) fn count_hook() {
    io().dirty_counters = true;
    let counts=&mut io().counts;counts.hooks+=1.0;counts.output_words+=F4AIHook::WORDS as f64;
}

/// Game.step's pre-AI viewpoint count. The body identity was registered at
/// construction; counting never rebuilds or observes its lazy hit shapes.
#[no_mangle]
pub extern "C" fn f4_viewpoint_clock() {
    if unsafe { (*addr_of_mut!(IO)).is_some() } { io().dirty_counters = true; io().counts.lifecycle += 1.0; }
    super::f4_tail::refresh_mobile_views();
    let unit=f3::viewpoint();
    let count=sim().store.unit.get(unit).expect("current viewpoint").tick_count+1.0;
    f3::viewpoint_count(count);
}
