//! CosmeticOutbox's publication path with retained queue, TimSort and packet
//! storage. Event Rc identities survive replacement and cumulative ACKs;
//! neither a key clone nor a packet-sized allocation occurs while publishing.
use std::{collections::VecDeque, rc::Rc};
use jsrt::{JsMap, js_max, sort::{SortScratch, js_sort_buffered}};
use super::{codec::{Result, Writer}, cosmetics::{self, Event, Entry, Kind, Owner, Projectile}};

const OWED: u8 = 2;
const MAX_EVENTS: usize = 1024;
const MAX_BATCH: usize = 4096;
const PROJECTILES: usize = 32768;
const RESEND: f64 = 30.;
// Every encoded event has a 15-byte header, a length and a payload. Two
// datagrams therefore retain at most this many receipts in one publication.
const RECEIPTS: usize = 104;
// Valid projectile ids are finite u32 values; NaN marks absent attribution.
// Receipts retain identity and attribution without copying each event key.
struct Receipt { identity: u64, projectile: f64 }
struct Sent { tick: f64, events: Vec<Receipt> }
impl Receipt {
    fn of(event: &Event) -> Option<Self> {
        if event.meta.key.len() > 11 { return None; }
        Some(Self { identity: event.identity, projectile: event.meta.projectile.unwrap_or(f64::NAN) })
    }
}

pub struct Outbox {
    range: f64,
    entries: Vec<Entry>, candidates: Vec<Entry>, sort: SortScratch<Entry>,
    inflight: VecDeque<Sent>, spare: Vec<Sent>,
    projectiles: JsMap<f64, Projectile>,
    first: Writer, second: Writer,
    pub dropped: f64, pub owed_dropped: f64, pub resent: f64,
    pub history_evictions: u64, pruned: f64,
}
impl Outbox {
    // CosmeticOutbox allocates queue, receipts and sort scratch at admission.
    // Expired projectile history is reclaimed in place after packing.
    // A saturated history drops its oldest identity and counts that event.
    // It must never grow a frame-path table or panic after a long pause.
    pub fn new(range: f64) -> Self {
        let mut projectiles = JsMap::new(); projectiles.reserve(256);
        let mut sort = SortScratch::default(); sort.reserve(256);
        Self { range, entries: Vec::with_capacity(512), candidates: Vec::with_capacity(256), sort,
            inflight: VecDeque::with_capacity(32), spare: (0..32).map(|_| Sent { tick: 0., events: Vec::with_capacity(RECEIPTS) }).collect(),
            projectiles, first: Writer(Vec::with_capacity(1100)), second: Writer(Vec::with_capacity(1100)),
            dropped: 0., owed_dropped: 0., resent: 0., history_evictions: 0, pruned: f64::NEG_INFINITY }
    }
    /// CosmeticOutbox admits a batch before packing. Measured battles need
    /// fewer than 256 queued events and 128 projectile identities per peer.
    /// Larger journals grow here, before packet/receipt borrows are taken.
    pub fn prepare(&mut self, batch: usize) -> Result<()> {
        let needed = self.entries.len() + batch.min(MAX_BATCH);
        if needed > self.entries.capacity() { self.entries.try_reserve(needed.saturating_mul(2).saturating_sub(self.entries.len()))
            .map_err(|_| "cosmetic retention capacity exceeded")?; }
        let candidates = self.entries.len().min(MAX_EVENTS);
        if candidates > self.candidates.capacity() {
            self.candidates.try_reserve(candidates.saturating_sub(self.candidates.len())).map_err(|_| "cosmetic retention capacity exceeded")?;
            self.sort.try_reserve(candidates).map_err(|_| "cosmetic retention capacity exceeded")?;
        }
        self.projectiles.try_reserve((self.projectiles.size() + RECEIPTS).next_power_of_two().min(PROJECTILES + MAX_EVENTS))
            .map_err(|_| "cosmetic retention capacity exceeded")?;
        Ok(())
    }
    /// offer replaces a live key in place, then drops oldest entries after
    /// the whole batch. Dropping while inserting would change replacement order.
    pub fn offer(&mut self, events: &[Rc<Event>], position: Option<[f64; 3]>, owner: Owner) {
        for event in events.iter().take(MAX_BATCH) {
            // These are wire records, not arbitrary byte blobs. Reject an
            // invalid header here so receipt bounds cannot grow while packing.
            if event.bytes.len() < 19 || event.meta.key.len() > 11 { self.dropped += 1.; continue; }
            let known = event.meta.projectile.and_then(|id| self.projectiles.get(&id));
            let confirmed = known.is_some_and(|k| k.confirmed);
            let mut d2 = cosmetics::cosmetic_priority(&event.view, position, owner, self.range);
            if event.meta.kind == Kind::Terminal {
                if known.is_some() { d2 = d2.or(Some(self.range * self.range)); }
                else if event.schema == 7. { continue; }
            }
            let Some(d2) = d2 else { continue };
            if event.meta.kind == Kind::Terminal {
                let id = event.meta.projectile.unwrap_or(f64::NAN);
                self.entries.retain(|e| !(e.event.meta.kind == Kind::Sample && e.event.meta.projectile == Some(id)));
            }
            let rank = if d2 < 0. { 0 } else if event.meta.kind == Kind::Launch || event.meta.kind == Kind::Sample && !confirmed { 1 }
                else if event.meta.must { OWED } else if event.meta.kind == Kind::Presentation { 3 } else { 4 };
            let value = Entry { event: event.clone(), d2, rank, sent_tick: -1. };
            if let Some(entry) = self.entries.iter_mut().find(|e| e.event.meta.key == event.meta.key) { *entry = value; }
            else { self.entries.push(value); }
        }
        let excess = self.entries.len().saturating_sub(MAX_EVENTS);
        self.entries.drain(..excess); self.dropped += excess as f64;
        self.dropped += events.len().saturating_sub(MAX_BATCH) as f64;
    }
    // CosmeticOutbox.acknowledge distinguishes cumulative loss from delivery.
    // Only the exact acknowledged publication confirms projectile identity.
    // Earlier batches become resend candidates in their original queue order.
    fn settle(&mut self, mut sent: Sent, delivered: bool) {
        for event in sent.events.drain(..) {
            if delivered { if let Some(known) = self.projectiles.get_mut(&event.projectile).filter(|_| event.projectile.is_finite()) { known.confirmed = true; } }
            let Some(at) = self.entries.iter().position(|e| e.event.identity == event.identity) else { continue };
            let entry = &mut self.entries[at];
            if entry.event.identity != event.identity || entry.sent_tick != sent.tick { continue; }
            if delivered { self.entries.remove(at); } else { entry.sent_tick = -1.; self.resent += 1.; }
        }
        self.spare.push(sent);
    }
    // SnapshotHost accepts an ACK only while both baseline rings hold it.
    // This owner therefore settles receipts after that admission succeeds.
    // A stale transport ACK cannot confirm a payload the client never got.
    pub fn acknowledge(&mut self, tick: f64) {
        while self.inflight.front().is_some_and(|s| s.tick <= tick) {
            let sent = self.inflight.pop_front().unwrap(); let delivered = sent.tick == tick; self.settle(sent, delivered);
        }
    }
    // Resync releases every inflight receipt for another delivery attempt.
    // It does not forget confirmed projectile history or reorder live entries.
    // The client baseline owner decides when this transition is required.
    pub fn resend(&mut self) { while let Some(sent) = self.inflight.pop_front() { self.settle(sent, false); } }
    /// pack keeps insertion order when everything fits, otherwise V8's stable
    /// rank/distance sort. Sent batches retain events, never borrowed producers.
    pub fn pack(&mut self, tick: f64, first_budget: f64, second_budget: f64, confirmable: bool, spare: f64) -> Result<()> {
        self.entries.retain(|entry| {
            if entry.event.meta.expires >= tick || entry.event.meta.expires.is_nan() { return true; }
            if entry.sent_tick < 0. { self.dropped += 1.; if entry.rank <= OWED { self.owed_dropped += 1.; } } false
        });
        if tick >= self.pruned + RESEND || tick < self.pruned {
            self.pruned = tick; let mut cursor = 0;
            while let Some((at, id, p)) = self.projectiles.next_after(cursor) { cursor = at + 1; if p.expires < tick { self.projectiles.delete(&id); } }
        }
        while self.inflight.front().is_some_and(|s| s.tick <= tick - RESEND) {
            let sent = self.inflight.pop_front().unwrap(); self.settle(sent, false);
        }
        // CosmeticOutbox.pack preserves insertion order while all bytes fit.
        // Overflow uses V8 stable rank/distance ordering, including tied entries.
        // Reusable TimSort storage keeps its temporary Rc copies off the allocator.
        self.candidates.clear(); let mut bytes = 0;
        for entry in &self.entries {
            if entry.sent_tick >= 0. || !confirmable && entry.rank <= OWED { continue; }
            bytes += 2 + entry.event.bytes.len(); self.candidates.push(entry.clone());
        }
        if bytes as f64 > first_budget { js_sort_buffered(&mut self.candidates,
            &mut |a, b| if a.rank != b.rank { a.rank as f64 - b.rank as f64 } else { a.d2 - b.d2 }, &mut self.sort); }
        self.first.0.clear(); self.second.0.clear(); let mut refinements = 0;
        // Publication cannot create more than one batch per tick. If a caller
        // fills all slots anyway, settle the oldest as lost, matching expiry.
        if self.spare.is_empty() { if let Some(sent) = self.inflight.pop_front() { self.settle(sent, false); } }
        let mut owed = self.spare.pop().ok_or("cosmetic retention capacity exceeded")?; owed.tick = tick;
        for entry in self.candidates.drain(..) {
            let event = entry.event; let size = 2 + event.bytes.len();
            let w = if (self.first.0.len() + size) as f64 <= first_budget.min(1100.) { &mut self.first }
                else if (self.second.0.len() + size) as f64 <= second_budget.min(1100.) { &mut self.second } else { continue };
            if entry.rank > OWED && (refinements + size) as f64 > spare { continue; }
            if entry.rank > OWED { refinements += size; }
            w.u16(event.bytes.len() as f64)?; w.raw(&event.bytes);
            if matches!(event.meta.kind, Kind::Launch | Kind::Sample) {
                let until = if event.meta.kind == Kind::Launch { event.meta.expires }
                    else { tick + (event.view.life * 60.).ceil() + 18. + 24. };
                let id = event.meta.projectile.unwrap_or(f64::NAN);
                if let Some(known) = self.projectiles.get_mut(&id) { known.expires = js_max(known.expires, until); }
                else {
                    // History has no visible bytes; bounded eviction uses the
                    // same oldest-first rule as the owed queue. Count it so
                    // malformed/unbounded sources cannot hide saturation.
                    if self.projectiles.size() >= PROJECTILES {
                        if let Some((_, oldest, _)) = self.projectiles.next_after(0) { self.projectiles.delete(&oldest); self.history_evictions += 1; }
                    }
                    self.projectiles.set(id, Projectile { expires: until, confirmed: false });
                }
            }
            if confirmable && entry.rank <= OWED {
                if let Some(e) = self.entries.iter_mut().find(|e| e.event.meta.key == event.meta.key) { e.sent_tick = tick; }
                if let Some(receipt) = Receipt::of(&event) { owed.events.push(receipt); }
            } else { self.entries.retain(|e| e.event.meta.key != event.meta.key); }
        }
        // CosmeticOutbox stores receipts only for confirmable owed records.
        // Their unique identities identify replacements without retaining old payloads.
        // An empty batch returns its receipt bank immediately.
        if owed.events.is_empty() { self.spare.push(owed); } else { self.inflight.push_back(owed); }
        self.sort.clear(); self.projectiles.compact_in_place(); Ok(())
    }
    pub fn load(&self) -> (usize, usize, usize) { (self.entries.len(), self.inflight.iter().map(|s| s.events.len()).sum(), self.projectiles.size()) }
    pub fn bytes(&self) -> (&[u8], &[u8]) { (&self.first.0, &self.second.0) }
    /// Inspection is outside publication and may materialize a diagnostic list.
    pub fn inspect(&self) -> Vec<f64> {
        let mut out = vec![self.dropped, self.owed_dropped, self.resent, self.pruned, self.entries.len() as f64];
        for e in &self.entries {
            out.push(e.event.meta.key.len() as f64); out.extend(e.event.meta.key.iter().map(|&n| n as f64));
            out.extend([e.event.identity as f64, e.d2, e.rank as f64, e.sent_tick]);
        }
        out.push(self.inflight.len() as f64);
        for sent in &self.inflight { out.extend([sent.tick, sent.events.len() as f64]); out.extend(sent.events.iter().map(|e| e.identity as f64)); }
        out.push(self.projectiles.size() as f64);
        let mut cursor = 0; while let Some((at, id, p)) = self.projectiles.next_after(cursor) { cursor = at + 1; out.extend([id, p.expires, p.confirmed as u8 as f64]); }
        out
    }
}
