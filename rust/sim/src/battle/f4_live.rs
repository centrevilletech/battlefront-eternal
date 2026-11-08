//! LiveObjects.update's owned module entry. The phase takes its Store shell
//! and retained work buffers once. Owner services temporarily lend the Store
//! back around recursive hits; no container borrow spans those services.
use crate::world::owner::World as Root;
use super::owner::BattleOwner;
use super::{animated, f4, held_bodies::{self, Bodies, Positions}, live_phase, prepared, props, supply, traps};

use crate::gen::*;
use crate::store::{Ref, StrId};
use crate::units::f3::{self, WorldServices};

use three::{Quaternion, Vector3};

pub(super) struct Io { live: Vec<f64>, tail: Vec<f64>, descriptor: Vec<f64>, handle: Vec<f64>, counters: Vec<f64>, counts: F4Counters, version: u32, pending_publication: bool, dirty_counters: bool }
fn io(battle: &mut BattleOwner) -> &mut Io { battle.io.as_mut().expect("live phase initialized") }
#[derive(Clone, Copy)]
struct Gain { health: StrId, ammo: StrId }
#[derive(Clone)]
struct Animation { handle: F4Handle, original: Ref<Prop>, flag: Option<usize>, index: usize }
pub(super) struct State {
    shell: Option<Store>, local: Option<Ref<Unit>>, body_revision: u64, bodies: Bodies, targets: supply::Targets, supply: supply::Prepared,
    stations: Vec<usize>, regenerators: Vec<usize>, traps: Vec<(usize, F4Handle)>,
    hulls: Vec<Ref<Vehicle>>,
}
pub(super) struct Metadata {
    animations: Vec<Animation>, gains: Vec<Option<Gain>>, grips: Vec<Vec<Vec<Option<u32>>>>,
    sides: Vec<Option<(Ref<Vehicle>, Option<StrId>, Option<StrId>)>>,
}
fn metadata(battle: &BattleOwner) -> &Metadata { battle.metadata.as_ref().expect("prepared live metadata") }
fn metadata_mut(battle: &mut BattleOwner) -> &mut Metadata {
    assert!(battle.live_phase.is_some(), "prepare content outside the live phase");
    battle.metadata.as_mut().expect("prepared live metadata")
}
fn state(battle: &mut BattleOwner) -> &mut State { battle.live_phase.as_mut().expect("idle live owner") }
pub(crate) fn reset(battle: &mut BattleOwner) { battle.live_phase = None; battle.io = None; battle.metadata = None; }
fn args<T>(w: &mut Root, take: impl FnOnce(&[f64], &[f64]) -> T) -> T { let (row, heap) = w.sim.calls.args(); take(row, heap) }
fn return_handle(w: &mut Root, handle: F4Handle) -> Option<&[f64]> {
    handle.put(&mut io(&mut w.battle).handle, &mut Vec::new()); Some(&io(&mut w.battle).handle[..])
}
pub fn f4_live_init_in(w: &mut Root) -> Option<&[f64]> {
    assert!(f3::active_in(&w.units), "canonical unit owner required");
    let setup = args(w, F4LiveSetup::take);
    let store = &w.sim.store;
    let mut bodies = Bodies::new((store.unit.slots() as usize).max(setup.registry_capacity as usize), setup.registry_capacity as usize);
    for (r, _) in store.unit.iter() { bodies.adopt(r); }
    let mut stations = Vec::new(); let mut regenerators = Vec::new(); let mut traps = Vec::new();
    for index in 0..crate::units::f3_props::count_in(&w.units) {
        let handle = crate::units::f3_props::owner_in(w, index);
        if handle.kind == F4ObjectKind::Trap { traps.push((index, handle)); continue; }
        let (_, settings, supply) = f4::station_in(&mut w.battle, &handle);
        if supply.is_some() { stations.push(index); }
        else if settings.kind != props::Kind::Objective && settings.regenerates { regenerators.push(index); }
    }
    let live = vec![0.0; F4LiveInput::WORDS]; let tail = vec![0.0; F4TailInput::WORDS];
    let phase_row = vec![0.0; F4Phase::WORDS]; let handle = vec![0.0; F4Handle::WORDS]; let counters = vec![0.0; F4Counters::WORDS];
    {
        w.battle.io = Some(Io { live, tail, descriptor: phase_row, handle, counters, counts: F4Counters::default(), version: 0, pending_publication: false, dirty_counters: true });
        w.battle.live_phase = Some(State { shell: Some(Store::default()), local: setup.local, body_revision: f3::unit_revision_in(&w.units), bodies,
            targets: supply::Targets::new(setup.unit_capacity as usize, setup.hull_capacity as usize), supply: supply::Prepared::default(),
            stations, regenerators, traps, hulls: Vec::with_capacity(setup.hull_capacity as usize) });
        w.battle.metadata = Some(Metadata { animations: Vec::new(), gains: Vec::new(), grips: Vec::new(), sides: Vec::new() });
    }
    descriptor_words(w)
}
pub fn f4_building_handle_in(w: &mut Root, index: u32) -> Option<&[f64]> { let owner = crate::units::f3_props::owner_in(w, index as usize); return_handle(w, owner) }
pub fn f4_unit_class_in(w: &mut Root) {
    let c = args(w, F4UnitClass::take); let slot = c.def.0 as usize;
    state(&mut w.battle).supply.class(c.def, c.vehicle); let s = metadata_mut(&mut w.battle); s.gains.resize(s.gains.len().max(slot + 1), None);
    s.gains[slot] = Some(Gain { health: c.health_effect, ammo: c.ammo_effect });
}
pub fn f4_weapon_class_in(w: &mut Root) { let weapon = args(w, F4WeaponClass::take); state(&mut w.battle).supply.weapon(weapon.def, weapon.ammunition); }
pub fn f4_hull_class_in(w: &mut Root) {
    let h = args(w, F4HullClass::take); state(&mut w.battle).supply.hull(h.hull,h.vehicle); let s = metadata_mut(&mut w.battle); let slot = h.hull.slot as usize;
    s.sides.resize(s.sides.len().max(slot + 1), None); s.sides[slot] = Some((h.hull, h.side, h.label));
    let capacity = s.sides.len(); let s = state(&mut w.battle); s.targets.reserve(0, capacity);
    s.hulls.reserve(capacity.saturating_sub(s.hulls.len())); f4::live_in(&mut w.battle).prepare_hulls(capacity);
}
pub fn f4_grip_class_in(w: &mut Root) {
    let g = args(w, F4GripClass::take); let s = metadata_mut(&mut w.battle);
    let index = s.animations.iter().position(|a| a.handle == g.animated).expect("prepared animated handle");
    s.grips.resize_with(s.grips.len().max(g.def.0 as usize + 1), Vec::new);
    let class = &mut s.grips[g.def.0 as usize]; class.resize_with(class.len().max(index + 1), Vec::new); class[index] = g.clips;
}
pub fn f4_animated_birth_in(w: &mut Root) -> Option<&[f64]> {
    let input = args(w, F4AnimationSetup::take); let mut animation = prepared::animated(input.birth);
    let index = metadata(&w.battle).animations.len(); animation.state.index = index;
    let handle = f4::live_in(&mut w.battle).animated.adopt(1.0, animation);
    // Body order is fixed at construction. Flags share F2/F3's reservation;
    // door posing writes its byte directly, without a collision export.
    let flag = input.body.and_then(|body| {
        let collision = crate::vehicles::module::collision_world_in(w).expect("prepared animated collision");
        let kernel = collision.kernel_slot(body.slot)?;
        w.collision.state.worlds[f3::collision_world_in(&w.units)].order.iter().position(|&slot| slot as usize == kernel)
    });
    metadata_mut(&mut w.battle).animations.push(Animation { handle: handle.clone(), original: input.prop, flag, index });
    return_handle(w, handle)
}
/// AnimatedProp.state's scalar publication. The body key retained by the
/// native holder is the Unit id, even after that body changes gameplay life.
pub(crate) fn wire_animation_count_in(battle: &BattleOwner) -> usize { metadata(battle).animations.len() }
pub(crate) fn wire_animation_in(battle: &BattleOwner, index: usize) -> Option<[f64; 7]> {
    let original = metadata(battle).animations.get(index)?.handle.clone();
    let handle = f4::live_ref(battle).animated.current(&original)?;
    let prop = f4::live_ref(battle).animated.get(&handle)?;
    if prop.settings.grabs.is_empty() || prop.state.slot < 0 { return None; }
    let value = &prop.state;
    let (id, life, grab, since) = value.victim.as_ref().map_or((0., 0., -1., 0.),
        |v| (v.unit as f64, v.generation, v.grab as f64, v.since));
    Some([value.slot as f64, value.started, value.start_time, id, life, grab, since])
}
/// LogTrap.stateRecord refreshes on any phase/start/direction change, with
/// the authored trap order independent of its position in buildings.
pub(crate) fn wire_trap_count_in(battle: &BattleOwner) -> usize { battle.live_phase.as_ref().expect("prepared live phase").traps.len() }
pub(crate) fn wire_trap_in(battle: &BattleOwner, index: usize) -> Option<[f64; 3]> {
    let handle = battle.live_phase.as_ref().expect("prepared live phase").traps.get(index)?.1.clone(); let value = f4::trap_in(battle, &handle)?;
    let phase = match value.phase { traps::Phase::Down => 0., traps::Phase::Reset => 1., traps::Phase::Armed => 2., traps::Phase::Swing => 3. };
    Some([phase, value.started.unwrap_or(value.tick), u8::from(value.sprung) as f64])
}

pub(crate) fn descriptor_words(w: &mut Root) -> Option<&[f64]> {
    let mut phase = F4Phase { version: io(&mut w.battle).version,
        input_capacity: F4LiveInput::WORDS.max(F4TailInput::WORDS) as u32, ..F4Phase::default() };
    #[cfg(target_arch = "wasm32")]
    bind_descriptor(w, &mut phase);
    let io = io(&mut w.battle); io.dirty_counters = false; io.counts.put(&mut io.counters, &mut Vec::new()); phase.put(&mut io.descriptor, &mut Vec::new()); Some(&io.descriptor[..])
}
// The native publication owns typed banks; facade offsets are bound only
// after the shared phase has completed its counters and changed rows.
#[cfg(target_arch = "wasm32")]
fn bind_descriptor(w: &mut Root, phase: &mut F4Phase) {
    phase.live_input = io(&mut w.battle).live.as_ptr() as u32;
    phase.tail_input = io(&mut w.battle).tail.as_ptr() as u32;
    phase.buildings = f4::live_in(&mut w.battle).buildings.publication.ptr() as u32;
    phase.animated = f4::live_in(&mut w.battle).animated.publication.ptr() as u32;
    phase.traps = f4::live_in(&mut w.battle).traps.publication.ptr() as u32;
    phase.counters = io(&mut w.battle).counters.as_ptr() as u32;
    phase.battle = super::f4_core::publication_ptr(&mut w.battle);
    phase.events = w.sim.events.ptr() as u32;
}
// The retained banks the descriptor names, for a native host that reads them
// by name rather than by address (Context::read_bank); None before f4_live_init.
pub(crate) fn counters_bank_in(w: &Root) -> Option<&[f64]> { w.battle.io.as_ref().map(|io| &io.counters[..]) }
pub(crate) fn live_input_bank_mut_in(w: &mut Root) -> Option<&mut [f64]> { w.battle.io.as_mut().map(|io| &mut io.live[..]) }
pub(crate) fn tail_input_bank_mut_in(w: &mut Root) -> Option<&mut [f64]> { w.battle.io.as_mut().map(|io| &mut io.tail[..]) }
pub(crate) fn buildings_bank_in(w: &Root) -> Option<&[f64]> { w.battle.live.as_ref().map(|live| &live.buildings.publication.words[..]) }
pub(crate) fn animated_bank_in(w: &Root) -> Option<&[f64]> { w.battle.live.as_ref().map(|live| &live.animated.publication.words[..]) }
pub(crate) fn traps_bank_in(w: &Root) -> Option<&[f64]> { w.battle.live.as_ref().map(|live| &live.traps.publication.words[..]) }
/// Only actual foreign barriers and outer returns publish. Native setters mark
/// their rows; an intermediate commit must not overwrite an unread delta.
pub(crate) fn publish_in(w: &mut Root) {
    if crate::world::deferred_in(&w.tick) {return;}
    if w.battle.io.is_none() { return; }
    let rows = f4::live_in(&mut w.battle).flush();
    let words = { let live = f4::live_in(&mut w.battle); live.buildings.publication.words.len() + live.animated.publication.words.len() + live.traps.publication.words.len() };
    record_publication(&mut w.battle, rows, words - 3 * crate::store::block::HEADER);
    let io = io(&mut w.battle);
    // Counters can advance without any changed reader row. Only actual live
    // or battle payloads invalidate the facade epoch at a foreign barrier.
    if io.pending_publication {
        io.version = io.version.wrapping_add(1); io.pending_publication = false;
    } else if !io.dirty_counters {
        // Empty observer barriers keep the same pointers, epoch and counters.
        // Rewriting this descriptor for every aimer would scale with bodies.
        return;
    }
    descriptor_words(w);
}
pub(crate) fn record_publication(battle: &mut BattleOwner, rows: usize, words: usize) {
    if battle.io.is_none() {return;}
    if rows != 0 { io(battle).pending_publication = true; }
    let counts = &mut io(battle).counts; counts.published_rows += rows as f64; counts.output_words += words as f64;
}
pub(crate) fn count_command(battle: &mut BattleOwner) {count_external(battle, F4Command::WORDS);}
pub(crate) fn count_external(battle: &mut BattleOwner, words:usize) {
    if battle.io.is_some() {io(battle).dirty_counters = true;io(battle).counts.commands += 1.0;io(battle).counts.input_words += words as f64;}
}
pub fn f4_live_publish_in(w: &mut Root) -> Option<&[f64]> { super::f4_core::publish_in(w); publish_in(w); descriptor_words(w) }
pub(crate) fn with_store_in<T>(w: &mut Root, store: &mut Store, run: impl FnOnce(&mut Root) -> T) -> T {
    std::mem::swap(store, &mut w.sim.store); let result = run(w); std::mem::swap(store, &mut w.sim.store); result
}
use super::stats::WorldText as Text;

struct Supplies<'w, 'a> { w: &'w mut Root, station: F4Handle, services: WorldServices<'a> }
impl supply::World for Supplies<'_, '_> {
    fn hull_health(&mut self, s: &mut Store, r: Ref<Vehicle>, _: f64, value: f64) {
        with_store_in(self.w, s, |w| crate::units::f3_hulls::mutate_in(w, r, value, None, self.services.reborrow()));
    }
    fn hull_ammo(&mut self, s: &mut Store, r: Ref<Vehicle>, life: f64, amount: f64) -> bool {
        with_store_in(self.w, s, |w| crate::vehicles::module::f2::firing::supply_station_in(w, r, life, amount))

    }
    fn changed(&mut self, _: &mut Store, key: supply::Key) {
        match key { supply::Key::Unit { reference, .. } => f3::changed_in(self.w, reference),
            supply::Key::Hull { reference, .. } => crate::vehicles::module::f2::mark_changed_in(&mut self.w.fleet, reference) }
    }
    fn served(&mut self, _: &props::Station, _: supply::Key, class: crate::store::DefId, p: Vector3, gave: u8) {
        let gain = metadata(&self.w.battle).gains[class.0 as usize].expect("prepared gain effects");
        if gave & 1 != 0 { F4Effect { name: gain.health, position: array(p) }.emit(&mut self.w.sim.events); }
        if gave & 2 != 0 { F4Effect { name: gain.ammo, position: array(p) }.emit(&mut self.w.sim.events); }
    }
    fn sound(&mut self, _: &props::Station, first: Option<supply::Key>) {
        // PowerupStation.update identifies the first soldier actually supplied.
        // A hull-only pulse has no soldier and must preserve its null recipient.
        let unit=match first {Some(supply::Key::Unit {reference,..})=>Some(reference),_=>None};
        F4Serve { station: self.station.clone(), unit }.emit(&mut self.w.sim.events);
    }
}
struct Animator<'w, 'a, 'b> { w: &'w mut Root, store: &'a mut Store, positions: &'a Positions, binding: &'a Animation, services: WorldServices<'b> }
impl Animator<'_, '_, '_> {
    fn stage(&mut self, prop: &animated::Animated) {
        f4::live_in(&mut self.w.battle).animated.stage_with(&self.binding.handle, |row, heap| live_phase::write_animated(prop, row, heap));
        if let (Some(order), Some(enabled)) = (self.binding.flag, prop.body_enabled) {
            if let Some(flag) = self.w.movement.flags_mut().get_mut(order) { *flag = u8::from(enabled); }
        }
    }
    fn clip(&self, body: &animated::Body, grab: usize) -> Option<u32> {
        let u = self.store.unit.get(body.reference?)?;
        metadata(&self.w.battle).grips.get(u.class_def.0 as usize)?.get(self.binding.index)?.get(grab).copied().flatten()
    }
}
impl animated::World for Animator<'_, '_, '_> {
    fn near(&mut self, _: &animated::Animated, settings: &animated::Settings, _: &[Vector3]) -> bool { self.positions.near(self.store, settings) }
    fn refresh_body(&mut self, p: &animated::Animated, b: &mut animated::Body) { held_bodies::refresh(self.store, p, self.binding.original, b); }
    fn body_changed(&mut self, p: &animated::Animated, b: &animated::Body) {
        self.stage(p);
        let Some(key) = held_bodies::commit(self.store, self.binding.original, b) else { return };
        f3::changed_in(self.w, key.reference());
        F4Held { key, prop: b.held.as_ref().map(|_| self.binding.handle.clone()), grab: b.held.as_ref().map_or(0, |h| h.grab as u32),
            since: b.held.as_ref().map_or(0.0, |h| h.since), frame: b.held.as_ref().and_then(|h| h.frame.as_ref()).map(|m| m.elements),
            time: b.held.as_ref().and_then(|h| h.time) }.emit(&mut self.w.sim.events);
    }
    fn has_rider_clip(&mut self, _: &animated::Animated, b: &animated::Body, grab: usize) -> bool { self.clip(b, grab).is_some() }
    fn sound(&mut self, p: &animated::Animated, name: StrId, position: Vector3) { self.stage(p); F4Sound { name, position: array(position) }.emit(&mut self.w.sim.events); }
    fn let_go(&mut self, p: &animated::Animated, b: &mut animated::Body) {
        self.store.unit.get_mut(b.reference.expect("held Unit")).unwrap().let_go();
        self.refresh_body(p, b); self.body_changed(p, b);
    }
    fn damage(&mut self, p: &animated::Animated, b: &mut animated::Body, amount: f64) {
        self.stage(p); let r = b.reference.expect("swallowed Unit");
        with_store_in(self.w, self.store, |w| crate::combat::live::damage_in(w, r, amount, Attacker::Null, self.services.reborrow()));
    }
    fn remove(&mut self, p: &animated::Animated, b: &mut animated::Body) {
        self.stage(p); let r = b.reference.expect("removed Unit");
        with_store_in(self.w, self.store, |w| f3::remove_native_in(w, r, self.services.reborrow()));
    }
    fn rider_root(&mut self, _: &animated::Animated, b: &animated::Body, _: StrId, time: f64) -> Option<(Vector3, Quaternion)> {
        crate::units::live_pose::held_root_in(self.w, b.reference?, self.clip(b, b.held.as_ref()?.grab)?, time)
    }
}
struct Logs<'w, 'a, 'b> { w: &'w mut Root, store: &'a mut Store, handle: F4Handle, index: usize,  services: WorldServices<'b> }
impl Logs<'_, '_, '_> { fn commit(&mut self, t: &traps::Trap) { f4::put_trap_in(&mut self.w.battle, &self.handle, *t); } }
impl traps::World for Logs<'_, '_, '_> {
    type Vehicle = Ref<Vehicle>;
    fn alive(&mut self, r: Self::Vehicle) -> bool { self.store.vehicle.get(r).is_some_and(|v| v.alive) }
    fn sphere(&mut self, r: Self::Vehicle) -> ([f64; 3], f64) {
        with_store_in(self.w, self.store, |w| { let (p, radius) = crate::units::f3_hulls::sphere_in(w, r, true); (array(p), radius) })
    }
    fn side(&mut self, r: Self::Vehicle) -> Option<StrId> { metadata(&self.w.battle).sides.get(r.slot as usize).copied().flatten().filter(|(key, _, _)| *key == r).and_then(|(_, side, _)| side) }
    fn clear_attacker(&mut self, t: &mut traps::Trap, r: Self::Vehicle) { self.commit(t); self.store.vehicle.get_mut(r).unwrap().last_attacker = OrdnanceOwner::Null; crate::vehicles::module::f2::mark_changed_in(&mut self.w.fleet, r); }
    fn set_health(&mut self, t: &mut traps::Trap, r: Self::Vehicle, value: f64) {
        self.commit(t); with_store_in(self.w, self.store, |w| crate::units::f3_hulls::mutate_in(w, r, value, None, self.services.reborrow()));
        *t = f4::trap_in(&mut self.w.battle, &self.handle).expect("live trap after hull death");
    }
    fn set_dying(&mut self, _: &mut traps::Trap, r: Self::Vehicle, value: f64) { self.store.vehicle.get_mut(r).unwrap().dying = value; crate::vehicles::module::f2::mark_changed_in(&mut self.w.fleet, r); }
    fn hide_root(&mut self, _: &mut traps::Trap, r: Self::Vehicle) { self.store.vehicle.get_mut(r).unwrap().visible = false; crate::vehicles::module::f2::mark_changed_in(&mut self.w.fleet, r); }
    fn show(&mut self, t: &mut traps::Trap, built: bool) {
        self.commit(t); with_store_in(self.w, self.store, |w| crate::units::f3_props::trap_show_in(w, self.index, t, built, self.services.reborrow()));
        *t = f4::trap_in(&mut self.w.battle, &self.handle).expect("shown trap");
    }
    fn sound(&mut self, t: &mut traps::Trap, name: StrId, at: [f64; 3]) { self.commit(t); F4Sound { name, position: at }.emit(&mut self.w.sim.events); }
    fn effect(&mut self, t: &mut traps::Trap, name: StrId, at: [f64; 3]) { self.commit(t); F4Effect { name, position: at }.emit(&mut self.w.sim.events); }
}
fn array(p: Vector3) -> [f64; 3] { [p.x, p.y, p.z] }

pub fn f4_live_in(w: &mut Root) -> Option<&[f64]> {
    let before = crate::allocations::snapshot(); crate::allocations::begin();
    let input = F4LiveInput::take(&io(&mut w.battle).live, &[]);
    let outer = f3::external_in(w);
    f3::with_services_in(w, |w, services| {
        let services = services.expect("native live services");
        let mut state = { w.battle.live_phase.take().expect("nonrecursive live phase") };
        let mut store = std::mem::replace(&mut w.sim.store, state.shell.take().expect("retained store shell"));
        state.advance(w, &mut store, &input, services);
        state.shell = Some(std::mem::replace(&mut w.sim.store, store)); { w.battle.live_phase = Some(state); }
    });
    { let io = io(&mut w.battle); io.counts.live += 1.0; io.counts.input_words += F4LiveInput::WORDS as f64; }
    outer.finish(w, true);
    crate::allocations::end(); let used = crate::allocations::snapshot().since(before);
    io(&mut w.battle).counts.allocations = used.calls as u32; io(&mut w.battle).counts.reallocations = used.reallocs as u32; io(&mut w.battle).counts.allocation_bytes = used.bytes as u32;
    descriptor_words(w)
}

/// External writes validate the complete F4 identity before any owner service.
/// Native combat calls the same services directly inside the phase.
pub fn f4_live_command_in(w: &mut Root) -> u32 {
    let command = args(w, F4Command::take); count_command(&mut w.battle);
    let current = match command.target.kind {
        F4ObjectKind::Building => f4::live_in(&mut w.battle).buildings.current(&command.target),
        F4ObjectKind::Trap => f4::live_in(&mut w.battle).traps.current(&command.target),
        F4ObjectKind::Animated => f4::live_in(&mut w.battle).animated.current(&command.target), _ => None,
    };
    if current.as_ref() != Some(&command.target) { return 0; }
    if command.op == F4CommandOp::Play {
        let accepted = f4::live_in(&mut w.battle).play(&command.target, command.value as i32, command.flag, command.tick);
        if accepted {
            let enabled = f4::live_in(&mut w.battle).animated.get(&command.target).and_then(|a| a.state.body_enabled);
            let flag = metadata(&w.battle).animations.iter().find(|a| a.handle == command.target).and_then(|a| a.flag);
            if let (Some(flag), Some(enabled)) = (flag, enabled) { if let Some(flag) = w.movement.flags_mut().get_mut(flag) { *flag = u8::from(enabled); } }
            super::f4_core::publish_in(w); publish_in(w);
        }
        return u32::from(accepted);
    }
    if !matches!(command.op, F4CommandOp::Health | F4CommandOp::Damage) { return 0; }
    let index = (0..crate::units::f3_props::count_in(&w.units)).find(|&i| crate::units::f3_props::owner_in(w, i) == command.target);
    let Some(index) = index else { return 0 };
    let outer = f3::external_in(w);
    f3::with_services_in(w, |w, services| crate::units::f3_props::command_hit_in(w, index, command.value,
        (command.op == F4CommandOp::Damage).then_some(command.attacker), command.damage_info, services.expect("native prop command")));
    outer.finish(w, true); 1
}

/// Prepared hull display names are shared with the tail's mounted statistics.
pub(crate) fn hull_label(battle: &mut BattleOwner, r: Ref<Vehicle>) -> Option<StrId> {
    metadata(battle).sides.get(r.slot as usize).copied().flatten().filter(|(h,_,_)| *h == r).and_then(|(_,_,label)|label)
}

/// The shape reader can run during recursive damage while an animated rig
/// is checked out. Its staged scalar victim remains readable at that point;
/// immutable clip bindings live outside the phase's movable work buffers.
pub(crate) fn held_clip_in(battle: &mut BattleOwner, r: Ref<Unit>, unit: &Unit) -> Option<u32> {
    let Nullable::Value(held) = &unit.held else { return None };
    let bindings = battle.metadata.as_ref()?;
    let binding = bindings.animations.iter().find(|a| a.original == held.prop)?.clone();
    let handle = f4::live_in(battle).animated.current(&binding.handle)?;
    let grab = if let Some(actor) = f4::live_in(battle).animated.get(&handle) {
        let v = actor.state.victim.as_ref()?;
        if v.unit != r.slot as usize || v.generation != unit.generation { return None; }
        v.grab
    } else {
        let row = f4::live_in(battle).animated.staged(&handle)?;
        let view = F4AnimatedView::take(row, &[]);
        if !view.has_victim || view.victim.unit != r.slot as f64 || view.victim.generation != unit.generation { return None; }
        view.victim.grab as usize
    };
    metadata(battle).grips.get(unit.class_def.0 as usize)?.get(binding.index)?.get(grab).copied().flatten()
}

/// A prepared animation owns held geometry even when a class has no grip clip.
pub(crate) fn owns_held_in(battle: &mut BattleOwner, prop:Ref<Prop>)->bool {
    { battle.metadata.as_ref() }.is_some_and(|m|m.animations.iter().any(|a|a.original==prop))
}

pub(crate) fn tail_input(battle: &mut BattleOwner) -> F4TailInput {F4TailInput::take(&io(battle).tail,&[])}
pub(crate) fn count_tail(w: &mut Root, commands:usize,before:crate::allocations::Counts) {
    let used=crate::allocations::snapshot().since(before);let counts=&mut io(&mut w.battle).counts;
    counts.tail+=1.0;counts.input_words+=(F4TailInput::WORDS+commands*HsCommand::WORDS) as f64;
    counts.allocations=used.calls as u32;counts.reallocations=used.reallocs as u32;counts.allocation_bytes=used.bytes as u32;
    descriptor_words(w);
}

pub(crate) fn count_hook(battle: &mut BattleOwner) {
    io(battle).dirty_counters = true;
    let counts=&mut io(battle).counts;counts.hooks+=1.0;counts.output_words+=F4AIHook::WORDS as f64;
}

/// Game.step's pre-AI viewpoint count. The body identity was registered at
/// construction; counting never rebuilds or observes its lazy hit shapes.
pub fn f4_viewpoint_clock_in(w: &mut Root) {
    if w.battle.io.is_some() { io(&mut w.battle).dirty_counters = true; io(&mut w.battle).counts.lifecycle += 1.0; }
    super::f4_tail::refresh_mobile_views_in(w);
    let unit=f3::viewpoint_in(&w.units);
    let count=w.sim.store.unit.get(unit).expect("current viewpoint").tick_count+1.0;
    f3::viewpoint_count_in(w, count);
}

impl State {
    // LiveObjects.update borrows only its detached frame, metadata and the
    // canonical Store shell. Each supply/trap/animation service returns that
    // shell at its original synchronous hook, before continuing this frame.
    fn advance(&mut self, w: &mut Root, store: &mut Store, input: &F4LiveInput, mut services: WorldServices<'_>) {
        // Ordinary ticks retain structural membership. A new native body or
        // class replacement admits its integer slot before a prop can see it.
        let revision = f3::unit_revision_in(&w.units);
        if self.body_revision != revision {
            for (r, _) in store.unit.iter() { if self.bodies.frames.get(r.slot as usize).and_then(|b| b.reference) != Some(r) { self.bodies.adopt(r); } }
            self.body_revision = revision;
        }
        if !self.stations.is_empty() {
            let driving = self.local.and_then(|r| store.unit.get(r)).and_then(|u| u.seat).and_then(|r| store.seat_lease.get(r)).is_some_and(|l| l.vehicle.is_some());
            self.targets.begin(store, self.local, driving, f3::built_in(&w.units), &store.vehicles.vehicles);
        }
        for &index in &self.stations {
            if crate::units::f3_props::needs_regeneration_in(w, index) {
                with_store_in(w, store, |w| crate::units::f3_props::regenerate_in(w, index, input.dt, services.reborrow()));
            }
            let handle = crate::units::f3_props::owner_in(w, index); let (mut station, _, supply) = f4::station_in(&mut w.battle, &handle);
            self.targets.pulse(&mut station, input.dt, &supply.expect("station supply"), store, &self.supply,
                &mut Supplies { w, station: handle.clone(), services: services.reborrow() });
            f4::put_station(&mut w.battle, &handle, station);
        }
        for &index in &self.regenerators {
            if crate::units::f3_props::needs_regeneration_in(w, index) {
                with_store_in(w, store, |w| crate::units::f3_props::regenerate_in(w, index, input.dt, services.reborrow()));
            }
        }
        with_store_in(w, store, |w| crate::vehicles::module::f2::live_seats::idle_turrets_in(w, input.dt));
        if !metadata(&w.battle).animations.is_empty() { self.bodies.positions.begin(store, self.local, f3::built_in(&w.units)); }
        for index in 0..metadata(&w.battle).animations.len() {
            // Keep the scalar binding off the root while held-body callbacks
            // read the canonical clip and victim tables recursively.
            let binding = metadata(&w.battle).animations[index].clone();
            self.bodies.refresh_order(store);
            let Some(mut prop) = f4::live_in(&mut w.battle).animated.checkout(&binding.handle) else { continue };
            let mut world = Animator { w, store: store, positions: &self.bodies.positions, binding: &binding, services: services.reborrow() };
            prop.step(input.dt, input.tick, &[], &mut self.bodies.frames, &self.bodies.order, &mut world); world.stage(&prop.state);
            assert!(f4::live_in(&mut w.battle).animated.restore(&binding.handle, prop).is_ok(), "live animated row");
        }
        for (index, handle) in &self.traps {
            self.hulls.clear(); self.hulls.extend_from_slice(&store.vehicles.vehicles);
            let (mut trap, settings, mut victims) = f4::take_trap(&mut w.battle, handle);
            trap.step_with_scratch(input.dt, input.tick, &settings, &Text, &self.hulls,
                &mut Logs { w, store: store, handle: handle.clone(), index: *index, services: services.reborrow() }, &mut victims);
            f4::put_trap_in(&mut w.battle, handle, trap); f4::return_victims(&mut w.battle, handle, victims);
        }
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;

    #[no_mangle]
    pub extern "C" fn f4_live_init() -> *const f64 { f4_live_init_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_building_handle(index: u32) -> *const f64 { f4_building_handle_in(crate::abi::facade::world(), index).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_unit_class() { f4_unit_class_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_weapon_class() { f4_weapon_class_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_hull_class() { f4_hull_class_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_grip_class() { f4_grip_class_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_animated_birth() -> *const f64 { f4_animated_birth_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_live_publish() -> *const f64 { f4_live_publish_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_live() -> *const f64 { f4_live_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_live_command() -> u32 { f4_live_command_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_viewpoint_clock() { f4_viewpoint_clock_in(crate::abi::facade::world()) }
}

impl super::stats::TextWorld for Logs<'_, '_, '_> { fn text_nonempty(&self,id:StrId)->bool { !self.w.sim.strings.units(id).is_empty() } }
