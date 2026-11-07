//! One snapshot-host publication: capture, eligibility, baseline choice,
//! cosmetic delivery and own packets run before the transport sees any spans.
use crate::{gen::*, store::block::Block};
use super::{codec::{self, Result, Writer}, host::Host, own_ring, delivery, Runtime};

// SnapshotHost admits at most this many protocol peers. Ordinary arenas
// cover the measured 32-client battle envelope; larger admissions and paused
// journals grow at a publication boundary. Packet bytes and UTF-16 edge
// statuses share spans, without one f64 word for each byte.
const CLIENTS: usize = 256;
const ARENA: usize = 128 * 1024;
const OUTPUT_WORDS: usize = 4096;

#[derive(Default, Clone, Copy)]
pub struct Actor {
    pub present: bool, pub spectator: bool, pub unit: Option<(f64, f64)>, pub position: Option<[f64; 3]>,
    pub ack: f64, pub ack_tick: f64, pub discarded_input: f64, pub discarded_actions: f64,
}
pub struct Correction<'a> { pub id: f64, pub generation: f64, pub schema: f64, pub payload: &'a [u8] }
/// The source owns canonical records. Implementations supply only retained
/// borrows; neither an actor facade nor a host callback is invoked here.
pub trait World {
    fn validate(&self) -> Result<()> { Ok(()) }
    fn capture(&mut self, tick: f64, epoch: f64, runtime: &mut Runtime) -> Result<()>;
    fn actor(&self, peer: u32) -> Actor;
    fn summaries(&mut self, peer: u32, epoch: f64, fields: &mut Fields) -> Result<()>;
    fn own(&mut self, peer: u32, epoch: f64) -> Result<Correction<'_>>;
    fn events(&mut self, events: &mut super::journal::Events);
    fn created(&self) -> &[WireDynamicFact] { &[] }
    fn retired(&self) -> &[u32] { &[] }
    fn status(&self, id: crate::store::StrId) -> &[u16];
}
/// Own summaries have two protocol fields. Their nested byte allocations
/// return to this client when the next staggered capture replaces them.
pub struct Fields { pub values: Vec<WireField>, spare: Vec<WireField> }
impl Default for Fields {
    // ownSummaryFields keeps only the spawn and personal-record schemas.
    // Separate free and live banks let a replacement reuse both byte vectors.
    // A failed copy returns its bank before the next publication retries.
    fn default() -> Self { Self { values: Vec::with_capacity(2), spare: (0..2).map(|_| WireField { payload: Vec::with_capacity(900), ..Default::default() }).collect() } }
}
impl Fields {
    pub fn clear(&mut self) { self.spare.extend(self.values.drain(..)); }
    pub fn push(&mut self, source: &WireField) -> Result<()> {
        let mut field = self.spare.pop().ok_or("own summary capacity exceeded")?;
        if source.payload.len() > field.payload.capacity() { self.spare.push(field); return Err("summary exceeds bounds"); }
        field.id = source.id; field.schema = source.schema; field.generation = source.generation;
        field.payload.clear(); field.payload.extend_from_slice(&source.payload); self.values.push(field); Ok(())
    }
}
pub struct Personal {
    // The transport client id and the source's qualified peer id have
    // different lifetimes. Never infer one from Vec position or roster slot.
    // Native ACK admission consults this ring before settling the outbox.
    // Queued event identities remain independent of own-body baselines.
    pub peer: u32, pub ring: own_ring::Ring, pub outbox: delivery::Outbox,
    pub fields: Fields, pub cached: bool,
    /// Cold binding to the same battle's reliable edge owner. It is released
    /// with this client before that owner can be disposed by the host.
    pub edges: *const super::edges::Host,
    correction: Writer, summaries: Writer, packet: Writer,
}
impl Personal {
    pub fn new(peer: u32, edges: *const super::edges::Host) -> Result<Self> {
        Ok(Self { peer, ring: own_ring::Ring::new(128)?, outbox: delivery::Outbox::new(300.), fields: Fields::default(), cached: false, edges,
            correction: Writer(Vec::with_capacity(2200)), summaries: Writer(Vec::with_capacity(2048)), packet: Writer(Vec::with_capacity(1100)) })
    }
}
#[derive(Clone, Copy)]
struct Recipient { at: usize, full: bool, actor: Actor }
struct Candidate { tick: f64, holders: [u64; 4] }
struct Group { value: WirePublishedGroup, joined: bool }
impl Default for Group {
    fn default() -> Self { Self { value: WirePublishedGroup { clients: Vec::with_capacity(CLIENTS), parts: Vec::with_capacity(256), ..Default::default() }, joined: false } }
}
/// All result bytes belong to this publisher until its next call. The host
/// copies spans into transport-owned buffers after the native borrow ends.
pub struct Publisher {
    pub output: WirePublicationOutput, pub block: Block, pub bytes: Vec<u8>,
    // Encoded groups are cached before a client's own preparation finishes.
    // A failed own builder can leave a cache entry, but only a later join
    // makes that group visible to transport. group_order records those joins.
    groups: [Group; 4], group_count: usize, group_order: Vec<usize>,
    recipients: Vec<Recipient>, candidates: Vec<Candidate>, chosen: Vec<f64>, costs: [f64; CLIENTS],
    edge_spare: Vec<WirePublishedEdges>, reliable_spare: Vec<WirePublishedReliable>,
    pub events: super::journal::Events, last_encodes: u32,
}
impl Default for Publisher {
    fn default() -> Self {
        let mut block = Block::default(); block.words.reserve(OUTPUT_WORDS + WirePublicationOutput::WORDS + 5); block.heap.reserve(OUTPUT_WORDS);
        Self { output: WirePublicationOutput { own: Vec::with_capacity(512), acks: Vec::with_capacity(CLIENTS), edges: Vec::with_capacity(CLIENTS),
                created: Vec::with_capacity(128), retired: Vec::with_capacity(4096), records: Vec::with_capacity(64), reliable: Vec::with_capacity(CLIENTS), faults: Vec::with_capacity(CLIENTS + 1), ..Default::default() },
            block, bytes: Vec::with_capacity(ARENA), groups: std::array::from_fn(|_| Group::default()), group_count: 0, group_order: Vec::with_capacity(4),
            recipients: Vec::with_capacity(CLIENTS), candidates: Vec::with_capacity(CLIENTS * 8), chosen: Vec::with_capacity(3), costs: [1024.; CLIENTS],
            edge_spare: (0..32).map(|_| WirePublishedEdges { received: Vec::with_capacity(32), results: Vec::with_capacity(32), ..Default::default() }).collect(),
            reliable_spare: (0..32).map(|_| WirePublishedReliable { records: Vec::with_capacity(64), ..Default::default() }).collect(), events: Default::default(), last_encodes: 0 }
    }
}
fn append(arena: &mut Vec<u8>, bytes: &[u8]) -> Result<WireSpan> {
    if bytes.len() > arena.capacity().saturating_sub(arena.len()) { return Err("publication byte capacity exceeded"); }
    let offset = arena.len() as u32; arena.extend_from_slice(bytes); Ok(WireSpan { offset, length: bytes.len() as u32 })
}
impl Publisher {
    /// Pool growth belongs to client admission, before a publication borrows
    /// anything. One event can be owed to several clients without byte copies.
    /// Read-only publication occupancy, outside the timed capture borrow.
    pub fn load(&self, host: &Host, runtime: &Runtime) -> WireLoad {
        let mut out = WireLoad::default();
        if let Some(s) = runtime.latest() {
            out.slots = s.slots.len() as u32; out.fields = s.world.len() as u32;
            out.field_bytes = s.world.iter().map(|f| f.payload.len() as u32).sum(); out.max_field_bytes = s.world.iter().map(|f| f.payload.len() as u32).max().unwrap_or(0);
            out.seats = s.slots.iter().map(|s| s.entry.mount.seats.len() as u32).sum();
        }
        for c in &host.clients { if let Some(p) = &c.publication {
            out.clients += 1;
            if let Some(own) = p.ring.latest() {
                let bytes = own.payload.as_ref().map_or(0, |b| b.len() as u32); out.own_bytes += bytes; out.max_own_bytes = out.max_own_bytes.max(bytes);
                out.own_fields += own.summaries.len() as u32; out.own_field_bytes += own.summaries.iter().map(|f| f.payload.len() as u32).sum::<u32>();
                out.max_own_field_bytes = out.max_own_field_bytes.max(own.summaries.iter().map(|f| f.payload.len() as u32).max().unwrap_or(0));
            }
            let (events, receipts, history) = p.outbox.load();
            out.queued_events += events as u32; out.max_queued_events = out.max_queued_events.max(events as u32);
            out.receipts += receipts as u32; out.max_receipts = out.max_receipts.max(receipts as u32);
            out.histories += history as u32; out.max_histories = out.max_histories.max(history as u32);
        } }
        let (size, busy, bytes, max) = self.events.load(); out.pool_size = size as u32; out.pool_busy = busy as u32;
        out.events = self.events.cosmetics().len() as u32; out.event_bytes = bytes as u32; out.max_event_bytes = max as u32; out.gameplay = self.events.gameplay().len() as u32;
        out.output_words = self.block.words.len() as u32; out.output_bytes = self.bytes.len() as u32;
        out.parts = self.groups[..self.group_count].iter().map(|g| g.value.parts.len() as u32).sum(); out
    }
    pub fn admit(&mut self, clients: usize) {
        self.events.reserve(clients);
        // Edge and reliable banks follow actual admissions. A 32-peer battle
        // does not need the nested storage for all 256 protocol connections.
        while self.edge_spare.len() + self.output.edges.len() < clients.min(CLIENTS) {
            self.edge_spare.push(WirePublishedEdges { received: Vec::with_capacity(32), results: Vec::with_capacity(32), ..Default::default() });
        }
        while self.reliable_spare.len() + self.output.reliable.len() < clients.min(CLIENTS) {
            self.reliable_spare.push(WirePublishedReliable { records: Vec::with_capacity(64), ..Default::default() });
        }
        let bytes = ARENA.max(clients.min(CLIENTS) * 2400 + 48 * 1024);
        self.bytes.reserve(bytes.saturating_sub(self.bytes.len()));
    }
    // Metadata rows with nested lists return to their original free banks.
    // Clearing the outer Vec alone would free those lists at the next push.
    // Snapshot and per-client baseline rings deliberately survive this reset.
    fn reset(&mut self, epoch: f64, tick: f64, unix: f64, dropped: f64) {
        self.output.epoch = epoch; self.output.tick = tick; self.output.captured_at_unix_ms = unix; self.output.dropped_ticks = dropped;
        self.output.created.clear(); self.output.drained = false; self.output.retired.clear(); self.output.own.clear(); self.output.acks.clear(); self.output.records.clear(); self.output.faults.clear();
        self.edge_spare.extend(self.output.edges.drain(..)); self.reliable_spare.extend(self.output.reliable.drain(..));
        self.bytes.clear(); self.recipients.clear(); self.candidates.clear(); self.chosen.clear(); self.group_count = 0; self.group_order.clear(); self.costs.fill(1024.);
        for group in &mut self.groups { group.value.clients.clear(); group.value.parts.clear(); group.value.bytes = 0.; group.joined = false; }
    }
    fn fault(&mut self, client: u32, error: &'static str) {
        // Literal error bytes need no string interning on their first fault.
        if let Ok(message) = append(&mut self.bytes, error.as_bytes()) { self.output.faults.push(WirePublishedFault { client, message }); }
    }
    // chooseSharedBaselines uses one holder bit per recipient. Own-only
    // recipients have no holder bits and cannot influence the shared choice.
    // Sort unique ticks newest first before comparing gain: a tied candidate
    // must keep the newer tick, including disjoint acknowledgement histories.
    // The empty baseline costs 1024, independently of payload byte length.
    fn baselines(&mut self, host: &mut Host, tick: f64) {
        for (index, recipient) in self.recipients.iter().enumerate().filter(|(_, r)| r.full) {
            let c = &mut host.clients[recipient.at];
            c.acked.retain(|&a| tick - a < 128. && host.ring.has(a as i64, Some(host.epoch as u32))
                && c.publication.as_ref().is_some_and(|p| p.ring.has(a, host.epoch)));
            for &t in &c.acked {
                let at = if let Some(at) = self.candidates.iter().position(|c| c.tick == t) { at }
                    else { self.candidates.push(Candidate { tick: t, holders: [0; 4] }); self.candidates.len() - 1 };
                self.candidates[at].holders[index / 64] |= 1 << (index % 64);
            }
        }
        self.candidates.sort_unstable_by(|a, b| b.tick.partial_cmp(&a.tick).unwrap_or(std::cmp::Ordering::Equal));
        if self.candidates.len() <= 3 { self.chosen.extend(self.candidates.iter().map(|c| c.tick)); return; }
        while self.chosen.len() < 3 {
            let mut best = None; let mut gain = 0.;
            for c in &self.candidates {
                let age = tick - c.tick; let mut candidate_gain = 0.;
                for i in 0..self.recipients.len() { if c.holders[i / 64] & (1 << (i % 64)) != 0 && self.costs[i] > age { candidate_gain += self.costs[i] - age; } }
                if candidate_gain > gain { gain = candidate_gain; best = Some(c); }
            }
            let Some(best) = best else { break }; if best.tick < 0. { break; }
            self.chosen.push(best.tick);
            for i in 0..self.recipients.len() { if best.holders[i / 64] & (1 << (i % 64)) != 0 { self.costs[i] = self.costs[i].min(tick - best.tick); } }
        }
        self.chosen.sort_unstable_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    }
    // partsFor encodes each selected baseline once. Copying its datagrams
    // into the publication arena lets the encoder reuse scratch for the next
    // baseline without changing bytes already assigned to another client.
    fn group(&mut self, runtime: &mut Runtime, tick: f64, epoch: f64, base: f64, interval: f64) -> Result<usize> {
        if let Some(at) = self.groups[..self.group_count].iter().position(|g| g.value.baseline == base) { return Ok(at); }
        let parts = runtime.encode(tick, epoch, base, interval)?;
        let at = self.group_count; let group = self.groups.get_mut(at).ok_or("publication baseline capacity exceeded")?;
        group.value.baseline = base;
        for part in parts { group.value.parts.push(append(&mut self.bytes, part)?); group.value.bytes += part.len() as f64; }
        self.group_count += 1; Ok(at)
    }
    /// SnapshotHost.publish preserves the order of ACKs, feedback, reliable
    /// events, recipient faults, group joins and first/second own datagrams.
    /// Edge pointers were bound at admission and must remain live for this call.
    pub unsafe fn run(&mut self, host: &mut Host, runtime: &mut Runtime, world: &mut impl World, tick: f64, at: f64, unix: f64, dropped: f64) {
        self.reset(host.epoch, tick, unix, dropped);
        self.output.ring_newest = host.ring.newest_tick() as f64; self.output.encodes = self.last_encodes;
        if let Err(error) = world.validate() { self.fault(0, error); self.serialize(); return; }
        let mut captured = false;
        if host.due(tick) {
            match world.capture(tick, host.epoch, runtime).and_then(|_| host.captured(tick)) {
                Ok(()) => captured = true, Err(error) => self.fault(0, error),
            }
            world.events(&mut self.events);
            if self.output.created.try_reserve(world.created().len()).is_err() || self.output.records.try_reserve(self.events.gameplay().len()).is_err() {
                self.fault(0, "publication record capacity exceeded"); self.serialize(); return;
            }
            self.output.created.extend_from_slice(world.created()); self.output.retired.extend_from_slice(world.retired()); self.output.drained = true;
            // A long pause can release a complete reliable journal. Grow its
            // arena before materializing any index, so every reliable reference
            // still names an emitted record. Failed growth rejects the snapshot.
            let journal_bytes: usize = self.events.gameplay().iter().map(|r| r.payload.len()).sum();
            if journal_bytes > 64 * 1024 && self.bytes.try_reserve(journal_bytes + ARENA).is_err() {
                self.fault(0, "publication byte capacity exceeded"); self.serialize(); return;
            }
            for record in self.events.gameplay() {
                let payload = match append(&mut self.bytes, &record.payload) {
                    Ok(span) => span,
                    Err(error) => { self.fault(0, error); self.serialize(); return; }
                };
                self.output.records.push(WirePublishedRecord { id: record.id, generation: record.generation, tick: record.tick, schema: record.schema, payload });
            }
        }
        // ACKs and edge outcomes go to every started client, even when the
        // snapshot is not due, the client is stale, or its budget is spent.
        // Cosmetic offers and gameplay feedback precede the delivery decision;
        // a skipped snapshot must not lose something still owed to that peer.
        for (index, c) in host.clients.iter_mut().take(CLIENTS).enumerate() {
            if !c.started { continue; } let Some(personal) = c.publication.as_mut() else { continue };
            let actor = world.actor(personal.peer);
            if actor.present { self.output.acks.push(WirePublishedAck { client: c.id, generation: actor.unit.map_or(0., |u| u.1),
                ack: actor.ack, ack_tick: actor.ack_tick, discarded_input: actor.discarded_input, discarded_actions: actor.discarded_actions }); }
            if !personal.edges.is_null() {
                let edges = &*personal.edges;
                if !edges.received.is_empty() || !edges.results.is_empty() {
                    let mut row = self.edge_spare.pop().unwrap(); row.client = c.id; row.epoch = edges.epoch; row.generation = edges.generation;
                    row.received.clear(); row.received.extend(edges.received.iter().copied().take(32)); row.results.clear();
                    for result in edges.results.iter().take(32) {
                        let units = world.status(result.status);
                        if units.len() * 2 > self.bytes.capacity().saturating_sub(self.bytes.len()) { break; }
                        let offset = self.bytes.len() as u32;
                        for unit in units { self.bytes.extend_from_slice(&unit.to_le_bytes()); }
                        row.results.push(WirePublishedOutcome { id: result.id, tick: result.tick, status: WireSpan { offset, length: (units.len() * 2) as u32 } });
                    }
                    self.output.edges.push(row);
                }
            }
            if !captured || !c.ready { continue; }
            if at - c.heard_at > host.stale_after { c.metrics[9] += 1.; continue; }
            if let Err(error) = personal.outbox.prepare(self.events.cosmetics().len()) { self.fault(c.id, error); continue; }
            personal.outbox.offer(self.events.cosmetics(), actor.position, actor.unit.map_or(super::cosmetics::Owner::default(), |u| super::cosmetics::Owner::number(u.0)));
            if let Err(error) = personal.outbox.prepare(0) { self.fault(c.id, error); continue; }
            let mut reliable = self.reliable_spare.pop().unwrap(); reliable.client = c.id; reliable.records.clear();
            if reliable.records.try_reserve(self.events.gameplay().len()).is_err() {
                self.reliable_spare.push(reliable); self.fault(c.id, "publication record capacity exceeded"); continue;
            }
            for (i, event) in self.events.gameplay().iter().enumerate() {
                // Death feedback concerns every page. Damage concerns only
                // the attacker's and target's own identities.
                if event.schema != 3. || actor.unit.is_some_and(|u| super::journal::concerns(event, u.0)) { reliable.records.push(i as u32); }
            }
            if reliable.records.is_empty() { self.reliable_spare.push(reliable); } else { self.output.reliable.push(reliable); }
            let full = c.budget_used(at) < host.budget;
            if !full { c.metrics[7] += 1.; if at - c.own_sent_at < 100. { continue; } }
            self.recipients.push(Recipient { at: index, full, actor });
        }
        self.baselines(host, tick);
        for i in 0..self.recipients.len() {
            let recipient = self.recipients[i]; let c = &mut host.clients[recipient.at];
            let base = {
                let p = c.publication.as_ref().unwrap();
                c.acked.retain(|&a| tick - a < 128. && host.ring.has(a as i64, Some(host.epoch as u32)) && p.ring.has(a, host.epoch));
                if recipient.full { self.chosen.iter().find(|t| c.acked.contains(t)).copied() } else { c.acked.first().copied() }.unwrap_or(0.)
            };
            // Keep this client's original exception boundary. In particular,
            // encoding shared parts comes before summaries and correction,
            // while joining the visible group follows successful event packing.
            // A later own-packet failure keeps charges for earlier sends.
            let result = (|| -> Result<()> {
                let group = if recipient.full { Some(self.group(runtime, tick, host.epoch, base, host.interval)?) } else { None };
                if recipient.full && base == 0. { c.metrics[6] += 1.; }
                let p = c.publication.as_mut().unwrap();
                if !p.cached || tick >= c.summaries_due {
                    if recipient.actor.spectator { p.fields.clear(); } else { world.summaries(p.peer, host.epoch, &mut p.fields)?; }
                    p.cached = true; c.summaries_due = tick + 6.;
                }
                let correction = if recipient.actor.spectator { None } else { Some(world.own(p.peer, host.epoch)?) };
                // prepareOwn stores exactly the new personal field set, while
                // a missing correction inherits its chosen baseline's body.
                // It commits before cosmetic packing, as the prior host did.
                let prepared = p.ring.write(tick, host.epoch, base, correction.as_ref().map(|c| c.payload), &p.fields.values)?;
                let (own, previous) = p.ring.prepared(&prepared);
                p.correction.0.clear();
                if let Some(correction) = correction { codec::own_correction_into(&mut p.correction, correction.id, correction.generation, correction.schema,
                    own.payload.as_deref().unwrap(), previous.and_then(|b| b.payload.as_deref()))?; }
                codec::world_section_into(&mut p.summaries, &own.summaries, previous.map_or(&[], |b| &b.summaries))?;
                let has_correction = !p.correction.0.is_empty();
                // ownDatagramBudget excludes the potential cosmetic length
                // prefix even when no event fits. Refinements spend only the
                // state budget left after shared parts and this first packet;
                // owed events retain their higher delivery priority.
                let cosmetic_budget = (1100. - 20. - 1. - p.correction.0.len() as f64 - if has_correction { 2. } else { 0. } - (2. + p.summaries.0.len() as f64) - 2.).max(0.);
                let shared_bytes = group.map_or(0., |at| self.groups[at].value.bytes);
                let spare = host.budget - c.sent_bytes - shared_bytes - (1100. - cosmetic_budget);
                p.outbox.pack(tick, cosmetic_budget, 1077., recipient.full, spare)?;
                if let Some(group_index) = group {
                    let g = &mut self.groups[group_index]; if !g.joined { g.joined = true; self.group_order.push(group_index); } g.value.clients.push(c.id);
                    c.metrics[1] += g.value.parts.len() as f64; c.metrics[2] += g.value.bytes;
                    super::host::charge(&mut c.sent, &mut c.sent_bytes, at, g.value.bytes);
                } else { c.metrics[8] += 1.; }
                let (first, second) = p.outbox.bytes();
                let parts = codec::OwnParts { epoch: host.epoch, tick, interval: host.interval, baseline: base, ack: recipient.actor.ack,
                    second: false, count: if second.is_empty() { 1. } else { 2. }, correction: has_correction.then_some(&p.correction.0),
                    summaries: Some(&p.summaries.0), cosmetics: Some(first) };
                codec::own_datagram_into(&mut p.packet, parts)?;
                let span = append(&mut self.bytes, &p.packet.0)?; self.output.own.push(WirePublishedOwn { client: c.id, span });
                c.metrics[1] += 1.; c.metrics[2] += p.packet.0.len() as f64; c.own_sent_at = at;
                super::host::charge(&mut c.sent, &mut c.sent_bytes, at, p.packet.0.len() as f64);
                if !second.is_empty() {
                    codec::own_datagram_into(&mut p.packet, codec::OwnParts { epoch: host.epoch, tick, interval: host.interval, baseline: base, ack: recipient.actor.ack,
                        second: true, count: 2., correction: None, summaries: None, cosmetics: Some(second) })?;
                    let span = append(&mut self.bytes, &p.packet.0)?; self.output.own.push(WirePublishedOwn { client: c.id, span });
                    c.metrics[1] += 1.; c.metrics[2] += p.packet.0.len() as f64;
                    super::host::charge(&mut c.sent, &mut c.sent_bytes, at, p.packet.0.len() as f64);
                }
                c.metrics[3] = p.outbox.dropped; c.metrics[4] = p.outbox.owed_dropped; c.metrics[5] = p.outbox.resent;
                if recipient.full { c.metrics[0] += 1.; }
                Ok(())
            })();
            if let Err(error) = result { self.fault(c.id, error); }
        }
        self.output.ring_newest = host.ring.newest_tick() as f64; if captured { self.last_encodes = self.group_count as u32; } self.output.encodes = self.last_encodes;
        self.output.bytes = self.bytes.as_ptr() as usize as u32; self.output.byte_length = self.bytes.len() as u32;
        self.serialize();
    }
    // The generated layout writes descriptors, not one f64 per packet byte.
    // Reserve both halves of Block up front because finish appends the heap
    // after the fixed row. JS copies every span before the next native call.
    fn serialize(&mut self) {
        self.output.bytes = self.bytes.as_ptr() as usize as u32; self.output.byte_length = self.bytes.len() as u32;
        let words = self.output.own.len() * WirePublishedOwn::WORDS + self.output.acks.len() * WirePublishedAck::WORDS +
            self.output.edges.iter().map(|e| WirePublishedEdges::WORDS + e.received.len() + e.results.len() * WirePublishedOutcome::WORDS).sum::<usize>() +
            self.output.created.len() * WireDynamicFact::WORDS + self.output.retired.len() + self.output.records.len() * WirePublishedRecord::WORDS +
            self.output.reliable.iter().map(|r| WirePublishedReliable::WORDS + r.records.len()).sum::<usize>() +
            self.output.faults.len() * WirePublishedFault::WORDS + self.group_order.iter().map(|&at| { let g = &self.groups[at].value;
                WirePublishedGroup::WORDS + g.clients.len() + g.parts.len() * WireSpan::WORDS }).sum::<usize>();
        let fixed = WirePublicationOutput::WORDS + crate::store::block::HEADER;
        if self.block.heap.try_reserve(words.saturating_sub(self.block.heap.len())).is_err() ||
            self.block.words.try_reserve((fixed + words).saturating_sub(self.block.words.len())).is_err() {
            self.output.shared.clear(); self.output.own.clear(); self.output.acks.clear(); self.output.edges.clear(); self.output.created.clear();
            self.output.retired.clear(); self.output.records.clear(); self.output.reliable.clear(); self.output.faults.clear(); self.group_order.clear();
            self.fault(0, "publication result capacity exceeded");
        }
        self.block.begin(1, WirePublicationOutput::WORDS); let (row, heap) = self.block.row(0);
        self.output.put(row, heap);
        // Cache entries whose client's own preparation failed are not groups
        // the transport saw. Join order determines the emitted group order.
        let at = heap.len(); row[WirePublicationOutput::SHARED] = at as f64; row[WirePublicationOutput::SHARED + 1] = self.group_order.len() as f64;
        heap.resize(at + self.group_order.len() * WirePublishedGroup::WORDS, 0.);
        for (i, &group) in self.group_order.iter().enumerate() {
            let mut words = [0.; WirePublishedGroup::WORDS]; self.groups[group].value.put(&mut words, heap);
            let start = at + i * WirePublishedGroup::WORDS; heap[start..start + WirePublishedGroup::WORDS].copy_from_slice(&words);
        }
        self.block.finish();
    }
}
