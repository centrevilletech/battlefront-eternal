//! SnapshotClient's multipart and ACK ownership, beside the retained display
//! ring. Wire groups use the tested reader above; no baseline becomes a page
//! object and no sampled pose leaves this memory on its way to a display phase.
use super::*;
use crate::network_layout::{net_header as h, net_kind as k, net_limits as l, net_record as r, net_seat as s};
use bfe_pose::{ timeline::Timeline,
    timeline_layout::{timeline_frame as tf, timeline_limits as tl, timeline_record as tr}, };

#[derive(Clone, Copy)]
// SnapshotClient entries retain wire values, not dequantized display values.
// Small position deltas must add to exactly these integer codes. Keeping both
// flags separately avoids widening the complete immutable baseline table.
struct Entry { present: bool, wide: bool, data: [i32; r::STRIDE], }
impl Default for Entry {
    fn default() -> Self { Self { present: false, wide: false, data: [0; r::STRIDE] } } }
impl Entry {
    // Wire integers fit their protocol domains; only ids/generations need
    // unsigned lanes. Animation words retain their original f32 bits. This
    // keeps each immutable version compact without rounding decoded fields.
    fn get(&self, at: usize) -> f64 { let n = self.data[at];
        if matches!(at, r::TIME | r::ROLL | r::LEAN | r::LEGS | r::SHIELD) { f32::from_bits(n as u32) as f64
        } else if matches!(at, r::ID | r::GENERATION) { n as u32 as f64 } else { n as f64 } }
    fn set(&mut self, at: usize, value: f64) {
        self.data[at] = if matches!(at, r::TIME | r::ROLL | r::LEAN | r::LEGS | r::SHIELD) {
            (value as f32).to_bits() as i32 } else if matches!(at, r::ID | r::GENERATION) { value as u32 as i32 } else {
            value as i32 }; }
    fn range(&mut self, at: usize, values: &[f64]) { for (i, &n) in values.iter().enumerate() { self.set(at + i, n); } }
    fn values(&self) -> [f64; r::STRIDE] { std::array::from_fn(|i| self.get(i)) } }
#[derive(Clone, Copy, Default)]
// worldFieldKey is the numeric (schema,id) pair; generation is payload state.
// A handle owns one reference in the field bank. Snapshot rows retain just the
// handle, so unchanged opaque state is never copied with an entity baseline.
struct Field { id: u32, schema: u16, generation: u32, handle: usize, }
struct FieldBank { bytes: Vec<[u8; l::FIELD_PAYLOAD]>, lengths: Vec<usize>, refs: Vec<u32>, meta: Vec<Field>,
    free: Vec<usize>, }
impl FieldBank {
    // The server reserves at most 192 world fields per publication. This
    // bank covers a distinct version in every retained and assembling frame;
    // larger foreign streams are refused without growing a packet store.
    // Encode-only owners do not reserve receive payloads. The later setup call
    // creates this bank once, before the first packet can enter measured work.
    fn empty() -> Self {
        Self { bytes: Vec::new(), lengths: Vec::new(), refs: Vec::new(), meta: Vec::new(), free: Vec::new() } }
    fn new() -> Self { Self { bytes: vec![[0; l::FIELD_PAYLOAD]; l::FIELD_VERSIONS],
            lengths: vec![0; l::FIELD_VERSIONS], refs: vec![0; l::FIELD_VERSIONS],
            meta: vec![Field::default(); l::FIELD_VERSIONS], free: (0..l::FIELD_VERSIONS).rev().collect(), } }
    // decodeWorld installs a payload only after its complete byte span exists.
    // The free list bounds distinct versions across completed and partial ticks.
    // A full bank reports an ordinary receive error instead of growing memory.
    fn put(&mut self, bytes: &[u8]) -> Result<usize> { if bytes.len() > l::FIELD_PAYLOAD {
            return Err("snapshot field exceeds bounds"); }
        let at = self.free.pop().ok_or("snapshot field storage full")?;
        self.bytes[at][..bytes.len()].copy_from_slice(bytes);
        self.lengths[at] = bytes.len();
        self.refs[at] = 1;
        Ok(at) }
    fn get(&self, at: usize) -> &[u8] { &self.bytes[at][..self.lengths[at]] }
    // Immutable snapshot copies share unchanged payloads. Only a decoded change
    // owns fresh bytes; baseline retention therefore costs one integer per field.
    fn retain(&mut self, at: usize) { self.refs[at] += 1; }
    // The last baseline or unfinished assembly returns this slot to the pool.
    // No payload can be overwritten while another immutable version retains it.
    fn release(&mut self, at: usize) { self.refs[at] -= 1;
        if self.refs[at] == 0 { self.free.push(at); } } }
// assemblyFor used shallow Map copies because decoded entries were immutable.
// Here the protocol slots are compact numeric rows; order preserves insertion
// order for cold captures and identity lookups after repeated slot updates.
struct Snapshot { tick: f64, epoch: f64, valid: bool, entries: Vec<Entry>, order: Vec<usize>, world: Vec<u16>,
    own: Vec<u16>, payload: [u8; l::PAYLOAD], length: usize, has_payload: bool, }
impl Snapshot {
    fn new() -> Self { Self { tick: 0., epoch: 0., valid: false, entries: vec![Entry::default(); l::SLOTS],
            order: Vec::with_capacity(l::SLOTS), world: Vec::with_capacity(l::WORLD_FIELDS),
            own: Vec::with_capacity(l::WORLD_FIELDS), payload: [0; l::PAYLOAD], length: 0, has_payload: false, } }
    // Release handles before recycling a completed or unfinished snapshot.
    // Only active slots need clearing: the order vector names every live row,
    // including rows introduced by an earlier part of this same tick.
    fn clear(&mut self, bank: &mut FieldBank) { for field in self.world.drain(..).chain(self.own.drain(..)) {
            bank.release(field as usize); }
        for &slot in &self.order { self.entries[slot].present = false; }
        self.order.clear();
        self.valid = false;
        self.length = 0;
        self.has_payload = false; }
    // Map.set replaces in place without moving an existing key. New slots append
    // in packet order, independently of their numeric id or protocol slot value.
    fn set(&mut self, slot: usize, entry: Entry) { if !self.entries[slot].present { self.order.push(slot); }
        self.entries[slot] = entry; }
    // A removal changes insertion order only when this assembly held the slot.
    // A subsequent birth at that slot appends rather than reviving its old order.
    fn remove(&mut self, slot: usize) { if self.entries[slot].present { self.entries[slot].present = false;
            if let Some(i) = self.order.iter().position(|&s| s == slot) { self.order.remove(i); } } }
    // Copy only active entity rows and reference-counted world handles. Copying
    // all reserved protocol slots would turn a sparse packet into fleet work.
    // The correction bytes are opaque; their original bits survive unchanged.
    fn copy_from(&mut self, other: &Self, bank: &mut FieldBank) { self.clear(bank);
        self.tick = other.tick;
        self.epoch = other.epoch;
        self.valid = other.valid;
        for &slot in &other.order { self.set(slot, other.entries[slot]); }
        for (&field, target) in other.world.iter().map(|f| (f, false)).chain(other.own.iter().map(|f| (f, true))) {
            bank.retain(field as usize);
            if target { self.own.push(field); } else { self.world.push(field); } }
        self.length = other.length;
        self.has_payload = other.has_payload;
        self.payload[..self.length].copy_from_slice(&other.payload[..other.length]); } }
// SnapshotClient.complete waits for every entity part and its own datagram.
// The second cosmetic datagram is separately required when own announces it.
// A fixed bitmap admits all 256 protocol parts without allocating a Set.
struct Assembly { active: bool, tick: f64, baseline: f64, created: u64, parts: [bool; 256], count: usize,
    received: usize, own: bool, second: bool, own_parts: usize, ack: f64, }
impl Default for Assembly {
    fn default() -> Self { Self { active: false, tick: 0., baseline: 0., created: 0, parts: [false; 256], count: 0,
            received: 0, own: false, second: false, own_parts: 0, ack: 0., } } }
#[derive(Clone, Copy)]
// seatChangesFor keys ownership by body id, with the body's generation kept
// alongside the vehicle generation. Neither id is a dense protocol index.
struct Seat { id: f64, generation: f64, vehicle: f64, vehicle_generation: f64, index: f64, }
#[derive(Clone, Copy)]
struct Change { field: Field, removed: bool, }

pub struct Session { links:Vec<std::rc::Weak<std::cell::RefCell<crate::network::LinkTable>>>,
    pub header: Vec<f64>, pub records: Vec<f64>, pub seat_output: Vec<f64>, pub payloads: Vec<u8>,
    pub timeline: Timeline, snapshots: Vec<Snapshot>, ring: Vec<usize>, assemblies: Vec<Assembly>, bank: FieldBank,
    seats: Vec<Seat>, changed: Vec<usize>, removed: Vec<usize>, staging: Vec<Entry>, fields: Vec<Change>, epoch: f64,
    ack: f64, newest_entities: f64, newest_own: f64, current: Option<usize>, serial: u64, pending: bool,
    pub excluded: [f64; 2], pub lease: u64, pub missed: u64, pub hz: f64, pub extrapolate: f64, pub maximum: usize, }
impl Default for Session {
    fn default() -> Self { Self::new() } }
impl Session {
    // All tables, histories and output spans are cold reservations. Packet
    // assembly copies numeric rows and retains payload handles, never boxes
    // entries or allocates another immutable baseline.
    pub fn new() -> Self { Self { links:Vec::with_capacity(l::OWNERS),header: vec![0.; h::STRIDE], records: Vec::new(), seat_output: Vec::new(),
            payloads: Vec::new(), timeline: Timeline::new(), snapshots: Vec::new(),
            ring: Vec::with_capacity(l::BASELINES),
            assemblies: (0..l::ASSEMBLIES).map(|_| Assembly::default()).collect(), bank: FieldBank::empty(),
            seats: Vec::with_capacity(tl::ENTITIES), changed: Vec::with_capacity(l::SLOTS),
            removed: Vec::with_capacity(l::SLOTS), staging: Vec::new(), fields: Vec::with_capacity(l::WORLD_FIELDS),
            epoch: 0., ack: 0., newest_entities: -1., newest_own: -1., current: None, serial: 0, pending: false,
            excluded: [-1.; 2], lease: 0, missed: 0, hz: 60., extrapolate: 6., maximum: 64, } }
    // The page receiver calls this before transport delivery. Timeline-only and
    // encode-only owners avoid this larger reservation; repeated setup is inert.
    // The extra snapshot protects current lookups while an assembly is recycled.
    // SnapshotPresentation registers a display owner once, at cold admission.
    // Weak links preserve teardown ownership and stay within the fixed owner
    // budget; a full foreign owner set simply refuses an additional resource.
    pub(crate) fn unlink(&mut self,table:&std::rc::Rc<std::cell::RefCell<crate::network::LinkTable>>){let weak=std::rc::Rc::downgrade(table);self.links.retain(|p|!p.ptr_eq(&weak));}
    pub(crate) fn link(&mut self,table:&std::rc::Rc<std::cell::RefCell<crate::network::LinkTable>>)->bool{
        let weak=std::rc::Rc::downgrade(table);self.links.retain(|p|p.strong_count()!=0);
        if self.links.iter().any(|p|p.ptr_eq(&weak)){return true;}if self.links.len()==l::OWNERS{return false;}self.links.push(weak);true
    }
    pub fn reserve_receive(&mut self) { if !self.snapshots.is_empty() { return; }
        self.snapshots = (0..=l::BASELINES + l::ASSEMBLIES).map(|_| Snapshot::new()).collect();
        self.bank = FieldBank::new();
        self.staging = vec![Entry::default(); l::SLOTS];
        self.records = vec![0.; l::RECORDS * r::STRIDE];
        self.seat_output = vec![0.; l::SLOTS * 2 * s::STRIDE];
        self.payloads = vec![0; l::EXCHANGE]; }
    // SnapshotClient.reset discards assemblies, ACKs, seats and display history
    // at the same epoch boundary. The lease also invalidates cold display links,
    // so a pooled native owner cannot expose the preceding battle's output.
    pub fn reset(&mut self, epoch: f64) { for row in &mut self.snapshots { row.clear(&mut self.bank); }
        self.ring.clear();
        for a in &mut self.assemblies { *a = Assembly::default(); }
        self.seats.clear();
        self.epoch = epoch;
        self.header.fill(0.);
        self.header[h::EPOCH] = epoch;
        self.ack = 0.;
        self.missed = 0;
        self.newest_entities = -1.;
        self.newest_own = -1.;
        self.current = None;
        self.pending = false;
        self.timeline.reset(epoch);
        self.lease = self.lease.wrapping_add(1); }
    // baselineFor accepts only a completed tick from this epoch. An unfinished
    // snapshot may serve current gameplay lookups but cannot become an ACK base.
    fn held(&self, tick: f64) -> Option<usize> {
        self.ring.iter().copied().find(|&i| self.snapshots[i].tick == tick && self.snapshots[i].epoch == self.epoch) }
    // A keyframe starts empty. Other assemblies begin with the exact acknowledged
    // baseline, even if a newer part already changed their current protocol slots.
    // Split borrows keep both snapshots native without a temporary clone.
    fn copy(&mut self, to: usize, from: Option<usize>) { let Some(from) = from else {
            self.snapshots[to].clear(&mut self.bank);
            return; };
        if to == from { return; }
        let (a, b) = if to < from { let (a, b) = self.snapshots.split_at_mut(from);
            (&mut a[to], &b[0]) } else { let (a, b) = self.snapshots.split_at_mut(to);
            (&mut b[0], &a[from]) };
        a.copy_from(b, &mut self.bank); }
    // assemblyFor reuses a matching tick/base pair and replaces a conflicting
    // baseline at that tick. When the fixed store fills, evict the oldest birth,
    // as the bounded source Map did; a later keyframe repairs any missing parts.
    fn assembly(&mut self, tick: f64, baseline: f64, base: Option<usize>) -> usize {
        if let Some(at) = self.assemblies.iter().position(|a| a.active && a.tick == tick && a.baseline == baseline) {
            return at; }
        let at = self .assemblies .iter() .position(|a| a.active && a.tick == tick)
            .or_else(|| self.assemblies.iter().position(|a| !a.active))
            .unwrap_or_else(|| self.assemblies.iter().enumerate().min_by_key(|(_, a)| a.created).map_or(0, |(i, _)| i));
        // Current lookups may still name an incomplete tick being evicted.
        // Preserve that view separately before clearing its assembly slot;
        // gameplay prerequisites must not suddenly observe an empty baseline.
        if self.current == Some(l::BASELINES + at) { let saved = l::BASELINES + l::ASSEMBLIES;
            self.copy(saved, self.current);
            self.current = Some(saved); }
        self.copy(l::BASELINES + at, base);
        self.serial += 1;
        self.assemblies[at] = Assembly { active: true, tick, baseline, created: self.serial, ..Default::default() };
        at }
    // Only complete snapshots enter the immutable ACK ring. Duplicate parts do
    // not advance this decision, and own-only snapshots cannot fake entity parts.
    // ACK is monotonic even when a completed old tick falls outside the ring.
    fn complete(&mut self, at: usize) -> bool { let a = &self.assemblies[at];
        if !a.own || a.count == 0 || a.received < a.count || (a.own_parts > 1 && !a.second) { return false; }
        let tick = a.tick;
        let source = l::BASELINES + at;
        let target = if let Some(i) = self.held(tick) { Some(i) } else if self.ring.len() < l::BASELINES {
            (0..l::BASELINES).find(|i| !self.ring.contains(i))
        } else if self.ring.first().is_some_and(|&i| tick >= self.snapshots[i].tick) { Some(self.ring.remove(0))
        } else { None };
        if let Some(target) = target { self.copy(target, Some(source));
            if !self.ring.contains(&target) {
                let at = self.ring.iter().position(|&i| self.snapshots[i].tick > tick).unwrap_or(self.ring.len());
                self.ring.insert(at, target); }
            if self.current == Some(source) { self.current = Some(target); } }
        // Completion and retention are distinct. An old completed tick may
        // no longer fit the newest-capacity ring, but its acknowledgement
        // must never move backward or make a newer held baseline disappear.
        self.ack = self.ack.max(tick);
        self.assemblies[at].active = false;
        true }
    // The adapter applies lifecycle removals after receive. Commit this part
    // on the next receive/frame so those removals still precede its new poses,
    // exactly as SnapshotClient -> EntityGameState -> addSnapshot did.
    pub fn commit(&mut self) { if self.pending { self.timeline.push();
            self.pending = false; } }
    // SnapshotPresentation.update selects one display time for all resources.
    // Commit the last arrival before sampling, then retain prediction exclusions
    // beside the samples; body and hull phases consume these same native rows.
    pub fn present(&mut self, tick: f64, player: f64, vehicle: f64) { self.commit();
        self.excluded = [player, vehicle];
        let x = &mut self.timeline.input;
        x[tf::VERSION] = bfe_pose::timeline_layout::VERSION as f64;
        x[tf::TICK] = tick;
        x[tf::HZ] = self.hz;
        x[tf::EXTRAPOLATE] = self.extrapolate;
        self.timeline.present();
        // Retire mismatched lives/classes at the sample boundary, even for
        // an undrawn resource. Only a later semantic bind can reactivate it.
        for weak in &self.links{if let Some(table)=weak.upgrade(){table.borrow_mut().update(self);}}
        self.header[h::SAMPLED_TICK] = tick;
        self.header[h::DT] = self.timeline.input[tf::DT]; }
    // SnapshotClient.receive has one byte boundary and one changed-record result.
    // Header validation precedes epoch/baseline selection. Older epochs return
    // nothing, while a newer epoch clears retained state before applying its part.
    pub fn receive(&mut self, bytes: &[u8], bounds: &[f64; 6]) -> Result<()> { self.commit();
        self.header.fill(0.);
        self.header[h::EPOCH] = self.epoch;
        self.header[h::ACK_TICK] = self.ack;
        self.header[h::MISSING_BASELINES] = self.missed as f64;
        self.changed.clear();
        self.removed.clear();
        self.fields.clear();
        let head = header(bytes)?.words;
        if head[1] < self.epoch { return Ok(()); }
        let changed = head[1] > self.epoch;
        if changed { self.reset(head[1]); }
        self.header[h::MAGIC] = head[0];
        self.header[h::BASELINE_TICK] = head[4];
        self.header[h::PART_INDEX] = head[5];
        self.header[h::PART_COUNT] = head[6];
        self.header[h::HEADER_BYTES] = head[8];
        self.header[h::EPOCH] = self.epoch;
        self.header[h::TICK] = head[2];
        self.header[h::INTERVAL] = head[3];
        self.header[h::ACK_INPUT] = head[7];
        self.header[h::EPOCH_CHANGED] = changed as u8 as f64;
        let result =
            if head[0] == ENTITIES as f64 { self.entities(bytes, head, bounds) } else { self.own(bytes, head, bounds) };
        // Uncommitted field handles belong to the decoder on every error.
        // Completed fields transfer their single reference to the snapshot.
        for change in self.fields.drain(..) { if !change.removed { self.bank.release(change.field.handle); } }
        // Foreign streams can retain more field versions than game content.
        // Refuse that incomplete part as recoverable loss, leaving its ACK
        // withheld until an old assembly is evicted or a keyframe repairs it.
        let result=match result{Err("snapshot field storage full")=>{self.missing();Ok(())},other=>other};
        self.header[h::ACK_TICK] = self.ack;
        self.header[h::MISSING_BASELINES] = self.missed as f64;
        result }
    // Missing entity or correction baselines are recoverable loss, not malformed
    // presentation frames. Keep the missing count and leave the part unacknowledged.
    fn missing(&mut self) { self.missed += 1;
        self.header[h::VALID] = 0.; }
    // decodeWorld first stages wire-ordered changes without mutating an assembly.
    // Every delta reads the named immutable base, including repeated field keys.
    // Handles transfer only after the enclosing packet passes its trailing check.
    fn field_changes(&mut self, r: &mut Reader, base: Option<usize>, own: bool, source: &[u8]) -> Result<()> {
        let count = r.varint()?;
        if count > 4096. { return Err("snapshot world field count"); }
        for _ in 0..count as usize { let id = r.u32()?;
            let schema = r.u16()?;
            let generation = r.u32()?;
            let mode = r.u8()?;
            if mode > 2 { return Err("snapshot world field flag"); }
            if mode == 1 {
                self.fields.push(Change { field: Field { id, schema, generation, handle: 0 }, removed: true });
                continue; }
            let mut scratch = [0.; 256];
            let handle = if mode == 0 { let n = r.u16()? as usize;
                self.bank.put(r.take(n)?)? } else { let previous = base .and_then(|i| {
                        let rows = if own { &self.snapshots[i].own } else { &self.snapshots[i].world };
                        rows.iter().map(|&f| self.bank.meta[f as usize]).find(|f| f.id == id && f.schema == schema) })
                    .map(|f| self.bank.get(f.handle));
                let mut out = Output { values: &mut scratch, at: 0 };
                delta(r.counted_section()?, previous, &mut out)?;
                let n = if scratch[0] == 0. { scratch[2] } else { scratch[1] } as usize;
                if n > l::FIELD_PAYLOAD { return Err("snapshot field exceeds bounds"); }
                let raw = if scratch[0] == 0. { let start = scratch[1] as usize;
                    source.get(start..start + n).ok_or(EXCHANGE)? } else {
                    unsafe { std::slice::from_raw_parts(scratch.as_ptr().add(2).cast::<u8>(), n) } };
                self.bank.put(raw)? };
            self.bank.meta[handle] = Field { id, schema, generation, handle };
            self.fields.push(Change { field: Field { id, schema, generation, handle }, removed: false }); }
        Ok(()) }
    // applyEntities replaces fields by schema and id. Removing an absent field
    // is harmless; a replacement releases exactly this snapshot's old reference.
    // The pre-reserved handle list never grows beyond its explicit field bound.
    fn apply_fields(&mut self, snapshot: usize, own: bool) {
        let rows = if own { &mut self.snapshots[snapshot].own } else { &mut self.snapshots[snapshot].world };
        for change in self.fields.drain(..) { let at = rows.iter().position(|&f| { let f = self.bank.meta[f as usize];
                f.id == change.field.id && f.schema == change.field.schema });
            if let Some(at) = at { self.bank.release(rows[at] as usize);
                if change.removed { rows.remove(at); } else { rows[at] = change.field.handle as u16; }
            } else if !change.removed { if rows.len() == l::WORLD_FIELDS { self.bank.release(change.field.handle);
                } else { rows.push(change.field.handle as u16); } } } }
    // Changed gameplay records are the only numeric values the page still reads.
    // The publication window is larger than any MTU-limited packet can produce;
    // foreign oversized expansions return a receive fault without a buffer grow.
    fn emit(&mut self, record: &[f64; r::STRIDE]) -> Result<()> { let at = self.header[h::RECORDS] as usize;
        if at >= l::RECORDS { return Err("snapshot publication full"); }
        self.records[at * r::STRIDE..][..r::STRIDE].copy_from_slice(record);
        self.header[h::RECORDS] += 1.;
        Ok(()) }
    // Owned corrections and cosmetics cannot borrow the next packet's input.
    // Copy their opaque bytes once into the publication window; the JS consumer
    // then owns the payload it retains for prediction or deferred prerequisites.
    fn bytes_record( &mut self, kind: u32, id: f64, generation: f64, tick: f64, schema: f64, bytes: &[u8],
    ) -> Result<()> { let at = self.header[h::PAYLOAD_LENGTH] as usize;
        let end = at.checked_add(bytes.len()).ok_or(EXCHANGE)?;
        if end > self.payloads.len() { return Err("snapshot payload publication full"); }
        self.payloads[at..end].copy_from_slice(bytes);
        self.header[h::PAYLOAD_LENGTH] = end as f64;
        let mut row = [0.; r::STRIDE];
        row[r::KIND] = kind as f64;
        row[r::ID] = id;
        row[r::GENERATION] = generation;
        row[r::TICK] = tick;
        row[r::SCHEMA] = schema;
        row[r::PAYLOAD_OFFSET] = at as f64;
        row[r::PAYLOAD_LENGTH] = bytes.len() as f64;
        self.emit(&row) }
    // worldRecordFromField publishes nonremoved summaries in decoded wire order.
    // Removed fields update the retained baseline without producing a gameplay
    // record, matching both the entity and own summary lanes.
    fn emit_fields(&mut self, tick: f64) -> Result<()> { for at in 0..self.fields.len() { let c = self.fields[at];
            if c.removed { continue; }
            let mut bytes = [0; l::FIELD_PAYLOAD];
            let source = self.bank.get(c.field.handle);
            let n = source.len();
            bytes[..n].copy_from_slice(source);
            self.bytes_record( k::SUMMARY, c.field.id as f64, c.field.generation as f64, tick, c.field.schema as f64,
                &bytes[..n], )?; }
        Ok(()) }
    // receiveEntities applies each part at arrival, before the whole tick completes.
    // Stage all its groups first: a late malformed group must not partially change
    // the working assembly. Older ticks still supply history but not gameplay.
    fn entities(&mut self, bytes: &[u8], head: [f64; 9], bounds: &[f64; 6]) -> Result<()> {
        if self.held(head[2]).is_some() { return Ok(()); }
        let base = if head[4] == 0. { None } else { self.held(head[4]) };
        if head[4] != 0. && base.is_none() { self.missing();
            return Ok(()); }
        let at = self.assembly(head[2], head[4], base);
        let a = &self.assemblies[at];
        if a.count != 0 && a.count != head[6] as usize { return Err("snapshot part count changed"); }
        if a.parts[head[5] as usize] { return Ok(()); }
        let mut reader = Reader::new(bytes);
        reader.at = head[8] as usize;
        let n = reader.u16()? as usize;
        let start = reader.at;
        self.field_changes(&mut reader, base, false, bytes)?;
        if reader.at - start != n { return Err("snapshot world section size"); }
        let n = reader.varint()?;
        if n > 512. { return Err("snapshot record count"); }
        let mut touched = [false; l::SLOTS];
        let mut slot = -1.;
        for i in 0..n as usize { slot = if i == 0 { reader.u16()? as f64 } else { slot + reader.varint()? };
            if slot >= 512. { return Err("snapshot slot out of range"); }
            let index = slot as usize;
            let byte = reader.u8()?;
            let op = byte & 3;
            let mask = byte >> 2;
            if op == 2 { self.removed.push(index);
                continue; }
            let old = base.map_or(Entry::default(), |b| self.snapshots[b].entries[index]);
            let class = if op == 1 { reader.u8()? } else if op == 0 { if !old.present {
                    return Err("snapshot update without baseline entry"); }
                old.get(r::KIND) as u8 } else { return Err("snapshot op"); };
            if class > 5 { return Err("snapshot class"); }
            let mut entry = if op == 1 { Entry::default() } else { old };
            entry.present = true;
            entry.set(r::KIND, class as f64);
            if op == 1 { entry.set(r::GENERATION, reader.u32()? as f64);
                entry.set(r::ID, reader.u32()? as f64);
                entry.set(r::CLASS_ID, reader.u16()? as f64);
                entry.set(r::TEAM, reader.u8()? as f64);
                entry.set(r::DISC, 1.); }
            let groups = [6, 6, 6, 2, 4, 4][class as usize];
            if op == 1 && mask != (1 << groups) - 1 { return Err("snapshot new record mask"); }
            let baseline = BaselineSlot { exists: old.present, class, position: old .present
                    .then(|| [old.get(r::POSITION), old.get(r::POSITION + 1), old.get(r::POSITION + 2)]), };
            for g in 0..groups { if mask & (1 << g) != 0 { let mut values = [0.; 64];
                    let mut out = Output { values: &mut values, at: 0 };
                    group(&mut reader, class, g, if op == 1 { None } else { Some(&baseline) }, &mut out)?;
                    let n = out.at;
                    apply_group(&mut entry, g, &values[..n]); } }
            self.staging[index] = entry;
            if !touched[index] { touched[index] = true;
                self.changed.push(index); } }
        reader.finish("snapshot trailing bytes")?;
        // All reads used the immutable baseline, including repeated slots.
        // Install removals before the final changed rows, as applyEntities
        // did after decodeEntities finished constructing its changed Map.
        let snapshot = l::BASELINES + at;
        for &slot in &self.removed { self.snapshots[snapshot].remove(slot); }
        for &slot in &self.changed { self.snapshots[snapshot].set(slot, self.staging[slot]); }
        self.snapshots[snapshot].tick = head[2];
        self.snapshots[snapshot].epoch = self.epoch;
        self.snapshots[snapshot].valid = true;
        // Reordered parts still contribute their actual timestamps to history.
        // Only semantic application is monotonic; relabelling a late pose as
        // the newest tick would change interpolation and ownership timing.
        let gameplay = head[2] >= self.newest_entities;
        if gameplay { for i in 0..self.changed.len() { let entry = self.staging[self.changed[i]];
                let row = semantic(entry, head[2], bounds);
                self.emit(&row)?;
                if entry.get(r::KIND) == 2. { self.emit(&mount_record(entry, head[2]))?; } }
            self.emit_seats(snapshot)?;
            for i in 0..self.removed.len() {
                let entry = base.map_or(Entry::default(), |b| self.snapshots[b].entries[self.removed[i]]);
                if !entry.present { continue; }
                let mut row = [0.; r::STRIDE];
                row[r::KIND] = k::TOMBSTONE as f64;
                row[r::ID] = entry.get(r::ID);
                row[r::GENERATION] = entry.get(r::GENERATION);
                row[r::TICK] = head[2];
                self.emit(&row)?;
                if let Some(at) = self.seats.iter().position(|s| s.id == row[r::ID]) { self.seats.remove(at);
                    self.seat_change(row[r::ID], row[r::GENERATION], None)?; } }
            self.emit_fields(head[2])?;
            self.newest_entities = head[2]; }
        self.apply_fields(snapshot, false);
        if self.current.is_none_or(|i| head[2] >= self.snapshots[i].tick) { self.current = Some(snapshot); }
        self.pose_batch(head[2], bounds);
        let a = &mut self.assemblies[at];
        a.parts[head[5] as usize] = true;
        a.count = head[6] as usize;
        a.received += 1;
        self.header[h::ACK_INPUT] = a.ack;
        self.header[h::COMPLETE] = self.complete(at) as u8 as f64;
        self.header[h::VALID] = 1.;
        Ok(()) }
    // A seat output is an ownership fact, not a sampled transform. The body and
    // vehicle generations travel together so delayed leases cannot name a reused
    // resource; absence explicitly tells the adapter to release its current seat.
    fn seat_change(&mut self, id: f64, generation: f64, seat: Option<Seat>) -> Result<()> {
        let at = self.header[h::SEATS] as usize;
        if at >= l::SLOTS * 2 { return Err("snapshot seat publication full"); }
        let row = &mut self.seat_output[at * s::STRIDE..][..s::STRIDE];
        row.fill(0.);
        row[s::ID] = id;
        row[s::GENERATION] = generation;
        if let Some(seat) = seat { row[s::PRESENT] = 1.;
            row[s::VEHICLE_ID] = seat.vehicle;
            row[s::VEHICLE_GENERATION] = seat.vehicle_generation;
            row[s::SEAT_INDEX] = seat.index; }
        self.header[h::SEATS] += 1.;
        Ok(()) }
    // seatChangesFor resolves vehicle protocol slots in this tick's working
    // assembly, after every changed entry is installed. A vehicle can therefore
    // arrive after its rider within the same part without losing the lease.
    fn emit_seats(&mut self, snapshot: usize) -> Result<()> { for n in 0..self.changed.len() {
            let e = self.staging[self.changed[n]];
            if e.get(r::KIND) > 1. { continue; }
            let vehicle = if e.get(r::SLOT) >= 0. {
                self.snapshots[snapshot].entries.get(e.get(r::SLOT) as usize).filter(|e| e.present) } else { None };
            let seat = vehicle.map(|v| Seat { id: e.get(r::ID), generation: e.get(r::GENERATION), vehicle: v.get(r::ID),
                vehicle_generation: v.get(r::GENERATION), index: e.get(r::SEAT_COUNT), });
            if let Some(at) = self.seats.iter().position(|s| s.id == e.get(r::ID)) { if let Some(s) = seat {
                    self.seats[at] = s; } else { self.seats.remove(at); } } else if let Some(s) = seat {
                if self.seats.len() < tl::ENTITIES { self.seats.push(s); } }
            // The source comparison includes generation in the retained object,
            // but not its candidate. Every changed body therefore emits its seat.
            self.seat_change(e.get(r::ID), e.get(r::GENERATION), seat)?; }
        Ok(()) }
    // samplePoses includes only entries changed by this part. Repeating stale
    // baseline poses with a new timestamp would hold then jump at the real arrival.
    // These generated timeline rows stay in Rust through the display consumer.
    fn pose_batch(&mut self, tick: f64, bounds: &[f64; 6]) { let x = &mut self.timeline.input;
        x[tf::VERSION] = bfe_pose::timeline_layout::VERSION as f64;
        x[tf::EPOCH] = self.epoch;
        x[tf::TICK] = tick;
        x[tf::SNAPSHOTS] = self.maximum as f64;
        let mut count = 0;
        for &slot in &self.changed { let entry = self.staging[slot];
            let class = entry.get(r::KIND) as usize;
            if class > 2 { continue; }
            let row = semantic(entry, tick, bounds);
            let at = tf::STRIDE + count * tr::STRIDE;
            let target = &mut x[at..][..tr::STRIDE];
            target.fill(0.);
            target[tr::ID] = row[r::ID];
            target[tr::GENERATION] = row[r::GENERATION];
            target[tr::KIND] = [0., 2., 1.][class];
            target[tr::DISC] = entry.get(r::DISC);
            for (a, b, n) in
                [(r::POSITION, tr::POSITION, 3), (r::VELOCITY, tr::VELOCITY, 3), (r::QUATERNION, tr::QUATERNION, 4)]
            { target[b..b + n].copy_from_slice(&row[a..a + n]); }
            for (a, b) in [ (r::CLASS_ID, tr::CLASS_ID), (r::TEAM, tr::TEAM), (r::HEALTH, tr::HEALTH),
                (r::STATE, tr::STATE), (r::YAW, tr::YAW), (r::PITCH, tr::PITCH), (r::POSTURE, tr::POSTURE),
                (r::WEAPON, tr::WEAPON), (r::FIRE_COUNTER, tr::FIRE_COUNTER), (r::MODE, tr::MODE),
                (r::REVERSE, tr::REVERSE), (r::DEAD, tr::DEAD), (r::SHIELD, tr::SHIELD), (r::SHIELD_ON, tr::SHIELD_ON),
                (r::TIME, tr::TIME), (r::ROLL, tr::ROLL), (r::LEAN, tr::LEAN), (r::LEGS, tr::LEGS),
                (r::POWER, tr::POWER), ] { target[b] = row[a]; }
            // Hulls carry a wire firing byte but the former sampled hull record
        // omitted it. Only body kinds participate in firing-counter unwrap;
        // adding hull counters here would invent an observable sample field.
        target[tr::HAS_FIRE_COUNTER] = if class < 2 { 1. } else { 0. };
            count += 1; }
        x[tf::COUNT] = count as f64;
        self.pending = true; }
    // receiveOwn decodes correction, summaries and cosmetics in their source order.
    // Correction/summary failures request baseline repair. Cosmetic unpack errors
    // remain distinct, and duplicate own packets cannot reapply gameplay state.
    fn own(&mut self, bytes: &[u8], head: [f64; 9], bounds: &[f64; 6]) -> Result<()> {
        let base = if head[4] == 0. { None } else { self.held(head[4]) };
        if head[4] != 0. && base.is_none() { self.missing();
            return Ok(()); }
        let mut reader = Reader::new(bytes);
        reader.at = 20;
        let flags = match reader.u8() {Ok(flags)=>flags,Err(_)=>{self.missing();return Ok(());}};
        let mut correction = None;
        let mut body_bytes = [0; l::PAYLOAD];
        let mut body_len = 0;
        // The correction delta may contain NaNs or signed zero as opaque bytes.
        // Its length and word mask are validated, but no numeric conversion
        // may rewrite those payload bits before prediction consumes them.
        let parsed = (|| -> Result<()> { if flags & 1 != 0 { let mut body = reader.counted_section()?;
                let id = body.u32()?;
                let generation = body.u32()?;
                let schema = body.u16()?;
                let previous = base .filter(|&b| self.snapshots[b].has_payload)
                    .map(|b| &self.snapshots[b].payload[..self.snapshots[b].length]);
                let n = body.bytes.len() - body.at;
                let mut scratch = [0.; 256];
                let mut out = Output { values: &mut scratch, at: 0 };
                delta(body.section(n)?, previous, &mut out).map_err(|_| "own baseline not held")?;
                if scratch[0] == 0. { body_len = scratch[2] as usize;
                    let start = scratch[1] as usize;
                    if body_len > l::PAYLOAD { return Err(EXCHANGE); }
                    body_bytes[..body_len].copy_from_slice(&bytes[start..start + body_len]); } else {
                    body_len = scratch[1] as usize;
                    if body_len > l::PAYLOAD { return Err(EXCHANGE); }
                    let raw = unsafe { std::slice::from_raw_parts(scratch.as_ptr().add(2).cast::<u8>(), body_len) };
                    body_bytes[..body_len].copy_from_slice(raw); }
                correction = Some((id, generation, schema)); }
            if flags & 2 != 0 { let mut section = reader.counted_section()?;
                self.field_changes(&mut section, base, true, bytes)?;
                section.finish("snapshot world section trailing bytes")?; }
            Ok(()) })();
        if parsed.is_err() { self.missing();
            return Ok(()); }
        // receiveOwn masks the complete datagram decoder, including section
        // lengths and trailing bytes. Unpacking the valid cosmetic bundle
        // happens afterwards, so its own malformed-event errors still throw.
        let cosmetics=if flags&4!=0 {match reader.counted_section(){Ok(section)=>Some(section),Err(_)=>{self.missing();return Ok(());}}}else{None};
        if reader.finish("own datagram trailing bytes").is_err(){self.missing();return Ok(());}
        if let Some(bundle) = cosmetics { let mut reader = bundle;
            while reader.at < reader.bytes.len() { let mut values = [0.; 256];
                let mut out = Output { values: &mut values, at: 0 };
                cosmetic(reader.counted_section()?, bounds, &mut out)?;
                let id = values[0];
                let generation = values[1];
                let tick = values[2];
                let schema = values[3];
                if values[4] == 0. { let start = values[5] as usize;
                    let n = values[6] as usize;
                    self.bytes_record(k::COMBAT, id, generation, tick, schema, &bytes[start..start + n])?; } else {
                    let n = values[5] as usize;
                    let raw = unsafe { std::slice::from_raw_parts(values.as_ptr().add(6).cast::<u8>(), n) };
                    self.bytes_record(k::COMBAT, id, generation, tick, schema, raw)?; } } }
        // A second own datagram can arrive first. Remember its completion
        // fact independently from the first datagram's correction/ACK data;
        // repeated cosmetics still pass through their existing deduplication.
        if flags & 8 != 0 { if self.held(head[2]).is_none() { let at = self.assembly(head[2], head[4], base);
                self.assemblies[at].second = true;
                self.header[h::COMPLETE] = self.complete(at) as u8 as f64; }
            self.header[h::VALID] = 1.;
            return Ok(()); }
        if self.held(head[2]).is_some() { self.header[h::RECORDS] = 0.;
            return Ok(()); }
        let at = self.assembly(head[2], head[4], base);
        if self.assemblies[at].own { self.header[h::RECORDS] = 0.;
            return Ok(()); }
        let snapshot = l::BASELINES + at;
        if head[2] >= self.newest_own { if let Some((id, generation, schema)) = correction { self.bytes_record(
                    k::CORRECTION, id as f64, generation as f64, head[2], schema as f64, &body_bytes[..body_len], )?; }
            self.emit_fields(head[2])?;
            self.newest_own = head[2]; }
        if correction.is_some() { self.snapshots[snapshot].payload[..body_len].copy_from_slice(&body_bytes[..body_len]);
            self.snapshots[snapshot].length = body_len;
            self.snapshots[snapshot].has_payload = true; }
        self.apply_fields(snapshot, true);
        let a = &mut self.assemblies[at];
        a.own = true;
        a.own_parts = head[6] as usize;
        a.ack = head[7];
        self.header[h::COMPLETE] = self.complete(at) as u8 as f64;
        self.header[h::VALID] = 1.;
        Ok(()) } }

// Group values are already validated and in wire order. Retain encoded values
// for future deltas; semantic conversion happens only for changed records.
// decodeEntities applies selected class groups to the baseline's wire row.
// A new entry already required its complete class mask. Group order differs
// by class, so neither gameplay record offsets nor seat ordinals substitute.
fn apply_group(entry: &mut Entry, group: u8, values: &[f64]) { let class = entry.get(r::KIND) as u8;
    if class != 3 && group == 0 { entry.set(r::DISC, values[0]);
        entry.range(r::POSITION, &values[1..4]);
        return; }
    if class != 3 && group == 1 { entry.wide = values[0] != 0.;
        entry.range(r::VELOCITY, &values[1..4]);
        return; }
    let fields: &[usize] = match (class, group) { (0 | 1 | 5, 2) => &[r::YAW, r::PITCH], (0, 3) => &[r::HEALTH],
        (0, 4) => &[r::POSTURE, r::CHARGE, r::STATE, r::WEAPON, r::SLOT, r::SEAT_COUNT],
        (1, 3) => &[r::HEALTH, r::POSTURE, r::STATE, r::WEAPON, r::SLOT, r::SEAT_COUNT],
        (1, 4) => &[r::MODE, r::REVERSE, r::DEAD, r::SHIELD_ON, r::TIME, r::ROLL, r::LEAN, r::LEGS, r::SHIELD],
        (0 | 1 | 2, 5) => &[r::FIRE_COUNTER], (2, 2) => { entry.range(r::QUATERNION, values);
            return; }
        (2, 3) => &[r::HEALTH, r::STATE, r::POWER], (2, 4) => { let n = values[5] as usize;
            for (i, at) in
                [r::SLOT, r::TILT, r::MOUNT_ROLL, r::WALK_PHASE, r::WALK_BLEND, r::SEAT_COUNT].into_iter().enumerate()
            { entry.set(at, values[i]); }
            entry.range(r::SEAT_ANGLES, &values[6..6 + n * 2]);
            return; }
        (3, 0) => &[r::YAW, r::PITCH, r::GUN_YAW, r::GUN_PITCH], (3, 1) => &[r::HEALTH, r::STATE, r::TEAM],
        (4, 2) => &[r::YAW], (4, 3) => { entry.set(r::ACTIVE, values[0]);
            entry.set(r::DEFINITION_ID, values[1]);
            entry.range(r::UP, &values[2..5]);
            return; }
        (5, 3) => { entry.set(r::ACTIVE, values[0]);
            entry.set(r::DEFINITION_ID, values[1]);
            if values[0] != 0. { entry.set(r::TEAM, values[2]);
                entry.set(r::HEALTH, values[3]); } else { entry.set(r::HEALTH, values[2]); }
            return; }
        _ => &[], };
    for (i, &at) in fields.iter().enumerate() { entry.set(at, values[i]); } }
// recordFromEntry dequantizes only changed gameplay records and direct samples.
// The original multiply/divide order is shared with the proven wire reader;
// retained baselines never round-trip through these floating-point values.
fn semantic(entry: Entry, tick: f64, bounds: &[f64; 6]) -> [f64; r::STRIDE] { let mut d = entry.values();
    d[r::TICK] = tick;
    let class = d[r::KIND] as u8;
    if class == 3 { for at in [r::YAW, r::PITCH, r::GUN_YAW, r::GUN_PITCH] { d[at] = d[at] * PI / 32767.; }
        d[r::BUILDING] = d[r::CLASS_ID];
        return d; }
    let position = position_values([d[r::POSITION], d[r::POSITION + 1], d[r::POSITION + 2]], bounds);
    d[r::POSITION..r::POSITION + 3].copy_from_slice(&position);
    for i in 0..3 { d[r::VELOCITY + i] = if entry.wide { d[r::VELOCITY + i] / 32. } else { d[r::VELOCITY + i] * 0.25 };
    }
    if class == 2 { let q =
            quaternion(d[r::QUATERNION] as usize, [d[r::QUATERNION + 1], d[r::QUATERNION + 2], d[r::QUATERNION + 3]]);
        d[r::QUATERNION..r::QUATERNION + 4].copy_from_slice(&q); } else { d[r::YAW] /= ANGLE;
        d[r::PITCH] /= ANGLE; }
    if class == 4 { for i in 0..3 { d[r::UP + i] /= 32767.; } }
    d }
// mountRecordFromEntry gives lean and seat angles their signed angle scale.
// Walk phase and blend use separate units; keeping those conversions distinct
// preserves authored walker timing and the order of every seat angle pair.
fn mount_record(entry: Entry, tick: f64) -> [f64; r::STRIDE] { let mut d = entry.values();
    d[r::KIND] = k::MOUNT as f64;
    d[r::TICK] = tick;
    d[r::TILT] = d[r::TILT] * PI / 32767.;
    d[r::MOUNT_ROLL] = d[r::MOUNT_ROLL] * PI / 32767.;
    d[r::WALK_PHASE] /= 1000.;
    d[r::WALK_BLEND] /= 255.;
    for i in 0..(d[r::SEAT_COUNT] as usize).min(16) * 2 { d[r::SEAT_ANGLES + i] = d[r::SEAT_ANGLES + i] * PI / 32767.; }
    d }

impl Session {
    /// Resource prerequisites are occasional packet-time lookups, never a
    /// display sampler. The same changed-record layout returns their values.
    pub fn lookup(&mut self, kind: u32, id: f64, bounds: &[f64; 6]) -> bool { self.header[h::RECORDS] = 0.;
        let Some(index) = self.current else { return false; };
        let row = &self.snapshots[index];
        let class = if kind == k::MOUNT { k::VEHICLE } else { kind };
        let Some(entry) =
            row.order.iter().map(|&i| row.entries[i]).find(|e| e.get(r::KIND) == class as f64 && e.get(r::ID) == id)
        else { return false; };
        let record = if kind == k::MOUNT { mount_record(entry, row.tick) } else { semantic(entry, row.tick, bounds) };
        self.emit(&record).is_ok() }
    /// An explicit, cold replay capture. Live receive and display never call
    /// this serializer or reconstruct immutable baseline objects on the page.
    pub fn capture(&self, tick: f64) -> serde_json::Value { use serde_json::json;
        let snapshot = if tick.is_nan() { self.current } else { self.held(tick) };
        let value = snapshot.map(|i| self.capture_snapshot(i));
        let seats:Vec<_>=self.seats.iter().map(|s|json!([s.id,{"vehicleId":s.vehicle,"vehicleGeneration":s.vehicle_generation,"seatIndex":s.index,"generation":s.generation}])).collect();
        // Include staged changes independently from current: an older packet
        // can contribute display samples without replacing the newest view.
        // This distinction is inspected only by explicit replay capture.
        json!({"epoch":self.epoch,"ackTick":self.ack,"missingBaselines":self.missed,"seats":seats,"snapshot":value,"changed":self.changed.iter().map(|&i|capture_entry(self.staging[i])).collect::<Vec<_>>()})
    }
    // Cold corpus captures preserve Map insertion order and opaque byte ownership.
    // They serialize retained rows explicitly; no packet or displayed frame calls
    // this path, and its temporary collections never enter the allocation claim.
    fn capture_snapshot(&self, index: usize) -> serde_json::Value { use serde_json::json;
        let row = &self.snapshots[index];
        let fields = |fields: &[u16]| -> Vec<serde_json::Value> {
            fields.iter().map(|&handle|{let f=self.bank.meta[handle as usize];json!([format!("{}:{}",f.schema,f.id),{"id":f.id,"schema":f.schema,"generation":f.generation,"payload":self.bank.get(f.handle)}])}).collect()
        };
        let slots: Vec<_> = row.order.iter().map(|&i| json!([i, capture_entry(row.entries[i])])).collect();
        json!({"tick":row.tick,"epoch":row.epoch,"slots":slots,"world":fields(&row.world),"own":{"payload":if row.has_payload{Some(&row.payload[..row.length])}else{None},"summaries":fields(&row.own)}})
    } }
// The diagnostic shape mirrors encoded entries, including absent class fields.
// It is intentionally separate from gameplay records: reverse conversion would
// lose the exact baseline codes that protocol equality needs to inspect.
// Class codes were checked before an entry entered any retained snapshot.
// The capture therefore needs no alternate decode or error-recovery path;
// gameplay never depends on this diagnostic object's allocation or identity.
fn capture_entry(entry: Entry) -> serde_json::Value { use serde_json::json;
    let d = entry.values();
    let class = d[r::KIND] as usize;
    let classes = ["soldier", "droideka", "vehicle", "turret", "item", "remote"];
    let mut out = json!({"cls":classes[class],"generation":d[r::GENERATION],"entityId":d[r::ID],"classId":d[r::CLASS_ID],"team":d[r::TEAM],"disc":d[r::DISC]!=0.});
    let fields = out.as_object_mut().unwrap();
    let mut scalar = |name: &str, at: usize| { fields.insert(name.into(), json!(d[at])); };
    if class == 3 { for (name, at) in [ ("yaw", r::YAW), ("pitch", r::PITCH), ("gunYaw", r::GUN_YAW),
            ("gunPitch", r::GUN_PITCH), ("health", r::HEALTH), ("state", r::STATE), ] { scalar(name, at); }
        return out; }
    fields.insert("pos".into(), json!(&d[r::POSITION..r::POSITION + 3]));
    fields.insert("vel".into(), json!({"wide":entry.wide,"v":&d[r::VELOCITY..r::VELOCITY+3]}));
    if class != 2 { fields.insert("yaw".into(), json!(d[r::YAW]));
        if class != 4 { fields.insert("pitch".into(), json!(d[r::PITCH])); } }
    if class <= 2 { for (name, at) in [("health", r::HEALTH), ("state", r::STATE), ("fireCounter", r::FIRE_COUNTER)] {
            fields.insert(name.into(), json!(d[at])); } }
    if class <= 1 { for (name, at) in
            [("posture", r::POSTURE), ("weapon", r::WEAPON), ("seatSlot", r::SLOT), ("seatIndex", r::SEAT_COUNT)]
        { fields.insert(name.into(), json!(d[at])); }
        if class == 0 { fields.insert("charge".into(), json!(d[r::CHARGE])); } else {
            fields.insert("mode".into(), json!(d[r::MODE]));
            for (name, at) in [("reverse", r::REVERSE), ("dead", r::DEAD), ("shieldOn", r::SHIELD_ON)] {
                fields.insert(name.into(), json!(d[at] != 0.)); }
            fields.insert("anim".into(), json!([d[r::TIME], d[r::ROLL], d[r::LEAN], d[r::LEGS], d[r::SHIELD]])); } }
    if class == 2 { fields.insert("power".into(), json!(d[r::POWER]));
        fields.insert("quat".into(), json!({"index":d[r::QUATERNION],"v":&d[r::QUATERNION+1..r::QUATERNION+4]}));
        let seats: Vec<_> = (0..d[r::SEAT_COUNT] as usize)
            .map(|i| [d[r::SEAT_ANGLES + i * 2], d[r::SEAT_ANGLES + i * 2 + 1]])
            .collect();
        fields.insert("mount".into(),json!({"slot":if d[r::SLOT]<0.{None}else{Some(d[r::SLOT])},"tilt":d[r::TILT],"roll":d[r::MOUNT_ROLL],"walkPhase":d[r::WALK_PHASE],"walkBlend":d[r::WALK_BLEND],"seats":seats}));
    }
    if class >= 4 { fields.insert("active".into(), json!(d[r::ACTIVE] != 0.));
        fields.insert("definitionId".into(), json!(d[r::DEFINITION_ID]));
        if class == 4 { fields.insert("up".into(), json!(&d[r::UP..r::UP + 3])); } else {
            fields.insert("health".into(), json!(d[r::HEALTH])); } }
    out }
