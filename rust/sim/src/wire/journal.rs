//! Retained publication views of the ordered world journal. The enclosing
//! tick still delivers foreign presentation facts; canonical packet metadata,
//! compact bytes, recipient selection and retention belong to this owner.
use std::rc::Rc;
use super::{codec::{Result, Writer}, cosmetics::{self, Compact, Event, Kind, Metadata, Record, View}};
const BATCH: usize = 4096;
fn empty_event() -> Rc<Event> { Rc::new(Event { identity: 0, schema: 0., meta: Metadata { key: Vec::with_capacity(11), kind: Kind::Presentation,
    projectile: None, must: false, expires: 0. }, view: View::default(), bytes: Vec::with_capacity(512) }) }
fn empty_record() -> Record { Record { id: 0., generation: 0., tick: 0., schema: 0., combat: true, payload: Vec::with_capacity(78) } }

pub struct Events {
    pool: Vec<Rc<Event>>, free: Vec<usize>, busy: Vec<usize>, cosmetics: Vec<Rc<Event>>, expiry: Vec<f64>,
    gameplay: Vec<Record>, records: Vec<Record>, compact: Compact, writer: Writer, payload: Vec<u8>, serial: u64,
    pub malformed: u64, pub saturated: u64,
}
impl Default for Events {
    fn default() -> Self {
        let mut out = Self { pool: Vec::new(), free: Vec::new(), busy: Vec::new(), cosmetics: Vec::with_capacity(BATCH), expiry: Vec::with_capacity(BATCH),
            gameplay: Vec::with_capacity(BATCH), records: (0..BATCH).map(|_| empty_record()).collect(),
            compact: Compact { offset: Vec::with_capacity(3), ..Default::default() }, writer: Writer(Vec::with_capacity(512)),
            payload: Vec::with_capacity(900), serial: 0, malformed: 0, saturated: 0 };
        out.reserve(0); out
    }
}
impl Events {
    // CombatCapture.drain retains shared event bytes once across recipients.
    // The measured 32-client peak is 351 entries; admission leaves headroom.
    // Only owed entries hold payloads after their source batch is released.
    pub fn reserve(&mut self, clients: usize) {
        let count = 512 + clients.min(256) * 8;
        self.pool.reserve(count.saturating_sub(self.pool.len())); self.free.reserve(count.saturating_sub(self.free.len())); self.busy.reserve(count.saturating_sub(self.busy.len()));
        while self.pool.len() < count { self.free.push(self.pool.len()); self.pool.push(empty_event()); }
    }
    // CombatCapture.drain can exceed the ordinary battle batch after a pause.
    // Reserve its retained identities before encoding any event. The shared
    // pool grows with outstanding bytes, independently of recipient count.
    pub fn prepare(&mut self, incoming: usize) -> Result<()> {
        let needed = self.busy.len() + incoming.min(BATCH * 2);
        if needed <= self.pool.len() { return Ok(()); }
        let count = needed.next_power_of_two();
        self.pool.try_reserve(count - self.pool.len()).map_err(|_| "publication event capacity exceeded")?;
        self.free.try_reserve(count).map_err(|_| "publication event capacity exceeded")?;
        self.busy.try_reserve(count).map_err(|_| "publication event capacity exceeded")?;
        while self.pool.len() < count { self.free.push(self.pool.len()); self.pool.push(empty_event()); }
        Ok(())
    }
    // SnapshotHost.publish drains one ordered batch after shared capture.
    // Clearing a batch does not cancel receipts retained by client outboxes.
    // Recycle its gameplay payloads before accepting the next journal.
    pub fn begin(&mut self) {
        self.cosmetics.clear(); self.expiry.clear(); self.records.extend(self.gameplay.drain(..));
        // Only owed queue entries retain Event Rcs. Inflight receipts retain
        // identities alone, so all other bytes can return to this free bank.
        self.busy.retain(|&at| { if Rc::strong_count(&self.pool[at]) == 1 { self.free.push(at); false } else { true } });
    }
    pub fn load(&self) -> (usize, usize, usize, usize) { (self.pool.len(), self.busy.len(), self.cosmetics.iter().map(|e| e.bytes.len()).sum(), self.cosmetics.iter().map(|e| e.bytes.len()).max().unwrap_or(0)) }
    pub fn len(&self) -> usize { self.cosmetics.len() + self.gameplay.len() }
    /// CombatCapture.emit prunes before capacity and decoded-view checks.
    /// Keep the journal lifetime distinct from cosmetic delivery's deadline.
    pub fn prune(&mut self, tick: f64) {
        let mut at = 0;
        self.cosmetics.retain(|_| { let keep = self.expiry[at] >= tick; at += 1; keep });
        self.expiry.retain(|&expiry| expiry >= tick);
        let mut at = 0;
        while at < self.gameplay.len() {
            let payload = &self.gameplay[at].payload;
            let expiry = u32::from_le_bytes(payload[..4].try_into().unwrap()) as f64;
            if expiry < tick { self.records.push(self.gameplay.remove(at)); } else { at += 1; }
        }
        self.busy.retain(|&at| { if Rc::strong_count(&self.pool[at]) == 1 { self.free.push(at); false } else { true } });
    }
    pub fn cosmetics(&self) -> &[Rc<Event>] { &self.cosmetics }
    pub fn gameplay(&self) -> &[Record] { &self.gameplay }
    // encodeCosmeticEvent consumes borrowed payload bytes immediately.
    // Move the reusable scratch Vec through Record instead of cloning it.
    // The foreign journal remains readable until this complete call returns.
    pub fn emit_raw(&mut self, id: f64, generation: f64, tick: f64, schema: f64, payload: &[u8], view: View) {
        if payload.len() > self.payload.capacity() { self.malformed += 1; return; }
        let mut record = Record { id, generation, tick, schema, combat: true, payload: std::mem::take(&mut self.payload) };
        record.payload.clear(); record.payload.extend_from_slice(payload);
        if self.emit(&record, view).is_err() { self.malformed += 1; }
        self.payload = record.payload;
    }
    fn emit(&mut self, record: &Record, view: View) -> Result<()> {
        // gameplayFor retains damage and death in their reliable record format.
        // Cosmetic compaction must not change these payloads or recipient ids.
        // The 78-byte feedback shape bounds every pooled gameplay allocation.
        if record.schema == 3. || record.schema == 4. {
            if record.payload.len() != 78 { return Err("invalid feedback payload"); }
            let Some(mut saved) = self.records.pop() else { self.saturated += 1; return Ok(()) };
            saved.id = record.id; saved.generation = record.generation; saved.tick = record.tick; saved.schema = record.schema;
            saved.payload.clear(); saved.payload.extend_from_slice(&record.payload); self.gameplay.push(saved); return Ok(());
        }
        if self.cosmetics.len() == BATCH { self.saturated += 1; return Ok(()); }
        // compactCosmeticRecord quantizes launch and impact geometry once.
        // Its decoded rounded view also drives distance priority and expiry.
        // Using the unrounded source view would change which client receives it.
        let compact = record.schema == 1. || record.schema == 2.;
        if compact { cosmetics::decode_compact_into(record, &mut self.compact)?; }
        let bytes = if compact { 256 } else { record.payload.len() + 32 };
        if bytes > self.writer.0.capacity() { self.writer.0.try_reserve(bytes.saturating_sub(self.writer.0.len()))
            .map_err(|_| "publication event capacity exceeded")?; }
        cosmetics::encode_cosmetic_event_into(record, &super::records::default_bounds(), compact.then_some(&self.compact), &mut self.writer)?;
        let Some(at) = self.free.pop() else { self.saturated += 1; return Ok(()) };
        let event = Rc::get_mut(&mut self.pool[at]).ok_or("publication event bank still borrowed")?;
        self.serial = self.serial.wrapping_add(1); event.identity = self.serial; event.schema = record.schema;
        event.view = if compact { self.compact.view.clone() } else { view };
        let (prefix, kind, projectile, must, expires) = if record.schema == 1. { ('l', Kind::Launch, Some(record.id), true, (self.compact.execution_tick + self.compact.view.life * 60.).ceil() + 24.) }
            else if record.schema == 2. || record.schema == 7. { ('t', Kind::Terminal, Some(record.id), true, record.tick + 18. + 24.) }
            else if record.schema == 6. { ('s', Kind::Sample, Some(record.id), false, record.tick + 18.) }
            else if record.schema == 5. { ('e', Kind::Explosion, None, true, record.tick + 18.) }
            else { ('e', Kind::Presentation, None, false, record.tick + 24.) };
        event.meta.kind = kind; event.meta.projectile = projectile; event.meta.must = must; event.meta.expires = expires;
        // Wire ids were validated by the encoder. Their decimal key is at
        // most ten digits; write it without formatting or allocating text.
        event.meta.key.clear(); event.meta.key.push(prefix as u16);
        let mut digits = [0; 10]; let mut cursor = digits.len(); let mut id = record.id as u32;
        loop { cursor -= 1; digits[cursor] = (id % 10) as u16 + 48; id /= 10; if id == 0 { break; } }
        event.meta.key.extend_from_slice(&digits[cursor..]);
        std::mem::swap(&mut event.bytes, &mut self.writer.0);
        let expiry = if record.schema == 8. { record.tick + 120. } else if record.payload.len() >= 4 { u32::from_le_bytes(record.payload[..4].try_into().unwrap()) as f64 } else { 0. };
        self.cosmetics.push(self.pool[at].clone()); self.expiry.push(expiry); self.busy.push(at); Ok(())
    }
}
// gameplayFor sends damage to attacker and target, using protocol ids.
// Death is broadcast by the caller rather than this pair predicate.
// A short invalid payload cannot be interpreted as an accidental id zero.
pub fn concerns(record: &Record, id: f64) -> bool {
    if record.payload.len() < 16 { return false; }
    let at = |index| u32::from_le_bytes(record.payload[index..index + 4].try_into().unwrap()) as f64;
    at(4) == id || at(12) == id
}
