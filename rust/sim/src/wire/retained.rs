//! snapshotFromRecords and SnapshotRing with storage reserved at attachment.
//! A frame owns its entry and field banks until eviction; readers never alias
//! a producer's scratch or a later publication that reused its numeric slot.
use crate::{gen::*, store::Strings};
use super::{codec::Result, records::{self, SnapshotSlotTable}, ring::SnapshotRing};
use jsrt::{same_value_zero, num_truthy, JsMap};

const WORKING_SLOTS: usize = 256;

struct Frame {
    snapshot: WireSnapshot,
    slots: Vec<WireSlot>, fields: Vec<WireField>, by_id: Vec<(f64, f64)>, hints: JsMap<f64, [usize; 2]>,
}
impl Frame {
    fn new(seats: usize, fields: usize) -> Self {
        // A slot can change class after retirement. Keep both optional banks
        // ready, rather than allocating when its first droideka or hull arrives.
        let slots = (0..WORKING_SLOTS).map(|_| WireSlot { slot: 0., entry: WireEntry {
            anim: Vec::with_capacity(5), mount: WireMount { seats: Vec::with_capacity(seats), ..Default::default() },
            ..Default::default() } }).collect();
        let mut hints = JsMap::new(); hints.reserve(WORKING_SLOTS);
        Self { snapshot: WireSnapshot { slots: Vec::with_capacity(WORKING_SLOTS), world: Vec::with_capacity(fields.min(192)), ..Default::default() },
            slots, fields: (0..fields.min(192)).map(|_| WireField { payload: Vec::with_capacity(900), ..Default::default() }).collect(),
            by_id: Vec::with_capacity(records::SLOT_COUNT), hints }
    }
    // snapshotFromRecords can reuse a numeric slot for a different class.
    // Reset only live metadata while keeping all nested capacity available.
    // Held frames own their bytes independently of the producer's next sample.
    fn begin(&mut self, tick: f64, epoch: f64) {
        // Draining returns every nested allocation to this same frame. A
        // failed build follows this route too before its next retry.
        self.slots.extend(self.snapshot.slots.drain(..)); self.fields.extend(self.snapshot.world.drain(..));
        self.snapshot.tick = tick; self.snapshot.epoch = epoch; self.by_id.clear();
    }
    fn capture<'a>(&mut self, tick: f64, epoch: f64, records: &[WireRecord], world: impl Iterator<Item = &'a WireField>,
        slots: &mut SnapshotSlotTable, bounds: &WireBounds, previous: Option<&WireSnapshot>, strings: &Strings) -> Result<()> {
        self.begin(tick, epoch);
        // World insertion order is stagger order. Byte copies retain exact
        // Unicode/JSON payloads without depending on a facade's object identity.
        for source in world {
            let field = self.fields.pop().ok_or("snapshot world capacity exceeded")?;
            self.snapshot.world.push(field);
            let field = self.snapshot.world.last_mut().unwrap();
            if source.payload.len() > field.payload.capacity() { return Err("snapshot record exceeds datagram"); }
            field.id = source.id; field.schema = source.schema; field.generation = source.generation;
            field.payload.clear(); field.payload.extend_from_slice(&source.payload);
        }
        // snapshotFromRecords uses the last seat and mount record for an id.
        // Index the bounded source once instead of rescanning its wide records
        // for every entity. Keys keep SameValueZero semantics, not row identity.
        self.hints.clear();
        for (at, record) in records.iter().enumerate() {
            let hint = match record.kind { WireRecordKind::Seat => 0, WireRecordKind::Mount => 1, _ => continue };
            if let Some(indices) = self.hints.get_mut(&record.id) { indices[hint] = at; }
            else {
                if self.hints.size() == WORKING_SLOTS { return Err("snapshot slot table full"); }
                let mut indices = [usize::MAX; 2]; indices[hint] = at; self.hints.set(record.id, indices);
            }
        }
        // Previous entries use the protocol slot, independent of source order.
        // Preserve the first previous occurrence, matching Array.find exactly.
        let mut before_by_slot: [Option<&WireEntry>; records::SLOT_COUNT] = [None; records::SLOT_COUNT];
        if let Some(previous) = previous { for row in &previous.slots {
            if row.slot.is_finite() && row.slot >= 0. && row.slot.fract() == 0. && row.slot < records::SLOT_COUNT as f64 {
                let before = &mut before_by_slot[row.slot as usize]; if before.is_none() { *before = Some(&row.entry); }
            }
        } }
        let mut current_by_slot = [usize::MAX; records::SLOT_COUNT];
        // Vehicle slots exist before rider leases resolve them. Keep the
        // first pass separate from output order, as snapshotFromRecords does.
        for record in records.iter().filter(|r| r.kind == WireRecordKind::Vehicle) {
            let slot = slots.assign_unit(record.id)?;
            if let Some((_, held)) = self.by_id.iter_mut().find(|(id, _)| same_value_zero(*id, record.id)) { *held = slot; }
            else {
                if self.by_id.len() == self.by_id.capacity() { return Err("snapshot slot table full"); }
                self.by_id.push((record.id, slot));
            }
        }
        for record in records {
            use WireRecordKind as Kind;
            if record.kind == Kind::Tombstone { slots.release_unit(record.id); continue; }
            let entity = matches!(record.kind, Kind::Soldier | Kind::Droideka | Kind::Vehicle | Kind::Turret);
            if !entity && !matches!(record.kind, Kind::Item | Kind::Remote) { continue; }
            let slot = if entity {
                self.by_id.iter().find(|(id, _)| same_value_zero(*id, record.id)).map(|(_, slot)| Ok(*slot))
                    .unwrap_or_else(|| slots.assign_unit(record.id))?
            } else { slots.assign_entity(if record.kind == Kind::Item { 1 } else { 2 }, record.id)? };
            let hints = if entity { self.hints.get(&record.id).copied().unwrap_or([usize::MAX; 2]) } else { [usize::MAX; 2] };
            let seat = records.get(hints[0]);
            let seat_slot = seat.and_then(|s| s.vehicle_id).filter(|id| num_truthy(*id)).map_or(-1., |id| slots.slot_for_unit(id));
            let mount = records.get(hints[1]);
            let protocol_slot = super::codec::slot(slot)?;
            let before = before_by_slot[protocol_slot];
            let at = if current_by_slot[protocol_slot] != usize::MAX { current_by_slot[protocol_slot] }
                else {
                    let mut row = self.slots.pop().ok_or("snapshot slot table full")?; row.slot = slot;
                    self.snapshot.slots.push(row); let at = self.snapshot.slots.len() - 1; current_by_slot[protocol_slot] = at; at
                };
            let entry = &mut self.snapshot.slots[at].entry;
            let reused = records::build_entry_into(entry, record, bounds, mount, if seat_slot >= 0. { seat_slot } else { -1. },
                seat.and_then(|s| s.seat_index).unwrap_or(0.), before, strings)?;
            if entity { records::discontinuity(entry, before, reused, tick, previous.map_or(tick, |p| p.tick), bounds); }
        }
        Ok(())
    }
}

// SnapshotRing retains compact bytes instead of 129 complete sets of nested
// Vec banks. The working frames are independent: capture can inspect the last
// sample while an encoder reads any held baseline. IEEE bits stay unchanged.
struct Packed { tick: f64, epoch: f64, bytes: Vec<u8> }
impl Packed {
    fn new() -> Self { Self { tick: 0., epoch: 0., bytes: Vec::with_capacity(32 * 1024) } }
    fn write(&mut self, snapshot: &WireSnapshot, heap: &mut Vec<f64>) -> Result<()> {
        let mut row = [0.; WireSlot::WORDS];
        // SnapshotRing writes each generated slot once. Fallible reservation
        // at this publication boundary grows an exceptional packed frame
        // amortized, without a second serialization or a protocol-sized bank.
        self.bytes.clear(); self.tick = snapshot.tick; self.epoch = snapshot.epoch;
        self.bytes.extend_from_slice(&(snapshot.slots.len() as u16).to_le_bytes());
        self.bytes.extend_from_slice(&(snapshot.world.len() as u16).to_le_bytes());
        for slot in &snapshot.slots {
            heap.clear(); slot.put(&mut row, heap);
            self.bytes.try_reserve(2).map_err(|_| "retained snapshot capacity exceeded")?;
            self.bytes.extend_from_slice(&(heap.len() as u16).to_le_bytes());
            for word in row.iter().chain(heap.iter()) { put_word(&mut self.bytes, *word)?; }
        }
        for field in &snapshot.world {
            for word in [field.id, field.schema, field.generation] { put_word(&mut self.bytes, word)?; }
            self.bytes.try_reserve(2 + field.payload.len()).map_err(|_| "retained snapshot capacity exceeded")?;
            self.bytes.extend_from_slice(&(field.payload.len() as u16).to_le_bytes()); self.bytes.extend_from_slice(&field.payload);
        }
        Ok(())
    }
    fn read(&self, frame: &mut Frame, heap: &mut Vec<f64>) -> Result<()> {
        frame.begin(self.tick, self.epoch); let mut cursor = self.bytes.as_slice();
        let slots = take_u16(&mut cursor)?; let fields = take_u16(&mut cursor)?;
        let mut row = [0.; WireSlot::WORDS];
        for _ in 0..slots {
            let count = take_u16(&mut cursor)?;
            if count > heap.capacity() { return Err("snapshot mount capacity exceeded"); }
            for word in &mut row { *word = take_word(&mut cursor)?; }
            heap.clear(); for _ in 0..count { heap.push(take_word(&mut cursor)?); }
            let mut slot = frame.slots.pop().ok_or("snapshot slot table full")?;
            let anim = WireSlot::ENTRY + WireEntry::ANIM;
            let seats = WireSlot::ENTRY + WireEntry::MOUNT + WireMount::SEATS;
            let (aa, an, sa, sn) = (row[anim] as usize, row[anim + 1] as usize, row[seats] as usize, row[seats + 1] as usize);
            // Generated take sees empty variable sections. Move the existing
            // banks into its scalar result, then refill them from retained words.
            // This preserves generated field layouts without allocating lists.
            row[anim] = 0.; row[anim + 1] = 0.; row[seats] = 0.; row[seats + 1] = 0.;
            let mut value = WireSlot::take(&row, &[]);
            value.entry.anim = std::mem::take(&mut slot.entry.anim);
            value.entry.mount.seats = std::mem::take(&mut slot.entry.mount.seats);
            value.entry.anim.clear(); value.entry.mount.seats.clear();
            if an > value.entry.anim.capacity() || sn > value.entry.mount.seats.capacity() || aa + an > heap.len() || sa + sn * 2 > heap.len() {
                return Err("invalid retained snapshot");
            }
            value.entry.anim.extend_from_slice(&heap[aa..aa + an]);
            for pair in heap[sa..sa + sn * 2].chunks_exact(2) { value.entry.mount.seats.push([pair[0], pair[1]]); }
            frame.snapshot.slots.push(value);
        }
        for _ in 0..fields {
            let mut field = frame.fields.pop().ok_or("snapshot world capacity exceeded")?;
            field.id = take_word(&mut cursor)?; field.schema = take_word(&mut cursor)?; field.generation = take_word(&mut cursor)?;
            let count = take_u16(&mut cursor)?;
            if count > field.payload.capacity() || count > cursor.len() { return Err("invalid retained snapshot"); }
            field.payload.clear(); field.payload.extend_from_slice(&cursor[..count]); cursor = &cursor[count..];
            frame.snapshot.world.push(field);
        }
        if !cursor.is_empty() { return Err("invalid retained snapshot"); } Ok(())
    }
}
fn take_u16(cursor: &mut &[u8]) -> Result<usize> {
    if cursor.len() < 2 { return Err("invalid retained snapshot"); }
    let value = u16::from_le_bytes(cursor[..2].try_into().unwrap()) as usize; *cursor = &cursor[2..]; Ok(value)
}
// Quantized values have contiguous nonzero high bytes. Count their empty
// ends with integer intrinsics instead of testing sixteen bytes per word.
// The run remains lossless for arbitrary IEEE values, including NaN and -0.
#[inline]
#[cfg(test)]
fn word_len(word: f64) -> usize {
    let bits = word.to_bits();
    if bits == 0 { 1 } else { 9 - (bits.leading_zeros() / 8 + bits.trailing_zeros() / 8) as usize }
}
#[inline]
fn put_word(bytes: &mut Vec<u8>, word: f64) -> Result<()> {
    let bits = word.to_bits();
    if bits == 0 {
        bytes.try_reserve(1).map_err(|_| "retained snapshot capacity exceeded")?;
        bytes.push(0); return Ok(());
    }
    let skip = (bits.trailing_zeros() / 8) as usize;
    let count = 8 - (bits.leading_zeros() / 8) as usize - skip;
    bytes.try_reserve(count + 1).map_err(|_| "retained snapshot capacity exceeded")?;
    bytes.push(((skip << 4) | count) as u8);
    bytes.extend_from_slice(&bits.to_le_bytes()[skip..skip + count]); Ok(())
}
#[inline]
fn take_word(cursor: &mut &[u8]) -> Result<f64> {
    let (&run, tail) = cursor.split_first().ok_or("invalid retained snapshot")?;
    if run == 0 { *cursor = tail; return Ok(0.); }
    let (skip, count) = ((run >> 4) as usize, (run & 15) as usize);
    if count == 0 || skip + count > 8 || tail.len() < count { return Err("invalid retained snapshot"); }
    let mut raw = [0u8; 8]; raw[skip..skip + count].copy_from_slice(&tail[..count]);
    *cursor = &tail[count..]; Ok(f64::from_le_bytes(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_hints_keep_last_records_and_first_previous_slot() {
        let strings = Strings::default(); let bounds = records::default_bounds();
        let soldier = WireRecord { kind: WireRecordKind::Soldier, id: 0., generation: 1., class_id: Some(1.),
            team: Some(1.), health: Some(100.), yaw: Some(0.), pitch: Some(0.), ..Default::default() };
        let vehicle = WireRecord { kind: WireRecordKind::Vehicle, id: 10., generation: 1., class_id: Some(2.),
            team: Some(1.), health: Some(500.), quaternion: Some([0., 0., 0., 1.]), ..Default::default() };
        // Side records precede/follow their bodies, and signed zero is the
        // same identity. Last hints replace earlier hints without reordering.
        let rows = vec![
            WireRecord { kind: WireRecordKind::Seat, id: -0., vehicle_id: Some(10.), seat_index: Some(0.), ..Default::default() },
            soldier.clone(), vehicle,
            WireRecord { kind: WireRecordKind::Mount, id: 10., seats: Some(vec![WireSeatAngles { yaw: 0., pitch: 0. }]), ..Default::default() },
            WireRecord { kind: WireRecordKind::Seat, id: 0., vehicle_id: Some(10.), seat_index: Some(1.), ..Default::default() },
            WireRecord { kind: WireRecordKind::Mount, id: 10., seats: Some(vec![WireSeatAngles { yaw: 0.2, pitch: -0.3 }, WireSeatAngles { yaw: 0.4, pitch: 0.5 }]), ..Default::default() },
            WireRecord { health: Some(99.), ..soldier },
        ];
        let mut reference_slots = SnapshotSlotTable::default(); let mut actual_slots = SnapshotSlotTable::default();
        let mut previous = records::snapshot_from_records(1., 1., &rows, vec![], &mut reference_slots, &bounds, None, &strings).unwrap();
        let mut frame = Frame::new(2, 0);
        frame.capture(1., 1., &rows, [].iter(), &mut actual_slots, &bounds, None, &strings).unwrap();
        let mut duplicate = previous.slots.iter().find(|r| r.entry.cls == WireClass::Soldier).unwrap().clone();
        assert_eq!(duplicate.entry.seat_index, 1.); assert_eq!(duplicate.entry.health, 99.);
        duplicate.entry.pos = [65535.; 3]; previous.slots.push(duplicate);
        let expected = records::snapshot_from_records(2., 1., &rows, vec![], &mut reference_slots, &bounds, Some(&previous), &strings).unwrap();
        frame.capture(2., 1., &rows, [].iter(), &mut actual_slots, &bounds, Some(&previous), &strings).unwrap();
        assert_eq!(super::super::codec::encode_entities(&frame.snapshot, None, 1.).unwrap(),
            super::super::codec::encode_entities(&expected, None, 1.).unwrap());
    }
    #[test]
    fn retained_words_preserve_every_ieee_byte_without_truncation() {
        let mut bytes = Vec::with_capacity(16);
        for bits in [0, 1, 0x8000000000000000, 0x7ff8000000000001, 0x7ff0000000000000,
            0xfff0000000000000, 0x0123000045006789, u64::MAX, 1f64.to_bits(), f64::MAX.to_bits()] {
            bytes.clear(); let word = f64::from_bits(bits); put_word(&mut bytes, word).unwrap();
            assert_eq!(bytes.len(), word_len(word));
            let mut cursor = bytes.as_slice(); assert_eq!(take_word(&mut cursor).unwrap().to_bits(), bits); assert!(cursor.is_empty());
            if bytes.len() > 1 { assert!(take_word(&mut &bytes[..bytes.len() - 1]).is_err()); }
        }
        assert!(take_word(&mut &[0x78][..]).is_err());
    }
}

pub struct Ring {
    ring: SnapshotRing<usize>, frames: Vec<Packed>, free: Vec<usize>,
    current: Frame, stage: Frame, baseline: Frame, heap: Vec<f64>, present: bool,
}
impl Ring {
    pub fn new(seats: usize, fields: usize, depth: usize) -> Result<Self> {
        if seats > 255 || fields > 1024 || depth == 0 || depth > 128 { return Err("invalid retained snapshot capacity"); }
        // The measured 32-client envelope fits a 32 KiB packed frame. Only
        // three full working snapshots retain per-slot and per-field banks.
        // The extra packed row makes failed capture transactional at eviction.
        Ok(Self { ring: SnapshotRing::new(depth)?, frames: (0..=depth).map(|_| Packed::new()).collect(), free: (0..=depth).rev().collect(),
            current: Frame::new(seats, fields), stage: Frame::new(seats, fields), baseline: Frame::new(seats, fields),
            heap: Vec::with_capacity(5 + seats * 2), present: false })
    }
    pub fn latest(&self) -> Option<&WireSnapshot> { self.present.then_some(&self.current.snapshot) }
    // encodeEntities normally reads the current working snapshot directly.
    // Old requested ticks use stage, and each selected baseline uses its own
    // scratch. Neither operation changes the previous capture's observation.
    pub fn pair(&mut self, tick: f64, epoch: f64, baseline: Option<f64>) -> Result<(&WireSnapshot, Option<&WireSnapshot>)> {
        let at = *self.ring.get(tick as i64, Some(epoch as u32)).ok_or("snapshot not held")?;
        let current = if self.current.snapshot.tick == tick && self.current.snapshot.epoch == epoch { &self.current.snapshot }
            else { self.frames[at].read(&mut self.stage, &mut self.heap)?; &self.stage.snapshot };
        let base = if let Some(tick) = baseline {
            let at = *self.ring.get(tick as i64, Some(epoch as u32)).ok_or("chosen snapshot baseline not held")?;
            self.frames[at].read(&mut self.baseline, &mut self.heap)?; Some(&self.baseline.snapshot)
        } else { None };
        Ok((current, base))
    }
    pub fn get(&mut self, tick: f64, epoch: f64) -> Option<&WireSnapshot> { self.pair(tick, epoch, None).ok().map(|p| p.0) }
    // snapshotFromRecords stages the complete capture before replacing ring
    // metadata. A rejected snapshot leaves all retained baselines readable.
    pub fn capture<'a>(&mut self, tick: f64, epoch: f64, records: &[WireRecord], world: impl Iterator<Item = &'a WireField>,
        slots: &mut SnapshotSlotTable, bounds: &WireBounds, strings: &Strings) -> Result<()> {
        if !tick.is_finite() || tick.fract() != 0. || tick.abs() > 9007199254740991. { return Err("invalid snapshot ring tick"); }
        if self.ring.len() == self.ring.capacity() && tick < self.ring.oldest_tick() as f64 { return Err("snapshot not held"); }
        self.stage.capture(tick, epoch, records, world, slots, bounds, self.present.then_some(&self.current.snapshot), strings)?;
        let at = self.free.pop().ok_or("retained snapshot capacity exceeded")?;
        if let Err(error) = self.frames[at].write(&self.stage.snapshot, &mut self.heap) { self.free.push(at); return Err(error); }
        let replaced = self.ring.get(tick as i64, None).copied(); let free = &mut self.free;
        self.ring.set_with_eviction(tick as i64, epoch as u32, at, |old, _| free.push(old))?;
        if let Some(old) = replaced { self.free.push(old); }
        std::mem::swap(&mut self.current, &mut self.stage); self.present = true; Ok(())
    }
}
