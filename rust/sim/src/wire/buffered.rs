//! encodeEntities with retained work storage. Each record is encoded once;
//! riders stay grouped with their mounts and parts retain ascending slots.
use super::codec::{self, Result, Writer, MAX_DATAGRAM};
use crate::gen::{WireClass as Class, WireEntry, WireSnapshot};

const SLOTS: usize = 512;
const PARTS: usize = 32;
const PAYLOAD: usize = 900;
struct Part { slots: Vec<usize>, fields: Vec<usize>, bytes: usize }
impl Part {
    fn new() -> Self {
        // Even one-byte removals need a slot delta. World removals need at
        // least eleven bytes, so a datagram cannot contain 128 of them.
        Self { slots: Vec::with_capacity(SLOTS), fields: Vec::with_capacity(128), bytes: 0 }
    }
    fn clear(&mut self, overhead: usize) { self.slots.clear(); self.fields.clear(); self.bytes = overhead; }
}

pub struct Encoder {
    records: Vec<Writer>, fields: Vec<Writer>, field_count: usize,
    touched: Vec<usize>, group: Vec<usize>, grouped: Vec<usize>, groups: Vec<(usize, usize)>,
    parts: Vec<Part>, output: Vec<Vec<u8>>, world: Writer,
}
impl Encoder {
    /// Reserve from the loaded world's field bound before publication. A
    /// candidate and baseline can have disjoint fields, hence the double bank.
    pub fn new(max_fields: usize) -> Self {
        let writers = |count| (0..count).map(|_| Writer(Vec::with_capacity(MAX_DATAGRAM))).collect();
        Self { records: (0..SLOTS).map(|_| Writer(Vec::with_capacity(256))).collect(), fields: writers(max_fields.min(192).saturating_mul(2)), field_count: 0,
            touched: Vec::with_capacity(SLOTS), group: Vec::with_capacity(SLOTS), grouped: Vec::with_capacity(SLOTS),
            groups: Vec::with_capacity(SLOTS), parts: (0..PARTS).map(|_| Part::new()).collect(),
            output: (0..PARTS).map(|_| Vec::with_capacity(MAX_DATAGRAM)).collect(), world: Writer(Vec::with_capacity(MAX_DATAGRAM)) }
    }

    /// World records precede entity records in a datagram, but pack after
    /// the indivisible mount/rider units. Keep the original conservative
    /// overhead calculation, including the extended multipart header retry.
    fn pack(&mut self, header: usize) -> Result<usize> {
        let overhead = header + 4;
        let mut count = 1; self.parts[0].clear(overhead);
        for &(start, end) in &self.groups {
            let slots = &self.grouped[start..end];
            let size: usize = slots.iter().map(|&slot| self.records[slot].0.len() + 2).sum();
            if overhead + size > MAX_DATAGRAM { return Err("snapshot record exceeds datagram"); }
            if self.parts[count - 1].bytes + size > MAX_DATAGRAM {
                if count == PARTS { return Err("snapshot needs too many parts"); }
                self.parts[count].clear(overhead); count += 1;
            }
            let part = &mut self.parts[count - 1]; part.bytes += size; part.slots.extend_from_slice(slots);
        }
        for index in 0..self.field_count {
            let size = self.fields[index].0.len();
            if overhead + size > MAX_DATAGRAM { return Err("snapshot record exceeds datagram"); }
            if self.parts[count - 1].bytes + size > MAX_DATAGRAM {
                if count == PARTS { return Err("snapshot needs too many parts"); }
                self.parts[count].clear(overhead); count += 1;
            }
            let part = &mut self.parts[count - 1]; part.bytes += size; part.fields.push(index);
        }
        Ok(count)
    }

    /// Returned buffers belong to this encoder until its next invocation.
    /// The host consumes them while building its publication arena, before
    /// another baseline encode can replace this scratch result.
    pub fn encode(&mut self, current: &WireSnapshot, baseline: Option<&WireSnapshot>, interval: f64) -> Result<&[Vec<u8>]> {
        let mut entries: [Option<&WireEntry>; SLOTS] = [None; SLOTS];
        let mut bases = [None; SLOTS];
        for row in &current.slots { entries[codec::slot(row.slot)?] = Some(&row.entry); }
        if let Some(base) = baseline { for row in &base.slots { bases[codec::slot(row.slot)?] = Some(&row.entry); } }
        self.touched.clear();
        for slot in 0..SLOTS {
            let record = &mut self.records[slot]; record.0.clear();
            match entries[slot] {
                Some(entry) => {
                    if entry.mount.seats.len() > 32 { record.0.try_reserve(MAX_DATAGRAM).map_err(|_| "snapshot record exceeds datagram")?; }
                    codec::record_into(record, entry, bases[slot])?;
                },
                None if bases[slot].is_some() => record.0.push(2),
                _ => {}
            }
            if !record.0.is_empty() { self.touched.push(slot); }
        }

        // Field equality uses generation and payload bytes; insertion order
        // is capture/stagger order, not the numeric schema or identity order.
        self.field_count = 0;
        let before = baseline.map_or(&[][..], |b| b.world.as_slice());
        for field in &current.world {
            let base = before.iter().find(|b| b.id == field.id && b.schema == field.schema);
            if base.is_some_and(|b| b.generation == field.generation && b.payload == field.payload) { continue; }
            if field.payload.len() > PAYLOAD { return Err("snapshot record exceeds datagram"); }
            let out = self.fields.get_mut(self.field_count).ok_or("snapshot needs too many parts")?;
            out.0.clear(); codec::field(out, field, false, base)?; self.field_count += 1;
        }
        for field in before {
            if current.world.iter().any(|c| c.id == field.id && c.schema == field.schema) { continue; }
            let out = self.fields.get_mut(self.field_count).ok_or("snapshot needs too many parts")?;
            out.0.clear(); codec::field(out, field, true, None)?; self.field_count += 1;
        }

        // A group claims each changed slot once. Sorting only within the
        // group preserves the codec's greedy grouping order and byte budget.
        let mut claimed = [false; SLOTS]; self.groups.clear(); self.grouped.clear();
        for &slot in &self.touched {
            if claimed[slot] { continue; }
            self.group.clear(); self.group.push(slot); claimed[slot] = true;
            if entries[slot].is_some_and(|e| e.cls == Class::Vehicle) {
                for &other in &self.touched {
                    if !claimed[other] && entries[other].is_some_and(|e| e.seat_slot == slot as f64 && matches!(e.cls, Class::Soldier | Class::Droideka)) {
                        self.group.push(other); claimed[other] = true;
                    }
                }
            }
            if let Some(entry) = entries[slot].filter(|e| matches!(e.cls, Class::Soldier | Class::Droideka) && e.seat_slot >= 0.) {
                let mount = codec::slot(entry.seat_slot)?;
                if !claimed[mount] && !self.records[mount].0.is_empty() { self.group.push(mount); claimed[mount] = true; }
            }
            self.group.sort_unstable(); let start = self.grouped.len();
            self.grouped.extend_from_slice(&self.group); self.groups.push((start, self.grouped.len()));
        }
        let mut count = self.pack(20)?;
        if count > 15 { count = self.pack(24)?; }
        for index in 0..count {
            let part = &mut self.parts[index]; part.slots.sort_unstable();
            let mut writer = Writer(std::mem::take(&mut self.output[index])); writer.0.clear();
            let result = (|| {
                codec::header(&mut writer, codec::ENTITIES, current.epoch, current.tick, interval,
                    baseline.map_or(0., |b| b.tick), index, count, 0.)?;
                self.world.0.clear(); self.world.varint(part.fields.len());
                for &field in &part.fields { self.world.raw(&self.fields[field].0); }
                writer.u16(self.world.0.len() as f64)?; writer.raw(&self.world.0); writer.varint(part.slots.len());
                let mut previous = 0;
                for (ordinal, &slot) in part.slots.iter().enumerate() {
                    if ordinal == 0 { writer.u16(slot as f64)?; } else { writer.varint(slot - previous); }
                    previous = slot; writer.raw(&self.records[slot].0);
                }
                if writer.0.len() > MAX_DATAGRAM { return Err("snapshot part overflow"); }
                Ok(())
            })();
            // Return storage even after a malformed record failed midway.
            // The next publication must not inherit an empty-capacity buffer.
            self.output[index] = writer.0; result?;
        }
        Ok(&self.output[..count])
    }
}
