//! EntityWorldCapture.capture over canonical slabs. Protocol item slots and
//! known tombstones belong to the capture session, independently of slab reuse.
use crate::gen::{Air, Item, Member, Store, UnitBody, Vehicle, WireRecord,
    WireRecordKind as Kind, WireRecordState, WireSeatAngles, WireStoreSetup};
use crate::store::{Ref, Strings};
use crate::combat::defs::{K, WeaponDefs};
use super::{codec::Result, shots::Counters};
use jsrt::{js_max, js_min, js_round, num_or};

/// The owning phases supply borrowed, scalar views of their retained sidecars.
/// No view samples geometry, publishes mirrors, calls JS or advances a clock.
pub trait World {
    fn walk(&self, hull: Ref<Vehicle>) -> (f64, f64);
    fn remote_count(&self) -> usize;
    fn remote(&self, slot: usize) -> Option<(u32, Ref<crate::gen::ReconDroid>)>;
    fn turret_count(&self) -> usize;
    fn turret(&self, index: usize) -> Option<Turret>;
}
#[derive(Clone, Copy)]
pub struct Turret {
    pub reference: Ref<crate::gen::Turret>,
    pub id: f64, pub life: f64, pub building: f64, pub state: crate::store::StrId,
    pub team: f64, pub health: f64, pub yaw: f64, pub pitch: f64,
    pub gun_yaw: f64, pub gun_pitch: f64,
}
#[derive(Clone, Copy, Default)]
struct Known { id: f64, life: f64 }
#[derive(Clone, Copy)]
struct ItemSlot {
    // EntityItemCapture numbers the visible lives it actually sampled. A
    // physical item can be born and retire entirely between publications;
    // that unseen life must not consume a protocol slot or generation.
    source: Option<Ref<Item>>, life: f64, present: bool, active: bool,
    raw: Option<[f64; 10]>, sample: [f64; 10], bytes: [u8; 42], definition: f64,
}
impl Default for ItemSlot {
    fn default() -> Self { Self { source: None, life: 0., present: false, active: false,
        raw: None, sample: [0.; 10], bytes: [0; 42], definition: 0. } }
}
#[derive(Clone, Copy, Default)]
struct RemoteSlot {
    // EntityRemoteCapture keeps its last record even after the native launch
    // slot is vacant. Retirement is different from an inactive death sample.
    present: bool, life: f64, active: bool, definition: f64, team: f64,
    position: [f64; 3], velocity: [f64; 3], yaw: f64, pitch: f64, health: f64,
}

/// Reserve both outer rows and nested seat storage before the first sample.
/// Rows stay initialized, so shortening a publication cannot drop buffers that
/// the next publication would have to allocate again.
pub struct Capture {
    classes: Vec<u32>, weapon_names: Vec<u32>, posts: Vec<u32>, transport_posts: Vec<u32>, weapons: Vec<u32>, items: Vec<u32>, remotes: Vec<u32>,
    rows: Vec<WireRecord>, count: usize, known: Vec<Known>, next_known: Vec<Known>,
    item_slots: Vec<ItemSlot>, remote_slots: Vec<RemoteSlot>, remote_order: Vec<usize>, max_entities: usize, max_seats: usize,
}
pub(crate) fn definitions(values: &[crate::gen::WireStoreDefinition]) -> Result<Vec<u32>> {
    // Definition handles are dense module-local integers. Resolve them once,
    // rather than finding authored names for each mine or weapon every tick.
    // Missing entries remain zero and reject only if an actual sample needs
    // them, preserving captures of maps with extra, unfielded side content.
    let length = values.iter().map(|v| v.definition as usize).max().map_or(0, |n| n + 1);
    if length > 1_048_576 { return Err("invalid wire definition capacity"); }
    let mut ids = vec![0; length];
    for entry in values {
        if entry.id == 0 || entry.id > 65535 { return Err("invalid entity registry id"); }
        ids[entry.definition as usize] = entry.id;
    }
    Ok(ids)
}
// transportPosts names the authored pad, independently of the hull class
// currently occupying it. Parse those immutable names once at attachment.
fn transport_ids(values: &[crate::gen::WireStoreName], strings: &Strings) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    for value in values {
        let name = strings.units(value.name); let prefix = b"transport-";
        if name.len() <= prefix.len() || !name[..prefix.len()].iter().zip(prefix).all(|(&a, &b)| a == b as u16) { continue; }
        let mut index = 0usize;
        for &c in &name[prefix.len()..] {
            if !(48..=57).contains(&c) { return Err("invalid transport post name"); }
            index = index.checked_mul(10).and_then(|n| n.checked_add((c - 48) as usize)).filter(|&n| n <= 65535).ok_or("invalid transport post name")?;
        }
        out.resize(out.len().max(index + 1), 0); out[index] = value.id;
    }
    Ok(out)
}
fn required(ids: &[u32], key: u32) -> Result<f64> {
    // Protocol zero means no weapon/definition; it cannot identify content.
    // A stale or unregistered native handle must never silently become zero.
    ids.get(key as usize).filter(|&&id| id != 0).map(|&id| id as f64)
        .ok_or("entity registry prerequisite missing")
}
fn name_ids(values: &[crate::gen::WireStoreName], strings: &Strings) -> Result<Vec<u32>> {
    let mut classes = vec![0; strings.list.len()];
    for entry in values {
        let slot = classes.get_mut(entry.name.0 as usize).ok_or("invalid wire class")?;
        if entry.id == 0 || entry.id > 65535 { return Err("invalid entity registry id"); }
        *slot = entry.id;
    }
    Ok(classes)
}
fn health(value: f64) -> Result<f64> {
    // captureEntitySoldier/Vehicle saturate heroes' full-precision health to
    // the wire's sixteen bits. Nonfinite health is a capture fault, not a
    // reason to unwind through the simulation or stop the next world tick.
    if !value.is_finite() { return Err("entity health is not a number"); }
    Ok(js_round(js_min(65535., js_max(0., value))))
}
impl Capture {
    pub fn new(setup: &WireStoreSetup, strings: &Strings) -> Result<Self> {
        // These are cold reservation bounds, independently checked after the
        // generated descriptor guard. Live excess returns a Result below;
        // it cannot force a hidden grow on a slow machine's frame path.
        if setup.item_capacity > 256 || setup.remote_capacity > 4096 || setup.entity_capacity > 512
            || setup.record_capacity > 4096 || setup.seat_capacity > 255 {
            return Err("invalid wire capture capacity");
        }
        let classes = name_ids(&setup.classes, strings)?;
        let mut rows = Vec::with_capacity(setup.record_capacity as usize);
        for _ in 0..setup.record_capacity {
            rows.push(WireRecord { seats: Some(Vec::with_capacity(setup.seat_capacity as usize)), ..Default::default() });
        }
        Ok(Self { classes, transport_posts: transport_ids(&setup.posts, strings)?, weapon_names: name_ids(&setup.weapon_names, strings)?, posts: name_ids(&setup.posts, strings)?, weapons: definitions(&setup.weapons)?, items: definitions(&setup.items)?,
            remotes: definitions(&setup.remotes)?, rows, count: 0,
            known: Vec::with_capacity(setup.entity_capacity as usize),
            next_known: Vec::with_capacity(setup.entity_capacity as usize),
            item_slots: vec![ItemSlot::default(); setup.item_capacity as usize],
            remote_slots: vec![RemoteSlot::default(); setup.remote_capacity as usize],
            remote_order: Vec::with_capacity(setup.remote_capacity as usize),
            max_entities: setup.entity_capacity as usize, max_seats: setup.seat_capacity as usize })
    }
    pub(crate) fn class_id(&self, name: crate::store::StrId) -> f64 { self.classes.get(name.0 as usize).copied().unwrap_or(0) as f64 }
    pub(crate) fn weapon_name_id(&self, name: crate::store::StrId) -> f64 { self.weapon_names.get(name.0 as usize).copied().unwrap_or(0) as f64 }
    pub(crate) fn transport_post_id(&self, index: f64) -> Option<f64> {
        if !index.is_finite() || index < 0. || index.fract() != 0. || index > 65535. { return None; }
        required(&self.transport_posts, index as u32).ok()
    }
    pub(crate) fn post_id(&self, name: crate::store::StrId) -> Option<f64> { required(&self.posts, name.0).ok() }
    pub(crate) fn weapon_id(&self, definition: u32) -> Option<f64> { required(&self.weapons, definition).ok() }
    pub(crate) fn remote_id(&self, definition: u32) -> Option<f64> { required(&self.remotes, definition).ok() }
    pub fn records(&self) -> &[WireRecord] { &self.rows[..self.count] }
    pub(crate) fn seat_capacity(&self) -> usize { self.max_seats }
    /// Player admission may prepare another class after a descriptor was
    /// requested. Rebind cold definition handles without resetting any wire
    /// item generation, known order or retained terminal sample.
    pub fn rebind(&mut self, setup: &WireStoreSetup, strings: &Strings) -> Result<()> {
        if setup.item_capacity as usize != self.item_slots.len() || setup.remote_capacity as usize != self.remote_slots.len()
            || setup.entity_capacity as usize != self.max_entities || setup.record_capacity as usize != self.rows.len()
            || setup.seat_capacity as usize != self.max_seats { return Err("wire capture capacity changed"); }
        let classes = name_ids(&setup.classes, strings)?; let weapons = definitions(&setup.weapons)?;
        let items = definitions(&setup.items)?; let remotes = definitions(&setup.remotes)?;
        let weapon_names = name_ids(&setup.weapon_names, strings)?; let posts = name_ids(&setup.posts, strings)?;
        self.transport_posts = transport_ids(&setup.posts, strings)?;
        self.classes = classes; self.weapon_names = weapon_names; self.posts = posts; self.weapons = weapons; self.items = items; self.remotes = remotes;
        Ok(())
    }
    fn push(&mut self, value: WireRecord) -> Result<&mut WireRecord> {
        let row = self.rows.get_mut(self.count).ok_or("semantic record capacity exceeded")?;
        self.count += 1;
        // Move only the retained seat allocation across scalar replacement.
        // Non-mount rows ignore it; mount writes fill it below in seat order.
        let mut seats = row.seats.take().unwrap(); seats.clear();
        *row = value; row.seats = Some(seats);
        Ok(row)
    }
    fn seen(&mut self, id: f64, life: f64) -> Result<()> {
        // EntityWorldCapture replaces its known Map with this tick's live
        // insertion order, followed by prior tombstones. Reusing the previous
        // vector in place would change later tombstone release/LIFO order.
        if let Some(row) = self.next_known.iter_mut().find(|r| r.id == id) {
            row.life = life;
        } else {
            if self.next_known.len() >= self.max_entities { return Err("semantic fixed-roster capacity exceeded"); }
            self.next_known.push(Known { id, life });
        }
        Ok(())
    }
    /// Items are admitted in source insertion order into the first free wire
    /// slot. Reclaim absent refs before admitting replacements, as item_begin
    /// did, even when the generated slab reused a lower physical index.
    /// The combat registry's id for an item definition, 0 for one the
    /// registry lacks.
    pub(crate) fn item_id(&self, def: u32) -> f64 {
        self.items.get(def as usize).copied().filter(|&id| id != 0).map_or(0.0, f64::from)
    }
    fn capture_items(&mut self, store: &Store) -> Result<()> {
        for slot in &mut self.item_slots {
            if slot.source.is_some_and(|r| !store.items.items.contains(&r)
                || store.item.get(r).is_none_or(|item| item.gone == Some(true))) {
                slot.source = None; slot.active = false; slot.raw = None;
            }
        }
        for &reference in &store.items.items {
            let Some(item) = store.item.get(reference).filter(|i| i.gone != Some(true)) else { continue };
            let index = self.item_slots.iter().position(|s| s.source == Some(reference))
                .or_else(|| self.item_slots.iter().position(|s| s.source.is_none()))
                .ok_or("item capture capacity exceeded")?;
            let slot = &mut self.item_slots[index];
            if slot.source.is_none() {
                // A generated slab generation qualifies an object identity.
                // The independent protocol generation advances on admission,
                // including when a pooled physical slot is reused immediately.
                if slot.life == 4294967295. { return Err("item slot generation exhausted"); }
                slot.life += 1.; slot.source = Some(reference);
            }
            let raw = [item.position[0], item.position[1], item.position[2], item.velocity[0],
                item.velocity[1], item.velocity[2], item.yaw, item.up[0], item.up[1], item.up[2]];
            if slot.active && slot.raw == Some(raw) { continue; }
            slot.raw = Some(raw);
            let definition = required(&self.items, item.def.0)?;
            let bytes = item_bytes(index, slot.life, definition, raw)?;
            // item_commit retains the earlier full-precision sample when its
            // fine semantic bytes are equal. Snapshot cells use a different
            // quantizer, so simply forwarding the newest raw values is wrong.
            if !slot.active || slot.bytes != bytes {
                slot.sample = raw; slot.bytes = bytes; slot.definition = definition;
            }
            slot.present = true; slot.active = true;
        }
        for index in 0..self.item_slots.len() {
            let slot = self.item_slots[index];
            if !slot.present { continue; }
            if !slot.active {
                // An inactive semantic record has no pose or definition.
                // Carrying the dead item's previous pose here changes the
                // snapshot entry that repairs a client after packet loss.
                self.push(WireRecord { kind: Kind::Item, id: index as f64, generation: slot.life, ..Default::default() })?;
                continue;
            }
            let p = slot.sample;
            self.push(WireRecord { kind: Kind::Item, id: index as f64, generation: slot.life,
                active: slot.active, definition_id: Some(slot.definition), position: Some([p[0], p[1], p[2]]),
                velocity: Some([p[3], p[4], p[5]]), yaw: Some(p[6]), up: Some([p[7], p[8], p[9]]), ..Default::default() })?;
        }
        Ok(())
    }
    /// The only publication traversal. It reads rows at the completed tick;
    /// lazy hit shapes are deliberately absent from this producer.
    pub fn capture(&mut self, store: &Store, defs: &WeaponDefs, world: &impl World, shots: &Counters) -> Result<()> {
        self.count = 0;
        self.next_known.clear();
        self.capture_items(store)?;
        // Network slots retain their admitted generations even after the owner
        // leaves. Dead rows remain inactive until the authority reuses a slot.
        if world.remote_count() > self.remote_slots.len() { return Err("remote capture capacity exceeded"); }
        for index in 0..world.remote_count() {
            let Some((generation, reference)) = world.remote(index) else { continue };
            let Some(droid) = store.recon_droid.get(reference) else { continue };
            // Capture scans slots in numeric order, but its retained Map is
            // ordered by first observation. Outstanding reservations can make
            // a lower slot appear only after a higher one was already sampled.
            if !self.remote_slots[index].present { self.remote_order.push(index); }
            self.remote_slots[index] = RemoteSlot { present: true, life: generation as f64, active: !droid.dead,
                definition: if droid.dead { 0. } else { required(&self.remotes, droid.class)? }, team: droid.team,
                position: droid.position, velocity: droid.velocity, yaw: droid.yaw, pitch: droid.pitch,
                health: js_max(0., js_min(65535., js_round(droid.health))) };
        }
        for order in 0..self.remote_order.len() {
            let index = self.remote_order[order];
            let d = self.remote_slots[index];
            if !d.present { continue; }
            if !d.active {
                self.push(WireRecord { kind: Kind::Remote, id: index as f64, generation: d.life, ..Default::default() })?;
            } else {
                self.push(WireRecord { kind: Kind::Remote, id: index as f64, generation: d.life,
                    active: true, definition_id: Some(d.definition), team: Some(d.team),
                    position: Some(d.position), velocity: Some(d.velocity), yaw: Some(d.yaw),
                    pitch: Some(d.pitch), health: Some(d.health), ..Default::default() })?;
            }
        }
        // Mixed registry order includes corpses. A removed pooled body still
        // has a row but no longer has an entity record in this publication.
        for member in &store.units.list {
            let Member::Unit(reference) = member else { continue };
            let Some(unit) = store.unit.get(*reference).filter(|u| !u.removed) else { continue };
            let current = current(&unit.combat.weapons, unit.combat.selected[0], 0., defs);
            // Arsenal.current interprets selected[0] within its channel, not
            // within the complete weapon vector. The base class still names
            // a charging weapon even when its current level has changed.
            let weapon = current.map(|w| required(&self.weapons, w.def.0)).transpose()?.unwrap_or(0.);
            let charge = if unit.dead { 0. } else { current.filter(|w| w.charging).map_or(0., |w| {
                let most = num_or(defs.class(w.def).base().get(K::MaxPressedTime).num(), 0.);
                if most > 0. { js_max(1., js_min(63., js_round((w.held / most) * 63.))) } else { 0. }
            }) };
            let mut record = WireRecord { kind: Kind::Soldier, id: reference.slot as f64,
                generation: unit.generation, class_id: Some(required(&self.classes, unit.class_name.0)?),
                team: Some(unit.team), health: Some(health(unit.combat.health)?), weapon,
                fire_counter: Some(shots.counter(reference.slot as f64, unit.generation)), charge: Some(charge), ..Default::default() };
            match &unit.soldier {
                UnitBody::Soldier(s) => {
                    record.position = Some(s.position); record.velocity = Some(s.velocity);
                    record.yaw = Some(s.aim_yaw); record.pitch = Some(s.aim_pitch); record.posture = s.posture as u8 as f64;
                    // Death takes precedence over a dive; thrown corpses keep
                    // their separate state. These are semantic protocol tags,
                    // deliberately distinct from the native Air discriminant.
                    record.state = WireRecordState::F64(if unit.dead { if s.air == Some(Air::Thrown) { 8. } else { 7. } }
                        else if jsrt::num_truthy(s.dive) { if s.dive > 0. { 5. } else { 6. } }
                        else { s.air.map_or(0., |air| air as u8 as f64 + 1.) });
                }
                UnitBody::Droideka(d) => {
                    // Droideka.posture is a getter over fold mode. Its body
                    // dead/shield flags remain separate from Unit.dead, and
                    // animation values retain their original signed-zero bits.
                    record.kind = Kind::Droideka; record.position = Some(d.position); record.velocity = Some(d.velocity);
                    record.yaw = Some(d.aim_yaw); record.pitch = Some(d.aim_pitch);
                    record.posture = if d.mode >= crate::rig::droideka::FOLD as f64 { 1. } else { 0. };
                    record.state = WireRecordState::F64(if unit.dead { 7. } else { 0. });
                    record.mode = d.mode; record.reverse = d.reverse; record.dead = d.dead;
                    record.shield_on = d.shield.as_ref().is_some_and(|s| s.on);
                    record.shield = d.shield.as_ref().map_or(0., |s| num_or(s.value, 0.));
                    record.time = d.time; record.roll = d.roll; record.lean = d.lean; record.legs = d.legs;
                }
            }
            self.seen(record.id, record.generation)?;
            self.push(record)?;
            if let Some(lease) = unit.seat.and_then(|r| store.seat_lease.get(r)) {
                // A complete snapshot needs the current qualified lease only.
                // snapshotFromRecords treats an absent lease as unmounted and
                // derives the same discontinuity from the preceding snapshot.
                let mount = lease.vehicle.and_then(|r| store.vehicle.get(r)).map(|v| v.target_id)
                    .or_else(|| lease.turret.and_then(|r| world_turret(world, r)));
                self.push(WireRecord { kind: Kind::Seat, id: reference.slot as f64, generation: unit.generation,
                    vehicle_id: Some(mount.unwrap_or(0.)), seat_index: Some(lease.index), ..Default::default() })?;
            }
        }
        // Hulls follow infantry, so slot pre-assignment still lets an earlier
        // rider name a mount encountered later in the record sequence.
        for &reference in &store.vehicles.vehicles {
            let Some(v) = store.vehicle.get(reference).filter(|v| v.alive || v.dying > 0.) else { continue };
            self.seen(v.target_id, v.target_generation)?;
            self.push(WireRecord { kind: Kind::Vehicle, id: v.target_id, generation: v.target_generation,
                class_id: Some(required(&self.classes, v.name.0)?), team: v.team,
                position: Some(v.position), velocity: Some(v.velocity), quaternion: Some(v.quaternion),
                health: Some(health(v.health)?), state: WireRecordState::F64(if v.dying > 0. { 6. }
                    else { v.flight_state.map_or(5., |s| s as u8 as f64) }),
                // captureEntityVehicle overloads power with the remaining
                // dying animation milliseconds. No crew pose follows a dying
                // hull, even though the hull's entity record remains present.
                power: if v.dying > 0. { js_min(65535., (v.dying * 1000.).ceil()) } else { js_round(num_or(v.power.unwrap_or(0.), 0.)) }, ..Default::default() })?;
            if !v.alive { continue; }
            let (phase, blend) = world.walk(reference);
            if v.seats.len() > self.max_seats { return Err("mount seat capacity exceeded"); }
            let row = self.push(WireRecord { kind: Kind::Mount, id: v.target_id, generation: v.target_generation,
                slot: (v.slot_index >= 0. && v.slot_index < 255.).then_some(v.slot_index),
                tilt: num_or(v.tilt, 0.), roll: num_or(v.roll, 0.), walk_phase: num_or(phase, 0.), walk_blend: num_or(blend, 0.), ..Default::default() })?;
            let seats = row.seats.as_mut().unwrap();
            for seat in &v.seats { seats.push(WireSeatAngles { yaw: num_or(seat.aim_yaw, 0.), pitch: num_or(seat.pitch, 0.) }); }
        }
        for index in 0..world.turret_count() {
            // Turrets are authored live-building records with crew angles;
            // they do not join the fixed infantry/hull tombstone book.
            let Some(t) = world.turret(index).filter(|t| t.building >= 0. && t.building <= 65535.) else { continue };
            self.push(WireRecord { kind: Kind::Turret, id: t.id, generation: t.life, building: t.building,
                state: WireRecordState::Str(t.state), team: Some(js_max(0., js_min(255., num_or(t.team, 0.)))),
                health: Some(health(if t.health.is_finite() { t.health } else { 65535. })?),
                yaw: Some(num_or(t.yaw, 0.)), pitch: Some(num_or(t.pitch, 0.)),
                gun_yaw: num_or(t.gun_yaw, 0.), gun_pitch: num_or(t.gun_pitch, 0.), ..Default::default() })?;
        }
        for index in 0..self.known.len() {
            // Persistent tombstones release a protocol slot once, and then
            // remain part of known order until the same id is live again.
            let row = self.known[index];
            if self.next_known.iter().any(|r| r.id == row.id) { continue; }
            self.push(WireRecord { kind: Kind::Tombstone, id: row.id, generation: row.life, ..Default::default() })?;
            if self.next_known.len() >= self.max_entities { return Err("semantic fixed-roster capacity exceeded"); }
            self.next_known.push(row);
        }
        std::mem::swap(&mut self.known, &mut self.next_known);
        Ok(())
    }
}

pub(crate) fn current<'a>(weapons: &'a [crate::gen::Weapon], selected: f64, channel: f64, defs: &WeaponDefs) -> Option<&'a crate::gen::Weapon> {
    if !selected.is_finite() || selected < 0. || selected.fract() != 0. { return None; }
    weapons.iter().filter(|w| w.in_channel(defs, channel)).nth(selected as usize)
}
// TurretSeats' array index, its authored building index and its opaque native
// reference can differ. Resolve the qualified ref, never a guessed array index.
fn world_turret(world: &impl World, reference: Ref<crate::gen::Turret>) -> Option<f64> {
    (0..world.turret_count()).filter_map(|i| world.turret(i)).find(|t| t.reference == reference).map(|t| t.id)
}

/// The item raw-value shortcut uses the semantic codec's finer quantization,
/// not snapshot quantization. Keeping that first comparison preserves samples
/// on opposite sides of a snapshot cell within the same semantic byte cell.
fn item_bytes(id: usize, life: f64, definition: f64, raw: [f64; 10]) -> Result<[u8; 42]> {
    let mut bytes = [0; 42]; let mut at = 0;
    fn integer(bytes: &mut [u8], at: &mut usize, value: f64, min: f64, max: f64, width: usize) -> Result<()> {
        if !value.is_finite() || value.fract() != 0. || value < min || value > max { return Err("entity integer out of range"); }
        bytes[*at..*at + width].copy_from_slice(&(value as i64).to_le_bytes()[..width]); *at += width; Ok(())
    }
    integer(&mut bytes, &mut at, 9., 0., 255., 1)?;
    integer(&mut bytes, &mut at, id as f64, 0., 255., 4)?;
    integer(&mut bytes, &mut at, life, 0., 4294967295., 4)?;
    at += 4; bytes[at] = 1; at += 1;
    if (v8math::hypot3(raw[7], raw[8], raw[9]) - 1.).abs() > 0.001 || raw[7..].iter().any(|n| !n.is_finite()) { return Err("invalid item surface normal"); }
    integer(&mut bytes, &mut at, definition, 0., 65535., 2)?;
    for (index, value) in raw.iter().enumerate() {
        let (value, scale, width) = match index {
            0..=2 => (*value, 256., 4), 3..=5 => (*value, 32., 2),
            6 => (v8math::atan2(v8math::sin(*value), v8math::cos(*value)), 32767. / std::f64::consts::PI, 2),
            _ => (*value, 32767., 2),
        };
        if !value.is_finite() { return Err("nonfinite entity field"); }
        let max = if width == 2 { 32767. } else { 2147483647. };
        integer(&mut bytes, &mut at, js_round(value * scale), -max, max, width)?;
    }
    Ok(bytes)
}
