//! Semantic capture records to snapshot entries, following
//! runtime/snapshot-records.js and snapshot-codec.js quantizers.
use crate::gen::{WireBounds, WireClass, WireEntry, WireField, WireMount, WireQuaternion,
    WireRecord, WireRecordKind, WireRecordState, WireSlot, WireSnapshot, WireVelocity};
use crate::store::Strings;
use jsrt::{js_max, js_min, js_round, num_or, num_truthy, same_value_zero, to_int32};

pub const SLOT_COUNT: usize = 512;
const PI: f64 = std::f64::consts::PI;
const ANGLE: f64 = 65536.0 / (2.0 * PI);
const QUAT_RANGE: f64 = std::f64::consts::FRAC_1_SQRT_2;

pub fn default_bounds() -> WireBounds {
    WireBounds { x_min: -1024.0, x_extent: 2048.0, y_min: -400.0,
        y_extent: 700.0, z_min: -1024.0, z_extent: 2048.0 }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 { js_max(lo, js_min(hi, v)) }

pub fn quantize_position(p: [f64; 3], b: &WireBounds) -> [f64; 3] {
    [clamp(js_round((p[0] - b.x_min) / b.x_extent * 65535.0), 0.0, 65535.0),
     clamp(js_round((p[1] - b.y_min) / b.y_extent * 65535.0), 0.0, 65535.0),
     clamp(js_round((p[2] - b.z_min) / b.z_extent * 65535.0), 0.0, 65535.0)]
}

pub fn quantize_velocity(v: [f64; 3], force_wide: bool) -> WireVelocity {
    let wide = force_wide || !v.iter().all(|x| x.is_finite() && x.abs() <= 31.75);
    WireVelocity { wide, v: v.map(|x| if wide { clamp(js_round(x * 32.0), -32767.0, 32767.0) }
        else { clamp(js_round(x / 0.25), -127.0, 127.0) }) }
}

pub fn quantize_yaw(yaw: f64) -> f64 {
    let wrapped = ((yaw % (2.0 * PI)) + 2.0 * PI) % (2.0 * PI);
    (to_int32(js_round(wrapped * ANGLE)) & 0xffff) as f64
}

pub fn quantize_pitch(pitch: f64) -> f64 { clamp(js_round(pitch * ANGLE), -32768.0, 32767.0) }

pub fn angle_code(value: f64) -> f64 { clamp(js_round(value * 32767.0 / PI), -32767.0, 32767.0) }

pub fn quantize_quaternion(q: [f64; 4]) -> WireQuaternion {
    let mut index = 0;
    for i in 1..4 { if q[i].abs() > q[index].abs() { index = i; } }
    let sign = if q[index] < 0.0 { -1.0 } else { 1.0 };
    let mut v = [0.0; 3];
    let mut n = 0;
    for (i, x) in q.iter().enumerate() {
        if i != index { v[n] = clamp(js_round((x * sign / QUAT_RANGE * 0.5 + 0.5) * 32767.0), 0.0, 32767.0); n += 1; }
    }
    WireQuaternion { index: index as f64, v }
}

fn mount_entry(mount: Option<&WireRecord>) -> WireMount {
    match mount {
        Some(m) => WireMount {
            slot: m.slot, tilt: angle_code(m.tilt), roll: angle_code(m.roll),
            walk_phase: js_round(js_max(0.0, js_min(65.535, num_or(m.walk_phase, 0.0))) * 1000.0),
            walk_blend: js_round(js_max(0.0, js_min(1.0, num_or(m.walk_blend, 0.0))) * 255.0),
            seats: m.seats.as_deref().unwrap_or(&[]).iter().map(|s| [angle_code(s.yaw), angle_code(s.pitch)]).collect(),
        },
        None => WireMount::default(),
    }
}

/// Wire slots are recycled LIFO. Exhaustion consumes `next`, like the JS
/// post-increment, but never inserts a key that did not receive a slot.
#[derive(Clone, Debug, Default)]
pub struct SnapshotSlotTable {
    by_key: Vec<(String, f64)>,
    by_unit_id: Vec<(f64, f64)>,
    free: Vec<f64>,
    next: f64,
}

impl SnapshotSlotTable {
    pub fn assign(&mut self, key: String, unit_id: Option<f64>) -> Result<f64, &'static str> {
        if let Some((_, slot)) = self.by_key.iter().find(|(k, _)| k == &key) { return Ok(*slot); }
        let slot = self.free.pop().unwrap_or_else(|| { let slot = self.next; self.next += 1.0; slot });
        if slot >= SLOT_COUNT as f64 { return Err("snapshot slot table full"); }
        self.by_key.push((key, slot));
        if let Some(id) = unit_id {
            if let Some((_, value)) = self.by_unit_id.iter_mut().find(|(key, _)| same_value_zero(*key, id)) { *value = slot; }
            else { self.by_unit_id.push((id, slot)); }
        }
        Ok(slot)
    }
    pub fn assign_unit(&mut self, id: f64) -> Result<f64, &'static str> {
        let existing = self.slot_for_unit(id);
        if existing >= 0.0 { Ok(existing) } else { self.assign(format!("unit:{}", jsrt::numfmt::number_to_string(id)), Some(id)) }
    }
    pub fn slot_for(&self, key: &str) -> f64 {
        self.by_key.iter().find(|(k, _)| k == key).map(|(_, slot)| *slot).unwrap_or(-1.0)
    }
    pub fn slot_for_unit(&self, id: f64) -> f64 {
        self.by_unit_id.iter().find(|(key, _)| same_value_zero(*key, id)).map(|(_, slot)| *slot).unwrap_or(-1.0)
    }
    pub fn release(&mut self, key: &str) {
        if let Some(at) = self.by_key.iter().position(|(k, _)| k == key) {
            let (_, slot) = self.by_key.remove(at);
            self.free.push(slot);
            self.by_unit_id.retain(|(_, value)| *value != slot);
        }
    }
    pub fn release_unit(&mut self, id: f64) {
        let slot = self.slot_for_unit(id);
        if slot < 0.0 { return; }
        if let Some((key, _)) = self.by_key.iter().find(|(_, value)| *value == slot) {
            let key = key.clone(); self.release(&key);
        }
    }
}

fn record_class(kind: WireRecordKind) -> Result<WireClass, &'static str> {
    Ok(match kind {
        WireRecordKind::Soldier => WireClass::Soldier, WireRecordKind::Droideka => WireClass::Droideka,
        WireRecordKind::Vehicle => WireClass::Vehicle, WireRecordKind::Turret => WireClass::Turret,
        WireRecordKind::Item => WireClass::Item, WireRecordKind::Remote => WireClass::Remote,
        _ => return Err("snapshot entry from non-entity record"),
    })
}

fn state_number(state: &WireRecordState) -> f64 {
    match state { WireRecordState::F64(n) => *n, _ => f64::NAN }
}

/// Equality here follows capture's `===`, which deliberately differs from
/// the codec's float32 Object.is comparison for droideka animation words.
fn same_entry(a: &WireEntry, b: &WireEntry) -> bool {
    if b.disc || a.cls != b.cls || a.entity_id != b.entity_id || a.generation != b.generation
        || a.class_id != b.class_id || a.team != b.team { return false; }
    if a.cls == WireClass::Turret {
        return a.yaw == b.yaw && a.pitch == b.pitch && a.gun_yaw == b.gun_yaw && a.gun_pitch == b.gun_pitch
            && a.health == b.health && a.state == b.state;
    }
    if a.pos != b.pos || a.vel != b.vel { return false; }
    match a.cls {
        WireClass::Soldier | WireClass::Droideka => {
            let same = a.yaw == b.yaw && a.pitch == b.pitch && a.health == b.health && a.posture == b.posture
                && a.state == b.state && a.weapon == b.weapon && a.seat_slot == b.seat_slot
                && a.seat_index == b.seat_index && a.fire_counter == b.fire_counter;
            same && if a.cls == WireClass::Soldier { a.charge == b.charge } else {
                a.mode == b.mode && a.reverse == b.reverse && a.dead == b.dead && a.shield_on == b.shield_on && a.anim == b.anim
            }
        },
        WireClass::Vehicle => a.quat == b.quat && a.mount == b.mount && a.health == b.health
            && a.state == b.state && a.power == b.power && a.fire_counter == b.fire_counter,
        WireClass::Item => a.up == b.up && a.yaw == b.yaw && a.active == b.active && a.definition_id == b.definition_id,
        WireClass::Remote => a.yaw == b.yaw && a.pitch == b.pitch && a.active == b.active
            && a.definition_id == b.definition_id && a.health == b.health,
        WireClass::Turret => unreachable!(),
    }
}

fn build_entry(record: &WireRecord, bounds: &WireBounds, mount: Option<&WireRecord>, seat_slot: f64,
    seat_index: f64, previous: Option<&WireEntry>, strings: &Strings) -> Result<(WireEntry, bool), &'static str> {
    let cls = record_class(record.kind)?;
    let mut entry = WireEntry { cls, entity_id: record.id, generation: record.generation,
        class_id: record.class_id.unwrap_or(0.0), team: record.team.unwrap_or(0.0), seat_slot: -1.0,
        ..WireEntry::default() };
    if cls == WireClass::Turret {
        entry.class_id = record.building;
        entry.health = record.health.unwrap_or(f64::NAN);
        entry.state = match &record.state {
            WireRecordState::Str(s) => {
                let units = strings.units(*s);
                if units.iter().copied().eq("unbuilt".encode_utf16()) { 0.0 }
                else if units.iter().copied().eq("built".encode_utf16()) { 1.0 }
                else if units.iter().copied().eq("destroyed".encode_utf16()) { 2.0 }
                else { -1.0 }
            },
            _ => -1.0,
        };
        entry.yaw = angle_code(record.yaw.unwrap_or(f64::NAN));
        entry.pitch = angle_code(record.pitch.unwrap_or(f64::NAN));
        entry.gun_yaw = angle_code(record.gun_yaw);
        entry.gun_pitch = angle_code(record.gun_pitch);
    } else {
        entry.pos = quantize_position(record.position.unwrap_or([0.0; 3]), bounds);
        entry.vel = quantize_velocity(record.velocity.unwrap_or([0.0; 3]), cls == WireClass::Item);
        match cls {
            WireClass::Soldier | WireClass::Droideka => {
                entry.yaw = quantize_yaw(record.yaw.unwrap_or(f64::NAN));
                entry.pitch = quantize_pitch(record.pitch.unwrap_or(f64::NAN));
                entry.health = record.health.unwrap_or(f64::NAN);
                entry.posture = (to_int32(record.posture) & 3) as f64;
                entry.state = state_number(&record.state);
                entry.weapon = record.weapon;
                entry.fire_counter = (to_int32(record.fire_counter.unwrap_or(0.0)) & 0xff) as f64;
                entry.seat_slot = seat_slot;
                entry.seat_index = seat_index;
                if cls == WireClass::Soldier { entry.charge = record.charge.unwrap_or(0.0); }
                else {
                    entry.anim = vec![record.time, record.roll, record.lean, record.legs, record.shield];
                    entry.mode = record.mode; entry.reverse = record.reverse; entry.dead = record.dead; entry.shield_on = record.shield_on;
                }
            },
            WireClass::Vehicle => {
                entry.quat = quantize_quaternion(record.quaternion.ok_or("snapshot vehicle quaternion missing")?);
                entry.mount = mount_entry(mount);
                entry.health = record.health.unwrap_or(f64::NAN);
                entry.state = state_number(&record.state);
                entry.power = record.power;
                entry.fire_counter = (to_int32(record.fire_counter.unwrap_or(0.0)) & 0xff) as f64;
            },
            WireClass::Item => {
                entry.up = record.up.unwrap_or([0.0, 1.0, 0.0]).map(|x| clamp(js_round(x * 32767.0), -32767.0, 32767.0));
                entry.yaw = quantize_yaw(record.yaw.unwrap_or(0.0));
                entry.active = record.active; entry.definition_id = record.definition_id.unwrap_or(0.0);
            },
            WireClass::Remote => {
                entry.yaw = quantize_yaw(record.yaw.unwrap_or(0.0));
                entry.pitch = quantize_pitch(record.pitch.unwrap_or(0.0));
                entry.active = record.active; entry.definition_id = record.definition_id.unwrap_or(0.0);
                entry.health = record.health.unwrap_or(0.0);
            },
            WireClass::Turret => unreachable!(),
        }
    }
    if let Some(before) = previous {
        // Reused nested values keep their signed-zero bits even if a new
        // entry is required by an unrelated field or the discontinuity bit.
        if cls != WireClass::Turret {
            if entry.pos == before.pos { entry.pos = before.pos; }
            if entry.vel == before.vel { entry.vel = before.vel.clone(); }
        }
        if cls == WireClass::Vehicle {
            if entry.quat == before.quat { entry.quat = before.quat.clone(); }
            if entry.mount == before.mount { entry.mount = before.mount.clone(); }
        }
        if cls == WireClass::Droideka && entry.anim == before.anim { entry.anim = before.anim.clone(); }
        if cls == WireClass::Item && entry.up == before.up { entry.up = before.up; }
        if same_entry(&entry, before) { return Ok((before.clone(), true)); }
    }
    Ok((entry, false))
}

pub fn entry_from_record(record: &WireRecord, bounds: &WireBounds, mount: Option<&WireRecord>, seat_slot: f64,
    seat_index: f64, previous: Option<&WireEntry>, strings: &Strings) -> Result<WireEntry, &'static str> {
    build_entry(record, bounds, mount, seat_slot, seat_index, previous, strings).map(|(entry, _)| entry)
}

fn put_slot(slots: &mut Vec<WireSlot>, slot: f64, entry: WireEntry) {
    if let Some(row) = slots.iter_mut().find(|s| same_value_zero(s.slot, slot)) { row.entry = entry; }
    else { slots.push(WireSlot { slot, entry }); }
}

/// Capture record order determines slot allocation and the resulting Map
/// order. Vehicle slots are assigned in a first pass before their riders.
pub fn snapshot_from_records(tick: f64, epoch: f64, records: &[WireRecord], world: Vec<WireField>,
    slots: &mut SnapshotSlotTable, bounds: &WireBounds, previous: Option<&WireSnapshot>, strings: &Strings) -> Result<WireSnapshot, &'static str> {
    let mut snapshot = WireSnapshot { tick, epoch, slots: Vec::new(), world };
    let mut by_id: Vec<(f64, f64)> = Vec::new();
    for r in records.iter().filter(|r| r.kind == WireRecordKind::Vehicle) {
        let slot = slots.assign_unit(r.id)?;
        if let Some((_, held)) = by_id.iter_mut().find(|(id, _)| same_value_zero(*id, r.id)) { *held = slot; }
        else { by_id.push((r.id, slot)); }
    }
    for r in records {
        if r.kind == WireRecordKind::Tombstone { slots.release_unit(r.id); continue; }
        match r.kind {
            WireRecordKind::Soldier | WireRecordKind::Droideka | WireRecordKind::Vehicle | WireRecordKind::Turret => {
                let slot = match by_id.iter().find(|(id, _)| same_value_zero(*id, r.id)) {
                    Some((_, slot)) => *slot, None => slots.assign_unit(r.id)?,
                };
                let seat = records.iter().rev().find(|s| s.kind == WireRecordKind::Seat && same_value_zero(s.id, r.id));
                let seat_slot = match seat.and_then(|s| s.vehicle_id).filter(|id| num_truthy(*id)) {
                    Some(id) => slots.slot_for_unit(id), None => -1.0,
                };
                let mount = records.iter().rev().find(|m| m.kind == WireRecordKind::Mount && same_value_zero(m.id, r.id));
                let before = previous.and_then(|s| s.slots.iter().find(|r| same_value_zero(r.slot, slot))).map(|s| &s.entry);
                let (mut entry, reused) = build_entry(r, bounds, mount, if seat_slot >= 0.0 { seat_slot } else { -1.0 },
                    seat.and_then(|s| s.seat_index).unwrap_or(0.0), before, strings)?;
                if let Some(before) = before.filter(|before| !reused && before.generation == entry.generation && before.cls == entry.cls) {
                    if matches!(entry.cls, WireClass::Soldier | WireClass::Droideka) && before.seat_slot != entry.seat_slot { entry.disc = true; }
                    if entry.cls != WireClass::Turret {
                        let gap = js_max(1.0 / 60.0, (tick - previous.expect("previous entry").tick) / 60.0);
                        let v = entry.vel.v;
                        let speed = if entry.vel.wide { v8math::hypot3(v[0] / 32.0, v[1] / 32.0, v[2] / 32.0) }
                            else { v8math::hypot3(v[0] * 0.25, v[1] * 0.25, v[2] * 0.25) };
                        let allowance = 3.0 + js_max(if entry.cls == WireClass::Vehicle { 60.0 } else { 12.0 }, speed) * gap;
                        let dx = (entry.pos[0] - before.pos[0]) * bounds.x_extent / 65535.0;
                        let dy = (entry.pos[1] - before.pos[1]) * bounds.y_extent / 65535.0;
                        let dz = (entry.pos[2] - before.pos[2]) * bounds.z_extent / 65535.0;
                        entry.disc = entry.disc || v8math::hypot3(dx, dy, dz) > allowance;
                    }
                }
                put_slot(&mut snapshot.slots, slot, entry);
            },
            WireRecordKind::Item | WireRecordKind::Remote => {
                let prefix = if r.kind == WireRecordKind::Item { "item" } else { "remote" };
                let slot = slots.assign(format!("{prefix}:{}", jsrt::numfmt::number_to_string(r.id)), None)?;
                let before = previous.and_then(|s| s.slots.iter().find(|r| same_value_zero(r.slot, slot))).map(|s| &s.entry);
                put_slot(&mut snapshot.slots, slot, build_entry(r, bounds, None, -1.0, 0.0, before, strings)?.0);
            },
            _ => {},
        }
    }
    Ok(snapshot)
}
